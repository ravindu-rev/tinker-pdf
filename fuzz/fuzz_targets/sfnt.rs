//! The sfnt container itself: the table directory, the tables plan 05's first
//! milestone names, and the subsetter that rebuilds a directory from one.
//!
//! `truetype` drives the outlines in `glyf`; this drives everything around
//! them, because a font whose directory lies about where a table is never
//! reaches an outline at all.
#![no_main]
use libfuzzer_sys::fuzz_target;

use std::collections::BTreeSet;
use tinker_pdf_font::{glyphs_for, subset, Sfnt};

fuzz_target!(|data: &[u8]| {
    let Some(font) = Sfnt::parse(data) else {
        return;
    };

    // Every offset and length in the directory is attacker controlled, and
    // `table` is the one place that has to disbelieve all of them.
    for tag in [
        b"head", b"hhea", b"hmtx", b"maxp", b"cmap", b"post", b"name", b"OS/2", b"loca", b"CFF ",
    ] {
        let _ = font.table(u32::from_be_bytes(*tag));
    }
    let _ = font.units_per_em;

    // `cmap` in each of the formats read, reached through the characters that
    // land in different segments of format 4 and past the BMP for format 12.
    for c in [
        '\0',
        ' ',
        'A',
        '\u{7F}',
        '\u{FFFD}',
        '\u{20AC}',
        '\u{1F600}',
    ] {
        let _ = font.glyph_for_char(c);
    }
    // `post` 2.0 names, including the two spellings a lookup has to reject.
    for name in ["A", ".notdef", "uni20AC", "", "\u{FFFD}"] {
        let _ = font.glyph_for_name(name);
    }
    // `hmtx` past `numberOfHMetrics`, where the last advance repeats.
    for glyph in 0..64u16 {
        let _ = font.advance(glyph);
    }

    // The subsetter reads the directory and writes a new one, so a hostile
    // input reaches an assembler as well as a parser. The glyph set is kept
    // small on purpose: the interesting failures are in the table surgery,
    // not in how many glyphs it copies.
    let wanted = glyphs_for(data, "Aa0 \u{20AC}");
    let _ = subset(data, &wanted.iter().copied().take(16).collect());
    let _ = subset(data, &BTreeSet::from([0u16, 1, 2, 3]));
    let _ = subset(data, &BTreeSet::new());
});
