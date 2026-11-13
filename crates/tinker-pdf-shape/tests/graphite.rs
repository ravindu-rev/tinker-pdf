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
    /// The description's flags; bit 5 a collision pass.
    flags: u8,
    /// How many user attributes it counts.
    user_attributes: u8,
    /// The first glyph attribute of its collision settings.
    attr_collisions: u8,
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
    head.extend([spec.flags, 0, 0, 0, 1, 2]);
    if major >= 2 {
        // Mirroring and skip-pass attributes, and no justification levels.
        head.extend([3, 4, 0]);
    }
    head.extend(u16s(&[0]));
    // User attributes, components a ligature, direction, collisions, and
    // two reserved bytes.
    head.extend([spec.user_attributes, 0, 1, spec.attr_collisions, 0, 0]);
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
        ..DescriptionSpec::default()
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

// --- the stack machine (FT-07d.7) ----------------------------------------
//
// `Stream` is a slot stream as a test states it — each slot's glyph and
// attributes, the features, what is attached to what, and each glyph's
// attributes and box top — so that a program's every read is answered from
// what the test wrote. Each program is the bytes the GTF appendix's table
// lays an instruction out as, and its answer is the one the table's text
// gives.

use std::cell::Cell;

use tinker_pdf_shape::graphite::machine::{self, Block, Bounds, Budget, Fault, Frame, Metric, Op};
use tinker_pdf_shape::graphite::{Reads, Site, SlotAttribute};

/// A slot stream as a test states it.
#[derive(Debug, Default)]
struct Stream {
    /// Each slot's glyph.
    glyphs: Vec<u16>,
    /// `(slot, attribute, index)` to value; anything absent is 0.
    slot_attributes: BTreeMap<(usize, SlotAttribute, u8), i32>,
    /// `(glyph, attribute)` to value; anything absent is 0.
    glyph_attributes: BTreeMap<(u16, u16), i32>,
    /// Each glyph's box top; a level past 0 adds a thousand a level, so a
    /// test sees the level arrive.
    tops: BTreeMap<u16, i32>,
    /// Each feature's value, every slot alike.
    features: Vec<i32>,
    /// Which slot each attached slot is attached to.
    attached: BTreeMap<usize, usize>,
    /// Glyphs whose attributes the face cannot read.
    unreadable: Vec<u16>,
    /// How many reads the code made.
    reads: Cell<usize>,
}

impl Stream {
    fn read(&self) {
        self.reads.set(self.reads.get() + 1);
    }
}

impl Reads for Stream {
    fn slot_count(&self) -> usize {
        self.glyphs.len()
    }

    fn slot_attribute(
        &self,
        slot: usize,
        attribute: SlotAttribute,
        index: u8,
    ) -> Result<i32, Refusal> {
        self.read();
        Ok(self
            .slot_attributes
            .get(&(slot, attribute, index))
            .copied()
            .unwrap_or(0))
    }

    fn glyph_attribute(&self, slot: usize, attribute: u16) -> Result<i32, Refusal> {
        self.read();
        let glyph = self.glyphs[slot];
        if self.unreadable.contains(&glyph) {
            return Err(Refusal::GlyphUnreadable { glyph });
        }
        Ok(self
            .glyph_attributes
            .get(&(glyph, attribute))
            .copied()
            .unwrap_or(0))
    }

    fn metric(&self, slot: usize, metric: Metric, level: u8) -> Result<i32, Refusal> {
        self.read();
        let Metric::BoundingBoxTop = metric;
        let top = self.tops.get(&self.glyphs[slot]).copied().unwrap_or(0);
        Ok(top + i32::from(level) * 1000)
    }

    fn feature(&self, _slot: usize, feature: u8) -> i32 {
        self.read();
        self.features
            .get(usize::from(feature))
            .copied()
            .unwrap_or(0)
    }

    fn attached_to(&self, slot: usize) -> Option<usize> {
        self.attached.get(&slot).copied()
    }
}

/// The bounds the machine's tests check against: four classes, two
/// features and three user attributes.
const BOUNDS: Bounds = Bounds {
    classes: 4,
    features: 2,
    user_attributes: 3,
    positioning: false,
};

/// A rule's constraint, in pass 3.
const CONSTRAINT: Site = Site {
    pass: 3,
    block: Block::Constraint(7),
};

/// A rule's action, in pass 3.
const ACTION: Site = Site {
    pass: 3,
    block: Block::Action(7),
};

/// `code` checked as the block at `site`, under the default ceilings.
fn checked(code: &[u8], site: Site) -> Result<machine::Program, Refusal> {
    machine::check(code, site, &BOUNDS, &Limits::DEFAULT)
}

/// The stream the machine's tests read: glyphs 10, 11 and 12; slot 1, the
/// one under test, attached to slot 0; and the attributes, tops and
/// features the cases read.
fn reading_stream() -> Stream {
    Stream {
        glyphs: vec![10, 11, 12],
        slot_attributes: BTreeMap::from([
            ((0, SlotAttribute::ShiftX, 0), -40),
            ((2, SlotAttribute::ShiftX, 0), 250),
            ((1, SlotAttribute::User, 2), 6),
        ]),
        glyph_attributes: BTreeMap::from([((10, 5), 33), ((11, 5), 77), ((12, 300), -9)]),
        tops: BTreeMap::from([(10, 600), (11, 700)]),
        features: vec![0, 2],
        attached: BTreeMap::from([(1, 0)]),
        ..Stream::default()
    }
}

/// The answer `code`, checked as a constraint, gives with slot 1 under
/// test at scan position 0 of [`reading_stream`].
fn answered(code: &[u8]) -> Result<i32, Refusal> {
    let program = checked(code, CONSTRAINT)?;
    let mut budget = Budget::new(&Limits::DEFAULT);
    machine::answer(
        &program,
        &reading_stream(),
        Frame {
            position: 0,
            under_test: Some(1),
        },
        &mut budget,
    )
}

/// The refusal `code` meets at `site`, as `(fault, at)`.
fn refused(code: &[u8], site: Site, limits: &Limits) -> (Fault, u16) {
    match machine::check(code, site, &BOUNDS, limits) {
        Err(Refusal::CodeRefused {
            pass,
            block,
            at,
            fault,
        }) => {
            assert_eq!((pass, block), (site.pass, site.block), "{code:02X?}");
            (fault, at)
        }
        other => panic!("{code:02X?} was not refused: {other:?}"),
    }
}

/// A program per computing instruction, its answer the one the GTF
/// appendix's table states; then the constraint examples the design lists,
/// transcribed and counted.
#[test]
fn each_computing_instruction_does_what_the_specification_states() {
    let cases: &[(&str, &[u8], i32)] = &[
        ("Nop", &[0x00, 0x01, 5, 0x30], 5),
        ("PushByte, signed", &[0x01, 0xFD, 0x30], -3),
        ("PushByteU, unsigned", &[0x02, 0xFD, 0x30], 253),
        ("PushShort, signed", &[0x03, 0xFF, 0xFE, 0x30], -2),
        ("PushShortU, unsigned", &[0x04, 0xFF, 0xFE, 0x30], 65_534),
        (
            "PushLong",
            &[0x05, 0x01, 0x02, 0x03, 0x04, 0x30],
            0x0102_0304,
        ),
        ("PushLong, signed", &[0x05, 0x80, 0, 0, 0, 0x30], i32::MIN),
        ("Add", &[0x01, 7, 0x01, 5, 0x06, 0x30], 12),
        (
            "Add wraps",
            &[0x05, 0x7F, 0xFF, 0xFF, 0xFF, 0x01, 1, 0x06, 0x30],
            i32::MIN,
        ),
        (
            "Sub: the top from the next",
            &[0x01, 7, 0x01, 5, 0x07, 0x30],
            2,
        ),
        ("Mul", &[0x01, 7, 0x01, 0xFB, 0x08, 0x30], -35),
        (
            "Div: the next by the top",
            &[0x01, 7, 0x01, 2, 0x09, 0x30],
            3,
        ),
        (
            "Div truncates toward zero",
            &[0x01, 0xF9, 0x01, 2, 0x09, 0x30],
            -3,
        ),
        ("Min", &[0x01, 7, 0x01, 0xFB, 0x0A, 0x30], -5),
        ("Max", &[0x01, 7, 0x01, 0xFB, 0x0B, 0x30], 7),
        ("Neg", &[0x01, 7, 0x0C, 0x30], -7),
        ("Neg wraps", &[0x05, 0x80, 0, 0, 0, 0x0C, 0x30], i32::MIN),
        ("Trunc8", &[0x04, 0x12, 0x34, 0x0D, 0x30], 0x34),
        ("Trunc8 of −1", &[0x01, 0xFF, 0x0D, 0x30], 0xFF),
        ("Trunc16", &[0x05, 0, 1, 0x23, 0x45, 0x0E, 0x30], 0x2345),
        ("Trunc16 of −1", &[0x01, 0xFF, 0x0E, 0x30], 0xFFFF),
        ("Cond, true", &[0x01, 10, 0x01, 20, 0x01, 1, 0x0F, 0x30], 20),
        (
            "Cond, false",
            &[0x01, 10, 0x01, 20, 0x01, 0, 0x0F, 0x30],
            10,
        ),
        ("And", &[0x01, 2, 0x01, 3, 0x10, 0x30], 1),
        ("And, one false", &[0x01, 2, 0x01, 0, 0x10, 0x30], 0),
        ("Or", &[0x01, 0, 0x01, 5, 0x11, 0x30], 1),
        ("Or, both false", &[0x01, 0, 0x01, 0, 0x11, 0x30], 0),
        ("Not", &[0x01, 0, 0x12, 0x30], 1),
        ("Not of a true", &[0x01, 7, 0x12, 0x30], 0),
        ("Equal", &[0x01, 4, 0x01, 4, 0x13, 0x30], 1),
        ("NotEqu", &[0x01, 4, 0x01, 4, 0x14, 0x30], 0),
        (
            "Less: the next than the top",
            &[0x01, 3, 0x01, 4, 0x15, 0x30],
            1,
        ),
        ("Less, the other way", &[0x01, 4, 0x01, 3, 0x15, 0x30], 0),
        ("Gtr", &[0x01, 4, 0x01, 3, 0x16, 0x30], 1),
        ("LessEq", &[0x01, 4, 0x01, 4, 0x17, 0x30], 1),
        ("GtrEq", &[0x01, 3, 0x01, 4, 0x18, 0x30], 0),
        ("BitAnd", &[0x01, 0b1100, 0x01, 0b1010, 0x3E, 0x30], 0b1000),
        ("BitOr", &[0x01, 0b1100, 0x01, 0b1010, 0x3F, 0x30], 0b1110),
        ("BitNot", &[0x01, 0, 0x40, 0x30], -1),
        (
            "SetBits: mask cleared, value set",
            &[0x01, 0b0110, 0x41, 0, 0b0010, 0, 0b1001, 0x30],
            0b1101,
        ),
        (
            "SetBits' operands are signed",
            &[0x01, 0, 0x41, 0, 0, 0x80, 0, 0x30],
            -32_768,
        ),
        ("PushVersion", &[0x37, 0x30], 0x0005_0000),
        ("RetZero", &[0x01, 5, 0x31], 0),
        ("RetTrue", &[0x01, 0, 0x32], 1),
        ("PopRet ends the code", &[0x01, 4, 0x30, 0x01, 8], 4),
        ("the code's end answers the top", &[0x01, 9], 9),
        ("an empty constraint answers its true", &[], 1),
        ("PushSlotAttr, a slot after", &[0x28, 20, 1, 0x30], 250),
        ("PushSlotAttr, a slot before", &[0x28, 20, 0xFF, 0x30], -40),
        ("PushISlotAttr", &[0x2E, 55, 0, 2, 0x30], 6),
        ("PushGlyphAttr", &[0x29, 5, 0, 0x30], 77),
        ("PushGlyphAttr, 16-bit", &[0x3C, 0x01, 0x2C, 1, 0x30], -9),
        ("PushGlyphAttr, none stored", &[0x29, 6, 0, 0x30], 0),
        ("PushGlyphMetric", &[0x2A, 2, 0, 0, 0x30], 700),
        ("PushGlyphMetric, a level", &[0x2A, 2, 0, 2, 0x30], 2_700),
        ("PushFeat", &[0x2B, 1, 0, 0x30], 2),
        ("PushAttToGlyphAttr", &[0x2C, 5, 0, 0x30], 33),
        ("PushAttToGlyphAttr, 16-bit", &[0x3D, 0, 5, 0, 0x30], 33),
        ("PushAttToGlyphAttr, unattached", &[0x2C, 5, 1, 0x30], 0),
        ("PushAttToGlyphMetric", &[0x2D, 2, 0, 0, 0x30], 600),
        (
            "ContextItem at the slot under test runs",
            &[0x22, 1, 2, 0x01, 0, 0x30],
            0,
        ),
        (
            "ContextItem at another is skipped, a true in its place",
            &[0x22, 0, 2, 0x01, 0, 0x30],
            1,
        ),
    ];
    for (name, code, expected) in cases {
        assert_eq!(answered(code), Ok(*expected), "{name}: {code:02X?}");
    }
    assert_eq!(cases.len(), 62, "a program per case");

    let mut held = 0usize;
    let run = |program: &machine::Program, stream: &Stream, under_test: usize| {
        let mut budget = Budget::new(&Limits::DEFAULT);
        let frame = Frame {
            position: 0,
            under_test: Some(under_test),
        };
        machine::answer(program, stream, frame, &mut budget)
    };

    // The compiler documentation: "the number 11509 is written as '44 –11'
    // (since 11509 = x2CF5, x2C = 44 and xF5 = –11)" — `PushShort`'s two
    // bytes, most significant first.
    assert_eq!(answered(&[0x03, 44, 0xF5, 0x30]), Ok(11_509));
    held += 1;

    // GDL 4.1.2: `gA > gB / _ {ligatures == all}`, `ligatures` feature 0
    // and `all` its value 3 — a slot test, so a `ContextItem` for the rule's
    // one slot.
    let ligatures = checked(&[0x22, 0, 6, 0x2B, 0, 0, 0x01, 3, 0x13, 0x30], CONSTRAINT)
        .expect("the rule checks");
    let mut stream = Stream {
        glyphs: vec![1],
        features: vec![3],
        ..Stream::default()
    };
    assert_eq!(run(&ligatures, &stream, 0), Ok(1), "ligatures == all");
    stream.features = vec![2];
    assert_eq!(run(&ligatures, &stream, 0), Ok(0), "ligatures == std");
    held += 1;

    // GDL 4.1.3: `gA > gB / _ {bb.height > 1000m}`. No vendored face's
    // code numbers `bb.height`, so the rule naming it is refused by name,
    // whatever number it is given; the comparison runs through `bb.top`,
    // the one metric the faces reveal.
    for number in (0..=255u8).filter(|n| *n != 2) {
        let code = [0x22, 0, 8, 0x2A, number, 0, 0, 0x03, 0x03, 0xE8, 0x16, 0x30];
        assert_eq!(
            refused(&code, CONSTRAINT, &Limits::DEFAULT),
            (Fault::UnknownMetric(number), 3)
        );
    }
    let tall = checked(
        &[0x22, 0, 8, 0x2A, 2, 0, 0, 0x03, 0x03, 0xE8, 0x16, 0x30],
        CONSTRAINT,
    )
    .expect("bb.top checks");
    let mut stream = Stream {
        glyphs: vec![4],
        tops: BTreeMap::from([(4, 1_200)]),
        ..Stream::default()
    };
    assert_eq!(run(&tall, &stream, 0), Ok(1));
    stream.tops.insert(4, 800);
    assert_eq!(run(&tall, &stream, 0), Ok(0));
    held += 1;

    // GDL 6.9.2.1: a feature's default is tested on its public and its
    // hidden id together, `eng == descender && eng__Engs == descender`:
    // `cv43` feature 0, `Engs` feature 1, `descender` 0.
    let default = checked(
        &[
            0x2B, 0, 0, 0x01, 0, 0x13, 0x2B, 1, 0, 0x01, 0, 0x13, 0x10, 0x30,
        ],
        CONSTRAINT,
    )
    .expect("the test checks");
    let mut stream = Stream {
        glyphs: vec![1],
        features: vec![0, 0],
        ..Stream::default()
    };
    assert_eq!(run(&default, &stream, 0), Ok(1), "both at the default");
    stream.features = vec![0, 2];
    assert_eq!(run(&default, &stream, 0), Ok(0), "the hidden id set");
    stream.features = vec![1, 0];
    assert_eq!(run(&default, &stream, 0), Ok(0), "the public id set");
    held += 1;

    assert_eq!(held, 4, "the computing examples the design counts");
}

/// Every fault the check names, each at the instruction it names, so that
/// the block never runs; and every byte sequence checked and, where it
/// passes, run without a panic.
#[test]
fn code_that_fails_its_check_never_runs() {
    let limits = Limits::DEFAULT;
    let cases: &[(&[u8], Site, Fault, u16)] = &[
        (&[0x43], CONSTRAINT, Fault::IllegalOpcode(0x43), 0),
        (&[0x01, 1, 0xFF], CONSTRAINT, Fault::IllegalOpcode(0xFF), 2),
        (&[0x1A, 1], ACTION, Fault::Unimplemented(0x1A), 0),
        (
            &[0x2F, 0, 0, 0, 0x30],
            CONSTRAINT,
            Fault::Unimplemented(0x2F),
            0,
        ),
        (&[0x36, 0, 0x30], CONSTRAINT, Fault::Unimplemented(0x36), 0),
        (&[0x39], ACTION, Fault::Unimplemented(0x39), 0),
        (&[0x3A], ACTION, Fault::Unimplemented(0x3A), 0),
        (&[0x05, 1, 2], CONSTRAINT, Fault::Truncated, 0),
        (&[0x01, 1, 0x03, 1], CONSTRAINT, Fault::Truncated, 2),
        (&[0x21, 3, 0, 0], ACTION, Fault::Truncated, 0),
        (&[0x01, 1, 0x41, 0, 1, 0], CONSTRAINT, Fault::Truncated, 2),
        (
            &[0x22, 0, 1, 0x03, 0, 1],
            CONSTRAINT,
            Fault::SkipOffBoundary,
            0,
        ),
        (
            &[0x22, 0, 9, 0x01, 1],
            CONSTRAINT,
            Fault::SkipOffBoundary,
            0,
        ),
        (
            &[0x22, 0, 5, 0x22, 1, 5, 0x01, 1, 0x01, 2, 0x10],
            CONSTRAINT,
            Fault::SkipOffBoundary,
            3,
        ),
        (
            &[0x22, 0, 4, 0x01, 1, 0x01, 2, 0x10],
            CONSTRAINT,
            Fault::NotOneTest,
            7,
        ),
        (
            &[0x22, 0, 0, 0x01, 1, 0x10],
            CONSTRAINT,
            Fault::NotOneTest,
            3,
        ),
        (
            &[0x22, 0, 2, 0x01, 1, 0x30],
            ACTION,
            Fault::ContextItemInAction,
            0,
        ),
        (&[0x19], CONSTRAINT, Fault::ActionInConstraint(0x19), 0),
        (
            &[0x01, 1, 0x23, 20],
            CONSTRAINT,
            Fault::ActionInConstraint(0x23),
            2,
        ),
        (&[0x06, 0x30], CONSTRAINT, Fault::StackUnderflow, 0),
        (&[0x30], ACTION, Fault::StackUnderflow, 0),
        (&[0x23, 20], ACTION, Fault::StackUnderflow, 0),
        (
            &[0x28, 1, 0, 0x30],
            CONSTRAINT,
            Fault::UnknownSlotAttribute(1),
            0,
        ),
        (
            &[0x28, 18, 0, 0x30],
            CONSTRAINT,
            Fault::UnknownSlotAttribute(18),
            0,
        ),
        (
            &[0x01, 1, 0x23, 255],
            ACTION,
            Fault::UnknownSlotAttribute(255),
            2,
        ),
        (
            &[0x2A, 0, 0, 0, 0x30],
            CONSTRAINT,
            Fault::UnknownMetric(0),
            0,
        ),
        (
            &[0x2D, 3, 0, 0, 0x30],
            CONSTRAINT,
            Fault::UnknownMetric(3),
            0,
        ),
        (
            &[0x2E, 20, 0, 0, 0x30],
            CONSTRAINT,
            Fault::AttributeMisused(20),
            0,
        ),
        (
            &[0x01, 1, 0x27, 55, 0],
            ACTION,
            Fault::AttributeMisused(55),
            2,
        ),
        (
            &[0x01, 1, 0x33, 15, 0],
            ACTION,
            Fault::AttributeMisused(15),
            2,
        ),
        (&[0x01, 1, 0x24, 62], ACTION, Fault::AttributeMisused(62), 2),
        (
            &[0x2A, 2, 0, 0xFF, 0x30],
            CONSTRAINT,
            Fault::OperandOutOfRange,
            0,
        ),
        (
            &[0x2E, 55, 0, 0xFF, 0x30],
            CONSTRAINT,
            Fault::OperandOutOfRange,
            0,
        ),
        (&[0x1C, 4], ACTION, Fault::ClassOutOfRange(4), 0),
        (&[0x3B, 1, 0], ACTION, Fault::ClassOutOfRange(256), 0),
        (&[0x1D, 0, 1, 9], ACTION, Fault::ClassOutOfRange(9), 0),
        (&[0x38, 0, 0, 1, 0, 9], ACTION, Fault::ClassOutOfRange(9), 0),
        (
            &[0x2B, 2, 0, 0x30],
            CONSTRAINT,
            Fault::FeatureOutOfRange(2),
            0,
        ),
        (
            &[0x01, 1, 0x42, 7, 0],
            ACTION,
            Fault::FeatureOutOfRange(7),
            2,
        ),
        (
            &[0x2E, 55, 0, 3, 0x30],
            CONSTRAINT,
            Fault::UserAttributeOutOfRange(3),
            0,
        ),
        (
            &[0x01, 1, 0x33, 55, 3],
            ACTION,
            Fault::UserAttributeOutOfRange(3),
            2,
        ),
    ];
    for (code, site, fault, at) in cases {
        assert_eq!(refused(code, *site, &limits), (*fault, *at), "{code:02X?}");
    }
    // A block whose stack would rise past the ceiling: three pushes over a
    // constraint's true, under a ceiling of three; and a ceiling of none,
    // which a constraint's true alone passes.
    let shallow = Limits {
        max_stack_depth: 3,
        ..Limits::DEFAULT
    };
    assert_eq!(
        refused(&[0x01, 1, 0x01, 2, 0x01, 3], CONSTRAINT, &shallow),
        (Fault::StackOverflow, 4)
    );
    let none = Limits {
        max_stack_depth: 0,
        ..Limits::DEFAULT
    };
    assert_eq!(refused(&[], CONSTRAINT, &none), (Fault::StackOverflow, 0));
    assert!(machine::check(&[0x01, 1, 0x30], ACTION, &BOUNDS, &shallow).is_ok());

    // A block whose first instruction reads and whose second is refused is
    // never run: there is no program to run, and the stream was never
    // read.
    let stream = reading_stream();
    let code = [0x28, 20, 0, 0x2A, 9, 0, 0, 0x30];
    assert!(checked(&code, CONSTRAINT).is_err());
    assert_eq!(stream.reads.get(), 0);

    // Every byte sequence of one and two bytes, and a fixed sweep of longer
    // ones, checked as a constraint and as an action: none panics, and each
    // that passes runs to an answer or a refusal.
    let mut sequences: Vec<Vec<u8>> = (0..=255u8).map(|b| vec![b]).collect();
    for first in 0..=255u8 {
        for second in 0..=255u8 {
            sequences.push(vec![first, second]);
        }
    }
    let mut seed = 0x2545_F491u32;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    for _ in 0..20_000 {
        let length = 3 + (next() % 22) as usize;
        let code: Vec<u8> = (0..length).map(|_| (next() % 0x44) as u8).collect();
        sequences.push(code);
    }
    let mut passed = 0usize;
    for code in &sequences {
        for site in [CONSTRAINT, ACTION] {
            if let Ok(program) = machine::check(code, site, &BOUNDS, &limits) {
                passed += 1;
                if !site.block.is_action() {
                    let mut budget = Budget::new(&limits);
                    let frame = Frame {
                        position: 0,
                        under_test: Some(1),
                    };
                    let _ = machine::answer(&program, &stream, frame, &mut budget);
                }
            }
        }
    }
    assert!(passed > 0, "some sequences pass their check");
}

/// Every instruction executed is charged against the run's one budget, a
/// skipped block's instructions excepted; the instruction past it is
/// refused by name, and so is every block the run runs after it.
#[test]
fn a_program_past_its_budget_is_refused_and_named() {
    let stream = reading_stream();
    let frame = Frame {
        position: 0,
        under_test: Some(1),
    };
    let four = checked(&[0x01, 1, 0x01, 2, 0x06, 0x30], CONSTRAINT).expect("checks");
    assert_eq!(four.instructions().len(), 4);

    let exact = Limits {
        max_instructions: 4,
        ..Limits::DEFAULT
    };
    let mut budget = Budget::new(&exact);
    assert_eq!(machine::answer(&four, &stream, frame, &mut budget), Ok(3));
    assert_eq!(budget.left(), 0);
    assert!(!budget.is_spent());
    // The next block the run runs finds the budget spent.
    assert_eq!(
        machine::answer(&four, &stream, frame, &mut budget),
        Err(Refusal::TooManyInstructions)
    );
    assert!(budget.is_spent());

    let short = Limits {
        max_instructions: 3,
        ..Limits::DEFAULT
    };
    let mut budget = Budget::new(&short);
    assert_eq!(
        machine::answer(&four, &stream, frame, &mut budget),
        Err(Refusal::TooManyInstructions)
    );

    // A skipped block costs its `ContextItem` and nothing inside it.
    let skipped = checked(&[0x22, 0, 5, 0x01, 1, 0x01, 2, 0x06, 0x30], CONSTRAINT).expect("checks");
    let two = Limits {
        max_instructions: 2,
        ..Limits::DEFAULT
    };
    let mut budget = Budget::new(&two);
    assert_eq!(
        machine::answer(&skipped, &stream, frame, &mut budget),
        Ok(1)
    );
    assert_eq!(budget.left(), 0);

    // Two blocks share one budget.
    let seven = Limits {
        max_instructions: 7,
        ..Limits::DEFAULT
    };
    let mut budget = Budget::new(&seven);
    assert_eq!(machine::answer(&four, &stream, frame, &mut budget), Ok(3));
    assert_eq!(
        machine::answer(&four, &stream, frame, &mut budget),
        Err(Refusal::TooManyInstructions)
    );
    // The run's budget is scaled to its length.
    assert_eq!(Budget::new(&Limits::for_glyphs(1_000)).left(), 64_000_000);
}

/// What stops a program that passed its check: a division the machine
/// cannot do, a slot offset outside the stream, a read the stream refuses
/// and an action's program run as a constraint, each named; and a pass
/// constraint, which tests no slot.
#[test]
fn a_program_that_cannot_go_on_is_refused_and_named() {
    let named = |code: &[u8], fault: Fault, at: u16| {
        assert_eq!(
            answered(code),
            Err(Refusal::CodeRefused {
                pass: 3,
                block: Block::Constraint(7),
                at,
                fault,
            }),
            "{code:02X?}"
        );
    };
    named(&[0x01, 7, 0x01, 0, 0x09, 0x30], Fault::DivisionByZero, 4);
    named(
        &[0x05, 0x80, 0, 0, 0, 0x01, 0xFF, 0x09, 0x30],
        Fault::DivisionOverflow,
        7,
    );
    named(&[0x28, 20, 5, 0x30], Fault::SlotOutOfRange, 0);
    named(&[0x28, 20, 0x80, 0x30], Fault::SlotOutOfRange, 0);
    named(&[0x2B, 0, 2, 0x30], Fault::SlotOutOfRange, 0);

    // A glyph the face cannot read is the stream's refusal, passed on.
    let mut stream = reading_stream();
    stream.unreadable = vec![11];
    let program = checked(&[0x29, 5, 0, 0x30], CONSTRAINT).expect("checks");
    let frame = Frame {
        position: 0,
        under_test: Some(1),
    };
    let mut budget = Budget::new(&Limits::DEFAULT);
    assert_eq!(
        machine::answer(&program, &stream, frame, &mut budget),
        Err(Refusal::GlyphUnreadable { glyph: 11 })
    );

    // An action's program handed to a constraint's run is refused at its
    // first instruction that acts.
    let action = checked(&[0x1B, 0x31], ACTION).expect("an action checks");
    assert_eq!(
        machine::answer(&action, &stream, frame, &mut budget),
        Err(Refusal::CodeRefused {
            pass: 3,
            block: Block::Action(7),
            at: 0,
            fault: Fault::ActionInConstraint(0x1B),
        })
    );

    // A pass constraint tests no slot: its offsets count from the stream's
    // first slot, and every `ContextItem` in it is skipped.
    let pass = Site {
        pass: 3,
        block: Block::PassConstraint,
    };
    let none = Frame {
        position: 0,
        under_test: None,
    };
    let skipped = checked(&[0x22, 0, 2, 0x01, 0, 0x30], pass).expect("checks");
    let mut budget = Budget::new(&Limits::DEFAULT);
    assert_eq!(machine::answer(&skipped, &stream, none, &mut budget), Ok(1));
    let first = checked(&[0x28, 20, 0, 0x30], pass).expect("checks");
    assert_eq!(machine::answer(&first, &stream, none, &mut budget), Ok(-40));
    assert!(machine::holds(&skipped, &stream, none, &mut budget).expect("runs"));
}

/// Every block of the three vendored faces passes its check against its
/// own description's bounds; each slot attribute and metric number the
/// design reads off them decodes to its name in the rule it was read from;
/// and Awami Nastaliq's two-slot constraint holds for every slot of its
/// match, as the design's skipped-test reading has it.
#[test]
fn the_vendored_faces_code_checks_and_names_its_attributes() {
    let limits = Limits::DEFAULT;
    let mut blocks = BTreeMap::new();
    let mut programs: BTreeMap<(&'static str, u8, String), machine::Program> = BTreeMap::new();
    for (name, bytes) in [
        ("Dai Banna", DAI_BANNA),
        ("Tagmukay", TAGMUKAY),
        ("Awami", AWAMI),
    ] {
        let face = Sfnt::parse(bytes).expect("the face parses");
        let tables = Tables::parse(&face, &limits).expect("a description");
        let features = tables
            .features()
            .map_or(0, |f| f.expect("its Feat reads").len());
        let silf = graphite::Silf::parse(tables.silf()).expect("its Silf reads");
        let description = silf.description(0).expect("its description");
        let bounds = Bounds::of(&description, features);
        let mut counted = [0usize; 3];
        for number in 0..usize::from(description.passes) {
            let pass = description.pass(number, &limits).expect("the pass reads");
            let pass_number = u8::try_from(number).expect("a few passes");
            let mut each = |block: Block, code: &[u8], slot: usize| {
                let site = Site {
                    pass: pass_number,
                    block,
                };
                let program = machine::check(code, site, &bounds, &limits)
                    .unwrap_or_else(|refusal| panic!("{name} {site:?}: {refusal:?}"));
                counted[slot] += 1;
                programs.insert((name, pass_number, format!("{block:?}")), program);
            };
            if let Some(code) = pass.pass_constraint() {
                each(Block::PassConstraint, code, 2);
            }
            for rule in 0..pass.rules {
                let constraint = pass.constraint(rule).expect("its constraint");
                each(Block::Constraint(rule), constraint, 0);
                let action = pass.action(rule).expect("its action");
                each(Block::Action(rule), action, 1);
            }
        }
        blocks.insert(name, counted);
    }
    assert_eq!(
        blocks,
        BTreeMap::from([
            ("Awami", [2_731, 2_731, 6]),
            ("Dai Banna", [59, 59, 0]),
            ("Tagmukay", [13, 13, 0]),
        ]),
        "rule constraints, actions and pass constraints, every one checked"
    );

    let ops = |name: &'static str, pass: u8, block: Block| -> Vec<Op> {
        programs
            .get(&(name, pass, format!("{block:?}")))
            .expect("a checked block")
            .instructions()
            .iter()
            .map(|instruction| instruction.op)
            .collect()
    };
    let set = |attribute| Op::SetAttribute { attribute };
    let read = |attribute, slot| Op::PushSlotAttribute { attribute, slot };

    // Dai Banna SIL's line-break rule: `breakweight`, 14.
    assert_eq!(
        ops("Dai Banna", 0, Block::Action(0)),
        vec![
            Op::PutCopy { slot: 0 },
            Op::Push(-30),
            set(SlotAttribute::BreakWeight),
            Op::Next,
            Op::ReturnZero
        ]
    );
    // Tagmukay's ligatures reference their components, 15; its features
    // are tested by their index in `Feat`.
    assert!(ops("Tagmukay", 0, Block::Action(0))
        .iter()
        .any(|op| matches!(
            op,
            Op::SetIndexedSlotReference {
                attribute: SlotAttribute::ComponentReference,
                ..
            }
        )));
    assert_eq!(
        ops("Tagmukay", 1, Block::Constraint(3)),
        vec![
            Op::PushFeature {
                feature: 1,
                slot: 0
            },
            Op::PopReturn
        ]
    );
    // Awami Nastaliq's pass 17, rule 17: `shift`, `collision` and
    // `sequence`, in the order the rule writes them.
    let flags = SlotAttribute::CollisionFlags;
    assert_eq!(
        ops("Awami", 17, Block::Action(17)),
        vec![
            Op::PutCopy { slot: 0 },
            Op::Push(300),
            set(SlotAttribute::ShiftX),
            Op::Push(350),
            set(SlotAttribute::ShiftY),
            read(flags, 0),
            Op::SetBits { mask: 2, value: 0 },
            set(flags),
            read(flags, 0),
            Op::SetBits { mask: 1, value: 1 },
            set(flags),
            Op::Push(-200),
            set(SlotAttribute::CollisionMinX),
            Op::Push(250),
            set(SlotAttribute::CollisionMaxX),
            Op::Push(-300),
            set(SlotAttribute::CollisionMinY),
            Op::Push(200),
            set(SlotAttribute::CollisionMaxY),
            Op::Push(4),
            set(SlotAttribute::SequenceClass),
            Op::Push(1),
            set(SlotAttribute::SequenceProxClass),
            Op::Push(8),
            set(SlotAttribute::SequenceOrder),
            Op::Next,
            Op::ReturnZero,
        ]
    );
    // Its pass 17, rule 69: `margin = 300m; marginweight = 500`.
    let margins = ops("Awami", 17, Block::Action(69));
    let wanted = [
        Op::Push(300),
        set(SlotAttribute::CollisionMargin),
        Op::Push(500),
        set(SlotAttribute::CollisionMarginWeight),
    ];
    assert!(margins.windows(4).any(|w| w == wanted));
    // Its pass 9, rule 73: the exclusion glyph and its offset.
    let exclude = ops("Awami", 9, Block::Action(73));
    for (attribute, glyph_attribute) in [
        (SlotAttribute::CollisionExcludeGlyph, 130u16),
        (SlotAttribute::CollisionExcludeOffsetX, 72),
        (SlotAttribute::CollisionExcludeOffsetY, 73),
    ] {
        let pair = [
            Op::PushGlyphAttribute {
                attribute: glyph_attribute,
                slot: 0,
            },
            set(attribute),
        ];
        assert!(exclude.windows(2).any(|w| w == pair), "{attribute:?}");
    }
    // Its pass 7, rule 0: the cursive attachment, its points, `insert` and
    // `seqWidth`, its `user5`.
    let cursive = ops("Awami", 7, Block::Action(0));
    for wanted in [
        Op::SetSlotReference {
            attribute: SlotAttribute::AttachTo,
        },
        set(SlotAttribute::AttachWithX),
        set(SlotAttribute::AttachWithY),
        set(SlotAttribute::AttachAtX),
        set(SlotAttribute::AttachAtY),
        set(SlotAttribute::Insert),
        read(SlotAttribute::AdvanceX, -6),
        Op::PushIndexedSlotAttribute {
            attribute: SlotAttribute::User,
            slot: -6,
            index: 4,
        },
        Op::SetIndexedAttribute {
            attribute: SlotAttribute::User,
            index: 4,
        },
        Op::PushAttachedGlyphAttribute {
            attribute: 86,
            slot: 0,
        },
    ] {
        assert!(cursive.contains(&wanted), "{wanted:?}");
    }
    // Its pass 12, rule 22: `{position.y + bb.top > SHORTFINAL2}`.
    assert_eq!(
        &ops("Awami", 12, Block::Constraint(22))[1..],
        &[
            read(SlotAttribute::PositionY, 0),
            Op::PushGlyphMetric {
                metric: Metric::BoundingBoxTop,
                slot: 0,
                level: 0
            },
            Op::Add,
            Op::Push(2_700),
            Op::Greater,
            Op::PopReturn,
        ]
    );
    // Its pass 19, rule 16: `advance.x += max(0, @N.collision.fix.x) + 300m`.
    assert_eq!(
        ops("Awami", 19, Block::Action(16)),
        vec![
            Op::PutCopy { slot: 0 },
            Op::Push(0),
            read(SlotAttribute::CollisionFixX, 1),
            Op::Max,
            Op::Push(300),
            Op::Add,
            Op::AddAttribute {
                attribute: SlotAttribute::AdvanceX
            },
            Op::Next,
            Op::ReturnZero,
        ]
    );
    // Its pass 11, rule 133: `shift.y -= 100m; collision.margin -= 50m`.
    assert_eq!(
        ops("Awami", 11, Block::Action(133)),
        vec![
            Op::PutCopy { slot: 0 },
            Op::Push(100),
            Op::SubtractAttribute {
                attribute: SlotAttribute::ShiftY
            },
            Op::Push(50),
            Op::SubtractAttribute {
                attribute: SlotAttribute::CollisionMargin
            },
            Op::Next,
            Op::ReturnZero,
        ]
    );
    // Its pass 6, rule 24: a feature test and `_ {directionality == 1}`.
    let direction = ops("Awami", 6, Block::Constraint(24));
    assert!(direction.contains(&read(SlotAttribute::Directionality, 0)));

    // Its pass 14, rule 0: `_=D {att.to==0}  _=H {att.to==0}`, two blocks
    // joined by one `And`. Every slot of the match answers true while D and
    // H are unattached, and the slot under test that is H answers false
    // once H is attached.
    let constraint = programs
        .get(&("Awami", 14u8, format!("{:?}", Block::Constraint(0))))
        .expect("a checked block");
    let blocks_in = constraint
        .instructions()
        .iter()
        .filter(|i| matches!(i.op, Op::ContextItem { .. }))
        .count();
    assert_eq!(blocks_in, 2);
    let mut stream = Stream {
        glyphs: vec![1; 6],
        ..Stream::default()
    };
    let answers = |stream: &Stream, under_test: usize| {
        let mut budget = Budget::new(&limits);
        let frame = Frame {
            position: 0,
            under_test: Some(under_test),
        };
        machine::holds(constraint, stream, frame, &mut budget).expect("runs")
    };
    for under_test in 0..6 {
        assert!(answers(&stream, under_test), "slot {under_test}");
    }
    stream
        .slot_attributes
        .insert((5, SlotAttribute::AttachTo, 0), -1);
    assert!(!answers(&stream, 5));
    assert!(answers(&stream, 4) && answers(&stream, 0));
}

// --- acting on the stream (FT-07d.8) -------------------------------------
//
// `acting_face` is a face built for actions: glyphs whose advances say who
// they are, a description whose classes the actions select from, glyph
// attributes a slot's defaults come from, and two features. Each action is
// the bytes the GTF appendix lays its instructions out as, hand-compiled
// from the GDL rule beside it, and run through `Slots::act` on a stream the
// test states; what it leaves is read back through the stream.

use tinker_pdf_shape::graphite::{Acted, FeatureValues, Slots, Source};

/// Where the action tests' actions stand: pass 3, before the positioning
/// passes `acting_face`'s description begins at 10.
const ACTING: Site = Site {
    pass: 3,
    block: Block::Action(1),
};

/// The classes the action tests select from: linear 0 `[40]`, 1
/// `[50, 51, 52]` and 6 the compiler documentation's upper vowels, the rest
/// one glyph each; lookup 10 its lower vowels and 11 `[30, 31, 32]`.
fn acting_classes() -> Vec<u8> {
    let mut linear: Vec<Vec<u16>> = (0..10).map(|n| vec![200 + n]).collect();
    linear[0] = vec![40];
    linear[1] = vec![50, 51, 52];
    linear[6] = vec![0x53, 0x57, 0x5B, 0x61, 0x67];
    let lower = vec![(0x21, 0), (0x25, 1), (0x29, 2), (0x2F, 3), (0x35, 4)];
    let input = vec![(30, 0), (31, 1), (32, 2)];
    class_map(&linear, &[lower, input], true)
}

/// The face the action tests run over: 220 glyphs, glyph `n` `500 + n`
/// wide; a `Silf` 4 description of [`acting_classes`], three user
/// attributes, positioning from pass 10, and a collision pass whose glyph
/// attributes start at 20; glyph 5's break weight −15 and directionality 3;
/// glyph 7's collision flags 1, `min.x` −100, margin 150 and sequence class
/// 4; and a `Feat` of two features, the first declaring 0 to 2, the second 0
/// and 1.
fn acting_face() -> Vec<u8> {
    let spec = DescriptionSpec {
        class_map: acting_classes(),
        order: [0, 10, 10, 255],
        flags: 0x20,
        user_attributes: 3,
        attr_collisions: 20,
        ..DescriptionSpec::default()
    };
    let silf = silf_table(4, &[spec]);
    let mut maps = vec![BTreeMap::new(); 220];
    maps[5] = BTreeMap::from([(1, -15), (2, 3)]);
    maps[7] = BTreeMap::from([(20, 1), (21, -100), (25, 150), (27, 4)]);
    let index = Index {
        long: false,
        names: false,
    };
    let (glat, gloc) = glat_and_gloc(GlatVersion::Two, &maps, &[], index, 32);
    let feat = feat_table(&[
        (id(b"aaaa"), 0x8000, &[0, 1, 2]),
        (id(b"bbbb"), 0x8000, &[0, 1]),
    ]);
    Face::new(220, &[])
        .with(b"Silf", silf)
        .with(b"Glat", glat)
        .with(b"Gloc", gloc)
        .with(b"Feat", feat)
        .build()
}

/// Runs `test` over a stream of `glyphs` — each `(glyph, first, last)`, the
/// last two byte offsets of the characters it stands for — of the face
/// `bytes`, its features at their defaults.
fn with_stream<R>(
    bytes: &[u8],
    glyphs: &[(u16, u32, u32)],
    test: impl FnOnce(&mut Slots<'_>) -> R,
) -> R {
    let limits = Limits::DEFAULT;
    let sfnt = Sfnt::parse(bytes).expect("the face parses");
    let tables = Tables::parse(&sfnt, &limits).expect("a description");
    let silf = graphite::Silf::parse(tables.silf()).expect("its Silf reads");
    let description = silf.description(0).expect("its description");
    let attributes = tables
        .attributes(&limits)
        .map(|read| read.expect("its attributes read"));
    let features = tables.features().map(|read| read.expect("its Feat reads"));
    let values = features.as_ref().map_or_else(Vec::new, |features| {
        FeatureValues::defaults(features).as_slice().to_vec()
    });
    let source = Source {
        sfnt: &sfnt,
        attributes: attributes.as_ref(),
        description: &description,
        features: features.as_ref(),
    };
    let mut slots = Slots::new(source, glyphs, &values);
    test(&mut slots)
}

/// `code`, checked as an action of pass `pass`, run at `position`.
fn act_in(
    slots: &mut Slots<'_>,
    pass: u8,
    code: &[u8],
    position: usize,
    limits: &Limits,
) -> Result<Acted, Refusal> {
    let features = slots.face().features.map_or(0, |features| features.len());
    let bounds = Bounds::for_pass(slots.face().description, features, usize::from(pass));
    let site = Site {
        pass,
        block: ACTING.block,
    };
    let program = machine::check(code, site, &bounds, limits)?;
    let mut budget = Budget::new(limits);
    slots.act(&program, position, &mut budget, limits)
}

/// `code`, checked as an action of pass 3, run at `position`.
fn act(slots: &mut Slots<'_>, code: &[u8], position: usize) -> Result<Acted, Refusal> {
    act_in(slots, ACTING.pass, code, position, &Limits::DEFAULT)
}

/// The three slots most action tests start from: glyph 30, the input
/// class's first; glyph 5, with a break weight; glyph 7, with collision
/// attributes; each standing for one character.
const THREE: [(u16, u32, u32); 3] = [(30, 0, 0), (5, 1, 1), (7, 2, 2)];

/// Each slot's characters.
fn spans(slots: &Slots<'_>) -> Vec<(u32, u32)> {
    (0..slots.len())
        .map(|at| slots.slot(at).expect("a slot").characters())
        .collect()
}

/// `attribute` of slot `at`, as the stream reads it.
fn attribute(slots: &Slots<'_>, at: usize, attribute: SlotAttribute, index: u8) -> i32 {
    slots
        .slot_attribute(at, attribute, index)
        .expect("the stream answers")
}

/// The pointer and offset an action leaves.
const fn acted(pointer: i64, offset: i32) -> Acted {
    Acted { pointer, offset }
}

/// A program per action instruction, the stream it leaves the one the GTF
/// appendix's table describes; a slot's defaults coming from its glyph; the
/// operands counting slots as the action found them; a positioning pass
/// held to one-to-one; what stops an action, each named, its effects to
/// that point standing; and the vendored Dai Banna SIL's composition, run.
#[test]
fn each_action_instruction_rewrites_the_stream_as_specified() {
    let face = acting_face();
    let run = |glyphs: &[(u16, u32, u32)], code: &[u8], position: usize| {
        with_stream(&face, glyphs, |slots| {
            let result = act(slots, code, position);
            (result, slots.glyphs(), spans(slots))
        })
    };
    let mut programs = 0usize;

    // `Next` and `CopyNext` move the pointer and leave the slots.
    let (result, glyphs, _) = run(&THREE, &[0x19, 0x19, 0x31], 0);
    assert_eq!((result, glyphs), (Ok(acted(2, 0)), vec![30, 5, 7]));
    let (result, glyphs, _) = run(&THREE, &[0x1B, 0x31], 1);
    assert_eq!((result, glyphs), (Ok(acted(2, 0)), vec![30, 5, 7]));
    programs += 2;

    // `PutGlyph`, in both widths: the class's first glyph.
    let (result, glyphs, _) = run(&THREE, &[0x1C, 0, 0x19, 0x31], 1);
    assert_eq!((result, glyphs), (Ok(acted(2, 0)), vec![30, 40, 7]));
    let (_, glyphs, _) = run(&THREE, &[0x3B, 0, 1, 0x31], 0);
    assert_eq!(glyphs, vec![50, 5, 7]);
    programs += 2;

    // `PutSubs`, in both widths: the input glyph's index in the input class
    // selects the output class's glyph; the input slot may be another.
    let (_, glyphs, _) = run(&THREE, &[0x1D, 0, 11, 1, 0x31], 0);
    assert_eq!(glyphs, vec![50, 5, 7]);
    let (_, glyphs, _) = run(
        &[(32, 0, 0), (70, 1, 1)],
        &[0x38, 0xFF, 0, 11, 0, 1, 0x31],
        1,
    );
    assert_eq!(glyphs, vec![32, 52]);
    programs += 2;

    // `PutCopy` copies the input slot's glyph and what was set on it, not
    // its characters; the glyph's own defaults come with the glyph.
    with_stream(&face, &THREE, |slots| {
        assert_eq!(act(slots, &[0x1E, 2, 0x31], 0), Ok(acted(0, 0)));
        assert_eq!(slots.glyphs(), vec![7, 5, 7]);
        assert_eq!(spans(slots)[0], (0, 0));
        assert_eq!(attribute(slots, 0, SlotAttribute::CollisionFlags, 0), 1);
    });
    // "The glyph that was in the input": slot 0 is changed, then copied to
    // slot 1 as the action found it.
    with_stream(&face, &THREE, |slots| {
        let code = [0x01, 9, 0x23, 20, 0x3B, 0, 0, 0x19, 0x1E, 0xFF, 0x31];
        assert_eq!(act(slots, &code, 0), Ok(acted(1, 0)));
        assert_eq!(slots.glyphs(), vec![40, 30, 7]);
        assert_eq!(attribute(slots, 0, SlotAttribute::ShiftX, 0), 9);
        assert_eq!(attribute(slots, 1, SlotAttribute::ShiftX, 0), 0);
    });
    programs += 2;

    // `Insert` puts a slot before the current one and makes it current;
    // `Delete` removes the current one and makes the one before current.
    let (result, glyphs, spans_after) = run(&THREE, &[0x1F, 0x1C, 0, 0x31], 1);
    assert_eq!(result, Ok(acted(1, 0)));
    assert_eq!(glyphs, vec![30, 40, 5, 7]);
    assert_eq!(
        spans_after[1],
        (1, 1),
        "the characters of the slot after it"
    );
    let (result, glyphs, spans_after) = run(&THREE, &[0x20, 0x19, 0x31], 1);
    assert_eq!(result, Ok(acted(1, 0)));
    assert_eq!(glyphs, vec![30, 7]);
    assert_eq!(spans_after, vec![(0, 1), (2, 2)]);
    programs += 2;

    // `Assoc` gives the current slot the span of the input slots it names.
    let (_, _, spans_after) = run(&THREE, &[0x21, 2, 0, 1, 0x19, 0x31], 1);
    assert_eq!(spans_after, vec![(0, 0), (1, 2), (2, 2)]);
    programs += 1;

    // `AttrSet`, `AttrAdd` and `AttrSub`, the last two from the glyph's
    // own value where no rule set one.
    with_stream(&face, &THREE, |slots| {
        act(slots, &[0x01, 9, 0x23, 20, 0x31], 0).expect("acts");
        act(slots, &[0x01, 9, 0x24, 0, 0x31], 1).expect("acts");
        act(slots, &[0x01, 50, 0x25, 64, 0x31], 2).expect("acts");
        assert_eq!(attribute(slots, 0, SlotAttribute::ShiftX, 0), 9);
        assert_eq!(attribute(slots, 1, SlotAttribute::AdvanceX, 0), 505 + 9);
        assert_eq!(attribute(slots, 2, SlotAttribute::CollisionMargin, 0), 100);
        // And the defaults no rule touched: the glyph's break weight and
        // directionality, `insert` true, the rest 0.
        assert_eq!(attribute(slots, 1, SlotAttribute::BreakWeight, 0), -15);
        assert_eq!(attribute(slots, 1, SlotAttribute::Directionality, 0), 3);
        assert_eq!(attribute(slots, 0, SlotAttribute::Insert, 0), 1);
        assert_eq!(attribute(slots, 2, SlotAttribute::CollisionMinX, 0), -100);
        assert_eq!(attribute(slots, 2, SlotAttribute::SequenceClass, 0), 4);
        assert_eq!(attribute(slots, 0, SlotAttribute::CollisionFlags, 0), 0);
        assert_eq!(attribute(slots, 0, SlotAttribute::AdvanceX, 0), 530);
    });
    programs += 3;

    // `AttrSetSlot`: `attach.to` names a slot relative to the current one,
    // and reads back as the offset to it.
    with_stream(&face, &THREE, |slots| {
        act(slots, &[0x01, 0xFF, 0x26, 2, 0x31], 1).expect("acts");
        assert_eq!(slots.attached_to(1), Some(0));
        assert_eq!(attribute(slots, 1, SlotAttribute::AttachTo, 0), -1);
        assert_eq!(attribute(slots, 0, SlotAttribute::AttachTo, 0), 0);
    });
    // `IAttrSetSlot`: a component's reference.
    with_stream(&face, &THREE, |slots| {
        act(slots, &[0x01, 2, 0x27, 15, 3, 0x31], 0).expect("acts");
        assert_eq!(attribute(slots, 0, SlotAttribute::ComponentReference, 3), 2);
        assert_eq!(attribute(slots, 0, SlotAttribute::ComponentReference, 4), 0);
    });
    programs += 2;

    // `IAttrSet`, `IAttrAdd` and `IAttrSub` on a user attribute.
    with_stream(&face, &THREE, |slots| {
        let code = [
            0x01, 4, 0x33, 55, 2, 0x01, 3, 0x34, 55, 2, 0x01, 1, 0x35, 55, 2, 0x31,
        ];
        act(slots, &code, 0).expect("acts");
        assert_eq!(attribute(slots, 0, SlotAttribute::User, 2), 6);
        assert_eq!(attribute(slots, 0, SlotAttribute::User, 1), 0);
    });
    programs += 3;

    // `SetFeat`: the feature of the slot at the offset, clipped to the
    // largest value it declares.
    with_stream(&face, &THREE, |slots| {
        let code = [0x01, 9, 0x42, 0, 1, 0x01, 1, 0x42, 1, 0, 0x31];
        act(slots, &code, 0).expect("acts");
        assert_eq!(slots.feature(1, 0), 2);
        assert_eq!(slots.feature(0, 1), 1);
        assert_eq!(slots.feature(2, 0), 0);
    });
    programs += 1;

    // What an action returns: `PopRet`'s value, `RetZero`, `RetTrue`, and
    // the top of the stack where it ends without one.
    for (code, position, expected) in [
        (&[0x19, 0x01, 0xFE, 0x30][..], 0, acted(1, -2)),
        (&[0x31][..], 1, acted(1, 0)),
        (&[0x32][..], 1, acted(1, 1)),
        (&[0x01, 5][..], 0, acted(0, 5)),
        (&[][..], 2, acted(2, 0)),
    ] {
        assert_eq!(run(&THREE, code, position).0, Ok(expected), "{code:02X?}");
    }
    programs += 5;
    assert_eq!(programs, 25, "a program per case");

    // A positioning pass makes only one-to-one substitutions: an action of
    // one that inserts, deletes or copies another slot fails its check.
    with_stream(&face, &THREE, |slots| {
        for (code, opcode) in [
            (&[0x1F][..], 0x1F),
            (&[0x20][..], 0x20),
            (&[0x1E, 1][..], 0x1E),
        ] {
            assert!(matches!(
                act_in(slots, 12, code, 0, &Limits::DEFAULT),
                Err(Refusal::CodeRefused {
                    pass: 12,
                    fault: Fault::NotOneToOne(op),
                    ..
                }) if op == opcode
            ));
        }
        let one_to_one = [0x1E, 0, 0x1C, 0, 0x1D, 0, 11, 1, 0x31];
        assert!(act_in(slots, 12, &one_to_one, 0, &Limits::DEFAULT).is_ok());
    });

    // What stops an action, each named at its instruction; what it did
    // before stands.
    let stopped = |glyphs: &[(u16, u32, u32)], code: &[u8], at: u16, fault: Fault| {
        let (result, glyphs, _) = run(glyphs, code, 0);
        assert_eq!(
            result,
            Err(Refusal::CodeRefused {
                pass: 3,
                block: Block::Action(1),
                at,
                fault,
            }),
            "{code:02X?}"
        );
        glyphs
    };
    stopped(&THREE, &[0x19, 0x19, 0x19, 0x19], 3, Fault::SlotOutOfRange);
    stopped(
        &THREE,
        &[0x19, 0x19, 0x19, 0x1C, 0],
        3,
        Fault::SlotOutOfRange,
    );
    stopped(
        &[(99, 0, 0)],
        &[0x1D, 0, 11, 1],
        0,
        Fault::NoGlyphInClass(11),
    );
    stopped(
        &[(31, 0, 0)],
        &[0x1D, 0, 11, 0],
        0,
        Fault::NoGlyphInClass(0),
    );
    stopped(&THREE, &[0x21, 1, 5, 0x31], 0, Fault::SlotOutOfRange);
    stopped(&THREE, &[0x01, 9, 0x26, 2], 2, Fault::SlotOutOfRange);
    let kept = stopped(&THREE, &[0x1C, 0, 0x21, 1, 9], 2, Fault::SlotOutOfRange);
    assert_eq!(
        kept,
        vec![40, 5, 7],
        "the glyph put before the refusal stands"
    );
    // A read the stream cannot answer: a slot's position, which positioning
    // resolves, and a box from a face with no `glyf`.
    assert_eq!(
        run(&THREE, &[0x28, 19, 0, 0x23, 20], 0).0,
        Err(Refusal::PositionNotResolved)
    );
    assert_eq!(
        run(&THREE, &[0x2A, 2, 0, 0, 0x23, 20], 0).0,
        Err(Refusal::GlyphBoundsUnavailable { glyph: 30 })
    );

    // The vendored Dai Banna SIL's composition rule, as compiled: `e` and the
    // acute's pseudo-glyph become `é`, standing for both characters.
    let limits = Limits::DEFAULT;
    let sfnt = Sfnt::parse(DAI_BANNA).expect("Dai Banna SIL parses");
    let e = sfnt.glyph_for_char('e').expect("e");
    let composed = sfnt.glyph_for_char('\u{E9}').expect("é");
    with_stream(DAI_BANNA, &[(e, 0, 0), (413, 1, 1)], |slots| {
        let description = slots.face().description;
        let pass = description.pass(1, &limits).expect("pass 1");
        let found = pass.candidates(&slots.glyphs(), 0, &limits);
        let rule = found.candidates[0].rule;
        let features = slots.face().features.map_or(0, |f| f.len());
        let bounds = Bounds::for_pass(description, features, 1);
        let site = Site {
            pass: 1,
            block: Block::Action(rule),
        };
        let code = pass.action(rule).expect("its action");
        let program = machine::check(code, site, &bounds, &limits).expect("it checks");
        let mut budget = Budget::new(&limits);
        let result = slots.act(&program, 0, &mut budget, &limits);
        assert_eq!(result, Ok(acted(1, 0)));
        assert_eq!(slots.glyphs(), vec![composed]);
        assert_eq!(spans(slots), vec![(0, 1)]);
    });
}

/// An inserted slot takes the characters of the slot after it, or of the
/// one before it at the stream's end, until an `Assoc` gives it others; a
/// deleted slot's characters join the slot before it, or after it at the
/// start, unless an `Assoc` of its action names it; and the clusters of
/// the stream that comes out never go backwards.
#[test]
fn an_inserted_slot_takes_the_cluster_the_design_states() {
    let face = acting_face();
    // A character of two bytes in the middle.
    let three = [(30, 0, 0), (5, 1, 2), (7, 3, 3)];
    let after = |code: &[u8], position: usize| {
        with_stream(&face, &three, |slots| {
            act(slots, code, position).expect("acts");
            let clusters = slots.clusters();
            assert!(
                clusters.windows(2).all(|pair| pair[0] <= pair[1]),
                "{clusters:?}"
            );
            (spans(slots), clusters)
        })
    };
    // Inserted at the start, in the middle and at the end.
    assert_eq!(
        after(&[0x1F, 0x1C, 0, 0x31], 0),
        (vec![(0, 0), (0, 0), (1, 2), (3, 3)], vec![0, 0, 1, 3])
    );
    assert_eq!(
        after(&[0x1F, 0x1C, 0, 0x31], 1),
        (vec![(0, 0), (1, 2), (1, 2), (3, 3)], vec![0, 1, 1, 3])
    );
    assert_eq!(
        after(&[0x19, 0x19, 0x19, 0x1F, 0x1C, 0, 0x31], 0),
        (vec![(0, 0), (1, 2), (3, 3), (3, 3)], vec![0, 1, 3, 3])
    );
    // Inserted at the start and associated with the second input slot: it
    // stands before the first, so the two are one cluster.
    assert_eq!(
        after(&[0x1F, 0x1C, 0, 0x21, 1, 2, 0x31], 0),
        (vec![(1, 2), (0, 0), (1, 2), (3, 3)], vec![0, 0, 0, 3])
    );
    // Deleted, unassociated: into the slot before, or after at the start.
    assert_eq!(
        after(&[0x20, 0x19, 0x31], 1),
        (vec![(0, 2), (3, 3)], vec![0, 3])
    );
    assert_eq!(
        after(&[0x20, 0x19, 0x31], 0),
        (vec![(0, 2), (3, 3)], vec![0, 3])
    );
    // Deleted and then named by the next slot's `Assoc`, as the GDL's joiner
    // is: the slot before keeps its own.
    assert_eq!(
        after(&[0x20, 0x19, 0x21, 2, 0xFF, 0, 0x31], 1),
        (vec![(0, 0), (1, 3)], vec![0, 1])
    );
    // Two slots exchanged: the span they make is one cluster.
    assert_eq!(
        after(
            &[0x1E, 1, 0x21, 1, 1, 0x19, 0x1E, 0xFF, 0x21, 1, 0xFF, 0x31],
            0
        ),
        (vec![(1, 2), (0, 0), (3, 3)], vec![0, 0, 3])
    );
}

/// Every insertion is charged against `Limits::max_glyphs`: the one past it
/// is refused by name at its instruction, the action ends there, and what
/// it did before stands.
#[test]
fn an_insertion_past_the_glyph_ceiling_is_refused_and_named() {
    let face = acting_face();
    let ceiling = |max_glyphs: usize| Limits {
        max_glyphs,
        ..Limits::DEFAULT
    };
    with_stream(&face, &THREE, |slots| {
        let result = act_in(slots, 3, &[0x1C, 0, 0x1F, 0x31], 0, &ceiling(3));
        assert_eq!(
            result,
            Err(Refusal::CodeRefused {
                pass: 3,
                block: Block::Action(1),
                at: 2,
                fault: Fault::TooManySlots,
            })
        );
        assert_eq!(slots.glyphs(), vec![40, 5, 7]);
    });
    with_stream(&face, &THREE, |slots| {
        let result = act_in(slots, 3, &[0x1F, 0x1F, 0x31], 0, &ceiling(4));
        assert!(matches!(
            result,
            Err(Refusal::CodeRefused {
                at: 1,
                fault: Fault::TooManySlots,
                ..
            })
        ));
        assert_eq!(slots.len(), 4, "the first insertion stands");
    });
    with_stream(&face, &THREE, |slots| {
        let result = act_in(slots, 3, &[0x1F, 0x1F, 0x31], 0, &ceiling(5));
        assert_eq!(result, Ok(acted(0, 0)));
        assert_eq!(slots.len(), 5);
    });
    // A run's own ceiling leaves room for a run's worth of insertions.
    with_stream(&face, &THREE, |slots| {
        let code: Vec<u8> = std::iter::repeat(0x1F).take(64).chain([0x31]).collect();
        assert!(act_in(slots, 3, &code, 0, &Limits::for_glyphs(3)).is_ok());
        assert_eq!(slots.len(), 67);
    });
}

/// The design's examples of acting on the stream, each a GDL rule
/// hand-compiled into the action the GTF lays out, run and counted.
#[test]
fn the_action_examples_hold() {
    let face = acting_face();
    let mut held = 0usize;
    let result = |glyphs: &[(u16, u32, u32)], code: &[u8], position: usize| {
        with_stream(&face, glyphs, |slots| {
            act(slots, code, position).expect("acts");
            (slots.glyphs(), spans(slots), slots.clusters())
        })
    };

    // The compiler documentation's `PutSubs 0 10 6`: "a", index 0 of the
    // lower vowels' class 10, becomes "A", index 0 of the upper vowels' 6.
    assert_eq!(
        result(&[(0x21, 0, 0)], &[0x1D, 0, 10, 6, 0x31], 0).0,
        vec![0x53]
    );
    held += 1;

    // GDL 4.4.1: `c(192) > clsLowTones$1 / clsVowel _` — the vowel before
    // selects the tone.
    let low_tone = [0x1D, 0xFF, 11, 1, 0x19, 0x31];
    assert_eq!(
        result(&[(31, 0, 0), (70, 1, 1)], &low_tone, 1).0,
        vec![31, 51]
    );
    held += 1;

    // GDL 4.4.2: `clsCons clsVowel1 clsVowel2 > @2 @1 @3`.
    let reorder = [
        0x1E, 1, 0x21, 1, 1, 0x19, 0x1E, 0xFF, 0x21, 1, 0xFF, 0x19, 0x1E, 0, 0x21, 1, 0, 0x19, 0x31,
    ];
    let (glyphs, spans_after, clusters) = result(&THREE, &reorder, 0);
    assert_eq!(glyphs, vec![5, 30, 7]);
    assert_eq!(spans_after, vec![(1, 1), (0, 0), (2, 2)]);
    assert_eq!(clusters, vec![0, 0, 2]);
    held += 1;

    // GDL 4.4.3: `clsCons gDepI > @2:2 @1:1` — each glyph moved with its
    // own character.
    let swap = [
        0x1E, 1, 0x21, 1, 1, 0x19, 0x1E, 0xFF, 0x21, 1, 0xFF, 0x19, 0x31,
    ];
    let (glyphs, spans_after, clusters) = result(&[(60, 0, 0), (61, 1, 1)], &swap, 0);
    assert_eq!(glyphs, vec![61, 60]);
    assert_eq!(spans_after, vec![(1, 1), (0, 0)]);
    assert_eq!(clusters, vec![0, 0]);
    held += 1;

    // GDL 4.4.4: `_ gDepI > @3 _ / _ clsConsC _` — the dependent I inserted
    // before the consonant and deleted after it, associated with itself.
    let moved = [0x1F, 0x1E, 2, 0x21, 1, 2, 0x19, 0x1B, 0x20, 0x19, 0x31];
    let (glyphs, spans_after, clusters) = result(&[(60, 0, 0), (61, 1, 1)], &moved, 0);
    assert_eq!(glyphs, vec![61, 60]);
    assert_eq!(spans_after, vec![(1, 1), (0, 0)]);
    assert_eq!(clusters, vec![0, 0]);
    held += 1;

    // GDL 4.4.4: `ZWJ clsCons > _ clsConsJoin:(2 3) / clsCons _ _` — the
    // joiner deleted into the consonant after it, the consonant before
    // keeping its own character.
    let joined = [0x20, 0x19, 0x1D, 0, 11, 1, 0x21, 2, 0xFF, 0, 0x19, 0x31];
    let (glyphs, spans_after, clusters) = result(&[(40, 0, 0), (90, 1, 3), (32, 4, 4)], &joined, 1);
    assert_eq!(glyphs, vec![40, 52]);
    assert_eq!(spans_after, vec![(0, 0), (1, 4)]);
    assert_eq!(clusters, vec![0, 1]);
    held += 1;

    // GDL 4.4.4: the Thai split vowel, `_ VSchwa > VE:4 VShort / _ clsCons
    // clsTone? _` — both parts standing for the vowel.
    let split = [
        0x1F, 0x1C, 0, 0x21, 1, 3, 0x19, 0x1B, 0x1B, 0x1C, 2, 0x19, 0x31,
    ];
    let (glyphs, spans_after, clusters) = result(&[(60, 0, 0), (61, 1, 1), (62, 2, 2)], &split, 0);
    assert_eq!(glyphs, vec![40, 60, 61, 202]);
    assert_eq!(spans_after, vec![(2, 2), (0, 0), (1, 1), (2, 2)]);
    assert_eq!(clusters, vec![0, 0, 0, 0]);
    held += 1;

    // GDL 4.4.5: `Co ZWJ Ce > Coe:(1 2 3){component{o.reference = @1;
    // e.ref = @3}} _ _` — one ligature for the three, its first component
    // itself and its last a slot it deleted.
    let ligature = [
        0x1C, 0, 0x21, 3, 0, 1, 2, 0x01, 0, 0x27, 15, 0, 0x01, 2, 0x27, 15, 1, 0x19, 0x20, 0x19,
        0x20, 0x19, 0x31,
    ];
    with_stream(&face, &[(63, 0, 0), (64, 1, 3), (65, 4, 4)], |slots| {
        assert_eq!(act(slots, &ligature, 0), Ok(acted(1, 0)));
        assert_eq!(slots.glyphs(), vec![40]);
        assert_eq!(spans(slots), vec![(0, 4)]);
        assert_eq!(attribute(slots, 0, SlotAttribute::ComponentReference, 0), 0);
        assert_eq!(attribute(slots, 0, SlotAttribute::ComponentReference, 1), 0);
    });
    held += 1;

    // GDL 6.3: `clsToneMark gRaiseMark > @1 { raiseFlag = true } _` in the
    // substitution table, and `clsToneMark { shift.y = 100m } / _ {
    // raiseFlag == true }` in the positioning table: the flag, `user1`, set
    // by one pass and read by a later one.
    with_stream(&face, &[(66, 0, 0), (67, 1, 1)], |slots| {
        let raise = [0x1E, 0, 0x01, 1, 0x33, 55, 0, 0x19, 0x20, 0x19, 0x31];
        act(slots, &raise, 0).expect("the substitution acts");
        assert_eq!(slots.glyphs(), vec![66]);
        let features = slots.face().features.map_or(0, |f| f.len());
        let bounds = Bounds::for_pass(slots.face().description, features, 12);
        let site = Site {
            pass: 12,
            block: Block::Constraint(0),
        };
        let test = [0x22, 0, 7, 0x2E, 55, 0, 0, 0x01, 1, 0x13, 0x30];
        let program = machine::check(&test, site, &bounds, &Limits::DEFAULT).expect("checks");
        let mut budget = Budget::new(&Limits::DEFAULT);
        let frame = Frame {
            position: 0,
            under_test: Some(0),
        };
        assert!(machine::holds(&program, &*slots, frame, &mut budget).expect("runs"));
        let shift = [0x1E, 0, 0x01, 100, 0x23, 21, 0x19, 0x31];
        act_in(slots, 12, &shift, 0, &Limits::DEFAULT).expect("the positioning acts");
        assert_eq!(attribute(slots, 0, SlotAttribute::ShiftY, 0), 100);
    });
    held += 1;

    assert_eq!(held, 9, "the action examples the design counts");
}

/// A slot's collision and sequence defaults are its glyph's attributes from
/// `attCollisions` on, in the order the design reads off Awami Nastaliq: its
/// upper nuqtas' glyph-table `collision { margin = STDMARGIN; marginweight =
/// STDMARGINWT }` and `sequence { class = ORDER_UNUQTAS; order =
/// ORDER_LEFTDOWN; … }` read 150, 400, 1, 0 and 1 in a slot no rule touched.
#[test]
fn a_slot_takes_its_collision_defaults_from_its_glyph() {
    let limits = Limits::DEFAULT;
    let sfnt = Sfnt::parse(AWAMI).expect("Awami Nastaliq parses");
    let tables = Tables::parse(&sfnt, &limits).expect("a description");
    let silf = graphite::Silf::parse(tables.silf()).expect("its Silf reads");
    let description = silf.description(0).expect("its description");
    assert_eq!(description.attr_collisions, 4);
    assert_ne!(description.flags & 0x20, 0, "a collision pass");
    let attributes = tables
        .attributes(&limits)
        .expect("Gloc and Glat")
        .expect("readable");
    let base = 4u16;
    let upper = (0..u16::try_from(attributes.glyph_count()).expect("a face's glyphs"))
        .find(|glyph| {
            let at = |offset: u16| attributes.attribute(*glyph, base + offset);
            at(5) == Some(150) && at(6) == Some(400) && at(7) == Some(1) && at(9) == Some(1)
        })
        .expect("an upper nuqta");
    let features = tables.features().map(|read| read.expect("its Feat reads"));
    let values = features.as_ref().map_or_else(Vec::new, |features| {
        FeatureValues::defaults(features).as_slice().to_vec()
    });
    let source = Source {
        sfnt: &sfnt,
        attributes: Some(&attributes),
        description: &description,
        features: features.as_ref(),
    };
    let slots = Slots::new(source, &[(upper, 0, 1)], &values);
    let read = |attribute| attribute_of(&slots, attribute);
    assert_eq!(read(SlotAttribute::CollisionMargin), 150);
    assert_eq!(read(SlotAttribute::CollisionMarginWeight), 400);
    assert_eq!(read(SlotAttribute::SequenceClass), 1);
    assert_eq!(read(SlotAttribute::SequenceProxClass), 0);
    assert_eq!(read(SlotAttribute::SequenceOrder), 1);
    for limit in [
        SlotAttribute::CollisionMinX,
        SlotAttribute::CollisionMinY,
        SlotAttribute::CollisionMaxX,
        SlotAttribute::CollisionMaxY,
    ] {
        assert_eq!(read(limit), 0, "no glyph table sets {limit:?}");
    }
    let advance = i32::from(sfnt.advance(upper).expect("an advance"));
    assert_eq!(read(SlotAttribute::AdvanceX), advance);
}

/// `attribute` of slot 0.
fn attribute_of(slots: &Slots<'_>, attribute: SlotAttribute) -> i32 {
    slots
        .slot_attribute(0, attribute, 0)
        .expect("the stream answers")
}

// --- passes running (FT-07d.9) -------------------------------------------
//
// `compile` turns rules as a test states them — each slot's glyphs, how
// many slots come before the scan position, and the code — into the pass
// the GTF lays out: a machine found by subset construction over the rules,
// each padded at its front with `ANY` to the longest pre-context, as the
// compiler documentation says the compiler pads them, its states in the
// GTF's order (transitional ones that do not accept, transitional ones that
// do, final ones), each glyph below the face's count its own column, and
// each rule's sort key its length. The GDL examples are stated as such rules
// beside the GDL they compile, and run through `pass::substitute` or
// `pass::run_passes` on the stream the example names.

use std::collections::VecDeque;

use tinker_pdf_shape::graphite::pass;

/// A rule as a test states it.
#[derive(Clone, Debug, Default)]
struct RuleSpec {
    /// How many of its slots come before the scan position.
    pre: u8,
    /// Each slot's glyphs; an empty slot is `ANY`.
    slots: Vec<Vec<u16>>,
    /// Its constraint's code.
    constraint: Vec<u8>,
    /// Its action's code.
    action: Vec<u8>,
}

/// A machine state: each rule a subset construction is part way through,
/// and how many of its padded slots it has matched.
type MachineState = Vec<(usize, usize)>;

/// The pass `rules` compile to over glyphs `0..glyphs`, under the pass's
/// own loop and back-up counts.
fn compile(rules: &[RuleSpec], glyphs: u16, max_rule_loop: u8, max_backup: u8) -> PassSpec {
    let max_pre = rules.iter().map(|rule| rule.pre).max().unwrap_or(0);
    let min_pre = rules.iter().map(|rule| rule.pre).min().unwrap_or(0);
    let padded: Vec<Vec<Vec<u16>>> = rules
        .iter()
        .map(|rule| {
            let mut slots = vec![Vec::new(); usize::from(max_pre - rule.pre)];
            slots.extend(rule.slots.iter().cloned());
            slots
        })
        .collect();
    let takes = |slot: &Vec<u16>, glyph: u16| slot.is_empty() || slot.contains(&glyph);
    // The start state for each count of pre-context slots the stream lacks:
    // every rule, that many of its slots passed over.
    let starts: Vec<MachineState> = (0..=usize::from(max_pre - min_pre))
        .map(|skipped| (0..rules.len()).map(|rule| (rule, skipped)).collect())
        .collect();
    let mut found: Vec<MachineState> = Vec::new();
    let mut rows: BTreeMap<MachineState, Vec<Option<MachineState>>> = BTreeMap::new();
    let mut queue: VecDeque<MachineState> = starts.iter().cloned().collect();
    while let Some(state) = queue.pop_front() {
        if rows.contains_key(&state) {
            continue;
        }
        let row: Vec<Option<MachineState>> = (0..glyphs)
            .map(|glyph| {
                let next: MachineState = state
                    .iter()
                    .filter(|(rule, at)| {
                        padded[*rule]
                            .get(*at)
                            .is_some_and(|slot| takes(slot, glyph))
                    })
                    .map(|(rule, at)| (*rule, at + 1))
                    .collect();
                (!next.is_empty()).then_some(next)
            })
            .collect();
        for next in row.iter().flatten() {
            queue.push_back(next.clone());
        }
        rows.insert(state.clone(), row);
        found.push(state);
    }
    let accepts = |state: &MachineState| state.iter().any(|(rule, at)| *at == padded[*rule].len());
    let moves = |state: &MachineState| rows[state].iter().any(Option::is_some);
    // The GTF's order, the first start state first.
    let mut order: Vec<MachineState> = vec![starts[0].clone()];
    let groups = [
        |accepting: bool, transitional: bool| !accepting && transitional,
        |accepting: bool, transitional: bool| accepting && transitional,
        |accepting: bool, transitional: bool| accepting && !transitional,
    ];
    for group in groups {
        for state in &found {
            if !order.contains(state) && group(accepts(state), moves(state)) {
                order.push(state.clone());
            }
        }
    }
    // A state that neither accepts nor moves is a dead end: a row of zeros.
    for state in &found {
        if !order.contains(state) {
            order.insert(1, state.clone());
        }
    }
    let id = |state: &MachineState| -> u16 {
        u16::try_from(order.iter().position(|s| s == state).expect("a state")).expect("a few")
    };
    let success: Vec<&MachineState> = order.iter().filter(|state| accepts(state)).collect();
    let transitional =
        u16::try_from(order.len() - success.iter().filter(|s| !moves(s)).count()).expect("a few");
    PassSpec {
        max_rule_loop,
        max_backup,
        ranges: (0..glyphs).map(|glyph| (glyph, glyph, glyph)).collect(),
        columns: glyphs,
        rows: u16::try_from(order.len()).expect("a few"),
        transitional,
        success: u16::try_from(success.len()).expect("a few"),
        rule_lists: success
            .iter()
            .map(|state| {
                let mut accepted: Vec<u16> = state
                    .iter()
                    .filter(|(rule, at)| *at == padded[*rule].len())
                    .map(|(rule, _)| u16::try_from(*rule).expect("a few"))
                    .collect();
                accepted.sort_unstable();
                accepted
            })
            .collect(),
        min_pre,
        max_pre,
        start_states: starts.iter().map(id).collect(),
        rules: rules
            .iter()
            .map(|rule| {
                let key = u16::try_from(rule.slots.len()).expect("a short rule");
                (key, rule.pre, rule.constraint.clone(), rule.action.clone())
            })
            .collect(),
        transitions: order[..usize::from(transitional)]
            .iter()
            .map(|state| {
                rows[state]
                    .iter()
                    .map(|next| next.as_ref().map_or(0, |next| id(next)))
                    .collect()
            })
            .collect(),
        ..PassSpec::default()
    }
}

/// A face of `glyphs` glyphs, glyph `n` `500 + n` wide, whose `Silf` of
/// version `major` holds `description`, with glyph attributes `maps` and a
/// `Feat` where one is given.
fn passing_face(
    glyphs: u16,
    major: u32,
    description: DescriptionSpec,
    maps: &[BTreeMap<u16, i16>],
    feat: Option<Vec<u8>>,
) -> Vec<u8> {
    let silf = silf_table(major, &[description]);
    let mut all = maps.to_vec();
    all.resize(usize::from(glyphs), BTreeMap::new());
    let index = Index {
        long: false,
        names: false,
    };
    let (glat, gloc) = glat_and_gloc(GlatVersion::Two, &all, &[], index, 8);
    let mut face = Face::new(glyphs, &[])
        .with(b"Silf", silf)
        .with(b"Glat", glat)
        .with(b"Gloc", gloc);
    if let Some(feat) = feat {
        face = face.with(b"Feat", feat);
    }
    face.build()
}

/// A description of `passes`, its classes `linear` then `lookups`, its
/// positioning from pass 10, for a `Silf` of `major`.
fn passing_description(
    passes: Vec<PassSpec>,
    linear: &[Vec<u16>],
    lookups: &[Vec<(u16, u16)>],
    major: u32,
) -> DescriptionSpec {
    DescriptionSpec {
        class_map: class_map(linear, lookups, major >= 4),
        passes,
        order: [0, 10, 10, 255],
        user_attributes: 2,
        ..DescriptionSpec::default()
    }
}

/// Runs `test` over a stream of `glyphs`, glyph `n` of it standing for
/// character `n`, of the face `bytes`, its features `features` or, where
/// none are given, their defaults.
fn passing<R>(
    bytes: &[u8],
    glyphs: &[u16],
    features: Option<&[i16]>,
    test: impl FnOnce(&mut Slots<'_>) -> R,
) -> R {
    let limits = Limits::DEFAULT;
    let sfnt = Sfnt::parse(bytes).expect("the face parses");
    let tables = Tables::parse(&sfnt, &limits).expect("a description");
    let silf = graphite::Silf::parse(tables.silf()).expect("its Silf reads");
    let description = silf.description(0).expect("its description");
    let attributes = tables
        .attributes(&limits)
        .map(|read| read.expect("its attributes read"));
    let feat = tables.features().map(|read| read.expect("its Feat reads"));
    let values = match features {
        Some(values) => values.to_vec(),
        None => feat.as_ref().map_or_else(Vec::new, |feat| {
            FeatureValues::defaults(feat).as_slice().to_vec()
        }),
    };
    let source = Source {
        sfnt: &sfnt,
        attributes: attributes.as_ref(),
        description: &description,
        features: feat.as_ref(),
    };
    let stream: Vec<(u16, u32, u32)> = glyphs
        .iter()
        .enumerate()
        .map(|(at, glyph)| {
            let at = u32::try_from(at).expect("a short run");
            (*glyph, at, at)
        })
        .collect();
    let mut slots = Slots::new(source, &stream, &values);
    test(&mut slots)
}

/// What the substitution stage of the face `bytes` makes of `glyphs`: its
/// glyphs, its clusters and what it refused.
fn substituted(
    bytes: &[u8],
    glyphs: &[u16],
    right_to_left: bool,
    features: Option<&[i16]>,
) -> (Vec<u16>, Vec<u32>, Vec<Refusal>) {
    passing(bytes, glyphs, features, |slots| {
        let limits = Limits::DEFAULT;
        let mut budget = Budget::new(&limits);
        let refusals = pass::substitute(slots, right_to_left, &limits, &mut budget);
        (slots.glyphs(), slots.clusters(), refusals)
    })
}

/// A rule of slots `slots`, `pre` of them before the scan position, whose
/// action puts class `class`'s glyph in its first modified slot and moves
/// on: `X > Y / …`, the scan position moving to the slot after it.
fn put_rule(pre: u8, slots: &[&[u16]], class: u8) -> RuleSpec {
    RuleSpec {
        pre,
        slots: slots.iter().map(|slot| slot.to_vec()).collect(),
        action: vec![0x1C, class, 0x19, 0x31],
        ..RuleSpec::default()
    }
}

/// `_ {user1 == value}` at `offset`: a `ContextItem` block reading user
/// attribute 0.
fn alt_is(offset: u8, value: u8) -> Vec<u8> {
    vec![0x22, offset, 7, 0x2E, 55, 0, 0, 0x01, value, 0x13]
}

/// The GDL's substitution examples, each its rules compiled into a pass and
/// run over the stream it names; the outputs are the GDL's own, counted.
#[test]
fn substitution_passes_reproduce_the_specifications_examples() {
    let mut held = 0usize;
    let one_pass = |rules: &[RuleSpec], glyphs: u16, linear: &[Vec<u16>], loops: u8, backup: u8| {
        let pass = compile(rules, glyphs, loops, backup);
        let description = passing_description(vec![pass], linear, &[], 4);
        passing_face(glyphs, 4, description, &[], None)
    };

    // 4.1.1: `X > Y / A _ A` over `A X A X A X A` — "the output we might
    // expect (A Y A Y A Y A)", the final A of each match rescanned. A 1, X
    // 2, Y 3.
    let face = one_pass(&[put_rule(1, &[&[1], &[2], &[1]], 0)], 8, &[vec![3]], 5, 0);
    let (glyphs, ..) = substituted(&face, &[1, 2, 1, 2, 1, 2, 1], false, None);
    assert_eq!(glyphs, vec![1, 3, 1, 3, 1, 3, 1]);
    held += 1;

    // 4.1.7.1: `B > Y / W A? _ C` — two rules, with the A and without — and
    // `C > D / B _ E`: "WABC" gives "WAYC", and "WBCE" gives "WYCE", the
    // shorter B rule as long as the C rule and earlier in the file. W 1, A
    // 2, B 3, C 4, E 5; Y 6, D 7.
    let rules = [
        put_rule(2, &[&[1], &[2], &[3], &[4]], 0),
        put_rule(1, &[&[1], &[3], &[4]], 0),
        put_rule(1, &[&[3], &[4], &[5]], 1),
    ];
    let face = one_pass(&rules, 8, &[vec![6], vec![7]], 5, 0);
    assert_eq!(
        substituted(&face, &[1, 2, 3, 4], false, None).0,
        vec![1, 2, 6, 4]
    );
    held += 1;
    assert_eq!(
        substituted(&face, &[1, 3, 4, 5], false, None).0,
        vec![1, 6, 4, 5]
    );
    held += 1;

    // 4.1.7.2: `A > B / W _` and `A > C / _ X Y` over "WAXY": "the output
    // would be WCXY", the second rule's longer context winning. W 1, A 2, X
    // 3, Y 4; B 5, C 6.
    let rules = [
        put_rule(1, &[&[1], &[2]], 0),
        put_rule(0, &[&[2], &[3], &[4]], 1),
    ];
    let face = one_pass(&rules, 8, &[vec![5], vec![6]], 5, 0);
    assert_eq!(
        substituted(&face, &[1, 2, 3, 4], false, None).0,
        vec![1, 6, 3, 4]
    );
    held += 1;

    // 5.1, the example file's two substitution passes. Dotted i 10-12,
    // dotless 13-15; tones 20-24 and upper modifiers 25-28, together the
    // upper diacritics; level pitches 30-34 and contours 35-40; the digits
    // 1-5 41-45, the hyphen 46, the raising pseudo-glyph 50; their raised
    // forms 61-66. Linear classes: 0 dotless, 1 level numbers, 2 and 3 a
    // contour's first and second numbers ("133551", "515133"), 4 the
    // raise, 5 the hyphen, 6 the raised forms; lookup classes 7 dotted, 8
    // level, 9 contour, 10 what can be raised.
    let tones: Vec<u16> = (20..=24).collect();
    let modifiers: Vec<u16> = (25..=28).collect();
    let diacritics: Vec<u16> = (20..=28).collect();
    let pitch_number = vec![0x2B, 0, 0, 0x01, 1, 0x13, 0x30];
    let first = vec![
        // clsDottedI > clsDotlessI / _ clsUDia;
        RuleSpec {
            slots: vec![vec![10, 11, 12], diacritics.clone()],
            action: vec![0x1D, 0, 7, 0, 0x19, 0x31],
            ..RuleSpec::default()
        },
        // clsTone clsUMod > @2 @1;
        RuleSpec {
            slots: vec![tones, modifiers],
            action: vec![
                0x1E, 1, 0x21, 1, 1, 0x19, 0x1E, 0xFF, 0x21, 1, 0xFF, 0x19, 0x31,
            ],
            ..RuleSpec::default()
        },
        // if (fPitchNum == numbers) cls1Pitch _ > cls1Num gRaise;
        RuleSpec {
            slots: vec![(30..=34).collect()],
            constraint: pitch_number.clone(),
            action: vec![0x1D, 0, 8, 1, 0x19, 0x1F, 0x1C, 4, 0x19, 0x31],
            ..RuleSpec::default()
        },
        // cls2Pitch _ _ _ _ _ > cls2Num1 gRaise C("-") gRaise cls2Num2$1 gRaise;
        RuleSpec {
            slots: vec![(35..=40).collect()],
            constraint: pitch_number,
            action: vec![
                0x1D, 0, 9, 2, 0x19, 0x1F, 0x1C, 4, 0x19, 0x1F, 0x1C, 5, 0x19, 0x1F, 0x1C, 4, 0x19,
                0x1F, 0x1D, 0, 9, 3, 0x19, 0x1F, 0x1C, 4, 0x19, 0x31,
            ],
            ..RuleSpec::default()
        },
    ];
    // clsRaise gRaise > clsRaised _;
    let second = vec![RuleSpec {
        slots: vec![(41..=46).collect(), vec![50]],
        action: vec![0x1D, 0, 10, 6, 0x19, 0x20, 0x19, 0x31],
        ..RuleSpec::default()
    }];
    let linear = [
        vec![13, 14, 15],
        vec![41, 42, 43, 44, 45],
        vec![41, 43, 43, 45, 45, 41],
        vec![45, 41, 45, 41, 43, 43],
        vec![50],
        vec![46],
        vec![61, 62, 63, 64, 65, 66],
    ];
    let lookups = [
        vec![(10, 0), (11, 1), (12, 2)],
        (30..=34).zip(0..).collect::<Vec<(u16, u16)>>(),
        (35..=40).zip(0..).collect::<Vec<(u16, u16)>>(),
        (41..=46).zip(0..).collect::<Vec<(u16, u16)>>(),
    ];
    let description = passing_description(
        vec![compile(&first, 70, 5, 0), compile(&second, 70, 5, 0)],
        &linear,
        &lookups,
        4,
    );
    let feat = feat_table(&[(64_000, 0x8000, &[0, 1])]);
    let face = passing_face(70, 4, description, &[], Some(feat));
    let numbers: &[i16] = &[1];
    let letters: &[i16] = &[0];
    // The dotless i: before an upper diacritic, and not before anything else.
    assert_eq!(substituted(&face, &[10, 20], false, None).0, vec![13, 20]);
    assert_eq!(substituted(&face, &[11, 41], false, None).0, vec![11, 41]);
    held += 1;
    // A tone and a modifier exchanged, one cluster.
    let (glyphs, clusters, _) = substituted(&face, &[21, 26], false, None);
    assert_eq!((glyphs, clusters), (vec![26, 21], vec![0, 0]));
    held += 1;
    // Pitch letters as numbers, under the feature only: the first pass
    // alone, a level pitch one number and a contour three, each marked.
    let first_pass = |glyphs: &[u16], features: &[i16]| {
        passing(&face, glyphs, Some(features), |slots| {
            let limits = Limits::DEFAULT;
            let mut budget = Budget::new(&limits);
            let mut refusals = Vec::new();
            pass::run_passes(slots, 0..1, &limits, &mut budget, &mut refusals);
            assert!(refusals.is_empty(), "{refusals:?}");
            slots.glyphs()
        })
    };
    assert_eq!(first_pass(&[30], numbers), vec![41, 50]);
    assert_eq!(first_pass(&[36], numbers), vec![43, 50, 46, 50, 41, 50]);
    assert_eq!(first_pass(&[36], letters), vec![36]);
    held += 1;
    // Raised glyphs: the second pass makes each marked one its raised form,
    // the mark gone, all of them the pitch letter's one character.
    let (glyphs, clusters, refusals) = substituted(&face, &[36], false, Some(numbers));
    assert_eq!(glyphs, vec![63, 66, 61]);
    assert_eq!(clusters, vec![0, 0, 0]);
    assert!(refusals.is_empty(), "{refusals:?}");
    assert_eq!(substituted(&face, &[30], false, Some(numbers)).0, vec![61]);
    held += 1;

    // 6.4.1: the chain of A alternating A1 and A2, its last A2, found by
    // running to the chain's end and backing up over it, `MaxRuleLoop = 20;
    // MaxBackup = 10`, `Alt` user attribute 0. A 1, A1 2, A2 3; 9 any other.
    let mark =
        |value: u8, back: u8| vec![0x1E, 0, 0x01, value, 0x33, 55, 0, 0x19, 0x01, back, 0x30];
    let both = |first: u8, second: u8| {
        let mut code = alt_is(0, first);
        code.extend(alt_is(1, second));
        code.extend([0x10, 0x30]);
        code
    };
    let single = |offset: u8, value: u8| {
        let mut code = alt_is(offset, value);
        code.push(0x30);
        code
    };
    let chain = [
        // gA > @ / _ gA {Alt == 0};
        RuleSpec {
            slots: vec![vec![1], vec![1]],
            constraint: single(1, 0),
            action: vec![0x1E, 0, 0x19, 0x31],
            ..RuleSpec::default()
        },
        // gA > @ {Alt = 2} / ^ ANY _ {Alt == 0};
        RuleSpec {
            pre: 1,
            slots: vec![vec![], vec![1]],
            constraint: single(0, 0),
            action: mark(2, 0xFE),
            ..RuleSpec::default()
        },
        // gA > @ {Alt = 2} / ^ ANY _ {Alt == 0} gA {Alt == 1};
        RuleSpec {
            pre: 1,
            slots: vec![vec![], vec![1], vec![1]],
            constraint: both(0, 1),
            action: mark(2, 0xFE),
            ..RuleSpec::default()
        },
        // gA > @ {Alt = 1} / ^ ANY _ {Alt == 0} gA {Alt == 2};
        RuleSpec {
            pre: 1,
            slots: vec![vec![], vec![1], vec![1]],
            constraint: both(0, 2),
            action: mark(1, 0xFE),
            ..RuleSpec::default()
        },
        // gA > @ {Alt = 2} / ^ _ {Alt == 0} gA {Alt == 1};
        RuleSpec {
            slots: vec![vec![1], vec![1]],
            constraint: both(0, 1),
            action: mark(2, 0xFF),
            ..RuleSpec::default()
        },
        // gA > @ {Alt = 1} / ^ _ {Alt == 0} gA {Alt == 2};
        RuleSpec {
            slots: vec![vec![1], vec![1]],
            constraint: both(0, 2),
            action: mark(1, 0xFF),
            ..RuleSpec::default()
        },
        // gA > gA1 / _ {Alt == 1};
        RuleSpec {
            slots: vec![vec![1]],
            constraint: single(0, 1),
            action: vec![0x1C, 0, 0x19, 0x31],
            ..RuleSpec::default()
        },
        // gA > gA2 / _ {Alt == 2};
        RuleSpec {
            slots: vec![vec![1]],
            constraint: single(0, 2),
            action: vec![0x1C, 1, 0x19, 0x31],
            ..RuleSpec::default()
        },
    ];
    let face = one_pass(&chain, 10, &[vec![2], vec![3]], 20, 10);
    assert_eq!(
        substituted(&face, &[9, 1, 1, 1, 1], false, None).0,
        vec![9, 2, 3, 2, 3]
    );
    assert_eq!(
        substituted(&face, &[9, 1, 1, 1], false, None).0,
        vec![9, 3, 2, 3]
    );
    assert_eq!(
        substituted(&face, &[1, 1, 1, 1], false, None).0,
        vec![2, 3, 2, 3]
    );
    // Under `MaxBackup = 0` the chain cannot be backed over: the position
    // is left where it stood, and only the last A changes.
    let stuck = one_pass(&chain, 10, &[vec![2], vec![3]], 20, 0);
    assert_eq!(
        substituted(&stuck, &[1, 1, 1, 1], false, None).0,
        vec![1, 1, 1, 3]
    );
    held += 1;

    assert_eq!(held, 9, "the substitution examples the design counts");
}

/// A rule whose action leaves the pointer where it found it fires
/// `maxRuleLoop` times at each position, and then the position is moved on;
/// a `maxRuleLoop` of 0 lets it fire once; and a pass that would fire past
/// the run's budget stops, named once.
#[test]
fn a_rule_that_never_moves_the_pointer_stops_at_the_loop_ceiling() {
    // A user attribute counts the firings: `A {user1 += 1}`, returning 0
    // with no `Next`.
    let rule = RuleSpec {
        slots: vec![vec![1]],
        action: vec![0x01, 1, 0x34, 55, 0, 0x31],
        ..RuleSpec::default()
    };
    let counts = |loops: u8, limits: &Limits| {
        let pass = compile(std::slice::from_ref(&rule), 4, loops, 0);
        let description = passing_description(vec![pass], &[vec![2]], &[], 4);
        let face = passing_face(4, 4, description, &[], None);
        passing(&face, &[1, 1, 1], None, |slots| {
            let mut budget = Budget::new(limits);
            let refusals = pass::substitute(slots, false, limits, &mut budget);
            let fired: Vec<i32> = (0..slots.len())
                .map(|at| {
                    slots
                        .slot_attribute(at, SlotAttribute::User, 0)
                        .expect("answers")
                })
                .collect();
            (fired, refusals)
        })
    };
    assert_eq!(counts(3, &Limits::DEFAULT), (vec![3, 3, 3], vec![]));
    assert_eq!(counts(0, &Limits::DEFAULT), (vec![1, 1, 1], vec![]));
    assert_eq!(counts(255, &Limits::DEFAULT), (vec![255, 255, 255], vec![]));
    // Three instructions a firing: a budget of thirty stops the pass part
    // way through its first slot, and says so once.
    let tight = Limits {
        max_instructions: 30,
        ..Limits::DEFAULT
    };
    let (fired, refusals) = counts(255, &tight);
    assert_eq!(fired, vec![10, 0, 0]);
    assert_eq!(refusals, vec![Refusal::TooManyInstructions]);
}

/// What decides whether a pass and its rules run: a pass constraint that is
/// false skips the pass, and one refused is named and skips it; a rule
/// whose constraint or action fails its check never fires, and the next
/// candidate is tried; an action refused as it runs is named, what it did
/// stands, and the position moves on.
#[test]
fn a_pass_runs_only_what_its_code_allows() {
    let ligature = |action: Vec<u8>, constraint: Vec<u8>| RuleSpec {
        slots: vec![vec![1], vec![2]],
        constraint,
        action,
        ..RuleSpec::default()
    };
    let join = vec![0x1C, 0, 0x21, 2, 0, 1, 0x19, 0x20, 0x19, 0x31];
    let face_of = |rules: &[RuleSpec], pass_constraint: Vec<u8>| {
        let mut pass = compile(rules, 8, 5, 0);
        pass.pass_constraint = pass_constraint;
        let description = passing_description(vec![pass], &[vec![3], vec![4]], &[], 4);
        passing_face(
            8,
            4,
            description,
            &[],
            Some(feat_table(&[(7, 0x8000, &[0, 1])])),
        )
    };
    let run = |face: &[u8], features: &[i16]| substituted(face, &[1, 2], false, Some(features));
    // The pass constraint `feature 0 == 1`: false skips the pass.
    let gated = face_of(
        &[ligature(join.clone(), vec![])],
        vec![0x2B, 0, 0, 0x01, 1, 0x13, 0x30],
    );
    assert_eq!(run(&gated, &[1]).0, vec![3]);
    assert_eq!(run(&gated, &[0]).0, vec![1, 2]);
    // A pass constraint that fails its check skips the pass, named.
    let broken = face_of(&[ligature(join.clone(), vec![])], vec![0x43]);
    let (glyphs, _, refusals) = run(&broken, &[1]);
    assert_eq!(glyphs, vec![1, 2]);
    assert_eq!(
        refusals,
        vec![Refusal::CodeRefused {
            pass: 0,
            block: Block::PassConstraint,
            at: 0,
            fault: Fault::IllegalOpcode(0x43),
        }]
    );
    // The longer rule's constraint fails its check: it never fires, and the
    // shorter rule after it does.
    let mut rules = vec![ligature(join.clone(), vec![0x06, 0x30])];
    rules.push(RuleSpec {
        slots: vec![vec![1]],
        action: vec![0x1C, 1, 0x19, 0x31],
        ..RuleSpec::default()
    });
    let (glyphs, _, refusals) = run(&face_of(&rules, vec![]), &[0]);
    assert_eq!(glyphs, vec![4, 2]);
    assert!(matches!(
        refusals.as_slice(),
        [Refusal::CodeRefused {
            block: Block::Constraint(0),
            fault: Fault::StackUnderflow,
            ..
        }]
    ));
    // An action that fails its check, likewise.
    rules[0] = ligature(vec![0x1A, 1], vec![]);
    let (glyphs, _, refusals) = run(&face_of(&rules, vec![]), &[0]);
    assert_eq!(glyphs, vec![4, 2]);
    assert!(matches!(
        refusals.as_slice(),
        [Refusal::CodeRefused {
            block: Block::Action(0),
            fault: Fault::Unimplemented(0x1A),
            ..
        }]
    ));
    // An action refused as it runs: its `PutGlyph` stands, it is named, and
    // the position moves on.
    rules[0] = ligature(vec![0x1C, 0, 0x21, 1, 9, 0x31], vec![]);
    let (glyphs, _, refusals) = run(&face_of(&rules, vec![]), &[0]);
    assert_eq!(glyphs, vec![3, 2]);
    assert!(matches!(
        refusals.as_slice(),
        [Refusal::CodeRefused {
            block: Block::Action(0),
            fault: Fault::SlotOutOfRange,
            ..
        }]
    ));
}

/// A run at an odd level is passed in logical order, as every run is, so a
/// rule matches the order the text is stored in; a reversed pass sees it
/// turned end to end and turns it back; and a description with a bidi pass
/// and a `Silf` of 4 or later mirrors each glyph of an odd-level run before
/// the pass it names, one without either mirroring nothing. The GDL's
/// examples, counted.
#[test]
fn a_right_to_left_run_is_passed_in_the_order_the_design_states() {
    let mut held = 0usize;
    // `A B > AB:(1 2) _`, a ligature a rule finds only in the order A, B.
    let ligature = |first: u16, second: u16| RuleSpec {
        slots: vec![vec![first], vec![second]],
        action: vec![0x1C, 0, 0x21, 2, 0, 1, 0x19, 0x20, 0x19, 0x31],
        ..RuleSpec::default()
    };
    let face_of = |rules: &[RuleSpec], flags: u8, joined: u16, bidi: u8, major: u32| {
        let mut pass = compile(rules, 32, 5, 0);
        pass.flags = flags;
        let mut description = passing_description(vec![pass], &[vec![joined]], &[], major);
        description.order[3] = bidi;
        // Glyph attribute 3, the description's mirroring attribute: the
        // parentheses 20 and 21 each other's mirror.
        let maps = vec![BTreeMap::new(); 20]
            .into_iter()
            .chain([BTreeMap::from([(3, 21)]), BTreeMap::from([(3, 20)])])
            .collect::<Vec<_>>();
        passing_face(32, major, description, &maps, None)
    };
    // A 1, B 2; AB 3, BA 4.
    let forward = face_of(&[ligature(1, 2)], 0, 3, 255, 4);
    assert_eq!(substituted(&forward, &[1, 2], true, None).0, vec![3]);
    assert_eq!(substituted(&forward, &[1, 2], false, None).0, vec![3]);
    // A reversed pass sees B, A: the rule for A, B does not match, the rule
    // for B, A does, and the stream is turned back after.
    let reversed = face_of(&[ligature(1, 2)], 0x20, 3, 255, 4);
    assert_eq!(substituted(&reversed, &[1, 2], true, None).0, vec![1, 2]);
    let backwards = face_of(&[ligature(2, 1)], 0x20, 4, 255, 4);
    let (glyphs, clusters, _) = substituted(&backwards, &[1, 2, 5], true, None);
    assert_eq!((glyphs, clusters), (vec![4, 5], vec![0, 2]));

    // GDL 4.1.4.2: "ABC 123 DEF" in a right-to-left system is three runs
    // here, at levels 1, 2 and 1, each passed in logical order — so each
    // run's rule over its stored order fires — and the consumer's per-line
    // reorder draws "FED 123 CBA". A, B, C 1-3; 1, 2, 3 6-8; D, E, F 11-13;
    // each run's ligature 30.
    let triple = |a: u16, b: u16, c: u16| RuleSpec {
        slots: vec![vec![a], vec![b], vec![c]],
        action: vec![
            0x1C, 0, 0x21, 3, 0, 1, 2, 0x19, 0x20, 0x19, 0x20, 0x19, 0x31,
        ],
        ..RuleSpec::default()
    };
    let runs = face_of(
        &[triple(1, 2, 3), triple(6, 7, 8), triple(11, 12, 13)],
        0,
        30,
        255,
        4,
    );
    for (glyphs, odd) in [([1, 2, 3], true), ([6, 7, 8], false), ([11, 12, 13], true)] {
        assert_eq!(
            substituted(&runs, &glyphs, odd, None).0,
            vec![30],
            "{glyphs:?}"
        );
    }
    held += 1;

    // GDL 6.6: a parenthesis at an odd level takes its mirror, before the
    // pass after the bidi pass; at an even level, without a bidi pass, and
    // in a `Silf` before 4, it does not.
    let mirrored = face_of(&[ligature(1, 2)], 0, 3, 0, 4);
    assert_eq!(
        substituted(&mirrored, &[20, 1, 2, 21], true, None).0,
        vec![21, 3, 20]
    );
    assert_eq!(
        substituted(&mirrored, &[20, 1, 2, 21], false, None).0,
        vec![20, 3, 21]
    );
    let no_bidi = face_of(&[ligature(1, 2)], 0, 3, 255, 4);
    assert_eq!(
        substituted(&no_bidi, &[20, 1, 2, 21], true, None).0,
        vec![20, 3, 21]
    );
    let early = face_of(&[ligature(1, 2)], 0, 3, 0, 3);
    assert_eq!(
        substituted(&early, &[20, 1, 2, 21], true, None).0,
        vec![20, 3, 21]
    );
    // The bidi pass placed past the substitution passes mirrors at the
    // stage's end.
    let late = face_of(&[ligature(1, 2)], 0, 3, 10, 4);
    assert_eq!(
        substituted(&late, &[20, 1, 2, 21], true, None).0,
        vec![21, 3, 20]
    );
    held += 1;

    assert_eq!(held, 2, "the bidi examples the design counts");
}

/// The vendored Dai Banna SIL's two passes run whole: its line-break pass
/// gives the consonant after a prevowel its break weight, and its
/// substitution pass composes `e` and the acute's pseudo-glyph into `é`.
#[test]
fn the_vendored_faces_passes_run() {
    let sfnt = Sfnt::parse(DAI_BANNA).expect("Dai Banna SIL parses");
    let glyph = |c: char| sfnt.glyph_for_char(c).expect("a glyph");
    let (e, composed) = (glyph('e'), glyph('\u{E9}'));
    let (prevowel, consonant) = (glyph('\u{19B6}'), glyph('\u{1982}'));
    passing(DAI_BANNA, &[e, 413, prevowel, consonant], None, |slots| {
        let limits = Limits::DEFAULT;
        let mut budget = Budget::new(&limits);
        let refusals = pass::substitute(slots, false, &limits, &mut budget);
        assert!(refusals.is_empty(), "{refusals:?}");
        assert_eq!(slots.glyphs(), vec![composed, prevowel, consonant]);
        assert_eq!(slots.clusters(), vec![0, 2, 3]);
        assert_eq!(
            slots.slot_attribute(2, SlotAttribute::BreakWeight, 0),
            Ok(-30)
        );
    });
}
