//! CFF: the tables that decide *which* charstring runs, and then the
//! charstring itself.
//!
//! Two halves, and they fail differently. The charset, the encoding, the
//! string INDEX and FDSelect are offsets and counts the file supplies, read
//! before a single outline operator executes -- a run that only asked for
//! outlines would never touch them, which is why this target asks a font what
//! its glyphs are called and which glyph a code, a name and a CID select. The
//! charstrings underneath are subroutine recursion, `seac`, `flex`, and the
//! hintmask whose length depends on how many stems were declared before it:
//! the operator most often counted wrongly.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_font::Cff;

fuzz_target!(|data: &[u8]| {
    let Some(cff) = Cff::parse(data) else {
        return;
    };
    let count = cff.glyph_count().min(64) as u16;

    for glyph in 0..count {
        let _ = cff.outline(glyph);
        let _ = cff.advance(glyph);
        // A CID-keyed font may carry a matrix per Font DICT, so this reads
        // FDSelect and the FDArray for every glyph.
        let _ = cff.font_matrix_for(glyph);

        // The charset, and the string INDEX behind it. Feeding the name back
        // in is what reaches the *resolving* half with something the font
        // actually holds: arbitrary bytes almost never name a real glyph, so
        // a target that only looked names up from the input would exercise
        // the miss path and call it coverage.
        if let Some(name) = cff.glyph_name(glyph) {
            let _ = cff.gid_for_name(name);
        }
    }

    // Every code, because the encoding is a 256-entry table however short the
    // one in the file is, and a supplement can put a glyph at any of them.
    for code in 0..=u8::MAX {
        let _ = cff.gid_for_code(code);
    }

    // The reverse charset. The CIDs tried are the ones the font declares --
    // taken from its own glyphs -- plus the edges, where `u16` runs out and
    // where a truncating cast would have found a glyph that is not there.
    for glyph in 0..count {
        let _ = cff.gid_for_cid(u32::from(glyph));
    }
    for cid in [0, 1, 0xFFFF, 0x1_0000, u32::MAX] {
        let _ = cff.gid_for_cid(cid);
    }

    // A name from the input as well, for the paths a hit never reaches: the
    // string INDEX walked to its end, and the standard strings searched in
    // full.
    if let Ok(name) = core::str::from_utf8(&data[..data.len().min(64)]) {
        let _ = cff.gid_for_name(name);
    }
    let _ = cff.is_cid();
});
