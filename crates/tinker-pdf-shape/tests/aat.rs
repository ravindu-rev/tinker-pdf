//! AAT shaping: `docs/design/shaping.md`, *AAT shaping*.
//!
//! # What these tests can and cannot say
//!
//! The faces and tables below are assembled here, byte by byte, from Apple's
//! *TrueType Reference Manual* — its `morx` chapter, its *AAT special tables*
//! chapter for lookup and state tables, and the `feat` and `mort` chapters —
//! and every expectation is computed from the bytes the test wrote or stated
//! beside them, the standing `tests/synthetic.rs` states for itself. A test
//! here catches a disagreement between the readers and the reading the
//! design settles from the manual; it cannot catch a misreading the two
//! share. Where the manual gives a worked example, the example is transcribed
//! as bytes, each misprint corrected and named where it is, and the manual's
//! own answer is the expectation; the text-rendering-tests `MORX` sections,
//! whose expected output their authors wrote, are what hold the chain to
//! faces a compiler built (`tests/text_rendering.rs`).

use std::collections::BTreeMap;
use tinker_pdf_font::Sfnt;

use tinker_pdf_shape::aat::{
    Cursor, Entry, Lookup, LookupError, LookupFormat, Site, StateError, StateTable, Traversal,
    DELETED, DELETED_GLYPH, END_OF_TEXT, OUT_OF_BOUNDS,
};
use tinker_pdf_shape::bidi::{BaseDirection, Level};
use tinker_pdf_shape::shape::Run;
use tinker_pdf_shape::unicode::Script;
use tinker_pdf_shape::{Buffer, Limits, Orientation, ShapedGlyph, ShapedRun, Shaper, Tag, Warning};

// --- bytes ---------------------------------------------------------------

fn u16s(values: &[u16]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_be_bytes()).collect()
}

fn u32s(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_be_bytes()).collect()
}

/// A length or count as the `uint32` a table stores it in.
fn count32(n: usize) -> u32 {
    u32::try_from(n).expect("a test table under 4 GiB")
}

/// A length or count as a `uint16`.
fn count16(n: usize) -> u16 {
    u16::try_from(n).expect("a test table under 64 KiB")
}

// --- OpenType Layout tables, for the faces that carry one beside AAT ------
//
// In `tests/synthetic.rs`'s register: a layout table is a header of counts
// and `Offset16`s followed by what they point at, so `assemble` appends the
// parts and patches each slot with where its part landed.

/// A header, with `slots[i]` patched to the offset of `parts[i]`.
fn assemble(mut head: Vec<u8>, slots: &[usize], parts: &[Vec<u8>]) -> Vec<u8> {
    assert_eq!(slots.len(), parts.len(), "a slot for every part");
    let mut body: Vec<u8> = Vec::new();
    for (slot, part) in slots.iter().zip(parts) {
        let offset = count16(head.len() + body.len());
        head[*slot..*slot + 2].copy_from_slice(&offset.to_be_bytes());
        body.extend_from_slice(part);
    }
    head.extend(body);
    head
}

/// Coverage format 1: the glyphs, sorted, as the format requires.
fn coverage(glyphs: &[u16]) -> Vec<u8> {
    let mut sorted = glyphs.to_vec();
    sorted.sort_unstable();
    let mut out = u16s(&[1, count16(sorted.len())]);
    out.extend(u16s(&sorted));
    out
}

/// One lookup of `kind`, its subtables in order.
fn lookup(kind: u16, subtables: &[Vec<u8>]) -> Vec<u8> {
    let mut head = u16s(&[kind, 0, count16(subtables.len())]);
    head.extend(vec![0u8; subtables.len() * 2]);
    let slots: Vec<usize> = (0..subtables.len()).map(|i| 6 + i * 2).collect();
    assemble(head, &slots, subtables)
}

/// `LigatureSubstFormat1` of one ligature: `first` then `rest` become `into`.
fn ligature_subst(first: u16, rest: &[u16], into: u16) -> Vec<u8> {
    let mut ligature = u16s(&[into, count16(rest.len() + 1)]);
    ligature.extend(u16s(rest));
    let set = assemble(u16s(&[1, 0]), &[2], &[ligature]);
    assemble(u16s(&[1, 0, 1, 0]), &[2, 6], &[coverage(&[first]), set])
}

/// `PairPosFormat1` of one pair, `first` then `second`, the first glyph's
/// advance changed by `x_advance`.
fn pair_kern(first: u16, second: u16, x_advance: i16) -> Vec<u8> {
    let mut set = u16s(&[1, second]);
    set.extend(x_advance.to_be_bytes());
    assemble(
        u16s(&[1, 0, 0x0004, 0, 1, 0]),
        &[2, 10],
        &[coverage(&[first]), set],
    )
}

/// A `GSUB` or `GPOS`: `DFLT` and `latn`, each default language system
/// naming every feature, and each feature naming the lookups given.
fn layout_table(features: &[([u8; 4], Vec<u16>)], lookups: &[Vec<u8>]) -> Vec<u8> {
    let scripts = [*b"DFLT", *b"latn"];
    let feature_count = count16(features.len());
    let mut lang_sys = u16s(&[0, 0xFFFF, feature_count]);
    lang_sys.extend(u16s(&(0..feature_count).collect::<Vec<u16>>()));
    let script = assemble(u16s(&[0, 0]), &[0], &[lang_sys]);

    let mut script_head = u16s(&[count16(scripts.len())]);
    let mut script_slots = Vec::new();
    for tag in &scripts {
        script_head.extend_from_slice(tag);
        script_slots.push(script_head.len());
        script_head.extend(u16s(&[0]));
    }
    let script_list = assemble(script_head, &script_slots, &vec![script; scripts.len()]);

    let mut feature_head = u16s(&[feature_count]);
    let mut feature_slots = Vec::new();
    let mut feature_tables = Vec::new();
    for (tag, indices) in features {
        feature_head.extend_from_slice(tag);
        feature_slots.push(feature_head.len());
        feature_head.extend(u16s(&[0]));
        let mut table = u16s(&[0, count16(indices.len())]);
        table.extend(u16s(indices));
        feature_tables.push(table);
    }
    let feature_list = assemble(feature_head, &feature_slots, &feature_tables);

    let mut list_head = u16s(&[count16(lookups.len())]);
    list_head.extend(vec![0u8; lookups.len() * 2]);
    let slots: Vec<usize> = (0..lookups.len()).map(|i| 2 + i * 2).collect();
    let lookup_list = assemble(list_head, &slots, lookups);

    assemble(
        u16s(&[1, 0, 0, 0, 0]),
        &[4, 6, 8],
        &[script_list, feature_list, lookup_list],
    )
}

// --- AAT tables ----------------------------------------------------------
//
// Read by the manual's own layouts. A lookup table (*AAT special tables*,
// "Lookup Tables") is a `uint16` format and the format's header; formats 2,
// 4 and 6 open with a `BinSrchHeader` — `unitSize`, `nUnits` and the three
// search fields the chapter says "are no longer used", written here as it
// defines them anyway — and end with a guard unit of `0xFFFF` that `nUnits`
// does not count. A `morx` (*The 'morx' table*, "Table Format") is a version,
// an unused word and a chain count, then its chains: `defaultFlags`,
// `chainLength`, `nFeatureEntries` and `nSubtables`, the feature entries of
// twelve bytes each, and the subtables, each a `length`, a `coverage` and
// `subFeatureFlags` before its body.

/// The three `BinSrchHeader` search fields for `units` units of `size`
/// bytes: the size times the largest power of two not above the count, that
/// power's logarithm, and the size times what is left over.
fn search_fields(size: u16, units: u16) -> [u16; 3] {
    let mut power = 1u16;
    let mut log = 0u16;
    while units > 0 && power <= units / 2 {
        power *= 2;
        log += 1;
    }
    if units == 0 {
        return [0, 0, 0];
    }
    [size * power, log, size * (units - power)]
}

/// A format 6 lookup table, single table: `(glyph, value)` pairs, sorted,
/// with the guard.
fn lookup_single(pairs: &[(u16, u16)]) -> Vec<u8> {
    let mut sorted = pairs.to_vec();
    sorted.sort_unstable();
    let units = count16(sorted.len());
    let [range, selector, shift] = search_fields(4, units);
    let mut out = u16s(&[6, 4, units, range, selector, shift]);
    for (glyph, value) in &sorted {
        out.extend(u16s(&[*glyph, *value]));
    }
    out.extend(u16s(&[0xFFFF, 0]));
    out
}

/// A `morx` table of `version` holding `chains`.
fn morx_table(version: u16, chains: &[Vec<u8>]) -> Vec<u8> {
    let mut out = u16s(&[version, 0]);
    out.extend(count32(chains.len()).to_be_bytes());
    for chain in chains {
        out.extend_from_slice(chain);
    }
    out
}

/// One feature entry: a feature type and setting, and the flags it enables
/// and the complement of those it disables.
type FeatureEntry = (u16, u16, u32, u32);

/// One `morx` chain: its default flags, its feature entries in table order,
/// and its subtables.
fn chain(default_flags: u32, features: &[FeatureEntry], subtables: &[Vec<u8>]) -> Vec<u8> {
    let mut body: Vec<u8> = Vec::new();
    for (kind, setting, enable, disable) in features {
        body.extend(u16s(&[*kind, *setting]));
        body.extend(u32s(&[*enable, *disable]));
    }
    for subtable in subtables {
        body.extend_from_slice(subtable);
    }
    let mut out = u32s(&[
        default_flags,
        count32(16 + body.len()),
        count32(features.len()),
        count32(subtables.len()),
    ]);
    out.extend(body);
    out
}

/// One `morx` subtable: its `coverage`, whose low byte is its type, its
/// sub-feature flags and its body, padded to four bytes so that the chain's
/// length stays the multiple of four the chapter requires.
fn subtable(coverage: u32, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut padded = body.to_vec();
    while padded.len() % 4 != 0 {
        padded.push(0);
    }
    let mut out = u32s(&[count32(12 + padded.len()), coverage, flags]);
    out.extend(padded);
    out
}

/// A noncontextual subtable for horizontal and vertical text alike, under
/// sub-feature flags `flags`, mapping `pairs`.
fn noncontextual(flags: u32, pairs: &[(u16, u16)]) -> Vec<u8> {
    subtable(0x2000_0004, flags, &lookup_single(pairs))
}

// --- faces ---------------------------------------------------------------

/// A face described by what it carries; [`Face::build`] writes the sfnt.
#[derive(Clone, Debug, Default)]
struct Face {
    /// Units per em.
    em: u16,
    /// Each glyph's horizontal advance, glyph 0 being `.notdef`.
    advances: Vec<u16>,
    /// Each character's glyph.
    cmap: Vec<(char, u16)>,
    /// Every further table, by its tag.
    tables: Vec<([u8; 4], Vec<u8>)>,
}

impl Face {
    /// A face of `glyphs` glyphs at 1000 units per em, glyph `n` `500 + n`
    /// units wide so that every advance says which glyph it is, mapping
    /// `cmap`, with no other table.
    fn new(glyphs: u16, cmap: &[(char, u16)]) -> Face {
        Face {
            em: 1000,
            advances: (0..glyphs).map(|glyph| 500 + glyph).collect(),
            cmap: cmap.to_vec(),
            tables: Vec::new(),
        }
    }

    /// The same face with `table` added, or put in place of one of its tag.
    fn with(mut self, tag: &[u8; 4], table: Vec<u8>) -> Face {
        self.tables.retain(|(t, _)| t != tag);
        self.tables.push((*tag, table));
        self
    }

    /// The same face without the table of `tag`.
    fn without(mut self, tag: &[u8; 4]) -> Face {
        self.tables.retain(|(t, _)| t != tag);
        self
    }

    fn glyph(&self, c: char) -> u16 {
        self.cmap
            .iter()
            .find(|(of, _)| *of == c)
            .map(|(_, glyph)| *glyph)
            .expect("a character the face maps")
    }

    /// The glyph as a face with no layout tables gives it: its `hmtx`
    /// advance and nothing else.
    fn plain(&self, glyph: u16, cluster: u32) -> ShapedGlyph {
        ShapedGlyph {
            glyph,
            cluster,
            x_advance: i32::from(self.advances[usize::from(glyph)]),
            ..ShapedGlyph::default()
        }
    }

    /// The sfnt: `head`, `hhea`, `hmtx` and `cmap`, and the face's further
    /// tables, in tag order.
    fn build(&self) -> Vec<u8> {
        let glyphs = count16(self.advances.len());
        let mut tables: Vec<([u8; 4], Vec<u8>)> = Vec::new();

        let mut head = vec![0u8; 54];
        head[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
        head[12..16].copy_from_slice(&0x5F0F_3CF5u32.to_be_bytes());
        head[18..20].copy_from_slice(&self.em.to_be_bytes());
        tables.push((*b"head", head));

        let mut hhea = vec![0u8; 36];
        hhea[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
        hhea[34..36].copy_from_slice(&glyphs.to_be_bytes());
        tables.push((*b"hhea", hhea));
        let hmtx: Vec<u8> = self
            .advances
            .iter()
            .flat_map(|advance| [advance.to_be_bytes(), 0u16.to_be_bytes()].concat())
            .collect();
        tables.push((*b"hmtx", hmtx));

        let mut sorted = self.cmap.clone();
        sorted.sort_unstable();
        let groups = count32(sorted.len());
        let mut sub = u16s(&[12, 0]);
        sub.extend((16 + groups * 12).to_be_bytes());
        sub.extend(0u32.to_be_bytes());
        sub.extend(groups.to_be_bytes());
        for (c, glyph) in &sorted {
            sub.extend(u32::from(*c).to_be_bytes());
            sub.extend(u32::from(*c).to_be_bytes());
            sub.extend(u32::from(*glyph).to_be_bytes());
        }
        let mut cmap = u16s(&[0, 1, 3, 10]);
        cmap.extend(12u32.to_be_bytes());
        cmap.extend(sub);
        tables.push((*b"cmap", cmap));

        tables.extend(self.tables.iter().cloned());
        tables.sort_by_key(|(tag, _)| *tag);
        directory(&tables)
    }
}

/// An sfnt table directory of `tables`, in the order given, each table
/// padded to four bytes as the format asks.
fn directory(tables: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(0x0001_0000u32.to_be_bytes());
    out.extend(u16s(&[count16(tables.len()), 0, 0, 0]));
    let mut offset = 12 + tables.len() * 16;
    let mut body = Vec::new();
    for (tag, table) in tables {
        out.extend_from_slice(tag);
        out.extend(0u32.to_be_bytes());
        out.extend(count32(offset).to_be_bytes());
        out.extend(count32(table.len()).to_be_bytes());
        body.extend_from_slice(table);
        while body.len() % 4 != 0 {
            body.push(0);
        }
        offset = 12 + tables.len() * 16 + body.len();
    }
    out.extend(body);
    out
}

/// The runs `text` shapes to along a line, through `shaper`.
fn line_runs(shaper: &Shaper<'_>, text: &str) -> Vec<ShapedRun> {
    shaper.shape_text(text, BaseDirection::Auto).1
}

/// Every glyph of every run, in order.
fn glyphs(runs: &[ShapedRun]) -> Vec<ShapedGlyph> {
    runs.iter()
        .flat_map(|run| run.glyphs().iter().copied())
        .collect()
}

/// `text`'s byte offset of each character, as the clusters a run carries.
fn clusters(text: &str) -> Vec<u32> {
    text.char_indices()
        .map(|(at, _)| u32::try_from(at).expect("a short text"))
        .collect()
}

// --- the refusal (FT-07c.2) ----------------------------------------------
//
// A face whose layout is AAT's is shaped as a face with no layout tables is
// until its `morx` is run, and every run says so once: `Warning::TableNotRun`
// naming `morx` (ruling 10; ruling 3's refusal by name), or `mort` for a face
// carrying the predecessor table alone.

/// The swash `x` the AAT face's chain maps its `x` to: the last of its eight
/// glyphs, `.notdef`, `f`, `i`, `x`, an `fi`, `α`, `β` and the swash.
const SWASH_X: u16 = 7;

/// A Latin face with an `f`, an `i`, an `x` and an `fi`, and a Greek alpha
/// and beta, carrying a `morx` whose one chain maps `x` to its swash and no
/// OpenType Layout table.
fn aat_face() -> Face {
    let face = Face::new(8, &[('f', 1), ('i', 2), ('x', 3), ('α', 5), ('β', 6)]);
    let x = face.glyph('x');
    face.with(
        b"morx",
        morx_table(2, &[chain(1, &[], &[noncontextual(1, &[(x, SWASH_X)])])]),
    )
}

/// `f` and `i` ligated to glyph 4 under `liga`.
fn fi_ligature() -> Vec<u8> {
    layout_table(
        &[(*b"liga", vec![0])],
        &[lookup(4, &[ligature_subst(1, &[2], 4)])],
    )
}

/// The refusal a run over a face carrying `table` alone records.
fn not_run(table: &[u8; 4]) -> Warning {
    Warning::TableNotRun {
        table: Tag::new(table),
    }
}

/// How many of `warnings` name `table` as not run.
fn refusals(warnings: &[Warning], table: &[u8; 4]) -> usize {
    warnings.iter().filter(|w| **w == not_run(table)).count()
}

/// A face carrying `morx` and no `GSUB` shapes as the face with no layout
/// tables does, its `x` left as `cmap` gives it, and its run says so once;
/// given a `GSUB`, the same face shapes through it and names nothing.
#[test]
fn a_face_carrying_only_morx_is_named_and_shaped_unshaped() {
    let face = aat_face();
    let text = "fix";
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let runs = line_runs(&Shaper::new(&sfnt), text);
    assert_eq!(runs.len(), 1, "one Latin run");

    // The `cmap`-only run: the same face with no `morx` at all.
    let plain_face = face.clone().without(b"morx");
    let plain_bytes = plain_face.build();
    let plain_sfnt = Sfnt::parse(&plain_bytes).expect("the plain face parses");
    let plain = line_runs(&Shaper::new(&plain_sfnt), text);
    assert!(plain.iter().all(|run| run.warnings().is_empty()));

    let expected: Vec<ShapedGlyph> = text
        .chars()
        .zip(clusters(text))
        .map(|(c, cluster)| face.plain(face.glyph(c), cluster))
        .collect();
    assert_eq!(
        glyphs(&runs),
        expected,
        "the cmap-only glyphs, the x unswashed"
    );
    assert_eq!(glyphs(&runs), glyphs(&plain), "the face without its morx");
    assert_eq!(runs[0].warnings(), &[not_run(b"morx")], "named once");

    // The same face with a `GSUB` beside its `morx` takes the OpenType
    // path: the ligature forms, the `morx` stays unread, and nothing names it.
    let with_gsub = face.with(b"GSUB", fi_ligature());
    let gsub_bytes = with_gsub.build();
    let gsub_sfnt = Sfnt::parse(&gsub_bytes).expect("the face with a GSUB parses");
    let shaped = line_runs(&Shaper::new(&gsub_sfnt), text);
    assert_eq!(
        glyphs(&shaped)
            .iter()
            .map(|g| g.glyph)
            .collect::<Vec<u16>>(),
        vec![4, with_gsub.glyph('x')],
        "the GSUB's ligature, and no swash"
    );
    assert!(
        shaped.iter().all(|run| run.warnings().is_empty()),
        "nothing names morx: {:?}",
        shaped.iter().map(ShapedRun::warnings).collect::<Vec<_>>()
    );
}

/// Every run of a paragraph carries the refusal, each once however many
/// glyphs it holds, whatever features the caller asks for; a run of no
/// glyphs is still a run shaped without the face's chain.
#[test]
fn every_run_over_a_morx_only_face_is_named_once() {
    let face = aat_face();
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let text = "fixfixfix αβαβ fi";
    let none: [Tag; 0] = [];
    let liga = [Tag::new(b"liga")];
    let off = [(Tag::new(b"liga"), 0u32), (Tag::new(b"smcp"), 1)];
    let shapers = [
        Shaper::new(&sfnt),
        Shaper::new(&sfnt).with_features(&none, &none),
        Shaper::new(&sfnt).with_features(&liga, &none),
        Shaper::new(&sfnt).with_settings(&off),
    ];
    for shaper in &shapers {
        let runs = line_runs(shaper, text);
        assert!(runs.len() >= 2, "a Latin and a Greek run: {}", runs.len());
        for run in &runs {
            assert_eq!(run.warnings(), &[not_run(b"morx")], "run {:?}", run.text());
        }
        // Unshaped: one glyph a character, each as `cmap` maps it.
        let mapped: Vec<u16> = text
            .chars()
            .map(|c| {
                face.cmap
                    .iter()
                    .find(|(of, _)| *of == c)
                    .map_or(0, |(_, glyph)| *glyph)
            })
            .collect();
        assert_eq!(
            glyphs(&runs).iter().map(|g| g.glyph).collect::<Vec<u16>>(),
            mapped
        );
    }
    let empty = Run {
        text: 2..2,
        script: Script::Latin,
        level: Level::LTR,
        orientation: Orientation::Horizontal,
    };
    let shaped = Shaper::new(&sfnt).shape(text, &empty);
    assert!(shaped.glyphs().is_empty());
    assert_eq!(shaped.warnings(), &[not_run(b"morx")]);
}

/// A right-to-left run and a column of the face go without the chain too,
/// and each of their runs says so once, beside whatever else it says.
#[test]
fn a_right_to_left_run_and_a_column_over_a_morx_only_face_are_named_too() {
    let face = aat_face();
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let (_, runs) = Shaper::new(&sfnt).shape_text("\u{5D0}\u{5D1}", BaseDirection::Auto);
    assert_eq!(runs.len(), 1);
    assert!(!runs[0].direction().is_forward(), "a right-to-left run");
    assert_eq!(runs[0].warnings(), &[not_run(b"morx")]);

    // Latin stands sideways in a column and Han upright, so the column is
    // two runs, one of each.
    let runs = Shaper::new(&sfnt)
        .shape_text_vertical("fi\u{6F22}", BaseDirection::Auto)
        .1;
    let orientations: Vec<Orientation> = runs.iter().map(ShapedRun::orientation).collect();
    assert_eq!(
        orientations,
        vec![Orientation::Sideways, Orientation::Upright]
    );
    for run in &runs {
        assert_eq!(refusals(run.warnings(), b"morx"), 1, "{:?}", run.warnings());
    }
    // The upright run also says its metrics were synthesized, the face
    // having no `vmtx`: the refusal sits beside the column's own warning.
    assert!(runs[1]
        .warnings()
        .contains(&Warning::VerticalMetricsSynthesized));
}

/// A face carrying `mort` alone — the table `morx` extends, which no row
/// reads — is named by it; a face carrying both is named by its `morx`
/// alone, the manual having a face with both ignore its `mort`.
#[test]
fn a_mort_only_face_is_named() {
    // A `mort` header: version 1.0 as a `Fixed`, and no chains.
    let mort = [0u8, 1, 0, 0, 0, 0, 0, 0].to_vec();
    let face = aat_face().without(b"morx").with(b"mort", mort.clone());
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let runs = line_runs(&Shaper::new(&sfnt), "fix");
    assert_eq!(runs[0].warnings(), &[not_run(b"mort")]);
    assert_eq!(
        glyphs(&runs).iter().map(|g| g.glyph).collect::<Vec<u16>>(),
        vec![1, 2, 3]
    );

    let both = aat_face().with(b"mort", mort);
    let bytes = both.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let runs = line_runs(&Shaper::new(&sfnt), "fix");
    assert_eq!(runs[0].warnings(), &[not_run(b"morx")], "mort ignored");
}

/// A `GPOS` beside the `morx` is an OpenType Layout table too: the face
/// takes the OpenType path, its kerning applied and its `morx` unread, and
/// nothing names the `morx`.
#[test]
fn a_face_carrying_gpos_beside_its_morx_takes_the_opentype_path() {
    let face = aat_face().with(
        b"GPOS",
        layout_table(
            &[(*b"kern", vec![0])],
            &[lookup(2, &[pair_kern(1, 3, -40)])],
        ),
    );
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let runs = line_runs(&Shaper::new(&sfnt), "fx");
    let shaped = glyphs(&runs);
    assert_eq!(
        shaped.iter().map(|g| g.glyph).collect::<Vec<u16>>(),
        vec![1, 3],
        "no swash: the morx is not the path"
    );
    assert_eq!(
        shaped[0].x_advance,
        i32::from(face.advances[1]) - 40,
        "the pair kerned"
    );
    assert!(runs.iter().all(|run| run.warnings().is_empty()));

    // So does a `mort` beside a `GPOS`.
    let mort_face = face
        .without(b"morx")
        .with(b"mort", [0u8, 1, 0, 0, 0, 0, 0, 0].to_vec());
    let bytes = mort_face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let runs = line_runs(&Shaper::new(&sfnt), "fx");
    assert!(runs.iter().all(|run| run.warnings().is_empty()));
}

/// A `GSUB` or `GPOS` this crate cannot read is no OpenType path to take:
/// the face's only readable layout table is still its `morx`, and it is
/// named.
#[test]
fn an_unreadable_opentype_table_leaves_the_face_aat_only() {
    for tag in [b"GSUB", b"GPOS"] {
        let mut unreadable = fi_ligature();
        unreadable[0..2].copy_from_slice(&2u16.to_be_bytes());
        let face = aat_face().with(tag, unreadable);
        let bytes = face.build();
        let sfnt = Sfnt::parse(&bytes).expect("the face parses");
        let runs = line_runs(&Shaper::new(&sfnt), "fix");
        assert_eq!(
            glyphs(&runs).iter().map(|g| g.glyph).collect::<Vec<u16>>(),
            vec![1, 2, 3],
            "nothing ligated beside an unreadable {tag:?}"
        );
        assert!(runs.iter().all(|run| run.warnings() == [not_run(b"morx")]));
    }
}

/// A `Silf` beside a `morx` leaves the face AAT's, named by its `morx` and
/// not by its `Silf`; a `Silf` beside a `mort` is the Graphite description
/// that names the face, and the `mort` is not named beside it.
#[test]
fn a_silf_beside_an_aat_table_is_named_once() {
    // `Silf` 4.0 with no descriptions: present, which is all the precedence
    // reads.
    let mut silf = 0x0004_0000u32.to_be_bytes().to_vec();
    silf.extend(0x0004_0002u32.to_be_bytes());
    silf.extend(u16s(&[0, 0]));

    let face = aat_face().with(b"Silf", silf.clone());
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let runs = line_runs(&Shaper::new(&sfnt), "fix");
    assert_eq!(runs[0].warnings(), &[not_run(b"morx")]);

    let face = aat_face()
        .without(b"morx")
        .with(b"mort", [0u8, 1, 0, 0, 0, 0, 0, 0].to_vec())
        .with(b"Silf", silf);
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let runs = line_runs(&Shaper::new(&sfnt), "fix");
    assert_eq!(runs[0].warnings(), &[not_run(b"Silf")]);
}

// --- the shared machinery (FT-07c.4) -------------------------------------
//
// Lookup tables in each format, extended state tables, and the driver every
// machine runs on, read as *AAT special tables* defines them.

/// `value` as `size` big-endian bytes.
fn sized(value: u64, size: u16) -> Vec<u8> {
    let bytes = value.to_be_bytes();
    bytes[8 - usize::from(size)..].to_vec()
}

/// A value of `size` bytes with every byte set and none of them a guard's
/// `0xFF`.
fn large(size: u16) -> u64 {
    match size {
        1 => 0xAB,
        2 => 0xABCD,
        4 => 0xABCD_EF01,
        _ => 0xABCD_EF01_2345_6789,
    }
}

/// A format 0 lookup table: one two-byte value a glyph, from glyph 0.
fn lookup_simple(values: &[u16]) -> Vec<u8> {
    let mut out = u16s(&[0]);
    out.extend(u16s(values));
    out
}

/// A format 2 lookup table: segments `(first, last, value)`, each sharing
/// one value of `size` bytes, sorted by last glyph, with the guard.
fn lookup_segments(size: u16, segments: &[(u16, u16, u64)]) -> Vec<u8> {
    let unit = 4 + size;
    let mut sorted = segments.to_vec();
    sorted.sort_by_key(|(_, last, _)| *last);
    let units = count16(sorted.len());
    let [range, selector, shift] = search_fields(unit, units);
    let mut out = u16s(&[2, unit, units, range, selector, shift]);
    for (first, last, value) in &sorted {
        out.extend(u16s(&[*last, *first]));
        out.extend(sized(*value, size));
    }
    out.extend(u16s(&[0xFFFF, 0xFFFF]));
    out.extend(sized(0, size));
    out
}

/// A format 4 lookup table: segments `(first, values)`, in glyph order,
/// each with a value a glyph in an array after the guard, at the offset the
/// segment states from the start of the table.
fn lookup_segment_array(segments: &[(u16, Vec<u16>)]) -> Vec<u8> {
    let units = count16(segments.len());
    let [range, selector, shift] = search_fields(6, units);
    let header = 12 + 6 * (segments.len() + 1);
    let mut out = u16s(&[4, 6, units, range, selector, shift]);
    let mut arrays: Vec<u8> = Vec::new();
    for (first, values) in segments {
        let last = first + count16(values.len()) - 1;
        let offset = count16(header + arrays.len());
        out.extend(u16s(&[last, *first, offset]));
        arrays.extend(u16s(values));
    }
    out.extend(u16s(&[0xFFFF, 0xFFFF, 0]));
    out.extend(arrays);
    out
}

/// A format 6 lookup table: `(glyph, value)` pairs with values of `size`
/// bytes, sorted, with the guard.
fn lookup_pairs(size: u16, pairs: &[(u16, u64)]) -> Vec<u8> {
    let unit = 2 + size;
    let mut sorted = pairs.to_vec();
    sorted.sort_unstable();
    let units = count16(sorted.len());
    let [range, selector, shift] = search_fields(unit, units);
    let mut out = u16s(&[6, unit, units, range, selector, shift]);
    for (glyph, value) in &sorted {
        out.extend(u16s(&[*glyph]));
        out.extend(sized(*value, size));
    }
    out.extend(u16s(&[0xFFFF]));
    out.extend(sized(0, size));
    out
}

/// A format 8 lookup table: two-byte values from glyph `first`.
fn lookup_trimmed(first: u16, values: &[u16]) -> Vec<u8> {
    let mut out = u16s(&[8, first, count16(values.len())]);
    out.extend(u16s(values));
    out
}

/// A format 10 lookup table: values of `size` bytes from glyph `first`.
fn lookup_extended(size: u16, first: u16, values: &[u64]) -> Vec<u8> {
    let mut out = u16s(&[10, size, first, count16(values.len())]);
    for value in values {
        out.extend(sized(*value, size));
    }
    out
}

/// Pads `bytes` to a multiple of four, as a state table's parts are.
fn pad4(bytes: &mut Vec<u8>) {
    while bytes.len() % 4 != 0 {
        bytes.push(0);
    }
}

/// Writes `value` as a `uint32` at `at`.
fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
}

/// An extended state table: an `STXHeader` of `classes` and the offsets of
/// its three parts, then `words` more header words, zero, for the subtable
/// that embeds it to fill, then the class lookup table, the state array —
/// one line of entry indices a state — and the entry table, each padded to
/// four bytes.
fn stx(
    classes: u32,
    words: usize,
    class_table: &[u8],
    states: &[&[u16]],
    entries: &[&[u16]],
) -> Vec<u8> {
    let mut out = vec![0u8; 16 + 4 * words];
    put32(&mut out, 0, classes);
    let class_at = out.len();
    out.extend_from_slice(class_table);
    pad4(&mut out);
    let state_at = out.len();
    for line in states {
        out.extend(u16s(line));
    }
    pad4(&mut out);
    let entry_at = out.len();
    for entry in entries {
        out.extend(u16s(entry));
    }
    pad4(&mut out);
    put32(&mut out, 4, count32(class_at));
    put32(&mut out, 8, count32(state_at));
    put32(&mut out, 12, count32(entry_at));
    out
}

/// Where the tests below say a refusal happens.
fn site(subtable: u32) -> Site {
    Site {
        table: Tag::new(b"morx"),
        chain: 0,
        subtable,
    }
}

/// One lookup table of the tests below: its format, its bytes, and the
/// value each glyph it covers answers.
type LookupCase = (LookupFormat, Vec<u8>, BTreeMap<u16, u64>);

/// Every lookup table the tests below read.
fn lookup_cases() -> Vec<LookupCase> {
    let mut cases = Vec::new();

    // Format 0: as long as its table, so glyph 6 on answers nothing; a value
    // of `0xFFFF` is a value here, there being no search to guard.
    let values = [7u16, 0, 9, 0xFFFF, 3, 1];
    cases.push((
        LookupFormat::SimpleArray,
        lookup_simple(&values),
        (0u16..)
            .zip(values)
            .map(|(glyph, value)| (glyph, u64::from(value)))
            .collect(),
    ));

    // Format 2, at every value size: segments of one glyph and of many, a
    // gap between two, and fifty more to give the search some depth.
    for size in [1u16, 2, 4, 8] {
        let mut segments = vec![(10u16, 19u16, 1u64), (25, 25, 2), (30, 40, large(size))];
        for n in 0..50u16 {
            segments.push((1000 + n * 10, 1000 + n * 10 + 4, u64::from(n % 7)));
        }
        let expected = segments
            .iter()
            .flat_map(|(first, last, value)| (*first..=*last).map(move |glyph| (glyph, *value)))
            .collect();
        cases.push((
            LookupFormat::SegmentSingle,
            lookup_segments(size, &segments),
            expected,
        ));
    }

    // Format 4: a value a glyph, a zero among them, which is a value.
    let segments = vec![
        (50u16, vec![500u16, 501, 502]),
        (60, vec![600]),
        (70, vec![0, 701]),
        (0xFFF0, vec![1, 2]),
    ];
    let expected = segments
        .iter()
        .flat_map(|(first, values)| {
            (*first..)
                .zip(values.clone())
                .map(|(glyph, value)| (glyph, u64::from(value)))
        })
        .collect();
    cases.push((
        LookupFormat::SegmentArray,
        lookup_segment_array(&segments),
        expected,
    ));

    // Format 6, at every value size: a hundred pairs, and one at the top of
    // the glyph range below the guard.
    for size in [1u16, 2, 4, 8] {
        let mut pairs: Vec<(u16, u64)> =
            (0..100u16).map(|n| (n * 2 + 3, u64::from(n) + 1)).collect();
        pairs.push((0xFFFE, large(size)));
        let expected = pairs.iter().copied().collect();
        cases.push((
            LookupFormat::SingleTable,
            lookup_pairs(size, &pairs),
            expected,
        ));
    }

    // Format 8: glyphs 100 to 103.
    let values = [1u16, 0, 3, 0xFFFF];
    cases.push((
        LookupFormat::TrimmedArray,
        lookup_trimmed(100, &values),
        (100u16..)
            .zip(values)
            .map(|(glyph, value)| (glyph, u64::from(value)))
            .collect(),
    ));

    // Format 10, at every unit size, and up to the last glyph index.
    for size in [1u16, 2, 4, 8] {
        let values = [large(size), 0, 5];
        cases.push((
            LookupFormat::ExtendedTrimmedArray,
            lookup_extended(size, 200, &values),
            (200u16..).zip(values).collect(),
        ));
        cases.push((
            LookupFormat::ExtendedTrimmedArray,
            lookup_extended(size, 0xFFFD, &values),
            (0xFFFDu16..=0xFFFF).zip(values).collect(),
        ));
    }
    cases
}

/// Each format answers every glyph it covers with its value and every other
/// glyph with nothing, every glyph index asked.
#[test]
fn every_lookup_format_answers_the_glyphs_it_covers_and_no_other() {
    let cases = lookup_cases();
    let formats: Vec<u16> = cases.iter().map(|(format, _, _)| format.number()).collect();
    for number in [0u16, 2, 4, 6, 8, 10] {
        assert!(formats.contains(&number), "format {number} is held");
    }
    for (format, table, expected) in &cases {
        let lookup = Lookup::parse(table).expect("a readable lookup table");
        assert_eq!(lookup.format(), *format);
        for glyph in 0..=u16::MAX {
            let want = expected.get(&glyph).copied();
            assert_eq!(lookup.value(glyph), want, "{format:?}, glyph {glyph}");
            assert_eq!(
                lookup.value16(glyph),
                want.and_then(|value| u16::try_from(value).ok()),
                "{format:?}, glyph {glyph}, as sixteen bits"
            );
        }
    }
    // A format the manual does not define is named, and a header cut short
    // is no table.
    assert_eq!(
        Lookup::parse(&u16s(&[3, 0, 0])).err(),
        Some(LookupError::Format(3))
    );
    assert_eq!(Lookup::parse(&[0x00]).err(), Some(LookupError::Malformed));
}

/// A state table reads its class lookup, its state array a line a state and
/// its entries a field at a time, and answers nothing for a class with no
/// column, a state line or an entry it does not hold.
#[test]
fn a_state_table_reads_its_classes_states_and_entries() {
    // Seven classes; glyphs 10 to 12 in classes 4 to 6, and glyph 13 in a
    // class the state array has no column for.
    let classes = lookup_single(&[(10, 4), (11, 5), (12, 6), (13, 9)]);
    let lines: [&[u16]; 3] = [
        &[0, 1, 0, 0, 2, 3, 0],
        &[0, 1, 0, 0, 2, 3, 0],
        &[4, 1, 0, 0, 5, 5, 6],
    ];
    // Entries of eight bytes: new state, flags and two fields, as a
    // contextual subtable's are.
    let entries: [&[u16]; 7] = [
        &[0, 0, 0xFFFF, 0xFFFF],
        &[0, 0x4000, 0xFFFF, 0xFFFF],
        &[2, 0x8000, 0xFFFF, 0xFFFF],
        &[2, 0, 0, 1],
        &[0, 0, 1, 0xFFFF],
        &[2, 0xC000, 0xFFFF, 0],
        &[0, 0x2000, 3, 4],
    ];
    let bytes = stx(7, 1, &classes, &lines, &entries);
    let table = StateTable::parse(&bytes, 8).expect("a readable state table");
    assert_eq!(table.class_count(), 7);
    assert_eq!(table.entry_size(), 8);

    assert_eq!(table.class(10), 4);
    assert_eq!(table.class(11), 5);
    assert_eq!(table.class(12), 6);
    assert_eq!(table.class(13), 9);
    for glyph in [0u16, 9, 14, 500, 0xFFFE] {
        assert_eq!(table.class(glyph), OUT_OF_BOUNDS, "glyph {glyph}");
    }
    assert_eq!(table.class(DELETED), DELETED_GLYPH);

    for (state, line) in (0u16..).zip(lines) {
        for (class, index) in (0u32..).zip(line.iter()) {
            assert_eq!(
                table.entry_index(state, class),
                Some(*index),
                "state {state}, class {class}"
            );
        }
        assert_eq!(table.entry_index(state, 7), None, "no eighth column");
        assert_eq!(table.entry_index(state, 9), None, "no tenth column");
    }
    assert_eq!(table.entry_index(1000, END_OF_TEXT), None, "no such line");

    for (index, fields) in (0u16..).zip(entries) {
        let entry: Entry<'_> = table.entry(index).expect("an entry");
        assert_eq!(entry.new_state, fields[0]);
        assert_eq!(entry.flags, fields[1]);
        assert_eq!(entry.field(0), Some(fields[2]));
        assert_eq!(entry.field(1), Some(fields[3]));
        assert_eq!(entry.field(2), None, "an entry holds two fields");
        assert_eq!(entry.dont_advance(), fields[1] & 0x4000 != 0);
    }
    assert_eq!(table.entry(7), None, "the table holds seven entries");
    assert_eq!(table.transition(2, 4), table.entry(5));
    assert_eq!(table.transition(0, 9), None);

    // The same bytes read with four-byte entries give each entry's halves.
    let narrow = StateTable::parse(&bytes, 4).expect("readable");
    let second = narrow.entry(1).expect("an entry");
    assert_eq!((second.new_state, second.flags), (0xFFFF, 0xFFFF));
    assert_eq!(second.field(0), None);
}

/// Every transition the machine takes, as `(glyph, end of text)`.
type Walk = Vec<(Option<usize>, bool)>;

/// Runs `table` over `glyphs` the `traversal` way under `limits`, its actions
/// recording each transition and doing nothing else.
fn walk(
    table: &StateTable<'_>,
    glyphs: &[u16],
    traversal: Traversal,
    limits: &Limits,
) -> (Result<(), Warning>, Walk) {
    let mut buffer = Buffer::from_glyphs(glyphs);
    let mut seen: Walk = Vec::new();
    let mut record = |_: &mut Buffer, _: &Entry<'_>, cursor: &mut Cursor| -> Result<(), Warning> {
        seen.push((cursor.glyph, cursor.end_of_text));
        Ok(())
    };
    let result = table.run(&mut buffer, traversal, limits, site(0), &mut record);
    (result, seen)
}

/// A machine that holds its position on glyphs 10 and 11, both class 4;
/// every other glyph advances.
fn holding_machine() -> Vec<u8> {
    stx(
        5,
        0,
        &lookup_single(&[(10, 4), (11, 4)]),
        &[&[0, 0, 0, 0, 1], &[0, 0, 0, 0, 1]],
        &[&[0, 0], &[0, 0x4000]],
    )
}

/// A machine whose entries hold their position for ever stops at
/// `Limits::max_transitions`, with a typed warning naming its subtable and
/// the buffer as it stood: every action taken before the ceiling kept, and
/// no end of text taken.
#[test]
fn a_machine_that_never_advances_stops_at_its_ceiling() {
    let bytes = holding_machine();
    let table = StateTable::parse(&bytes, 4).expect("readable");
    for ceiling in [0u32, 1, 2, 3, 8, 64, 4_096] {
        let limits = Limits {
            max_transitions: ceiling,
            ..Limits::DEFAULT
        };
        let mut buffer = Buffer::from_glyphs(&[5, 10, 6]);
        let (mut taken, mut ends) = (0u32, 0u32);
        // Each held transition turns glyph 10 into 11 and back: the class
        // stays 4, so the machine never lets go.
        let mut turn =
            |buffer: &mut Buffer, entry: &Entry<'_>, cursor: &mut Cursor| -> Result<(), Warning> {
                taken += 1;
                ends += u32::from(cursor.end_of_text);
                if entry.dont_advance() {
                    let at = cursor.glyph.expect("a glyph");
                    let glyph = buffer.glyph_mut(at).expect("in the run");
                    glyph.glyph = 21 - glyph.glyph;
                }
                Ok(())
            };
        let result = table.run(&mut buffer, Traversal::Forward, &limits, site(3), &mut turn);
        assert_eq!(
            result,
            Err(Warning::TransitionBudgetExceeded { site: site(3) }),
            "ceiling {ceiling}"
        );
        assert_eq!(taken, ceiling, "every transition up to the ceiling");
        assert_eq!(ends, 0, "the end of text is never reached");
        // The first transition passes glyph 5; each after it turns the
        // held glyph once.
        let turns = ceiling.saturating_sub(1);
        let held = if turns % 2 == 0 { 10 } else { 11 };
        let glyphs: Vec<u16> = buffer.glyphs().iter().map(|g| g.glyph).collect();
        assert_eq!(glyphs, vec![5, held, 6], "ceiling {ceiling}");
    }

    // A run of `n` glyphs the machine walks without holding takes `n + 1`
    // transitions, the end of text's included: a ceiling of `n + 1` lets it
    // finish and one of `n` stops it short of the end.
    let glyphs = [5u16, 6, 7];
    let exact = Limits {
        max_transitions: 4,
        ..Limits::DEFAULT
    };
    let (result, seen) = walk(&table, &glyphs, Traversal::Forward, &exact);
    assert_eq!(result, Ok(()));
    assert_eq!(seen.len(), 4);
    let short = Limits {
        max_transitions: 3,
        ..Limits::DEFAULT
    };
    let (result, seen) = walk(&table, &glyphs, Traversal::Forward, &short);
    assert_eq!(
        result,
        Err(Warning::TransitionBudgetExceeded { site: site(0) })
    );
    assert!(seen.iter().all(|(_, end)| !end));

    // The ceiling a run's own limits give it stops the held glyph too, in a
    // backward walk as in a forward one.
    let scaled = Limits::for_glyphs(3);
    let mut buffer = Buffer::from_glyphs(&[5, 10, 6]);
    let mut noop =
        |_: &mut Buffer, _: &Entry<'_>, _: &mut Cursor| -> Result<(), Warning> { Ok(()) };
    assert_eq!(
        table.run(
            &mut buffer,
            Traversal::Backward,
            &scaled,
            site(1),
            &mut noop
        ),
        Err(Warning::TransitionBudgetExceeded { site: site(1) })
    );
}

/// The driver walks the run forward or backward, glyph by glyph, then takes
/// the end of text once with the last glyph it met as the current one; an
/// empty run takes the end of text alone.
#[test]
fn a_machine_walks_its_run_either_way_and_takes_the_end_once() {
    let bytes = holding_machine();
    let table = StateTable::parse(&bytes, 4).expect("readable");
    let limits = Limits::DEFAULT;
    let glyphs = [5u16, 6, 7];
    let (result, forward) = walk(&table, &glyphs, Traversal::Forward, &limits);
    assert_eq!(result, Ok(()));
    assert_eq!(
        forward,
        vec![
            (Some(0), false),
            (Some(1), false),
            (Some(2), false),
            (Some(2), true)
        ]
    );
    let (result, backward) = walk(&table, &glyphs, Traversal::Backward, &limits);
    assert_eq!(result, Ok(()));
    assert_eq!(
        backward,
        vec![
            (Some(2), false),
            (Some(1), false),
            (Some(0), false),
            (Some(0), true)
        ]
    );
    for traversal in [Traversal::Forward, Traversal::Backward] {
        let (result, empty) = walk(&table, &[], traversal, &limits);
        assert_eq!(result, Ok(()));
        assert_eq!(empty, vec![(None, true)], "{traversal:?}");
    }
}

/// A machine sees the deleted glyph as class 2, takes the entry its state
/// gives that class, and moves to the state each entry names.
#[test]
fn a_machine_moves_between_states_and_sees_the_deleted_glyph() {
    // State 0 moves to state 2 on glyph 10; state 2 marks an out-of-bounds
    // glyph with flag 0x0001 and a deleted one with 0x0002, and returns to
    // state 0 on glyph 10 with 0x0008.
    let bytes = stx(
        5,
        0,
        &lookup_single(&[(10, 4)]),
        &[&[0, 0, 0, 0, 1], &[0, 0, 0, 0, 1], &[0, 2, 3, 0, 4]],
        &[&[0, 0], &[2, 0], &[2, 0x0001], &[2, 0x0002], &[0, 0x0008]],
    );
    let table = StateTable::parse(&bytes, 4).expect("readable");
    let mut buffer = Buffer::from_glyphs(&[7, 10, 7, DELETED, 10, DELETED]);
    let mut flags: Vec<u16> = Vec::new();
    let mut record = |_: &mut Buffer, entry: &Entry<'_>, _: &mut Cursor| -> Result<(), Warning> {
        flags.push(entry.flags);
        Ok(())
    };
    let result = table.run(
        &mut buffer,
        Traversal::Forward,
        &Limits::DEFAULT,
        site(0),
        &mut record,
    );
    assert_eq!(result, Ok(()));
    // 7 in state 0; 10 moves to 2; 7 out of bounds in 2; the deleted glyph
    // in 2; 10 back to 0; the deleted glyph in 0; the end of text in 0.
    assert_eq!(flags, vec![0, 0, 0x0001, 0x0002, 0x0008, 0, 0]);
}

/// A state or a class the tables do not hold stops the machine where it
/// stands and names the subtable, the actions before it kept; an action's
/// own warning stops it the same way.
#[test]
fn a_state_or_class_the_table_does_not_hold_stops_the_machine() {
    // Glyph 10 moves to a state a thousand lines past the array; glyph 11 is
    // class 9 of five.
    let bytes = stx(
        5,
        0,
        &lookup_single(&[(10, 4), (11, 9)]),
        &[&[0, 0, 0, 0, 1], &[0, 0, 0, 0, 1]],
        &[&[0, 0], &[1000, 0]],
    );
    let table = StateTable::parse(&bytes, 4).expect("readable");
    for (glyphs, transitions) in [(vec![5u16, 10, 6], 2usize), (vec![5, 11, 6], 1)] {
        let (result, seen) = walk(&table, &glyphs, Traversal::Forward, &Limits::DEFAULT);
        assert_eq!(
            result,
            Err(Warning::MalformedAatSubtable { site: site(0) }),
            "{glyphs:?}"
        );
        assert_eq!(seen.len(), transitions, "{glyphs:?}");
    }

    // An action that refuses stops the walk at the glyph it refused.
    let bytes = holding_machine();
    let table = StateTable::parse(&bytes, 4).expect("readable");
    let mut buffer = Buffer::from_glyphs(&[5, 6, 7, 8]);
    let mut met = 0usize;
    let refusal = Warning::UnknownLookupFormat {
        site: site(5),
        format: 1,
    };
    let mut refuse = |_: &mut Buffer, _: &Entry<'_>, cursor: &mut Cursor| -> Result<(), Warning> {
        met += 1;
        if cursor.glyph == Some(1) {
            return Err(refusal);
        }
        Ok(())
    };
    assert_eq!(
        table.run(
            &mut buffer,
            Traversal::Forward,
            &Limits::DEFAULT,
            site(0),
            &mut refuse
        ),
        Err(refusal)
    );
    assert_eq!(met, 2);
}

/// Every prefix of every lookup table and of a state table reads as far as
/// it goes: a cut header is no table, a value past the cut answers nothing,
/// a machine over a cut table stops by name, and nothing panics. An offset
/// past the table's end is no table either.
#[test]
fn a_truncated_table_answers_nothing_and_never_panics() {
    for (format, table, expected) in lookup_cases() {
        for cut in 0..table.len() {
            let Ok(lookup) = Lookup::parse(&table[..cut]) else {
                continue;
            };
            for (glyph, value) in &expected {
                let answer = lookup.value(*glyph);
                assert!(
                    answer.is_none() || answer == Some(*value),
                    "{format:?} cut at {cut}: glyph {glyph} answered {answer:?}"
                );
            }
            let _ = lookup.value(0);
            let _ = lookup.value(u16::MAX);
        }
    }

    let bytes = stx(
        7,
        1,
        &lookup_single(&[(10, 4), (11, 5), (12, 6)]),
        &[
            &[0, 1, 0, 0, 2, 3, 0],
            &[0, 1, 0, 0, 2, 3, 0],
            &[0, 1, 0, 0, 0, 0, 0],
        ],
        &[
            &[0, 0, 0, 0],
            &[0, 0x4000, 0, 0],
            &[2, 0, 0, 0],
            &[0, 0, 0, 0],
        ],
    );
    for cut in 0..bytes.len() {
        match StateTable::parse(&bytes[..cut], 8) {
            Err(error) => assert_eq!(error, StateError::Malformed, "cut at {cut}"),
            Ok(table) => {
                let mut buffer = Buffer::from_glyphs(&[10, 11, 12, 3, DELETED]);
                let mut noop = |_: &mut Buffer,
                                _: &Entry<'_>,
                                _: &mut Cursor|
                 -> Result<(), Warning> { Ok(()) };
                let result = table.run(
                    &mut buffer,
                    Traversal::Forward,
                    &Limits::DEFAULT,
                    site(0),
                    &mut noop,
                );
                assert!(
                    matches!(
                        result,
                        Ok(())
                            | Err(Warning::MalformedAatSubtable { .. })
                            | Err(Warning::TransitionBudgetExceeded { .. })
                    ),
                    "cut at {cut}: {result:?}"
                );
            }
        }
    }
    // Each offset pointed one byte past the table.
    for slot in [4usize, 8, 12] {
        let mut past = bytes.clone();
        put32(&mut past, slot, count32(bytes.len() + 1));
        assert_eq!(
            StateTable::parse(&past, 8).err(),
            Some(StateError::Malformed),
            "offset at {slot}"
        );
    }
}
