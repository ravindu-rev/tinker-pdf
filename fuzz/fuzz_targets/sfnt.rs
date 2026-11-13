//! The sfnt container itself: the table directory, the tables plan 05's first
//! milestone names, and the subsetter that rebuilds a directory from one.
//!
//! `truetype` drives the outlines in `glyf`; this drives everything around
//! them, because a font whose directory lies about where a table is never
//! reaches an outline at all.
//!
//! # What this target checks, and what it does not
//!
//! **Only that the code did not panic, hang, or exhaust memory.** Every call
//! is discarded — the subsetter's and the colour tables' readers' included,
//! so a subset font that rebuilt its directory *wrongly* passes. So a run that
//! returned the *wrong* answer passes this target exactly as a correct one
//! does, and a green `cargo fuzz` here is evidence about ruling 1 and about
//! nothing else.
//!
//! That is worth writing down rather than leaving implied. Correctness lives
//! in `crates/tinker-pdf-font`'s own tests — the colour tables' in
//! `tests/colour_tables.rs` — and in the subset census, which checks that a
//! rebuilt face is one a conformant consumer accepts.
//!
//! Recorded because the same shape has already cost this repository once: the
//! `brotli` target asserts only self-consistency and could not have found the
//! ring-buffer defect that a decoded-bytes comparison found immediately.
#![no_main]
use libfuzzer_sys::fuzz_target;

use std::collections::BTreeSet;
use tinker_pdf_font::{
    glyf, glyphs_for, subset, substitute_glyph, BuiltInEncoding, ColourBitmaps, Colr, Cpal,
    PaletteChoice, Sbix, Sfnt, StrikeChoice,
};

fuzz_target!(|data: &[u8]| {
    let Some(font) = Sfnt::parse(data) else {
        return;
    };

    // Every offset and length in the directory is attacker controlled, and
    // `table` is the one place that has to disbelieve all of them.
    for tag in [
        b"head", b"hhea", b"hmtx", b"maxp", b"cmap", b"post", b"name", b"OS/2", b"loca", b"CFF ",
        b"vhea", b"vmtx", b"VORG",
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
    // The route a substituted Symbol or ZapfDingbats font is drawn by: the
    // face's names, its Unicode subtable and its (3,0) subtable, each read on
    // its own (`docs/design/symbol-faces.md`).
    for c in ['\u{03B1}', '\u{2701}', '\u{F8E5}', '\u{1F600}'] {
        let _ = font.glyph_for_unicode(c);
    }
    for code in [0x20u8, 0x61, 0xFE] {
        let _ = font.glyph_for_symbol_code(code);
    }
    for (encoding, name, code) in [
        (BuiltInEncoding::Symbol, Some("alpha"), 0x61u8),
        (BuiltInEncoding::ZapfDingbats, Some("a1"), 0x21),
        (BuiltInEncoding::Symbol, None, 0x20),
    ] {
        let _ = substitute_glyph(data, encoding, name, code);
    }
    // `hmtx` past `numberOfHMetrics`, where the last advance repeats, and the
    // vertical metrics the same way: `vmtx` past `numOfLongVerMetrics`, the
    // side bearings after it, `VORG`'s records bisected or scanned, and the
    // vertical origin, which reaches `glyf`'s header or a CFF charstring.
    for glyph in (0..64u16).chain([u16::MAX - 1, u16::MAX]) {
        let _ = font.advance(glyph);
        let _ = font.vertical_advance(glyph);
        let _ = font.top_side_bearing(glyph);
        let _ = font.vorg_origin(glyph);
        let _ = font.vertical_origin(glyph);
        let _ = glyf::bounds(&font, glyph);
    }

    // The colour tables (`docs/design/colour-fonts.md`), for the face's first
    // glyphs as `cmap` is walked for its first characters: every palette
    // entry a short palette can name, each choice of palette, and each glyph's
    // layers looked up in the palette chosen.
    let cpal = font
        .table(u32::from_be_bytes(*b"CPAL"))
        .and_then(Cpal::parse);
    if let Some(cpal) = &cpal {
        for palette in 0..4u16 {
            let _ = cpal.palette_type(palette);
            let _ = cpal.palette_label(palette);
            for entry in 0..16u16 {
                let _ = cpal.colour(palette, entry);
            }
        }
        for entry in 0..16u16 {
            let _ = cpal.entry_label(entry);
        }
    }
    if let Some(colr) = font
        .table(u32::from_be_bytes(*b"COLR"))
        .and_then(Colr::parse)
    {
        let _ = colr.warnings();
        for glyph in 0..64u16 {
            // Version 1's graph, every node and child id it hands back, and
            // the clip box of the glyph's range.
            if let Some(graph) = colr.paint_graph(glyph) {
                for node in graph.nodes() {
                    for child in node.children() {
                        let _ = graph.paint(child);
                    }
                    let _ = node.format();
                }
                let _ = graph.warnings();
            }
            let _ = colr.clip_box(glyph);
            let _ = colr.layer_records(glyph);
            if let Some(cpal) = &cpal {
                for choice in [
                    PaletteChoice::Default,
                    PaletteChoice::LightBackground,
                    PaletteChoice::DarkBackground,
                ] {
                    let _ = colr.layers(glyph, cpal, cpal.choose(choice));
                }
            }
        }
    }

    // `sbix`, sized by `maxp`: every glyph's bitmap in the first strikes, a
    // `dupe` resolved, and each strike option's choice among the strikes that
    // hold the glyph.
    if let (Some(table), Some(count)) =
        (font.table(u32::from_be_bytes(*b"sbix")), font.glyph_count())
    {
        if let Some(sbix) = Sbix::parse(table, count) {
            let _ = sbix.draws_outlines();
            let strikes = sbix.strikes().len().min(8);
            for glyph in 0..64u16 {
                for strike in 0..strikes {
                    let _ = sbix.glyph(strike, glyph);
                }
                for choice in [
                    StrikeChoice::Largest,
                    StrikeChoice::AtLeast(32),
                    StrikeChoice::Multiple(16),
                    StrikeChoice::Nearest(48),
                ] {
                    let _ = sbix.choose_strike(glyph, choice);
                }
            }
        }
    }

    // `CBLC` and `CBDT`: every glyph's PNG in the first strikes, through
    // whichever index subtable holds it, and each strike option's choice.
    if let (Some(cblc), Some(cbdt)) = (
        font.table(u32::from_be_bytes(*b"CBLC")),
        font.table(u32::from_be_bytes(*b"CBDT")),
    ) {
        if let Some(bitmaps) = ColourBitmaps::parse(cblc, cbdt) {
            let strikes = bitmaps.strikes().len().min(8);
            for glyph in 0..64u16 {
                for strike in 0..strikes {
                    let _ = bitmaps.glyph(strike, glyph);
                }
                for choice in [StrikeChoice::Largest, StrikeChoice::Nearest(48)] {
                    let _ = bitmaps.choose_strike(glyph, choice);
                }
            }
        }
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
