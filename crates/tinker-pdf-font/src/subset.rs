//! TrueType subsetting: the same font with the glyphs nobody drew removed.
//!
//! A document that embeds a face whole carries every glyph it has, which for
//! a CJK font is tens of megabytes to print a page of Latin text. Subsetting
//! is what makes embedding practical, and embedding is what makes a file
//! render the same everywhere.
//!
//! **Glyph identifiers do not change.** Every glyph the subset drops becomes
//! a zero-length entry in `loca` rather than a gap that later glyphs shuffle
//! into. That is a deliberate trade: a renumbering subset is smaller by the
//! width of the `loca` table and requires rewriting `cmap`, `hmtx`, every
//! composite glyph's component indices, and — outside this crate — the PDF
//! font dictionary's `/Widths`, `/W`, `/CIDToGIDMap` and any `/ToUnicode`.
//! Each of those is a separate chance to produce a file that renders *almost*
//! right, which is the failure mode hardest to notice and worst to ship. The
//! saving is a few kilobytes; `glyf` is where the megabytes are, and this
//! removes them either way.
//!
//! CFF is a different job and lives in [`crate::cff_subset`] — its
//! charstrings, its two subroutine INDEXes and the DICT offsets pointing at
//! all three have to be rebuilt together. [`subset`] is still the one door
//! for an sfnt or a CFF: it looks at what the bytes are and hands an
//! `OpenType/CFF` face or a bare CFF program to that. A Type 1 program has a
//! door of its own, [`crate::type1_subset::subset_type1`], because its
//! answer carries the three segment lengths a `/FontFile` stream states and
//! the names of the glyphs it keeps. `post` glyph names are dropped
//! wholesale rather than subsetted.
//!
//! [`extend_subset`] runs the other way (`docs/design/subsetting.md`,
//! *Extension*): handed a subset and the face a test proves it was cut from,
//! it cuts the face again to the subset's glyphs and the ones asked for, so a
//! subset this crate wrote — which never renumbers — gains glyphs at their own
//! identifiers.

use std::collections::{BTreeMap, BTreeSet};

use crate::cff::Cff;
use crate::sfnt::Sfnt;

/// How deep a composite may nest while the closure follows it.
const MAX_DEPTH: u32 = 8;
/// How many components the closure will follow in total, so a font whose
/// glyphs refer to each other in a cycle cannot spin.
const MAX_COMPONENTS: u32 = 4096;

/// Tables kept in the subset, by tag.
///
/// `glyf` and `loca` are rebuilt; the rest are copied through. `cvt `, `fpgm`
/// and `prep` are hinting tables this engine does not interpret, but another
/// reader of the same file will, and dropping them would make the embedded
/// font render worse elsewhere than it does here.
const KEEP: &[u32] = &[
    0x6376_7420, // cvt
    0x6670_676D, // fpgm
    0x676C_7966, // glyf
    0x6865_6164, // head
    0x6868_6561, // hhea
    0x686D_7478, // hmtx
    0x6C6F_6361, // loca
    0x6D61_7870, // maxp
    0x7072_6570, // prep
    0x636D_6170, // cmap
    0x4F53_2F32, // OS/2
];

/// Tables kept in an `OpenType/CFF` subset, by tag.
///
/// `CFF ` is rebuilt; the rest are copied through. The hinting tables of the
/// list above are absent from a CFF-flavoured face — its hints live in the
/// Private DICTs, which [`crate::cff_subset`] copies through — and `VORG` is
/// here because it is where such a face states vertical origins, which
/// nothing else in the file repeats.
const KEEP_CFF: &[u32] = &[
    0x4346_4620, // CFF
    0x636D_6170, // cmap
    0x6865_6164, // head
    0x6868_6561, // hhea
    0x686D_7478, // hmtx
    0x6D61_7870, // maxp
    0x4F53_2F32, // OS/2
    0x564F_5247, // VORG
];

/// The tags that decide which subsetter a face needs.
const TAG_GLYF: u32 = 0x676C_7966;
const TAG_LOCA: u32 = 0x6C6F_6361;
const TAG_CFF: u32 = 0x4346_4620;
const TAG_HEAD: u32 = 0x6865_6164;

fn be16(data: &[u8], at: usize) -> Option<u16> {
    let b = data.get(at..at + 2)?;
    Some(u16::from_be_bytes([b[0], b[1]]))
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    let b = data.get(at..at + 4)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// Where glyph `glyph` lives in `glyf`, from `loca`.
fn glyph_range(sfnt: &Sfnt, glyph: u16) -> Option<(usize, usize)> {
    let head = sfnt.table(0x6865_6164)?;
    let long_format = be16(head, 50)? as i16 != 0;
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
    (end >= start).then_some((start, end))
}

/// The glyphs a composite glyph is built from.
///
/// Public because the closure below is the only thing that needs it, and it
/// is the one piece of composite parsing that is about structure rather than
/// about drawing.
#[must_use]
pub fn components(sfnt: &Sfnt, glyph: u16) -> Vec<u16> {
    let mut out = Vec::new();
    let Some(glyf) = sfnt.table(0x676C_7966) else {
        return out;
    };
    let Some((start, end)) = glyph_range(sfnt, glyph) else {
        return out;
    };
    let Some(data) = glyf.get(start..end) else {
        return out;
    };
    // A non-negative contour count is a simple glyph, which has none.
    if be16(data, 0).map(|v| v as i16).unwrap_or(0) >= 0 {
        return out;
    }

    let mut at = 10usize;
    let mut guard = 0u32;
    loop {
        guard += 1;
        if guard > 64 {
            return out;
        }
        let (Some(flags), Some(index)) = (be16(data, at), be16(data, at + 2)) else {
            return out;
        };
        at += 4;
        out.push(index);

        // The argument and transform sizes decide where the next record
        // begins; getting this wrong walks off into the middle of a glyph
        // and invents components that are not there.
        at += if flags & 0x0001 != 0 { 4 } else { 2 };
        if flags & 0x0008 != 0 {
            at += 2;
        } else if flags & 0x0040 != 0 {
            at += 4;
        } else if flags & 0x0080 != 0 {
            at += 8;
        }

        if flags & 0x0020 == 0 {
            return out; // no MORE_COMPONENTS
        }
    }
}

/// Adds every glyph the requested ones depend on.
///
/// A composite glyph — an accented letter, most often — is a reference to
/// other glyphs. Keeping it without them draws nothing, which is how a
/// subsetter turns "é" into blank space and nobody notices until the file
/// reaches someone who reads a language that uses it.
fn close_over_components(sfnt: &Sfnt, glyphs: &mut BTreeSet<u16>) {
    let mut pending: Vec<u16> = glyphs.iter().copied().collect();
    let mut budget = MAX_COMPONENTS;
    let mut depth = 0u32;

    while !pending.is_empty() && depth < MAX_DEPTH {
        let mut next = Vec::new();
        for glyph in pending {
            if budget == 0 {
                break;
            }
            budget -= 1;
            for component in components(sfnt, glyph) {
                if glyphs.insert(component) {
                    next.push(component);
                }
            }
        }
        pending = next;
        depth += 1;
    }
}

/// Builds a font containing only `glyphs` and what they depend on.
///
/// Three shapes of program arrive here and each takes a different route: a
/// TrueType (or an `OpenType` face carrying `glyf`) is rebuilt below, an
/// `OpenType/CFF` face has its `CFF ` table rebuilt by
/// [`crate::cff_subset::subset_cff`] and the sfnt reassembled around it, and a
/// bare CFF program — which is what `/FontFile3` carries — goes straight
/// there.
///
/// Returns `None` when the program is not one this can rebuild, so a caller
/// can embed the original rather than a broken subset. Returning something is
/// always possible; returning something *smaller and correct* is not, and a
/// subset that renders wrong is worse than a font that is merely large.
#[must_use]
pub fn subset(program: &[u8], glyphs: &BTreeSet<u16>) -> Option<Vec<u8>> {
    subset_with(program, glyphs, &Reshape::default())
}

/// What [`subset_with`] changes beside the cut.
///
/// Empty — the default — changes nothing, and [`subset_with`] is then
/// [`subset`], byte for byte.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reshape {
    /// Advances to set, glyph by glyph, in the outlines' own units: font
    /// units for a `glyf` face, rounded to the integer `hmtx` holds;
    /// charstring units for a bare CFF, before its `FontMatrix`. A glyph the
    /// cut does not keep — one the face lacks among them — has no advance to
    /// set, and its entry is passed over.
    pub advances: BTreeMap<u16, f64>,
    /// A `cmap` table to write in place of the face's own — [`symbol_cmap`]'s,
    /// say. A `glyf` face only: a CFF has no `cmap` to replace.
    pub cmap: Option<Vec<u8>>,
}

impl Reshape {
    fn is_empty(&self) -> bool {
        self.advances.is_empty() && self.cmap.is_none()
    }
}

/// A `cmap` table of exactly one subtable, platform 3 encoding 0 — the
/// symbol encoding — in format 4, mapping `0xF000` plus each code to its
/// glyph.
///
/// The form a symbolic TrueType program is read by code: a reader prefixes
/// each byte of a string with the high byte of the subtable's range (ISO
/// 32000-2 9.6.5.4), and one subtable is all ISO 19005-1 6.3.7 admits on a
/// symbolic font. A segment a code, and the `0xFFFF` segment the format ends
/// with. A code mapped to glyph 0 is no mapping, and is left out.
#[must_use]
pub fn symbol_cmap(codes: &BTreeMap<u8, u16>) -> Vec<u8> {
    let pairs: Vec<(u16, u16)> = codes
        .iter()
        .filter(|(_, glyph)| **glyph != 0)
        .map(|(code, glyph)| (0xF000 | u16::from(*code), *glyph))
        .collect();
    let segments = pairs.len() + 1;
    let mut selector = 0u16;
    while (1usize << (selector + 1)) <= segments {
        selector += 1;
    }
    let search = 2 * (1u16 << selector);
    let doubled = (segments * 2) as u16;

    let mut sub = Vec::with_capacity(16 + segments * 8);
    // format, length (filled in below), language, segCountX2, searchRange,
    // entrySelector, rangeShift.
    for value in [4u16, 0, 0, doubled, search, selector, doubled - search] {
        sub.extend_from_slice(&value.to_be_bytes());
    }
    for (point, _) in &pairs {
        sub.extend_from_slice(&point.to_be_bytes());
    }
    sub.extend_from_slice(&0xFFFFu16.to_be_bytes());
    sub.extend_from_slice(&0u16.to_be_bytes()); // reservedPad
    for (point, _) in &pairs {
        sub.extend_from_slice(&point.to_be_bytes());
    }
    sub.extend_from_slice(&0xFFFFu16.to_be_bytes());
    for (point, glyph) in &pairs {
        sub.extend_from_slice(&glyph.wrapping_sub(*point).to_be_bytes());
    }
    sub.extend_from_slice(&1u16.to_be_bytes());
    // Every idRangeOffset zero: each glyph is its code plus its delta.
    sub.extend(core::iter::repeat_n(0u8, segments * 2));
    let length = sub.len() as u16;
    sub[2..4].copy_from_slice(&length.to_be_bytes());

    let mut table = Vec::with_capacity(12 + sub.len());
    // version, one encoding record: platform 3, encoding 0, at offset 12.
    for value in [0u16, 1, 3, 0] {
        table.extend_from_slice(&value.to_be_bytes());
    }
    table.extend_from_slice(&12u32.to_be_bytes());
    table.extend(sub);
    table
}

/// [`subset`], with the cut's advances set as `reshape` says.
///
/// A `glyf` face's `hmtx` takes each advance — widened to an entry for every
/// glyph where a glyph past `numberOfHMetrics` changes, or the last full
/// entry they repeat, so every other glyph keeps its own — and `hhea`'s
/// `numberOfHMetrics` and `advanceWidthMax` follow. A bare CFF's charstrings take each as their width operand
/// ([`crate::cff_subset::subset_cff_with`]). A `glyf` face's `cmap` is the
/// one `reshape` hands over, where it hands one. An `OpenType/CFF` face, whose
/// advances are two tables' to keep in step, is declined with anything to
/// set, as is an advance no `hmtx` entry can hold and a `cmap` for a bare
/// CFF.
#[must_use]
pub fn subset_with(program: &[u8], glyphs: &BTreeSet<u16>, reshape: &Reshape) -> Option<Vec<u8>> {
    let Some(sfnt) = Sfnt::parse(program) else {
        // Not an sfnt at all: a bare CFF program, or nothing this can read.
        // A CFF has no `cmap` to replace.
        if reshape.cmap.is_some() {
            return None;
        }
        return crate::cff_subset::subset_cff_with(program, glyphs, &reshape.advances);
    };
    if sfnt.table(TAG_GLYF).is_none() && sfnt.table(TAG_CFF).is_some() {
        if !reshape.is_empty() {
            return None;
        }
        return subset_opentype_cff(&sfnt, glyphs);
    }
    subset_truetype(&sfnt, glyphs, reshape)
}

/// `hhea` and `hmtx` with `advances` set (font units, rounded), every other
/// glyph's advance and every left side bearing as they were.
///
/// `None` where the face's metrics do not read — no `hhea`, no `hmtx`, a
/// `numberOfHMetrics` of zero or past the glyph count, an `hmtx` shorter than
/// both say — or an advance is no `u16`.
fn reshaped_metrics(
    sfnt: &Sfnt<'_>,
    glyph_count: u16,
    advances: &BTreeMap<u16, f64>,
) -> Option<(Vec<u8>, Vec<u8>)> {
    let hhea = sfnt.table(0x6868_6561)?;
    let hmtx = sfnt.table(0x686D_7478)?;
    let long = be16(hhea, 34)?;
    if long == 0 || long > glyph_count {
        return None;
    }

    // Every glyph's advance and side bearing, as the table states them: the
    // glyphs past `numberOfHMetrics` repeat the last advance.
    let mut metrics: Vec<(u16, u16)> = Vec::with_capacity(usize::from(glyph_count));
    for glyph in 0..glyph_count {
        let entry = if glyph < long {
            let at = usize::from(glyph) * 4;
            (be16(hmtx, at)?, be16(hmtx, at + 2)?)
        } else {
            let last = metrics.get(usize::from(long) - 1)?.0;
            let at = usize::from(long) * 4 + usize::from(glyph - long) * 2;
            (last, be16(hmtx, at)?)
        };
        metrics.push(entry);
    }

    let mut widened = false;
    for (&glyph, &advance) in advances {
        let units = advance.round();
        if !advance.is_finite() || !(0.0..=65_535.0).contains(&units) {
            return None;
        }
        let slot = metrics.get_mut(usize::from(glyph))?;
        slot.0 = units as u16;
        widened |= glyph + 1 >= long;
    }
    // A glyph past `numberOfHMetrics` takes the last long entry's advance, so
    // changing one there, or that last entry itself, gives every glyph an
    // entry of its own; otherwise the table keeps its shape.
    let long = if widened { glyph_count } else { long };

    let mut new_hmtx =
        Vec::with_capacity(usize::from(long) * 4 + usize::from(glyph_count - long) * 2);
    for (glyph, (advance, bearing)) in metrics.iter().enumerate() {
        if glyph < usize::from(long) {
            new_hmtx.extend_from_slice(&advance.to_be_bytes());
        }
        new_hmtx.extend_from_slice(&bearing.to_be_bytes());
    }
    let mut new_hhea = hhea.to_vec();
    let widest = metrics
        .iter()
        .map(|(advance, _)| *advance)
        .max()
        .unwrap_or(0);
    if new_hhea.len() < 36 {
        return None;
    }
    new_hhea[10..12].copy_from_slice(&widest.to_be_bytes());
    new_hhea[34..36].copy_from_slice(&long.to_be_bytes());
    Some((new_hhea, new_hmtx))
}

/// The `OpenType/CFF` route: the `CFF ` table subsetted, the sfnt rebuilt.
fn subset_opentype_cff(sfnt: &Sfnt<'_>, glyphs: &BTreeSet<u16>) -> Option<Vec<u8>> {
    let cff = crate::cff_subset::subset_cff(sfnt.table(TAG_CFF)?, glyphs)?;

    let mut tables: Vec<(u32, Vec<u8>)> = Vec::new();
    for &tag in KEEP_CFF {
        let data = if tag == TAG_CFF {
            cff.clone()
        } else {
            match sfnt.table(tag) {
                Some(data) => {
                    let mut data = data.to_vec();
                    // `head.checkSumAdjustment` is computed over the whole
                    // file with the field itself zero, so it is zeroed here
                    // and filled in by `assemble`.
                    if tag == TAG_HEAD && data.len() >= 12 {
                        data[8..12].copy_from_slice(&0u32.to_be_bytes());
                    }
                    data
                }
                None => continue,
            }
        };
        tables.push((tag, data));
    }
    tables.sort_by_key(|(tag, _)| *tag);
    Some(assemble(sfnt.version(), &tables))
}

/// The `glyf`/`loca` route.
fn subset_truetype(sfnt: &Sfnt<'_>, glyphs: &BTreeSet<u16>, reshape: &Reshape) -> Option<Vec<u8>> {
    let glyf = sfnt.table(TAG_GLYF)?;
    sfnt.table(TAG_LOCA)?;
    let head = sfnt.table(TAG_HEAD)?;
    let maxp = sfnt.table(0x6D61_7870)?;
    let glyph_count = be16(maxp, 4)?;

    // .notdef is what a reader draws for anything the font does not cover,
    // so it is never optional.
    let mut keep: BTreeSet<u16> = glyphs
        .iter()
        .copied()
        .filter(|&g| g < glyph_count)
        .collect();
    keep.insert(0);
    close_over_components(sfnt, &mut keep);

    // The advances asked of the glyphs the cut keeps; a dropped glyph has
    // none to set.
    let metrics = if reshape.advances.is_empty() {
        None
    } else {
        let kept: BTreeMap<u16, f64> = reshape
            .advances
            .iter()
            .filter(|(glyph, _)| keep.contains(glyph))
            .map(|(glyph, advance)| (*glyph, *advance))
            .collect();
        Some(reshaped_metrics(sfnt, glyph_count, &kept)?)
    };

    // Rebuild `glyf` in glyph order, with the dropped ones contributing
    // nothing. Offsets are recorded for every glyph, including the dropped
    // ones, which is what keeps the identifiers stable.
    let mut new_glyf: Vec<u8> = Vec::new();
    let mut offsets: Vec<u32> = Vec::with_capacity(usize::from(glyph_count) + 1);

    for glyph in 0..glyph_count {
        offsets.push(new_glyf.len() as u32);
        if !keep.contains(&glyph) {
            continue;
        }
        let Some((start, end)) = glyph_range(sfnt, glyph) else {
            continue;
        };
        let Some(data) = glyf.get(start..end) else {
            continue;
        };
        new_glyf.extend_from_slice(data);
        // Each glyph must begin on a two-byte boundary for the short `loca`
        // form; padding here costs a byte and keeps both forms available.
        while new_glyf.len() % 4 != 0 {
            new_glyf.push(0);
        }
    }
    offsets.push(new_glyf.len() as u32);

    // Always the long form. The short form halves its offsets, so it cannot
    // describe a `glyf` past 128 KiB, and choosing per font would mean two
    // paths where one will do.
    let mut new_loca: Vec<u8> = Vec::with_capacity(offsets.len() * 4);
    for offset in &offsets {
        new_loca.extend_from_slice(&offset.to_be_bytes());
    }

    // `head.indexToLocFormat` has to agree with the form just written, or the
    // reader halves offsets that were never doubled and every glyph after the
    // first lands in the middle of another.
    let mut new_head = head.to_vec();
    if new_head.len() >= 52 {
        new_head[50..52].copy_from_slice(&1u16.to_be_bytes());
        // Zeroed before the file checksum is computed, then filled in.
        new_head[8..12].copy_from_slice(&0u32.to_be_bytes());
    }

    let mut tables: Vec<(u32, Vec<u8>)> = Vec::new();
    for &tag in KEEP {
        let data = match (tag, &metrics) {
            (0x676C_7966, _) => new_glyf.clone(),
            (0x6C6F_6361, _) => new_loca.clone(),
            (0x6865_6164, _) => new_head.clone(),
            (0x6868_6561, Some((hhea, _))) => hhea.clone(),
            (0x686D_7478, Some((_, hmtx))) => hmtx.clone(),
            (0x636D_6170, _) if reshape.cmap.is_some() => reshape.cmap.clone().unwrap_or_default(),
            _ => match sfnt.table(tag) {
                Some(data) => data.to_vec(),
                None => continue,
            },
        };
        tables.push((tag, data));
    }
    tables.sort_by_key(|(tag, _)| *tag);

    Some(assemble(sfnt.version(), &tables))
}

/// Writes the table directory and the tables, with the checksums a validator
/// will look at.
///
/// `version` is the `sfntVersion` the output declares, and it is the *face's*
/// rather than the file's. The two differ for a TrueType Collection, and
/// taking it from byte zero of the original was a defect the corpus census
/// found: a `/FontFile2` carrying a `ttcf` came out of here as a single-font
/// directory that still said `ttcf`, so [`Sfnt::parse`] read the first table
/// record's tag as a font offset and refused the program this crate had just
/// written. Two documents in the fetched corpora embed one, and the text they
/// draw through it would have rendered as nothing at all. [`Sfnt::version`]
/// is the directory's own, which is what every table here came from.
pub(crate) fn assemble(version: u32, tables: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let count = tables.len() as u16;
    let mut out = Vec::new();

    out.extend_from_slice(&version.to_be_bytes());
    out.extend_from_slice(&count.to_be_bytes());

    // The binary-search hints. No reader needs them to be right, several
    // validators check them, and they are three lines.
    let mut entry_selector = 0u16;
    while (1u32 << (entry_selector + 1)) <= u32::from(count) {
        entry_selector += 1;
    }
    let search_range = (1u16 << entry_selector) * 16;
    out.extend_from_slice(&search_range.to_be_bytes());
    out.extend_from_slice(&entry_selector.to_be_bytes());
    out.extend_from_slice(&(count * 16 - search_range).to_be_bytes());

    // Offsets are only known once the directory's own size is, which is why
    // the directory is written twice: once as a placeholder, once for real.
    let directory_at = out.len();
    out.resize(directory_at + tables.len() * 16, 0);

    let mut head_at = None;
    let mut records = Vec::with_capacity(tables.len());
    for (tag, data) in tables {
        while out.len() % 4 != 0 {
            out.push(0);
        }
        let offset = out.len() as u32;
        if *tag == 0x6865_6164 {
            head_at = Some(offset as usize);
        }
        out.extend_from_slice(data);
        records.push((*tag, checksum(data), offset, data.len() as u32));
    }

    for (index, (tag, sum, offset, length)) in records.iter().enumerate() {
        let at = directory_at + index * 16;
        out[at..at + 4].copy_from_slice(&tag.to_be_bytes());
        out[at + 4..at + 8].copy_from_slice(&sum.to_be_bytes());
        out[at + 8..at + 12].copy_from_slice(&offset.to_be_bytes());
        out[at + 12..at + 16].copy_from_slice(&length.to_be_bytes());
    }

    // head.checkSumAdjustment: the constant minus the whole file's checksum,
    // computed with the field itself zero — which it is, having been zeroed
    // when `head` was copied.
    if let Some(head_at) = head_at {
        if head_at + 12 <= out.len() {
            let adjustment = 0xB1B0_AFBAu32.wrapping_sub(checksum(&out));
            out[head_at + 8..head_at + 12].copy_from_slice(&adjustment.to_be_bytes());
        }
    }

    out
}

/// The sum of a table's big-endian 32-bit words, the last one zero-padded.
fn checksum(data: &[u8]) -> u32 {
    let mut sum = 0u32;
    let mut at = 0usize;
    while at < data.len() {
        let mut word = [0u8; 4];
        let take = 4.min(data.len() - at);
        word[..take].copy_from_slice(&data[at..at + take]);
        sum = sum.wrapping_add(u32::from_be_bytes(word));
        at += 4;
    }
    sum
}

/// The glyphs a string needs, looked up through the font's own `cmap`.
///
/// A character the font does not cover resolves to `.notdef`, which the
/// subset keeps regardless, so it contributes nothing and is left out here.
/// The distinction matters to a caller measuring coverage: an empty result
/// for a whole string says this font cannot set that text at all.
#[must_use]
pub fn glyphs_for(program: &[u8], text: &str) -> BTreeSet<u16> {
    let mut out = BTreeSet::new();
    if let Some(sfnt) = Sfnt::parse(program) {
        for c in text.chars() {
            match sfnt.glyph_for_char(c) {
                Some(0) | None => {}
                Some(glyph) => {
                    out.insert(glyph);
                }
            }
        }
        return out;
    }
    // A bare CFF has no `cmap`: a character reaches a glyph through the
    // charset, by the name the character has. Falling back to the program's
    // own encoding covers the face whose glyphs are named in some scheme this
    // engine does not read, which would otherwise resolve nothing at all.
    let Some(cff) = Cff::parse(program) else {
        return out;
    };
    for c in text.chars() {
        let glyph = crate::encoding::glyph_name_for_char(c)
            .and_then(|name| cff.gid_for_name(&name))
            .or_else(|| {
                u8::try_from(u32::from(c))
                    .ok()
                    .and_then(|code| cff.gid_for_code(code))
            });
        if let Some(glyph) = glyph.filter(|g| *g != 0) {
            out.insert(glyph);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Extending a subset (FT-11.9).
// ---------------------------------------------------------------------------

/// A subset with glyphs of its source face added (`docs/design/subsetting.md`,
/// *Extension*): the face cut to the subset's glyphs and the new ones.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Extended {
    /// The program.
    pub program: Vec<u8>,
    /// A Type 1 program's three lengths (ISO 32000-2 Table 125); `None` for
    /// every other format.
    pub lengths: Option<[usize; 3]>,
    /// A Type 1 program's glyph names, in program order, for `/CharSet`;
    /// `None` for every other format.
    pub names: Option<Vec<Vec<u8>>>,
}

/// Why [`extend_subset`] did not extend a subset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ExtendRefusal {
    /// The subset and the face are not one format this extends — TrueType,
    /// CFF bare or in an `OpenType` face, or Type 1 — or not the same one.
    Format,
    /// The face is not the subset's source: a table, the glyph count, a
    /// name, an encoding, a matrix or a glyph differs where the subset keeps
    /// the face's.
    NotTheSource,
    /// A glyph asked for is past the face's last.
    NoSuchGlyph,
    /// The face cut to the subset's glyphs and the new ones does not rebuild.
    NotRebuildable,
    /// Comparing the two programs' glyphs spent the work it may.
    Budget,
}

impl core::fmt::Display for ExtendRefusal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            ExtendRefusal::Format => "the subset and the face are not one format this extends",
            ExtendRefusal::NotTheSource => "the face is not the subset's source",
            ExtendRefusal::NoSuchGlyph => "a glyph asked for is past the face's last",
            ExtendRefusal::NotRebuildable => "the face cut to the glyphs does not rebuild",
            ExtendRefusal::Budget => "comparing the programs' glyphs spent its work",
        })
    }
}

/// The outline steps one extension may spend comparing a subset's glyphs
/// with its face's (ruling 1): each glyph's run is bounded, a face of many
/// glyphs is not.
const EXTEND_BUDGET: u64 = 40_000_000;

/// The tables of a `glyf` face this crate's subsetter copies through, which a
/// subset of it carries byte for byte.
const COPIED: &[u32] = &[
    0x636D_6170, // cmap
    0x686D_7478, // hmtx
    0x6868_6561, // hhea
    0x4F53_2F32, // OS/2
    0x6376_7420, // cvt
    0x6670_676D, // fpgm
    0x7072_6570, // prep
];

/// The tables of an `OpenType/CFF` face the subsetter copies through.
const COPIED_CFF: &[u32] = &[
    0x636D_6170, // cmap
    0x6868_6561, // hhea
    0x686D_7478, // hmtx
    0x6D61_7870, // maxp
    0x4F53_2F32, // OS/2
    0x564F_5247, // VORG
];

/// `subset` with `glyphs` added: `face`, which the source test proves the
/// subset was cut from, cut again to the glyphs the subset keeps and to
/// `glyphs` — with every component a composite among them names, and
/// `.notdef` — at their own identifiers (`docs/design/subsetting.md`,
/// *Extension*). Every glyph the subset keeps draws as it did, by the test,
/// and nothing else is added.
///
/// `glyphs` are glyph identifiers of the face: glyph indices for TrueType and
/// CFF, and for Type 1 the charstrings' positions in the order
/// [`crate::Type1::parse`] reads the face's.
///
/// The source test, which a subset that renumbered its glyphs fails:
///
/// - **TrueType**: the same glyph count; `cmap`, `hmtx`, `hhea`, `OS/2`,
///   `cvt `, `fpgm` and `prep` byte for byte the face's, as this crate's
///   subsetter copies them; every glyph the subset's `glyf` holds the face's
///   at the same index, give or take the three bytes of padding a glyph may
///   carry.
/// - **CFF**, bare or in an `OpenType` face (whose copied tables are held to
///   the face's too, and `head` but its checksum): the same glyph count, the
///   same names or CIDs, collection, built-in encoding and font matrix of
///   every glyph; every glyph that draws in the subset outlining and
///   advancing as the face's at the same index, and one that draws nothing
///   kept where it advances as the face's.
/// - **Type 1**: the same clear-text portion; every charstring of the subset
///   named in the face, outlining and advancing as the face's of that name.
///
/// # Errors
///
/// An [`ExtendRefusal`] naming why; the caller keeps the subset, and a
/// document editor falls back to another font.
pub fn extend_subset(
    subset: &[u8],
    face: &[u8],
    glyphs: &BTreeSet<u16>,
) -> Result<Extended, ExtendRefusal> {
    let mut budget = EXTEND_BUDGET;
    match (Sfnt::parse(subset), Sfnt::parse(face)) {
        (Some(held), Some(source)) => {
            match (held.table(TAG_GLYF), source.table(TAG_GLYF)) {
                (Some(_), Some(_)) => return extend_truetype(&held, &source, face, glyphs),
                (None, None) => {}
                _ => return Err(ExtendRefusal::Format),
            }
            let (Some(held_cff), Some(source_cff)) = (held.table(TAG_CFF), source.table(TAG_CFF))
            else {
                return Err(ExtendRefusal::Format);
            };
            for &tag in COPIED_CFF {
                if held.table(tag) != source.table(tag) {
                    return Err(ExtendRefusal::NotTheSource);
                }
            }
            // `head` but `checkSumAdjustment`, which the subsetter computes.
            let unsummed = |table: Option<&[u8]>| {
                table.map(|head| {
                    let mut head = head.to_vec();
                    if let Some(sum) = head.get_mut(8..12) {
                        sum.fill(0);
                    }
                    head
                })
            };
            if unsummed(held.table(TAG_HEAD)) != unsummed(source.table(TAG_HEAD)) {
                return Err(ExtendRefusal::NotTheSource);
            }
            let keep = cff_kept(held_cff, source_cff, glyphs, &mut budget)?;
            let program =
                crate::subset::subset(face, &keep).ok_or(ExtendRefusal::NotRebuildable)?;
            Ok(Extended {
                program,
                lengths: None,
                names: None,
            })
        }
        (None, None) => {
            if crate::Type1::parse(subset).is_some() && crate::Type1::parse(face).is_some() {
                return extend_type1(subset, face, glyphs, &mut budget);
            }
            if Cff::parse(subset).is_none() || Cff::parse(face).is_none() {
                return Err(ExtendRefusal::Format);
            }
            let keep = cff_kept(subset, face, glyphs, &mut budget)?;
            let program =
                crate::cff_subset::subset_cff(face, &keep).ok_or(ExtendRefusal::NotRebuildable)?;
            Ok(Extended {
                program,
                lengths: None,
                names: None,
            })
        }
        _ => Err(ExtendRefusal::Format),
    }
}

/// Whether two `glyf` entries are one glyph: equal, but for up to three
/// bytes of zero padding a subsetter or a producer appends.
fn same_glyph(a: &[u8], b: &[u8]) -> bool {
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    long.starts_with(short)
        && long.len() - short.len() <= 3
        && long[short.len()..].iter().all(|byte| *byte == 0)
}

/// The `glyf` route: the source test, then the face cut to the subset's
/// glyphs and `glyphs`.
fn extend_truetype(
    held: &Sfnt<'_>,
    source: &Sfnt<'_>,
    face: &[u8],
    glyphs: &BTreeSet<u16>,
) -> Result<Extended, ExtendRefusal> {
    let count = |sfnt: &Sfnt<'_>| sfnt.table(0x6D61_7870).and_then(|maxp| be16(maxp, 4));
    let glyph_count = count(source).ok_or(ExtendRefusal::NotTheSource)?;
    if count(held) != Some(glyph_count) {
        return Err(ExtendRefusal::NotTheSource);
    }
    for &tag in COPIED {
        if held.table(tag) != source.table(tag) {
            return Err(ExtendRefusal::NotTheSource);
        }
    }
    let (Some(held_glyf), Some(source_glyf)) = (held.table(TAG_GLYF), source.table(TAG_GLYF))
    else {
        return Err(ExtendRefusal::Format);
    };
    let mut keep: BTreeSet<u16> = BTreeSet::from([0]);
    for glyph in 0..glyph_count {
        let (start, end) = glyph_range(held, glyph).ok_or(ExtendRefusal::NotTheSource)?;
        if start == end {
            continue;
        }
        let kept = held_glyf
            .get(start..end)
            .ok_or(ExtendRefusal::NotTheSource)?;
        let (from, to) = glyph_range(source, glyph).ok_or(ExtendRefusal::NotTheSource)?;
        let own = source_glyf
            .get(from..to)
            .ok_or(ExtendRefusal::NotTheSource)?;
        if !same_glyph(kept, own) {
            return Err(ExtendRefusal::NotTheSource);
        }
        keep.insert(glyph);
    }
    if glyphs.iter().any(|glyph| *glyph >= glyph_count) {
        return Err(ExtendRefusal::NoSuchGlyph);
    }
    keep.extend(glyphs.iter().copied());
    let program = subset(face, &keep).ok_or(ExtendRefusal::NotRebuildable)?;
    Ok(Extended {
        program,
        lengths: None,
        names: None,
    })
}

/// The CFF source test over two CFF tables, and the glyphs to cut the face
/// to: those the subset keeps, `.notdef`, and `glyphs`.
fn cff_kept(
    held: &[u8],
    source: &[u8],
    glyphs: &BTreeSet<u16>,
    budget: &mut u64,
) -> Result<BTreeSet<u16>, ExtendRefusal> {
    let (Some(held), Some(source)) = (Cff::parse(held), Cff::parse(source)) else {
        return Err(ExtendRefusal::Format);
    };
    let count = source.glyph_count();
    if held.glyph_count() != count
        || held.is_cid() != source.is_cid()
        || held.registry_ordering() != source.registry_ordering()
        || held.font_matrix != source.font_matrix
    {
        return Err(ExtendRefusal::NotTheSource);
    }
    if !source.is_cid()
        && (0..=255u8).any(|code| held.gid_for_code(code) != source.gid_for_code(code))
    {
        return Err(ExtendRefusal::NotTheSource);
    }
    let mut spend = |steps: u64| -> Result<(), ExtendRefusal> {
        *budget = budget.checked_sub(steps).ok_or(ExtendRefusal::Budget)?;
        Ok(())
    };
    let mut keep: BTreeSet<u16> = BTreeSet::from([0]);
    for gid in 0..count {
        let glyph = u16::try_from(gid).map_err(|_| ExtendRefusal::NotTheSource)?;
        if held.glyph_name(glyph) != source.glyph_name(glyph)
            || held.cid_for_gid(glyph) != source.cid_for_gid(glyph)
            || held.font_matrix_for(glyph) != source.font_matrix_for(glyph)
        {
            return Err(ExtendRefusal::NotTheSource);
        }
        let (kept, kept_steps) = held.outline_counted(glyph);
        let (own, own_steps) = source.outline_counted(glyph);
        spend(kept_steps.saturating_add(own_steps).saturating_mul(2))?;
        let (Some(kept), Some(own)) = (kept, own) else {
            return Err(ExtendRefusal::NotTheSource);
        };
        let advances = held.advance(glyph) == source.advance(glyph);
        if !kept.is_empty() {
            // A glyph that draws in the subset is the face's.
            if kept.segments != own.segments || !advances {
                return Err(ExtendRefusal::NotTheSource);
            }
            keep.insert(glyph);
        } else if own.is_empty() && advances {
            // One that draws nothing in either, kept with its advance.
            keep.insert(glyph);
        }
    }
    if glyphs.iter().any(|glyph| usize::from(*glyph) >= count) {
        return Err(ExtendRefusal::NoSuchGlyph);
    }
    keep.extend(glyphs.iter().copied());
    Ok(keep)
}

/// The Type 1 route: the source test, then the face cut by
/// [`crate::subset_type1`] to the subset's glyphs and `glyphs`.
fn extend_type1(
    subset: &[u8],
    face: &[u8],
    glyphs: &BTreeSet<u16>,
    budget: &mut u64,
) -> Result<Extended, ExtendRefusal> {
    let (Some(held_layout), Some(source_layout)) = (
        crate::type1::Layout::read(subset),
        crate::type1::Layout::read(face),
    ) else {
        return Err(ExtendRefusal::Format);
    };
    let held_clear = held_layout.data.get(..held_layout.encrypted_at);
    if held_clear != source_layout.data.get(..source_layout.encrypted_at) {
        return Err(ExtendRefusal::NotTheSource);
    }
    let (Some(held), Some(source)) = (crate::Type1::parse(subset), crate::Type1::parse(face))
    else {
        return Err(ExtendRefusal::Format);
    };
    let mut spend = |steps: u64| -> Result<(), ExtendRefusal> {
        *budget = budget.checked_sub(steps).ok_or(ExtendRefusal::Budget)?;
        Ok(())
    };
    let mut keep: BTreeSet<u16> = BTreeSet::new();
    for index in 0..held.glyph_count() {
        let name = held.glyph_name(index).ok_or(ExtendRefusal::NotTheSource)?;
        let own = source
            .glyph_for_name(name)
            .ok_or(ExtendRefusal::NotTheSource)?;
        let glyph = u16::try_from(index).map_err(|_| ExtendRefusal::NotTheSource)?;
        let (kept, kept_steps) = held.outline_counted(glyph);
        let (drawn, drawn_steps) = source.outline_counted(own);
        spend(kept_steps.saturating_add(drawn_steps).saturating_mul(2))?;
        let same = match (kept, drawn) {
            (Some(a), Some(b)) => a.segments == b.segments,
            (None, None) => true,
            _ => false,
        };
        if !same || held.advance(glyph) != source.advance(own) {
            return Err(ExtendRefusal::NotTheSource);
        }
        keep.insert(own);
    }
    if glyphs
        .iter()
        .any(|glyph| usize::from(*glyph) >= source.glyph_count())
    {
        return Err(ExtendRefusal::NoSuchGlyph);
    }
    keep.extend(glyphs.iter().copied());
    let cut = crate::subset_type1(face, &keep).map_err(|_| ExtendRefusal::NotRebuildable)?;
    Ok(Extended {
        program: cut.program,
        lengths: Some(cut.lengths),
        names: Some(cut.names),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A font with `count` glyphs, each a square of a different size, and a
    /// format-4 `cmap` mapping 'A'.. onto them.
    ///
    /// Hand-built rather than loaded: the point of most of these tests is
    /// what the *output* looks like, and a fixture whose every byte is known
    /// makes "the subset dropped the right thing" checkable by arithmetic.
    fn font(count: u16) -> Vec<u8> {
        let mut glyf: Vec<u8> = Vec::new();
        let mut loca: Vec<u32> = Vec::new();

        for glyph in 0..count {
            loca.push(glyf.len() as u32);
            // One contour, four points: a simple glyph, ten bytes of header
            // plus the contour data.
            glyf.extend_from_slice(&1i16.to_be_bytes()); // contours
            glyf.extend_from_slice(&0i16.to_be_bytes()); // xMin
            glyf.extend_from_slice(&0i16.to_be_bytes()); // yMin
            glyf.extend_from_slice(&(100 + glyph as i16).to_be_bytes()); // xMax
            glyf.extend_from_slice(&100i16.to_be_bytes()); // yMax
            glyf.extend_from_slice(&3u16.to_be_bytes()); // end point
            glyf.extend_from_slice(&0u16.to_be_bytes()); // no instructions
                                                         // On curve, and both deltas one positive byte: 0x01 | 0x02 |
                                                         // 0x10 | 0x04 | 0x20. Without the short-and-positive bits each
                                                         // delta is a two-byte signed value and the four bytes below are
                                                         // read as two points and then a run off the end of the glyph.
            glyf.extend_from_slice(&[0x37; 4]);
            glyf.extend_from_slice(&[10, 20, 30, 40]); // x deltas
            glyf.extend_from_slice(&[10, 20, 30, 40]); // y deltas
            while glyf.len() % 4 != 0 {
                glyf.push(0);
            }
        }
        loca.push(glyf.len() as u32);

        let mut loca_bytes = Vec::new();
        for offset in &loca {
            loca_bytes.extend_from_slice(&offset.to_be_bytes());
        }

        let mut head = vec![0u8; 54];
        head[18..20].copy_from_slice(&1000u16.to_be_bytes()); // unitsPerEm
        head[50..52].copy_from_slice(&1u16.to_be_bytes()); // long loca

        let mut maxp = vec![0u8; 32];
        maxp[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
        maxp[4..6].copy_from_slice(&count.to_be_bytes());

        let mut hhea = vec![0u8; 36];
        hhea[34..36].copy_from_slice(&count.to_be_bytes()); // numberOfHMetrics

        let mut hmtx = Vec::new();
        for glyph in 0..count {
            hmtx.extend_from_slice(&(500 + glyph).to_be_bytes()); // advance
            hmtx.extend_from_slice(&0i16.to_be_bytes()); // lsb
        }

        // cmap format 4, one segment mapping 'A'..'A'+count-1 onto glyph ids
        // by a constant delta, plus the required terminating segment.
        // 'A' maps to glyph 1, not glyph 0: glyph zero is what an uncovered
        // character resolves to, so a fixture that maps 'A' there cannot tell
        // "the font has this" from "the font does not".
        let first = u16::from(b'A');
        let last = first + count - 2;
        let mut sub = Vec::new();
        sub.extend_from_slice(&4u16.to_be_bytes()); // format
        sub.extend_from_slice(&32u16.to_be_bytes()); // length
        sub.extend_from_slice(&0u16.to_be_bytes()); // language
        sub.extend_from_slice(&4u16.to_be_bytes()); // segCountX2
        sub.extend_from_slice(&4u16.to_be_bytes()); // searchRange
        sub.extend_from_slice(&1u16.to_be_bytes()); // entrySelector
        sub.extend_from_slice(&0u16.to_be_bytes()); // rangeShift
        sub.extend_from_slice(&last.to_be_bytes()); // endCode[0]
        sub.extend_from_slice(&0xFFFFu16.to_be_bytes()); // endCode[1]
        sub.extend_from_slice(&0u16.to_be_bytes()); // reservedPad
        sub.extend_from_slice(&first.to_be_bytes()); // startCode[0]
        sub.extend_from_slice(&0xFFFFu16.to_be_bytes()); // startCode[1]
        sub.extend_from_slice(&1u16.wrapping_sub(first).to_be_bytes()); // idDelta[0]
        sub.extend_from_slice(&1u16.to_be_bytes()); // idDelta[1]
        sub.extend_from_slice(&0u16.to_be_bytes()); // idRangeOffset[0]
        sub.extend_from_slice(&0u16.to_be_bytes()); // idRangeOffset[1]

        let mut cmap = Vec::new();
        cmap.extend_from_slice(&0u16.to_be_bytes()); // version
        cmap.extend_from_slice(&1u16.to_be_bytes()); // one table
        cmap.extend_from_slice(&3u16.to_be_bytes()); // platform: Windows
        cmap.extend_from_slice(&1u16.to_be_bytes()); // encoding: BMP
        cmap.extend_from_slice(&12u32.to_be_bytes()); // offset
        cmap.extend_from_slice(&sub);

        let tables: Vec<(u32, Vec<u8>)> = {
            let mut t = vec![
                (0x636D_6170, cmap),
                (0x676C_7966, glyf),
                (0x6865_6164, head),
                (0x6868_6561, hhea),
                (0x686D_7478, hmtx),
                (0x6C6F_6361, loca_bytes),
                (0x6D61_7870, maxp),
            ];
            t.sort_by_key(|(tag, _)| *tag);
            t
        };
        assemble(0x0001_0000, &tables)
    }

    /// `program` with its `hmtx` holding `long` full entries, the advances
    /// `advances` gives the first `long` glyphs, and a side bearing of
    /// `10 * glyph` for every glyph: the shape a face whose last glyphs share
    /// one advance is stored in.
    fn compressed(program: &[u8], long: u16, advances: &[u16]) -> Vec<u8> {
        let sfnt = Sfnt::parse(program).expect("the fixture parses");
        let count = sfnt.glyph_count().expect("a maxp");
        let mut tables: Vec<(u32, Vec<u8>)> = Vec::new();
        for tag in [
            0x636D_6170u32,
            0x676C_7966,
            0x6865_6164,
            0x6868_6561,
            0x686D_7478,
            0x6C6F_6361,
            0x6D61_7870,
        ] {
            let mut data = sfnt.table(tag).expect("the fixture's table").to_vec();
            if tag == 0x6868_6561 {
                data[34..36].copy_from_slice(&long.to_be_bytes());
            }
            if tag == 0x686D_7478 {
                data.clear();
                for glyph in 0..count {
                    if glyph < long {
                        data.extend_from_slice(&advances[usize::from(glyph)].to_be_bytes());
                    }
                    data.extend_from_slice(&(glyph * 10).to_be_bytes());
                }
            }
            tables.push((tag, data));
        }
        tables.sort_by_key(|(tag, _)| *tag);
        assemble(0x0001_0000, &tables)
    }

    /// An empty reshape is the cut byte for byte; a reshape sets exactly the
    /// advances it names, of kept glyphs, rounded to `hmtx`'s integers, and
    /// leaves every other advance, every side bearing and every outline.
    #[test]
    fn advances_are_set_where_asked_and_nowhere_else() {
        let program = font(8);
        let keep: BTreeSet<u16> = [1u16, 2, 3].into_iter().collect();
        assert_eq!(
            subset_with(&program, &keep, &Reshape::default()),
            subset(&program, &keep),
            "nothing to set is the cut itself"
        );

        let reshape = Reshape {
            advances: BTreeMap::from([(2u16, 640.4), (3, 610.6), (6, 999.0)]),
            ..Reshape::default()
        };
        let out = subset_with(&program, &keep, &reshape).expect("it reshapes");
        let before = Sfnt::parse(&program).expect("the fixture parses");
        let after = Sfnt::parse(&out).expect("the reshaped cut parses");
        assert_eq!(after.advance(2), Some(640));
        assert_eq!(after.advance(3), Some(611));
        for glyph in [0u16, 1, 4, 5, 6, 7] {
            assert_eq!(
                after.advance(glyph),
                Some(500 + glyph),
                "glyph {glyph}: not asked, or not kept"
            );
        }
        for glyph in [1u16, 2, 3] {
            assert_eq!(
                crate::glyf::outline(&after, glyph).map(|o| o.segments),
                crate::glyf::outline(&before, glyph).map(|o| o.segments),
                "glyph {glyph} draws as it did"
            );
        }
        let hhea = after.table(0x6868_6561).expect("an hhea");
        assert_eq!(be16(hhea, 10), Some(640), "advanceWidthMax follows");
    }

    /// A glyph past `numberOfHMetrics` shares the last full entry's advance,
    /// so setting one gives every glyph an entry of its own, and the glyphs
    /// after it keep the advance they had rather than taking the new one.
    #[test]
    fn an_advance_past_the_long_entries_widens_hmtx() {
        let program = compressed(&font(8), 2, &[500, 501]);
        let source = Sfnt::parse(&program).expect("the fixture parses");
        assert_eq!(
            source.advance(6),
            Some(501),
            "the fixture repeats its last advance"
        );

        let keep: BTreeSet<u16> = [1u16, 5].into_iter().collect();
        let reshape = Reshape {
            advances: BTreeMap::from([(5u16, 700.0)]),
            ..Reshape::default()
        };
        let out = subset_with(&program, &keep, &reshape).expect("it reshapes");
        let after = Sfnt::parse(&out).expect("the reshaped cut parses");
        assert_eq!(after.advance(5), Some(700));
        for glyph in [1u16, 2, 3, 4, 6, 7] {
            assert_eq!(after.advance(glyph), Some(501), "glyph {glyph}");
        }
        for glyph in 0..8u16 {
            assert_eq!(
                after.left_side_bearing(glyph),
                Some(glyph as i16 * 10),
                "glyph {glyph} keeps its side bearing"
            );
        }
        let hhea = after.table(0x6868_6561).expect("an hhea");
        assert_eq!(be16(hhea, 34), Some(8), "every glyph has an entry now");
        assert_eq!(be16(hhea, 10), Some(700));

        // The last full entry is the one the glyphs after it repeat, so
        // setting it widens the table too, and they keep theirs.
        let reshape = Reshape {
            advances: BTreeMap::from([(1u16, 650.0)]),
            ..Reshape::default()
        };
        let out = subset_with(&program, &keep, &reshape).expect("it reshapes");
        let after = Sfnt::parse(&out).expect("the reshaped cut parses");
        assert_eq!(after.advance(1), Some(650));
        assert_eq!(after.advance(7), Some(501));
        assert_eq!(
            be16(after.table(0x6868_6561).expect("an hhea"), 34),
            Some(8)
        );

        // Before it, the table keeps its shape.
        let reshape = Reshape {
            advances: BTreeMap::from([(0u16, 450.0)]),
            ..Reshape::default()
        };
        let out = subset_with(&program, &keep, &reshape).expect("it reshapes");
        let after = Sfnt::parse(&out).expect("the reshaped cut parses");
        assert_eq!(after.advance(0), Some(450));
        assert_eq!(after.advance(7), Some(501));
        assert_eq!(
            be16(after.table(0x6868_6561).expect("an hhea"), 34),
            Some(2)
        );
    }

    /// An advance `hmtx` cannot hold — negative, past 65 535, not a number —
    /// declines the reshape rather than writing another number, and an
    /// `OpenType/CFF` face, whose advances are two tables', declines any.
    #[test]
    fn an_advance_no_entry_holds_is_declined() {
        let program = font(8);
        let keep: BTreeSet<u16> = [1u16].into_iter().collect();
        for bad in [-1.0, 70_000.0, f64::NAN, f64::INFINITY] {
            let reshape = Reshape {
                advances: BTreeMap::from([(1u16, bad)]),
                ..Reshape::default()
            };
            assert_eq!(subset_with(&program, &keep, &reshape), None, "{bad}");
        }
        let otto = assemble(
            0x4F54_544F,
            &[(TAG_CFF, vec![1, 0, 4, 4]), (TAG_HEAD, vec![0; 54])],
        );
        let reshape = Reshape {
            advances: BTreeMap::from([(1u16, 600.0)]),
            ..Reshape::default()
        };
        assert_eq!(subset_with(&otto, &keep, &reshape), None);
    }

    /// A symbol `cmap` is one (3,0) subtable reaching each code's glyph at
    /// `0xF000` plus the code — as the face's character lookup reads a code
    /// below 256 through such a subtable, and as its symbol-code lookup
    /// does — and nothing else: a code it was not handed, and a code handed
    /// glyph 0, reach nothing. A `glyf` face's cut takes it in place of its
    /// own, and a bare CFF, which has none, declines one.
    #[test]
    fn a_symbol_cmap_maps_each_code_at_f000() {
        let codes = BTreeMap::from([(0x20u8, 3u16), (0x41, 5), (0x42, 0), (0xFF, 7)]);
        let table = symbol_cmap(&codes);
        assert_eq!(be16(&table, 2), Some(1), "exactly one subtable");
        assert_eq!(
            (be16(&table, 4), be16(&table, 6)),
            (Some(3), Some(0)),
            "the symbol encoding"
        );

        let program = font(8);
        let keep: BTreeSet<u16> = [3u16, 5, 7].into_iter().collect();
        let reshape = Reshape {
            cmap: Some(table.clone()),
            ..Reshape::default()
        };
        let out = subset_with(&program, &keep, &reshape).expect("it reshapes");
        let sfnt = Sfnt::parse(&out).expect("the cut parses");
        assert_eq!(sfnt.table(0x636D_6170), Some(table.as_slice()));
        for (code, glyph) in [(0x20u8, Some(3u16)), (0x41, Some(5)), (0xFF, Some(7))] {
            assert_eq!(sfnt.glyph_for_char(char::from(code)), glyph, "{code:#04X}");
            assert_eq!(sfnt.glyph_for_symbol_code(code), glyph, "{code:#04X}");
        }
        for code in [0x42u8, 0x43, 0x00] {
            assert_eq!(
                sfnt.glyph_for_char(char::from(code)).filter(|g| *g != 0),
                None,
                "{code:#04X} reaches nothing"
            );
        }
        assert_eq!(
            sfnt.glyph_for_char('A'),
            Some(5),
            "a character below 256 is read at F0xx, as a code is"
        );

        // A program that is no sfnt — a bare CFF — has no `cmap` to replace.
        assert_eq!(subset_with(&[1, 0, 4, 4], &keep, &reshape), None);
    }

    #[test]
    fn the_fixture_is_a_font_this_crate_can_read() {
        let program = font(8);
        let sfnt = Sfnt::parse(&program).expect("it parses");
        assert_eq!(sfnt.units_per_em, 1000);
        assert_eq!(sfnt.glyph_for_char('A'), Some(1));
        assert_eq!(sfnt.glyph_for_char('C'), Some(3));
        assert_eq!(sfnt.advance(3), Some(503));

        // `outline` returns Some for a glyph that draws nothing, so the
        // segment count is what actually says the fixture is readable.
        let outline = crate::glyf::outline(&sfnt, 3).expect("an outline");
        assert!(!outline.segments.is_empty(), "the fixture's glyphs draw");
    }

    #[test]
    fn a_subset_keeps_the_glyphs_it_was_asked_for() {
        let program = font(8);
        let wanted: BTreeSet<u16> = [2u16, 5].into_iter().collect();
        let subset = subset(&program, &wanted).expect("it subsets");

        let sfnt = Sfnt::parse(&subset).expect("the subset parses");
        for glyph in [2u16, 5] {
            let outline = crate::glyf::outline(&sfnt, glyph).expect("an outline");
            assert!(
                !outline.segments.is_empty(),
                "glyph {glyph} was asked for and must still draw"
            );
        }
    }

    /// The headline: the glyphs nobody asked for are gone, and the file is
    /// smaller for it. Without this the whole exercise is a no-op that still
    /// says it worked.
    #[test]
    fn a_subset_drops_the_glyphs_it_was_not_asked_for() {
        let program = font(64);
        let wanted: BTreeSet<u16> = [2u16, 5].into_iter().collect();
        let subset = subset(&program, &wanted).expect("it subsets");

        assert!(
            subset.len() < program.len(),
            "the subset is smaller: {} vs {}",
            subset.len(),
            program.len()
        );

        let sfnt = Sfnt::parse(&subset).expect("the subset parses");
        for glyph in [7u16, 30, 63] {
            let outline = crate::glyf::outline(&sfnt, glyph).unwrap_or_default();
            assert!(
                outline.segments.is_empty(),
                "glyph {glyph} was not asked for and must draw nothing"
            );
        }
    }

    /// Identifiers staying put is the property everything outside this crate
    /// depends on — `/Widths`, `cmap`, composite components. If the subset
    /// renumbered, the character 'C' would come out as some other letter.
    #[test]
    fn glyph_identifiers_do_not_move() {
        let program = font(16);
        let wanted: BTreeSet<u16> = [2u16, 9].into_iter().collect();
        let subset = subset(&program, &wanted).expect("it subsets");

        let original = Sfnt::parse(&program).expect("it parses");
        let reduced = Sfnt::parse(&subset).expect("the subset parses");

        assert_eq!(reduced.glyph_for_char('C'), original.glyph_for_char('C'));
        assert_eq!(reduced.glyph_for_char('C'), Some(3));
        assert_eq!(reduced.advance(2), original.advance(2));
        assert_eq!(reduced.advance(9), original.advance(9));
        assert_eq!(
            crate::glyf::outline(&reduced, 2).map(|o| o.segments.len()),
            crate::glyf::outline(&original, 2).map(|o| o.segments.len()),
            "and the outline is the same one"
        );
    }

    #[test]
    fn the_notdef_glyph_survives_even_unasked() {
        let program = font(8);
        let wanted: BTreeSet<u16> = [3u16].into_iter().collect();
        let subset = subset(&program, &wanted).expect("it subsets");
        let sfnt = Sfnt::parse(&subset).expect("the subset parses");
        let outline = crate::glyf::outline(&sfnt, 0).expect("an outline");
        assert!(
            !outline.segments.is_empty(),
            "glyph zero is what a reader draws for anything missing"
        );
    }

    /// A composite glyph is a reference to others. Keeping it without them
    /// draws blank space — the failure nobody notices until the file reaches
    /// a reader of a language that uses accents.
    #[test]
    fn a_composites_components_come_with_it() {
        let mut program = font(8);

        // Rewrite glyph 6 as a composite referring to glyph 3, in place, and
        // repoint `loca` at the new data appended to `glyf`.
        let sfnt = Sfnt::parse(&program).expect("it parses");
        let glyf_at = {
            let table = sfnt.table(0x676C_7966).expect("glyf");
            table.as_ptr() as usize - program.as_ptr() as usize
        };
        let glyf_len = sfnt.table(0x676C_7966).expect("glyf").len();
        let loca_at = {
            let table = sfnt.table(0x6C6F_6361).expect("loca");
            table.as_ptr() as usize - program.as_ptr() as usize
        };
        let (start, end) = glyph_range(&sfnt, 6).expect("a range");
        assert!(end - start >= 16, "room for a composite record");

        let mut composite = Vec::new();
        composite.extend_from_slice(&(-1i16).to_be_bytes()); // composite
        composite.extend_from_slice(&0i16.to_be_bytes()); // xMin
        composite.extend_from_slice(&0i16.to_be_bytes()); // yMin
        composite.extend_from_slice(&100i16.to_be_bytes()); // xMax
        composite.extend_from_slice(&100i16.to_be_bytes()); // yMax
        composite.extend_from_slice(&0x0002u16.to_be_bytes()); // ARGS_ARE_XY
        composite.extend_from_slice(&3u16.to_be_bytes()); // component: glyph 3
        composite.extend_from_slice(&[0, 0]); // byte offsets
        while composite.len() < end - start {
            composite.push(0);
        }
        program[glyf_at + start..glyf_at + start + composite.len()]
            .copy_from_slice(&composite[..end - start]);
        assert!(glyf_len > 0);
        let _ = loca_at;

        let sfnt = Sfnt::parse(&program).expect("it parses");
        assert_eq!(
            components(&sfnt, 6),
            vec![3],
            "the fixture really is a composite now"
        );

        // Ask for the composite alone; its component must come along.
        let wanted: BTreeSet<u16> = [6u16].into_iter().collect();
        let subset = subset(&program, &wanted).expect("it subsets");
        let reduced = Sfnt::parse(&subset).expect("the subset parses");

        let outline = crate::glyf::outline(&reduced, 6).expect("an outline");
        assert!(
            !outline.segments.is_empty(),
            "the composite draws, which means its component survived"
        );
    }

    /// `face` as the only member of a TrueType Collection.
    ///
    /// The header is 16 bytes — `ttcf`, a version, the number of fonts and
    /// one offset — and every table record in the face then has to move by
    /// those 16, because a member's table offsets are measured from the start
    /// of the **collection** and not of the member. That layout is the
    /// OpenType specification's; what this file adjudicates is only what this
    /// crate writes when it is handed one.
    ///
    /// The bug this exists for is not hypothetical and was not found here:
    /// `crates/tinker-pdf/tests/cff_subset_census.rs` found it in two
    /// documents of the fetched corpora, which is third-party bytes and the
    /// only evidence that says real producers embed collections at all.
    fn collection_of(face: &[u8]) -> Vec<u8> {
        const HEADER: u32 = 16;
        let mut out = Vec::new();
        out.extend_from_slice(b"ttcf");
        out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        out.extend_from_slice(&1u32.to_be_bytes());
        out.extend_from_slice(&HEADER.to_be_bytes());
        out.extend_from_slice(face);

        let count = be16(face, 4).expect("the face has a table count");
        for index in 0..usize::from(count) {
            let at = HEADER as usize + 12 + index * 16;
            let offset = be32(&out, at + 8).expect("the record is present");
            out[at + 8..at + 12].copy_from_slice(&(offset + HEADER).to_be_bytes());
        }
        out
    }

    /// A collection comes out a *font*, not a collection.
    ///
    /// `Sfnt::parse` reads a `ttcf` by taking the first member, so everything
    /// assembled here came from that member's directory — and the output is a
    /// single flat directory, which must therefore declare that member's own
    /// `sfntVersion`. Declaring the file's instead produced bytes claiming to
    /// be a collection whose first member began at the offset spelled by its
    /// own first table tag: unreadable, by this crate and by any other.
    #[test]
    fn a_collection_is_rebuilt_as_the_face_it_was_read_as() {
        let collection = collection_of(&font(8));
        assert_eq!(&collection[..4], b"ttcf", "the fixture is a collection");
        assert!(
            Sfnt::parse(&collection).is_some(),
            "and one this crate reads, or the test below proves nothing"
        );

        let wanted: BTreeSet<u16> = [1u16, 3].into_iter().collect();
        let out = subset(&collection, &wanted).expect("a collection subsets");

        assert_eq!(
            be32(&out, 0),
            Some(0x0001_0000),
            "the output declares the member's version, not the file's"
        );
        let sfnt = Sfnt::parse(&out).expect("the subset is a font this crate can read");
        assert_eq!(
            be16(sfnt.table(0x6D61_7870).expect("maxp"), 4),
            Some(8),
            "and the glyph identifiers did not move"
        );
        for glyph in &wanted {
            let outline = crate::glyf::outline(&sfnt, *glyph).unwrap_or_default();
            assert!(
                !outline.segments.is_empty(),
                "glyph {glyph} was asked for and must still draw"
            );
        }
        let dropped = crate::glyf::outline(&sfnt, 5).unwrap_or_default();
        assert!(
            dropped.segments.is_empty(),
            "glyph 5 was not asked for and must draw nothing"
        );
    }

    #[test]
    fn a_program_that_is_not_truetype_is_refused_rather_than_mangled() {
        assert!(subset(b"", &BTreeSet::new()).is_none());
        assert!(subset(b"not a font at all", &BTreeSet::new()).is_none());
        // A valid directory with no `glyf`: a CFF-flavoured OpenType, which
        // needs its own charstring index rebuilt and is not this.
        let stripped = assemble(0x4F54_544F, &[(0x6865_6164, vec![0u8; 54])]);
        assert!(subset(&stripped, &BTreeSet::new()).is_none());
    }

    #[test]
    fn glyphs_are_looked_up_through_the_fonts_own_cmap() {
        let program = font(8);
        let wanted = glyphs_for(&program, "CA");
        assert_eq!(wanted, [1u16, 3].into_iter().collect::<BTreeSet<_>>());
        assert!(
            glyphs_for(&program, "\u{4E00}").is_empty(),
            "a character the font does not cover resolves to .notdef, which \
             the subset keeps anyway, so it asks for nothing extra"
        );
    }

    #[test]
    fn asking_for_glyphs_the_font_does_not_have_is_harmless() {
        let program = font(4);
        let wanted: BTreeSet<u16> = [1u16, 9999].into_iter().collect();
        let subset = subset(&program, &wanted).expect("it subsets");
        let sfnt = Sfnt::parse(&subset).expect("the subset parses");
        assert!(crate::glyf::outline(&sfnt, 1).is_some());
    }

    // -----------------------------------------------------------------------
    // Extending a subset (FT-11.9).
    // -----------------------------------------------------------------------

    /// The glyphs a `glyf` program holds: every index whose entry is not
    /// empty.
    fn held(program: &[u8]) -> BTreeSet<u16> {
        let sfnt = Sfnt::parse(program).expect("an sfnt");
        let count = be16(sfnt.table(0x6D61_7870).expect("maxp"), 4).expect("a count");
        (0..count)
            .filter(|glyph| glyph_range(&sfnt, *glyph).is_some_and(|(start, end)| end > start))
            .collect()
    }

    /// Type 1 numbers in their shortest forms (chapter 6.2).
    fn n(values: &[i32]) -> Vec<u8> {
        let mut out = Vec::new();
        for &v in values {
            if (-107..=107).contains(&v) {
                out.push((v + 139) as u8);
            } else if (108..=1131).contains(&v) {
                let v = v - 108;
                out.push((v / 256 + 247) as u8);
                out.push((v % 256) as u8);
            } else {
                let v = -v - 108;
                out.push((v / 256 + 251) as u8);
                out.push((v % 256) as u8);
            }
        }
        out
    }

    /// A glyph `width` wide drawing a `w` by `h` box at `(x, y)`: `hsbw`,
    /// `rmoveto`, three sides, `closepath`, `endchar`.
    fn boxed(width: i32, x: i32, y: i32, w: i32, h: i32) -> Vec<u8> {
        [
            n(&[0, width]),
            vec![13],
            n(&[x, y]),
            vec![21],
            n(&[w]),
            vec![6],
            n(&[h]),
            vec![7],
            n(&[-w]),
            vec![6, 9, 14],
        ]
        .concat()
    }

    /// A Type 1 program of `.notdef` and `A` to `E`, boxes each `size` apart,
    /// encoded at 65 to 69, and no subroutines.
    fn type1_face(size: i32) -> Vec<u8> {
        use crate::type1::{encrypt, CHARSTRING_KEY, EEXEC_KEY};
        let lead = [0x55u8; 4];
        let mut glyphs = vec![(".notdef".to_owned(), [n(&[0, 250]), vec![13, 14]].concat())];
        for (at, name) in ["A", "B", "C", "D", "E"].iter().enumerate() {
            let at = at as i32;
            glyphs.push(((*name).to_owned(), boxed(600, 50, 0, size + at * 10, 400)));
        }
        let mut clear = b"%!PS-AdobeFont-1.0: Fixture 001.000\n11 dict begin\n\
            /FontName /Fixture def\n/FontType 1 def\n\
            /FontMatrix [0.001 0 0 0.001 0 0] readonly def\n\
            /FontBBox {0 0 1000 1000} readonly def\n\
            /Encoding 256 array\n0 1 255 {1 index exch /.notdef put} for\n"
            .to_vec();
        for (code, name) in ["A", "B", "C", "D", "E"].iter().enumerate() {
            clear.extend_from_slice(format!("dup {} /{name} put\n", 65 + code).as_bytes());
        }
        clear.extend_from_slice(b"readonly def\ncurrentdict end\ncurrentfile eexec\n");
        let mut private = b"dup /Private 8 dict dup begin\n/lenIV 4 def\n/Subrs 0 array\nND\n\
            2 index /CharStrings 6 dict dup begin\n"
            .to_vec();
        for (name, code) in &glyphs {
            let encrypted = encrypt(code, CHARSTRING_KEY, &lead);
            private.extend_from_slice(format!("/{name} {} RD ", encrypted.len()).as_bytes());
            private.extend_from_slice(&encrypted);
            private.extend_from_slice(b" ND\n");
        }
        private.extend_from_slice(
            b"end\nend\nreadonly put\nnoaccess put\n\
              dup /FontName get exch definefont pop\nmark currentfile closefile\n",
        );
        let mut out = clear;
        out.extend(encrypt(&private, EEXEC_KEY, &lead));
        out.extend_from_slice(b"\n0000000000000000\ncleartomark\n");
        out
    }

    /// The CFF the Type 1 face converts to, and the CFF glyph of each of its
    /// names.
    fn cff_face(size: i32) -> (Vec<u8>, BTreeMap<String, u16>) {
        let converted = crate::type1_to_cff(&type1_face(size)).expect("the fixture converts");
        let gids = converted
            .names
            .iter()
            .enumerate()
            .map(|(gid, name)| (String::from_utf8_lossy(name).into_owned(), gid as u16))
            .collect();
        (converted.program, gids)
    }

    /// **A TrueType subset gains exactly the glyphs asked for**, at their own
    /// indices, every glyph it kept the face's, and nothing else.
    #[test]
    fn a_truetype_subset_extends_with_exactly_the_glyphs_asked_for() {
        let face = font(10);
        let cut = subset(&face, &[2u16, 5].into_iter().collect()).expect("it subsets");
        assert_eq!(held(&cut), [0u16, 2, 5].into_iter().collect());

        let extended =
            extend_subset(&cut, &face, &[3u16, 7].into_iter().collect()).expect("it extends");
        assert_eq!(extended.lengths, None);
        assert_eq!(
            held(&extended.program),
            [0u16, 2, 3, 5, 7].into_iter().collect()
        );
        assert_eq!(
            extended.program,
            subset(&face, &[2u16, 3, 5, 7].into_iter().collect()).expect("the cut"),
            "the face cut to the subset's glyphs and the new ones"
        );
        let (before, after) = (
            Sfnt::parse(&face).expect("face"),
            Sfnt::parse(&extended.program).expect("extension"),
        );
        for glyph in [0u16, 2, 3, 5, 7] {
            assert_eq!(
                crate::glyf::outline(&after, glyph).map(|o| o.segments),
                crate::glyf::outline(&before, glyph).map(|o| o.segments),
                "glyph {glyph}"
            );
        }
        // Extending again with a glyph it holds adds nothing.
        let again = extend_subset(&extended.program, &face, &[3u16].into_iter().collect())
            .expect("it extends");
        assert_eq!(again.program, extended.program);
    }

    /// A face that is not the subset's source is refused: another glyph
    /// count, and one glyph drawn otherwise.
    #[test]
    fn a_truetype_face_that_is_not_the_source_is_refused() {
        let face = font(10);
        let cut = subset(&face, &[2u16].into_iter().collect()).expect("it subsets");
        let wanted: BTreeSet<u16> = [3u16].into_iter().collect();
        assert_eq!(
            extend_subset(&cut, &font(11), &wanted),
            Err(ExtendRefusal::NotTheSource)
        );
        // Glyph 2's last y delta, one more in a face otherwise the same.
        let mut other = face.clone();
        let sfnt = Sfnt::parse(&face).expect("it parses");
        let glyf = sfnt.table(0x676C_7966).expect("glyf");
        let glyf_at = glyf.as_ptr() as usize - face.as_ptr() as usize;
        let (start, _) = glyph_range(&sfnt, 2).expect("glyph 2");
        other[glyf_at + start + 25] = other[glyf_at + start + 25].wrapping_add(1);
        assert_eq!(
            extend_subset(&cut, &other, &wanted),
            Err(ExtendRefusal::NotTheSource)
        );
        // A table the subsetter copies, changed.
        let mut reshaped = BTreeMap::new();
        reshaped.insert(2u16, 900.0);
        let widened = subset_with(
            &face,
            &[2u16].into_iter().collect(),
            &Reshape {
                advances: reshaped,
                cmap: None,
            },
        )
        .expect("it subsets");
        assert_eq!(
            extend_subset(&widened, &face, &wanted),
            Err(ExtendRefusal::NotTheSource)
        );
    }

    #[test]
    fn a_glyph_past_the_face_is_refused() {
        let face = font(10);
        let cut = subset(&face, &[2u16].into_iter().collect()).expect("it subsets");
        assert_eq!(
            extend_subset(&cut, &face, &[10u16].into_iter().collect()),
            Err(ExtendRefusal::NoSuchGlyph)
        );
    }

    /// **A CFF subset gains exactly the glyphs asked for**: the kept and the
    /// new draw as in the face, every other glyph is empty.
    #[test]
    fn a_cff_subset_extends_with_exactly_the_glyphs_asked_for() {
        let (face, gids) = cff_face(100);
        let cut = crate::subset_cff(&face, &[gids["A"], gids["C"]].into_iter().collect())
            .expect("it subsets");
        let extended =
            extend_subset(&cut, &face, &[gids["D"]].into_iter().collect()).expect("it extends");
        let (source, after) = (
            Cff::parse(&face).expect("face"),
            Cff::parse(&extended.program).expect("extension"),
        );
        assert_eq!(after.glyph_count(), source.glyph_count());
        for (name, &gid) in &gids {
            let drawn = after.outline(gid).expect("it answers");
            if ["A", "C", "D"].contains(&name.as_str()) {
                assert_eq!(
                    drawn.segments,
                    source.outline(gid).expect("face").segments,
                    "{name}"
                );
                assert_eq!(after.advance(gid), source.advance(gid), "{name}");
            } else {
                assert!(drawn.is_empty(), "{name} was not asked for");
            }
        }
    }

    #[test]
    fn a_cff_face_that_is_not_the_source_is_refused() {
        let (face, gids) = cff_face(100);
        let cut = crate::subset_cff(&face, &[gids["A"]].into_iter().collect()).expect("it subsets");
        let (other, _) = cff_face(120);
        assert_eq!(
            extend_subset(&cut, &other, &[gids["B"]].into_iter().collect()),
            Err(ExtendRefusal::NotTheSource)
        );
        assert_eq!(
            extend_subset(&cut, &face, &[60u16].into_iter().collect()),
            Err(ExtendRefusal::NoSuchGlyph)
        );
    }

    /// **A Type 1 subset gains the charstrings asked for**, with its three
    /// lengths and its names, every kept glyph drawing as in the face.
    #[test]
    fn a_type1_subset_extends_with_the_glyphs_asked_for() {
        let face = type1_face(100);
        let source = crate::Type1::parse(&face).expect("the face parses");
        let index = |name: &[u8]| source.glyph_for_name(name).expect("in the face");
        let cut =
            crate::subset_type1(&face, &[index(b"A")].into_iter().collect()).expect("it subsets");
        let extended = extend_subset(&cut.program, &face, &[index(b"C")].into_iter().collect())
            .expect("it extends");
        assert_eq!(
            extended.names,
            Some(vec![b".notdef".to_vec(), b"A".to_vec(), b"C".to_vec()])
        );
        let lengths = extended.lengths.expect("Table 125's lengths");
        assert_eq!(lengths.iter().sum::<usize>(), extended.program.len());
        let after = crate::Type1::parse(&extended.program).expect("the extension parses");
        for name in [b".notdef".as_slice(), b"A", b"C"] {
            let (a, b) = (after.glyph_for_name(name).expect("kept"), index(name));
            assert_eq!(
                after.outline(a).map(|o| o.segments),
                source.outline(b).map(|o| o.segments)
            );
            assert_eq!(after.advance(a), source.advance(b));
        }
        // Another face of the same names is not its source.
        assert_eq!(
            extend_subset(
                &cut.program,
                &type1_face(120),
                &[index(b"C")].into_iter().collect()
            ),
            Err(ExtendRefusal::NotTheSource)
        );
    }

    /// Programs of two formats, and bytes that are no program, are not
    /// extended.
    #[test]
    fn programs_of_two_formats_are_refused() {
        let truetype = font(10);
        let (cff, _) = cff_face(100);
        let type1 = type1_face(100);
        let none = BTreeSet::new();
        for (subset, face) in [
            (truetype.as_slice(), cff.as_slice()),
            (cff.as_slice(), type1.as_slice()),
            (type1.as_slice(), truetype.as_slice()),
            (b"not a font".as_slice(), truetype.as_slice()),
        ] {
            assert_eq!(
                extend_subset(subset, face, &none),
                Err(ExtendRefusal::Format)
            );
        }
    }
}
