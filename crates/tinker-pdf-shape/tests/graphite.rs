//! Graphite shaping: `docs/design/shaping.md`, *Graphite shaping*.
//!
//! # What these tests can and cannot say
//!
//! The faces below are assembled here, table by table, and every expectation
//! is computed from the bytes the test wrote or stated beside them — the
//! standing `tests/synthetic.rs` states for itself. A test here catches a
//! disagreement between the readers and the reading the design settles from
//! the Graphite Table Format; it cannot catch a misreading the two share.
//! Where the specification gives a worked example, the example is
//! transcribed as bytes and the source's own answer is the expectation; the
//! vendored faces under `data/graphite` are what hold the readings to bytes
//! their authors' compiler wrote; and the fixtures that can say a shaped run
//! is right are H-64's.

use std::collections::BTreeMap;

use tinker_pdf_font::Sfnt;
use tinker_pdf_shape::bidi::{BaseDirection, Level};
use tinker_pdf_shape::graphite::{self, Attributes, GlatVersion, Octabox, Refusal, Subbox, Tables};
use tinker_pdf_shape::shape::Run;
use tinker_pdf_shape::unicode::Script;
use tinker_pdf_shape::{Limits, Orientation, ShapedGlyph, ShapedRun, Shaper, Tag, Warning};

// --- a face assembler ----------------------------------------------------
//
// In `tests/synthetic.rs`'s register: an OpenType Layout table is a header
// of counts and `Offset16`s followed by what they point at, so `assemble`
// appends the parts and patches each slot with where its part landed. The
// face around the tables is the smallest sfnt the shaper reads: `cmap`,
// `head`, `hhea`, `hmtx` and `maxp`, and whatever further tables a test
// names.

fn u16s(values: &[u16]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_be_bytes()).collect()
}

/// A header, with `slots[i]` patched to the offset of `parts[i]`.
fn assemble(mut head: Vec<u8>, slots: &[usize], parts: &[Vec<u8>]) -> Vec<u8> {
    assert_eq!(slots.len(), parts.len(), "a slot for every part");
    let mut body: Vec<u8> = Vec::new();
    for (slot, part) in slots.iter().zip(parts) {
        let offset = u16::try_from(head.len() + body.len()).expect("a table under 64 KB");
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
    let mut out = u16s(&[1, u16::try_from(sorted.len()).expect("a small coverage")]);
    out.extend(u16s(&sorted));
    out
}

/// One lookup of `kind`, its subtables in order.
fn lookup(kind: u16, subtables: &[Vec<u8>]) -> Vec<u8> {
    let count = u16::try_from(subtables.len()).expect("a small lookup");
    let mut head = u16s(&[kind, 0, count]);
    head.extend(vec![0u8; subtables.len() * 2]);
    let slots: Vec<usize> = (0..subtables.len()).map(|i| 6 + i * 2).collect();
    assemble(head, &slots, subtables)
}

/// `LigatureSubstFormat1` of one ligature: `first` then `rest` become `into`.
fn ligature_subst(first: u16, rest: &[u16], into: u16) -> Vec<u8> {
    let components = u16::try_from(rest.len() + 1).expect("a small ligature");
    let mut ligature = u16s(&[into, components]);
    ligature.extend(u16s(rest));
    let set = assemble(u16s(&[1, 0]), &[2], &[ligature]);
    assemble(u16s(&[1, 0, 1, 0]), &[2, 6], &[coverage(&[first]), set])
}

/// `PairPosFormat1` of one pair, `first` then `second`, the first glyph's
/// advance changed by `x_advance`.
fn pair_kern(first: u16, second: u16, x_advance: i16) -> Vec<u8> {
    let mut set = u16s(&[1, second]);
    set.extend(x_advance.to_be_bytes());
    // Format 1, the coverage, `XAdvance` for the first glyph and nothing for
    // the second, one pair set.
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
    let feature_count = u16::try_from(features.len()).expect("a small feature list");
    let mut lang_sys = u16s(&[0, 0xFFFF, feature_count]);
    lang_sys.extend(u16s(&(0..feature_count).collect::<Vec<u16>>()));
    let script = assemble(u16s(&[0, 0]), &[0], &[lang_sys]);

    let mut script_head = u16s(&[u16::try_from(scripts.len()).expect("two scripts")]);
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
        let mut table = u16s(&[0, u16::try_from(indices.len()).expect("a small feature")]);
        table.extend(u16s(indices));
        feature_tables.push(table);
    }
    let feature_list = assemble(feature_head, &feature_slots, &feature_tables);

    let mut list_head = u16s(&[u16::try_from(lookups.len()).expect("a small lookup list")]);
    list_head.extend(vec![0u8; lookups.len() * 2]);
    let slots: Vec<usize> = (0..lookups.len()).map(|i| 2 + i * 2).collect();
    let lookup_list = assemble(list_head, &slots, lookups);

    assemble(
        u16s(&[1, 0, 0, 0, 0]),
        &[4, 6, 8],
        &[script_list, feature_list, lookup_list],
    )
}

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

    fn build(&self) -> Vec<u8> {
        let glyphs = u16::try_from(self.advances.len()).expect("a small face");
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

        let mut maxp = vec![0u8; 6];
        maxp[0..4].copy_from_slice(&0x0000_5000u32.to_be_bytes());
        maxp[4..6].copy_from_slice(&glyphs.to_be_bytes());
        tables.push((*b"maxp", maxp));

        let mut sorted = self.cmap.clone();
        sorted.sort_unstable();
        let groups = u32::try_from(sorted.len()).expect("a small cmap");
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
    out.extend(u16s(&[
        u16::try_from(tables.len()).expect("a small directory"),
        0,
        0,
        0,
    ]));
    let mut offset = 12 + tables.len() * 16;
    let mut body = Vec::new();
    for (tag, table) in tables {
        out.extend_from_slice(tag);
        out.extend(0u32.to_be_bytes());
        out.extend(u32::try_from(offset).expect("a small face").to_be_bytes());
        out.extend(
            u32::try_from(table.len())
                .expect("a small table")
                .to_be_bytes(),
        );
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

// --- a Graphite description ----------------------------------------------
//
// FT-07d.2 reads no Graphite table, so the tables here need only be
// present: a `Silf` of version 4 with no descriptions, and a `Glat`,
// `Gloc`, `Feat` and `Sill` of the smallest form each version takes.

/// `Silf` 4.0: its version, a compiler version, and no descriptions.
fn empty_silf() -> Vec<u8> {
    let mut out = 0x0004_0000u32.to_be_bytes().to_vec();
    out.extend(0x0004_0002u32.to_be_bytes());
    out.extend(u16s(&[0, 0]));
    out
}

/// The four Graphite tables beside `Silf`: a `Glat` and a `Gloc` of no
/// attributes for `glyphs` glyphs, a `Feat` of no features and a `Sill` of
/// no languages.
fn graphite_companions(glyphs: u16) -> Vec<([u8; 4], Vec<u8>)> {
    let glat = 0x0001_0000u32.to_be_bytes().to_vec();
    let mut gloc = 0x0001_0000u32.to_be_bytes().to_vec();
    gloc.extend(u16s(&[0, 0]));
    gloc.extend(u16s(&vec![4; usize::from(glyphs) + 1]));
    let mut feat = 0x0002_0000u32.to_be_bytes().to_vec();
    feat.extend(u16s(&[0, 0, 0, 0]));
    let mut sill = 0x0001_0000u32.to_be_bytes().to_vec();
    sill.extend(u16s(&[0, 0, 0, 0]));
    sill.extend(*b"\0\0\0\0");
    sill.extend(u16s(&[0, 20]));
    vec![
        (*b"Glat", glat),
        (*b"Gloc", gloc),
        (*b"Feat", feat),
        (*b"Sill", sill),
    ]
}

/// A Latin face with an `f`, an `i`, an `fi` and a Greek alpha and beta,
/// carrying a Graphite description and no OpenType Layout table.
fn graphite_face() -> Face {
    let mut face = Face::new(7, &[('f', 1), ('i', 2), ('x', 3), ('α', 5), ('β', 6)])
        .with(b"Silf", empty_silf());
    for (tag, table) in graphite_companions(7) {
        face = face.with(&tag, table);
    }
    face
}

/// `f` and `i` ligated to glyph 4 under `liga`.
fn fi_ligature() -> Vec<u8> {
    layout_table(
        &[(*b"liga", vec![0])],
        &[lookup(4, &[ligature_subst(1, &[2], 4)])],
    )
}

/// The refusal: the warning each run carries.
fn silf_not_run() -> Warning {
    Warning::TableNotRun {
        table: Tag::new(b"Silf"),
    }
}

/// How many of `warnings` name `Silf` as not run.
fn refusals(warnings: &[Warning]) -> usize {
    warnings.iter().filter(|w| **w == silf_not_run()).count()
}

// --- the refusal (FT-07d.2) ----------------------------------------------

/// A face whose only layout tables are Graphite's shapes as a face with no
/// layout tables does, and its run says so once; given a `GSUB`, the same
/// face shapes through it and says nothing of `Silf`.
#[test]
fn a_face_carrying_only_graphite_tables_is_named_and_shaped_unshaped() {
    let face = graphite_face();
    let text = "fix";
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let runs = line_runs(&Shaper::new(&sfnt), text);
    assert_eq!(runs.len(), 1, "one Latin run");

    // The `cmap`-only run: the same face with no Graphite table at all.
    let mut plain_face = face.clone();
    for tag in [b"Silf", b"Glat", b"Gloc", b"Feat", b"Sill"] {
        plain_face = plain_face.without(tag);
    }
    let plain_bytes = plain_face.build();
    let plain_sfnt = Sfnt::parse(&plain_bytes).expect("the plain face parses");
    let plain = line_runs(&Shaper::new(&plain_sfnt), text);
    assert!(plain.iter().all(|run| run.warnings().is_empty()));

    let expected: Vec<ShapedGlyph> = text
        .chars()
        .zip(clusters(text))
        .map(|(c, cluster)| face.plain(face.glyph(c), cluster))
        .collect();
    assert_eq!(glyphs(&runs), expected, "the cmap-only glyphs, unligated");
    assert_eq!(glyphs(&runs), glyphs(&plain), "the face without its Silf");
    assert_eq!(runs[0].warnings(), &[silf_not_run()], "named once");

    // The same face with a `GSUB` beside its `Silf` takes the OpenType
    // path: the ligature forms, and nothing names `Silf`.
    let with_gsub = face.with(b"GSUB", fi_ligature());
    let gsub_bytes = with_gsub.build();
    let gsub_sfnt = Sfnt::parse(&gsub_bytes).expect("the face with a GSUB parses");
    let shaped = line_runs(&Shaper::new(&gsub_sfnt), text);
    let shaped_glyphs = glyphs(&shaped);
    assert_eq!(
        shaped_glyphs.iter().map(|g| g.glyph).collect::<Vec<u16>>(),
        vec![4, with_gsub.glyph('x')],
        "the GSUB's ligature"
    );
    assert!(
        shaped.iter().all(|run| refusals(run.warnings()) == 0),
        "nothing names Silf: {:?}",
        shaped.iter().map(ShapedRun::warnings).collect::<Vec<_>>()
    );
}

/// Every run of a paragraph carries the refusal, and each carries it once,
/// however many glyphs it holds.
#[test]
fn every_run_over_a_graphite_only_face_is_named_once() {
    let face = graphite_face();
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let shaper = Shaper::new(&sfnt);
    let text = "fixfixfix αβαβ fi";
    let runs = line_runs(&shaper, text);
    assert!(runs.len() >= 2, "a Latin and a Greek run: {}", runs.len());
    for run in &runs {
        assert_eq!(
            refusals(run.warnings()),
            1,
            "run {:?}: {:?}",
            run.text(),
            run.warnings()
        );
    }
    // A run of no glyphs at all is still a run shaped without the face's
    // description.
    let empty = Run {
        text: 2..2,
        script: Script::Latin,
        level: Level::LTR,
        orientation: Orientation::Horizontal,
    };
    let shaped = shaper.shape(text, &empty);
    assert!(shaped.glyphs().is_empty());
    assert_eq!(shaped.warnings(), &[silf_not_run()]);
}

/// A `GPOS` beside the `Silf` is an OpenType Layout table too: the face
/// takes the OpenType path, its kerning applied, and nothing names `Silf`.
#[test]
fn a_face_carrying_gpos_beside_its_silf_takes_the_opentype_path() {
    let face = graphite_face().with(
        b"GPOS",
        layout_table(
            &[(*b"kern", vec![0])],
            &[lookup(2, &[pair_kern(1, 2, -40)])],
        ),
    );
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let runs = line_runs(&Shaper::new(&sfnt), "fi");
    let shaped = glyphs(&runs);
    assert_eq!(
        shaped[0].x_advance,
        i32::from(face.advances[1]) - 40,
        "the pair kerned"
    );
    assert!(runs.iter().all(|run| refusals(run.warnings()) == 0));
}

/// A `GSUB` this crate cannot read is no OpenType path to take: the face's
/// only readable layout tables are still Graphite's, and it is named.
#[test]
fn an_unreadable_gsub_leaves_the_face_graphite_only() {
    let mut unreadable = fi_ligature();
    unreadable[0..2].copy_from_slice(&2u16.to_be_bytes());
    let face = graphite_face().with(b"GSUB", unreadable);
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let runs = line_runs(&Shaper::new(&sfnt), "fi");
    assert_eq!(
        glyphs(&runs).iter().map(|g| g.glyph).collect::<Vec<u16>>(),
        vec![1, 2],
        "nothing ligated"
    );
    assert!(runs.iter().all(|run| refusals(run.warnings()) == 1));
}

/// A face carrying `morx` beside its `Silf` is AAT's to shape, so its runs
/// do not name `Silf`; a face with Graphite's attribute tables and no `Silf`
/// has no description to go without.
#[test]
fn only_a_silf_with_no_other_layout_table_is_named() {
    let with_morx = graphite_face().with(b"morx", 0x0002_0000u32.to_be_bytes().to_vec());
    let bytes = with_morx.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face with morx parses");
    let runs = line_runs(&Shaper::new(&sfnt), "fi");
    assert!(runs.iter().all(|run| refusals(run.warnings()) == 0));

    let without_silf = graphite_face().without(b"Silf");
    let bytes = without_silf.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face without Silf parses");
    let runs = line_runs(&Shaper::new(&sfnt), "fi");
    assert!(runs.iter().all(|run| run.warnings().is_empty()));

    // A `GDEF` is not a table that runs: beside a `Silf` alone, the face is
    // still a Graphite description and still named.
    let mut gdef = 0x0001_0000u32.to_be_bytes().to_vec();
    gdef.extend(u16s(&[0, 0, 0, 0]));
    let with_gdef = graphite_face().with(b"GDEF", gdef);
    let bytes = with_gdef.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face with GDEF parses");
    let runs = line_runs(&Shaper::new(&sfnt), "fi");
    assert!(runs.iter().all(|run| refusals(run.warnings()) == 1));
}

/// A column of the face is shaped without its description too, and each of
/// its runs says so once, beside whatever else the column says.
#[test]
fn a_column_over_a_graphite_only_face_is_named_too() {
    let face = graphite_face();
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
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
        assert_eq!(refusals(run.warnings()), 1, "{:?}", run.warnings());
    }
    // The upright run also says its metrics were synthesized, the face
    // having no `vmtx`: the refusal sits beside the column's own warnings.
    assert!(runs[1]
        .warnings()
        .contains(&Warning::VerticalMetricsSynthesized));
}

// --- the vendored faces (FT-07d.3) ---------------------------------------
//
// By `include_bytes!`, so no test here depends on a filesystem it could find
// missing and skip, and so a run under `wasm32-wasip1`, which grants none,
// reads the same faces.

/// Dai Banna SIL 4.000: a Graphite description and no OpenType Layout.
const DAI_BANNA: &[u8] = include_bytes!("../data/graphite/DaiBannaSIL-Regular.ttf");
/// Tagmukay 2.000: a Graphite description beside a `GSUB`.
const TAGMUKAY: &[u8] = include_bytes!("../data/graphite/Tagmukay-Regular.ttf");
/// Awami Nastaliq 3.500: compressed tables beside a one-lookup `GPOS`.
const AWAMI: &[u8] = include_bytes!("../data/graphite/AwamiNastaliq-Regular.ttf");

/// The table tag `tag` as `Sfnt::table` takes it.
fn tag(tag: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*tag)
}

/// The `name` table's Windows, Unicode BMP, US English string `id`, read by
/// this test's own few lines: a format 0 or 1 header, its records, and
/// UTF-16BE storage.
fn name(face: &Sfnt<'_>, id: u16) -> Option<String> {
    let table = face.table(tag(b"name"))?;
    let u16_at = |at: usize| -> Option<u16> {
        Some(u16::from_be_bytes([*table.get(at)?, *table.get(at + 1)?]))
    };
    let count = usize::from(u16_at(2)?);
    let storage = usize::from(u16_at(4)?);
    for record in 0..count {
        let at = 6 + record * 12;
        let (platform, encoding, language, name_id) = (
            u16_at(at)?,
            u16_at(at + 2)?,
            u16_at(at + 4)?,
            u16_at(at + 6)?,
        );
        if (platform, encoding, language, name_id) != (3, 1, 0x0409, id) {
            continue;
        }
        let length = usize::from(u16_at(at + 8)?);
        let start = storage + usize::from(u16_at(at + 10)?);
        let bytes = table.get(start..start + length)?;
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect();
        return String::from_utf16(&units).ok();
    }
    None
}

/// Each vendored face is the release THIRDPARTY.md records: it parses as an
/// sfnt, carries the four Graphite tables and `Sill`, names the family and
/// version its release names, is the length its release's file is, and
/// carries exactly the OpenType Layout tables the design's precedence turns
/// on.
#[test]
fn the_fixture_faces_are_the_ones_vendored() {
    // (bytes, family, version, length, GSUB, GPOS)
    let faces: [(&[u8], &str, &str, usize, bool, bool); 3] = [
        (
            DAI_BANNA,
            "Dai Banna SIL",
            "Version 4.000",
            68_172,
            false,
            false,
        ),
        (TAGMUKAY, "Tagmukay", "Version 2.000", 124_240, true, false),
        (
            AWAMI,
            "Awami Nastaliq",
            "Version 3.500",
            644_356,
            false,
            true,
        ),
    ];
    for (bytes, family, version, length, gsub, gpos) in faces {
        assert_eq!(bytes.len(), length, "{family}'s length");
        let face = Sfnt::parse(bytes).unwrap_or_else(|| panic!("{family} parses"));
        for table in [b"Silf", b"Glat", b"Gloc", b"Feat", b"Sill"] {
            assert!(
                face.table(tag(table)).is_some_and(|t| !t.is_empty()),
                "{family} carries {}",
                String::from_utf8_lossy(table)
            );
        }
        assert_eq!(name(&face, 1).as_deref(), Some(family), "the family");
        assert_eq!(
            name(&face, 5).as_deref(),
            Some(version),
            "{family}'s version"
        );
        assert_eq!(face.table(tag(b"GSUB")).is_some(), gsub, "{family}'s GSUB");
        assert_eq!(face.table(tag(b"GPOS")).is_some(), gpos, "{family}'s GPOS");
        assert!(
            face.table(tag(b"morx")).is_none(),
            "{family} carries no morx"
        );
    }
}

/// The vendored Graphite-only face is refused by name on a real face's
/// bytes, and shapes as its `cmap` alone gives it: U+0301 is in its
/// pseudo-glyph map and not in its `cmap`, so until its passes run the
/// accent is `.notdef`. The two faces carrying OpenType Layout beside their
/// Graphite tables take that path and are not named.
#[test]
fn the_vendored_faces_take_the_path_the_precedence_gives() {
    let dai_banna = Sfnt::parse(DAI_BANNA).expect("Dai Banna SIL parses");
    let text = "e\u{301}";
    let runs = line_runs(&Shaper::new(&dai_banna), text);
    assert!(!runs.is_empty());
    for run in &runs {
        assert_eq!(refusals(run.warnings()), 1, "{:?}", run.warnings());
    }
    let shaped = glyphs(&runs);
    let e = dai_banna.glyph_for_char('e').expect("Dai Banna SIL maps e");
    assert_eq!(
        dai_banna.glyph_for_char('\u{301}'),
        None,
        "U+0301 is not in its cmap"
    );
    assert_eq!(
        shaped.iter().map(|g| g.glyph).collect::<Vec<u16>>(),
        vec![e, 0],
        "e and .notdef, uncomposed"
    );

    for (bytes, family, text) in [
        (TAGMUKAY, "Tagmukay", "\u{2D30}\u{2D31}\u{2D33}"),
        (AWAMI, "Awami Nastaliq", "\u{627}\u{631}\u{62F}\u{648}"),
    ] {
        let face = Sfnt::parse(bytes).unwrap_or_else(|| panic!("{family} parses"));
        let runs = line_runs(&Shaper::new(&face), text);
        assert!(!runs.is_empty(), "{family} shapes");
        assert!(
            runs.iter().all(|run| refusals(run.warnings()) == 0),
            "{family} is not named"
        );
    }
}

// --- glyph attributes (FT-07d.4) ----------------------------------------
//
// A glyph's attributes are a map from number to value; `glat_and_gloc`
// writes such maps as each `Glat` version stores them — runs of consecutive
// numbers, an octabox first where the version carries them — and a `Gloc`
// in either offset size, so the answers the reader gives are compared with
// the maps the test began from.

/// How a test writes its `Gloc`.
#[derive(Clone, Copy, Debug)]
struct Index {
    /// Offsets of four bytes rather than two.
    long: bool,
    /// The name array at the end, each attribute given `1000 + n`.
    names: bool,
}

/// The `Glat` and `Gloc` storing `glyphs`' maps in `version`, each block
/// opening with `octaboxes[glyph]` where the version carries octaboxes.
fn glat_and_gloc(
    version: GlatVersion,
    glyphs: &[BTreeMap<u16, i16>],
    octaboxes: &[Option<Octabox>],
    index: Index,
    count: u16,
) -> (Vec<u8>, Vec<u8>) {
    let mut glat: Vec<u8> = match version {
        GlatVersion::One => 0x0001_0000u32.to_be_bytes().to_vec(),
        GlatVersion::Two => 0x0002_0000u32.to_be_bytes().to_vec(),
        GlatVersion::Three { octaboxes } => {
            let mut out = 0x0003_0000u32.to_be_bytes().to_vec();
            out.extend(u32::from(octaboxes).to_be_bytes());
            out
        }
    };
    let mut offsets = vec![glat.len()];
    for (glyph, map) in glyphs.iter().enumerate() {
        if let (GlatVersion::Three { octaboxes: true }, Some(Some(octabox))) =
            (version, octaboxes.get(glyph))
        {
            glat.extend(octabox.cells.to_be_bytes());
            glat.extend(octabox.diagonals);
            for sub in &octabox.subboxes {
                glat.extend([sub.left, sub.right, sub.bottom, sub.top]);
                glat.extend(sub.diagonals);
            }
        }
        // Runs of consecutive numbers, in order.
        let mut runs: Vec<(u16, Vec<i16>)> = Vec::new();
        for (number, value) in map {
            match runs.last_mut() {
                Some((first, values))
                    if usize::from(*first) + values.len() == usize::from(*number) =>
                {
                    values.push(*value);
                }
                _ => runs.push((*number, vec![*value])),
            }
        }
        for (first, values) in runs {
            if version == GlatVersion::One {
                glat.push(u8::try_from(first).expect("a byte attribute number"));
                glat.push(u8::try_from(values.len()).expect("a byte count"));
            } else {
                glat.extend(first.to_be_bytes());
                glat.extend(u16::try_from(values.len()).expect("a count").to_be_bytes());
            }
            for value in values {
                glat.extend(value.to_be_bytes());
            }
        }
        offsets.push(glat.len());
    }
    let flags = u16::from(index.long) | (u16::from(index.names) << 1);
    let mut gloc = 0x0001_0000u32.to_be_bytes().to_vec();
    gloc.extend(u16s(&[flags, count]));
    for offset in offsets {
        if index.long {
            gloc.extend(u32::try_from(offset).expect("small").to_be_bytes());
        } else {
            gloc.extend(u16::try_from(offset).expect("small").to_be_bytes());
        }
    }
    if index.names {
        gloc.extend(u16s(&(0..count).map(|n| 1000 + n).collect::<Vec<u16>>()));
    }
    (glat, gloc)
}

/// The glyphs every version's test stores: an empty glyph, a single run, a
/// glyph of three runs with the extremes of an `i16`, a glyph of two runs
/// with a number past the first run's, and, where the version numbers
/// attributes in 16 bits, attributes past 255.
fn attribute_maps(version: GlatVersion) -> Vec<BTreeMap<u16, i16>> {
    let mut maps = vec![
        BTreeMap::new(),
        BTreeMap::from([(0, 5)]),
        BTreeMap::from([(1, -2), (2, 300), (3, 4), (9, i16::MIN), (10, i16::MAX)]),
        BTreeMap::from([(7, 1), (8, -1), (200, 9)]),
    ];
    if version != GlatVersion::One {
        maps.push(BTreeMap::from([
            (255, 3),
            (256, 4),
            (300, 42),
            (65_535, -7),
        ]));
    }
    maps
}

/// An octabox for glyph `glyph`, with a sub-box in each cell its bits name.
fn octabox_for(glyph: u8, cells: u16) -> Octabox {
    Octabox {
        cells,
        diagonals: [glyph, glyph + 1, glyph + 2, glyph + 3],
        subboxes: (0..16u8)
            .filter(|cell| cells & (1 << cell) != 0)
            .map(|cell| Subbox {
                cell,
                left: cell,
                right: cell + 16,
                bottom: cell + 32,
                top: cell + 48,
                diagonals: [cell + 64, cell + 80, cell + 96, cell + 112],
            })
            .collect(),
    }
}

/// Every version answers every glyph's every attribute as stored, 0 where it
/// stores nothing, through either offset size, with or without the name
/// array, octaboxes and all; and the same tables through `Tables`, a `Glat`
/// 3 compressed and decoded on the way.
#[test]
fn every_glat_version_answers_the_attributes_it_stores() {
    let versions = [
        GlatVersion::One,
        GlatVersion::Two,
        GlatVersion::Three { octaboxes: false },
        GlatVersion::Three { octaboxes: true },
    ];
    let indexes = [
        Index {
            long: false,
            names: false,
        },
        Index {
            long: true,
            names: false,
        },
        Index {
            long: false,
            names: true,
        },
        Index {
            long: true,
            names: true,
        },
    ];
    let mut checked = 0usize;
    for version in versions {
        let maps = attribute_maps(version);
        // Glyph 0 is empty and so has no octabox; the others each have one:
        // one with no sub-boxes, one with every cell, one with two, one with
        // the lower left alone.
        let cells = [0u16, 0x0000, 0xFFFF, 0x1008, 0x0001];
        let octaboxes: Vec<Option<Octabox>> = (0..maps.len())
            .map(|glyph| {
                (glyph > 0).then(|| octabox_for(u8::try_from(glyph).expect("small"), cells[glyph]))
            })
            .collect();
        let highest = maps
            .iter()
            .flat_map(|map| map.keys().copied())
            .max()
            .unwrap_or(0);
        for index in indexes {
            let count = 12;
            let (glat, gloc) = glat_and_gloc(version, &maps, &octaboxes, index, count);
            let attributes =
                Attributes::parse(&gloc, &glat, &Limits::DEFAULT).expect("the tables read");
            assert_eq!(attributes.version(), version);
            assert_eq!(attributes.glyph_count(), maps.len());
            assert_eq!(attributes.attribute_count(), count);
            for number in 0..count + 2 {
                let name = attributes.attribute_name(number);
                if index.names && number < count {
                    assert_eq!(name, Some(1000 + number));
                } else {
                    assert_eq!(name, None, "{number}");
                }
            }
            for (glyph, map) in maps.iter().enumerate() {
                let glyph = u16::try_from(glyph).expect("small");
                let mut numbers: Vec<u16> = (0..=highest.min(400)).collect();
                numbers.extend([highest, 65_534, 65_535]);
                for number in numbers {
                    let expected = map.get(&number).copied().unwrap_or(0);
                    assert_eq!(
                        attributes.attribute(glyph, number),
                        Some(expected),
                        "{version:?} {index:?} glyph {glyph} attribute {number}"
                    );
                    checked += 1;
                }
                let octabox = attributes.octabox(glyph).expect("readable");
                if version == (GlatVersion::Three { octaboxes: true }) {
                    assert_eq!(octabox, octaboxes[usize::from(glyph)].clone());
                } else {
                    assert_eq!(octabox, None);
                }
            }
            let beyond = u16::try_from(maps.len()).expect("small");
            assert_eq!(attributes.attribute(beyond, 0), None);
        }

        // The same `Glat`, compressed where its version can be, read through
        // a face's `Tables`.
        let (glat, gloc) = glat_and_gloc(version, &maps, &octaboxes, indexes[0], 12);
        let stored = match version {
            GlatVersion::Three { .. } => lz4_literals(&glat),
            _ => glat.clone(),
        };
        let silf = empty_silf();
        let tables = Tables::from_tables(
            Some(&silf[..]),
            Some(&stored[..]),
            Some(&gloc[..]),
            None,
            None,
            &Limits::DEFAULT,
        )
        .expect("a description");
        assert_eq!(tables.glat(), Some(&glat[..]), "{version:?} decoded");
        let attributes = tables
            .attributes(&Limits::DEFAULT)
            .expect("both tables")
            .expect("readable");
        for (glyph, map) in maps.iter().enumerate() {
            for (number, value) in map {
                let glyph = u16::try_from(glyph).expect("small");
                assert_eq!(attributes.attribute(glyph, *number), Some(*value));
            }
        }
    }
    assert!(checked > 10_000, "{checked} answers compared");
}

/// `table` as GTF compression scheme 1 stores it: its version, the scheme
/// and size word, and an LZ4 block of literals alone holding the table.
fn lz4_literals(table: &[u8]) -> Vec<u8> {
    let size = u32::try_from(table.len()).expect("small");
    let mut out = table[0..4].to_vec();
    out.extend(((1u32 << 27) | size).to_be_bytes());
    out.extend(lz4_literal_token(table.len()));
    out.extend_from_slice(table);
    out
}

/// An LZ4 token for `literals` literals and no match, with the literal
/// count's continuation bytes after it.
fn lz4_literal_token(literals: usize) -> Vec<u8> {
    let mut out = Vec::new();
    if literals < 15 {
        out.push(u8::try_from(literals).expect("short") << 4);
    } else {
        out.push(0xF0);
        let mut left = literals - 15;
        while left >= 255 {
            out.push(255);
            left -= 255;
        }
        out.push(u8::try_from(left).expect("under 255"));
    }
    out
}

/// A glyph whose block cannot be read answers `None` for every attribute and
/// names itself, and its neighbours still answer.
#[test]
fn an_offset_past_the_glat_answers_none() {
    let version = GlatVersion::Two;
    let maps = attribute_maps(version);
    let index = Index {
        long: false,
        names: false,
    };
    let (glat, gloc) = glat_and_gloc(version, &maps, &[], index, 0);
    // `Gloc`'s offsets: eight bytes of version, flags and count, then one
    // `USHORT` a glyph and one more.
    let offset_at = |glyph: usize| 8 + glyph * 2;
    let read = |gloc: &[u8], glyph: usize| be16(gloc, offset_at(glyph));
    let set = |gloc: &mut Vec<u8>, glyph: usize, value: u16| {
        gloc[offset_at(glyph)..offset_at(glyph) + 2].copy_from_slice(&value.to_be_bytes());
    };
    let glat_len = u16::try_from(glat.len()).expect("small");

    // The last glyph's end past `Glat`'s end.
    let mut past = gloc.clone();
    set(&mut past, maps.len(), glat_len + 2);
    // Glyph 2's end set before its start: glyph 2 ends before it starts, and
    // glyph 3, whose start it is, starts before glyph 2's own.
    let mut backwards = gloc.clone();
    set(&mut backwards, 3, read(&gloc, 2) - 2);
    // Glyph 1 starting inside the header: glyph 0's end is glyph 1's start.
    let mut header = gloc.clone();
    set(&mut header, 1, 2);
    // Glyph 3 cut short inside its last run.
    let mut cut = gloc.clone();
    set(&mut cut, 4, read(&gloc, 4) - 1);

    // (what, the index, the glyphs it breaks, the glyphs whose data it moves
    // and so whose answers say nothing)
    let last = u16::try_from(maps.len() - 1).expect("small");
    let cases: [(&str, Vec<u8>, Vec<u16>, Vec<u16>); 4] = [
        ("an end past the Glat", past, vec![last], vec![]),
        ("an end before the start", backwards, vec![2], vec![3]),
        ("a start inside the header", header, vec![0, 1], vec![]),
        ("a run past its block", cut, vec![3], vec![4]),
    ];
    for (what, gloc, broken, moved) in cases {
        let attributes = Attributes::parse(&gloc, &glat, &Limits::DEFAULT).expect(what);
        for (glyph, map) in maps.iter().enumerate() {
            let glyph = u16::try_from(glyph).expect("small");
            if broken.contains(&glyph) {
                for number in [0u16, 1, 2, 7, 300] {
                    assert_eq!(attributes.attribute(glyph, number), None, "{what}: {glyph}");
                    assert_eq!(
                        attributes.lookup(glyph, number),
                        Err(Refusal::GlyphUnreadable { glyph }),
                        "{what}: {glyph}"
                    );
                }
                assert_eq!(
                    attributes.runs(glyph),
                    Err(Refusal::GlyphUnreadable { glyph })
                );
            } else if moved.contains(&glyph) {
                // The glyph's block now starts inside another's: readable or
                // not, it is not this glyph's data, so nothing is asserted.
            } else {
                for (number, value) in map {
                    assert_eq!(
                        attributes.attribute(glyph, *number),
                        Some(*value),
                        "{what}: glyph {glyph} still reads"
                    );
                }
            }
        }
    }
    // A glyph past the offsets.
    let attributes = Attributes::parse(&gloc, &glat, &Limits::DEFAULT).expect("readable");
    let beyond = u16::try_from(maps.len()).expect("small");
    assert_eq!(attributes.attribute(beyond, 0), None);
    assert_eq!(
        attributes.lookup(beyond, 0),
        Err(Refusal::GlyphUnreadable { glyph: beyond })
    );
}

/// The LZ4 Block Format's worked examples, each transcribed as the block
/// of a compressed `Glat` 3 and decoded through `graphite::decompress`:
/// three literal lengths and the offset of 1 that repeats a byte.
#[test]
fn the_lz4_block_formats_examples_decode() {
    // A table of `n` bytes: `Glat` 3's header, then `n - 8` bytes counting
    // from 1.
    let table = |n: usize| -> Vec<u8> {
        let mut out = 0x0003_0000u32.to_be_bytes().to_vec();
        out.extend(0u32.to_be_bytes());
        out.extend((1..=n - 8).map(|b| u8::try_from(b % 251).expect("a byte")));
        out
    };
    let mut examples: Vec<(&str, Vec<u8>, Vec<u8>)> = Vec::new();
    // "A literal length of 48 will be represented as: 15 ... 33".
    let plain = table(48);
    let mut block = vec![0xF0, 33];
    block.extend_from_slice(&plain);
    examples.push(("48 as 15 and 33", block, plain));
    // "A literal length of 280 will be represented as: 15, 255, 10".
    let plain = table(280);
    let mut block = vec![0xF0, 255, 10];
    block.extend_from_slice(&plain);
    examples.push(("280 as 15, 255 and 10", block, plain));
    // "A literal length of 15 will be represented as: 15, 0 ... yes, the
    // zero must be output".
    let plain = table(15);
    let mut block = vec![0xF0, 0];
    block.extend_from_slice(&plain);
    examples.push(("15 as 15 and 0", block, plain));
    // "A common case is an offset of 1, meaning the last byte is repeated
    // matchlength times": nine literals ending in 7, eleven more 7s from an
    // offset of 1, and a last sequence of one literal.
    let mut plain = 0x0003_0000u32.to_be_bytes().to_vec();
    plain.extend(0u32.to_be_bytes());
    plain.push(7);
    plain.extend([7u8; 11]);
    plain.push(7);
    let mut block = vec![0x97];
    block.extend_from_slice(&plain[..9]);
    block.extend([0x01, 0x00, 0x10, 7]);
    examples.push(("an offset of 1", block, plain));

    assert_eq!(examples.len(), 4, "the LZ4 examples the design counts");
    for (what, block, plain) in examples {
        let mut stored = plain[0..4].to_vec();
        let size = u32::try_from(plain.len()).expect("small");
        stored.extend(((1u32 << 27) | size).to_be_bytes());
        stored.extend(block);
        let decoded = graphite::decompress(graphite::GLAT, &stored, &Limits::DEFAULT)
            .unwrap_or_else(|refusal| panic!("{what}: {refusal:?}"));
        assert_eq!(&*decoded, &plain[..], "{what}");
    }
}

/// The GTF's sub-box bitmap: bit index (y × 4) + x, "bit 3 indicates the
/// presence of the lower right cell, and bit 12 the upper left cell".
#[test]
fn the_gtfs_subbox_bitmap_names_its_cells() {
    let version = GlatVersion::Three { octaboxes: true };
    let maps = vec![BTreeMap::from([(0u16, 1i16)])];
    let octabox = octabox_for(1, (1 << 3) | (1 << 12));
    let index = Index {
        long: false,
        names: false,
    };
    let (glat, gloc) = glat_and_gloc(version, &maps, &[Some(octabox)], index, 1);
    let attributes = Attributes::parse(&gloc, &glat, &Limits::DEFAULT).expect("readable");
    let read = attributes
        .octabox(0)
        .expect("readable")
        .expect("an octabox");
    let cells: Vec<(u8, u8, u8)> = read
        .subboxes
        .iter()
        .map(|sub| (sub.cell, sub.column(), sub.row()))
        .collect();
    // (bit, column, row): the lower right cell, then the upper left — the
    // one example the design counts for these two tables.
    let named = vec![(3u8, 3u8, 0u8), (12, 0, 3)];
    assert_eq!(cells, named, "bit 3 lower right, bit 12 upper left");
}

/// The table of `name`, carried by `face`, as bytes.
fn raw(face: &Sfnt<'_>, name: &[u8; 4]) -> Vec<u8> {
    face.table(tag(name))
        .unwrap_or_else(|| panic!("{} is carried", String::from_utf8_lossy(name)))
        .to_vec()
}

/// A `USHORT` of `table` at `at`.
fn be16(table: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([table[at], table[at + 1]])
}

/// Every vendored face's glyph attributes read whole: each glyph `Gloc`
/// counts, and each attribute number `Gloc` declares, answers without a
/// refusal; the count is `Gloc`'s own, which in Awami Nastaliq passes the
/// face's `maxp`, its pseudo-glyphs among the glyphs `Gloc` indexes.
#[test]
fn the_vendored_faces_attributes_read_whole() {
    let limits = Limits::DEFAULT;
    // (bytes, family, glyphs Gloc counts, the Glat version)
    let faces: [(&[u8], &str, usize, GlatVersion); 3] = [
        (DAI_BANNA, "Dai Banna SIL", 421, GlatVersion::One),
        (TAGMUKAY, "Tagmukay", 347, GlatVersion::One),
        (
            AWAMI,
            "Awami Nastaliq",
            1_727,
            GlatVersion::Three { octaboxes: true },
        ),
    ];
    for (bytes, family, glyphs, version) in faces {
        let face = Sfnt::parse(bytes).unwrap_or_else(|| panic!("{family} parses"));
        let tables = Tables::parse(&face, &limits).unwrap_or_else(|| panic!("{family}'s Silf"));
        assert_eq!(tables.refusals(), &[], "{family} decodes");
        let attributes = tables
            .attributes(&limits)
            .expect("Gloc and Glat")
            .unwrap_or_else(|refusal| panic!("{family}: {refusal:?}"));
        assert_eq!(attributes.version(), version, "{family}");
        assert_eq!(attributes.glyph_count(), glyphs, "{family}");
        for glyph in 0..glyphs {
            let glyph = u16::try_from(glyph).expect("glyph ids are 16 bits");
            for number in 0..attributes.attribute_count() {
                if let Err(refusal) = attributes.lookup(glyph, number) {
                    panic!("{family} glyph {glyph} attribute {number}: {refusal:?}");
                }
            }
        }
    }

    // Awami Nastaliq's `Gloc` indexes past its `maxp`: the pseudo-glyph its
    // `Silf` maps U+200E to, 1 673, has a block, whose break weight —
    // attribute 3, as its `Silf` names it — is the GDL's default of 30.
    let face = Sfnt::parse(AWAMI).expect("Awami Nastaliq parses");
    let real = usize::from(be16(&raw(&face, b"maxp"), 4));
    assert_eq!(real, 1_672);
    let tables = Tables::parse(&face, &limits).expect("a description");
    let attributes = tables.attributes(&limits).expect("both").expect("readable");
    assert!(attributes.glyph_count() > real);
    assert_eq!(attributes.attribute(1_673, 3), Some(30));
    assert_eq!(attributes.attribute(1_673, 0), Some(0));
}

/// An octabox's four diagonals are, in the order stored, the least and
/// greatest x + y and the least and greatest x − y over the glyph's points,
/// each `255 × distance / (width + height)` rounded down: held to every
/// simple glyph of the vendored Awami Nastaliq that has an octabox, its
/// points read from `glyf` by this test's own lines.
#[test]
fn an_octabox_bounds_its_glyphs_outline() {
    let limits = Limits::DEFAULT;
    let face = Sfnt::parse(AWAMI).expect("Awami Nastaliq parses");
    let tables = Tables::parse(&face, &limits).expect("a description");
    let attributes = tables.attributes(&limits).expect("both").expect("readable");
    let head = raw(&face, b"head");
    let loca = raw(&face, b"loca");
    let glyf = raw(&face, b"glyf");
    let glyphs = usize::from(be16(&raw(&face, b"maxp"), 4));
    let long_loca = be16(&head, 50) == 1;
    let location = |glyph: usize| -> usize {
        if long_loca {
            let at = glyph * 4;
            usize::try_from(u32::from_be_bytes([
                loca[at],
                loca[at + 1],
                loca[at + 2],
                loca[at + 3],
            ]))
            .expect("an offset")
        } else {
            usize::from(be16(&loca, glyph * 2)) * 2
        }
    };
    let mut checked = 0usize;
    for glyph in 0..glyphs {
        let (start, end) = (location(glyph), location(glyph + 1));
        if end <= start {
            continue;
        }
        let data = &glyf[start..end];
        let Some(points) = simple_glyph_points(data) else {
            continue;
        };
        let edge = |at: usize| i64::from(i16::from_be_bytes([data[at], data[at + 1]]));
        let (x_min, y_min, x_max, y_max) = (edge(2), edge(4), edge(6), edge(8));
        let span = (x_max - x_min) + (y_max - y_min);
        let glyph_id = u16::try_from(glyph).expect("16 bits");
        let Some(octabox) = attributes.octabox(glyph_id).expect("readable") else {
            continue;
        };
        if span <= 0 || points.is_empty() {
            continue;
        }
        let sums: Vec<i64> = points.iter().map(|(x, y)| x + y).collect();
        let differences: Vec<i64> = points.iter().map(|(x, y)| x - y).collect();
        let scaled = |value: i64, from: i64| (value - from) * 255 / span;
        let least = |values: &[i64]| values.iter().copied().min().expect("points");
        let greatest = |values: &[i64]| values.iter().copied().max().expect("points");
        let expected = [
            scaled(least(&sums), x_min + y_min),
            scaled(greatest(&sums), x_min + y_min),
            scaled(least(&differences), x_min - y_max),
            scaled(greatest(&differences), x_min - y_max),
        ];
        let stored = [
            i64::from(octabox.sum_min()),
            i64::from(octabox.sum_max()),
            i64::from(octabox.difference_min()),
            i64::from(octabox.difference_max()),
        ];
        assert_eq!(stored, expected, "glyph {glyph}");
        checked += 1;
    }
    assert_eq!(checked, 1_194, "every simple glyph with an octabox");
}

/// A simple `glyf` glyph's points, on and off the curve, in font units;
/// `None` for a composite.
fn simple_glyph_points(data: &[u8]) -> Option<Vec<(i64, i64)>> {
    let contours = usize::try_from(i16::from_be_bytes([data[0], data[1]])).ok()?;
    if contours == 0 {
        return Some(Vec::new());
    }
    let ends_at = 10;
    let count = usize::from(be16(data, ends_at + (contours - 1) * 2)) + 1;
    let instructions = usize::from(be16(data, ends_at + contours * 2));
    let mut at = ends_at + contours * 2 + 2 + instructions;
    let mut flags = Vec::with_capacity(count);
    while flags.len() < count {
        let flag = data[at];
        at += 1;
        flags.push(flag);
        if flag & 8 != 0 {
            let repeats = data[at];
            at += 1;
            for _ in 0..repeats {
                flags.push(flag);
            }
        }
    }
    flags.truncate(count);
    let mut axis = |short: u8, same: u8| -> Vec<i64> {
        let mut out = Vec::with_capacity(count);
        let mut value = 0i64;
        for flag in &flags {
            if flag & short != 0 {
                let delta = i64::from(data[at]);
                at += 1;
                value += if flag & same != 0 { delta } else { -delta };
            } else if flag & same == 0 {
                value += i64::from(i16::from_be_bytes([data[at], data[at + 1]]));
                at += 2;
            }
            out.push(value);
        }
        out
    };
    let xs = axis(2, 16);
    let ys = axis(4, 32);
    Some(xs.into_iter().zip(ys).collect())
}

// --- features and languages (FT-07d.5) ------------------------------------
//
// `feat_table` and `sill_table` write the two tables from what a test states
// — each feature's id, flags and settings in order, each language's code and
// settings — and the values the shaper gives are compared with what the
// statements and the design's order make of them.

/// A `Feat` 2 of `features`, each its id, flags and setting values in order.
fn feat_table(features: &[(u32, u16, &[i16])]) -> Vec<u8> {
    let mut out = 0x0002_0000u32.to_be_bytes().to_vec();
    out.extend(u16s(&[
        u16::try_from(features.len()).expect("a few features"),
        0,
        0,
        0,
    ]));
    let mut offset = 12 + features.len() * 16;
    let mut settings: Vec<u8> = Vec::new();
    for (index, (id, flags, values)) in features.iter().enumerate() {
        out.extend(id.to_be_bytes());
        out.extend(u16s(&[u16::try_from(values.len()).expect("a few"), 0]));
        out.extend(u32::try_from(offset).expect("small").to_be_bytes());
        let label = 256 + u16::try_from(index).expect("small");
        out.extend(u16s(&[*flags, label]));
        for (n, value) in values.iter().enumerate() {
            settings.extend(value.to_be_bytes());
            settings.extend((label * 16 + u16::try_from(n).expect("small")).to_be_bytes());
        }
        offset += values.len() * 4;
    }
    out.extend(settings);
    out
}

/// A `Sill` of `languages`, each its code and its settings, and the GTF's
/// one entry more.
fn sill_table(languages: &[([u8; 4], &[(u32, i16)])]) -> Vec<u8> {
    let count = u16::try_from(languages.len()).expect("a few");
    let mut out = 0x0001_0000u32.to_be_bytes().to_vec();
    out.extend(u16s(&[count, 0, 0, 0]));
    let mut offset = 12 + (languages.len() + 1) * 8;
    let mut settings: Vec<u8> = Vec::new();
    for (code, values) in languages {
        out.extend_from_slice(code);
        out.extend(u16s(&[
            u16::try_from(values.len()).expect("a few"),
            u16::try_from(offset).expect("small"),
        ]));
        for (id, value) in values.iter() {
            settings.extend(id.to_be_bytes());
            settings.extend(value.to_be_bytes());
            settings.extend([0, 0]);
        }
        offset += values.len() * 8;
    }
    // The bogus entry, whose offset finds the last language's end.
    out.extend(*b"+1\0\0");
    out.extend(u16s(&[0, u16::try_from(offset).expect("small")]));
    out.extend(settings);
    out
}

/// A feature id from its four characters.
fn id(tag: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*tag)
}

/// The Graphite face with `feat` and `sill` in place of its own.
fn face_with(feat: Vec<u8>, sill: Vec<u8>) -> Face {
    graphite_face().with(b"Feat", feat).with(b"Sill", sill)
}

/// The features most tests ask about: a boolean pair, one whose default is
/// listed first and is not 0, one of three values, the language feature of
/// no settings, and one whose flags name its default by index.
fn five_features() -> Vec<u8> {
    feat_table(&[
        (id(b"ss01"), 0x8000, &[0, 1]),
        (id(b"hehk"), 0x8000, &[1, 0]),
        (id(b"punc"), 0x8000, &[0, 1, 2]),
        (1, 0x8000, &[]),
        (id(b"cv78"), 0xC001, &[1, 2]),
    ])
}

/// The languages most tests ask about.
fn two_languages() -> Vec<u8> {
    sill_table(&[
        (*b"ks\0\0", &[(id(b"punc"), 1), (id(b"ss01"), 1)]),
        (*b"urd\0", &[(id(b"hehk"), 0)]),
    ])
}

/// What a test asks a shaper for: its settings, its own features where it
/// names them, and its language.
#[derive(Clone, Copy, Debug, Default)]
struct Ask<'a> {
    settings: &'a [(Tag, u32)],
    features: Option<(&'a [Tag], &'a [Tag])>,
    language: Option<Tag>,
}

impl<'a> Ask<'a> {
    fn settings(settings: &'a [(Tag, u32)]) -> Ask<'a> {
        Ask {
            settings,
            ..Ask::default()
        }
    }

    fn language(tag: &[u8; 4]) -> Ask<'a> {
        Ask {
            language: Some(Tag::new(tag)),
            ..Ask::default()
        }
    }
}

/// The feature values and refusals of the face `bytes` through a shaper
/// asked for `ask`.
fn values_in(bytes: &[u8], ask: &Ask<'_>) -> (Vec<i16>, Vec<Refusal>) {
    let sfnt = Sfnt::parse(bytes).expect("the face parses");
    let mut shaper = Shaper::new(&sfnt).with_settings(ask.settings);
    if let Some((gsub, gpos)) = ask.features {
        shaper = shaper.with_features(gsub, gpos);
    }
    if let Some(language) = ask.language {
        shaper = shaper.with_language(language);
    }
    let (values, refusals) = shaper.graphite_features().expect("a Graphite description");
    (values.as_slice().to_vec(), refusals)
}

/// [`values_in`] for a face this test describes.
fn values_of(face: &Face, ask: &Ask<'_>) -> (Vec<i16>, Vec<Refusal>) {
    values_in(&face.build(), ask)
}

/// A feature keeps its default — its first setting, or the one its flags
/// name — unless a setting or a turned-on feature names it, and a face with
/// no `Silf` has no Graphite features to value at all.
#[test]
fn a_feature_keeps_its_default_unless_a_setting_names_it() {
    let face = face_with(five_features(), two_languages());
    let defaults = vec![0, 1, 0, 0, 2];
    assert_eq!(
        values_of(&face, &Ask::default()),
        (defaults.clone(), vec![])
    );

    let punc = [(Tag::new(b"punc"), 2)];
    assert_eq!(
        values_of(&face, &Ask::settings(&punc)),
        (vec![0, 1, 2, 0, 2], vec![])
    );
    let hehk = [(Tag::new(b"hehk"), 0)];
    assert_eq!(
        values_of(&face, &Ask::settings(&hehk)),
        (vec![0, 0, 0, 0, 2], vec![])
    );
    // The language feature has no settings: the GDL's boolean, 0 or 1.
    let lang = [(Tag(1), 1)];
    assert_eq!(
        values_of(&face, &Ask::settings(&lang)),
        (vec![0, 1, 0, 1, 2], vec![])
    );
    // A turned-on feature is set to 1, and every other keeps its default.
    let on = [Tag::new(b"ss01")];
    assert_eq!(
        values_of(
            &face,
            &Ask {
                features: Some((&on[..], &[][..])),
                ..Ask::default()
            }
        ),
        (vec![1, 1, 0, 0, 2], vec![])
    );
    assert_eq!(
        values_of(
            &face,
            &Ask {
                features: Some((&[][..], &on[..])),
                ..Ask::default()
            }
        ),
        (vec![1, 1, 0, 0, 2], vec![])
    );
    // Settings over a turned-on feature: the setting is the later word.
    let off = [(Tag::new(b"ss01"), 0)];
    assert_eq!(
        values_of(
            &face,
            &Ask {
                settings: &off,
                features: Some((&on[..], &[][..])),
                language: None,
            }
        ),
        (defaults, vec![])
    );

    // A face with no `Silf` is no Graphite description.
    let plain = face.clone().without(b"Silf");
    let bytes = plain.build();
    let sfnt = Sfnt::parse(&bytes).expect("parses");
    assert!(Shaper::new(&sfnt).graphite_features().is_none());
    // One with a `Silf` and no `Feat` has none to value.
    let bare = face.without(b"Feat");
    assert_eq!(values_of(&bare, &Ask::default()), (vec![], vec![]));
}

/// A language `Sill` lists takes its settings, asked for by a tag whose
/// letters are its code; one `Sill` does not list takes the defaults and is
/// not named; and the caller's settings stand over the language's.
#[test]
fn a_language_takes_its_sill_settings() {
    let face = face_with(five_features(), two_languages());
    assert_eq!(
        values_of(&face, &Ask::language(b"URD ")),
        (vec![0, 0, 0, 0, 2], vec![])
    );
    assert_eq!(
        values_of(&face, &Ask::language(b"urd ")),
        (vec![0, 0, 0, 0, 2], vec![])
    );
    assert_eq!(
        values_of(&face, &Ask::language(b"ks  ")),
        (vec![1, 1, 1, 0, 2], vec![])
    );
    // An OpenType language tag whose letters are not a code `Sill` lists
    // selects nothing, and nothing is named: most languages are not listed.
    assert_eq!(
        values_of(&face, &Ask::language(b"TRK ")),
        (vec![0, 1, 0, 0, 2], vec![])
    );
    let hehk = [(Tag::new(b"hehk"), 1)];
    assert_eq!(
        values_of(
            &face,
            &Ask {
                settings: &hehk,
                ..Ask::language(b"urd ")
            }
        ),
        (vec![0, 1, 0, 0, 2], vec![])
    );
    let on = [Tag::new(b"ss01")];
    assert_eq!(
        values_of(
            &face,
            &Ask {
                features: Some((&on[..], &[][..])),
                ..Ask::language(b"urd ")
            }
        ),
        (vec![1, 0, 0, 0, 2], vec![])
    );
    // A `Sill` value is the face's own statement, taken as written even
    // where the feature does not declare it.
    let odd = face_with(
        five_features(),
        sill_table(&[(*b"xyz\0", &[(id(b"punc"), 9)])]),
    );
    assert_eq!(
        values_of(&odd, &Ask::language(b"xyz ")),
        (vec![0, 1, 9, 0, 2], vec![])
    );
}

/// A tag naming no feature the face declares, and a value a feature does not
/// declare, is ignored and named, each once; what is declared still applies.
#[test]
fn a_setting_the_face_does_not_declare_is_ignored_and_named() {
    let face = face_with(five_features(), two_languages());
    let settings = [
        (Tag::new(b"kern"), 0),
        (Tag::new(b"punc"), 7),
        (Tag::new(b"ss01"), 1),
        (Tag::new(b"kern"), 0),
        (Tag::new(b"hehk"), 70_000),
    ];
    assert_eq!(
        values_of(&face, &Ask::settings(&settings)),
        (
            vec![1, 1, 0, 0, 2],
            vec![
                Refusal::UndeclaredFeature {
                    feature: id(b"kern")
                },
                Refusal::UndeclaredSetting {
                    feature: id(b"punc"),
                    value: 7
                },
                Refusal::UndeclaredSetting {
                    feature: id(b"hehk"),
                    value: 70_000
                },
            ]
        )
    );
    // `with_features` names OpenType's features, and a Graphite face has
    // none of them: each is named once, whichever list names it.
    let gsub = [Tag::new(b"liga"), Tag::new(b"ccmp")];
    let gpos = [Tag::new(b"kern"), Tag::new(b"liga")];
    assert_eq!(
        values_of(
            &face,
            &Ask {
                features: Some((&gsub[..], &gpos[..])),
                ..Ask::default()
            }
        ),
        (
            vec![0, 1, 0, 0, 2],
            vec![
                Refusal::UndeclaredFeature {
                    feature: id(b"liga")
                },
                Refusal::UndeclaredFeature {
                    feature: id(b"ccmp")
                },
                Refusal::UndeclaredFeature {
                    feature: id(b"kern")
                },
            ]
        )
    );
    // A feature declaring 2 and not 1 is not turned on by being named.
    let two = face_with(
        feat_table(&[(id(b"alts"), 0x8000, &[0, 2])]),
        two_languages(),
    );
    let on = [Tag::new(b"alts")];
    assert_eq!(
        values_of(
            &two,
            &Ask {
                features: Some((&on[..], &[][..])),
                ..Ask::default()
            }
        ),
        (
            vec![0],
            vec![Refusal::UndeclaredSetting {
                feature: id(b"alts"),
                value: 1
            }]
        )
    );
    // A language's setting naming a feature `Feat` does not declare.
    let stray = face_with(
        five_features(),
        sill_table(&[(*b"xx\0\0", &[(id(b"zzzz"), 1), (id(b"ss01"), 1)])]),
    );
    assert_eq!(
        values_of(&stray, &Ask::language(b"xx  ")),
        (
            vec![1, 1, 0, 0, 2],
            vec![Refusal::UndeclaredFeature {
                feature: id(b"zzzz")
            }]
        )
    );
    // A `Feat` or `Sill` that cannot be read is answered without, and named.
    let mut broken = five_features();
    broken[1] = 9;
    let unreadable = face_with(broken, two_languages());
    assert_eq!(
        values_of(&unreadable, &Ask::default()),
        (
            vec![],
            vec![Refusal::UnknownVersion {
                table: graphite::FEAT,
                version: 0x0009_0000
            }]
        )
    );
    let mut broken = two_languages();
    broken[1] = 7;
    let unreadable = face_with(five_features(), broken);
    assert_eq!(
        values_of(&unreadable, &Ask::language(b"ks  ")),
        (
            vec![0, 1, 0, 0, 2],
            vec![Refusal::UnknownVersion {
                table: graphite::SILL,
                version: 0x0007_0000
            }]
        )
    );
}

/// The `feat` chapter's and the GDL's feature examples, transcribed as
/// `Feat` bytes and counted.
#[test]
fn the_feat_examples_hold() {
    let mut held = 0usize;

    // The `feat` chapter: "number spacing", flags 0xC001 — exclusive, with
    // the default named by its low byte, setting 1: proportional numbers.
    let spacing = face_with(
        feat_table(&[(id(b"nspc"), 0xC001, &[0, 1])]),
        two_languages(),
    );
    assert_eq!(values_of(&spacing, &Ask::default()), (vec![1], vec![]));
    held += 1;

    // The GDL's `ligatures`: settings all 3, std 2, min 1 and no 0, the
    // default std, which a compiled `Feat` lists first.
    let ligatures = face_with(
        feat_table(&[(id(b"ligs"), 0x8000, &[2, 3, 1, 0])]),
        two_languages(),
    );
    assert_eq!(values_of(&ligatures, &Ask::default()), (vec![2], vec![]));
    let none = [(Tag::new(b"ligs"), 0)];
    assert_eq!(
        values_of(&ligatures, &Ask::settings(&none)),
        (vec![0], vec![])
    );
    held += 1;

    // The GDL's `eng`: its public id `cv43` and its hidden `Engs`, two
    // features, the second an alias, each keeping a value of its own.
    let eng = feat_table(&[
        (id(b"cv43"), 0x8000, &[0, 1, 2, 3]),
        (id(b"Engs"), 0x8001, &[0, 1, 2, 3]),
    ]);
    let features = graphite::Features::parse(&eng).expect("readable");
    assert!(!features.feature(0).expect("cv43").is_alias());
    assert!(features.feature(1).expect("Engs").is_alias());
    let face = face_with(eng, two_languages());
    let capital = [(Tag::new(b"Engs"), 2)];
    assert_eq!(
        values_of(&face, &Ask::settings(&capital)),
        (vec![0, 2], vec![])
    );
    held += 1;

    assert_eq!(held, 3, "the Feat examples the design counts");
}

/// The GTF's and the GDL's language examples, transcribed as `Sill` bytes
/// and counted.
#[test]
fn the_sill_examples_hold() {
    let mut held = 0usize;

    // "The code “en” is represented by the four bytes [101, 110, 0, 0]."
    assert_eq!(
        graphite::Languages::code_for(Tag::new(b"en  ")),
        [101, 110, 0, 0]
    );
    let english = face_with(
        five_features(),
        sill_table(&[([101, 110, 0, 0], &[(id(b"ss01"), 1)])]),
    );
    assert_eq!(
        values_of(&english, &Ask::language(b"en  ")),
        (vec![1, 1, 0, 0, 2], vec![])
    );
    held += 1;

    // The GDL's language table for Arabic script: three groups, each a list
    // of codes and the settings they share. Its settings are names; here
    // each feature's values number them: `meem` none 0 and sindhi 1, `edig`
    // none 0, sindhi 1 and urdu 2, `shkr` none 0 and sindhiUrdu 1, `hehA`
    // none 0, kurdish 1 and urdu 2.
    let features = feat_table(&[
        (id(b"meem"), 0x8000, &[0, 1]),
        (id(b"edig"), 0x8000, &[0, 1, 2]),
        (id(b"shkr"), 0x8000, &[0, 1]),
        (id(b"hehA"), 0x8000, &[0, 1, 2]),
    ]);
    let sindhi: &[(u32, i16)] = &[(id(b"meem"), 1), (id(b"edig"), 1), (id(b"shkr"), 1)];
    let kurdish: &[(u32, i16)] = &[(id(b"hehA"), 1)];
    let urdu: &[(u32, i16)] = &[(id(b"hehA"), 2), (id(b"edig"), 2), (id(b"shkr"), 1)];
    let groups: [(&[&[u8; 4]], &[(u32, i16)], [i16; 4]); 3] = [
        (&[b"snd\0", b"sd\0\0"], sindhi, [1, 1, 1, 0]),
        (
            &[b"bdh\0", b"ckb\0", b"kmr\0", b"kur\0", b"sdh\0"],
            kurdish,
            [0, 0, 0, 1],
        ),
        (&[b"urd\0", b"ur\0\0"], urdu, [0, 2, 1, 2]),
    ];
    let mut entries: Vec<([u8; 4], &[(u32, i16)])> = Vec::new();
    for (codes, settings, _) in &groups {
        for code in *codes {
            entries.push((**code, *settings));
        }
    }
    entries.sort_by_key(|(code, _)| *code);
    let face = face_with(features, sill_table(&entries));
    for (codes, _, expected) in &groups {
        for code in *codes {
            let mut tag = **code;
            for byte in &mut tag {
                if *byte == 0 {
                    *byte = b' ';
                }
            }
            assert_eq!(
                values_of(&face, &Ask::language(&tag)),
                (expected.to_vec(), vec![]),
                "{}",
                String::from_utf8_lossy(&tag)
            );
        }
    }
    held += 1;

    assert_eq!(held, 2, "the Sill examples the design counts");
}

/// The vendored faces' `Feat` and `Sill` read as their authors compiled
/// them: Dai Banna SIL's `Feat` 1 holds the language feature alone, and
/// Awami Nastaliq's twelve features take their first settings as defaults
/// and its languages their `Sill` settings.
#[test]
fn the_vendored_faces_features_and_languages_read() {
    assert_eq!(values_in(DAI_BANNA, &Ask::default()), (vec![0], vec![]));
    let face = Sfnt::parse(DAI_BANNA).expect("parses");
    let feat = face.table(tag(b"Feat")).expect("a Feat");
    let features = graphite::Features::parse(feat).expect("Feat 1 reads");
    let only = features.feature(0).expect("one feature");
    assert_eq!((only.id, only.settings), (1, 0));

    assert_eq!(
        values_in(TAGMUKAY, &Ask::default()),
        (vec![0, 0, 0], vec![])
    );
    let ss01 = [(Tag::new(b"ss01"), 1)];
    assert_eq!(
        values_in(TAGMUKAY, &Ask::settings(&ss01)),
        (vec![1, 0, 0], vec![])
    );

    // hehk hedo lamv cv85 punc cv78 hamz hihm wdsp shrt agca, and the
    // language feature.
    let defaults = vec![1, 0, 0, 0, 0, 1, 0, 0, 2, 3, 3, 0];
    assert_eq!(
        values_in(AWAMI, &Ask::default()),
        (defaults.clone(), vec![])
    );
    let mut kashmiri = defaults.clone();
    kashmiri[6] = 1;
    assert_eq!(
        values_in(AWAMI, &Ask::language(b"ks  ")),
        (kashmiri.clone(), vec![])
    );
    assert_eq!(
        values_in(AWAMI, &Ask::language(b"kas ")),
        (kashmiri, vec![])
    );
    let mut gujari = defaults.clone();
    gujari[2] = 1;
    assert_eq!(values_in(AWAMI, &Ask::language(b"gju ")), (gujari, vec![]));
    let mut malay = defaults;
    malay[6] = 1;
    malay[7] = 1;
    assert_eq!(values_in(AWAMI, &Ask::language(b"msa ")), (malay, vec![]));
}

// --- `Silf` and its machines (FT-07d.6) ----------------------------------
//
// `PassSpec` states a pass as the GTF lays one out — its ranges, its states
// and transitions, the rules each accepting state lists, and each rule's sort
// key, pre-context and code — and `silf_table` writes a `Silf` of one
// description around such passes at any version the design reads. The GDL's
// rule examples are hand-compiled into these, state by state, beside the
// rules they compile.

/// A pass as a test states it.
#[derive(Clone, Debug, Default)]
struct PassSpec {
    flags: u8,
    max_rule_loop: u8,
    max_rule_context: u8,
    max_backup: u8,
    /// `(first, last, column)`.
    ranges: Vec<(u16, u16, u16)>,
    columns: u16,
    rows: u16,
    transitional: u16,
    success: u16,
    /// Each success state's rules, from state `rows - success` on.
    rule_lists: Vec<Vec<u16>>,
    min_pre: u8,
    max_pre: u8,
    start_states: Vec<u16>,
    /// Each rule's sort key, pre-context, constraint and action.
    rules: Vec<(u16, u8, Vec<u8>, Vec<u8>)>,
    /// One row a transitional state, one entry a column.
    transitions: Vec<Vec<u16>>,
    pass_constraint: Vec<u8>,
    collision_threshold: u8,
}

/// The pass `spec` as a `Silf` of major version `major` stores it, its code
/// offsets counted from `base`, the pass's offset in its description.
fn pass_bytes(spec: &PassSpec, major: u32, base: usize) -> Vec<u8> {
    let rule_count = u16::try_from(spec.rules.len()).expect("a few rules");
    let mut fixed: Vec<u8> = vec![
        spec.flags,
        spec.max_rule_loop,
        spec.max_rule_context,
        spec.max_backup,
    ];
    fixed.extend(rule_count.to_be_bytes());
    // The offsets of the code are patched once the fixed part's length is
    // known; their places are remembered here.
    let code_slot = fixed.len();
    if major >= 2 {
        fixed.extend([0u8; 2 + 4]);
    }
    fixed.extend([0u8; 4 + 4 + 4]);
    fixed.extend(u16s(&[
        spec.rows,
        spec.transitional,
        spec.success,
        spec.columns,
        u16::try_from(spec.ranges.len()).expect("a few"),
        0,
        0,
        0,
    ]));
    for (first, last, column) in &spec.ranges {
        fixed.extend(u16s(&[*first, *last, *column]));
    }
    let mut offset = 0u16;
    let mut rule_map: Vec<u16> = Vec::new();
    fixed.extend(offset.to_be_bytes());
    for list in &spec.rule_lists {
        offset += u16::try_from(list.len()).expect("a few");
        fixed.extend(offset.to_be_bytes());
        rule_map.extend(list);
    }
    fixed.extend(u16s(&rule_map));
    fixed.extend([spec.min_pre, spec.max_pre]);
    fixed.extend(u16s(&spec.start_states));
    fixed.extend(u16s(
        &spec
            .rules
            .iter()
            .map(|(key, ..)| *key)
            .collect::<Vec<u16>>(),
    ));
    fixed.extend(spec.rules.iter().map(|(_, pre, ..)| *pre));
    if major >= 2 {
        fixed.push(spec.collision_threshold);
        fixed.extend(
            u16::try_from(spec.pass_constraint.len())
                .expect("short")
                .to_be_bytes(),
        );
    }
    let mut constraints: Vec<u8> = Vec::new();
    let mut constraint_offsets = vec![0u16];
    let mut actions: Vec<u8> = Vec::new();
    let mut action_offsets = vec![0u16];
    for (_, _, constraint, action) in &spec.rules {
        constraints.extend(constraint);
        constraint_offsets.push(u16::try_from(constraints.len()).expect("short"));
        actions.extend(action);
        action_offsets.push(u16::try_from(actions.len()).expect("short"));
    }
    fixed.extend(u16s(&constraint_offsets));
    fixed.extend(u16s(&action_offsets));
    for row in &spec.transitions {
        assert_eq!(row.len(), usize::from(spec.columns), "a cell a column");
        fixed.extend(u16s(row));
    }
    if major >= 2 {
        fixed.push(0);
    }
    let pass_code = base + fixed.len();
    let rule_code = pass_code + spec.pass_constraint.len();
    let action_code = rule_code + constraints.len();
    let mut at = code_slot;
    if major >= 2 {
        // `fsmOffset`: the offset of the state counts in the pass.
        let fsm = u16::try_from(code_slot + 2 + 4 + 12).expect("small");
        fixed[at..at + 2].copy_from_slice(&fsm.to_be_bytes());
        at += 2;
        fixed[at..at + 4].copy_from_slice(&u32::try_from(pass_code).expect("small").to_be_bytes());
        at += 4;
    }
    fixed[at..at + 4].copy_from_slice(&u32::try_from(rule_code).expect("small").to_be_bytes());
    fixed[at + 4..at + 8]
        .copy_from_slice(&u32::try_from(action_code).expect("small").to_be_bytes());
    let mut out = fixed;
    out.extend(&spec.pass_constraint);
    out.extend(constraints);
    out.extend(actions);
    out
}

/// A description as a test states it.
#[derive(Clone, Debug, Default)]
struct DescriptionSpec {
    script_tags: Vec<[u8; 4]>,
    /// `(code point, glyph)`, in the order stored.
    pseudos: Vec<(u32, u16)>,
    class_map: Vec<u8>,
    passes: Vec<PassSpec>,
    /// `iSubst`, `iPos`, `iJust`, `iBidi`.
    order: [u8; 4],
}

/// A class map of `linear` classes and then `lookups`, its offsets `ULONG`s
/// where `wide`.
fn class_map(linear: &[Vec<u16>], lookups: &[Vec<(u16, u16)>], wide: bool) -> Vec<u8> {
    let count = linear.len() + lookups.len();
    let head = 4 + (count + 1) * if wide { 4 } else { 2 };
    let mut body: Vec<u8> = Vec::new();
    let mut offsets = vec![head];
    for class in linear {
        body.extend(u16s(class));
        offsets.push(head + body.len());
    }
    for class in lookups {
        let n = u16::try_from(class.len()).expect("a few");
        let range = if n == 0 {
            0
        } else {
            1u16 << (15 - n.leading_zeros())
        };
        let selector = if range == 0 {
            0
        } else {
            15 - range.leading_zeros()
        };
        body.extend(u16s(&[
            n,
            range,
            u16::try_from(selector).expect("small"),
            n - range,
        ]));
        for (glyph, index) in class {
            body.extend(u16s(&[*glyph, *index]));
        }
        offsets.push(head + body.len());
    }
    let mut out = u16s(&[
        u16::try_from(count).expect("a few"),
        u16::try_from(linear.len()).expect("a few"),
    ]);
    for offset in offsets {
        if wide {
            out.extend(u32::try_from(offset).expect("small").to_be_bytes());
        } else {
            out.extend(u16::try_from(offset).expect("small").to_be_bytes());
        }
    }
    out.extend(body);
    out
}

/// The description `spec` as a `Silf` of major version `major` stores it.
fn description_bytes(spec: &DescriptionSpec, major: u32) -> Vec<u8> {
    let mut head: Vec<u8> = Vec::new();
    if major >= 3 {
        head.extend(0x0004_0000u32.to_be_bytes());
        head.extend([0u8; 4]);
    }
    let passes = u8::try_from(spec.passes.len()).expect("a few passes");
    head.extend(u16s(&[500, 0, 0]));
    head.extend([passes]);
    head.extend(spec.order);
    // flags, the cross-line contexts, and the pseudo, break weight and
    // directionality attributes.
    head.extend([0, 0, 0, 0, 1, 2]);
    if major >= 2 {
        // Mirroring and skip-pass attributes, and no justification levels.
        head.extend([3, 4, 0]);
    }
    head.extend(u16s(&[0]));
    // User attributes, components a ligature, direction, collisions, and
    // two reserved bytes.
    head.extend([0, 0, 1, 0, 0, 0]);
    if major >= 2 {
        // The third reserved byte, no critical features, a reserved byte.
        head.extend([0, 0, 0]);
    }
    head.push(u8::try_from(spec.script_tags.len()).expect("a few"));
    for tag in &spec.script_tags {
        head.extend_from_slice(tag);
    }
    head.extend(u16s(&[499]));
    let pass_offset = head.len();
    head.extend(vec![0u8; (spec.passes.len() + 1) * 4]);
    let pseudos_offset = head.len();
    head.extend(u16s(&[
        u16::try_from(spec.pseudos.len()).expect("a few"),
        0,
        0,
        0,
    ]));
    for (code, glyph) in &spec.pseudos {
        head.extend(code.to_be_bytes());
        head.extend(glyph.to_be_bytes());
    }
    head.extend(&spec.class_map);
    if major >= 3 {
        head[4..6].copy_from_slice(&u16::try_from(pass_offset).expect("small").to_be_bytes());
        head[6..8].copy_from_slice(&u16::try_from(pseudos_offset).expect("small").to_be_bytes());
    }
    let mut offsets = Vec::new();
    let mut body: Vec<u8> = Vec::new();
    for pass in &spec.passes {
        let at = head.len() + body.len();
        offsets.push(at);
        body.extend(pass_bytes(pass, major, at));
    }
    offsets.push(head.len() + body.len());
    for (n, offset) in offsets.iter().enumerate() {
        let at = pass_offset + n * 4;
        head[at..at + 4].copy_from_slice(&u32::try_from(*offset).expect("small").to_be_bytes());
    }
    head.extend(body);
    head
}

/// A `Silf` of major version `major` holding `descriptions`.
fn silf_table(major: u32, descriptions: &[DescriptionSpec]) -> Vec<u8> {
    let mut out = (major << 16).to_be_bytes().to_vec();
    if major >= 3 {
        out.extend(0x0004_0002u32.to_be_bytes());
    }
    out.extend(u16s(&[
        u16::try_from(descriptions.len()).expect("a few"),
        0,
    ]));
    let mut at = out.len() + descriptions.len() * 4;
    let mut bodies: Vec<Vec<u8>> = Vec::new();
    for spec in descriptions {
        out.extend(u32::try_from(at).expect("small").to_be_bytes());
        let body = description_bytes(spec, major);
        at += body.len();
        bodies.push(body);
    }
    for body in bodies {
        out.extend(body);
    }
    out
}

/// A description of the one pass `pass` and nothing else.
fn one_pass(pass: PassSpec) -> DescriptionSpec {
    DescriptionSpec {
        class_map: class_map(&[], &[], true),
        passes: vec![pass],
        order: [0, 1, 1, 255],
        ..DescriptionSpec::default()
    }
}

/// Pass 0 of the description in `silf`.
fn first_pass(silf: &[u8]) -> graphite::Pass<'_> {
    let table = graphite::Silf::parse(silf).expect("the Silf reads");
    let description = table.description(0).expect("its description");
    description
        .pass(0, &Limits::DEFAULT)
        .expect("its first pass")
}

/// The rules and their ends the machine finds at each position of `glyphs`
/// where it finds any.
fn found_at(pass: &graphite::Pass<'_>, glyphs: &[u16]) -> BTreeMap<usize, Vec<(u16, usize)>> {
    let mut out = BTreeMap::new();
    for position in 0..=glyphs.len() {
        let matched = pass.candidates(glyphs, position, &Limits::DEFAULT);
        assert!(matched.refusals.is_empty(), "{:?}", matched.refusals);
        if !matched.candidates.is_empty() {
            out.insert(
                position,
                matched.candidates.iter().map(|c| (c.rule, c.end)).collect(),
            );
        }
    }
    out
}

/// The GDL's rule examples, hand-compiled into machines as the GTF lays
/// them out, each pass's machine finding at each position the rules the GDL
/// says it does, in its order: counted.
#[test]
fn a_pass_finds_the_rule_its_machine_selects() {
    let mut held = 0usize;

    // 4.1.1: `X > Y / A _ A` over `A X A X A X A`. A is glyph 1 and column
    // 0, X glyph 2 and column 1; one pre-context slot, so the machine reads
    // from the slot before the position. States: 0 –A→ 1 –X→ 2 –A→ 3, the
    // last accepting rule 0.
    let rescan = PassSpec {
        ranges: vec![(1, 1, 0), (2, 2, 1)],
        columns: 2,
        rows: 4,
        transitional: 3,
        success: 1,
        rule_lists: vec![vec![0]],
        min_pre: 1,
        max_pre: 1,
        start_states: vec![0],
        rules: vec![(3, 1, vec![], vec![0x31])],
        transitions: vec![vec![1, 0], vec![0, 2], vec![3, 0]],
        ..PassSpec::default()
    };
    let silf = silf_table(4, &[one_pass(rescan)]);
    let pass = first_pass(&silf);
    // "In order for the output we might expect (A Y A Y A Y A)": the rule
    // matches at each X.
    assert_eq!(
        found_at(&pass, &[1, 2, 1, 2, 1, 2, 1]),
        BTreeMap::from([(1, vec![(0, 3)]), (3, vec![(0, 5)]), (5, vec![(0, 7)])])
    );
    held += 1;

    // 4.1.7: rule 0 `A B C > X Y Z / _ _ _` and rule 1 `A > D / _ B`. A, B
    // and C are glyphs 1 to 3, columns 0 to 2. States: 0 –A→ 1 –B→ 2 –C→ 3;
    // 2 accepts rule 1 and goes on, 3 accepts rule 0. "Longer matches have
    // priority over shorter matches."
    let ordering = PassSpec {
        ranges: vec![(1, 1, 0), (2, 2, 1), (3, 3, 2)],
        columns: 3,
        rows: 4,
        transitional: 3,
        success: 2,
        rule_lists: vec![vec![1], vec![0]],
        start_states: vec![0],
        rules: vec![(3, 0, vec![], vec![0x31]), (2, 0, vec![], vec![0x31])],
        transitions: vec![vec![1, 0, 0], vec![0, 2, 0], vec![0, 0, 3]],
        ..PassSpec::default()
    };
    let silf = silf_table(4, &[one_pass(ordering)]);
    let pass = first_pass(&silf);
    assert_eq!(
        found_at(&pass, &[1, 2, 3]),
        BTreeMap::from([(0, vec![(0, 3), (1, 2)])])
    );
    held += 1;

    // 4.1.7.1: `B > Y / W A? _ C`, resolved to rule 0 `B > Y / W A _ C` and
    // rule 1 `B > Y / W _ C`, and rule 2 `C > D / B _ E`. W, A, B, C and E
    // are glyphs 1 to 5, columns 0 to 4. The longest pre-context is two, so
    // rules 1 and 2 are read from one slot further back, the compiler's
    // `ANY` in front of them. States: 0 reads the first slot (W → 1, any
    // other → 2); 1 holds all three rules, 2 rules 1 and 2; 3 is rule 1
    // after its W, 4 rule 0 after W A, 5 rule 2 after its B; 6, 7 and 8 one
    // slot on; 9, 10 and 11 accept rules 1, 0 and 2.
    let optional = PassSpec {
        ranges: vec![(1, 1, 0), (2, 2, 1), (3, 3, 2), (4, 4, 3), (5, 5, 4)],
        columns: 5,
        rows: 12,
        transitional: 9,
        success: 3,
        rule_lists: vec![vec![1], vec![0], vec![2]],
        min_pre: 1,
        max_pre: 2,
        // Two slots before the position, from state 0; one, from the state
        // after the `ANY` the missing slot stands for.
        start_states: vec![0, 2],
        rules: vec![
            (4, 2, vec![], vec![0x31]),
            (3, 1, vec![], vec![0x31]),
            (3, 1, vec![], vec![0x31]),
        ],
        transitions: vec![
            vec![1, 2, 2, 2, 2],
            vec![3, 4, 5, 0, 0],
            vec![3, 0, 5, 0, 0],
            vec![0, 0, 6, 0, 0],
            vec![0, 0, 7, 0, 0],
            vec![0, 0, 0, 8, 0],
            vec![0, 0, 0, 9, 0],
            vec![0, 0, 0, 10, 0],
            vec![0, 0, 0, 0, 11],
        ],
        ..PassSpec::default()
    };
    let silf = silf_table(4, &[one_pass(optional)]);
    let pass = first_pass(&silf);
    // "WABC" becomes "WAYC": rule 0 at the B.
    assert_eq!(
        found_at(&pass, &[1, 2, 3, 4]),
        BTreeMap::from([(2, vec![(0, 4)])])
    );
    held += 1;
    // "WBCE" becomes "WYCE": the shorter rule 1, at the B, is reached first;
    // rule 2 would match at the C only on the input as it stands.
    assert_eq!(
        found_at(&pass, &[1, 3, 4, 5]),
        BTreeMap::from([(1, vec![(1, 3)]), (2, vec![(2, 4)])])
    );
    held += 1;

    // 4.1.7.2: rule 0 `A > B / W _` and rule 1 `A > C / _ X Y` over `WAXY`.
    // W, A, X and Y are glyphs 1 to 4, columns 0 to 3; rule 1 is read from
    // one slot back. States: 0 –W→ 1, other → 2; 1 –A→ 5, accepting rule 0
    // and going on; 2 –A→ 3; 3 and 5 –X→ 4; 4 –Y→ 6, accepting rule 1.
    let context = PassSpec {
        ranges: vec![(1, 1, 0), (2, 2, 1), (3, 3, 2), (4, 4, 3)],
        columns: 4,
        rows: 7,
        transitional: 6,
        success: 2,
        rule_lists: vec![vec![0], vec![1]],
        min_pre: 0,
        max_pre: 1,
        start_states: vec![0, 2],
        rules: vec![(2, 1, vec![], vec![0x31]), (3, 0, vec![], vec![0x31])],
        transitions: vec![
            vec![1, 2, 2, 2],
            vec![0, 5, 0, 0],
            vec![0, 3, 0, 0],
            vec![0, 0, 4, 0],
            vec![0, 0, 0, 6],
            vec![0, 0, 4, 0],
        ],
        ..PassSpec::default()
    };
    let silf = silf_table(4, &[one_pass(context)]);
    let pass = first_pass(&silf);
    // "Neither rule is considered to match when the scan position is at the
    // beginning of the input ... Now the scan position is before the A, and
    // both rules match. But the second rule has precedence."
    assert_eq!(
        found_at(&pass, &[1, 2, 3, 4]),
        BTreeMap::from([(1, vec![(1, 4), (0, 2)])])
    );
    held += 1;

    assert_eq!(held, 5, "the rule-finding examples the design counts");
}

/// The compiler documentation's worked classes — linear class 6 of the
/// upper-case vowels and indexed class 10 of the lower-case ones — read
/// through a whole `Silf` at both offset widths: each lower vowel's index in
/// class 10 selects its upper vowel in class 6.
#[test]
fn a_class_map_answers_both_kinds_of_class() {
    let upper: Vec<u16> = vec![0x53, 0x57, 0x5B, 0x61, 0x67];
    let lower: Vec<(u16, u16)> = vec![(0x21, 0), (0x25, 1), (0x29, 2), (0x2F, 3), (0x35, 4)];
    let mut linear: Vec<Vec<u16>> = (0..10).map(|n| vec![200 + n]).collect();
    linear[6] = upper.clone();
    for (major, wide) in [(2u32, false), (3, false), (4, true), (5, true)] {
        let spec = DescriptionSpec {
            class_map: class_map(&linear, &[lower.clone()], wide),
            order: [0, 0, 0, 255],
            ..DescriptionSpec::default()
        };
        let silf = silf_table(major, &[spec]);
        let table = graphite::Silf::parse(&silf).expect("the Silf reads");
        let description = table.description(0).expect("its description");
        let classes = description.classes();
        assert_eq!(classes.len(), 11, "{major}");
        assert_eq!(classes.linear_count(), 10);
        assert_eq!(classes.kind(6), Some(graphite::ClassKind::Linear));
        assert_eq!(classes.kind(10), Some(graphite::ClassKind::Lookup));
        assert_eq!(classes.kind(11), None);
        assert_eq!(classes.class_len(6), Some(5));
        assert_eq!(classes.class_len(10), Some(5));
        // `PutSubs 0 10 6`: "a" is index 0 of class 10, which is "A" in
        // class 6, and so on through the vowels.
        for (n, (glyph, _)) in lower.iter().enumerate() {
            let index = classes.index(10, *glyph).expect("a lower vowel");
            assert_eq!(usize::from(index), n);
            assert_eq!(classes.glyph(6, usize::from(index)), Some(upper[n]));
            assert_eq!(classes.glyph(10, n), Some(*glyph));
        }
        // A linear class gives its glyphs' indices by reading them in order.
        assert_eq!(classes.index(6, 0x5B), Some(2));
        // Past a class, past the map, and a glyph in neither.
        assert_eq!(classes.glyph(6, 5), None);
        assert_eq!(classes.glyph(11, 0), None);
        assert_eq!(classes.index(10, 0x22), None);
        assert_eq!(classes.index(6, 0x22), None);
        assert_eq!(classes.index(12, 0x21), None);
    }
}

/// The ranges, the start states and the rule order as the GTF has them,
/// beyond the GDL's examples: a glyph in no range jams; fewer slots before
/// the position than any rule's pre-context matches nothing; ranges out of
/// order are read in order, the first winning.
#[test]
fn a_machine_reads_its_ranges_and_starts_as_the_gtf_says() {
    // One rule, `A B` (glyphs 1 and 2), with glyphs 1 and 3 in column 0 —
    // its ranges out of order, so read in order — and 2 in column 1.
    let spec = PassSpec {
        ranges: vec![(3, 3, 0), (2, 2, 1), (1, 1, 0)],
        columns: 2,
        rows: 3,
        transitional: 2,
        success: 1,
        rule_lists: vec![vec![0]],
        start_states: vec![0],
        rules: vec![(2, 0, vec![], vec![0x31])],
        transitions: vec![vec![1, 0], vec![0, 2]],
        ..PassSpec::default()
    };
    let silf = silf_table(4, &[one_pass(spec)]);
    let pass = first_pass(&silf);
    assert_eq!(pass.column(1), Some(0));
    assert_eq!(pass.column(3), Some(0));
    assert_eq!(pass.column(2), Some(1));
    assert_eq!(pass.column(4), None);
    assert_eq!(
        found_at(&pass, &[1, 2]),
        BTreeMap::from([(0, vec![(0, 2)])])
    );
    assert_eq!(
        found_at(&pass, &[3, 2]),
        BTreeMap::from([(0, vec![(0, 2)])])
    );
    // A glyph in no range jams the machine.
    assert_eq!(found_at(&pass, &[4, 2]), BTreeMap::new());
    assert_eq!(found_at(&pass, &[1, 4]), BTreeMap::new());

    // A pass of no machine finds nothing anywhere.
    let none = PassSpec {
        flags: 0x05,
        start_states: vec![0],
        ..PassSpec::default()
    };
    let silf = silf_table(5, &[one_pass(none)]);
    let pass = first_pass(&silf);
    assert!(!pass.has_machine());
    assert_eq!(pass.collision_runs(), 5);
    assert_eq!(found_at(&pass, &[1, 2, 3]), BTreeMap::new());

    // Where the shortest pre-context is two, the first two positions match
    // nothing; the start states are the GTF's, indexed by the slots missing.
    let late = PassSpec {
        ranges: vec![(1, 1, 0)],
        columns: 1,
        rows: 4,
        transitional: 3,
        success: 1,
        rule_lists: vec![vec![0]],
        min_pre: 2,
        max_pre: 2,
        start_states: vec![0],
        rules: vec![(3, 2, vec![], vec![0x31])],
        transitions: vec![vec![1], vec![2], vec![3]],
        ..PassSpec::default()
    };
    let silf = silf_table(4, &[one_pass(late)]);
    let pass = first_pass(&silf);
    assert_eq!(pass.start(0), None);
    assert_eq!(pass.start(1), None);
    assert_eq!(pass.start(2), Some((0, 0)));
    assert_eq!(pass.start(5), Some((3, 0)));
    assert_eq!(
        found_at(&pass, &[1, 1, 1, 1]),
        BTreeMap::from([(2, vec![(0, 3)]), (3, vec![(0, 4)])])
    );
}

/// A transition to a state past the table, or from a row the table does not
/// hold, ends the walk and is named, the rules found before it standing; so
/// is a rule past the rule count, a walk past the context ceiling, and a
/// position past the candidate ceiling; and a pass whose counts the GTF's
/// order cannot hold, or past the pass ceiling, does not run.
#[test]
fn a_transition_past_its_table_is_refused_and_named() {
    // `A B C`: state 2 accepts rule 1 and goes on; the transition from 2
    // names state 9, past the table's four.
    let mut spec = PassSpec {
        ranges: vec![(1, 1, 0), (2, 2, 1), (3, 3, 2)],
        columns: 3,
        rows: 4,
        transitional: 3,
        success: 2,
        rule_lists: vec![vec![1], vec![0]],
        start_states: vec![0],
        rules: vec![(3, 0, vec![], vec![0x31]), (2, 0, vec![], vec![0x31])],
        transitions: vec![vec![1, 0, 0], vec![0, 2, 0], vec![0, 0, 9]],
        ..PassSpec::default()
    };
    let silf = silf_table(4, &[one_pass(spec.clone())]);
    let pass = first_pass(&silf);
    let matched = pass.candidates(&[1, 2, 3], 0, &Limits::DEFAULT);
    assert_eq!(
        matched.refusals,
        vec![Refusal::TransitionPastTable { pass: 0, state: 2 }]
    );
    assert_eq!(
        matched
            .candidates
            .iter()
            .map(|c| (c.rule, c.end))
            .collect::<Vec<_>>(),
        vec![(1, 2)],
        "the rule found before the transition stands"
    );

    // A start state past the table.
    let mut start = spec.clone();
    start.transitions[2][2] = 3;
    start.start_states = vec![7];
    let silf = silf_table(4, &[one_pass(start)]);
    let pass = first_pass(&silf);
    let matched = pass.candidates(&[1, 2, 3], 0, &Limits::DEFAULT);
    assert_eq!(
        matched.refusals,
        vec![Refusal::TransitionPastTable { pass: 0, state: 7 }]
    );
    assert!(matched.candidates.is_empty());

    // A rule map naming a rule past the count: named, and the rest stand.
    spec.transitions[2][2] = 3;
    spec.rule_lists = vec![vec![1, 5], vec![0]];
    let silf = silf_table(4, &[one_pass(spec.clone())]);
    let pass = first_pass(&silf);
    let matched = pass.candidates(&[1, 2, 3], 0, &Limits::DEFAULT);
    assert_eq!(
        matched.refusals,
        vec![Refusal::RuleOutOfRange { pass: 0, rule: 5 }]
    );
    assert_eq!(
        matched
            .candidates
            .iter()
            .map(|c| c.rule)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );

    // Past the context ceiling: two slots read, the third refused.
    spec.rule_lists = vec![vec![1], vec![0]];
    let silf = silf_table(4, &[one_pass(spec.clone())]);
    let pass = first_pass(&silf);
    let short = Limits {
        max_context_length: 2,
        ..Limits::DEFAULT
    };
    let matched = pass.candidates(&[1, 2, 3], 0, &short);
    assert_eq!(
        matched.refusals,
        vec![Refusal::RuleContextTooLong { pass: 0 }]
    );
    assert_eq!(
        matched
            .candidates
            .iter()
            .map(|c| c.rule)
            .collect::<Vec<_>>(),
        vec![1]
    );

    // Past the candidate ceiling: the first gathered stands.
    let few = Limits {
        max_rule_candidates: 1,
        ..Limits::DEFAULT
    };
    let matched = pass.candidates(&[1, 2, 3], 0, &few);
    assert_eq!(
        matched.refusals,
        vec![Refusal::TooManyCandidates { pass: 0 }]
    );
    assert_eq!(
        matched
            .candidates
            .iter()
            .map(|c| c.rule)
            .collect::<Vec<_>>(),
        vec![1]
    );

    // A truncated transition table: the row the walk needs is not there.
    let mut silf = silf_table(4, &[one_pass(spec.clone())]);
    let cut = silf.len() - 3;
    silf.truncate(cut);
    let table = graphite::Silf::parse(&silf).expect("the header still reads");
    let description = table.description(0).expect("its description");
    assert_eq!(
        description.pass(0, &Limits::DEFAULT).map(|_| ()),
        Err(Refusal::PassMalformed { pass: 0 })
    );

    // Counts the GTF's order cannot hold: more success states than states,
    // and a state that does not accept with no transitions.
    for (rows, transitional, success) in [(4u16, 3u16, 5u16), (4, 1, 2), (2, 3, 1)] {
        let mut bad = spec.clone();
        bad.rows = rows;
        bad.transitional = transitional;
        bad.success = success;
        bad.transitions.truncate(usize::from(transitional));
        bad.rule_lists.resize(usize::from(success), Vec::new());
        let silf = silf_table(4, &[one_pass(bad)]);
        let table = graphite::Silf::parse(&silf).expect("reads");
        let description = table.description(0).expect("its description");
        assert_eq!(
            description.pass(0, &Limits::DEFAULT).map(|_| ()),
            Err(Refusal::PassMalformed { pass: 0 }),
            "{rows} {transitional} {success}"
        );
    }

    // A range naming a column the transitions do not have.
    let mut wide_range = spec.clone();
    wide_range.ranges.push((4, 4, 3));
    let silf = silf_table(4, &[one_pass(wide_range)]);
    let table = graphite::Silf::parse(&silf).expect("reads");
    let description = table.description(0).expect("its description");
    assert_eq!(
        description.pass(0, &Limits::DEFAULT).map(|_| ()),
        Err(Refusal::PassMalformed { pass: 0 })
    );

    // Passes past the ceiling, and past the count.
    let two = DescriptionSpec {
        class_map: class_map(&[], &[], true),
        passes: vec![spec.clone(), spec],
        order: [0, 2, 2, 255],
        ..DescriptionSpec::default()
    };
    let silf = silf_table(4, &[two]);
    let table = graphite::Silf::parse(&silf).expect("reads");
    let description = table.description(0).expect("its description");
    let one = Limits {
        max_passes: 1,
        ..Limits::DEFAULT
    };
    assert!(description.pass(0, &one).is_ok());
    assert_eq!(
        description.pass(1, &one).map(|_| ()),
        Err(Refusal::TooManyPasses { passes: 2 })
    );
    assert!(description.pass(1, &Limits::DEFAULT).is_ok());
    assert_eq!(
        description.pass(2, &Limits::DEFAULT).map(|_| ()),
        Err(Refusal::PassMalformed { pass: 2 })
    );
}

/// A `Silf` reads at every version the design covers — each version's
/// fields where the notes put them, a version-5 table plain and compressed
/// — and is refused at one it does not.
#[test]
fn a_silf_reads_at_every_version_it_covers() {
    let pass = PassSpec {
        flags: 0x20,
        max_rule_loop: 5,
        max_rule_context: 2,
        max_backup: 1,
        ranges: vec![(1, 1, 0)],
        columns: 1,
        rows: 2,
        transitional: 1,
        success: 1,
        rule_lists: vec![vec![0]],
        start_states: vec![0],
        rules: vec![(1, 0, vec![0x32], vec![0x1B, 0x31])],
        transitions: vec![vec![1]],
        pass_constraint: vec![0x01, 0x01, 0x30],
        collision_threshold: 7,
        ..PassSpec::default()
    };
    let spec = DescriptionSpec {
        script_tags: vec![*b"latn", *b"grek"],
        pseudos: vec![(0x2E, 300), (0x301, 301), (0x1F600, 302)],
        class_map: class_map(&[vec![9, 8]], &[], false),
        passes: vec![pass],
        order: [0, 1, 1, 255],
    };
    for major in 1..=5u32 {
        let mut spec = spec.clone();
        spec.class_map = class_map(&[vec![9, 8]], &[], major >= 4);
        let silf = silf_table(major, &[spec]);
        let table = graphite::Silf::parse(&silf).expect("the Silf reads");
        assert_eq!(table.version(), major << 16);
        assert_eq!(table.compiler_version().is_some(), major >= 3);
        assert_eq!(table.len(), 1);
        let description = table.description(0).expect("its description");
        assert_eq!(description.rule_version.is_some(), major >= 3, "{major}");
        assert_eq!(description.max_glyph, 500);
        assert_eq!(description.passes, 1);
        assert_eq!(
            (
                description.substitution,
                description.positioning,
                description.justification,
                description.bidi
            ),
            (0, 1, 1, 255)
        );
        assert_eq!(
            (
                description.attr_pseudo,
                description.attr_break_weight,
                description.attr_directionality
            ),
            (0, 1, 2)
        );
        if major >= 2 {
            assert_eq!(
                (description.attr_mirroring, description.attr_skip_passes),
                (3, 4)
            );
        } else {
            assert_eq!(
                (description.attr_mirroring, description.attr_skip_passes),
                (0, 0)
            );
        }
        assert_eq!(description.direction, 1);
        assert_eq!(
            description.script_tags,
            vec![Tag::new(b"latn"), Tag::new(b"grek")]
        );
        assert_eq!(description.line_break_glyph, 499);
        assert_eq!(description.pseudo_count(), 3);
        assert_eq!(description.pseudo(0x301), Some(301));
        assert_eq!(description.pseudo(0x1F600), Some(302));
        assert_eq!(description.pseudo(0x41), None);
        assert_eq!(description.classes().glyph(0, 1), Some(8));
        let first = description.pass(0, &Limits::DEFAULT).expect("its pass");
        assert!(first.is_reversed());
        assert_eq!(
            (
                first.max_rule_loop,
                first.max_rule_context,
                first.max_backup
            ),
            (5, 2, 1)
        );
        assert_eq!(first.constraint(0), Some(&[0x32][..]));
        assert_eq!(first.action(0), Some(&[0x1B, 0x31][..]));
        assert_eq!(first.constraint(1), None);
        if major >= 2 {
            assert_eq!(first.pass_constraint(), Some(&[0x01, 0x01, 0x30][..]));
            assert_eq!(first.collision_threshold, 7);
        } else {
            assert_eq!(first.pass_constraint(), None);
            assert_eq!(first.collision_threshold, 0);
        }
        let matched = first.candidates(&[1], 0, &Limits::DEFAULT);
        assert_eq!(matched.candidates.len(), 1, "{major}");

        if major == 5 {
            // Compressed, decoded, and read the same.
            let stored = lz4_literals(&silf);
            let decoded = graphite::decompress(graphite::SILF, &stored, &Limits::DEFAULT)
                .expect("it decodes");
            assert_eq!(&*decoded, &silf[..]);
            // Still compressed when it reaches the reader: refused.
            assert_eq!(
                graphite::Silf::parse(&stored).map(|_| ()),
                Err(Refusal::UnknownCompression {
                    table: graphite::SILF,
                    scheme: 1
                })
            );
        }
    }
    for version in [0u32, 6, 0x0100] {
        let silf = silf_table(version, &[]);
        assert_eq!(
            graphite::Silf::parse(&silf).map(|_| ()),
            Err(Refusal::UnknownVersion {
                table: graphite::SILF,
                version: version << 16
            })
        );
    }
    // A run takes the first description whose script tags hold its script,
    // and the first where none does.
    let mut arabic = spec.clone();
    arabic.script_tags = vec![*b"arab"];
    arabic.class_map = class_map(&[vec![1]], &[], true);
    let mut latin = spec;
    latin.class_map = class_map(&[vec![2]], &[], true);
    let silf = silf_table(4, &[arabic, latin]);
    let table = graphite::Silf::parse(&silf).expect("reads");
    let glyph_of = |script: &[u8; 4]| {
        table
            .description_for(Tag::new(script))
            .expect("a description")
            .expect("readable")
            .classes()
            .glyph(0, 0)
    };
    assert_eq!(glyph_of(b"latn"), Some(2));
    assert_eq!(glyph_of(b"arab"), Some(1));
    assert_eq!(glyph_of(b"cyrl"), Some(1));
}

/// The vendored faces' `Silf`s read as their authors compiled them, and the
/// machine finds in them the rules their text calls for: Dai Banna SIL's
/// composition of `e` and U+0301, through its pseudo-glyph map, to the rule
/// whose action puts the glyph its `cmap` gives `é`.
#[test]
fn the_vendored_faces_silf_reads() {
    let limits = Limits::DEFAULT;

    let face = Sfnt::parse(DAI_BANNA).expect("Dai Banna SIL parses");
    let tables = Tables::parse(&face, &limits).expect("a description");
    let silf = graphite::Silf::parse(tables.silf()).expect("its Silf reads");
    assert_eq!(silf.version(), 0x0004_0000);
    assert_eq!(silf.compiler_version(), Some(0x0004_0002));
    assert_eq!(silf.len(), 1);
    let description = silf.description(0).expect("its description");
    assert_eq!(description.rule_version, Some(0x0004_0000));
    assert_eq!(
        (
            description.passes,
            description.substitution,
            description.positioning,
            description.justification,
            description.bidi
        ),
        (2, 1, 2, 2, 255)
    );
    assert_eq!(
        (description.attr_break_weight, description.line_break_glyph),
        (2, 411)
    );
    assert_eq!(description.pseudo_count(), 8);
    let acute = description.pseudo(0x301).expect("U+0301 is a pseudo-glyph");
    assert_eq!(acute, 413);
    assert_eq!(face.glyph_for_char('\u{301}'), None);
    let classes = description.classes();
    assert_eq!((classes.len(), classes.linear_count()), (116, 116));

    // The line-break pass: a consonant after a prevowel is given a break
    // weight, slot attribute 14, by `PutCopy 0; PushByte -30; AttrSet 14;
    // Next; RetZero`.
    let breaks = description.pass(0, &limits).expect("pass 0");
    let prevowel = face.glyph_for_char('\u{19B6}').expect("U+19B6");
    let consonant = face.glyph_for_char('\u{1982}').expect("U+1982");
    let glyphs = [prevowel, consonant];
    assert!(breaks.candidates(&glyphs, 0, &limits).candidates.is_empty());
    let found = breaks.candidates(&glyphs, 1, &limits);
    assert_eq!(
        found
            .candidates
            .iter()
            .map(|c| (c.rule, c.end))
            .collect::<Vec<_>>(),
        vec![(0, 2)]
    );
    assert_eq!(
        breaks.action(0),
        Some(&[0x1E, 0x00, 0x01, 0xE2, 0x23, 0x0E, 0x19, 0x31][..])
    );

    // The substitution pass: `e` and the acute's pseudo-glyph are a rule
    // whose action opens with `PutGlyph` of a class holding `é`.
    let compose = description.pass(1, &limits).expect("pass 1");
    let e = face.glyph_for_char('e').expect("e");
    let found = compose.candidates(&[e, acute], 0, &limits);
    assert!(found.refusals.is_empty());
    assert_eq!(found.candidates.len(), 1);
    let rule = found.candidates[0].rule;
    assert_eq!(found.candidates[0].end, 2);
    let action = compose.action(rule).expect("its action");
    assert_eq!(action[0], 0x3B, "PutGlyph with a 16-bit class");
    let class = usize::from(u16::from_be_bytes([action[1], action[2]]));
    assert_eq!(classes.glyph(class, 0), face.glyph_for_char('\u{E9}'));

    // Tagmukay's `Silf` 2: no compiler version, `USHORT` class offsets, and
    // the two rules of its second pass that test its first two features —
    // `PushFeat` of features 0 and 1, its `ss01` and `ss02`.
    let face = Sfnt::parse(TAGMUKAY).expect("Tagmukay parses");
    let tables = Tables::parse(&face, &limits).expect("a description");
    let silf = graphite::Silf::parse(tables.silf()).expect("its Silf reads");
    assert_eq!(
        (silf.version(), silf.compiler_version()),
        (0x0002_0000, None)
    );
    let description = silf.description(0).expect("its description");
    assert_eq!(description.rule_version, None);
    assert_eq!(
        (
            description.passes,
            description.substitution,
            description.positioning
        ),
        (2, 0, 2)
    );
    assert_eq!(
        (
            description.attr_mirroring,
            description.ligature_components,
            description.max_components
        ),
        (4, 6, 5)
    );
    let classes = description.classes();
    assert_eq!((classes.len(), classes.linear_count()), (22, 17));
    let second = description.pass(1, &limits).expect("pass 1");
    assert_eq!(second.constraint(2), Some(&[0x2B, 0x00, 0x00, 0x30][..]));
    assert_eq!(second.constraint(3), Some(&[0x2B, 0x01, 0x00, 0x30][..]));

    // Awami Nastaliq's compressed `Silf` 5.1, decoded: twenty-one passes,
    // each readable, a reversed one, collision passes with and without a
    // machine, pass constraints; and a `maxGlyphID` below both its own
    // pseudo-glyph and the glyphs its `Gloc` indexes.
    let face = Sfnt::parse(AWAMI).expect("Awami Nastaliq parses");
    let tables = Tables::parse(&face, &limits).expect("a description");
    assert!(tables.refusals().is_empty());
    let silf = graphite::Silf::parse(tables.silf()).expect("its Silf reads");
    assert_eq!(silf.version(), 0x0005_0001);
    assert_eq!(silf.compiler_version(), Some(0x0005_0000));
    let description = silf.description(0).expect("its description");
    assert_eq!(
        (
            description.passes,
            description.substitution,
            description.positioning,
            description.bidi
        ),
        (21, 0, 7, 255)
    );
    assert_eq!(
        (
            description.direction,
            description.attr_collisions,
            description.user_attributes
        ),
        (2, 4, 6)
    );
    let lrm = description
        .pseudo(0x200E)
        .expect("U+200E is a pseudo-glyph");
    assert_eq!(lrm, 1_673);
    assert_eq!(description.max_glyph, 1_671);
    let attributes = tables.attributes(&limits).expect("both").expect("readable");
    assert!(usize::from(description.max_glyph) < attributes.glyph_count());
    assert!(lrm > description.max_glyph);
    for number in 0..usize::from(description.passes) {
        description
            .pass(number, &limits)
            .unwrap_or_else(|refusal| panic!("pass {number}: {refusal:?}"));
    }
    assert!(description.pass(6, &limits).expect("pass 6").is_reversed());
    let fix = description.pass(10, &limits).expect("pass 10");
    assert!(!fix.has_machine());
    assert_eq!(fix.collision_runs(), 5);
    let kern = description.pass(18, &limits).expect("pass 18");
    assert_eq!((kern.collision_runs(), kern.kerning()), (4, 1));
    assert_eq!(
        description
            .pass(9, &limits)
            .expect("pass 9")
            .pass_constraint(),
        Some(&[0x2B, 0x0A, 0x00, 0x01, 0x00, 0x16, 0x30][..])
    );
    assert_eq!(description.pass(3, &limits).expect("pass 3").max_backup, 50);
}
