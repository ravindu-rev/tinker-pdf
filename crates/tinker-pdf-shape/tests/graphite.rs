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

use tinker_pdf_font::Sfnt;
use tinker_pdf_shape::bidi::{BaseDirection, Level};
use tinker_pdf_shape::shape::Run;
use tinker_pdf_shape::unicode::Script;
use tinker_pdf_shape::{Orientation, ShapedGlyph, ShapedRun, Shaper, Tag, Warning};

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
