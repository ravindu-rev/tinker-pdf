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
