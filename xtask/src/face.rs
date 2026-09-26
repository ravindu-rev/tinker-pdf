//! A TrueType face this repository writes for itself.
//!
//! ## Why there is one at all
//!
//! The corpus's degradation rate is measured **without** font faces, because
//! this engine bundles none: a document that names Helvetica and embeds
//! nothing extracts its text perfectly and draws none of it, and every one of
//! those files counts as degraded. So the figure measures the font policy as
//! much as it measures the engine, `corpus/ratchet.json` says so in its own
//! note, and the roadmap asks for the other number.
//!
//! ## Why it is synthesised rather than fetched
//!
//! `crates/tinker-pdf/tests/substitute_fonts.rs` already builds a face on the
//! spot, and says why: "the test is identical on every platform and the
//! repository carries no font anyone has to licence". The same two reasons
//! apply to a corpus bar and a third joins them — a bar measured against
//! whatever face a runner image happened to ship is not a bar, for exactly the
//! reason `corpus-run` spawns a `tpdf` built from its own revision rather than
//! one found on `PATH`.
//!
//! ## What it can and cannot say
//!
//! Every glyph from 32 up is the same filled box. That answers **"was a face
//! available"** and nothing else: the shapes are wrong, and a bar measured
//! with this face says how much of the degradation was the absence of a face
//! rather than a defect in the engine. It cannot say that the text was set
//! correctly, and the ratchet's note says so where somebody reading the number
//! will see it.
//!
//! The face is versioned in that note, as `synthetic-2`. Changing what this
//! writes invalidates the bar rather than silently moving it — and on
//! 2026-08-31 it did: `synthetic-1`'s `head` was 54 zero bytes with two fields
//! poked into it, so the face had no `tableVersion`, no `magicNumber`, no
//! bounding box, no `maxp` and no table checksums. See [`SYNTHETIC`] for what
//! moved and what that costs. `corpus/ratchet-fonts.json` still records
//! `synthetic-1`, so the next corpus run will refuse to compare and say so.

use std::path::{Path, PathBuf};

/// The name the ratchet records for a run measured with this face.
///
/// Versioned on purpose. A different face is a different measurement, and the
/// comparator already refuses to compare two runs whose `--fonts` differ — so
/// the version has to be *in* the setting for that refusal to catch a change
/// to the face itself.
///
/// **`synthetic-2` since 2026-08-31.** `synthetic-1` wrote a `head` of 54 zero
/// bytes with two fields poked into it, so its `tableVersion` and `magicNumber`
/// were zero, its bounding box was empty, every directory checksum was zero and
/// there was no `maxp` — a face this engine happens to read and no conformant
/// tool accepts. The glyphs did not change and neither, in all likelihood, did
/// any number in `corpus/ratchet-fonts.json`; but "in all likelihood" is not a
/// measurement, which is the entire reason this constant exists. The recorded
/// bar still says `synthetic-1`, so the comparator will refuse the next run by
/// name and ask for a re-record. That refusal is the design working, not a
/// fault.
pub const SYNTHETIC: &str = "synthetic-2";

/// Where `--fonts synthetic` puts the face.
///
/// Under `target/`, because it is built rather than committed, and the same
/// path every time so a re-run does not accumulate copies.
#[must_use]
pub fn default_path(root: &Path) -> PathBuf {
    root.join(format!("target/fonts/tinker-{SYNTHETIC}.ttf"))
}

/// Writes the face, creating its directory.
pub fn write(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::write(path, bytes()).map_err(|e| format!("{}: {e}", path.display()))
}

/// The lowest character code that gets a glyph. Below 32 are the controls.
const FIRST: usize = 32;
/// The highest. A simple font's codes are bytes, so this is all of them.
const LAST: usize = 255;
/// Design units per em, and the side of the box in them.
const UNITS_PER_EM: u16 = 1000;
/// The box's side, leaving a little bearing inside the em.
const SIDE: i16 = 700;

/// The face, as bytes.
///
/// Four tables and no more: `head` for the units per em and the `loca` format,
/// `maxp` for the glyph count, `loca` for where each glyph is, and `glyf` for
/// the glyph itself.
///
/// `maxp` is not decoration and its absence was a defect. `loca` has one more
/// entry than the font has glyphs and carries no count of its own, so
/// **nothing can interpret `loca` without `maxp.numGlyphs`** — a reader has to
/// divide the table length by the entry width and hope. This engine did
/// exactly that and so did `tests/woff/make-fixtures.py`, which had to bolt a
/// `maxp` on before fontTools would open the file at all.
///
/// A `cmap` is still deliberately absent — a simple font's code *is* its glyph
/// index through this path, and a `cmap` would add a second opinion about
/// that. `hmtx` is absent for the same kind of reason: a PDF carries its own
/// `/Widths`, and a face that disagreed with them would move the text.
///
/// # Every field a conformant reader checks is set
///
/// The first version of this wrote `head` as 54 zero bytes with `unitsPerEm`
/// and `indexToLocFormat` poked into it, left every directory checksum zero,
/// and left the offset table's `searchRange` trio zero. This engine read it,
/// because this engine reads the two fields it needs and looks at nothing
/// else. Nothing stricter did: `ttf2woff` takes a WOFF's `flavor` from the
/// first four bytes of `head` and got zero, which is not an sfnt version, and
/// it validates every table checksum before it will pack a face at all.
///
/// A generator whose output only its own reader accepts is a generator that
/// tests the reader against itself, so all of it is set here — and the two
/// computed fields, `checkSumAdjustment` and the directory checksums, are
/// computed rather than zeroed.
#[must_use]
pub fn bytes() -> Vec<u8> {
    // A filled square, as one contour of four on-curve points.
    let mut glyph = Vec::new();
    glyph.extend_from_slice(&1i16.to_be_bytes()); // one contour
    glyph.extend_from_slice(&0i16.to_be_bytes()); // xMin
    glyph.extend_from_slice(&0i16.to_be_bytes()); // yMin
    glyph.extend_from_slice(&SIDE.to_be_bytes()); // xMax
    glyph.extend_from_slice(&SIDE.to_be_bytes()); // yMax
    glyph.extend_from_slice(&3u16.to_be_bytes()); // last point of contour 0
    glyph.extend_from_slice(&0u16.to_be_bytes()); // no instructions
    glyph.extend_from_slice(&[0x01, 0x01, 0x01, 0x01]); // on-curve, word deltas
    for dx in [0i16, SIDE, 0, -SIDE] {
        glyph.extend_from_slice(&dx.to_be_bytes());
    }
    for dy in [0i16, 0, SIDE, 0] {
        glyph.extend_from_slice(&dy.to_be_bytes());
    }

    // Each glyph needs its own slice of `glyf`, so the shape is repeated
    // rather than pointed at: a glyph is empty precisely when its `loca` entry
    // equals the next one, and offsets that merely repeat make every glyph
    // empty — which draws nothing and looks exactly like a face that failed to
    // load.
    let size = glyph.len() as u32;
    let mut glyf = Vec::with_capacity(glyph.len() * (LAST + 1 - FIRST));
    for _ in FIRST..=LAST {
        glyf.extend_from_slice(&glyph);
    }

    let mut loca = Vec::new();
    for index in 0..=LAST + 1 {
        let offset = (index.saturating_sub(FIRST)) as u32 * size;
        loca.extend_from_slice(&offset.to_be_bytes());
    }

    // `numGlyphs` is `loca`'s entry count less one, which is the relationship
    // that makes `loca` readable at all.
    let num_glyphs = u16::try_from(LAST + 1).unwrap_or(u16::MAX);
    let mut maxp = vec![0u8; 32];
    maxp[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes()); // version 1.0
    maxp[4..6].copy_from_slice(&num_glyphs.to_be_bytes());
    maxp[6..8].copy_from_slice(&4u16.to_be_bytes()); // maxPoints
    maxp[8..10].copy_from_slice(&1u16.to_be_bytes()); // maxContours

    // The offsets are the specification's. `checkSumAdjustment` at 8 is left
    // zero here and written once the whole font is laid out, because it is a
    // function of every byte including the directory entry that describes this
    // table.
    let mut head = vec![0u8; 54];
    head[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes()); // version 1.0
    head[4..8].copy_from_slice(&0x0001_0000u32.to_be_bytes()); // fontRevision
    head[12..16].copy_from_slice(&0x5F0F_3CF5u32.to_be_bytes()); // magicNumber
    head[16..18].copy_from_slice(&0x0003u16.to_be_bytes()); // baseline at y=0
    head[18..20].copy_from_slice(&UNITS_PER_EM.to_be_bytes());
    // `created` and `modified` stay zero: a clock here would make two runs on
    // one machine disagree, and 1904-01-01 is a real date rather than a
    // missing field.
    head[36..38].copy_from_slice(&0i16.to_be_bytes()); // xMin
    head[38..40].copy_from_slice(&0i16.to_be_bytes()); // yMin
    head[40..42].copy_from_slice(&SIDE.to_be_bytes()); // xMax
    head[42..44].copy_from_slice(&SIDE.to_be_bytes()); // yMax
    head[46..48].copy_from_slice(&8u16.to_be_bytes()); // lowestRecPPEM
    head[48..50].copy_from_slice(&2i16.to_be_bytes()); // fontDirectionHint
    head[50..52].copy_from_slice(&1i16.to_be_bytes()); // long loca

    // Ascending tag order, which the sfnt directory requires and which happens
    // to be the order these four were written in.
    let tables: [(&[u8; 4], &[u8]); 4] = [
        (b"glyf", &glyf),
        (b"head", &head),
        (b"loca", &loca),
        (b"maxp", &maxp),
    ];

    let count = tables.len();
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&(count as u16).to_be_bytes());
    // `searchRange`, `entrySelector` and `rangeShift` are a function of the
    // table count alone, and a reader doing a binary search over the directory
    // uses them. Zero told such a reader the directory was empty.
    let mut entry_selector = 0u16;
    while (1usize << (entry_selector + 1)) <= count {
        entry_selector += 1;
    }
    let search_range = 16u16 << entry_selector;
    out.extend_from_slice(&search_range.to_be_bytes());
    out.extend_from_slice(&entry_selector.to_be_bytes());
    out.extend_from_slice(&((count as u16) * 16 - search_range).to_be_bytes());

    let mut offset = 12 + count * 16;
    let mut body = Vec::new();
    for (tag, data) in tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&checksum(data).to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        offset += data.len();
        body.extend_from_slice(data);
        // Every table starts on a four-byte boundary, which is what makes the
        // zero-padded checksum below the same arithmetic a reader will do.
        while body.len() % 4 != 0 {
            body.push(0);
            offset += 1;
        }
    }
    out.extend_from_slice(&body);

    // `checkSumAdjustment`: 0xB1B0AFBA less the sum of the whole file with the
    // field itself zero. It is already zero, so the sum can be taken as it
    // stands. `head` is the second table, after the directory and `glyf`.
    let head_at = 12 + count * 16 + glyf.len();
    let adjustment = 0xB1B0_AFBAu32.wrapping_sub(checksum(&out));
    out[head_at + 8..head_at + 12].copy_from_slice(&adjustment.to_be_bytes());
    out
}

/// An sfnt checksum: the sum of the big-endian 32-bit words, wrapping, with a
/// final partial word zero-padded.
fn checksum(data: &[u8]) -> u32 {
    let mut sum = 0u32;
    for chunk in data.chunks(4) {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum = sum.wrapping_add(u32::from_be_bytes(word));
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The face is the same bytes every time.
    ///
    /// Not decoration: the bar recorded against it is only a bar if the thing
    /// it was recorded against does not drift. A clock, a hash-map iteration
    /// or an environment lookup in here would make two runs on one machine
    /// disagree, and the ratchet would report it as an engine regression.
    #[test]
    fn the_face_is_the_same_bytes_every_time() {
        assert_eq!(bytes(), bytes());
        assert!(bytes().len() > 4096, "it has glyphs in it");
    }

    /// A TrueType face, by its own header.
    #[test]
    fn it_is_a_truetype_face_with_the_four_tables() {
        let face = bytes();
        assert_eq!(&face[0..4], &0x0001_0000u32.to_be_bytes(), "sfnt version");
        assert_eq!(u16::from_be_bytes([face[4], face[5]]), 4, "four tables");
        for tag in [b"head", b"loca", b"glyf", b"maxp"] {
            assert!(
                face.windows(4).any(|w| w == tag),
                "{} is in the table directory",
                String::from_utf8_lossy(tag)
            );
        }
    }

    /// **Every field a conformant reader checks is set**, which `synthetic-1`
    /// could not say.
    ///
    /// Asserted field by field rather than against a golden blob, because the
    /// point is which fields exist and not what the file happens to hash to.
    /// Each of these was zero before, and `ttf2woff` — which reads a WOFF's
    /// `flavor` out of the first four bytes of `head`, and validates every
    /// table checksum — is the tool that found it.
    #[test]
    fn the_head_table_and_the_directory_are_filled_in() {
        let face = bytes();
        let mut found = None;
        let count = usize::from(u16::from_be_bytes([face[4], face[5]]));
        for i in 0..count {
            let at = 12 + i * 16;
            let tag = &face[at..at + 4];
            let sum = u32::from_be_bytes([face[at + 4], face[at + 5], face[at + 6], face[at + 7]]);
            let offset =
                u32::from_be_bytes([face[at + 8], face[at + 9], face[at + 10], face[at + 11]])
                    as usize;
            let length =
                u32::from_be_bytes([face[at + 12], face[at + 13], face[at + 14], face[at + 15]])
                    as usize;

            // No directory entry has a zero checksum, and each one is the sum
            // of the bytes it points at.
            assert_ne!(
                sum,
                0,
                "{} has a zero checksum",
                String::from_utf8_lossy(tag)
            );
            let mut expected = checksum(&face[offset..offset + length]);
            if tag == b"head" {
                // `head`'s directory checksum is defined with
                // `checkSumAdjustment` taken as zero, so recompute it that way.
                let mut zeroed = face[offset..offset + length].to_vec();
                zeroed[8..12].fill(0);
                expected = checksum(&zeroed);
            }
            assert_eq!(sum, expected, "{}", String::from_utf8_lossy(tag));
            if tag == b"head" {
                found = Some(offset);
            }
        }

        // The offset table's binary-search trio, which was zero.
        assert_eq!(
            u16::from_be_bytes([face[6], face[7]]),
            64,
            "searchRange for four tables"
        );
        assert_eq!(u16::from_be_bytes([face[8], face[9]]), 2, "entrySelector");
        assert_eq!(u16::from_be_bytes([face[10], face[11]]), 0, "rangeShift");

        let head = found.expect("a head table");
        let field =
            |at: usize| u32::from_be_bytes(face[head + at..head + at + 4].try_into().unwrap());
        assert_eq!(field(0), 0x0001_0000, "head.tableVersion");
        assert_eq!(field(12), 0x5F0F_3CF5, "head.magicNumber");
        assert_ne!(field(8), 0, "head.checkSumAdjustment was computed");

        // The whole-font sum with the adjustment zeroed is what the field
        // completes to 0xB1B0AFBA, which is the check a validator makes.
        let mut zeroed = face.clone();
        zeroed[head + 8..head + 12].fill(0);
        assert_eq!(
            checksum(&zeroed).wrapping_add(field(8)),
            0xB1B0_AFBA,
            "the font does not check out against itself"
        );

        // The bounding box, which was empty and is now the square's.
        let bbox: Vec<i16> = (36..44)
            .step_by(2)
            .map(|at| i16::from_be_bytes([face[head + at], face[head + at + 1]]))
            .collect();
        assert_eq!(bbox, vec![0, 0, SIDE, SIDE], "head's bounding box");

        // And `maxp` agrees with `loca`, which is the relationship that makes
        // `loca` readable at all.
        let maxp_at = (0..count)
            .map(|i| 12 + i * 16)
            .find(|at| &face[*at..*at + 4] == b"maxp")
            .expect("a maxp table");
        let maxp_off =
            u32::from_be_bytes(face[maxp_at + 8..maxp_at + 12].try_into().unwrap()) as usize;
        let num_glyphs = u16::from_be_bytes([face[maxp_off + 4], face[maxp_off + 5]]);
        let loca_at = (0..count)
            .map(|i| 12 + i * 16)
            .find(|at| &face[*at..*at + 4] == b"loca")
            .expect("a loca table");
        let loca_len =
            u32::from_be_bytes(face[loca_at + 12..loca_at + 16].try_into().unwrap()) as usize;
        assert_eq!(
            usize::from(num_glyphs) + 1,
            loca_len / 4,
            "maxp.numGlyphs and loca disagree"
        );
    }
}
