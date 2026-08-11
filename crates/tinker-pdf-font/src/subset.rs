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
//! What it does not do: CFF (`OpenType/CFF` needs its own charstring index
//! rebuilt, which is a different job) and `post` glyph names are dropped
//! wholesale rather than subsetted.

use std::collections::BTreeSet;

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
/// Returns `None` when the program is not a TrueType this can rebuild — a
/// bare CFF font, or one missing `glyf`/`loca` — so a caller can embed the
/// original rather than a broken subset. Returning something is always
/// possible; returning something *smaller and correct* is not, and a subset
/// that renders wrong is worse than a font that is merely large.
#[must_use]
pub fn subset(program: &[u8], glyphs: &BTreeSet<u16>) -> Option<Vec<u8>> {
    let sfnt = Sfnt::parse(program)?;
    let glyf = sfnt.table(0x676C_7966)?;
    sfnt.table(0x6C6F_6361)?;
    let head = sfnt.table(0x6865_6164)?;
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
    close_over_components(&sfnt, &mut keep);

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
        let Some((start, end)) = glyph_range(&sfnt, glyph) else {
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
        let data = match tag {
            0x676C_7966 => new_glyf.clone(),
            0x6C6F_6361 => new_loca.clone(),
            0x6865_6164 => new_head.clone(),
            _ => match sfnt.table(tag) {
                Some(data) => data.to_vec(),
                None => continue,
            },
        };
        tables.push((tag, data));
    }
    tables.sort_by_key(|(tag, _)| *tag);

    Some(assemble(program, &tables))
}

/// Writes the table directory and the tables, with the checksums a validator
/// will look at.
fn assemble(program: &[u8], tables: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let count = tables.len() as u16;
    let mut out = Vec::new();

    // The version the original declared, so a collection's first face stays
    // whatever it was.
    let version = be32(program, 0).unwrap_or(0x0001_0000);
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
    let Some(sfnt) = Sfnt::parse(program) else {
        return out;
    };
    for c in text.chars() {
        match sfnt.glyph_for_char(c) {
            Some(0) | None => {}
            Some(glyph) => {
                out.insert(glyph);
            }
        }
    }
    out
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
        assemble(&0x0001_0000u32.to_be_bytes(), &tables)
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

    #[test]
    fn a_program_that_is_not_truetype_is_refused_rather_than_mangled() {
        assert!(subset(b"", &BTreeSet::new()).is_none());
        assert!(subset(b"not a font at all", &BTreeSet::new()).is_none());
        // A valid directory with no `glyf`: a CFF-flavoured OpenType, which
        // needs its own charstring index rebuilt and is not this.
        let stripped = assemble(
            &0x4F54_544Fu32.to_be_bytes(),
            &[(0x6865_6164, vec![0u8; 54])],
        );
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
}
