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

use tinker_pdf_font::Sfnt;
use tinker_pdf_shape::bidi::{BaseDirection, Level};
use tinker_pdf_shape::shape::Run;
use tinker_pdf_shape::unicode::Script;
use tinker_pdf_shape::{Orientation, ShapedGlyph, ShapedRun, Shaper, Tag, Warning};

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
