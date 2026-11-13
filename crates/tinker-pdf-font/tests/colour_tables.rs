//! The colour tables, read from bytes the tests assemble field by field
//! (`docs/design/colour-fonts.md`).
//!
//! Every table here is written from the OpenType 1.9.1 chapter's own layout —
//! the CPAL, COLR, sbix, CBLC and CBDT chapters — by a builder in this file,
//! so each assertion compares what the reader hands back with the bytes the
//! test put there, and none with another program's reading (ruling 13). The
//! builders also write the `fuzz/corpus/sfnt` seeds the `sfnt` target starts
//! from (`write_the_fuzz_seeds`), so the seeds and the fixtures cannot drift.

use tinker_pdf_font::colr::{ColrWarning, FOREGROUND};
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

/// The seeds draw what they claim to: each is read through its directory to
/// the colour tables the `sfnt` target walks, so a seed edited into a refusal
/// fails here rather than being written out as a seed reaching nothing.
#[test]
fn the_written_seeds_reach_the_colour_tables() {
    let seed = colr_v0_seed();
    let (colr, cpal) = colour_tables_of(&seed);
    let (colr, cpal) = (colr.expect("COLR"), cpal.expect("CPAL"));
    let palette = cpal.choose(PaletteChoice::DarkBackground);
    assert_eq!(palette, 1);
    assert_eq!(colr.layers(1, &cpal, palette).map(|l| l.len()), Some(3));
    assert_eq!(cpal.palette_label(0), Some(256));
}

/// The committed seeds, where this checkout carries the fuzz tree, reach the
/// colour tables as the ones written here do.
#[test]
fn the_committed_seeds_reach_the_colour_tables() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/sfnt");
    let Ok(seed) = std::fs::read(dir.join("colr-v0.ttf")) else {
        println!("colour-seeds: SKIPPED (no fuzz/corpus/sfnt/colr-v0.ttf)");
        return;
    };
    let (colr, cpal) = colour_tables_of(&seed);
    let (colr, cpal) = (colr.expect("COLR"), cpal.expect("CPAL"));
    assert!(colr
        .layers(1, &cpal, 0)
        .is_some_and(|layers| !layers.is_empty()));
    println!("RAN colour-seeds: colr-v0.ttf reaches COLR and CPAL");
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
    for (name, bytes) in [("colr-v0.ttf", colr_v0_seed())] {
        std::fs::write(base.join(name), bytes).expect("the corpus directory is there");
    }
}
