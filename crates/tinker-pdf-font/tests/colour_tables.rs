//! The colour tables, read from bytes the tests assemble field by field
//! (`docs/design/colour-fonts.md`).
//!
//! Every table here is written from the OpenType 1.9.1 chapter's own layout —
//! the CPAL, COLR, sbix, CBLC and CBDT chapters — by a builder in this file,
//! so each assertion compares what the reader hands back with the bytes the
//! test put there, and none with another program's reading (ruling 13). The
//! builders also write the `fuzz/corpus/sfnt` seeds the `sfnt` target starts
//! from (`write_the_fuzz_seeds`), so the seeds and the fixtures cannot drift.

use tinker_pdf_font::colr::{
    ClipBox, ColourLine, ColourStop, ColrRefusal, ColrWarning, CompositeMode, Extend, Paint,
    PaintId, FOREGROUND, MAX_PAINT_DEPTH, MAX_PAINT_NODES,
};
use tinker_pdf_font::{
    ColourIndex, Colr, Cpal, Layer, LayerColour, PaletteChoice, PaletteType, Rgba, Sfnt,
};

/// A colour, written as the reader hands it back.
const fn rgba(red: u8, green: u8, blue: u8, alpha: u8) -> Rgba {
    Rgba {
        red,
        green,
        blue,
        alpha,
    }
}

/// A `CPAL` table's contents, laid out by [`CpalTable::build`] as *Palette
/// table header* and *Palette entries and color records* lay them out.
#[derive(Clone)]
struct CpalTable {
    version: u16,
    /// `numPaletteEntries`.
    entries: u16,
    /// `colorRecordIndices`: each palette's first record.
    firsts: Vec<u16>,
    /// The colour records, in the reader's component order.
    records: Vec<Rgba>,
    /// Version 1's arrays, each written where given and its offset zero where
    /// not.
    types: Option<Vec<u32>>,
    labels: Option<Vec<u16>>,
    entry_labels: Option<Vec<u16>>,
}

impl CpalTable {
    fn build(&self) -> Vec<u8> {
        let palettes = self.firsts.len();
        let header = 12 + 2 * palettes + if self.version >= 1 { 12 } else { 0 };
        let mut out = Vec::new();
        out.extend_from_slice(&self.version.to_be_bytes());
        out.extend_from_slice(&self.entries.to_be_bytes());
        out.extend_from_slice(&(palettes as u16).to_be_bytes());
        out.extend_from_slice(&(self.records.len() as u16).to_be_bytes());
        out.extend_from_slice(&(header as u32).to_be_bytes());
        for first in &self.firsts {
            out.extend_from_slice(&first.to_be_bytes());
        }
        let mut arrays: Vec<u8> = Vec::new();
        let mut after = header + 4 * self.records.len();
        if self.version >= 1 {
            let mut place = |bytes: Option<Vec<u8>>, out: &mut Vec<u8>| match bytes {
                Some(bytes) => {
                    out.extend_from_slice(&(after as u32).to_be_bytes());
                    after += bytes.len();
                    arrays.extend_from_slice(&bytes);
                }
                None => out.extend_from_slice(&0u32.to_be_bytes()),
            };
            let words = |values: &Option<Vec<u16>>| {
                values
                    .as_ref()
                    .map(|v| v.iter().flat_map(|x| x.to_be_bytes()).collect::<Vec<u8>>())
            };
            let types = self
                .types
                .as_ref()
                .map(|v| v.iter().flat_map(|x| x.to_be_bytes()).collect::<Vec<u8>>());
            place(types, &mut out);
            place(words(&self.labels), &mut out);
            place(words(&self.entry_labels), &mut out);
        }
        for colour in &self.records {
            out.extend_from_slice(&[colour.blue, colour.green, colour.red, colour.alpha]);
        }
        out.extend_from_slice(&arrays);
        out
    }
}

/// Two palettes of three entries, palette 1 starting where palette 0 ends.
fn two_palettes() -> CpalTable {
    CpalTable {
        version: 0,
        entries: 3,
        firsts: vec![0, 3],
        records: vec![
            rgba(255, 0, 0, 255),
            rgba(0, 128, 0, 255),
            rgba(0, 0, 255, 128),
            rgba(10, 20, 30, 255),
            rgba(40, 50, 60, 200),
            rgba(70, 80, 90, 0),
        ],
        types: None,
        labels: None,
        entry_labels: None,
    }
}

/// A version 0 `COLR` table, or the version 0 half of a version 1 header with
/// its other offsets zero: the `BaseGlyph` records (glyph, first layer, layer
/// count) as given, then the layer records (glyph, palette index).
fn colr_v0(version: u16, base: &[(u16, u16, u16)], layers: &[(u16, u16)]) -> Vec<u8> {
    let header = if version == 1 { 34 } else { 14 };
    let base_at = header;
    let layers_at = base_at + 6 * base.len();
    let mut out = Vec::new();
    out.extend_from_slice(&version.to_be_bytes());
    out.extend_from_slice(&(base.len() as u16).to_be_bytes());
    out.extend_from_slice(&(base_at as u32).to_be_bytes());
    out.extend_from_slice(&(layers_at as u32).to_be_bytes());
    out.extend_from_slice(&(layers.len() as u16).to_be_bytes());
    if version == 1 {
        out.extend_from_slice(&[0; 20]);
    }
    for (glyph, first, count) in base {
        out.extend_from_slice(&glyph.to_be_bytes());
        out.extend_from_slice(&first.to_be_bytes());
        out.extend_from_slice(&count.to_be_bytes());
    }
    for (glyph, palette) in layers {
        out.extend_from_slice(&glyph.to_be_bytes());
        out.extend_from_slice(&palette.to_be_bytes());
    }
    out
}

/// Glyph 1 is three layers, glyph 2 one foreground layer, and glyph 3 the
/// last two of glyph 1's, a slice two base glyphs share.
fn three_glyphs() -> Vec<u8> {
    colr_v0(
        0,
        &[(1, 0, 3), (2, 3, 1), (3, 1, 2)],
        &[(10, 0), (11, 2), (12, 1), (13, FOREGROUND)],
    )
}

#[test]
fn colr_v0_layers_come_in_order_with_their_palette_colours() {
    let cpal_bytes = two_palettes().build();
    let cpal = Cpal::parse(&cpal_bytes).expect("a CPAL table");
    let colr_bytes = three_glyphs();
    let colr = Colr::parse(&colr_bytes).expect("a COLR table");
    assert_eq!(colr.version(), 0);
    assert!(colr.warnings().is_empty());

    let colour = |c: Rgba| LayerColour::Colour(c);
    assert_eq!(
        colr.layers(1, &cpal, 0),
        Some(vec![
            Layer {
                glyph: 10,
                colour: colour(rgba(255, 0, 0, 255))
            },
            Layer {
                glyph: 11,
                colour: colour(rgba(0, 0, 255, 128))
            },
            Layer {
                glyph: 12,
                colour: colour(rgba(0, 128, 0, 255))
            },
        ]),
        "bottom first, each entry of palette 0, its own alpha kept"
    );
    assert_eq!(
        colr.layers(1, &cpal, 1),
        Some(vec![
            Layer {
                glyph: 10,
                colour: colour(rgba(10, 20, 30, 255))
            },
            Layer {
                glyph: 11,
                colour: colour(rgba(70, 80, 90, 0))
            },
            Layer {
                glyph: 12,
                colour: colour(rgba(40, 50, 60, 200))
            },
        ]),
        "the same entries of palette 1"
    );
    assert_eq!(
        colr.layers(3, &cpal, 0),
        Some(vec![
            Layer {
                glyph: 11,
                colour: colour(rgba(0, 0, 255, 128))
            },
            Layer {
                glyph: 12,
                colour: colour(rgba(0, 128, 0, 255))
            },
        ]),
        "a slice two base glyphs share"
    );
    assert_eq!(
        colr.layer_records(1),
        Some(vec![
            (10, ColourIndex::Palette(0)),
            (11, ColourIndex::Palette(2)),
            (12, ColourIndex::Palette(1)),
        ])
    );
    assert!(colr.has_layers(2) && !colr.has_layers(0) && !colr.has_layers(4));
    assert_eq!(colr.layers(0, &cpal, 0), None, "no record, no colour glyph");
    assert_eq!(colr.layers(4, &cpal, 0), None);

    // The same records out of order are scanned, the first for a glyph
    // answering, and the table says so.
    let shuffled = colr_v0(
        0,
        &[(3, 1, 2), (1, 0, 3), (2, 3, 1), (1, 3, 1)],
        &[(10, 0), (11, 2), (12, 1), (13, FOREGROUND)],
    );
    let colr = Colr::parse(&shuffled).expect("a COLR table");
    assert_eq!(colr.warnings(), &[ColrWarning::BaseGlyphsOutOfOrder]);
    assert_eq!(
        colr.layer_records(1).map(|layers| layers.len()),
        Some(3),
        "the first record for glyph 1, not the second"
    );
    assert_eq!(colr.layer_records(3).map(|l| l.len()), Some(2));

    // A version 1 table's version 0 arrays read the same.
    let v1 = colr_v0(
        1,
        &[(1, 0, 3), (2, 3, 1), (3, 1, 2)],
        &[(10, 0), (11, 2), (12, 1), (13, FOREGROUND)],
    );
    let colr = Colr::parse(&v1).expect("a version 1 header");
    assert_eq!(colr.version(), 1);
    assert_eq!(colr.layer_records(3).map(|l| l.len()), Some(2));
    assert_eq!(
        colr.layer_records(2),
        Some(vec![(13, ColourIndex::Foreground)])
    );

    // A record of no layers is an empty glyph, as written.
    let empty = colr_v0(0, &[(5, 0, 0)], &[(10, 0)]);
    let colr = Colr::parse(&empty).expect("a COLR table");
    assert_eq!(colr.layers(5, &cpal, 0), Some(Vec::new()));
}

#[test]
fn cpal_v1_types_and_labels_read_as_written() {
    let table = CpalTable {
        version: 1,
        entries: 2,
        firsts: vec![0, 2, 4],
        records: vec![
            rgba(1, 2, 3, 4),
            rgba(5, 6, 7, 8),
            rgba(9, 10, 11, 12),
            rgba(13, 14, 15, 16),
            rgba(17, 18, 19, 20),
            rgba(21, 22, 23, 24),
        ],
        types: Some(vec![0, PaletteType::DARK_BACKGROUND, 0x0003 | 0x8000]),
        labels: Some(vec![256, 0xFFFF, 258]),
        entry_labels: Some(vec![0xFFFF, 300]),
    };
    let bytes = table.build();
    let cpal = Cpal::parse(&bytes).expect("a version 1 CPAL table");
    assert_eq!(cpal.version(), 1);
    assert_eq!((cpal.palette_count(), cpal.entry_count()), (3, 2));
    assert_eq!(cpal.colour(2, 1), Some(rgba(21, 22, 23, 24)));
    assert_eq!(cpal.colour(1, 0), Some(rgba(9, 10, 11, 12)));

    assert_eq!(cpal.palette_type(0), Some(PaletteType(0)));
    assert_eq!(cpal.palette_type(1), Some(PaletteType(2)));
    assert_eq!(
        cpal.palette_type(2),
        Some(PaletteType(0x8003)),
        "a reserved bit is kept, not dropped"
    );
    let both = cpal.palette_type(2).expect("a type");
    assert!(both.usable_with_light_background() && both.usable_with_dark_background());
    let dark = cpal.palette_type(1).expect("a type");
    assert!(dark.usable_with_dark_background() && !dark.usable_with_light_background());
    assert_eq!(cpal.palette_type(3), None, "past the palettes");

    assert_eq!(cpal.palette_label(0), Some(256));
    assert_eq!(cpal.palette_label(1), None, "0xFFFF is no string");
    assert_eq!(cpal.palette_label(2), Some(258));
    assert_eq!(cpal.entry_label(0), None);
    assert_eq!(cpal.entry_label(1), Some(300));
    assert_eq!(cpal.entry_label(2), None, "past the entries");

    assert_eq!(cpal.choose(PaletteChoice::Default), 0);
    assert_eq!(cpal.choose(PaletteChoice::Index(2)), 2);
    assert_eq!(cpal.choose(PaletteChoice::Index(3)), 0, "past the palettes");
    assert_eq!(
        cpal.choose(PaletteChoice::DarkBackground),
        1,
        "the first palette flagged for a dark background"
    );
    assert_eq!(cpal.choose(PaletteChoice::LightBackground), 2);

    // Version 1 with every array's offset zero has none of them, and a
    // choice by background falls to palette 0.
    let bare = CpalTable {
        types: None,
        labels: None,
        entry_labels: None,
        ..table.clone()
    };
    let bytes = bare.build();
    let cpal = Cpal::parse(&bytes).expect("a version 1 CPAL table");
    assert_eq!(cpal.palette_type(1), None);
    assert_eq!(cpal.palette_label(0), None);
    assert_eq!(cpal.entry_label(1), None);
    assert_eq!(cpal.choose(PaletteChoice::DarkBackground), 0);
    assert_eq!(cpal.colour(2, 1), Some(rgba(21, 22, 23, 24)));

    // Version 0 has no arrays at all.
    let bytes = two_palettes().build();
    let cpal = Cpal::parse(&bytes).expect("a version 0 CPAL table");
    assert_eq!(cpal.version(), 0);
    assert_eq!(cpal.palette_type(0), None);
    assert_eq!(cpal.choose(PaletteChoice::LightBackground), 0);
}

#[test]
fn the_foreground_index_is_the_text_colour() {
    assert_eq!(ColourIndex::from_raw(0xFFFF), ColourIndex::Foreground);
    assert_eq!(ColourIndex::from_raw(0xFFFE), ColourIndex::Palette(0xFFFE));
    assert_eq!(ColourIndex::from_raw(0), ColourIndex::Palette(0));

    let colr_bytes = three_glyphs();
    let colr = Colr::parse(&colr_bytes).expect("a COLR table");
    let cpal_bytes = two_palettes().build();
    let cpal = Cpal::parse(&cpal_bytes).expect("a CPAL table");
    for palette in 0..2 {
        assert_eq!(
            colr.layers(2, &cpal, palette),
            Some(vec![Layer {
                glyph: 13,
                colour: LayerColour::Foreground
            }]),
            "0xFFFF is the text's colour in palette {palette}, never an entry"
        );
    }

    // Even where the palette has an entry 0xFFFF would index: it is still the
    // foreground.
    let mut wide = two_palettes();
    wide.entries = 0xFFFF;
    wide.firsts = vec![0];
    wide.records = vec![rgba(1, 1, 1, 255); 4];
    let bytes = wide.build();
    let cpal = Cpal::parse(&bytes).expect("a CPAL table");
    assert_eq!(
        colr.layers(2, &cpal, 0),
        Some(vec![Layer {
            glyph: 13,
            colour: LayerColour::Foreground
        }])
    );
    assert_eq!(cpal.colour(0, 0xFFFE), None, "past the records it holds");
}

#[test]
fn out_of_range_indices_and_overrunning_counts_answer_none() {
    let cpal_bytes = two_palettes().build();
    let cpal = Cpal::parse(&cpal_bytes).expect("a CPAL table");
    assert_eq!(cpal.colour(2, 0), None, "a palette past numPalettes");
    assert_eq!(cpal.colour(0, 3), None, "an entry past numPaletteEntries");

    // A palette whose first record leaves too few records answers for the
    // entries it has.
    let mut short = two_palettes();
    short.firsts = vec![0, 4];
    let bytes = short.build();
    let cpal = Cpal::parse(&bytes).expect("a CPAL table");
    assert_eq!(cpal.colour(1, 1), Some(rgba(70, 80, 90, 0)));
    assert_eq!(cpal.colour(1, 2), None, "index 6 is past numColorRecords");

    // Records the count claims and the table does not hold.
    let mut truncated = two_palettes().build();
    truncated.truncate(truncated.len() - 6);
    let cpal = Cpal::parse(&truncated).expect("the header is whole");
    assert_eq!(cpal.colour(0, 0), Some(rgba(255, 0, 0, 255)));
    assert_eq!(
        cpal.colour(1, 1),
        None,
        "the fifth record runs past the end"
    );
    assert_eq!(cpal.colour(1, 2), None);

    // A header that does not fit is no table: the first indices, version 1's
    // offsets, and a version this reader has not read.
    let whole = two_palettes().build();
    assert!(Cpal::parse(&whole[..13]).is_none());
    assert!(Cpal::parse(&[]).is_none());
    let mut v1 = two_palettes();
    v1.version = 1;
    let v1 = v1.build();
    assert!(Cpal::parse(&v1[..20]).is_none(), "version 1's offsets cut");
    let mut v2 = two_palettes().build();
    v2[1] = 2;
    assert!(Cpal::parse(&v2).is_none());

    // A types array that runs past the table is read as absent.
    let mut overrun = CpalTable {
        version: 1,
        types: Some(vec![1, 2]),
        ..two_palettes()
    }
    .build();
    let at = 12 + 2 * 2;
    overrun[at..at + 4].copy_from_slice(&0xFFFF_FFF0u32.to_be_bytes());
    let cpal = Cpal::parse(&overrun).expect("a CPAL table");
    assert_eq!(cpal.palette_type(0), None);
    assert_eq!(cpal.choose(PaletteChoice::LightBackground), 0);

    let cpal_bytes = two_palettes().build();
    let cpal = Cpal::parse(&cpal_bytes).expect("a CPAL table");

    // A layer naming an entry the palette lacks: no colour glyph.
    let bad_entry = colr_v0(0, &[(1, 0, 2)], &[(10, 0), (11, 3)]);
    let colr = Colr::parse(&bad_entry).expect("a COLR table");
    assert_eq!(colr.layers(1, &cpal, 0), None);
    assert!(
        colr.layer_records(1).is_some(),
        "the records themselves read"
    );

    // A slice past numLayerRecords.
    let past = colr_v0(0, &[(1, 2, 3)], &[(10, 0), (11, 1), (12, 2)]);
    let colr = Colr::parse(&past).expect("a COLR table");
    assert_eq!(colr.layer_records(1), None);
    assert_eq!(colr.layers(1, &cpal, 0), None);

    // Layer records the count claims and the table does not hold.
    let mut cut = colr_v0(0, &[(1, 0, 3)], &[(10, 0), (11, 1), (12, 2)]);
    cut.truncate(cut.len() - 2);
    let colr = Colr::parse(&cut).expect("the header is whole");
    assert_eq!(colr.layer_records(1), None);

    // A BaseGlyph array that runs past the table: every glyph answers None.
    let mut no_base = three_glyphs();
    no_base[2..4].copy_from_slice(&500u16.to_be_bytes());
    let colr = Colr::parse(&no_base).expect("the header is whole");
    assert!(!colr.has_layers(1));
    assert_eq!(colr.layers(1, &cpal, 0), None);

    // A header that does not fit, and a version this reader has not read.
    let whole = three_glyphs();
    assert!(Colr::parse(&whole[..13]).is_none());
    let v1 = colr_v0(1, &[(1, 0, 1)], &[(10, 0)]);
    assert!(Colr::parse(&v1[..30]).is_none(), "a version 1 header cut");
    let mut v2 = three_glyphs();
    v2[1] = 2;
    assert!(Colr::parse(&v2).is_none());

    // A palette index the chosen palette is not: `layers` asks the palette
    // it is given, and palette 2 is not this table's.
    let colr = Colr::parse(&whole).expect("a COLR table");
    assert_eq!(colr.layers(1, &cpal, 2), None);
}

/// Every prefix of each assembled table, and the table with each byte
/// flipped in turn, reads to an answer or a refusal and never a panic
/// (ruling 1); the lookups walk every glyph and entry the counts could name.
#[test]
fn every_prefix_and_flip_of_cpal_and_colr_v0_answers_without_panicking() {
    let tables = [
        two_palettes().build(),
        CpalTable {
            version: 1,
            types: Some(vec![1, 2]),
            labels: Some(vec![256, 257]),
            entry_labels: Some(vec![1, 2, 3]),
            ..two_palettes()
        }
        .build(),
        three_glyphs(),
        colr_v0(1, &[(1, 0, 3)], &[(10, 0), (11, 1), (12, FOREGROUND)]),
    ];
    let walk = |bytes: &[u8]| {
        if let Some(cpal) = Cpal::parse(bytes) {
            for palette in 0..4 {
                let _ = cpal.palette_type(palette);
                let _ = cpal.palette_label(palette);
                for entry in 0..8 {
                    let _ = cpal.colour(palette, entry);
                    let _ = cpal.entry_label(entry);
                }
            }
            for choice in [
                PaletteChoice::Default,
                PaletteChoice::Index(1),
                PaletteChoice::LightBackground,
                PaletteChoice::DarkBackground,
            ] {
                assert!(cpal.choose(choice) < cpal.palette_count().max(1));
            }
        }
        if let Some(colr) = Colr::parse(bytes) {
            let palette_bytes = two_palettes().build();
            let cpal = Cpal::parse(&palette_bytes).expect("a CPAL table");
            for glyph in 0..16 {
                let _ = colr.layer_records(glyph);
                let _ = colr.layers(glyph, &cpal, 0);
            }
        }
    };
    for table in &tables {
        for end in 0..=table.len() {
            walk(&table[..end]);
        }
        for at in 0..table.len() {
            for flip in [0x01u8, 0x80, 0xFF] {
                let mut bytes = table.clone();
                bytes[at] ^= flip;
                walk(&bytes);
            }
        }
    }
}

/// An sfnt holding `tables`, in the order given, each padded to four bytes;
/// the search hints and checksums the reader does not read are zeros.
fn face(tables: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0; 6]);
    let mut offset = 12 + 16 * tables.len();
    let mut body = Vec::new();
    for (tag, table) in tables {
        out.extend_from_slice(*tag);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(table.len() as u32).to_be_bytes());
        body.extend_from_slice(table);
        let padded = table.len().div_ceil(4) * 4;
        body.resize(body.len() + (padded - table.len()), 0);
        offset += padded;
    }
    out.extend_from_slice(&body);
    out
}

/// A `head` of 54 bytes stating `units_per_em`, the rest zeros but for the
/// magic number.
fn head(units_per_em: u16) -> Vec<u8> {
    let mut head = vec![0u8; 54];
    head[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    head[12..16].copy_from_slice(&0x5F0F_3CF5u32.to_be_bytes());
    head[18..20].copy_from_slice(&units_per_em.to_be_bytes());
    head
}

/// A version 0.5 `maxp`: the glyph count and nothing else.
fn maxp(glyphs: u16) -> Vec<u8> {
    let mut maxp = 0x0000_5000u32.to_be_bytes().to_vec();
    maxp.extend_from_slice(&glyphs.to_be_bytes());
    maxp
}

/// `fuzz/corpus/sfnt/colr-v0.ttf`: [`three_glyphs`]'s layers over a version 1
/// `CPAL` with both palettes typed and labelled, in a face of 16 glyphs.
fn colr_v0_seed() -> Vec<u8> {
    let cpal = CpalTable {
        version: 1,
        types: Some(vec![
            PaletteType::LIGHT_BACKGROUND,
            PaletteType::DARK_BACKGROUND,
        ]),
        labels: Some(vec![256, 0xFFFF]),
        entry_labels: Some(vec![300, 301, 0xFFFF]),
        ..two_palettes()
    };
    face(&[
        (b"COLR", three_glyphs()),
        (b"CPAL", cpal.build()),
        (b"head", head(1000)),
        (b"maxp", maxp(16)),
    ])
}

/// A colour line to write: its extend value and its stops, each an offset,
/// a palette index, an alpha and, for a `VarColorLine`, a `varIndexBase`.
#[derive(Clone)]
struct Line {
    extend: u8,
    stops: Vec<(i16, u16, i16, u32)>,
}

/// A paint table to write (*Paint tables*), its subtables written after it so
/// every `Offset24` runs forward from the table, as the format requires.
#[derive(Clone)]
enum P {
    /// Format 1.
    Layers { first: u32, count: u8 },
    /// Format 2, or 3 with a `varIndexBase`.
    Solid {
        palette: u16,
        alpha: i16,
        var: Option<u32>,
    },
    /// Format 4, or 5: `x0 y0 x1 y1 x2 y2`.
    Linear {
        line: Line,
        points: [i16; 6],
        var: Option<u32>,
    },
    /// Format 6, or 7.
    Radial {
        line: Line,
        c0: [i16; 2],
        r0: u16,
        c1: [i16; 2],
        r1: u16,
        var: Option<u32>,
    },
    /// Format 8, or 9.
    Sweep {
        line: Line,
        centre: [i16; 2],
        start: i16,
        end: i16,
        var: Option<u32>,
    },
    /// Format 10.
    Glyph { glyph: u16, paint: Box<P> },
    /// Format 11.
    ColrGlyph { glyph: u16 },
    /// Format 12, or 13 with the affine's `varIndexBase`.
    Transform {
        matrix: [i32; 6],
        var: Option<u32>,
        paint: Box<P>,
    },
    /// Formats 14 to 31: the format, its 16-bit fields in order, a variable
    /// format's `varIndexBase`, and the child.
    Fields {
        format: u8,
        fields: Vec<i16>,
        var: Option<u32>,
        paint: Box<P>,
    },
    /// Format 32: the source, then the backdrop.
    Composite {
        mode: u8,
        source: Box<P>,
        backdrop: Box<P>,
    },
    /// Bytes written as they are.
    Raw(Vec<u8>),
}

fn u24(value: usize) -> [u8; 3] {
    let v = value as u32;
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

/// A colour line, its stops ten bytes each where `variable`.
fn line_bytes(line: &Line, variable: bool) -> Vec<u8> {
    let mut out = vec![line.extend];
    out.extend_from_slice(&(line.stops.len() as u16).to_be_bytes());
    for (offset, palette, alpha, var) in &line.stops {
        out.extend_from_slice(&offset.to_be_bytes());
        out.extend_from_slice(&palette.to_be_bytes());
        out.extend_from_slice(&alpha.to_be_bytes());
        if variable {
            out.extend_from_slice(&var.to_be_bytes());
        }
    }
    out
}

/// A paint table, then everything it points to.
fn emit(paint: &P) -> Vec<u8> {
    let mut out = Vec::new();
    let var_len = |var: &Option<u32>| if var.is_some() { 4 } else { 0 };
    let var_bytes = |out: &mut Vec<u8>, var: &Option<u32>| {
        if let Some(var) = var {
            out.extend_from_slice(&var.to_be_bytes());
        }
    };
    match paint {
        P::Layers { first, count } => {
            out.push(1);
            out.push(*count);
            out.extend_from_slice(&first.to_be_bytes());
        }
        P::Solid {
            palette,
            alpha,
            var,
        } => {
            out.push(if var.is_some() { 3 } else { 2 });
            out.extend_from_slice(&palette.to_be_bytes());
            out.extend_from_slice(&alpha.to_be_bytes());
            var_bytes(&mut out, var);
        }
        P::Linear { line, points, var } => {
            out.push(if var.is_some() { 5 } else { 4 });
            out.extend_from_slice(&u24(16 + var_len(var)));
            for value in points {
                out.extend_from_slice(&value.to_be_bytes());
            }
            var_bytes(&mut out, var);
            out.extend_from_slice(&line_bytes(line, var.is_some()));
        }
        P::Radial {
            line,
            c0,
            r0,
            c1,
            r1,
            var,
        } => {
            out.push(if var.is_some() { 7 } else { 6 });
            out.extend_from_slice(&u24(16 + var_len(var)));
            out.extend_from_slice(&c0[0].to_be_bytes());
            out.extend_from_slice(&c0[1].to_be_bytes());
            out.extend_from_slice(&r0.to_be_bytes());
            out.extend_from_slice(&c1[0].to_be_bytes());
            out.extend_from_slice(&c1[1].to_be_bytes());
            out.extend_from_slice(&r1.to_be_bytes());
            var_bytes(&mut out, var);
            out.extend_from_slice(&line_bytes(line, var.is_some()));
        }
        P::Sweep {
            line,
            centre,
            start,
            end,
            var,
        } => {
            out.push(if var.is_some() { 9 } else { 8 });
            out.extend_from_slice(&u24(12 + var_len(var)));
            out.extend_from_slice(&centre[0].to_be_bytes());
            out.extend_from_slice(&centre[1].to_be_bytes());
            out.extend_from_slice(&start.to_be_bytes());
            out.extend_from_slice(&end.to_be_bytes());
            var_bytes(&mut out, var);
            out.extend_from_slice(&line_bytes(line, var.is_some()));
        }
        P::Glyph { glyph, paint } => {
            out.push(10);
            out.extend_from_slice(&u24(6));
            out.extend_from_slice(&glyph.to_be_bytes());
            out.extend_from_slice(&emit(paint));
        }
        P::ColrGlyph { glyph } => {
            out.push(11);
            out.extend_from_slice(&glyph.to_be_bytes());
        }
        P::Transform { matrix, var, paint } => {
            out.push(if var.is_some() { 13 } else { 12 });
            // The affine right after the table, the child after the affine.
            out.extend_from_slice(&u24(7 + 24 + var_len(var)));
            out.extend_from_slice(&u24(7));
            for value in matrix {
                out.extend_from_slice(&value.to_be_bytes());
            }
            var_bytes(&mut out, var);
            out.extend_from_slice(&emit(paint));
        }
        P::Fields {
            format,
            fields,
            var,
            paint,
        } => {
            out.push(*format);
            out.extend_from_slice(&u24(4 + 2 * fields.len() + var_len(var)));
            for value in fields {
                out.extend_from_slice(&value.to_be_bytes());
            }
            var_bytes(&mut out, var);
            out.extend_from_slice(&emit(paint));
        }
        P::Composite {
            mode,
            source,
            backdrop,
        } => {
            let source = emit(source);
            out.push(32);
            out.extend_from_slice(&u24(8));
            out.push(*mode);
            out.extend_from_slice(&u24(8 + source.len()));
            out.extend_from_slice(&source);
            out.extend_from_slice(&emit(backdrop));
        }
        P::Raw(bytes) => out.extend_from_slice(bytes),
    }
    out
}

/// A `ClipList` of `(startGlyphID, endGlyphID, ClipBox bytes)` records, each
/// box after the records.
fn clip_list(format: u8, records: &[(u16, u16, Vec<u8>)]) -> Vec<u8> {
    let mut out = vec![format];
    out.extend_from_slice(&(records.len() as u32).to_be_bytes());
    let mut at = 5 + 7 * records.len();
    let mut boxes = Vec::new();
    for (start, end, clip) in records {
        out.extend_from_slice(&start.to_be_bytes());
        out.extend_from_slice(&end.to_be_bytes());
        out.extend_from_slice(&u24(at));
        at += clip.len();
        boxes.extend_from_slice(clip);
    }
    out.extend_from_slice(&boxes);
    out
}

/// A `ClipBox`: format 1, or format 2 with a `varIndexBase`.
fn clip_box_bytes(bounds: [i16; 4], var: Option<u32>) -> Vec<u8> {
    let mut out = vec![if var.is_some() { 2 } else { 1 }];
    for value in bounds {
        out.extend_from_slice(&value.to_be_bytes());
    }
    if let Some(var) = var {
        out.extend_from_slice(&var.to_be_bytes());
    }
    out
}

/// A version 1 `COLR` with no version 0 records: the `BaseGlyphList` of
/// `bases`, in the order given, the `LayerList` of `layers` — each entry its
/// own table but where `aliases` maps it to an earlier entry's — and the
/// `ClipList` where given, then every table.
fn colr_v1_with(
    bases: &[(u16, P)],
    layers: &[P],
    aliases: &[(usize, usize)],
    clips: Option<&[u8]>,
) -> Vec<u8> {
    let base_list_at = 34;
    let layer_list_at = base_list_at + 4 + 6 * bases.len();
    let layer_list_len = if layers.is_empty() {
        0
    } else {
        4 + 4 * layers.len()
    };
    let clip_at = layer_list_at + layer_list_len;
    let mut tables_at = clip_at + clips.map_or(0, <[u8]>::len);

    let mut tables = Vec::new();
    let mut base_records = Vec::new();
    for (glyph, paint) in bases {
        base_records.push((*glyph, tables_at - base_list_at));
        let bytes = emit(paint);
        tables_at += bytes.len();
        tables.extend_from_slice(&bytes);
    }
    let mut layer_offsets: Vec<usize> = Vec::new();
    for (i, paint) in layers.iter().enumerate() {
        if let Some(&(_, earlier)) = aliases.iter().find(|(entry, _)| *entry == i) {
            let offset = layer_offsets[earlier];
            layer_offsets.push(offset);
            continue;
        }
        layer_offsets.push(tables_at - layer_list_at);
        let bytes = emit(paint);
        tables_at += bytes.len();
        tables.extend_from_slice(&bytes);
    }

    let mut out = Vec::new();
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&[0; 12]); // no version 0 records
    out.extend_from_slice(&(base_list_at as u32).to_be_bytes());
    let layer_offset = if layers.is_empty() { 0 } else { layer_list_at };
    out.extend_from_slice(&(layer_offset as u32).to_be_bytes());
    let clip_offset = if clips.is_some() { clip_at } else { 0 };
    out.extend_from_slice(&(clip_offset as u32).to_be_bytes());
    out.extend_from_slice(&[0; 8]); // no variation data
    out.extend_from_slice(&(bases.len() as u32).to_be_bytes());
    for (glyph, offset) in base_records {
        out.extend_from_slice(&glyph.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
    }
    if !layers.is_empty() {
        out.extend_from_slice(&(layers.len() as u32).to_be_bytes());
        for offset in layer_offsets {
            out.extend_from_slice(&(offset as u32).to_be_bytes());
        }
    }
    if let Some(clips) = clips {
        out.extend_from_slice(clips);
    }
    out.extend_from_slice(&tables);
    out
}

fn colr_v1(bases: &[(u16, P)], layers: &[P]) -> Vec<u8> {
    colr_v1_with(bases, layers, &[], None)
}

fn solid(palette: u16, alpha: i16) -> P {
    P::Solid {
        palette,
        alpha,
        var: None,
    }
}

fn solid_node(palette: u16, alpha: i16) -> Paint {
    Paint::Solid {
        colour: ColourIndex::Palette(palette),
        alpha,
        var_index_base: None,
    }
}

fn boxed(paint: P) -> Box<P> {
    Box::new(paint)
}

fn stop(offset: i16, palette: u16, alpha: i16, var_index_base: Option<u32>) -> ColourStop {
    ColourStop {
        offset,
        colour: ColourIndex::from_raw(palette),
        alpha,
        var_index_base,
    }
}

/// The glyph's graph, whose root holds `expected` and whose nodes below it
/// are `below`, by id, with nothing warned.
fn assert_graph(colr: &Colr<'_>, glyph: u16, expected: &Paint, below: &[(u32, Paint)]) {
    let graph = colr
        .paint_graph(glyph)
        .unwrap_or_else(|| panic!("glyph {glyph} has a graph"));
    assert_eq!(
        graph.paint(graph.root()),
        Some(expected),
        "glyph {glyph}'s root"
    );
    assert_eq!(
        expected.format(),
        Some(glyph as u8),
        "glyph {glyph}'s format"
    );
    for (id, node) in below {
        assert_eq!(
            graph.paint(PaintId(*id)),
            Some(node),
            "glyph {glyph}'s node {id}"
        );
    }
    assert!(
        graph.warnings().is_empty(),
        "glyph {glyph}: {:?}",
        graph.warnings()
    );
}

#[test]
fn every_paint_format_reads_to_its_node() {
    let child = || boxed(solid(7, 0x4000));
    let fields = |format: u8, fields: &[i16], var: Option<u32>| P::Fields {
        format,
        fields: fields.to_vec(),
        var,
        paint: child(),
    };
    let pad_line = Line {
        extend: 0,
        stops: vec![(0, 1, 0x4000, 0), (0x4000, 2, 0x2000, 0)],
    };
    let bases: Vec<(u16, P)> = vec![
        (1, P::Layers { first: 0, count: 2 }),
        (2, solid(3, 0x3000)),
        (
            3,
            P::Solid {
                palette: FOREGROUND,
                alpha: 0x1000,
                var: Some(77),
            },
        ),
        (
            4,
            P::Linear {
                line: pad_line.clone(),
                points: [1, 2, 3, 4, 5, -6],
                var: None,
            },
        ),
        (
            5,
            P::Linear {
                line: Line {
                    extend: 1,
                    stops: vec![(-0x2000, FOREGROUND, 0x4000, 10), (0x6000, 0, 0x1000, 12)],
                },
                points: [-1, -2, -3, -4, -5, 6],
                var: Some(99),
            },
        ),
        (
            6,
            P::Radial {
                line: Line {
                    extend: 2,
                    stops: vec![(0, 4, 0x4000, 0)],
                },
                c0: [10, 20],
                r0: 30,
                c1: [-40, 50],
                r1: 60,
                var: None,
            },
        ),
        (
            7,
            P::Radial {
                line: Line {
                    extend: 0,
                    stops: vec![(0x1000, 5, 0x3FFF, 1)],
                },
                c0: [0, 0],
                r0: 0,
                c1: [100, 100],
                r1: 0xFFFF,
                var: Some(7),
            },
        ),
        (
            8,
            P::Sweep {
                line: pad_line,
                centre: [100, -100],
                start: 0,
                end: 0x4000,
                var: None,
            },
        ),
        (
            9,
            P::Sweep {
                line: Line {
                    extend: 1,
                    stops: vec![(0, 2, 0x4000, 3)],
                },
                centre: [-1, 1],
                start: i16::MIN,
                end: 0x3FFF,
                var: Some(9),
            },
        ),
        (
            10,
            P::Glyph {
                glyph: 42,
                paint: child(),
            },
        ),
        (11, P::ColrGlyph { glyph: 2 }),
        (
            12,
            P::Transform {
                matrix: [0x1_0000, 0, 0, 0x1_0000, 0x2_0000, -0x3_0000],
                var: None,
                paint: child(),
            },
        ),
        (
            13,
            P::Transform {
                matrix: [0x8000, 0x4000, -0x4000, 0x8000, 0, 1],
                var: Some(5),
                paint: child(),
            },
        ),
        (14, fields(14, &[11, -12], None)),
        (15, fields(15, &[13, 14], Some(15))),
        (16, fields(16, &[0x2000, 0x6000], None)),
        (17, fields(17, &[0x1000, 0x1001], Some(17))),
        (18, fields(18, &[0x2000, 0x6000, 7, 8], None)),
        (19, fields(19, &[-0x4000, 0x4000, -7, -8], Some(19))),
        (20, fields(20, &[0x3000], None)),
        (21, fields(21, &[0x5000], Some(21))),
        (22, fields(22, &[0x3000, 9, 10], None)),
        (23, fields(23, &[0x7FFF, -9, -10], Some(23))),
        (24, fields(24, &[0x2000], None)),
        (25, fields(25, &[-0x2000], Some(25))),
        (26, fields(26, &[0x1000, 11, 12], None)),
        (27, fields(27, &[0x1001, -11, -12], Some(27))),
        (28, fields(28, &[0x1000, -0x1000], None)),
        (29, fields(29, &[0x0800, 0x0400], Some(29))),
        (30, fields(30, &[0x0100, 0x0200, 13, 14], None)),
        (31, fields(31, &[-0x0100, -0x0200, -13, -14], Some(31))),
        (
            32,
            P::Composite {
                mode: 23,
                source: boxed(solid(1, 0x4000)),
                backdrop: boxed(solid(2, 0x2000)),
            },
        ),
    ];
    let layers = vec![solid(1, 0x4000), solid(2, 0x2000)];
    let bytes = colr_v1(&bases, &layers);
    let colr = Colr::parse(&bytes).expect("a version 1 COLR table");
    assert!(colr.warnings().is_empty(), "{:?}", colr.warnings());
    let leaf = solid_node(7, 0x4000);
    let one = PaintId(0);

    assert_graph(
        &colr,
        1,
        &Paint::Layers(vec![PaintId(0), PaintId(1)]),
        &[(0, solid_node(1, 0x4000)), (1, solid_node(2, 0x2000))],
    );
    assert_graph(&colr, 2, &solid_node(3, 0x3000), &[]);
    assert_graph(
        &colr,
        3,
        &Paint::Solid {
            colour: ColourIndex::Foreground,
            alpha: 0x1000,
            var_index_base: Some(77),
        },
        &[],
    );
    let pad = ColourLine {
        extend: Extend::Pad,
        stops: vec![stop(0, 1, 0x4000, None), stop(0x4000, 2, 0x2000, None)],
    };
    assert_graph(
        &colr,
        4,
        &Paint::LinearGradient {
            line: pad.clone(),
            p0: [1, 2],
            p1: [3, 4],
            p2: [5, -6],
            var_index_base: None,
        },
        &[],
    );
    assert_graph(
        &colr,
        5,
        &Paint::LinearGradient {
            line: ColourLine {
                extend: Extend::Repeat,
                stops: vec![
                    stop(-0x2000, FOREGROUND, 0x4000, Some(10)),
                    stop(0x6000, 0, 0x1000, Some(12)),
                ],
            },
            p0: [-1, -2],
            p1: [-3, -4],
            p2: [-5, 6],
            var_index_base: Some(99),
        },
        &[],
    );
    assert_graph(
        &colr,
        6,
        &Paint::RadialGradient {
            line: ColourLine {
                extend: Extend::Reflect,
                stops: vec![stop(0, 4, 0x4000, None)],
            },
            c0: [10, 20],
            r0: 30,
            c1: [-40, 50],
            r1: 60,
            var_index_base: None,
        },
        &[],
    );
    assert_graph(
        &colr,
        7,
        &Paint::RadialGradient {
            line: ColourLine {
                extend: Extend::Pad,
                stops: vec![stop(0x1000, 5, 0x3FFF, Some(1))],
            },
            c0: [0, 0],
            r0: 0,
            c1: [100, 100],
            r1: 0xFFFF,
            var_index_base: Some(7),
        },
        &[],
    );
    assert_graph(
        &colr,
        8,
        &Paint::SweepGradient {
            line: pad,
            centre: [100, -100],
            start_angle: 0,
            end_angle: 0x4000,
            var_index_base: None,
        },
        &[],
    );
    assert_graph(
        &colr,
        9,
        &Paint::SweepGradient {
            line: ColourLine {
                extend: Extend::Repeat,
                stops: vec![stop(0, 2, 0x4000, Some(3))],
            },
            centre: [-1, 1],
            start_angle: i16::MIN,
            end_angle: 0x3FFF,
            var_index_base: Some(9),
        },
        &[],
    );
    assert_graph(
        &colr,
        10,
        &Paint::Glyph {
            glyph: 42,
            paint: one,
        },
        &[(0, leaf.clone())],
    );
    assert_graph(
        &colr,
        11,
        &Paint::ColrGlyph {
            glyph: 2,
            paint: one,
        },
        &[(0, solid_node(3, 0x3000))],
    );
    assert_graph(
        &colr,
        12,
        &Paint::Transform {
            matrix: [0x1_0000, 0, 0, 0x1_0000, 0x2_0000, -0x3_0000],
            paint: one,
            var_index_base: None,
        },
        &[(0, leaf.clone())],
    );
    assert_graph(
        &colr,
        13,
        &Paint::Transform {
            matrix: [0x8000, 0x4000, -0x4000, 0x8000, 0, 1],
            paint: one,
            var_index_base: Some(5),
        },
        &[(0, leaf.clone())],
    );
    let child_of =
        |glyph: u16, node: Paint| assert_graph(&colr, glyph, &node, &[(0, leaf.clone())]);
    child_of(
        14,
        Paint::Translate {
            dx: 11,
            dy: -12,
            paint: one,
            var_index_base: None,
        },
    );
    child_of(
        15,
        Paint::Translate {
            dx: 13,
            dy: 14,
            paint: one,
            var_index_base: Some(15),
        },
    );
    let scale = |glyph: u16, x: i16, y: i16, centre: Option<[i16; 2]>, var: Option<u32>| {
        child_of(
            glyph,
            Paint::Scale {
                scale_x: x,
                scale_y: y,
                centre,
                paint: one,
                var_index_base: var,
            },
        );
    };
    scale(16, 0x2000, 0x6000, None, None);
    scale(17, 0x1000, 0x1001, None, Some(17));
    scale(18, 0x2000, 0x6000, Some([7, 8]), None);
    scale(19, -0x4000, 0x4000, Some([-7, -8]), Some(19));
    let uniform = |glyph: u16, s: i16, centre: Option<[i16; 2]>, var: Option<u32>| {
        child_of(
            glyph,
            Paint::ScaleUniform {
                scale: s,
                centre,
                paint: one,
                var_index_base: var,
            },
        );
    };
    uniform(20, 0x3000, None, None);
    uniform(21, 0x5000, None, Some(21));
    uniform(22, 0x3000, Some([9, 10]), None);
    uniform(23, 0x7FFF, Some([-9, -10]), Some(23));
    let rotate = |glyph: u16, angle: i16, centre: Option<[i16; 2]>, var: Option<u32>| {
        child_of(
            glyph,
            Paint::Rotate {
                angle,
                centre,
                paint: one,
                var_index_base: var,
            },
        );
    };
    rotate(24, 0x2000, None, None);
    rotate(25, -0x2000, None, Some(25));
    rotate(26, 0x1000, Some([11, 12]), None);
    rotate(27, 0x1001, Some([-11, -12]), Some(27));
    let skew = |glyph: u16, x: i16, y: i16, centre: Option<[i16; 2]>, var: Option<u32>| {
        child_of(
            glyph,
            Paint::Skew {
                x_angle: x,
                y_angle: y,
                centre,
                paint: one,
                var_index_base: var,
            },
        );
    };
    skew(28, 0x1000, -0x1000, None, None);
    skew(29, 0x0800, 0x0400, None, Some(29));
    skew(30, 0x0100, 0x0200, Some([13, 14]), None);
    skew(31, -0x0100, -0x0200, Some([-13, -14]), Some(31));
    assert_graph(
        &colr,
        32,
        &Paint::Composite {
            source: PaintId(0),
            mode: CompositeMode::Multiply,
            backdrop: PaintId(1),
        },
        &[(0, solid_node(1, 0x4000)), (1, solid_node(2, 0x2000))],
    );

    // A composite draws its backdrop first.
    let graph = colr.paint_graph(32).expect("a graph");
    let root = graph.paint(graph.root()).expect("a root");
    assert_eq!(root.children(), vec![PaintId(1), PaintId(0)]);
    assert!(!colr.has_paint(0) && colr.has_paint(32));
    assert!(colr.paint_graph(33).is_none(), "no record, no graph");
}

/// Every value of the `CompositeMode` enumeration reads to its mode, and one
/// it does not define reads as `COMPOSITE_CLEAR`, named; an extend value the
/// `Extend` enumeration does not define reads as `EXTEND_PAD`, named.
#[test]
fn every_composite_mode_and_an_unknown_one_read_as_the_clause_says() {
    let mut bases: Vec<(u16, P)> = (0..=27u8)
        .map(|mode| {
            (
                u16::from(mode),
                P::Composite {
                    mode,
                    source: boxed(solid(1, 0x4000)),
                    backdrop: boxed(solid(2, 0x4000)),
                },
            )
        })
        .collect();
    bases.push((
        28,
        P::Composite {
            mode: 28,
            source: boxed(solid(1, 0x4000)),
            backdrop: boxed(solid(2, 0x4000)),
        },
    ));
    bases.push((
        29,
        P::Linear {
            line: Line {
                extend: 3,
                stops: vec![(0, 1, 0x4000, 0)],
            },
            points: [0, 0, 100, 0, 0, 100],
            var: None,
        },
    ));
    let bytes = colr_v1(&bases, &[]);
    let colr = Colr::parse(&bytes).expect("a version 1 COLR table");
    for (value, expected) in CompositeMode::ALL.iter().enumerate() {
        let graph = colr.paint_graph(value as u16).expect("a graph");
        match graph.paint(graph.root()) {
            Some(Paint::Composite { mode, .. }) => assert_eq!(mode, expected, "mode {value}"),
            other => panic!("mode {value}: {other:?}"),
        }
        assert!(graph.warnings().is_empty());
        assert_eq!(CompositeMode::from_raw(value as u8), Some(*expected));
    }
    assert_eq!(CompositeMode::from_raw(28), None);
    let graph = colr.paint_graph(28).expect("a graph");
    assert!(matches!(
        graph.paint(graph.root()),
        Some(Paint::Composite {
            mode: CompositeMode::Clear,
            ..
        })
    ));
    assert_eq!(graph.warnings(), &[ColrWarning::CompositeModeRead(28)]);
    let graph = colr.paint_graph(29).expect("a graph");
    match graph.paint(graph.root()) {
        Some(Paint::LinearGradient { line, .. }) => assert_eq!(line.extend, Extend::Pad),
        other => panic!("{other:?}"),
    }
    assert_eq!(graph.warnings(), &[ColrWarning::ExtendRead(3)]);
}

#[test]
fn a_clip_box_is_read_for_its_glyph_range() {
    let first = clip_box_bytes([-10, -20, 300, 400], None);
    let second = clip_box_bytes([0, 0, 1000, 1000], Some(3));
    let mut unknown = clip_box_bytes([1, 2, 3, 4], None);
    unknown[0] = 3;
    let clips = clip_list(
        1,
        &[
            (5, 9, first.clone()),
            (12, 12, second.clone()),
            (15, 20, unknown),
            (30, 25, first.clone()),
        ],
    );
    let bytes = colr_v1_with(&[(5, solid(0, 0x4000))], &[], &[], Some(&clips));
    let colr = Colr::parse(&bytes).expect("a version 1 COLR table");
    assert!(colr.warnings().is_empty(), "{:?}", colr.warnings());
    let box_one = ClipBox {
        x_min: -10,
        y_min: -20,
        x_max: 300,
        y_max: 400,
        var_index_base: None,
    };
    let box_two = ClipBox {
        x_min: 0,
        y_min: 0,
        x_max: 1000,
        y_max: 1000,
        var_index_base: Some(3),
    };
    for (glyph, expected) in [
        (4, None),
        (5, Some(box_one)),
        (7, Some(box_one)),
        (9, Some(box_one)),
        (10, None),
        (12, Some(box_two)),
        (13, None),
        (16, None),
        (21, None),
        (27, None),
        (30, None),
    ] {
        assert_eq!(colr.clip_box(glyph), expected, "glyph {glyph}");
    }

    // Out of order, and overlapping: scanned, the first range holding a
    // glyph answering, and named.
    for records in [
        vec![(12, 12, second.clone()), (5, 9, first.clone())],
        vec![(5, 9, first.clone()), (8, 12, second.clone())],
    ] {
        let clips = clip_list(1, &records);
        let bytes = colr_v1_with(&[], &[], &[], Some(&clips));
        let colr = Colr::parse(&bytes).expect("a version 1 COLR table");
        assert_eq!(colr.warnings(), &[ColrWarning::ClipsOutOfOrder]);
        assert_eq!(colr.clip_box(8), Some(box_one));
        assert_eq!(colr.clip_box(12), Some(box_two));
    }

    // A ClipList of a format the clause does not define has no clips, and a
    // box past the table is none.
    let clips = clip_list(2, &[(5, 9, first.clone())]);
    let bytes = colr_v1_with(&[], &[], &[], Some(&clips));
    assert_eq!(Colr::parse(&bytes).expect("a table").clip_box(5), None);
    let mut clips = clip_list(1, &[(5, 9, first)]);
    clips.truncate(clips.len() - 2);
    let bytes = colr_v1_with(&[], &[], &[], Some(&clips));
    assert_eq!(Colr::parse(&bytes).expect("a table").clip_box(5), None);
}

#[test]
fn a_colr_glyph_cycle_is_refused_by_name() {
    let bases = vec![
        (1, P::ColrGlyph { glyph: 2 }),
        (2, P::ColrGlyph { glyph: 1 }),
        (3, P::Layers { first: 0, count: 1 }),
        (4, P::Layers { first: 1, count: 2 }),
    ];
    // Entry 0 names itself through the slice it is; entries 1 and 2 are one
    // table, shared.
    let layers = vec![
        P::Layers { first: 0, count: 1 },
        solid(1, 0x4000),
        solid(9, 0),
    ];
    let bytes = colr_v1_with(&bases, &layers, &[(2, 1)], None);
    let colr = Colr::parse(&bytes).expect("a version 1 COLR table");

    let graph = colr.paint_graph(1).expect("a graph");
    let Some(Paint::ColrGlyph { glyph: 2, paint }) = graph.paint(graph.root()) else {
        panic!("{:?}", graph.paint(graph.root()));
    };
    let Some(Paint::ColrGlyph { glyph: 1, paint }) = graph.paint(*paint) else {
        panic!("{:?}", graph.paint(*paint));
    };
    let Some(Paint::Refused(refusal @ ColrRefusal::Cycle { .. })) = graph.paint(*paint) else {
        panic!("the cycle is refused: {:?}", graph.paint(*paint));
    };
    assert_eq!(graph.warnings(), &[ColrWarning::PaintRefused(*refusal)]);

    let graph = colr.paint_graph(3).expect("a graph");
    let Some(Paint::Layers(outer)) = graph.paint(graph.root()) else {
        panic!("{:?}", graph.paint(graph.root()));
    };
    let Some(Paint::Layers(inner)) = graph.paint(outer[0]) else {
        panic!("{:?}", graph.paint(outer[0]));
    };
    assert!(matches!(
        graph.paint(inner[0]),
        Some(Paint::Refused(ColrRefusal::Cycle { .. }))
    ));
    assert_eq!(graph.warnings().len(), 1);

    // A table two parents reach is one node and no cycle.
    let graph = colr.paint_graph(4).expect("a graph");
    let Some(Paint::Layers(shared)) = graph.paint(graph.root()) else {
        panic!("{:?}", graph.paint(graph.root()));
    };
    assert_eq!(shared.len(), 2);
    assert_eq!(shared[0], shared[1], "one table, one node");
    assert_eq!(graph.nodes().len(), 2);
    assert!(graph.warnings().is_empty());
}

/// `n` translations, each the next's parent, over a solid.
fn chain(n: usize) -> P {
    let mut paint = solid(1, 0x4000);
    for _ in 0..n {
        paint = P::Fields {
            format: 14,
            fields: vec![1, 1],
            var: None,
            paint: boxed(paint),
        };
    }
    paint
}

#[test]
fn a_graph_past_its_depth_stops_with_a_warning() {
    let deepest = MAX_PAINT_DEPTH as usize - 1;
    let bases = vec![(1, chain(deepest)), (2, chain(deepest + 1))];
    let bytes = colr_v1(&bases, &[]);
    let colr = Colr::parse(&bytes).expect("a version 1 COLR table");

    let whole = colr.paint_graph(1).expect("a graph");
    assert_eq!(
        whole.nodes().len(),
        deepest + 1,
        "every paint, the solid last"
    );
    assert!(whole.warnings().is_empty());
    assert_eq!(whole.paint(PaintId(0)), Some(&solid_node(1, 0x4000)));

    let cut = colr.paint_graph(2).expect("a graph");
    assert_eq!(
        cut.warnings(),
        &[ColrWarning::PaintRefused(ColrRefusal::TooDeep)]
    );
    assert_eq!(
        cut.paint(PaintId(0)),
        Some(&Paint::Refused(ColrRefusal::TooDeep)),
        "the solid sat a paint past the limit"
    );
    let translations = cut
        .nodes()
        .iter()
        .filter(|node| matches!(node, Paint::Translate { .. }))
        .count();
    assert_eq!(translations, deepest + 1);
}

#[test]
fn a_graph_past_its_size_stops_with_a_warning() {
    // A root of 65 layers, each a slice of 255 distinct solids: more tables
    // than one glyph may hold.
    let (outer, each) = (65u32, 255u32);
    let mut layers: Vec<P> = (0..outer)
        .map(|k| P::Layers {
            first: outer + k * each,
            count: each as u8,
        })
        .collect();
    layers.extend((0..outer * each).map(|k| solid((k % 7) as u16, 0x4000)));
    let bases = vec![(
        1,
        P::Layers {
            first: 0,
            count: outer as u8,
        },
    )];
    let bytes = colr_v1(&bases, &layers);
    let colr = Colr::parse(&bytes).expect("a version 1 COLR table");
    let graph = colr.paint_graph(1).expect("a graph");
    let full = MAX_PAINT_NODES / (each as usize + 1);
    assert_eq!(
        graph.nodes().len(),
        full * (each as usize + 1) + 2,
        "the slices that fit, the refusal and the root"
    );
    assert_eq!(
        graph.warnings(),
        &[ColrWarning::PaintRefused(ColrRefusal::TooLarge)]
    );
    let Some(Paint::Layers(children)) = graph.paint(graph.root()) else {
        panic!("a root of layers");
    };
    assert_eq!(children.len(), outer as usize);
    assert_eq!(
        graph.paint(children[full]),
        Some(&Paint::Refused(ColrRefusal::TooLarge))
    );
}

#[test]
fn a_colour_line_overrunning_its_table_answers_none() {
    let overrun = |variable: bool| {
        let mut bytes = emit(&P::Linear {
            line: Line {
                extend: 0,
                stops: vec![(0, 1, 0x4000, 0), (0x4000, 2, 0x4000, 0)],
            },
            points: [0, 0, 100, 0, 0, 100],
            var: variable.then_some(1),
        });
        // The stop count, right after the line's extend byte.
        let line = if variable { 20 } else { 16 };
        bytes[line + 1..line + 3].copy_from_slice(&1000u16.to_be_bytes());
        P::Raw(bytes)
    };
    let bases = vec![
        (1, overrun(false)),
        (2, overrun(true)),
        (3, P::Layers { first: 0, count: 2 }),
        (4, P::Raw(vec![33, 0, 0, 0, 0])),
        (5, P::ColrGlyph { glyph: 99 }),
        (6, P::Raw(vec![10, 0, 0, 0, 0, 5])),
        (7, P::Layers { first: 2, count: 3 }),
        (8, P::Layers { first: 3, count: 2 }),
    ];
    let layers = vec![
        overrun(false),
        solid(1, 0x4000),
        P::Raw(vec![33, 0, 0, 0, 0]),
        P::ColrGlyph { glyph: 99 },
        solid(2, 0x4000),
    ];
    let bytes = colr_v1(&bases, &layers);
    let colr = Colr::parse(&bytes).expect("a version 1 COLR table");
    for glyph in [1, 2, 4, 5, 6] {
        assert!(
            colr.paint_graph(glyph).is_none(),
            "glyph {glyph}'s root reads as nothing, so the glyph has no graph"
        );
    }

    // Below the root, the overrunning line's gradient is refused by name and
    // its sibling still read.
    let graph = colr.paint_graph(3).expect("a graph");
    let Some(Paint::Layers(children)) = graph.paint(graph.root()) else {
        panic!("{:?}", graph.paint(graph.root()));
    };
    let Some(Paint::Refused(refusal @ ColrRefusal::Malformed { .. })) = graph.paint(children[0])
    else {
        panic!("{:?}", graph.paint(children[0]));
    };
    assert_eq!(graph.paint(children[1]), Some(&solid_node(1, 0x4000)));
    assert_eq!(graph.warnings(), &[ColrWarning::PaintRefused(*refusal)]);

    // A format the clause does not define, and a glyph the list does not hold.
    let graph = colr.paint_graph(7).expect("a graph");
    let Some(Paint::Layers(children)) = graph.paint(graph.root()) else {
        panic!("{:?}", graph.paint(graph.root()));
    };
    assert_eq!(
        graph.paint(children[0]),
        Some(&Paint::Refused(ColrRefusal::UnknownFormat(33)))
    );
    assert_eq!(
        graph.paint(children[1]),
        Some(&Paint::Refused(ColrRefusal::MissingColrGlyph(99)))
    );
    assert_eq!(graph.paint(children[2]), Some(&solid_node(2, 0x4000)));
    assert_eq!(graph.warnings().len(), 2);

    // A slice to the LayerList's end reads; one past it does not.
    let graph = colr.paint_graph(8).expect("the root reads");
    assert!(matches!(
        graph.paint(graph.root()),
        Some(Paint::Layers(children)) if children.len() == 2
    ));
    let bases = vec![(1, P::Layers { first: 4, count: 3 })];
    let bytes = colr_v1(&bases, &layers);
    let colr = Colr::parse(&bytes).expect("a version 1 COLR table");
    assert!(colr.paint_graph(1).is_none(), "a slice past the list");
}

/// The order of `BaseGlyphPaintRecord`s is checked once: out of order, they
/// are scanned and the first record for a glyph answers.
#[test]
fn base_glyph_paints_out_of_order_are_scanned_and_named() {
    let bases = vec![
        (9, solid(1, 0x4000)),
        (3, solid(2, 0x4000)),
        (9, solid(3, 0x4000)),
    ];
    let bytes = colr_v1(&bases, &[]);
    let colr = Colr::parse(&bytes).expect("a version 1 COLR table");
    assert_eq!(colr.warnings(), &[ColrWarning::BaseGlyphPaintsOutOfOrder]);
    let graph = colr.paint_graph(9).expect("a graph");
    assert_eq!(graph.paint(graph.root()), Some(&solid_node(1, 0x4000)));
    assert!(colr.paint_graph(3).is_some());
    assert!(colr.paint_graph(4).is_none());

    // A list whose records run past the table is no list.
    let mut bytes = colr_v1(&[(1, solid(1, 0x4000))], &[]);
    bytes[34..38].copy_from_slice(&500u32.to_be_bytes());
    let colr = Colr::parse(&bytes).expect("the header is whole");
    assert!(!colr.has_paint(1));
}

/// A version 1 table holding a few of every family, the graph the `colr-v1`
/// seed carries: layers of glyph-clipped fills and gradients, a composite, a
/// rotation about a centre reusing another glyph, a skew, and clip boxes.
fn seed_graph() -> Vec<u8> {
    let line = |extend: u8, stops: Vec<(i16, u16, i16, u32)>| Line { extend, stops };
    let glyph = |glyph: u16, paint: P| P::Glyph {
        glyph,
        paint: boxed(paint),
    };
    let bases = vec![
        (1, P::Layers { first: 0, count: 3 }),
        (
            2,
            P::Composite {
                mode: 3,
                source: boxed(glyph(
                    13,
                    P::Sweep {
                        line: line(0, vec![(0, 2, 0x4000, 0), (0x4000, 0, 0x4000, 0)]),
                        centre: [500, 500],
                        start: 0,
                        end: 0x4000,
                        var: None,
                    },
                )),
                backdrop: boxed(glyph(
                    14,
                    P::Solid {
                        palette: FOREGROUND,
                        alpha: 0x3000,
                        var: Some(0),
                    },
                )),
            },
        ),
        (
            3,
            P::Fields {
                format: 26,
                fields: vec![0x1000, 500, 500],
                var: None,
                paint: boxed(P::ColrGlyph { glyph: 1 }),
            },
        ),
        (
            4,
            P::Fields {
                format: 30,
                fields: vec![0x0800, 0, 0, 0],
                var: None,
                paint: boxed(glyph(15, solid(1, 0x4000))),
            },
        ),
    ];
    let layers = vec![
        glyph(10, solid(0, 0x4000)),
        glyph(
            11,
            P::Linear {
                line: line(1, vec![(0, 1, 0x4000, 0), (0x4000, 2, 0x2000, 0)]),
                points: [0, 0, 1000, 0, 0, 1000],
                var: None,
            },
        ),
        glyph(
            12,
            P::Transform {
                matrix: [0x1_0000, 0, 0, 0x1_0000, 0x10_0000, 0],
                var: None,
                paint: boxed(P::Radial {
                    line: line(2, vec![(0, 0, 0x4000, 0), (0x4000, 1, 0x4000, 0)]),
                    c0: [500, 500],
                    r0: 0,
                    c1: [500, 500],
                    r1: 500,
                    var: None,
                }),
            },
        ),
    ];
    let clips = clip_list(
        1,
        &[
            (1, 2, clip_box_bytes([0, 0, 1000, 1000], None)),
            (3, 4, clip_box_bytes([-100, -100, 1100, 1100], Some(0))),
        ],
    );
    colr_v1_with(&bases, &layers, &[], Some(&clips))
}

#[test]
fn every_prefix_and_flip_of_a_colr_v1_table_answers_without_panicking() {
    let table = seed_graph();
    let walk = |bytes: &[u8]| {
        if let Some(colr) = Colr::parse(bytes) {
            for glyph in 0..8 {
                if let Some(graph) = colr.paint_graph(glyph) {
                    assert!(graph.nodes().len() <= MAX_PAINT_NODES + MAX_PAINT_DEPTH as usize);
                    for node in graph.nodes() {
                        for child in node.children() {
                            assert!(graph.paint(child).is_some(), "a child names a node");
                        }
                    }
                }
                let _ = colr.clip_box(glyph);
            }
        }
    };
    walk(&table);
    for end in 0..=table.len() {
        walk(&table[..end]);
    }
    for at in 0..table.len() {
        for flip in [0x01u8, 0x80, 0xFF] {
            let mut bytes = table.clone();
            bytes[at] ^= flip;
            walk(&bytes);
        }
    }
}

/// `fuzz/corpus/sfnt/colr-v1.ttf`: [`seed_graph`] over [`two_palettes`].
fn colr_v1_seed() -> Vec<u8> {
    face(&[
        (b"COLR", seed_graph()),
        (b"CPAL", two_palettes().build()),
        (b"head", head(1000)),
        (b"maxp", maxp(16)),
    ])
}

/// The colour tables a seed carries, read through its directory, as the
/// `sfnt` target reads them.
fn colour_tables_of(face: &[u8]) -> (Option<Colr<'_>>, Option<Cpal<'_>>) {
    let font = Sfnt::parse(face).expect("the seed's directory reads");
    let colr = font
        .table(u32::from_be_bytes(*b"COLR"))
        .and_then(Colr::parse);
    let cpal = font
        .table(u32::from_be_bytes(*b"CPAL"))
        .and_then(Cpal::parse);
    (colr, cpal)
}

/// The `fuzz/corpus/sfnt` seeds the builders here write.
const SEEDS: [&str; 2] = ["colr-v0.ttf", "colr-v1.ttf"];

/// A seed's bytes, as the builders write it.
fn seed_bytes(name: &str) -> Vec<u8> {
    match name {
        "colr-v0.ttf" => colr_v0_seed(),
        "colr-v1.ttf" => colr_v1_seed(),
        _ => Vec::new(),
    }
}

/// Whether a seed reaches the readers it is there for.
fn reaches(name: &str, face: &[u8]) -> bool {
    let (colr, cpal) = colour_tables_of(face);
    let (Some(colr), Some(cpal)) = (colr, cpal) else {
        return false;
    };
    match name {
        "colr-v0.ttf" => {
            let palette = cpal.choose(PaletteChoice::DarkBackground);
            palette == 1
                && colr
                    .layers(1, &cpal, palette)
                    .is_some_and(|layers| layers.len() == 3)
                && cpal.palette_label(0) == Some(256)
        }
        "colr-v1.ttf" => {
            (1..=4).all(|glyph| {
                colr.paint_graph(glyph)
                    .is_some_and(|graph| graph.warnings().is_empty())
            }) && colr
                .clip_box(4)
                .is_some_and(|clip| clip.var_index_base == Some(0))
        }
        _ => false,
    }
}

/// The seeds draw what they claim to: each is read through its directory to
/// the colour tables the `sfnt` target walks, so a seed edited into a refusal
/// fails here rather than being written out as a seed reaching nothing.
#[test]
fn the_written_seeds_reach_the_colour_tables() {
    for name in SEEDS {
        assert!(reaches(name, &seed_bytes(name)), "{name}");
    }
}

/// The committed seeds, where this checkout carries the fuzz tree, reach the
/// colour tables as the ones written here do.
#[test]
fn the_committed_seeds_reach_the_colour_tables() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/sfnt");
    for name in SEEDS {
        let Ok(seed) = std::fs::read(dir.join(name)) else {
            println!("colour-seeds: SKIPPED (no fuzz/corpus/sfnt/{name})");
            continue;
        };
        assert!(reaches(name, &seed), "{name}");
        println!("RAN colour-seeds: {name} reaches its tables");
    }
}

/// Writes the `fuzz/corpus/sfnt` seeds these builders make, so the seeds and
/// the fixtures cannot drift apart.
///
/// Run with `--ignored` when a fixture changes; the corpus is committed, and a
/// run that rewrites it is a diff to look at rather than to apply blindly.
#[test]
#[ignore = "writes into fuzz/corpus/sfnt, which is committed"]
fn write_the_fuzz_seeds() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/sfnt");
    for name in SEEDS {
        std::fs::write(base.join(name), seed_bytes(name)).expect("the corpus directory is there");
    }
}
