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
    aat_setting, AatSetting, Cursor, Entry, Feat, FeatureEntry, Kerx, Lookup, LookupError,
    LookupFormat, Morx, Selection, Site, StateError, StateTable, Traversal, ANY_ORIENTATION,
    COMPONENT_STACK, CONTEXTUAL, CROSS_STREAM, DELETED, DELETED_GLYPH, DESCENDING, DONT_ADVANCE,
    END_OF_TEXT, INSERTION, KERNING_STACK, KERX_VARIATION, KERX_VERTICAL, LIGATURE, LOGICAL,
    NONCONTEXTUAL, OUT_OF_BOUNDS, PROCESS_DIRECTION, REARRANGEMENT, VERTICAL,
};
use tinker_pdf_shape::bidi::{BaseDirection, Level};
use tinker_pdf_shape::shape::Run;
use tinker_pdf_shape::unicode::Script;
use tinker_pdf_shape::{
    Buffer, Direction, Insertion, Limits, Orientation, ShapedGlyph, ShapedRun, Shaper, Tag, Warning,
};

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
type FeatureRow = (u16, u16, u32, u32);

/// One `morx` chain: its default flags, its feature entries in table order,
/// and its subtables.
fn chain(default_flags: u32, features: &[FeatureRow], subtables: &[Vec<u8>]) -> Vec<u8> {
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

// --- the face, and the path it takes (FT-07c.2, FT-07c.10) ----------------
//
// A face whose layout is AAT's is shaped through its `morx` chain, under the
// design's precedence: a face carrying a readable `GSUB` or `GPOS` takes the
// OpenType path whatever AAT tables it carries, and a face whose AAT table is
// one this crate does not run — a `mort`, or a `morx` it cannot read — is
// shaped by its `cmap` and `hmtx` alone, every run saying so once:
// `Warning::TableNotRun` (ruling 10; ruling 3's refusal by name).

/// The ligature of the AAT face's `f` and `i`.
const FI: u16 = 4;
/// The swash `x` the AAT face's chain maps its `x` to.
const SWASH_X: u16 = 7;
/// The alternate the chain maps the face's alef to.
const ALEF_ALT: u16 = 9;
/// The face's Han ideograph, and the vertical form its chain maps it to in a
/// column.
const HAN: u16 = 10;
/// See [`HAN`].
const VERTICAL_HAN: u16 = 11;

/// The same subtable under sub-feature flags `flags`.
fn under_flags(mut subtable: Vec<u8>, flags: u32) -> Vec<u8> {
    subtable[8..12].copy_from_slice(&flags.to_be_bytes());
    subtable
}

/// A ligature machine forming `fi` of `f` and `i`, glyphs 1 and 2.
fn fi_machine() -> Vec<u8> {
    ligature_subtable(
        &[(1, 4), (2, 5)],
        &[&[0, 0, 0, 0, 1, 0], &[0, 0, 0, 0, 1, 2]],
        &[[0, 0, 0], [1, 0x8000, 0], [0, 0xA000, 0]],
        &[0, LAST_ACTION],
        &[0; 3],
        &[FI],
    )
}

/// A rearrangement machine swapping `β` and an `α` after it, glyphs 6 and 5.
fn beta_alpha_machine() -> Vec<u8> {
    rearrangement_subtable(
        &[(6, 4), (5, 5)],
        &[&[0, 0, 0, 0, 1, 0], &[0, 0, 0, 0, 1, 2]],
        &[(0, 0), (1, 0x8000), (0, 0x2001)],
    )
}

/// The AAT face's chain: by default, flags 1 to 8 — its `x` swashed and its
/// alef made an alternate, `f` and `i` ligated, `βα` reordered, and in a
/// column its Han ideograph made the vertical form — with the Ligatures
/// feature's two settings, on and off, turning the ligature's flag on and
/// off.
fn aat_chain() -> Vec<u8> {
    morx_table(
        2,
        &[chain(
            0b1111,
            &[(1, 2, 0b0010, !0b0010), (1, 3, 0, !0b0010)],
            &[
                noncontextual(1, &[(3, SWASH_X), (8, ALEF_ALT)]),
                under_flags(fi_machine(), 2),
                under_flags(beta_alpha_machine(), 4),
                noncontextual_with(VERTICAL, 8, &lookup_single(&[(HAN, VERTICAL_HAN)])),
            ],
        )],
    )
}

/// A face of thirteen glyphs — `.notdef`, `f`, `i`, `x`, the `fi`, `α`, `β`,
/// the swash `x`, alef, its alternate, a Han ideograph, its vertical form, and
/// bet — carrying the chain [`aat_chain`] and no OpenType Layout table.
fn aat_face() -> Face {
    let face = Face::new(
        13,
        &[
            ('f', 1),
            ('i', 2),
            ('x', 3),
            ('α', 5),
            ('β', 6),
            ('\u{5D0}', 8),
            ('\u{5D1}', 12),
            ('\u{6F22}', HAN),
        ],
    );
    face.with(b"morx", aat_chain())
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

/// The glyph indices of `runs`, in order.
fn indices(runs: &[ShapedRun]) -> Vec<u16> {
    glyphs(runs).iter().map(|g| g.glyph).collect()
}

/// FT-07c.2's face given a chain: a face carrying `morx` and no `GSUB` or
/// `GPOS` is shaped through it — its `f` and `i` ligated, its `x` swashed,
/// its `βα` reordered — each glyph's advance its own `hmtx` advance, filled
/// after the chain, the ligature taking its first component's cluster and a
/// reordered pair's clusters merged; and no run names anything.
#[test]
fn a_face_carrying_only_morx_is_shaped_through_its_chain() {
    let face = aat_face();
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let shaper = Shaper::new(&sfnt);

    let runs = line_runs(&shaper, "fix");
    assert_eq!(runs.len(), 1, "one Latin run");
    assert_eq!(
        glyphs(&runs),
        vec![face.plain(FI, 0), face.plain(SWASH_X, 2)],
        "ligated and swashed, each advance the glyph's own"
    );
    assert!(runs[0].warnings().is_empty(), "{:?}", runs[0].warnings());

    let runs = line_runs(&shaper, "βα");
    assert_eq!(glyphs(&runs), vec![face.plain(5, 0), face.plain(6, 0)]);
    assert!(runs.iter().all(|run| run.warnings().is_empty()));

    // The same face with no `morx` is the `cmap`-only run: what the chain
    // changed.
    let plain_face = face.clone().without(b"morx");
    let plain_bytes = plain_face.build();
    let plain_sfnt = Sfnt::parse(&plain_bytes).expect("the plain face parses");
    let plain = line_runs(&Shaper::new(&plain_sfnt), "fix");
    let expected: Vec<ShapedGlyph> = "fix"
        .chars()
        .zip(clusters("fix"))
        .map(|(c, cluster)| face.plain(face.glyph(c), cluster))
        .collect();
    assert_eq!(glyphs(&plain), expected);
    assert!(plain.iter().all(|run| run.warnings().is_empty()));
}

/// Every run is shaped through the chain its caller asks for: from the
/// chain's defaults; from none where `with_features` names nothing; from
/// none with the named tags' settings asked; and from the defaults with
/// `with_settings`' tags asked over them. A tag no AAT feature answers is
/// named on each run, `kern` is no `morx` feature and named for nothing, and
/// a run of no glyphs is shaped to none, naming nothing.
#[test]
fn every_run_over_a_morx_face_takes_the_chain_its_caller_asks_for() {
    let face = aat_face();
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let none: [Tag; 0] = [];
    let liga = [Tag::new(b"liga")];
    let positioning = [Tag::new(b"kern"), Tag::new(b"mark")];
    let off = [(Tag::new(b"liga"), 0u32), (Tag::new(b"smcp"), 1)];
    let unknown = [(Tag::new(b"mark"), 1u32)];
    let named_mark = Warning::FeatureNotInAat {
        tag: Tag::new(b"mark"),
    };
    let (f, i, x, alpha, beta) = (1u16, 2u16, 3u16, 5u16, 6u16);
    let cases: Vec<(Shaper<'_>, Vec<u16>, Vec<u16>, Vec<Warning>)> = vec![
        (
            Shaper::new(&sfnt),
            vec![FI, SWASH_X],
            vec![alpha, beta],
            vec![],
        ),
        (
            Shaper::new(&sfnt).with_features(&none, &none),
            vec![f, i, x],
            vec![beta, alpha],
            vec![],
        ),
        (
            Shaper::new(&sfnt).with_features(&liga, &none),
            vec![FI, x],
            vec![beta, alpha],
            vec![],
        ),
        (
            Shaper::new(&sfnt).with_features(&liga, &positioning),
            vec![FI, x],
            vec![beta, alpha],
            vec![named_mark],
        ),
        (
            Shaper::new(&sfnt).with_settings(&off),
            vec![f, i, SWASH_X],
            vec![alpha, beta],
            vec![],
        ),
        (
            Shaper::new(&sfnt).with_settings(&unknown),
            vec![FI, SWASH_X],
            vec![alpha, beta],
            vec![named_mark],
        ),
    ];
    for (shaper, latin, greek, warned) in &cases {
        let runs = line_runs(shaper, "fix");
        assert_eq!(indices(&runs), *latin);
        let greek_runs = line_runs(shaper, "βα");
        assert_eq!(indices(&greek_runs), *greek);
        for run in runs.iter().chain(&greek_runs) {
            assert_eq!(run.warnings(), warned.as_slice(), "run {:?}", run.text());
        }
    }
    let empty = Run {
        text: 2..2,
        script: Script::Latin,
        level: Level::LTR,
        orientation: Orientation::Horizontal,
    };
    let shaped = Shaper::new(&sfnt).shape("fix", &empty);
    assert!(shaped.glyphs().is_empty());
    assert!(shaped.warnings().is_empty());
}

/// A right-to-left run is shaped through the chain and comes back in logical
/// order, its clusters where they were; in a column the sideways run takes
/// the subtables for horizontal text and the upright one the subtable for
/// vertical text, its only warning the column's own.
#[test]
fn a_right_to_left_run_and_a_column_take_the_chain_too() {
    let face = aat_face();
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let (_, runs) = Shaper::new(&sfnt).shape_text("\u{5D0}\u{5D1}", BaseDirection::Auto);
    assert_eq!(runs.len(), 1);
    assert!(!runs[0].direction().is_forward(), "a right-to-left run");
    assert_eq!(
        glyphs(&runs),
        vec![face.plain(ALEF_ALT, 0), face.plain(12, 2)]
    );
    assert!(runs[0].warnings().is_empty(), "{:?}", runs[0].warnings());

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
    assert_eq!(indices(&runs[..1]), vec![FI]);
    assert!(runs[0].warnings().is_empty(), "{:?}", runs[0].warnings());
    assert_eq!(indices(&runs[1..]), vec![VERTICAL_HAN]);
    // The upright run's metrics were synthesized, the face having no
    // `vmtx`: that is all it says.
    assert_eq!(runs[1].warnings(), &[Warning::VerticalMetricsSynthesized]);
    // Along a line the Han ideograph is not the vertical form.
    let runs = line_runs(&Shaper::new(&sfnt), "\u{6F22}");
    assert_eq!(indices(&runs), vec![HAN]);
}

/// A face carrying `mort` alone — the table `morx` extends, a non-goal of
/// the design — is shaped by its `cmap` and `hmtx` alone and named by it; a
/// face carrying both is shaped through its `morx` and named for neither,
/// the manual having a face with both ignore its `mort`.
#[test]
fn a_mort_only_face_is_named() {
    // A `mort` header: version 1.0 as a `Fixed`, and no chains.
    let mort = [0u8, 1, 0, 0, 0, 0, 0, 0].to_vec();
    let face = aat_face().without(b"morx").with(b"mort", mort.clone());
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let runs = line_runs(&Shaper::new(&sfnt), "fix");
    assert_eq!(runs[0].warnings(), &[not_run(b"mort")]);
    assert_eq!(indices(&runs), vec![1, 2, 3]);

    let both = aat_face().with(b"mort", mort);
    let bytes = both.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let runs = line_runs(&Shaper::new(&sfnt), "fix");
    assert!(runs[0].warnings().is_empty(), "mort ignored");
    assert_eq!(indices(&runs), vec![FI, SWASH_X]);
}

/// A `morx` this crate cannot read — a version other than 2 or 3, or a
/// header cut short — is no chain to shape through: the face is shaped by
/// its `cmap` and `hmtx` alone and every run names the table once.
#[test]
fn a_morx_this_crate_cannot_read_is_named_and_shaped_unshaped() {
    for unreadable in [
        [0u8, 1, 0, 0, 0, 0, 0, 0].to_vec(),
        [0u8, 4, 0, 0, 0, 0, 0, 0].to_vec(),
        [0u8, 2, 0].to_vec(),
    ] {
        let face = aat_face().with(b"morx", unreadable.clone());
        let bytes = face.build();
        let sfnt = Sfnt::parse(&bytes).expect("the face parses");
        let runs = line_runs(&Shaper::new(&sfnt), "fix βα");
        assert!(runs.len() >= 2);
        for run in &runs {
            assert_eq!(refusals(run.warnings(), b"morx"), 1, "{unreadable:?}");
            assert_eq!(run.warnings().len(), 1, "{:?}", run.warnings());
        }
        assert_eq!(
            indices(&runs[..1]),
            vec![1, 2, 3, 0],
            "unshaped, the space .notdef"
        );
    }
}

/// The precedence: a face carrying a readable `GSUB` or `GPOS` takes the
/// OpenType path, whatever AAT tables it carries beside them, its `morx`
/// unread and named for nothing; a face carrying neither readable takes its
/// chain.
#[test]
fn a_face_carrying_both_takes_the_path_the_design_states() {
    let kerning = || {
        layout_table(
            &[(*b"kern", vec![0])],
            &[lookup(2, &[pair_kern(1, 3, -40)])],
        )
    };
    let mort = [0u8, 1, 0, 0, 0, 0, 0, 0].to_vec();
    // `GSUB` beside the `morx`: its ligature, and no swash.
    let face = aat_face().with(b"GSUB", fi_ligature());
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let runs = line_runs(&Shaper::new(&sfnt), "fix");
    assert_eq!(indices(&runs), vec![FI, 3], "the GSUB's ligature, no swash");
    assert!(runs.iter().all(|run| run.warnings().is_empty()));
    // `βα` is not reordered: the chain is not run.
    let runs = line_runs(&Shaper::new(&sfnt), "βα");
    assert_eq!(indices(&runs), vec![6, 5]);

    // `GPOS` beside the `morx`: kerned, and neither ligated nor swashed —
    // a face with `GPOS` and no `GSUB` substitutes nothing.
    let face = aat_face().with(b"GPOS", kerning());
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let runs = line_runs(&Shaper::new(&sfnt), "fix");
    assert_eq!(indices(&runs), vec![1, 2, 3]);
    let runs = line_runs(&Shaper::new(&sfnt), "fx");
    let shaped = glyphs(&runs);
    assert_eq!(shaped[0].x_advance, i32::from(face.advances[1]) - 40);
    assert!(runs.iter().all(|run| run.warnings().is_empty()));

    // Both, with a `mort` and a `kerx` beside them: the OpenType path.
    let face = aat_face()
        .with(b"GSUB", fi_ligature())
        .with(b"GPOS", kerning())
        .with(b"mort", mort)
        .with(b"kerx", [0u8, 2, 0, 0, 0, 0, 0, 0].to_vec());
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let runs = line_runs(&Shaper::new(&sfnt), "fix");
    assert_eq!(indices(&runs), vec![FI, 3]);
    assert!(runs.iter().all(|run| run.warnings().is_empty()));

    // Neither: the chain.
    let face = aat_face();
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    assert_eq!(
        indices(&line_runs(&Shaper::new(&sfnt), "fix")),
        vec![FI, SWASH_X]
    );
}

/// A `GSUB` or `GPOS` this crate cannot read is no OpenType path to take:
/// the face's only readable layout table is its `morx`, and it is shaped
/// through it, naming nothing.
#[test]
fn an_unreadable_opentype_table_leaves_the_face_to_its_chain() {
    for tag in [b"GSUB", b"GPOS"] {
        let mut unreadable = fi_ligature();
        unreadable[0..2].copy_from_slice(&2u16.to_be_bytes());
        let face = aat_face().with(tag, unreadable);
        let bytes = face.build();
        let sfnt = Sfnt::parse(&bytes).expect("the face parses");
        let runs = line_runs(&Shaper::new(&sfnt), "fix");
        assert_eq!(
            indices(&runs),
            vec![FI, SWASH_X],
            "the chain beside an unreadable {tag:?}"
        );
        assert!(runs.iter().all(|run| run.warnings().is_empty()));
    }
}

/// A `Silf` beside a `morx` leaves the face AAT's, shaped through its chain
/// and named for neither table; a `Silf` beside a `mort` is the Graphite
/// description that names the face, and the `mort` is not named beside it.
#[test]
fn a_silf_beside_an_aat_table_leaves_one_path() {
    // `Silf` 4.0 with no descriptions: present, which is all the precedence
    // reads.
    let mut silf = 0x0004_0000u32.to_be_bytes().to_vec();
    silf.extend(0x0004_0002u32.to_be_bytes());
    silf.extend(u16s(&[0, 0]));

    let face = aat_face().with(b"Silf", silf.clone());
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let runs = line_runs(&Shaper::new(&sfnt), "fix");
    assert!(runs[0].warnings().is_empty(), "{:?}", runs[0].warnings());
    assert_eq!(indices(&runs), vec![FI, SWASH_X]);

    let face = aat_face()
        .without(b"morx")
        .with(b"mort", [0u8, 1, 0, 0, 0, 0, 0, 0].to_vec())
        .with(b"Silf", silf);
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let runs = line_runs(&Shaper::new(&sfnt), "fix");
    assert_eq!(runs[0].warnings(), &[not_run(b"Silf")]);
}

/// The AAT path maps a run's text as typed: a Brahmic character the
/// OpenType path decomposes for the cluster model reaches the chain whole.
#[test]
fn a_face_shaped_through_its_chain_maps_the_text_as_typed() {
    // U+1B13 BALINESE LETTER KA, and U+1B40 BALINESE VOWEL SIGN TALING
    // TEDUNG, which decomposes to U+1B3E and U+1B35.
    let face = Face::new(
        5,
        &[
            ('\u{1B13}', 4),
            ('\u{1B40}', 1),
            ('\u{1B3E}', 2),
            ('\u{1B35}', 3),
        ],
    );
    let text = "\u{1B13}\u{1B40}";
    let plain_bytes = face.build();
    let plain = Sfnt::parse(&plain_bytes).expect("the face parses");
    assert_eq!(
        indices(&line_runs(&Shaper::new(&plain), text)),
        vec![4, 2, 3],
        "decomposed where the face takes no chain"
    );
    let chained_face = face.with(b"morx", morx_table(2, &[]));
    let bytes = chained_face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let runs = line_runs(&Shaper::new(&sfnt), text);
    assert_eq!(indices(&runs), vec![4, 1], "as typed through the chain");
    assert!(runs.iter().all(|run| run.warnings().is_empty()));
}

/// A joiner is in the run while the chain runs — the ligature machine sees
/// it between the `f` and the `i`, and forms nothing — and leaves the run
/// after it, as it leaves after `GSUB`.
#[test]
fn a_joiner_is_seen_by_the_chain_and_leaves_the_run() {
    let face = aat_face();
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let runs = line_runs(&Shaper::new(&sfnt), "f\u{200D}i");
    assert_eq!(glyphs(&runs), vec![face.plain(1, 0), face.plain(2, 4)]);
    assert!(runs.iter().all(|run| run.warnings().is_empty()));
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

// --- the morx chain (FT-07c.5) -------------------------------------------
//
// Chains, their feature entries and flags, each subtable's coverage, the
// settings a caller's OpenType tags ask for, `feat`'s exclusive features,
// and the noncontextual subtable, the first type the chain runs.

/// A noncontextual subtable of `coverage`'s orientation bits under
/// sub-feature flags `flags`, its lookup table `lookup`.
fn noncontextual_with(coverage: u32, flags: u32, lookup: &[u8]) -> Vec<u8> {
    subtable(coverage | u32::from(NONCONTEXTUAL), flags, lookup)
}

/// A run of `glyphs` set `direction`, each glyph's cluster its position.
fn run_of(glyphs: &[u16], direction: Direction) -> Buffer {
    let mut buffer = Buffer::from_glyphs(glyphs);
    buffer.set_direction(direction);
    buffer
}

/// The glyphs `table`'s chains leave of `glyphs` under `selection` and
/// `limits`, run `direction`, and what they refused.
fn chained_under(
    table: &[u8],
    glyphs: &[u16],
    direction: Direction,
    selection: &Selection,
    limits: &Limits,
) -> (Vec<u16>, Vec<Warning>) {
    let morx = Morx::parse(table).expect("a readable morx");
    let mut buffer = run_of(glyphs, direction);
    let warnings = morx.run(&mut buffer, selection, limits);
    (buffer.glyphs().iter().map(|g| g.glyph).collect(), warnings)
}

/// [`chained_under`] the default limits.
fn chained(
    table: &[u8],
    glyphs: &[u16],
    direction: Direction,
    selection: &Selection,
) -> (Vec<u16>, Vec<Warning>) {
    chained_under(table, glyphs, direction, selection, &Limits::DEFAULT)
}

/// A selection asking for `settings` over the chains' defaults.
fn asking(settings: &[(u16, u16)]) -> Selection {
    Selection {
        start_at_defaults: true,
        settings: settings.to_vec(),
    }
}

/// The *Hierarchical Ligature Tables* and *Stand-Alone Ligature Tables*
/// examples of the `mort` chapter number no setting, naming them
/// "mandatory, normal, optional, and suppress"; the transcription numbers
/// them 0 to 3 of feature type 1.
const MANDATORY: (u16, u16) = (1, 0);
/// See [`MANDATORY`].
const NORMAL: (u16, u16) = (1, 1);
/// See [`MANDATORY`].
const OPTIONAL: (u16, u16) = (1, 2);
/// See [`MANDATORY`].
const SUPPRESS: (u16, u16) = (1, 3);

/// A chain of the four ligature settings' feature entries, `(enable,
/// disable)` each in the order the examples list them, over three
/// noncontextual subtables of the given sub-feature flags and glyph maps;
/// its default flags are 0, the examples stating none.
fn ligature_chain(entries: [(u32, u32); 4], subtables: [(u32, &[(u16, u16)]); 3]) -> Vec<u8> {
    let settings = [MANDATORY, NORMAL, OPTIONAL, SUPPRESS];
    let features: Vec<FeatureRow> = settings
        .iter()
        .zip(entries)
        .map(|((feature, setting), (enable, disable))| (*feature, *setting, enable, disable))
        .collect();
    let subtables: Vec<Vec<u8>> = subtables
        .iter()
        .map(|(flags, pairs)| noncontextual(*flags, pairs))
        .collect();
    morx_table(2, &[chain(0, &features, &subtables)])
}

/// A chain runs the subtables its flags select: from its default flags, a
/// requested feature entry's `(flags & disable) | enable` in table order,
/// later entries taking precedence; named features start from none. Held
/// to the `mort` chapter's two ligature-hierarchy examples' flags.
#[test]
fn a_chain_runs_the_subtables_its_flags_select() {
    // Two subtables: flag 1 maps glyph 10 to 20, flag 2 maps it to 30. The
    // default flags pick the first, and `liga`'s entry swaps them.
    let swap = morx_table(
        2,
        &[chain(
            1,
            &[(1, 2, 2, !1u32)],
            &[noncontextual(1, &[(10, 20)]), noncontextual(2, &[(10, 30)])],
        )],
    );
    let ltr = Direction::LeftToRight;
    assert_eq!(chained(&swap, &[10], ltr, &Selection::defaults()).0, [20]);
    assert_eq!(chained(&swap, &[10], ltr, &asking(&[(1, 2)])).0, [30]);
    let (liga, warnings) = Selection::from_opentype(None, &[(Tag::new(b"liga"), 1)], None);
    assert!(warnings.is_empty());
    assert_eq!(chained(&swap, &[10], ltr, &liga).0, [30]);
    // A caller who names its own features starts from no flag at all.
    let none = Selection {
        start_at_defaults: false,
        settings: Vec::new(),
    };
    assert_eq!(chained(&swap, &[10], ltr, &none).0, [10]);
    let named = Selection {
        start_at_defaults: false,
        settings: vec![(1, 2)],
    };
    assert_eq!(chained(&swap, &[10], ltr, &named).0, [30]);
    let morx = Morx::parse(&swap).expect("readable");
    let (chains, warnings) = morx.chains(&Limits::DEFAULT);
    assert!(warnings.is_empty());
    assert_eq!(chains[0].flags(&Selection::defaults()), 1);
    assert_eq!(chains[0].flags(&asking(&[(1, 2)])), 2);
    assert_eq!(chains[0].flags(&none), 0);

    // *Hierarchical Ligature Tables*: each setting enables its subtable and
    // the ones beneath it — mandatory `fi`, normal `æ`, optional `Æ`, here
    // glyphs 1, 2 and 3 made 11, 12 and 13.
    let hierarchical = ligature_chain(
        [
            (0x0000_0001, 0xFFFF_FFF9),
            (0x0000_0003, 0xFFFF_FFFB),
            (0x0000_0007, 0xFFFF_FFFF),
            (0x0000_0000, 0xFFFF_FFF8),
        ],
        [(1, &[(1, 11)]), (2, &[(2, 12)]), (4, &[(3, 13)])],
    );
    let glyphs = [1u16, 2, 3];
    for (asked, expected) in [
        (vec![MANDATORY], [11, 2, 3]),
        (vec![NORMAL], [11, 12, 3]),
        (vec![OPTIONAL], [11, 12, 13]),
        (vec![SUPPRESS], [1, 2, 3]),
        (vec![], [1, 2, 3]),
        // Later entries take precedence: optional after mandatory enables
        // everything, suppress after optional nothing.
        (vec![OPTIONAL, MANDATORY], [11, 12, 13]),
        (vec![SUPPRESS, OPTIONAL], [1, 2, 3]),
    ] {
        assert_eq!(
            chained(&hierarchical, &glyphs, ltr, &asking(&asked)).0,
            expected,
            "hierarchical, {asked:?}"
        );
    }

    // *Stand-Alone Ligature Tables*: each setting enables one subtable, each
    // holding every ligature of the settings beneath it, so "at most one
    // subtable is applied", the one of the setting with the highest
    // precedence.
    let stand_alone_entries = [
        (0x0000_0001, 0xFFFF_FFF9),
        (0x0000_0002, 0xFFFF_FFFA),
        (0x0000_0004, 0xFFFF_FFFC),
        (0x0000_0000, 0xFFFF_FFF8),
    ];
    let stand_alone = ligature_chain(
        stand_alone_entries,
        [
            (1, &[(1, 11)]),
            (2, &[(1, 11), (2, 12)]),
            (4, &[(1, 11), (2, 12), (3, 13)]),
        ],
    );
    for (asked, expected) in [
        (vec![MANDATORY], [11, 2, 3]),
        (vec![NORMAL], [11, 12, 3]),
        (vec![OPTIONAL], [11, 12, 13]),
        (vec![SUPPRESS], [1, 2, 3]),
        (vec![MANDATORY, OPTIONAL], [11, 12, 13]),
        (vec![NORMAL, MANDATORY], [11, 12, 3]),
    ] {
        assert_eq!(
            chained(&stand_alone, &glyphs, ltr, &asking(&asked)).0,
            expected,
            "stand-alone, {asked:?}"
        );
    }
    let morx = Morx::parse(&stand_alone).expect("readable");
    let (chains, _) = morx.chains(&Limits::DEFAULT);
    for (asked, flags) in [(MANDATORY, 1), (NORMAL, 2), (OPTIONAL, 4), (SUPPRESS, 0)] {
        assert_eq!(chains[0].flags(&asking(&[asked])), flags, "{asked:?}");
    }

    // "You can also use this feature table with the subtables from the
    // example by changing their subFeatureFlags to 0x00000007, 0x00000006,
    // and 0x00000004": the hierarchical subtables under the stand-alone
    // entries behave as the hierarchy does.
    let mixed = ligature_chain(
        stand_alone_entries,
        [(7, &[(1, 11)]), (6, &[(2, 12)]), (4, &[(3, 13)])],
    );
    for (asked, expected) in [
        (vec![MANDATORY], [11, 2, 3]),
        (vec![NORMAL], [11, 12, 3]),
        (vec![OPTIONAL], [11, 12, 13]),
        (vec![SUPPRESS], [1, 2, 3]),
    ] {
        assert_eq!(
            chained(&mixed, &glyphs, ltr, &asking(&asked)).0,
            expected,
            "flags 7, 6 and 4, {asked:?}"
        );
    }

    // Chains run in order, each from its own defaults: the second chain sees
    // what the first made.
    let two = morx_table(
        2,
        &[
            chain(1, &[], &[noncontextual(1, &[(10, 20)])]),
            chain(2, &[], &[noncontextual(2, &[(20, 40)])]),
        ],
    );
    assert_eq!(
        chained(&two, &[10, 20], ltr, &Selection::defaults()).0,
        [40, 40]
    );
}

/// The *Feature Name Table Example* of the `feat` chapter, every byte at
/// offsets 0 to 87, with one correction: its number-spacing feature's
/// `nSettings`, printed 1, is read as the 2 the two settings it lists at 80
/// and 84 make.
fn feat_example() -> Vec<u8> {
    let mut out = 0x0001_0000u32.to_be_bytes().to_vec();
    out.extend(u16s(&[4, 0]));
    out.extend(0u32.to_be_bytes());
    // All typographic features (0): one setting at 60, non-exclusive.
    out.extend(u16s(&[0, 1]));
    out.extend(60u32.to_be_bytes());
    out.extend(u16s(&[0, 260]));
    // Ligatures (1): one setting at 64, non-exclusive.
    out.extend(u16s(&[1, 1]));
    out.extend(64u32.to_be_bytes());
    out.extend(u16s(&[0, 256]));
    // Letter case (3): three settings at 68, exclusive, the first the
    // default.
    out.extend(u16s(&[3, 3]));
    out.extend(68u32.to_be_bytes());
    out.extend(u16s(&[0x8000, 262]));
    // Number spacing (6): two settings at 80 — the correction — exclusive,
    // the second (proportional) the default.
    out.extend(u16s(&[6, 2]));
    out.extend(80u32.to_be_bytes());
    out.extend(u16s(&[0xC001, 258]));
    // The settings.
    out.extend(u16s(&[0, 261]));
    out.extend(u16s(&[2, 257]));
    out.extend(u16s(&[0, 268, 3, 264, 4, 265]));
    out.extend(u16s(&[0, 259, 1, 260]));
    assert_eq!(out.len(), 88, "the example's 88 bytes");
    out
}

/// The selection `settings`, by tag name, make over the faces' defaults.
fn ask(settings: &[(&[u8; 4], u32)], feat: Option<&Feat<'_>>) -> (Selection, Vec<Warning>) {
    let settings: Vec<(Tag, u32)> = settings
        .iter()
        .map(|(name, value)| (Tag::new(name), *value))
        .collect();
    Selection::from_opentype(None, &settings, feat)
}

/// An exclusive feature keeps the last setting asked of it, its siblings
/// not asked, and asks nothing when set to 0; a non-exclusive one asks for
/// each setting, and for its off-setting when set to 0. Exclusivity is
/// `feat`'s where the face lists the feature and the registry's otherwise;
/// held to the `feat` chapter's example.
#[test]
fn an_exclusive_feature_setting_turns_its_siblings_off() {
    let tag = |bytes: &[u8; 4]| Tag::new(bytes);

    // The registry's exclusive Number Spacing (6): the last asked wins.
    let (selection, warnings) = ask(&[(b"tnum", 1), (b"pnum", 1)], None);
    assert!(warnings.is_empty());
    assert!(selection.start_at_defaults);
    assert_eq!(selection.settings, vec![(6, 1)]);
    assert_eq!(
        ask(&[(b"pnum", 1), (b"tnum", 1)], None).0.settings,
        vec![(6, 0)]
    );
    // Set to 0, an exclusive feature asks nothing; its defaults stand.
    assert!(ask(&[(b"smcp", 0)], None).0.settings.is_empty());
    assert_eq!(
        ask(&[(b"pcap", 1), (b"smcp", 0)], None).0.settings,
        vec![(37, 2)]
    );
    // A tag set twice is its last setting.
    assert!(ask(&[(b"smcp", 1), (b"smcp", 0)], None)
        .0
        .settings
        .is_empty());
    // The registry's non-exclusive Ligatures (1): every setting asked, and
    // the off-setting, one past the on, for a tag set to 0.
    assert_eq!(
        ask(&[(b"liga", 1), (b"dlig", 1)], None).0.settings,
        vec![(1, 2), (1, 4)]
    );
    assert_eq!(
        ask(&[(b"liga", 0), (b"dlig", 1)], None).0.settings,
        vec![(1, 3), (1, 4)]
    );
    // Two tags of one setting ask it once.
    assert_eq!(
        ask(&[(b"vert", 1), (b"vrt2", 1)], None).0.settings,
        vec![(4, 0)]
    );

    // The `feat` example.
    let bytes = feat_example();
    let feat = Feat::parse(&bytes).expect("a readable feat");
    assert_eq!(feat.feature_count(), 4);
    assert_eq!(feat.exclusive(0), Some(false));
    assert_eq!(feat.exclusive(1), Some(false));
    assert_eq!(feat.exclusive(3), Some(true));
    assert_eq!(feat.exclusive(6), Some(true));
    assert_eq!(feat.exclusive(2), None, "not listed");
    let letter_case = feat.feature(3).expect("listed");
    assert_eq!(
        (0..4).map(|n| letter_case.setting(n)).collect::<Vec<_>>(),
        vec![Some((0, 268)), Some((3, 264)), Some((4, 265)), None]
    );
    assert_eq!(letter_case.default_index(), 0);
    assert_eq!(
        letter_case.default_setting(),
        Some(0),
        "upper and lower case"
    );
    let spacing = feat.feature(6).expect("listed");
    assert_eq!(spacing.setting_count, 2);
    assert_eq!(spacing.setting(0), Some((0, 259)));
    assert_eq!(spacing.setting(1), Some((1, 260)));
    assert_eq!(spacing.default_index(), 1);
    assert_eq!(spacing.default_setting(), Some(1), "proportional");
    assert_eq!(spacing.name_index, 258);
    assert_eq!(feat.feature(1).and_then(|f| f.setting(0)), Some((2, 257)));
    assert_eq!(feat.feature(0).and_then(|f| f.setting(0)), Some((0, 261)));
    let order: Vec<u16> = (0..4)
        .filter_map(|n| feat.feature_at(n))
        .map(|f| f.feature)
        .collect();
    assert_eq!(order, vec![0, 1, 3, 6]);

    // Under the example's `feat`, number spacing stays exclusive, and a
    // chain whose two subtables are its two settings runs the one asked
    // last and not its sibling.
    let spacing_chain = morx_table(
        2,
        &[chain(
            0,
            &[(6, 0, 1, !3u32), (6, 1, 2, !3u32)],
            &[noncontextual(1, &[(5, 50)]), noncontextual(2, &[(5, 60)])],
        )],
    );
    let (selection, _) = ask(&[(b"tnum", 1), (b"pnum", 1)], Some(&feat));
    assert_eq!(selection.settings, vec![(6, 1)]);
    let ltr = Direction::LeftToRight;
    assert_eq!(chained(&spacing_chain, &[5], ltr, &selection).0, [60]);
    let (selection, _) = ask(&[(b"pnum", 1), (b"tnum", 1)], Some(&feat));
    assert_eq!(chained(&spacing_chain, &[5], ltr, &selection).0, [50]);

    // A `feat` that marks a registry-exclusive feature non-exclusive asks
    // every setting, and one that marks a non-exclusive feature exclusive
    // keeps the last.
    let mut table = 0x0001_0000u32.to_be_bytes().to_vec();
    table.extend(u16s(&[2, 0]));
    table.extend(0u32.to_be_bytes());
    table.extend(u16s(&[1, 0]));
    table.extend(36u32.to_be_bytes());
    table.extend(u16s(&[0x8000, 300]));
    table.extend(u16s(&[6, 0]));
    table.extend(36u32.to_be_bytes());
    table.extend(u16s(&[0x0000, 301]));
    let flipped = Feat::parse(&table).expect("readable");
    assert_eq!(
        ask(&[(b"tnum", 1), (b"pnum", 1)], Some(&flipped))
            .0
            .settings,
        vec![(6, 0), (6, 1)]
    );
    assert_eq!(
        ask(&[(b"liga", 1), (b"dlig", 1)], Some(&flipped))
            .0
            .settings,
        vec![(1, 4)]
    );
    assert!(
        ask(&[(b"liga", 0)], Some(&flipped)).0.settings.is_empty(),
        "an exclusive feature set to 0 asks nothing"
    );

    // Named features start from no flag; each asks its on-setting, and a
    // setting over a named tag replaces it.
    let named = [tag(b"liga"), tag(b"smcp")];
    let (selection, warnings) = Selection::from_opentype(Some(&named), &[], None);
    assert!(warnings.is_empty());
    assert!(!selection.start_at_defaults);
    assert_eq!(selection.settings, vec![(1, 2), (37, 1)]);
    let (selection, _) = Selection::from_opentype(Some(&named), &[(tag(b"liga"), 0)], None);
    assert_eq!(selection.settings, vec![(1, 3), (37, 1)]);
    let (selection, _) = Selection::from_opentype(Some(&[]), &[], None);
    assert!(!selection.start_at_defaults);
    assert!(selection.settings.is_empty());

    // A tag no AAT feature answers asks nothing and is named once; `kern`
    // is no `morx` feature and is not named.
    let (selection, warnings) = ask(
        &[
            (b"mark", 1),
            (b"mark", 0),
            (b"kern", 0),
            (b"xxxx", 1),
            (b"liga", 1),
        ],
        None,
    );
    assert_eq!(selection.settings, vec![(1, 2)]);
    assert_eq!(
        warnings,
        vec![
            Warning::FeatureNotInAat { tag: tag(b"mark") },
            Warning::FeatureNotInAat { tag: tag(b"xxxx") },
        ]
    );
}

/// Every row of the design's table, *The mapping*: each OpenType tag, the
/// AAT feature type and on-setting it asks for, and whether the registry
/// marks the feature exclusive.
#[test]
fn every_opentype_tag_of_the_mapping_asks_its_aat_feature() {
    let rows: &[(&[u8; 4], u16, u16, bool)] = &[
        (b"rlig", 1, 0, false),
        (b"liga", 1, 2, false),
        (b"dlig", 1, 4, false),
        (b"clig", 1, 18, false),
        (b"hlig", 1, 20, false),
        (b"calt", 36, 0, false),
        (b"swsh", 36, 2, false),
        (b"cswh", 36, 4, false),
        (b"smcp", 37, 1, true),
        (b"pcap", 37, 2, true),
        (b"c2sc", 38, 1, true),
        (b"c2pc", 38, 2, true),
        (b"onum", 21, 0, true),
        (b"lnum", 21, 1, true),
        (b"tnum", 6, 0, true),
        (b"pnum", 6, 1, true),
        (b"afrc", 11, 1, true),
        (b"frac", 11, 2, true),
        (b"sups", 10, 1, true),
        (b"subs", 10, 2, true),
        (b"ordn", 10, 3, true),
        (b"sinf", 10, 4, true),
        (b"zero", 14, 4, false),
        (b"mgrk", 15, 10, false),
        (b"case", 33, 0, false),
        (b"cpsp", 33, 2, false),
        (b"salt", 17, 1, true),
        (b"titl", 19, 4, true),
        (b"pwid", 22, 0, true),
        (b"fwid", 22, 1, true),
        (b"hwid", 22, 2, true),
        (b"twid", 22, 3, true),
        (b"qwid", 22, 4, true),
        (b"trad", 20, 0, true),
        (b"smpl", 20, 1, true),
        (b"jp78", 20, 2, true),
        (b"jp83", 20, 3, true),
        (b"jp90", 20, 4, true),
        (b"expt", 20, 10, true),
        (b"jp04", 20, 11, true),
        (b"hojo", 20, 12, true),
        (b"nlck", 20, 13, true),
        (b"tnam", 20, 14, true),
        (b"ruby", 28, 2, false),
        (b"ital", 32, 2, false),
        (b"hkna", 34, 0, false),
        (b"vkna", 34, 2, false),
        (b"vert", 4, 0, false),
        (b"vrt2", 4, 0, false),
    ];
    for (name, feature, selector, exclusive) in rows {
        assert_eq!(
            aat_setting(Tag::new(name)),
            Some(AatSetting {
                feature: *feature,
                selector: *selector,
                exclusive: *exclusive,
            }),
            "{}",
            Tag::new(name)
        );
    }
    // `ss01` to `ss20`: Stylistic Alternatives (35), the nth set at 2n.
    for n in 1..=20u16 {
        let name = format!("ss{n:02}");
        let bytes: [u8; 4] = name.as_bytes().try_into().expect("four bytes");
        assert_eq!(
            aat_setting(Tag::new(&bytes)),
            Some(AatSetting {
                feature: 35,
                selector: 2 * n,
                exclusive: false,
            }),
            "{name}"
        );
    }
    // `kern` turns `kerx` off and asks nothing of `morx`; a tag with no row
    // asks nothing.
    for name in [b"kern", b"mark", b"locl", b"ss21", b"ccmp"] {
        assert_eq!(aat_setting(Tag::new(name)), None, "{}", Tag::new(name));
    }
}

/// A subtable runs where its coverage takes the run's orientation: one for
/// vertical text in an upright run alone, one for horizontal text along a
/// line and in a sideways run, and one for both everywhere.
#[test]
fn a_subtable_for_the_other_orientation_is_skipped() {
    let table = morx_table(
        2,
        &[chain(
            1,
            &[],
            &[
                noncontextual_with(VERTICAL, 1, &lookup_single(&[(10, 20)])),
                noncontextual_with(0, 1, &lookup_single(&[(11, 21)])),
                noncontextual_with(ANY_ORIENTATION, 1, &lookup_single(&[(12, 22)])),
            ],
        )],
    );
    let glyphs = [10u16, 11, 12];
    let defaults = Selection::defaults();
    for direction in [Direction::LeftToRight, Direction::RightToLeft] {
        assert_eq!(
            chained(&table, &glyphs, direction, &defaults).0,
            [10, 21, 22],
            "{direction:?}"
        );
    }
    assert_eq!(
        chained(&table, &glyphs, Direction::TopToBottom, &defaults).0,
        [20, 11, 22]
    );
    let morx = Morx::parse(&table).expect("readable");
    let (chains, _) = morx.chains(&Limits::DEFAULT);
    let applies: Vec<(bool, bool)> = chains[0]
        .subtables()
        .iter()
        .map(|s| (s.applies(false), s.applies(true)))
        .collect();
    assert_eq!(applies, vec![(false, true), (true, false), (true, true)]);
}

/// The `mort` chapter's *Example: Non-contextual Glyph Substitution* carried
/// into a `morx`: its chain's default flags, its three feature entries —
/// vertical substitution on and off, and glyph effects off — and its one
/// subtable, the lookup at its offsets 64 to 87 mapping the horizontal
/// parentheses, glyphs 11 and 12, to the vertical ones, 135 and 136. Its
/// coverage is read as the bit says, vertical text: the comment calling it
/// horizontal contradicts both the bit and the example's purpose. The `mort`
/// header's fields widen to `morx`'s and its 16-bit coverage `0x8004` to
/// `0x80000004`.
fn noncontextual_example() -> Vec<u8> {
    let lookup = {
        let mut out = u16s(&[6, 4, 2, 8, 1, 0]);
        out.extend(u16s(&[11, 135, 12, 136]));
        out.extend(u16s(&[0xFFFF, 0]));
        out
    };
    assert_eq!(lookup.len(), 24, "offsets 64 to 87");
    morx_table(
        2,
        &[chain(
            0x0000_0001,
            &[
                (4, 0, 0x0000_0001, 0xFFFF_FFFF),
                (4, 1, 0x0000_0000, 0xFFFF_FFFE),
                (0, 1, 0x0000_0000, 0x0000_0000),
            ],
            &[subtable(0x8000_0004, 0x0000_0001, &lookup)],
        )],
    )
}

/// A noncontextual subtable maps each glyph its lookup covers with a value
/// other than 0 and leaves every other, in every lookup format; held to the
/// `mort` chapter's noncontextual example.
#[test]
fn a_noncontextual_subtable_maps_every_glyph_it_covers() {
    let upright = Direction::TopToBottom;
    let ltr = Direction::LeftToRight;
    let example = noncontextual_example();
    let defaults = Selection::defaults();
    assert_eq!(
        chained(&example, &[11, 12, 13], upright, &defaults),
        (vec![135, 136, 13], vec![])
    );
    assert_eq!(
        chained(&example, &[11, 12, 13], ltr, &defaults).0,
        [11, 12, 13],
        "a vertical subtable along a line"
    );
    // Vertical substitution off, asked as the setting or as `vert` set to
    // 0, and glyph effects off, each clears the flag.
    for selection in [
        asking(&[(4, 1)]),
        Selection::from_opentype(None, &[(Tag::new(b"vert"), 0)], None).0,
        asking(&[(0, 1)]),
    ] {
        assert_eq!(
            chained(&example, &[11, 12], upright, &selection).0,
            [11, 12],
            "{selection:?}"
        );
    }
    assert_eq!(
        chained(&example, &[11, 12], upright, &asking(&[(4, 0)])).0,
        [135, 136]
    );

    // Every format, mapping glyphs 20 to 24 to 120 to 124 but 22, whose
    // value is 0, "do nothing"; every glyph from 0 to 40 asked.
    let values: [(u16, u16); 5] = [(20, 120), (21, 121), (22, 0), (23, 123), (24, 124)];
    let mut simple = vec![0u16; 25];
    for (glyph, value) in values {
        simple[usize::from(glyph)] = value;
    }
    let lookups = [
        lookup_simple(&simple),
        lookup_segments(
            2,
            &values
                .iter()
                .map(|(glyph, value)| (*glyph, *glyph, u64::from(*value)))
                .collect::<Vec<_>>(),
        ),
        lookup_segment_array(&[(
            20,
            values.iter().map(|(_, value)| *value).collect::<Vec<u16>>(),
        )]),
        lookup_single(&values),
        lookup_trimmed(20, &values.map(|(_, value)| value)),
        lookup_extended(2, 20, &values.map(|(_, value)| u64::from(value))),
    ];
    let glyphs: Vec<u16> = (0..=40).collect();
    let expected: Vec<u16> = glyphs
        .iter()
        .map(|glyph| match values.iter().find(|(of, _)| of == glyph) {
            Some((_, value)) if *value != 0 => *value,
            _ => *glyph,
        })
        .collect();
    for lookup in &lookups {
        let table = morx_table(2, &[chain(1, &[], &[noncontextual_with(0, 1, lookup)])]);
        let (out, warnings) = chained(&table, &glyphs, ltr, &defaults);
        assert_eq!(
            out,
            expected,
            "format {}",
            u16::from_be_bytes([lookup[0], lookup[1]])
        );
        assert!(warnings.is_empty());
    }
}

/// A right-to-left run is held in layout order while its chain runs and
/// handed back in logical order, its clusters where they were.
#[test]
fn a_right_to_left_run_comes_back_in_logical_order() {
    let table = morx_table(2, &[chain(1, &[], &[noncontextual(1, &[(2, 20)])])]);
    let morx = Morx::parse(&table).expect("readable");
    let mut buffer = run_of(&[1, 2, 3], Direction::RightToLeft);
    assert!(morx
        .run(&mut buffer, &Selection::defaults(), &Limits::DEFAULT)
        .is_empty());
    let out: Vec<(u16, u32)> = buffer
        .glyphs()
        .iter()
        .map(|g| (g.glyph, g.cluster))
        .collect();
    assert_eq!(out, vec![(1, 0), (20, 1), (3, 2)]);
    assert_eq!(buffer.direction(), Direction::RightToLeft);
}

/// A glyph a subtable makes `0xFFFF` is deleted: no later subtable maps it,
/// and it leaves the run when the chains end, the glyphs around it keeping
/// their clusters.
#[test]
fn a_deleted_glyph_leaves_the_run_when_the_chains_end() {
    let table = morx_table(
        2,
        &[
            chain(
                1,
                &[],
                &[
                    noncontextual(1, &[(10, DELETED)]),
                    noncontextual(1, &[(DELETED, 77), (11, 21)]),
                ],
            ),
            chain(1, &[], &[noncontextual(1, &[(DELETED, 78)])]),
        ],
    );
    let morx = Morx::parse(&table).expect("readable");
    let mut buffer = run_of(&[9, 10, 11, 10], Direction::LeftToRight);
    assert!(morx
        .run(&mut buffer, &Selection::defaults(), &Limits::DEFAULT)
        .is_empty());
    let out: Vec<(u16, u32)> = buffer
        .glyphs()
        .iter()
        .map(|g| (g.glyph, g.cluster))
        .collect();
    assert_eq!(out, vec![(9, 0), (21, 2)]);
}

/// A subtable of a type this crate does not run is skipped and named, the
/// chain going on past it: 3, reserved, and a type above 5, unknown. Every
/// type the chapter defines is run, so a machine's subtable that cannot be
/// read is named as malformed instead.
#[test]
fn a_reserved_or_unknown_subtable_type_is_skipped_and_named() {
    for kind in [3u8, 6, 200] {
        let table = morx_table(
            2,
            &[chain(
                1,
                &[],
                &[
                    subtable(0x2000_0000 | u32::from(kind), 1, &[0; 16]),
                    noncontextual(1, &[(10, 20)]),
                ],
            )],
        );
        let (out, warnings) = chained(
            &table,
            &[10],
            Direction::LeftToRight,
            &Selection::defaults(),
        );
        assert_eq!(out, [20], "type {kind}: the chain went on");
        assert_eq!(
            warnings,
            vec![Warning::SubtableTypeNotRun {
                site: site(0),
                kind
            }]
        );
    }
    // A machine's subtable of sixteen zero bytes is a state table of no
    // classes, which is no state table: it is read, and named as malformed
    // rather than as a type not run.
    for kind in [REARRANGEMENT, CONTEXTUAL, LIGATURE, INSERTION] {
        let table = morx_table(
            2,
            &[chain(
                1,
                &[],
                &[subtable(0x2000_0000 | u32::from(kind), 1, &[0; 16])],
            )],
        );
        let (_, warnings) = chained(
            &table,
            &[10],
            Direction::LeftToRight,
            &Selection::defaults(),
        );
        assert_eq!(
            warnings,
            vec![Warning::MalformedAatSubtable { site: site(0) }],
            "type {kind}"
        );
    }
}

/// A table of a version other than 2 or 3 is no `morx`; a chain the table
/// cannot hold ends the walk and is named, the chains before it run; a
/// subtable its chain cannot hold ends its chain's subtables and is named;
/// a lookup it cannot read is named; and version 3's coverage arrays after
/// a chain's last subtable are not read.
#[test]
fn a_chain_or_subtable_the_table_cannot_hold_is_named() {
    for version in [0u16, 1, 4, 0xFFFF] {
        assert!(
            Morx::parse(&morx_table(version, &[])).is_none(),
            "{version}"
        );
    }
    assert!(Morx::parse(&[0, 2, 0, 0, 0]).is_none(), "a cut header");
    let good = chain(1, &[], &[noncontextual(1, &[(10, 20)])]);
    let ltr = Direction::LeftToRight;
    let defaults = Selection::defaults();

    // Version 3, a coverage array after the chain's last subtable: an
    // offset of 0 and a bitfield, read by no one.
    let mut with_coverage = good.clone();
    with_coverage.extend(u32s(&[0, 0xFFFF_FFFF]));
    let length = count32(with_coverage.len());
    with_coverage[4..8].copy_from_slice(&length.to_be_bytes());
    let three = morx_table(3, &[with_coverage]);
    assert_eq!(chained(&three, &[10], ltr, &defaults), (vec![20], vec![]));

    // A chain shorter than its header, and one longer than the table.
    let mut short = good.clone();
    short[4..8].copy_from_slice(&8u32.to_be_bytes());
    let mut long = good.clone();
    long[4..8].copy_from_slice(&4_000u32.to_be_bytes());
    for bad in [short, long] {
        let table = morx_table(2, &[good.clone(), bad]);
        assert_eq!(
            chained(&table, &[10], ltr, &defaults),
            (
                vec![20],
                vec![Warning::MalformedChain {
                    table: Tag::new(b"morx"),
                    chain: 1
                }]
            )
        );
    }
    // A chain count past the chains the table holds.
    let mut claims = morx_table(2, &[good.clone()]);
    claims[4..8].copy_from_slice(&3u32.to_be_bytes());
    assert_eq!(
        chained(&claims, &[10], ltr, &defaults),
        (
            vec![20],
            vec![Warning::MalformedChain {
                table: Tag::new(b"morx"),
                chain: 1
            }]
        )
    );
    // Feature entries past the chain: the chain cannot say where its
    // subtables are.
    let mut entries = chain(1, &[(1, 0, 1, 0)], &[]);
    entries[8..12].copy_from_slice(&900u32.to_be_bytes());
    let table = morx_table(2, &[entries]);
    assert_eq!(
        chained(&table, &[10], ltr, &defaults).1,
        vec![Warning::MalformedChain {
            table: Tag::new(b"morx"),
            chain: 0
        }]
    );

    // A subtable shorter than its header, and one longer than its chain:
    // the subtables before it run, the ones after it do not, and the next
    // chain runs.
    for length in [4u32, 4_000] {
        let mut bad = noncontextual(1, &[(11, 21)]);
        bad[0..4].copy_from_slice(&length.to_be_bytes());
        let first = chain(
            1,
            &[],
            &[
                noncontextual(1, &[(10, 20)]),
                bad,
                noncontextual(1, &[(12, 22)]),
            ],
        );
        let second = chain(1, &[], &[noncontextual(1, &[(13, 23)])]);
        let table = morx_table(2, &[first, second]);
        assert_eq!(
            chained(&table, &[10, 11, 12, 13], ltr, &defaults),
            (
                vec![20, 11, 12, 23],
                vec![Warning::MalformedAatSubtable { site: site(1) }]
            ),
            "length {length}"
        );
    }

    // A lookup of a format the manual does not define, one cut short, and
    // one whose value is no glyph index.
    let odd = morx_table(
        2,
        &[chain(
            1,
            &[],
            &[noncontextual_with(0, 1, &u16s(&[7, 0, 0]))],
        )],
    );
    assert_eq!(
        chained(&odd, &[10], ltr, &defaults),
        (
            vec![10],
            vec![Warning::UnknownLookupFormat {
                site: site(0),
                format: 7
            }]
        )
    );
    let cut = morx_table(
        2,
        &[chain(1, &[], &[noncontextual_with(0, 1, &u16s(&[6, 4]))])],
    );
    assert_eq!(
        chained(&cut, &[10], ltr, &defaults),
        (
            vec![10],
            vec![Warning::MalformedAatSubtable { site: site(0) }]
        )
    );
    let wide = morx_table(
        2,
        &[chain(
            1,
            &[],
            &[noncontextual_with(
                0,
                1,
                &lookup_pairs(4, &[(10, 0x0001_0000), (11, 21)]),
            )],
        )],
    );
    assert_eq!(
        chained(&wide, &[10, 11], ltr, &defaults),
        (
            vec![10, 21],
            vec![Warning::MalformedAatSubtable { site: site(0) }]
        )
    );
}

/// Past `Limits::max_subtables` — chains and subtables each counting one —
/// nothing more is read, and past `Limits::max_feature_entries` a chain's
/// later entries are not read; each is named, and what was read runs.
#[test]
fn the_subtable_and_feature_entry_ceilings_are_refused_by_name() {
    let table = morx_table(
        2,
        &[
            chain(
                1,
                &[],
                &[noncontextual(1, &[(10, 20)]), noncontextual(1, &[(11, 21)])],
            ),
            chain(
                1,
                &[],
                &[noncontextual(1, &[(12, 22)]), noncontextual(1, &[(13, 23)])],
            ),
        ],
    );
    let glyphs = [10u16, 11, 12, 13];
    let ltr = Direction::LeftToRight;
    let defaults = Selection::defaults();
    let budget = Warning::SubtableBudgetExceeded {
        table: Tag::new(b"morx"),
    };
    for (ceiling, expected, refused) in [
        (6u32, vec![20u16, 21, 22, 23], false),
        (5, vec![20, 21, 22, 13], true),
        (4, vec![20, 21, 12, 13], true),
        (3, vec![20, 21, 12, 13], true),
        (2, vec![20, 11, 12, 13], true),
        (0, vec![10, 11, 12, 13], true),
    ] {
        let limits = Limits {
            max_subtables: ceiling,
            ..Limits::DEFAULT
        };
        let (out, warnings) = chained_under(&table, &glyphs, ltr, &defaults, &limits);
        assert_eq!(out, expected, "ceiling {ceiling}");
        assert_eq!(
            warnings,
            if refused { vec![budget] } else { vec![] },
            "ceiling {ceiling}"
        );
    }

    // Two feature entries and a ceiling of one: the second, which would swap
    // the subtables, is not read, and the subtables are still found.
    let swap = morx_table(
        2,
        &[chain(
            1,
            &[(9, 9, 0, 0xFFFF_FFFF), (1, 2, 2, !1u32)],
            &[noncontextual(1, &[(10, 20)]), noncontextual(2, &[(10, 30)])],
        )],
    );
    let limits = Limits {
        max_feature_entries: 1,
        ..Limits::DEFAULT
    };
    let (out, warnings) = chained_under(&swap, &[10], ltr, &asking(&[(1, 2)]), &limits);
    assert_eq!(out, [20]);
    assert_eq!(
        warnings,
        vec![Warning::FeatureEntryBudgetExceeded {
            table: Tag::new(b"morx"),
            chain: 0
        }]
    );
    let morx = Morx::parse(&swap).expect("readable");
    let (chains, _) = morx.chains(&limits);
    assert_eq!(chains[0].features().len(), 1);
    assert_eq!(chains[0].subtables().len(), 2);
}

/// A chain's parts read back as the table states them: its default flags,
/// its feature entries in order, and each subtable's index, coverage, type,
/// flags and body.
#[test]
fn a_chain_reads_back_as_written() {
    let table = noncontextual_example();
    let morx = Morx::parse(&table).expect("readable");
    assert_eq!(morx.version(), 2);
    assert_eq!(morx.chain_count(), 1);
    let (chains, warnings) = morx.chains(&Limits::DEFAULT);
    assert!(warnings.is_empty());
    let chain = &chains[0];
    assert_eq!(chain.index(), 0);
    assert_eq!(chain.default_flags(), 1);
    assert_eq!(
        chain.features(),
        &[
            FeatureEntry {
                feature: 4,
                setting: 0,
                enable: 1,
                disable: 0xFFFF_FFFF
            },
            FeatureEntry {
                feature: 4,
                setting: 1,
                enable: 0,
                disable: 0xFFFF_FFFE
            },
            FeatureEntry {
                feature: 0,
                setting: 1,
                enable: 0,
                disable: 0
            },
        ]
    );
    let sub = chain.subtables()[0];
    assert_eq!(
        (sub.index(), sub.coverage, sub.kind(), sub.sub_feature_flags),
        (0, 0x8000_0004, NONCONTEXTUAL, 1)
    );
    assert_eq!(sub.body().len(), 24);
    assert_eq!(&sub.body()[..2], &[0, 6]);
}

// --- the rearrangement subtable (FT-07c.6) -------------------------------
//
// Marking a span's first and last glyphs, and every verb of the chapter's
// table applied to the glyphs between them.

/// One worked example of the manual's for a state machine: where it is,
/// the subtable it transcribes, and the glyphs in and out.
struct Worked {
    /// Chapter, section and figure.
    source: &'static str,
    /// The subtable, every byte as the manual gives it, its corrections named
    /// where it is built.
    subtable: fn() -> Vec<u8>,
    /// The glyphs in and out of each case the example states, over a
    /// left-to-right run.
    cases: &'static [(&'static [u16], &'static [u16])],
}

/// The rearrangement subtable's worked examples: none. The verb table
/// defines each verb's effect and shows no machine
/// (`docs/design/shaping.md`, *The manual's worked examples*), so the verbs
/// are held by [`every_rearrangement_verb_moves_what_it_names`] and the
/// machine by the readings below and text-rendering-tests `MORX-2` to
/// `MORX-17`; H-142's expectations are what the count waits on.
const REARRANGEMENT_EXAMPLES: &[Worked] = &[];

/// Runs each worked example through a one-chain `morx` whose default flags
/// select its subtable, and says how many there were.
fn hold_to_worked_examples(examples: &[Worked]) -> usize {
    for example in examples {
        let table = morx_table(2, &[chain(1, &[], &[(example.subtable)()])]);
        assert!(
            !example.cases.is_empty(),
            "{} states a case",
            example.source
        );
        for (input, output) in example.cases {
            let (out, warnings) = chained(
                &table,
                input,
                Direction::LeftToRight,
                &Selection::defaults(),
            );
            assert_eq!(out, *output, "{}, {input:?}", example.source);
            assert!(warnings.is_empty(), "{}: {warnings:?}", example.source);
        }
    }
    examples.len()
}

/// The rearrangement subtable reproduces each worked example
/// `docs/design/shaping.md` lists for type 0, and there are none.
#[test]
fn rearrangement_reproduces_the_manuals_worked_examples() {
    assert_eq!(hold_to_worked_examples(REARRANGEMENT_EXAMPLES), 0);
}

/// A rearrangement subtable for both orientations under flag 1: `classes`
/// gives glyphs their classes, `lines` the state array and `entries` each
/// entry's new state and flags.
fn rearrangement_subtable(
    classes: &[(u16, u16)],
    lines: &[&[u16]],
    entries: &[(u16, u16)],
) -> Vec<u8> {
    let entries: Vec<Vec<u16>> = entries
        .iter()
        .map(|(state, flags)| vec![*state, *flags])
        .collect();
    let entries: Vec<&[u16]> = entries.iter().map(Vec::as_slice).collect();
    let classes_count = lines.first().map_or(4, |line| line.len());
    subtable(
        ANY_ORIENTATION | u32::from(REARRANGEMENT),
        1,
        &stx(
            count32(classes_count),
            0,
            &lookup_single(classes),
            lines,
            &entries,
        ),
    )
}

/// The glyphs and clusters one subtable leaves of `glyphs`, walked as its
/// coverage says over a left-to-right run, and what it refused.
fn through(subtable: Vec<u8>, glyphs: &[u16], limits: &Limits) -> (Vec<(u16, u32)>, Vec<Warning>) {
    let table = morx_table(2, &[chain(1, &[], &[subtable])]);
    let morx = Morx::parse(&table).expect("a readable morx");
    let mut buffer = run_of(glyphs, Direction::LeftToRight);
    let warnings = morx.run(&mut buffer, &Selection::defaults(), limits);
    let out = buffer
        .glyphs()
        .iter()
        .map(|g| (g.glyph, g.cluster))
        .collect();
    (out, warnings)
}

/// Just the glyphs of [`through`].
fn glyphs_through(subtable: Vec<u8>, glyphs: &[u16]) -> Vec<u16> {
    let (out, warnings) = through(subtable, glyphs, &Limits::DEFAULT);
    assert!(warnings.is_empty(), "{warnings:?}");
    out.into_iter().map(|(glyph, _)| glyph).collect()
}

/// The chapter's verb table as it writes each verb.
const VERBS: [&str; 16] = [
    "no change",
    "Ax=>xA",
    "xD=>Dx",
    "AxD=>DxA",
    "ABx=>xAB",
    "ABx=>xBA",
    "xCD=>CDx",
    "xCD=>DCx",
    "AxCD=>CDxA",
    "AxCD=>DCxA",
    "ABxD=>DxAB",
    "ABxD=>DxBA",
    "ABxCD=>CDxAB",
    "ABxCD=>CDxBA",
    "ABxCD=>DCxAB",
    "ABxCD=>DCxBA",
];

/// `span` as the verb `pattern` writes it rearranged, read letter by
/// letter: the letters before `x` name the span's first glyphs, the ones
/// after it its last, `x` what lies between; a span with fewer glyphs than
/// the letters is left as it is.
fn by_the_table(pattern: &str, span: &[u16]) -> Vec<u16> {
    let Some((from, to)) = pattern.split_once("=>") else {
        return span.to_vec();
    };
    let (front, back) = from.split_once('x').expect("an x");
    if span.len() < front.len() + back.len() {
        return span.to_vec();
    }
    let middle = &span[front.len()..span.len() - back.len()];
    let mut named: BTreeMap<char, u16> = BTreeMap::new();
    for (letter, glyph) in front.chars().zip(span) {
        named.insert(letter, *glyph);
    }
    for (letter, glyph) in back.chars().zip(&span[span.len() - back.len()..]) {
        named.insert(letter, *glyph);
    }
    let mut out = Vec::new();
    for letter in to.chars() {
        if letter == 'x' {
            out.extend_from_slice(middle);
        } else {
            out.push(named[&letter]);
        }
    }
    out
}

/// A machine marking glyph 1 first and glyph 5 last, glyph 7 both, and
/// applying `verb` at the end of the text.
fn verb_machine(verb: u16) -> Vec<u8> {
    rearrangement_subtable(
        &[(1, 4), (5, 5), (7, 6)],
        &[&[2, 0, 0, 0, 1, 3, 4], &[2, 0, 0, 0, 1, 3, 4]],
        &[(0, 0), (0, 0x8000), (0, verb), (0, 0x2000), (0, 0xA000)],
    )
}

/// Each verb of the chapter's table moves the glyphs it names, over a
/// five-glyph run `A B x C D` marked from its first glyph to its last.
#[test]
fn every_rearrangement_verb_moves_what_it_names() {
    let run = [1u16, 2, 3, 4, 5];
    let expected: [[u16; 5]; 16] = [
        [1, 2, 3, 4, 5],
        [2, 3, 4, 5, 1],
        [5, 1, 2, 3, 4],
        [5, 2, 3, 4, 1],
        [3, 4, 5, 1, 2],
        [3, 4, 5, 2, 1],
        [4, 5, 1, 2, 3],
        [5, 4, 1, 2, 3],
        [4, 5, 2, 3, 1],
        [5, 4, 2, 3, 1],
        [5, 3, 4, 1, 2],
        [5, 3, 4, 2, 1],
        [4, 5, 3, 1, 2],
        [4, 5, 3, 2, 1],
        [5, 4, 3, 1, 2],
        [5, 4, 3, 2, 1],
    ];
    for (verb, (pattern, want)) in (0u16..).zip(VERBS.iter().zip(expected)) {
        assert_eq!(by_the_table(pattern, &run), want, "the table's verb {verb}");
        assert_eq!(
            glyphs_through(verb_machine(verb), &run),
            want,
            "verb {verb}, {pattern}"
        );
    }
}

/// A span shorter than the glyphs its verb names is left as it is, and one
/// just long enough moves them past an empty `x`: every verb over every span
/// from one glyph to six.
#[test]
fn a_span_shorter_than_its_verb_is_left_as_it_is() {
    for (verb, pattern) in (0u16..).zip(VERBS) {
        for len in 1..=6usize {
            // One glyph is marked first and last at once; a longer span runs
            // from glyph 1 to glyph 5 over glyphs no entry names.
            let run: Vec<u16> = if len == 1 {
                vec![7]
            } else {
                let mut run = vec![1u16];
                run.extend((0..len - 2).map(|n| 10 + u16::try_from(n).expect("small")));
                run.push(5);
                run
            };
            assert_eq!(
                glyphs_through(verb_machine(verb), &run),
                by_the_table(pattern, &run),
                "verb {verb} over {len}"
            );
        }
    }
}

/// Where no glyph was marked first, the span starts at the first glyph the
/// machine met (text-rendering-tests `MORX-7`); where none was marked last
/// there is no span, and a verb moves nothing (`MORX-3`).
#[test]
fn a_span_needs_a_last_glyph_and_starts_where_the_machine_did() {
    // `O B C D 1`: B, C and D mark the last glyph, `1` applies verb 1, and
    // nothing marks the first.
    let unstarted = rearrangement_subtable(
        &[(2, 4), (3, 4), (4, 4), (9, 5)],
        &[&[0, 0, 0, 0, 1, 2], &[0, 0, 0, 0, 1, 2]],
        &[(0, 0), (0, 0x2000), (0, 1)],
    );
    assert_eq!(
        glyphs_through(unstarted, &[8, 2, 3, 4, 9]),
        [2, 3, 4, 8, 9],
        "the O moves after the D"
    );
    // Verbs with no mark at all, and verbs after a first glyph alone.
    for (first, input) in [(0u16, [1u16, 2, 3, 9]), (0x8000, [1, 2, 3, 9])] {
        for verb in 1..=15u16 {
            let unmarked = rearrangement_subtable(
                &[(1, 4), (9, 5)],
                &[&[0, 0, 0, 0, 1, 2], &[0, 0, 0, 0, 1, 2]],
                &[(0, 0), (0, first), (0, verb)],
            );
            assert_eq!(
                glyphs_through(unmarked, &input),
                input,
                "verb {verb}, first {first:#x}"
            );
        }
    }
}

/// At the end of text the current glyph is the last one met: `markLast`
/// there extends the span to it (`MORX-11`), and the end-of-text verb acts
/// once, `dontAdvance` meaning nothing there (`MORX-10`, `MORX-17`).
#[test]
fn the_end_of_text_marks_the_last_glyph_and_its_verb_acts_once() {
    // `B A B B A A B X`: each A marks itself first and last; the end of text
    // marks the X last and applies verb 2, `xD ⇒ Dx`, to the trailing
    // `A B X`.
    let eleven = rearrangement_subtable(
        &[(1, 4)],
        &[&[0, 1, 1, 1, 2], &[0, 1, 1, 1, 2]],
        &[(0, 0x2002), (0, 0), (0, 0xA000)],
    );
    assert_eq!(
        glyphs_through(eleven, &[2, 1, 2, 2, 1, 1, 2, 24]),
        [2, 1, 2, 2, 1, 24, 1, 2]
    );
    // `A B`: A marks first, B last, and the end of text applies verb 1 with
    // `dontAdvance`: once, so the pair is swapped and not swapped back.
    let seventeen = rearrangement_subtable(
        &[(1, 4), (2, 5)],
        &[&[0, 1, 1, 1, 2, 3], &[0, 1, 1, 1, 2, 3]],
        &[(0, 0x4001), (0, 0), (0, 0x8000), (0, 0x2000)],
    );
    assert_eq!(glyphs_through(seventeen, &[1, 2]), [2, 1]);
    // `ABABAB`: each A marks itself first and last, each B last, and the end
    // of text swaps the last pair.
    let ten = rearrangement_subtable(
        &[(1, 4), (2, 5)],
        &[&[0, 1, 1, 1, 2, 3], &[0, 1, 1, 1, 2, 3]],
        &[(0, 0x0001), (0, 0), (0, 0xA000), (0, 0x2000)],
    );
    assert_eq!(glyphs_through(ten, &[1, 2, 1, 2, 1, 2]), [1, 2, 1, 2, 2, 1]);
}

/// The marks stay where they were after a verb, so each later verb acts on
/// the same positions (`MORX-6`): `OOOABCDEFGOOO3141` becomes
/// `OOOEFAGBCDOOO3141`.
#[test]
fn a_span_is_kept_after_its_verb() {
    // O is 30; A to G are 1 to 7; the digits 3, 1 and 4 are 43, 41 and 44.
    let machine = rearrangement_subtable(
        &[
            (1, 4),
            (2, 5),
            (3, 5),
            (4, 5),
            (5, 5),
            (6, 5),
            (7, 5),
            (41, 6),
            (43, 7),
            (44, 8),
        ],
        &[&[0, 0, 0, 0, 1, 2, 3, 4, 5], &[0, 0, 0, 0, 1, 2, 3, 4, 5]],
        &[(0, 0), (0, 0x8000), (0, 0x2000), (0, 1), (0, 3), (0, 4)],
    );
    let input = [
        30u16, 30, 30, 1, 2, 3, 4, 5, 6, 7, 30, 30, 30, 43, 41, 44, 41,
    ];
    let output = [
        30u16, 30, 30, 5, 6, 1, 7, 2, 3, 4, 30, 30, 30, 43, 41, 44, 41,
    ];
    assert_eq!(glyphs_through(machine, &input), output);
}

/// An entry with `dontAdvance` and `markLast` marks the glyph it holds, and
/// the next transition sees the same glyph (`MORX-13`): `ABCDE` becomes
/// `BCDEA`.
#[test]
fn dont_advance_with_mark_last_marks_the_glyph_it_holds() {
    // A marks first and last, B to D last; E in state 0 marks last, holds
    // and moves to state 1, where E applies verb 1.
    let machine = rearrangement_subtable(
        &[(1, 4), (2, 5), (3, 5), (4, 5), (5, 6)],
        &[
            &[0, 0, 0, 0, 1, 2, 3],
            &[0, 0, 0, 0, 1, 2, 3],
            &[4, 4, 4, 4, 1, 5, 6],
        ],
        &[
            (0, 0),
            (0, 0xA000),
            (0, 0x2000),
            (2, 0x6000),
            (2, 0),
            (2, 0x2000),
            (2, 0x0001),
        ],
    );
    assert_eq!(glyphs_through(machine, &[1, 2, 3, 4, 5]), [2, 3, 4, 5, 1]);
}

/// A backward walk marks positions in layout order: the span runs from the
/// first glyph it met, the rightmost, to the one it marked last, and its
/// verb names its glyphs from the left.
#[test]
fn a_backward_walk_marks_positions_in_layout_order() {
    let machine = {
        let mut bytes = rearrangement_subtable(
            &[(1, 4)],
            &[&[1, 0, 0, 0, 2], &[1, 0, 0, 0, 2]],
            &[(0, 0), (0, 0x0001), (0, 0x2000)],
        );
        // The coverage, at bytes 4 to 8 of the subtable: reverse layout
        // order, both orientations, type 0.
        bytes[4..8].copy_from_slice(&(DESCENDING | ANY_ORIENTATION).to_be_bytes());
        bytes
    };
    // Walking back from 5 to 1, glyph 1 is marked last; the span is the
    // whole run, and verb 1 moves its leftmost glyph to its end.
    assert_eq!(glyphs_through(machine, &[1, 2, 3, 4, 5]), [2, 3, 4, 5, 1]);
}

/// A rearranged span's clusters are merged, every glyph of it taking the
/// smallest, so they stay monotonic; a verb that leaves every glyph where
/// it was merges nothing.
#[test]
fn a_rearranged_span_merges_its_clusters_and_a_still_one_does_not() {
    // A marks first, D marks last and applies `verb` there.
    let machine = |verb: u16| {
        rearrangement_subtable(
            &[(1, 4), (5, 5)],
            &[&[0, 0, 0, 0, 1, 2], &[0, 0, 0, 0, 1, 2]],
            &[(0, 0), (0, 0x8000), (0, 0x2000 | verb)],
        )
    };
    let (out, warnings) = through(machine(3), &[9, 1, 2, 5, 9], &Limits::DEFAULT);
    assert!(warnings.is_empty());
    assert_eq!(out, vec![(9, 0), (5, 1), (2, 1), (1, 1), (9, 4)]);
    // Verb 6, `xCD ⇒ CDx`, over the two glyphs `A D`: C and D are those two,
    // and x is empty, so nothing moves and nothing merges.
    let (out, _) = through(machine(6), &[9, 1, 5, 9], &Limits::DEFAULT);
    assert_eq!(out, vec![(9, 0), (1, 1), (5, 2), (9, 3)]);
}

/// A run holding no glyph the machine's class table names is not walked at
/// all, its end-of-text verb included; holding one, it is.
#[test]
fn a_rearrangement_naming_no_glyph_of_the_run_does_not_run() {
    // A marks itself first and last; the end of text marks last and applies
    // verb 2, `xD ⇒ Dx`.
    let machine = || {
        rearrangement_subtable(
            &[(1, 4)],
            &[&[0, 1, 1, 1, 2], &[0, 1, 1, 1, 2]],
            &[(0, 0x2002), (0, 0), (0, 0xA000)],
        )
    };
    assert_eq!(glyphs_through(machine(), &[8, 9]), [8, 9]);
    assert_eq!(glyphs_through(machine(), &[8, 1, 9]), [8, 9, 1]);
}

/// A machine that holds a glyph for ever stops at its ceiling, named, the
/// run as it stood; a subtable that cannot be read is named and moves
/// nothing.
#[test]
fn a_rearrangement_that_loops_or_cannot_be_read_is_named() {
    // A marks first, B last, and C swaps them with verb 1 and holds.
    let looping = rearrangement_subtable(
        &[(1, 4), (2, 5), (3, 6)],
        &[&[0, 0, 0, 0, 1, 2, 3], &[0, 0, 0, 0, 1, 2, 3]],
        &[(0, 0), (0, 0x8000), (0, 0x2000), (0, 0x4001)],
    );
    for (ceiling, glyphs) in [(10u32, [1u16, 2, 3]), (11, [2, 1, 3])] {
        let limits = Limits {
            max_transitions: ceiling,
            ..Limits::DEFAULT
        };
        let (out, warnings) = through(looping.clone(), &[1, 2, 3], &limits);
        assert_eq!(
            out.iter().map(|(glyph, _)| *glyph).collect::<Vec<_>>(),
            glyphs,
            "ceiling {ceiling}: two passed, the rest swaps"
        );
        assert_eq!(
            warnings,
            vec![Warning::TransitionBudgetExceeded { site: site(0) }]
        );
    }

    let short = subtable(ANY_ORIENTATION | u32::from(REARRANGEMENT), 1, &[0, 0, 0, 5]);
    let (out, warnings) = through(short, &[1, 2], &Limits::DEFAULT);
    assert_eq!(out, vec![(1, 0), (2, 1)]);
    assert_eq!(
        warnings,
        vec![Warning::MalformedAatSubtable { site: site(0) }]
    );
    let odd_classes = subtable(
        ANY_ORIENTATION | u32::from(REARRANGEMENT),
        1,
        &stx(4, 0, &u16s(&[9, 0, 0]), &[&[0, 0, 0, 0]], &[&[0, 0]]),
    );
    let (_, warnings) = through(odd_classes, &[1, 2], &Limits::DEFAULT);
    assert_eq!(
        warnings,
        vec![Warning::UnknownLookupFormat {
            site: site(0),
            format: 9
        }]
    );
}

// --- the contextual glyph substitution subtable (FT-07c.7) ---------------
//
// A mark the machine sets and moves, and the per-glyph substitution tables
// its entries name for the marked glyph and the current one.

/// No substitution table: the value an index takes for none.
const NO_TABLE: u16 = 0xFFFF;

/// A contextual subtable over coverage `orientation`'s bits under flag 1:
/// `classes` gives glyphs their classes, `lines` the state array, `entries`
/// each entry's new state, flags, mark index and current index, and
/// `tables` the per-glyph substitution tables, each `(glyph, glyph)` pairs
/// read as a format 6 lookup table. The tables follow the entry table, the
/// array of their offsets first, and `substitutionTable` names where.
fn contextual_subtable(
    orientation: u32,
    classes: &[(u16, u16)],
    lines: &[&[u16]],
    entries: &[[u16; 4]],
    tables: &[&[(u16, u16)]],
) -> Vec<u8> {
    let entries: Vec<&[u16]> = entries.iter().map(|entry| &entry[..]).collect();
    let classes_count = lines.first().map_or(4, |line| line.len());
    let mut body = stx(
        count32(classes_count),
        1,
        &lookup_single(classes),
        lines,
        &entries,
    );
    let space = body.len();
    put32(&mut body, 16, count32(space));
    let array = 4 * tables.len();
    let mut lookups: Vec<u8> = Vec::new();
    for table in tables {
        body.extend(count32(array + lookups.len()).to_be_bytes());
        lookups.extend(lookup_single(table));
        pad4(&mut lookups);
    }
    body.extend(lookups);
    subtable(orientation | u32::from(CONTEXTUAL), 1, &body)
}

/// The `morx` chapter's *Examples*, "1. A contextual substitution table":
/// glyphs 50, 51, 201 and 202 after glyph 80 become the swashes 600, 601, 602
/// and 900, every byte at its offset as the example prints it, but for three
/// corrections. `nClasses`, printed 5, is 6: the state array's lines, printed
/// at 56, 68 and 80, are six entry indices, twelve bytes, apart, and the
/// example's own classes run to 5. The offset at 16, printed `xxx`, is 116,
/// where the example starts its per-glyph tables. And the per-glyph lookup's
/// units, printed at 132, 136, 144 and 148 with the guard at 152, are the
/// contiguous units they must be, 132 to 147 with the guard at 148: the
/// lookup's header ends at 132 and its units are four bytes.
fn swash_example() -> Vec<u8> {
    let mut body = u32s(&[6, 20, 56, 92, 116]);
    // The class lookup: format 6, unit size 4, five units, search range 16,
    // entry selector 2, range shift 4; then the units and the guard.
    body.extend(u16s(&[6, 4, 5, 16, 2, 4]));
    body.extend(u16s(&[50, 4, 51, 4, 80, 5, 201, 4, 202, 4]));
    body.extend(u16s(&[0xFFFF, 1]));
    // The state array: start of text, start of line, and "saw 80".
    body.extend(u16s(&[0, 0, 0, 0, 0, 1]));
    body.extend(u16s(&[0, 0, 0, 0, 0, 1]));
    body.extend(u16s(&[0, 0, 0, 0, 2, 1]));
    // The entries: back to state 0; to "saw 80"; back to state 0, the current
    // glyph put through per-glyph lookup 0.
    body.extend(u16s(&[0, 0, NO_TABLE, NO_TABLE]));
    body.extend(u16s(&[2, 0, NO_TABLE, NO_TABLE]));
    body.extend(u16s(&[0, 0, NO_TABLE, 0]));
    // The per-glyph tables: the offset of lookup 0 from here, then lookup 0.
    body.extend(u32s(&[4]));
    body.extend(u16s(&[6, 4, 4, 16, 2, 0]));
    body.extend(u16s(&[50, 600, 51, 601, 201, 602, 202, 900]));
    body.extend(u16s(&[0xFFFF, 1]));
    assert_eq!(body.len(), 152, "the example's bytes, 0 to 151");
    subtable(ANY_ORIENTATION | u32::from(CONTEXTUAL), 1, &body)
}

/// The capitalising machine of the *AAT special tables* chapter, "State
/// Tables": figure FF30's states, START and IN WORD, figure FF31's four
/// entries — START with no action, IN WORD with `<UPPER>`, IN WORD with
/// none, IN WORD with `<LOWER>` — and figure FF32's state array, out of
/// bounds to entry 0 in either state, a lower-case letter to 1 from START
/// and 2 in a word, an upper-case one to 2 from START and 3 in a word;
/// encoded as a contextual subtable with `UPPER` and `LOWER` as current-glyph
/// substitution tables. The letters `a` to `f` are glyphs 1 to 6 and `A` to
/// `F` 11 to 16, and a space is glyph 20, out of bounds. START is state 0,
/// and state 1, the start of line, a copy of it; IN WORD is state 2. The
/// classes the figures leave out are this transcription's: the end of text
/// and of a line return to START, and a deleted glyph keeps the state it
/// meets.
fn capitalising_example() -> Vec<u8> {
    let lower: Vec<(u16, u16)> = (1..=6).map(|glyph| (glyph, 4)).collect();
    let upper: Vec<(u16, u16)> = (11..=16).map(|glyph| (glyph, 5)).collect();
    let classes: Vec<(u16, u16)> = lower.into_iter().chain(upper).collect();
    let to_upper: Vec<(u16, u16)> = (1..=6).map(|glyph| (glyph, glyph + 10)).collect();
    let to_lower: Vec<(u16, u16)> = (11..=16).map(|glyph| (glyph, glyph - 10)).collect();
    contextual_subtable(
        ANY_ORIENTATION,
        &classes,
        &[
            &[0, 0, 0, 0, 1, 2],
            &[0, 0, 0, 0, 1, 2],
            &[0, 0, 2, 0, 2, 3],
        ],
        &[
            [0, 0, NO_TABLE, NO_TABLE],
            [2, 0, NO_TABLE, 0],
            [2, 0, NO_TABLE, NO_TABLE],
            [2, 0, NO_TABLE, 1],
        ],
        &[to_upper.as_slice(), to_lower.as_slice()],
    )
}

/// The contextual subtable's worked examples, as `docs/design/shaping.md`
/// lists them for type 1: two.
const CONTEXTUAL_EXAMPLES: &[Worked] = &[
    Worked {
        source: "the morx chapter, Examples, 1. A contextual substitution table",
        subtable: swash_example,
        cases: &[
            (&[80, 50], &[80, 600]),
            (&[80, 51], &[80, 601]),
            (&[80, 201], &[80, 602]),
            (&[80, 202], &[80, 900]),
            (&[50, 51, 201, 202], &[50, 51, 201, 202]),
            (&[80, 80, 50], &[80, 80, 600]),
            (&[80, 50, 51], &[80, 600, 51]),
            (&[7, 80, 202, 80, 51], &[7, 80, 900, 80, 601]),
        ],
    },
    Worked {
        source: "AAT special tables, State Tables, figures FF30, FF31 and FF32",
        subtable: capitalising_example,
        // Figure FF30's input and output: `abC deF` becomes `Abc Def`.
        cases: &[(&[1, 2, 13, 20, 4, 5, 16], &[11, 2, 3, 20, 14, 5, 6])],
    },
];

/// The contextual subtable reproduces each worked example
/// `docs/design/shaping.md` lists for type 1, and there are two.
#[test]
fn contextual_substitution_reproduces_the_manuals_worked_examples() {
    assert_eq!(hold_to_worked_examples(CONTEXTUAL_EXAMPLES), 2);
}

/// A machine marking glyph 1 and substituting the marked glyph through
/// table 0 at glyph 9.
fn marking_machine() -> Vec<u8> {
    contextual_subtable(
        ANY_ORIENTATION,
        &[(1, 4), (9, 5)],
        &[&[0, 0, 0, 0, 1, 2], &[0, 0, 0, 0, 1, 2]],
        &[
            [0, 0, NO_TABLE, NO_TABLE],
            [0, 0x8000, NO_TABLE, NO_TABLE],
            [0, 0, 0, NO_TABLE],
        ],
        &[&[(1, 101), (5, 105)]],
    )
}

/// The marked glyph is substituted wherever the machine has moved since
/// marking it, `setMark` moves the mark, the mark stays where it was after
/// a substitution, and where no glyph was marked the mark is the first glyph
/// the machine met (`MORX-19`).
#[test]
fn a_marked_glyph_is_substituted_after_the_machine_has_moved_past_it() {
    assert_eq!(
        glyphs_through(marking_machine(), &[1, 5, 6, 7, 9]),
        [101, 5, 6, 7, 9]
    );
    assert_eq!(
        glyphs_through(marking_machine(), &[1, 5, 1, 9]),
        [1, 5, 101, 9],
        "the second mark is the one substituted"
    );
    assert_eq!(
        glyphs_through(marking_machine(), &[1, 9, 9]),
        [101, 9, 9],
        "the mark stays on its glyph, whose new value the table does not map"
    );
    assert_eq!(
        glyphs_through(marking_machine(), &[5, 9]),
        [105, 9],
        "no mark set: the first glyph met"
    );
}

/// The marked glyph is substituted before the current one, so both indices
/// on one glyph chain (`MORX-22`); `setMark` takes the current glyph after
/// the substitutions are made (`MORX-21`).
#[test]
fn the_mark_is_substituted_first_and_set_after() {
    // A marks itself, holds and moves to state 2, where A puts the marked
    // glyph through table 0 (A to B) and the current one through table 1
    // (B to C).
    let chaining = contextual_subtable(
        ANY_ORIENTATION,
        &[(1, 4)],
        &[&[0, 0, 0, 0, 1], &[0, 0, 0, 0, 1], &[2, 2, 2, 2, 3]],
        &[
            [0, 0, NO_TABLE, NO_TABLE],
            [2, 0xC000, NO_TABLE, NO_TABLE],
            [2, 0, NO_TABLE, NO_TABLE],
            [0, 0, 0, 1],
        ],
        &[&[(1, 2)], &[(2, 3)]],
    );
    assert_eq!(glyphs_through(chaining, &[1]), [3]);

    // B marks itself; D marks itself and puts the marked glyph through
    // table 0: B is substituted, not D.
    let resetting = contextual_subtable(
        ANY_ORIENTATION,
        &[(2, 4), (4, 5)],
        &[&[0, 0, 0, 0, 1, 2], &[0, 0, 0, 0, 1, 2]],
        &[
            [0, 0, NO_TABLE, NO_TABLE],
            [0, 0x8000, NO_TABLE, NO_TABLE],
            [0, 0x8000, 0, NO_TABLE],
        ],
        &[&[(2, 12), (4, 14)]],
    );
    assert_eq!(
        glyphs_through(resetting, &[1, 2, 3, 4, 5]),
        [1, 12, 3, 4, 5]
    );
}

/// At the end of text the current glyph is the last one met, and the mark
/// is substituted before it (`MORX-20`); a run holding no glyph the machine
/// names takes no end of text at all.
#[test]
fn the_end_of_text_substitutes_the_mark_and_the_last_glyph() {
    // A, B and C mark themselves; the end of text puts the marked glyph
    // through table 0 and the current one through table 1.
    let machine = || {
        contextual_subtable(
            ANY_ORIENTATION,
            &[(1, 4), (2, 4), (3, 4)],
            &[&[0, 1, 1, 1, 2], &[0, 1, 1, 1, 2]],
            &[
                [0, 0, 0, 1],
                [0, 0, NO_TABLE, NO_TABLE],
                [0, 0x8000, NO_TABLE, NO_TABLE],
            ],
            &[
                &[(1, 11), (2, 12), (3, 13), (4, 14), (5, 15)],
                &[(1, 21), (2, 22), (3, 23), (4, 24), (5, 25)],
            ],
        )
    };
    for (input, output) in [
        (vec![1u16, 2, 3, 4, 5], vec![1u16, 2, 13, 4, 25]),
        (vec![1, 2, 3], vec![1, 2, 13]),
        (vec![1, 2, 5], vec![1, 12, 25]),
        (vec![1, 5], vec![11, 25]),
        (vec![1], vec![11]),
        (vec![5, 5], vec![5, 5]),
        (vec![5], vec![5]),
    ] {
        assert_eq!(glyphs_through(machine(), &input), output, "{input:?}");
    }
}

/// A current substitution with `dontAdvance` looks at the glyph again until
/// it leaves the classes that hold it (`MORX-23`); one that never lets go
/// stops at the ceiling, named, the run as it stood (`MORX-24`).
#[test]
fn dont_advance_holds_until_the_glyph_changes_and_no_further_than_the_ceiling() {
    let ladder = contextual_subtable(
        ANY_ORIENTATION,
        &[(1, 4), (2, 4), (3, 4), (4, 4)],
        &[&[0, 0, 0, 0, 1], &[0, 0, 0, 0, 1]],
        &[[0, 0, NO_TABLE, NO_TABLE], [0, 0x4000, NO_TABLE, 0]],
        &[&[(1, 2), (2, 3), (3, 4), (4, 5)]],
    );
    assert_eq!(glyphs_through(ladder, &[1, 2, 3, 4, 5]), [5, 5, 5, 5, 5]);

    let forever = contextual_subtable(
        ANY_ORIENTATION,
        &[(1, 4), (2, 4), (3, 4), (4, 4), (5, 4)],
        &[&[0, 0, 0, 0, 1], &[0, 0, 0, 0, 1]],
        &[[0, 0, NO_TABLE, NO_TABLE], [0, 0x4000, NO_TABLE, 0]],
        &[&[(1, 2), (2, 3), (3, 4), (4, 5), (5, 1)]],
    );
    let limits = Limits {
        max_transitions: 6,
        ..Limits::DEFAULT
    };
    let (out, warnings) = through(forever, &[1, 2], &limits);
    // Six substitutions of the first glyph: 1, 2, 3, 4, 5, 1, 2.
    assert_eq!(out, vec![(2, 0), (2, 1)]);
    assert_eq!(
        warnings,
        vec![Warning::TransitionBudgetExceeded { site: site(0) }]
    );
}

/// An index past the substitution tables the subtable holds substitutes
/// nothing and is named once, the machine going on; a glyph substituted to
/// `0xFFFF` is deleted and leaves the run when the chains end.
#[test]
fn a_missing_table_is_named_and_a_deleted_glyph_leaves() {
    // Glyph 1 names table 3 of two; glyph 2 names table 1; glyph 3 table 0.
    let machine = contextual_subtable(
        ANY_ORIENTATION,
        &[(1, 4), (2, 5), (3, 6)],
        &[&[0, 0, 0, 0, 1, 2, 3], &[0, 0, 0, 0, 1, 2, 3]],
        &[
            [0, 0, NO_TABLE, NO_TABLE],
            [0, 0, NO_TABLE, 3],
            [0, 0, NO_TABLE, 1],
            [0, 0, NO_TABLE, 0],
        ],
        &[&[(3, DELETED)], &[(2, 22)]],
    );
    let (out, warnings) = through(machine, &[1, 2, 1, 3, 9], &Limits::DEFAULT);
    assert_eq!(out, vec![(1, 0), (22, 1), (1, 2), (9, 4)]);
    assert_eq!(
        warnings,
        vec![Warning::SubstitutionTableMissing {
            site: site(0),
            index: 3
        }]
    );
}

/// A subtable with no `substitutionTable` offset, a substitution table of a
/// format the manual does not define, and a value no glyph index holds are
/// each named, and substitute nothing.
#[test]
fn a_contextual_subtable_that_cannot_be_read_is_named() {
    // An `STXHeader` whose three parts all sit at its own start, sixteen
    // bytes long: its class table reads as format 0 and gives glyph 0 class
    // 4, but there is no word at 16 for the substitution tables' offset.
    let headless = subtable(
        ANY_ORIENTATION | u32::from(CONTEXTUAL),
        1,
        &u32s(&[4, 0, 0, 0]),
    );
    let (out, warnings) = through(headless, &[0], &Limits::DEFAULT);
    assert_eq!(out, vec![(0, 0)]);
    assert_eq!(
        warnings,
        vec![Warning::MalformedAatSubtable { site: site(0) }]
    );

    // Table 0 of an undefined format, and table 1 of four-byte values, one
    // too large for a glyph index.
    let mut machine = contextual_subtable(
        ANY_ORIENTATION,
        &[(1, 4), (2, 5)],
        &[&[0, 0, 0, 0, 1, 2], &[0, 0, 0, 0, 1, 2]],
        &[
            [0, 0, NO_TABLE, NO_TABLE],
            [0, 0, NO_TABLE, 0],
            [0, 0, NO_TABLE, 1],
        ],
        &[&[(1, 11)], &[(2, 12)]],
    );
    // The first table's format, at the start of the lookups: the subtable
    // header is 12 bytes, and its state table's `substitutionTable` word
    // says where the offsets are.
    let space = 12
        + usize::try_from(u32::from_be_bytes(
            machine[12 + 16..12 + 20].try_into().expect("four bytes"),
        ))
        .expect("small");
    let first = space
        + usize::try_from(u32::from_be_bytes(
            machine[space..space + 4].try_into().expect("four bytes"),
        ))
        .expect("small");
    machine[first..first + 2].copy_from_slice(&3u16.to_be_bytes());
    let (out, warnings) = through(machine, &[1, 2], &Limits::DEFAULT);
    assert_eq!(out, vec![(1, 0), (12, 1)]);
    assert_eq!(
        warnings,
        vec![Warning::UnknownLookupFormat {
            site: site(0),
            format: 3
        }]
    );

    let wide = {
        let entries: [&[u16]; 2] = [&[0, 0, NO_TABLE, NO_TABLE], &[0, 0, NO_TABLE, 0]];
        let mut body = stx(
            5,
            1,
            &lookup_single(&[(1, 4), (2, 4)]),
            &[&[0, 0, 0, 0, 1], &[0, 0, 0, 0, 1]],
            &entries,
        );
        let space = body.len();
        put32(&mut body, 16, count32(space));
        body.extend(4u32.to_be_bytes());
        body.extend(lookup_pairs(4, &[(1, 0x0001_0000), (2, 22)]));
        subtable(ANY_ORIENTATION | u32::from(CONTEXTUAL), 1, &body)
    };
    let (out, warnings) = through(wide, &[1, 2], &Limits::DEFAULT);
    assert_eq!(out, vec![(1, 0), (22, 1)]);
    assert_eq!(
        warnings,
        vec![Warning::MalformedAatSubtable { site: site(0) }]
    );
}

/// The four processing orders bits 30 and 28 choose, over a left-to-right
/// run and a right-to-left one (`MORX-37` to `MORX-40`): A marks itself and
/// B, met after it, substitutes both, so a pair is substituted only where the
/// walk meets A first.
#[test]
fn each_processing_order_meets_the_glyphs_its_coverage_says() {
    let machine = |order: u32| {
        contextual_subtable(
            ANY_ORIENTATION | order,
            &[(1, 4), (2, 5)],
            &[&[0, 0, 0, 0, 1, 0], &[0, 0, 0, 0, 0, 2]],
            &[
                [0, 0, NO_TABLE, NO_TABLE],
                [1, 0x8000, NO_TABLE, NO_TABLE],
                [0, 0, 0, 0],
            ],
            &[&[(1, 11), (2, 12)]],
        )
    };
    let ltr = Direction::LeftToRight;
    let rtl = Direction::RightToLeft;
    // Each order, and for each run its logical glyphs and what they become:
    // `AB` and `BA` typed left to right, and right to left.
    let orders: [(u32, [[u16; 2]; 4]); 4] = [
        (0, [[11, 12], [2, 1], [1, 2], [12, 11]]),
        (LOGICAL, [[11, 12], [2, 1], [11, 12], [2, 1]]),
        (DESCENDING, [[1, 2], [12, 11], [11, 12], [2, 1]]),
        (DESCENDING | LOGICAL, [[1, 2], [12, 11], [1, 2], [12, 11]]),
    ];
    for (order, expected) in orders {
        let runs = [
            ([1u16, 2], ltr),
            ([2, 1], ltr),
            ([1, 2], rtl),
            ([2, 1], rtl),
        ];
        for ((glyphs, direction), want) in runs.iter().zip(expected) {
            let table = morx_table(2, &[chain(1, &[], &[machine(order)])]);
            let (out, warnings) = chained(&table, glyphs, *direction, &Selection::defaults());
            assert_eq!(out, want, "order {order:#x}, {glyphs:?} {direction:?}");
            assert!(warnings.is_empty());
        }
    }
}

// --- the ligature subtable (FT-07c.8) --------------------------------------
//
// A component stack the machine pushes glyphs on, and the action lists that
// pop them through the component table into the ligature list.

/// A ligature action's `last` flag, which also stores.
const LAST_ACTION: u32 = 0x8000_0000;
/// A ligature action's `store` flag.
const STORE_ACTION: u32 = 0x4000_0000;

/// A ligature subtable for both orientations under flag 1: `classes`,
/// `lines` and `entries` — new state, flags and action index — as its state
/// table's, then its action table, component table and ligature list, in
/// that order after the entries, each offset stated from the start of the
/// state table as the chapter's three `UInt32`s state it.
fn ligature_subtable(
    classes: &[(u16, u16)],
    lines: &[&[u16]],
    entries: &[[u16; 3]],
    actions: &[u32],
    components: &[u16],
    ligatures: &[u16],
) -> Vec<u8> {
    let entries: Vec<&[u16]> = entries.iter().map(|entry| &entry[..]).collect();
    let classes_count = lines.first().map_or(4, |line| line.len());
    let mut body = stx(
        count32(classes_count),
        3,
        &lookup_single(classes),
        lines,
        &entries,
    );
    let actions_at = body.len();
    body.extend(u32s(actions));
    let components_at = body.len();
    body.extend(u16s(components));
    let ligatures_at = body.len();
    body.extend(u16s(ligatures));
    put32(&mut body, 16, count32(actions_at));
    put32(&mut body, 20, count32(components_at));
    put32(&mut body, 24, count32(ligatures_at));
    subtable(ANY_ORIENTATION | u32::from(LIGATURE), 1, &body)
}

/// The `morx` chapter's *Examples*, "2. A ligature table": the 24 ligatures
/// of `a`, `b` or `c`, `d` or `e`, and `f` to `i` — glyphs 20 to 28 — to
/// glyphs 1000 to 1015, 1500 to 1506 and 1511, every byte at offsets 0 to 221
/// as the example prints it, but for one correction: the class lookup's third
/// segment, printed at offset 52 with the second's values `24, 23, 5`, is the
/// glyphs 25 to 28 in class 6 its own comment states, the class the state
/// array's last line acts on and nothing else maps.
fn ligature_example() -> Vec<u8> {
    let mut body = u32s(&[7, 28, 64, 120, 144, 156, 174]);
    // The class lookup: format 2, six-byte segments, three of them, search
    // range 12, entry selector 1, range shift 6; then the segments, each its
    // last glyph, first glyph and class, and the guard.
    body.extend(u16s(&[2, 6, 3, 12, 1, 6]));
    body.extend(u16s(&[22, 20, 4, 24, 23, 5, 28, 25, 6]));
    body.extend(u16s(&[0xFFFF, 0xFFFF, 1]));
    // The state array: start of text, start of line, "saw a, b, or c", and
    // "saw a, b, or c followed by d or e".
    body.extend(u16s(&[0, 0, 0, 0, 1, 0, 0]));
    body.extend(u16s(&[0, 0, 0, 0, 1, 0, 0]));
    body.extend(u16s(&[0, 0, 0, 0, 1, 2, 0]));
    body.extend(u16s(&[0, 0, 0, 0, 1, 2, 3]));
    // The entries: back to state 0; set a component and go to state 2; set
    // a component and go to state 3; set a component and act from action 0.
    body.extend(u16s(&[0, 0, 0, 2, 0x8000, 0, 3, 0x8000, 0, 0, 0xA000, 0]));
    // The actions: -25 for the component popped first, -19 for the second,
    // and -14 for the last.
    body.extend(u32s(&[0x3FFF_FFE7, 0x3FFF_FFED, 0xBFFF_FFF2]));
    // The component table.
    body.extend(u16s(&[0, 1, 2, 3, 0, 4, 0, 8, 16]));
    // The ligature list.
    let ligatures: Vec<u16> = (1000..=1015).chain(1500..=1506).chain([1511]).collect();
    body.extend(u16s(&ligatures));
    assert_eq!(body.len(), 222, "the example's bytes, 0 to 221");
    subtable(ANY_ORIENTATION | u32::from(LIGATURE), 1, &body)
}

/// The ligature subtable's worked examples, as `docs/design/shaping.md`
/// lists them for type 2: one.
const LIGATURE_EXAMPLES: &[Worked] = &[Worked {
    source: "the morx chapter, Examples, 2. A ligature table",
    subtable: ligature_example,
    cases: &[
        // The example's 24 ligatures, `a` to `c` at 20 to 22, `d` and `e`
        // at 23 and 24, `f` to `i` at 25 to 28.
        (&[20, 23, 25], &[1000]),
        (&[20, 23, 26], &[1001]),
        (&[20, 23, 27], &[1002]),
        (&[20, 23, 28], &[1003]),
        (&[20, 24, 25], &[1004]),
        (&[20, 24, 26], &[1005]),
        (&[20, 24, 27], &[1006]),
        (&[20, 24, 28], &[1007]),
        (&[21, 23, 25], &[1008]),
        (&[21, 23, 26], &[1009]),
        (&[21, 23, 27], &[1010]),
        (&[21, 23, 28], &[1011]),
        (&[21, 24, 25], &[1012]),
        (&[21, 24, 26], &[1013]),
        (&[21, 24, 27], &[1014]),
        (&[21, 24, 28], &[1015]),
        (&[22, 23, 25], &[1500]),
        (&[22, 23, 26], &[1501]),
        (&[22, 23, 27], &[1502]),
        (&[22, 23, 28], &[1503]),
        (&[22, 24, 25], &[1504]),
        (&[22, 24, 26], &[1505]),
        (&[22, 24, 27], &[1506]),
        (&[22, 24, 28], &[1511]),
        // Two components form nothing, and a first and third with no second
        // form nothing either.
        (&[20, 23], &[20, 23]),
        (&[20, 25], &[20, 25]),
        // A first component the machine starts again at stays pushed, and
        // the ligature forms from the components after it.
        (&[20, 20, 23, 25], &[20, 1000]),
        // Two ligatures in a row, and glyphs the class table does not name
        // around one.
        (&[20, 23, 25, 21, 24, 28], &[1000, 1015]),
        (&[7, 22, 24, 26, 7], &[7, 1505, 7]),
    ],
}];

/// The ligature subtable reproduces each worked example
/// `docs/design/shaping.md` lists for type 2, and there is one.
#[test]
fn ligature_reproduces_the_manuals_worked_examples() {
    assert_eq!(hold_to_worked_examples(LIGATURE_EXAMPLES), 1);
}

/// A machine over glyphs 10 and 11, both class 4: the first pushes and moves
/// to state 2, where any glyph the class table does not name keeps the
/// state, and the second pushes and acts, its own glyph deleted and the
/// first's replaced by ligature 99. `coverage` is the orientation and walk
/// bits of the subtable.
fn pairing_machine(coverage: u32) -> Vec<u8> {
    let mut bytes = ligature_subtable(
        &[(10, 4), (11, 4)],
        &[&[0, 0, 0, 0, 1], &[0, 0, 0, 0, 1], &[2, 2, 2, 2, 3]],
        &[[0, 0, 0], [2, 0x8000, 0], [2, 0, 0], [0, 0xA000, 0]],
        &[0, LAST_ACTION],
        &[0; 12],
        &[99],
    );
    // The coverage, at bytes 4 to 8 of the subtable.
    bytes[4..8].copy_from_slice(&(coverage | u32::from(LIGATURE)).to_be_bytes());
    bytes
}

/// A ligature takes the first cluster of its components whichever of them
/// it is stored at: stored at the first, the glyphs after it keep their
/// clusters; stored at a later one — walking a run backwards, or a
/// right-to-left run in layout order — the clusters from the ligature to its
/// first component are merged, so they stay monotonic.
#[test]
fn a_ligature_keeps_its_first_components_cluster() {
    // Forward: `10 5 11`, the ligature stored at the 10.
    let (out, warnings) = through(
        pairing_machine(ANY_ORIENTATION),
        &[10, 5, 11],
        &Limits::DEFAULT,
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(out, vec![(99, 0), (5, 1)]);
    // The manual's example: each ligature the cluster of its `a`, `b` or
    // `c`, and a glyph after it its own.
    let (out, _) = through(ligature_example(), &[22, 24, 26, 7], &Limits::DEFAULT);
    assert_eq!(out, vec![(1505, 0), (7, 3)]);
    let (out, _) = through(
        ligature_example(),
        &[7, 20, 23, 25, 21, 24, 28],
        &Limits::DEFAULT,
    );
    assert_eq!(out, vec![(7, 0), (1000, 1), (1015, 4)]);

    // Backward: the 11 is pushed first and the ligature stored there, so it
    // takes the 10's cluster, and the 5 between them is merged into it.
    let (out, warnings) = through(
        pairing_machine(ANY_ORIENTATION | DESCENDING),
        &[10, 5, 11],
        &Limits::DEFAULT,
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(out, vec![(5, 0), (99, 0)]);

    // A right-to-left run, walked in layout order: the ligature is stored at
    // the 11, its layout's first glyph, and takes the 10's cluster, the
    // first in the text; in logical order the 5 comes first, merged.
    let table = morx_table(2, &[chain(1, &[], &[pairing_machine(ANY_ORIENTATION)])]);
    let morx = Morx::parse(&table).expect("readable");
    let mut buffer = run_of(&[10, 5, 11], Direction::RightToLeft);
    assert!(morx
        .run(&mut buffer, &Selection::defaults(), &Limits::DEFAULT)
        .is_empty());
    let out: Vec<(u16, u32)> = buffer
        .glyphs()
        .iter()
        .map(|g| (g.glyph, g.cluster))
        .collect();
    assert_eq!(out, vec![(5, 0), (99, 0)]);
}

/// A machine pushing every glyph 10 and acting at glyph 11, its one action
/// replacing the top of the stack by ligature 99.
fn stacking_machine() -> Vec<u8> {
    ligature_subtable(
        &[(10, 4), (11, 5)],
        &[&[0, 0, 0, 0, 1, 2], &[0, 0, 0, 0, 1, 2]],
        &[[0, 0, 0], [0, 0x8000, 0], [0, 0xA000, 0]],
        &[LAST_ACTION],
        &[0; 12],
        &[99],
    )
}

/// The component stack holds sixteen glyphs: a push past them is refused
/// and named, once however often, and the machine goes on, its action
/// popping the sixteenth.
#[test]
fn a_component_stack_past_its_ceiling_is_refused_and_named() {
    assert_eq!(COMPONENT_STACK, 16, "the mort chapter's sixteen");
    let full = Warning::ComponentStackFull { site: site(0) };
    // Fifteen glyphs and the actor fill the stack exactly.
    let mut run = vec![10u16; 15];
    run.push(11);
    let (out, warnings) = through(stacking_machine(), &run, &Limits::DEFAULT);
    assert!(warnings.is_empty(), "{warnings:?}");
    let mut expected: Vec<u16> = vec![10; 15];
    expected.push(99);
    assert_eq!(out.iter().map(|(g, _)| *g).collect::<Vec<_>>(), expected);

    // Sixteen and the actor: the actor's own push is refused, and its action
    // pops the sixteenth 10.
    let mut run = vec![10u16; 16];
    run.push(11);
    let (out, warnings) = through(stacking_machine(), &run, &Limits::DEFAULT);
    assert_eq!(warnings, vec![full]);
    let mut expected: Vec<u16> = vec![10; 15];
    expected.extend([99, 11]);
    assert_eq!(out.iter().map(|(g, _)| *g).collect::<Vec<_>>(), expected);

    // Twenty and the actor: five pushes refused, named once.
    let mut run = vec![10u16; 20];
    run.push(11);
    let (out, warnings) = through(stacking_machine(), &run, &Limits::DEFAULT);
    assert_eq!(warnings, vec![full]);
    let mut expected: Vec<u16> = vec![10; 15];
    expected.push(99);
    expected.extend([10, 10, 10, 10, 11]);
    assert_eq!(out.iter().map(|(g, _)| *g).collect::<Vec<_>>(), expected);

    // A machine that pushes without advancing fills the stack, is refused,
    // and stops at its transition ceiling, both named.
    let holding = ligature_subtable(
        &[(10, 4)],
        &[&[0, 0, 0, 0, 1], &[0, 0, 0, 0, 1]],
        &[[0, 0, 0], [0, 0xC000, 0]],
        &[LAST_ACTION],
        &[0; 12],
        &[99],
    );
    let limits = Limits {
        max_transitions: 40,
        ..Limits::DEFAULT
    };
    let (out, warnings) = through(holding, &[10, 7], &limits);
    assert_eq!(out, vec![(10, 0), (7, 1)]);
    assert_eq!(
        warnings,
        vec![full, Warning::TransitionBudgetExceeded { site: site(0) }]
    );
}

/// The machine of text-rendering-tests `MORX-41`'s face: `a` and `b` push,
/// `c` pushes and acts, two actions of offset −2, the second last; glyphs `a`
/// to `c` are 2 to 4 and the ligatures `a_c` and `b_c` 5 and 6.
fn under_and_overflow_machine() -> Vec<u8> {
    ligature_subtable(
        &[(2, 4), (3, 5), (4, 6)],
        &[&[0, 0, 0, 0, 1, 1, 2]],
        &[[0, 0, 0], [0, 0x8000, 0], [0, 0xA000, 0]],
        &[0x3FFF_FFFE, 0xBFFF_FFFE],
        &[0, 1, 0, 0],
        &[5, 6],
    )
}

/// An action popping an empty stack ends its list there, what it formed
/// standing, and is named; an action, a component or a ligature the
/// subtable does not hold ends it the same way, named as malformed, the
/// glyph it would have replaced standing.
#[test]
fn an_action_list_the_stack_or_the_tables_cannot_serve_ends_and_is_named() {
    let empty = Warning::ComponentStackEmpty { site: site(0) };
    let malformed = Warning::MalformedAatSubtable { site: site(0) };
    let machine = under_and_overflow_machine;
    assert_eq!(glyphs_through(machine(), &[2, 4]), [5]);
    assert_eq!(glyphs_through(machine(), &[3, 4]), [6]);
    // `cc`: each `c` is deleted by the first action, and the second finds
    // the stack empty.
    let (out, warnings) = through(machine(), &[4, 4], &Limits::DEFAULT);
    assert_eq!(out, vec![]);
    assert_eq!(warnings, vec![empty]);
    // `abcc`: the first `c` forms `b_c`, which is pushed again over the `a`;
    // the second pops it, and its component index, 6 − 2, is past the
    // component table, where the ligature list's 5 stands: ligature 5 is
    // past the list, so `b_c` stands.
    let (out, warnings) = through(machine(), &[2, 3, 4, 4], &Limits::DEFAULT);
    assert_eq!(out, vec![(2, 0), (6, 1)]);
    assert_eq!(warnings, vec![malformed]);

    // An action index past the action table.
    let past_actions = ligature_subtable(
        &[(10, 4)],
        &[&[0, 0, 0, 0, 1], &[0, 0, 0, 0, 1]],
        &[[0, 0, 0], [0, 0xA000, 9]],
        &[LAST_ACTION],
        &[0; 12],
        &[99],
    );
    let (out, warnings) = through(past_actions, &[10], &Limits::DEFAULT);
    assert_eq!(out, vec![(10, 0)]);
    assert_eq!(warnings, vec![malformed]);
    // A component index past the component table, the ligature list empty
    // behind it.
    let past_components = ligature_subtable(
        &[(10, 4)],
        &[&[0, 0, 0, 0, 1], &[0, 0, 0, 0, 1]],
        &[[0, 0, 0], [0, 0xA000, 0]],
        &[LAST_ACTION],
        &[0; 4],
        &[],
    );
    let (out, warnings) = through(past_components, &[10], &Limits::DEFAULT);
    assert_eq!(out, vec![(10, 0)]);
    assert_eq!(warnings, vec![malformed]);
}

/// A subtable whose state table holds no offsets to its three tables, or an
/// offset past its end, is named, and changes nothing.
#[test]
fn a_ligature_subtable_that_cannot_be_read_is_named() {
    let malformed = Warning::MalformedAatSubtable { site: site(0) };
    // An `STXHeader` whose three parts all sit at its own start, sixteen
    // bytes long: its class table reads as format 0 and gives glyph 0 class
    // 4, but there is no word at 16 for the action table's offset.
    let headless = subtable(
        ANY_ORIENTATION | u32::from(LIGATURE),
        1,
        &u32s(&[4, 0, 0, 0]),
    );
    let (out, warnings) = through(headless, &[0], &Limits::DEFAULT);
    assert_eq!(out, vec![(0, 0)]);
    assert_eq!(warnings, vec![malformed]);

    // Each of the three offsets pointed one byte past the subtable: the body
    // starts twelve bytes into the subtable, and the offsets at 16, 20 and
    // 24 of the body.
    for slot in [16usize, 20, 24] {
        let mut bytes = pairing_machine(ANY_ORIENTATION);
        let body = bytes.len() - 12;
        put32(&mut bytes, 12 + slot, count32(body + 1));
        let (out, warnings) = through(bytes, &[10, 11], &Limits::DEFAULT);
        assert_eq!(out, vec![(10, 0), (11, 1)], "slot {slot}");
        assert_eq!(warnings, vec![malformed], "slot {slot}");
    }
}

/// An action with `store` forms a ligature in place of the glyph it pops and
/// the list goes on, the sum starting again for the next; each ligature is
/// pushed again, so a later action can make it a component of another.
#[test]
fn a_list_stores_each_ligature_and_a_ligature_is_a_component_of_the_next() {
    // Glyph 10 pushes, 11 pushes and acts: the 11's component value is 2,
    // stored as ligature 2, and the 10's is 1, stored as ligature 1 — the sum
    // started again.
    let mut components = vec![0u16; 10];
    components.extend([1, 2]);
    let storing = ligature_subtable(
        &[(10, 4), (11, 5)],
        &[&[0, 0, 0, 0, 1, 2], &[0, 0, 0, 0, 1, 2]],
        &[[0, 0, 0], [0, 0x8000, 0], [0, 0xA000, 0]],
        &[STORE_ACTION, LAST_ACTION],
        &components,
        &[50, 51, 52, 53],
    );
    let (out, warnings) = through(storing, &[10, 11], &Limits::DEFAULT);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(out, vec![(51, 0), (52, 1)], "each ligature its own sum");

    // Glyph 10 pushes, 11 pushes and forms ligature 60 of the two, and 12
    // pushes and forms ligature 61 of the 60 and itself.
    let mut components = vec![0u16; 64];
    components[60] = 1;
    let nesting = ligature_subtable(
        &[(10, 4), (11, 5), (12, 6)],
        &[&[0, 0, 0, 0, 1, 2, 3], &[0, 0, 0, 0, 1, 2, 3]],
        &[[0, 0, 0], [0, 0x8000, 0], [0, 0xA000, 0], [0, 0xA000, 2]],
        &[0, LAST_ACTION, 0, LAST_ACTION],
        &components,
        &[60, 61],
    );
    let (out, warnings) = through(nesting.clone(), &[10, 11, 12], &Limits::DEFAULT);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(out, vec![(61, 0)]);
    assert_eq!(glyphs_through(nesting, &[10, 11, 7]), [60, 7]);
}

/// A run holding no glyph the machine's class table names is not walked,
/// and a glyph a subtable before it deleted is class 2 to it, whatever the
/// class table says.
#[test]
fn a_ligature_machine_meets_only_the_glyphs_it_names() {
    assert_eq!(
        glyphs_through(pairing_machine(ANY_ORIENTATION), &[7, 8]),
        [7, 8]
    );
    // A noncontextual subtable deletes the 12 first: the ligature machine,
    // whose state 2 keeps its state on a deleted glyph, pairs the 10 with
    // the 11, and the deleted glyph leaves the run.
    let table = morx_table(
        2,
        &[chain(
            1,
            &[],
            &[
                noncontextual(1, &[(12, DELETED)]),
                pairing_machine(ANY_ORIENTATION),
            ],
        )],
    );
    let (out, warnings) = chained(
        &table,
        &[10, 12, 11],
        Direction::LeftToRight,
        &Selection::defaults(),
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(out, [99]);
}

// --- the glyph insertion subtable (FT-07c.9) -------------------------------
//
// Lists from the insertion glyph table inserted beside the current glyph and
// the marked one, the positions the machine keeps as glyphs go in around
// them, and the ceiling growth is held to.

/// An insertion entry's `setMark`.
const SET_MARK_INSERT: u16 = 0x8000;
/// `dontAdvance`.
const HOLD: u16 = 0x4000;
/// `currentIsKashidaLike`.
const CURRENT_KASHIDA: u16 = 0x2000;
/// `markedIsKashidaLike`.
const MARKED_KASHIDA: u16 = 0x1000;
/// `currentInsertBefore`.
const CURRENT_BEFORE: u16 = 0x0800;
/// `markedInsertBefore`.
const MARKED_BEFORE: u16 = 0x0400;

/// The flags' `currentInsertCount` field holding `n`.
const fn current_count(n: u16) -> u16 {
    n << 5
}

/// An insertion subtable for both orientations under flag 1: `classes`,
/// `lines` and `entries` — new state, flags, current insert index and marked
/// insert index — as its state table's, then its insertion glyph table after
/// the entries, `insertionActionOffset` naming where.
fn insertion_subtable(
    classes: &[(u16, u16)],
    lines: &[&[u16]],
    entries: &[[u16; 4]],
    glyphs: &[u16],
) -> Vec<u8> {
    let entries: Vec<&[u16]> = entries.iter().map(|entry| &entry[..]).collect();
    let classes_count = lines.first().map_or(4, |line| line.len());
    let mut body = stx(
        count32(classes_count),
        1,
        &lookup_single(classes),
        lines,
        &entries,
    );
    let glyphs_at = body.len();
    body.extend(u16s(glyphs));
    put32(&mut body, 16, count32(glyphs_at));
    subtable(ANY_ORIENTATION | u32::from(INSERTION), 1, &body)
}

/// The insertion subtable's worked examples: none. The chapter states the
/// flags and shows no machine (`docs/design/shaping.md`, *The manual's worked
/// examples*), so the machine is held by the readings below and
/// text-rendering-tests `MORX-29` to `MORX-36`; H-142's expectations are what
/// the count waits on.
const INSERTION_EXAMPLES: &[Worked] = &[];

/// The insertion subtable reproduces each worked example
/// `docs/design/shaping.md` lists for type 5, and there are none.
#[test]
fn insertion_reproduces_the_manuals_worked_examples() {
    assert_eq!(hold_to_worked_examples(INSERTION_EXAMPLES), 0);
}

// The glyphs of text-rendering-tests `MORX-29` to `MORX-32`'s faces: `A` to
// `D` are 2 to 5, `I` 6, `M` 7, `N` 8, `S` 9, `P` to `R` 10 to 12 and `X` to
// `Z` 13 to 15.
const IA: u16 = 2;
const IB: u16 = 3;
const IC: u16 = 4;
const ID: u16 = 5;
const II: u16 = 6;
const IM: u16 = 7;
const IN: u16 = 8;
const IS: u16 = 9;
const IP: u16 = 10;
const IQ: u16 = 11;
const IR: u16 = 12;
const IX: u16 = 13;
const IY: u16 = 14;
const IZ: u16 = 15;

/// The glyphs of a string of those faces' letters.
fn letters(text: &str) -> Vec<u16> {
    text.chars()
        .map(|c| match c {
            'A' => IA,
            'B' => IB,
            'C' => IC,
            'D' => ID,
            'I' => II,
            'M' => IM,
            'N' => IN,
            'S' => IS,
            'P' => IP,
            'Q' => IQ,
            'R' => IR,
            'X' => IX,
            'Y' => IY,
            'Z' => IZ,
            other => panic!("no glyph for {other}"),
        })
        .collect()
}

/// `MORX-29`'s machine, its flags `extra` added to each inserting entry: `M`
/// marks itself; `A` inserts `INS` before the marked glyph and `B` after it;
/// `C` inserts it before the current glyph and `D` after it.
fn twenty_nine(extra_marked: u16, extra_current: u16) -> Vec<u8> {
    insertion_subtable(
        &[(IA, 6), (IB, 7), (IC, 8), (ID, 9), (IM, 5)],
        &[&[0, 0, 0, 0, 0, 1, 2, 3, 4, 5]],
        &[
            [0, 0, NO_TABLE, NO_TABLE],
            [0, SET_MARK_INSERT, NO_TABLE, NO_TABLE],
            [0, MARKED_BEFORE | 3 | extra_marked, NO_TABLE, 0],
            [0, 3 | extra_marked, NO_TABLE, 0],
            [
                0,
                CURRENT_BEFORE | current_count(3) | extra_current,
                0,
                NO_TABLE,
            ],
            [0, current_count(3) | extra_current, 0, NO_TABLE],
        ],
        &[II, IN, IS],
    )
}

/// The glyphs, clusters and insertion kinds one subtable leaves of `glyphs`
/// over a left-to-right run, and what it refused.
fn inserted_through(
    subtable: Vec<u8>,
    glyphs: &[u16],
) -> (Vec<(u16, u32, Option<Insertion>)>, Vec<Warning>) {
    let table = morx_table(2, &[chain(1, &[], &[subtable])]);
    let morx = Morx::parse(&table).expect("a readable morx");
    let mut buffer = run_of(glyphs, Direction::LeftToRight);
    let warnings = morx.run(&mut buffer, &Selection::defaults(), &Limits::DEFAULT);
    let out = buffer
        .glyphs()
        .iter()
        .enumerate()
        .map(|(at, g)| (g.glyph, g.cluster, buffer.insertion(at)))
        .collect();
    (out, warnings)
}

/// An inserted glyph takes the cluster of the glyph it is inserted at, and
/// says how it was inserted. A kashida-like list keeps that cluster; a
/// split-vowel-like one is associated with the other glyph of its action —
/// the current glyph for a list at the mark, the marked glyph for one at the
/// current glyph — the clusters from the inserted glyphs to it merged.
#[test]
fn an_inserted_glyph_takes_the_cluster_it_is_inserted_at() {
    let k = Some(Insertion::KashidaLike);
    let v = Some(Insertion::SplitVowelLike);
    let kashida = || twenty_nine(MARKED_KASHIDA, CURRENT_KASHIDA);
    // `P M Q A`: `INS` before the marked `M`, each with the `M`'s cluster.
    let (out, warnings) = inserted_through(kashida(), &letters("PMQA"));
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        out,
        vec![
            (IP, 0, None),
            (II, 1, k),
            (IN, 1, k),
            (IS, 1, k),
            (IM, 1, None),
            (IQ, 2, None),
            (IA, 3, None),
        ]
    );
    // `P M Q B`: after the `M`.
    let (out, _) = inserted_through(kashida(), &letters("PMQB"));
    assert_eq!(
        out,
        vec![
            (IP, 0, None),
            (IM, 1, None),
            (II, 1, k),
            (IN, 1, k),
            (IS, 1, k),
            (IQ, 2, None),
            (IB, 3, None),
        ]
    );
    // `P C Q` and `P D Q`: before and after the current glyph, its cluster.
    let (out, _) = inserted_through(kashida(), &letters("PCQ"));
    assert_eq!(
        out,
        vec![
            (IP, 0, None),
            (II, 1, k),
            (IN, 1, k),
            (IS, 1, k),
            (IC, 1, None),
            (IQ, 2, None),
        ]
    );
    let (out, _) = inserted_through(kashida(), &letters("PDQ"));
    assert_eq!(
        out,
        vec![
            (IP, 0, None),
            (ID, 1, None),
            (II, 1, k),
            (IN, 1, k),
            (IS, 1, k),
            (IQ, 2, None),
        ]
    );

    // Split-vowel-like, the flags clear: `INS` at the `M` is associated with
    // the `A`, so the clusters from it to the `A` are merged.
    let split = || twenty_nine(0, 0);
    let (out, _) = inserted_through(split(), &letters("PMQA"));
    assert_eq!(
        out,
        vec![
            (IP, 0, None),
            (II, 1, v),
            (IN, 1, v),
            (IS, 1, v),
            (IM, 1, None),
            (IQ, 1, None),
            (IA, 1, None),
        ]
    );
    let (out, _) = inserted_through(split(), &letters("PMQB"));
    assert_eq!(
        out,
        vec![
            (IP, 0, None),
            (IM, 1, None),
            (II, 1, v),
            (IN, 1, v),
            (IS, 1, v),
            (IQ, 1, None),
            (IB, 1, None),
        ]
    );
    // At the current glyph, associated with the mark — here the first glyph
    // met, none having been marked — the clusters from it to the list
    // merged.
    let (out, _) = inserted_through(split(), &letters("PCQ"));
    assert_eq!(
        out,
        vec![
            (IP, 0, None),
            (II, 0, v),
            (IN, 0, v),
            (IS, 0, v),
            (IC, 1, None),
            (IQ, 2, None),
        ]
    );
    let (out, _) = inserted_through(split(), &letters("PDQ"));
    assert_eq!(
        out,
        vec![
            (IP, 0, None),
            (ID, 0, None),
            (II, 0, v),
            (IN, 0, v),
            (IS, 0, v),
            (IQ, 2, None),
        ]
    );

    // A right-to-left run, held in layout order for the chain: a list after
    // the current glyph in layout comes before it in the text, its cluster
    // the glyph's, so the clusters stay monotonic.
    let after = insertion_subtable(
        &[(10, 4)],
        &[&[0, 0, 0, 0, 1], &[0, 0, 0, 0, 1]],
        &[
            [0, 0, NO_TABLE, NO_TABLE],
            [0, current_count(1) | CURRENT_KASHIDA, 0, NO_TABLE],
        ],
        &[20],
    );
    let table = morx_table(2, &[chain(1, &[], &[after])]);
    let morx = Morx::parse(&table).expect("readable");
    let mut buffer = run_of(&[10, 11], Direction::RightToLeft);
    assert!(morx
        .run(&mut buffer, &Selection::defaults(), &Limits::DEFAULT)
        .is_empty());
    let out: Vec<(u16, u32)> = buffer
        .glyphs()
        .iter()
        .map(|g| (g.glyph, g.cluster))
        .collect();
    assert_eq!(out, vec![(20, 0), (10, 0), (11, 1)]);
}

/// The mark is a position: glyphs inserted before the marked glyph leave it
/// naming what now stands there. Held to text-rendering-tests `MORX-29` and
/// `MORX-30`, whose machine this is.
#[test]
fn the_mark_is_a_position_that_an_insertion_before_it_leaves() {
    for (input, output) in [
        ("PQRMMXXMMYYAZZ", "PQRMMXXMINSMYYAZZ"),
        ("PQRMMXXMMYYBZZ", "PQRMMXXMMINSYYBZZ"),
        ("PQRMMXXMMYYCZZ", "PQRMMXXMMYYINSCZZ"),
        ("PQRMMXXMMYYDZZ", "PQRMMXXMMYYDINSZZ"),
        ("PQRMMXXXAYYAZZ", "PQRMINSINSMXXXAYYAZZ"),
        // The second `INS` goes after the `I` the first put where the marked
        // `M` stood, not after the `M`.
        ("PQRMMXXXAYYBZZ", "PQRMIINSNSMXXXAYYBZZ"),
        ("PQRMMXXXBYYAZZ", "PQRMINSMINSXXXBYYAZZ"),
        ("PQRMMXXXBYYBZZ", "PQRMMINSINSXXXBYYBZZ"),
    ] {
        assert_eq!(
            glyphs_through(twenty_nine(0, 0), &letters(input)),
            letters(output),
            "{input}"
        );
    }
}

/// `setMark` on an inserting transition marks the position the current glyph
/// held when the transition began, before the insertion at the mark moved
/// it. Held to text-rendering-tests `MORX-31`, whose machine this is: `M`
/// marks itself, `A` inserts `INS` before the marked glyph and marks itself,
/// `B` the same after it.
#[test]
fn set_mark_on_an_inserting_transition_marks_where_the_glyph_stood() {
    let machine = || {
        insertion_subtable(
            &[(IA, 6), (IB, 7), (IM, 5)],
            &[&[0, 0, 0, 0, 0, 1, 2, 3]],
            &[
                [0, 0, NO_TABLE, NO_TABLE],
                [0, SET_MARK_INSERT, NO_TABLE, NO_TABLE],
                [0, SET_MARK_INSERT | MARKED_BEFORE | 3, NO_TABLE, 0],
                [0, SET_MARK_INSERT | 3, NO_TABLE, 0],
            ],
            &[II, IN, IS],
        )
    };
    for (input, output) in [
        ("XXAYYAZZ", "ININSSXXAYYAZZ"),
        ("XXAYYBYY", "INSINSXXAYYBYY"),
        ("XXBYYAZZ", "XIINSNSXBYYAZZ"),
        ("XXBYYBZZ", "XININSSXBYYBZZ"),
        ("MPQRAXYZA", "INSMINSPQRAXYZA"),
        ("MPQRAXYZB", "INSMPINSQRAXYZB"),
        ("MPQRBXYZA", "MINSINSPQRBXYZA"),
        ("MPQRBXYZB", "MINSPINSQRBXYZB"),
    ] {
        assert_eq!(
            glyphs_through(machine(), &letters(input)),
            letters(output),
            "{input}"
        );
    }
}

/// Where no glyph has been marked, the mark is the first glyph the machine
/// met, and an insertion at the mark goes beside it (`MORX-32`).
#[test]
fn an_insertion_at_no_mark_goes_beside_the_first_glyph_met() {
    let machine = || {
        insertion_subtable(
            &[(IA, 5), (IB, 6)],
            &[&[0, 0, 0, 0, 0, 1, 2]],
            &[
                [0, 0, NO_TABLE, NO_TABLE],
                [0, MARKED_BEFORE | 3, NO_TABLE, 0],
                [0, 3, NO_TABLE, 0],
            ],
            &[II, IN, IS],
        )
    };
    for (input, output) in [
        ("A", "INSA"),
        ("XAY", "INSXAY"),
        ("B", "BINS"),
        ("XBY", "XINSBY"),
    ] {
        assert_eq!(
            glyphs_through(machine(), &letters(input)),
            letters(output),
            "{input}"
        );
    }
}

/// Without `dontAdvance` the machine goes on past the current glyph and the
/// glyphs inserted after it, so an insertion is not read again (`MORX-33`);
/// with it the machine stays at its position, meeting the current glyph
/// again where the list went after it (`MORX-35`) and the first glyph
/// inserted where the list went before it.
#[test]
fn dont_advance_meets_the_position_again_and_an_insertion_is_not_read_again() {
    // `MORX-33`: `h` moves to state 1, where `a` inserts `ha` after itself;
    // `a` is 2 and `h` 3.
    let twice = || {
        insertion_subtable(
            &[(2, 5), (3, 4)],
            &[&[0, 0, 0, 0, 1, 0], &[0, 0, 0, 0, 0, 2]],
            &[
                [0, 0, NO_TABLE, NO_TABLE],
                [1, 0, NO_TABLE, NO_TABLE],
                [0, current_count(2), 0, NO_TABLE],
            ],
            &[3, 2],
        )
    };
    assert_eq!(glyphs_through(twice(), &[3, 2]), [3, 2, 3, 2]);
    assert_eq!(
        glyphs_through(twice(), &[3, 2, 3, 2]),
        [3, 2, 3, 2, 3, 2, 3, 2]
    );
    assert_eq!(glyphs_through(twice(), &[2, 3]), [2, 3]);

    // `MORX-35`: in state 0 `A` inserts `C` after itself and holds, moving to
    // state 1, where `A` inserts `B` after itself; `B` and `C` insert `E`.
    // `A` to `E` are 2 to 6, `X` and `Y` 7 and 8.
    let holding = || {
        insertion_subtable(
            &[(2, 4), (3, 5), (4, 5)],
            &[&[0, 0, 0, 0, 1, 2], &[0, 0, 0, 0, 3, 4]],
            &[
                [0, 0, NO_TABLE, NO_TABLE],
                [1, HOLD | current_count(1), 1, NO_TABLE],
                [0, current_count(1), 2, NO_TABLE],
                [0, current_count(1), 0, NO_TABLE],
                [1, current_count(1), 2, NO_TABLE],
            ],
            &[3, 4, 6],
        )
    };
    assert_eq!(glyphs_through(holding(), &[2]), [2, 3, 4, 6]);
    assert_eq!(glyphs_through(holding(), &[7, 2, 8]), [7, 2, 3, 4, 6, 8]);

    // A list before the current glyph, held: the machine meets the first
    // glyph inserted, 20, whose entry inserts 30 after it; had it met the 10
    // again, its entry would have inserted 40.
    let before = insertion_subtable(
        &[(10, 4), (20, 5)],
        &[
            &[0, 0, 0, 0, 1, 0],
            &[0, 0, 0, 0, 3, 2],
            &[4, 4, 4, 4, 4, 4],
        ],
        &[
            [0, 0, NO_TABLE, NO_TABLE],
            [1, HOLD | CURRENT_BEFORE | current_count(1), 0, NO_TABLE],
            [2, current_count(1), 1, NO_TABLE],
            [2, current_count(1), 2, NO_TABLE],
            [2, 0, NO_TABLE, NO_TABLE],
        ],
        &[20, 30, 40],
    );
    assert_eq!(glyphs_through(before, &[10]), [20, 30, 10]);
}

/// A glyph inserted on the side the walk goes on to is passed over, forward
/// or backward: a 10 inserting a 10 beside itself inserts one, however the
/// machine walks.
#[test]
fn an_insertion_is_not_read_again_whichever_way_the_walk_goes() {
    let machine = |coverage: u32, flags: u16| {
        let mut bytes = insertion_subtable(
            &[(10, 4)],
            &[&[0, 0, 0, 0, 1], &[0, 0, 0, 0, 1]],
            &[
                [0, 0, NO_TABLE, NO_TABLE],
                [0, flags | current_count(1), 0, NO_TABLE],
            ],
            &[10],
        );
        bytes[4..8].copy_from_slice(&(coverage | u32::from(INSERTION)).to_be_bytes());
        bytes
    };
    // Forward, after; backward, before: the side the walk goes on to.
    assert_eq!(glyphs_through(machine(ANY_ORIENTATION, 0), &[10]), [10, 10]);
    assert_eq!(
        glyphs_through(machine(ANY_ORIENTATION | DESCENDING, CURRENT_BEFORE), &[10]),
        [10, 10]
    );
    // Two glyphs, each inserting once, in both walks and on both sides.
    for (coverage, flags) in [
        (ANY_ORIENTATION, 0),
        (ANY_ORIENTATION, CURRENT_BEFORE),
        (ANY_ORIENTATION | DESCENDING, 0),
        (ANY_ORIENTATION | DESCENDING, CURRENT_BEFORE),
    ] {
        assert_eq!(
            glyphs_through(machine(coverage, flags), &[10, 7, 10]),
            [10, 10, 7, 10, 10],
            "coverage {coverage:#x}, flags {flags:#x}"
        );
    }
}

/// An insertion that would take the run past `Limits::max_glyphs` is refused
/// and named, and the machine goes on; a chain of subtables each multiplying
/// the run stops growing at the ceiling, each subtable that reached it named
/// once.
#[test]
fn an_insertion_past_the_glyph_ceiling_is_refused_and_named() {
    let triple = || {
        insertion_subtable(
            &[(10, 4)],
            &[&[0, 0, 0, 0, 1], &[0, 0, 0, 0, 1]],
            &[
                [0, 0, NO_TABLE, NO_TABLE],
                [0, current_count(3), 0, NO_TABLE],
            ],
            &[20, 21, 22],
        )
    };
    let limits = Limits {
        max_glyphs: 8,
        ..Limits::DEFAULT
    };
    let (out, warnings) = through(triple(), &[10, 10, 10], &limits);
    assert_eq!(
        out.iter().map(|(g, _)| *g).collect::<Vec<_>>(),
        vec![10, 20, 21, 22, 10, 10]
    );
    assert_eq!(
        warnings,
        vec![Warning::InsertionBudgetExceeded { site: site(0) }]
    );
    // Exactly at the ceiling is not past it.
    let limits = Limits {
        max_glyphs: 6,
        ..Limits::DEFAULT
    };
    let (out, warnings) = through(triple(), &[10, 7, 7], &limits);
    assert_eq!(out.len(), 6);
    assert!(warnings.is_empty(), "{warnings:?}");

    // `MORX-34`'s billion laughs in four subtables: each `a` inserts nine
    // `ha` after itself, so `ha` becomes 20 glyphs, then 200, then 992 of a
    // ceiling of 1 000, the forty-fifth insertion of the third subtable and
    // every one of the fourth refused.
    let laugh = || {
        insertion_subtable(
            &[(2, 4)],
            &[&[0, 0, 0, 0, 1]],
            &[
                [0, 0, NO_TABLE, NO_TABLE],
                [0, current_count(18), 0, NO_TABLE],
            ],
            &[3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2],
        )
    };
    let table = morx_table(2, &[chain(1, &[], &[laugh(), laugh(), laugh(), laugh()])]);
    let limits = Limits {
        max_glyphs: 1_000,
        ..Limits::for_glyphs(2)
    };
    let (out, warnings) = chained_under(
        &table,
        &[3, 2],
        Direction::LeftToRight,
        &Selection::defaults(),
        &limits,
    );
    assert_eq!(out.len(), 992);
    assert!(
        out.chunks(2).all(|pair| pair == [3, 2]),
        "every glyph a laugh"
    );
    assert_eq!(
        warnings,
        vec![
            Warning::InsertionBudgetExceeded { site: site(2) },
            Warning::InsertionBudgetExceeded { site: site(3) },
        ]
    );

    // `MORX-36`: inserting after itself and holding, for ever, stops at the
    // transition ceiling, a glyph inserted at each transition.
    let forever = insertion_subtable(
        &[(2, 4)],
        &[&[0, 0, 0, 0, 1], &[0, 0, 0, 0, 1]],
        &[
            [0, 0, NO_TABLE, NO_TABLE],
            [0, HOLD | current_count(1), 0, NO_TABLE],
        ],
        &[3],
    );
    let limits = Limits {
        max_transitions: 50,
        ..Limits::DEFAULT
    };
    let (out, warnings) = through(forever, &[2], &limits);
    assert_eq!(out.len(), 51);
    assert_eq!(
        warnings,
        vec![Warning::TransitionBudgetExceeded { site: site(0) }]
    );
}

/// A subtable with no insertion glyph table, or with one its offset puts past
/// its end, is named and inserts nothing; a list the table does not hold is
/// named and not inserted, the machine going on; and the end of text inserts
/// beside the last glyph met.
#[test]
fn an_insertion_subtable_that_cannot_be_read_is_named() {
    let malformed = Warning::MalformedAatSubtable { site: site(0) };
    let headless = subtable(
        ANY_ORIENTATION | u32::from(INSERTION),
        1,
        &u32s(&[4, 0, 0, 0]),
    );
    let (out, warnings) = through(headless, &[0], &Limits::DEFAULT);
    assert_eq!(out, vec![(0, 0)]);
    assert_eq!(warnings, vec![malformed]);

    let mut past = twenty_nine(0, 0);
    let body = past.len() - 12;
    put32(&mut past, 12 + 16, count32(body + 1));
    let (out, warnings) = through(past, &letters("PMQA"), &Limits::DEFAULT);
    assert_eq!(
        out.iter().map(|(g, _)| *g).collect::<Vec<_>>(),
        letters("PMQA")
    );
    assert_eq!(warnings, vec![malformed]);

    // A list of four from index 1 of a table of four, which ends the
    // subtable, then a good one.
    let short = insertion_subtable(
        &[(10, 4), (11, 5)],
        &[&[0, 0, 0, 0, 1, 2], &[0, 0, 0, 0, 1, 2]],
        &[
            [0, 0, NO_TABLE, NO_TABLE],
            [0, current_count(4), 1, NO_TABLE],
            [0, current_count(2), 0, NO_TABLE],
        ],
        &[20, 21, 22, 23],
    );
    let (out, warnings) = through(short, &[10, 11], &Limits::DEFAULT);
    assert_eq!(
        out.iter().map(|(g, _)| *g).collect::<Vec<_>>(),
        vec![10, 11, 20, 21]
    );
    assert_eq!(warnings, vec![malformed]);

    // `X A`: the `A` moves to state 1, whose end of text inserts 30 after
    // the last glyph met, with its cluster.
    let at_the_end = insertion_subtable(
        &[(2, 4)],
        &[&[0, 0, 0, 0, 1], &[0, 0, 0, 0, 1], &[2, 1, 1, 1, 1]],
        &[
            [0, 0, NO_TABLE, NO_TABLE],
            [2, 0, NO_TABLE, NO_TABLE],
            [0, current_count(1) | CURRENT_KASHIDA, 0, NO_TABLE],
        ],
        &[30],
    );
    let (out, warnings) = through(at_the_end, &[7, 2], &Limits::DEFAULT);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(out, vec![(7, 0), (2, 1), (30, 1)]);
}

// --- kerx: the pair formats (FT-07c.11) ------------------------------------
//
// The extended kerning table's header and subtable coverage, and the three
// pair formats, run in the positioning stage of a face shaped through its AAT
// tables: along the line into the left glyph's advance, across it into the
// right glyph's `y_offset`.

/// The kerning face's glyphs: `A`, `V`, `T`, `o` and `x`, then alef and bet.
const KA: u16 = 1;
/// See [`KA`].
const KV: u16 = 2;
/// See [`KA`].
const KT: u16 = 3;
/// See [`KA`].
const KO: u16 = 4;
/// See [`KA`].
const KX: u16 = 5;
/// See [`KA`].
const KALEF: u16 = 6;
/// See [`KA`].
const KBET: u16 = 7;

/// A face of eight glyphs — `.notdef`, `A`, `V`, `T`, `o`, `x`, alef and bet
/// — carrying no table but those given.
fn kerning_face() -> Face {
    Face::new(
        8,
        &[
            ('A', KA),
            ('V', KV),
            ('T', KT),
            ('o', KO),
            ('x', KX),
            ('\u{5D0}', KALEF),
            ('\u{5D1}', KBET),
            ('\u{6F22}', KX),
        ],
    )
}

/// A `kerx` table of `version` holding `subtables`.
fn kerx_table(version: u16, subtables: &[Vec<u8>]) -> Vec<u8> {
    let mut out = u16s(&[version, 0]);
    out.extend(count32(subtables.len()).to_be_bytes());
    for subtable in subtables {
        out.extend_from_slice(subtable);
    }
    out
}

/// One `kerx` subtable: its coverage, whose low byte is its format, its
/// `tupleCount` and its body, padded to four bytes.
fn kerx_subtable(coverage: u32, tuple_count: u32, body: &[u8]) -> Vec<u8> {
    let mut padded = body.to_vec();
    while padded.len() % 4 != 0 {
        padded.push(0);
    }
    let mut out = u32s(&[count32(12 + padded.len()), coverage, tuple_count]);
    out.extend(padded);
    out
}

/// A format 0 subtable's body: `nPairs` and the three search fields, then
/// the pairs, sorted by the key their two glyphs make.
fn kerx_pairs(pairs: &[(u16, u16, i16)]) -> Vec<u8> {
    let mut sorted = pairs.to_vec();
    sorted.sort_by_key(|(left, right, _)| (*left, *right));
    let units = count16(sorted.len());
    let [range, selector, shift] = search_fields(6, units);
    let mut out = u32s(&[
        u32::from(units),
        u32::from(range),
        u32::from(selector),
        u32::from(shift),
    ]);
    for (left, right, value) in &sorted {
        out.extend(u16s(&[*left, *right]));
        out.extend(value.to_be_bytes());
    }
    out
}

/// A format 2 subtable's body: `left` and `right` give glyphs their row and
/// column, and `rows` the kerning array, two-byte values a cell. The class
/// tables hold each class as the byte offset of its row or column — a row
/// pre-multiplied by the row's width — and every offset is from the start of
/// the subtable, twelve bytes before the body.
fn kerx_class_array(left: &[(u16, u16)], right: &[(u16, u16)], rows: &[&[i16]]) -> Vec<u8> {
    let width = count16(rows.first().map_or(0, |row| row.len()) * 2);
    let left_table = lookup_single(
        &left
            .iter()
            .map(|(glyph, row)| (*glyph, row * width))
            .collect::<Vec<_>>(),
    );
    let right_table = lookup_single(
        &right
            .iter()
            .map(|(glyph, column)| (*glyph, column * 2))
            .collect::<Vec<_>>(),
    );
    let left_at = 12 + 16;
    let right_at = left_at + left_table.len();
    let array_at = right_at + right_table.len();
    let mut out = u32s(&[
        u32::from(width),
        count32(left_at),
        count32(right_at),
        count32(array_at),
    ]);
    out.extend(left_table);
    out.extend(right_table);
    for row in rows {
        for value in *row {
            out.extend(value.to_be_bytes());
        }
    }
    out
}

/// A format 6 subtable's body: `rows` and `columns` give glyphs their row
/// and column index, the row's pre-multiplied by the column count, and
/// `cells` the kerning array, a row at a time; two-byte values, or four
/// where `long` sets `valuesAreLong`, in the index tables and the array.
fn kerx_index_array(
    rows: &[(u16, u64)],
    columns: &[(u16, u64)],
    cells: &[&[i32]],
    long: bool,
) -> Vec<u8> {
    let size: u16 = if long { 4 } else { 2 };
    let column_count = count16(cells.first().map_or(0, |row| row.len()));
    let premultiplied: Vec<(u16, u64)> = rows
        .iter()
        .map(|(glyph, row)| (*glyph, row * u64::from(column_count)))
        .collect();
    let row_table = lookup_pairs(size, &premultiplied);
    let column_table = lookup_pairs(size, columns);
    let rows_at = 12 + 20;
    let columns_at = rows_at + row_table.len();
    let array_at = columns_at + column_table.len();
    let mut out = u32s(&[u32::from(long)]);
    out.extend(u16s(&[count16(cells.len()), column_count]));
    out.extend(u32s(&[
        count32(rows_at),
        count32(columns_at),
        count32(array_at),
    ]));
    out.extend(row_table);
    out.extend(column_table);
    for row in cells {
        for value in *row {
            if long {
                out.extend(value.to_be_bytes());
            } else {
                out.extend(i16::try_from(*value).expect("short").to_be_bytes());
            }
        }
    }
    out
}

/// `kerx` subtable `subtable`'s site.
fn kerx_site(subtable: u32) -> Site {
    Site {
        table: Tag::new(b"kerx"),
        chain: 0,
        subtable,
    }
}

/// What `text` shapes to along a line through a face carrying `kerx`, and
/// what the runs said.
fn kerned(face: &Face, text: &str) -> (Vec<ShapedGlyph>, Vec<Warning>) {
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let runs = line_runs(&Shaper::new(&sfnt), text);
    let warnings = runs
        .iter()
        .flat_map(|run| run.warnings().iter().copied())
        .collect();
    (glyphs(&runs), warnings)
}

/// The kerning face's glyphs of `text`, each with its own advance moved by
/// the change `moved` gives its position.
fn moved_by(face: &Face, text: &str, moved: &[i32]) -> Vec<ShapedGlyph> {
    text.chars()
        .zip(clusters(text))
        .zip(moved)
        .map(|((c, cluster), by)| {
            let mut glyph = face.plain(face.glyph(c), cluster);
            glyph.x_advance += by;
            glyph
        })
        .collect()
}

/// Each pair format kerns the pairs it holds — `AV` by −80 and `To` by −50 —
/// each pair's first advance moved by its value, along a line, and leaves
/// every other pair as it was; two subtables kerning one pair add.
#[test]
fn each_kerx_pair_format_kerns_its_pairs() {
    let pairs = kerx_subtable(0, 0, &kerx_pairs(&[(KA, KV, -80), (KT, KO, -50)]));
    let classes = kerx_subtable(
        2,
        0,
        &kerx_class_array(
            &[(KA, 1), (KT, 2)],
            &[(KV, 1), (KO, 2)],
            &[&[0, 0, 0], &[0, -80, 0], &[0, 0, -50]],
        ),
    );
    let indices = |long: bool| {
        kerx_subtable(
            6,
            0,
            &kerx_index_array(
                &[(KA, 1), (KT, 2)],
                &[(KV, 1), (KO, 2)],
                &[&[0, 0, 0], &[0, -80, 0], &[0, 0, -50]],
                long,
            ),
        )
    };
    let text = "AVTox";
    for (name, subtable) in [
        ("format 0", pairs.clone()),
        ("format 2", classes.clone()),
        ("format 6", indices(false)),
        ("format 6, long values", indices(true)),
    ] {
        let face = kerning_face().with(b"kerx", kerx_table(2, &[subtable]));
        let (out, warnings) = kerned(&face, text);
        assert!(warnings.is_empty(), "{name}: {warnings:?}");
        assert_eq!(out, moved_by(&face, text, &[-80, 0, -50, 0, 0]), "{name}");
        // `VA`, `oT` and `xA`: pairs the tables do not hold.
        let (out, _) = kerned(&face, "VAoTxA");
        assert_eq!(out, moved_by(&face, "VAoTxA", &[0; 6]), "{name}");
    }
    // Two subtables, additive; versions 3 and 4 alike.
    for version in [2u16, 3, 4] {
        let face = kerning_face().with(
            b"kerx",
            kerx_table(version, &[pairs.clone(), classes.clone()]),
        );
        let (out, warnings) = kerned(&face, "AV");
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(out, moved_by(&face, "AV", &[-160, 0]), "version {version}");
    }
}

/// A cross-stream subtable moves the right glyph of each pair up by its
/// value, that glyph alone, and changes no advance.
#[test]
fn a_cross_stream_subtable_moves_glyphs_off_the_baseline() {
    let face = kerning_face().with(
        b"kerx",
        kerx_table(
            2,
            &[kerx_subtable(
                CROSS_STREAM,
                0,
                &kerx_pairs(&[(KA, KV, 30), (KV, KT, -12)]),
            )],
        ),
    );
    let (out, warnings) = kerned(&face, "AVTo");
    assert!(warnings.is_empty(), "{warnings:?}");
    let mut expected = moved_by(&face, "AVTo", &[0, 0, 0, 0]);
    expected[1].y_offset = 30;
    expected[2].y_offset = -12;
    assert_eq!(out, expected);
}

/// `kern` set to 0 is `font-kerning: none`, and turns `kerx` off; a caller
/// naming its own features kerns only where it names `kern`, and a setting
/// above 0 turns it back on.
#[test]
fn kern_set_to_zero_turns_kerx_off() {
    let face = kerning_face().with(
        b"kerx",
        kerx_table(2, &[kerx_subtable(0, 0, &kerx_pairs(&[(KA, KV, -80)]))]),
    );
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let kern = Tag::new(b"kern");
    let none: [Tag; 0] = [];
    let named = [kern];
    let off = [(kern, 0u32)];
    let on = [(kern, 1u32)];
    let later = [(kern, 0u32), (kern, 3)];
    let advance = |shaper: &Shaper<'_>| {
        let runs = line_runs(shaper, "AV");
        assert!(runs.iter().all(|run| run.warnings().is_empty()));
        glyphs(&runs)[0].x_advance
    };
    let plain = i32::from(face.advances[usize::from(KA)]);
    assert_eq!(advance(&Shaper::new(&sfnt)), plain - 80);
    assert_eq!(advance(&Shaper::new(&sfnt).with_settings(&off)), plain);
    assert_eq!(
        advance(&Shaper::new(&sfnt).with_settings(&later)),
        plain - 80
    );
    assert_eq!(
        advance(&Shaper::new(&sfnt).with_features(&none, &none)),
        plain
    );
    assert_eq!(
        advance(&Shaper::new(&sfnt).with_features(&none, &named)),
        plain - 80
    );
    assert_eq!(
        advance(&Shaper::new(&sfnt).with_features(&named, &none)),
        plain - 80
    );
    assert_eq!(
        advance(
            &Shaper::new(&sfnt)
                .with_features(&none, &none)
                .with_settings(&on)
        ),
        plain - 80
    );
    assert_eq!(
        advance(
            &Shaper::new(&sfnt)
                .with_features(&none, &named)
                .with_settings(&off)
        ),
        plain
    );
}

/// A subtable of a version-4 table whose `tupleCount` is above 0 holds
/// vectors for a variable face, and is refused by name, kerning nothing; the
/// count means nothing in a table of version 2 or 3.
#[test]
fn a_variation_tuple_is_refused_by_name() {
    let body = kerx_pairs(&[(KA, KV, -80)]);
    let face = kerning_face().with(
        b"kerx",
        kerx_table(
            4,
            &[
                kerx_subtable(KERX_VARIATION, 2, &body),
                kerx_subtable(0, 0, &kerx_pairs(&[(KT, KO, -50)])),
            ],
        ),
    );
    let (out, warnings) = kerned(&face, "AVTo");
    assert_eq!(
        warnings,
        vec![Warning::VariationKerningNotRun { site: kerx_site(0) }]
    );
    assert_eq!(out, moved_by(&face, "AVTo", &[0, 0, -50, 0]));
    for version in [2u16, 3] {
        let face = kerning_face().with(b"kerx", kerx_table(version, &[kerx_subtable(0, 2, &body)]));
        let (out, warnings) = kerned(&face, "AV");
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(out, moved_by(&face, "AV", &[-80, 0]), "version {version}");
    }
}

/// A subtable of vertical kerning values runs in no line and names nothing
/// there; in an upright run it is refused by name, and a horizontal subtable
/// is not for the run.
#[test]
fn a_vertical_subtable_runs_in_no_line_and_is_refused_in_a_column() {
    let face = kerning_face().with(
        b"kerx",
        kerx_table(
            2,
            &[
                kerx_subtable(
                    KERX_VERTICAL,
                    0,
                    &kerx_pairs(&[(KA, KV, -80), (KX, KX, -9)]),
                ),
                kerx_subtable(0, 0, &kerx_pairs(&[(KT, KO, -50), (KX, KX, -7)])),
            ],
        ),
    );
    let (out, warnings) = kerned(&face, "AVTo");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(out, moved_by(&face, "AVTo", &[0, 0, -50, 0]));

    // The face maps U+6F22 to its `x`, so the column's upright run holds two
    // of them.
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let runs = Shaper::new(&sfnt)
        .shape_text_vertical("\u{6F22}\u{6F22}", BaseDirection::Auto)
        .1;
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].orientation(), Orientation::Upright);
    assert_eq!(
        runs[0].warnings(),
        &[
            Warning::VerticalMetricsSynthesized,
            Warning::VerticalKerningNotRun { site: kerx_site(0) },
        ]
    );
    let upright: Vec<i32> = runs[0].glyphs().iter().map(|g| g.x_advance).collect();
    assert_eq!(upright, vec![0, 0], "nothing kerned down the column");
}

/// A right-to-left run is kerned in layout order: its pair is the bet and
/// the alef after it on the line, the bet's advance moved, and a pair the
/// table holds in the text's order is no pair of the line.
#[test]
fn a_right_to_left_run_is_kerned_in_layout_order() {
    let face = kerning_face().with(
        b"kerx",
        kerx_table(
            2,
            &[kerx_subtable(
                0,
                0,
                &kerx_pairs(&[(KBET, KALEF, -60), (KALEF, KBET, -5)]),
            )],
        ),
    );
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let (_, runs) = Shaper::new(&sfnt).shape_text("\u{5D0}\u{5D1}", BaseDirection::Auto);
    assert!(!runs[0].direction().is_forward());
    assert_eq!(
        glyphs(&runs),
        moved_by(&face, "\u{5D0}\u{5D1}", &[0, -60]),
        "the bet, left of the alef on the line, moved"
    );
}

/// The precedence: `kerx` kerns a face shaped through its AAT tables — with
/// a `morx` or with none — and no face carrying `GSUB` or `GPOS`.
#[test]
fn kerx_runs_under_the_precedence_the_design_states() {
    let kerx = || kerx_table(2, &[kerx_subtable(0, 0, &kerx_pairs(&[(KA, KV, -80)]))]);
    let gpos = || {
        layout_table(
            &[(*b"kern", vec![0])],
            &[lookup(2, &[pair_kern(KA, KV, -40)])],
        )
    };
    let gsub = || {
        layout_table(
            &[(*b"liga", vec![0])],
            &[lookup(4, &[ligature_subst(KT, &[KO], KX)])],
        )
    };
    // `kerx` alone, and beside a `morx` that maps `x` to itself.
    let alone = kerning_face().with(b"kerx", kerx());
    assert_eq!(kerned(&alone, "AV").0, moved_by(&alone, "AV", &[-80, 0]));
    let chained_face = alone.clone().with(
        b"morx",
        morx_table(2, &[chain(1, &[], &[noncontextual(1, &[(KX, KX)])])]),
    );
    let (out, warnings) = kerned(&chained_face, "AV");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(out, moved_by(&chained_face, "AV", &[-80, 0]));
    // `GPOS` beside it: the `GPOS` kerning alone.
    let with_gpos = alone.clone().with(b"GPOS", gpos());
    assert_eq!(
        kerned(&with_gpos, "AV").0,
        moved_by(&with_gpos, "AV", &[-40, 0])
    );
    // `GSUB` and no `GPOS`: the OpenType path, which kerns nothing.
    let with_gsub = alone.with(b"GSUB", gsub());
    let (out, warnings) = kerned(&with_gsub, "AV");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(out, moved_by(&with_gsub, "AV", &[0, 0]));
}

/// A `kerx` this crate cannot read is named on every run, which is not
/// kerned; a subtable the table cannot hold ends the walk, named, the ones
/// before it run; a format the chapter does not define, or the state-machine
/// format not run, is skipped and named, as a contextual kerning subtable
/// whose state table cannot be read is; and past `Limits::max_subtables`
/// nothing more is read.
#[test]
fn a_kerx_table_or_subtable_that_cannot_be_read_is_named() {
    let good = || kerx_subtable(0, 0, &kerx_pairs(&[(KA, KV, -80)]));
    for unreadable in [u16s(&[1, 0, 0, 0]), u16s(&[5, 0, 0, 0]), u16s(&[2, 0])] {
        let face = kerning_face().with(b"kerx", unreadable);
        let (out, warnings) = kerned(&face, "AV");
        assert_eq!(out, moved_by(&face, "AV", &[0, 0]));
        assert_eq!(
            warnings,
            vec![Warning::TableNotRun {
                table: Tag::new(b"kerx")
            }]
        );
    }

    // A subtable claiming more than the table holds, and one shorter than
    // its header.
    for length in [4_000u32, 8] {
        let mut bad = kerx_subtable(0, 0, &kerx_pairs(&[(KT, KO, -50)]));
        bad[0..4].copy_from_slice(&length.to_be_bytes());
        let face = kerning_face().with(b"kerx", kerx_table(2, &[good(), bad, good()]));
        let (out, warnings) = kerned(&face, "AVTo");
        assert_eq!(
            out,
            moved_by(&face, "AVTo", &[-80, 0, 0, 0]),
            "length {length}"
        );
        assert_eq!(
            warnings,
            vec![Warning::MalformedAatSubtable { site: kerx_site(1) }]
        );
    }

    // Formats 3, 5 and 7, undefined, and 4, a state machine not run.
    for format in [3u8, 5, 7, 255, 4] {
        let face = kerning_face().with(
            b"kerx",
            kerx_table(2, &[kerx_subtable(u32::from(format), 0, &[0; 32]), good()]),
        );
        let (out, warnings) = kerned(&face, "AV");
        assert_eq!(out, moved_by(&face, "AV", &[-80, 0]), "format {format}");
        assert_eq!(
            warnings,
            vec![Warning::SubtableTypeNotRun {
                site: kerx_site(0),
                kind: format
            }],
            "format {format}"
        );
    }
    // Format 1 runs, and a zeroed body is no state table it can read.
    let face = kerning_face().with(
        b"kerx",
        kerx_table(2, &[kerx_subtable(1, 0, &[0; 32]), good()]),
    );
    let (out, warnings) = kerned(&face, "AV");
    assert_eq!(out, moved_by(&face, "AV", &[-80, 0]));
    assert_eq!(
        warnings,
        vec![Warning::MalformedAatSubtable { site: kerx_site(0) }]
    );

    // The subtable ceiling, read through the table directly.
    let table = kerx_table(2, &[good(), good(), good()]);
    let kerx = Kerx::parse(&table).expect("readable");
    assert_eq!((kerx.version(), kerx.subtable_count()), (2, 3));
    let limits = Limits {
        max_subtables: 2,
        ..Limits::DEFAULT
    };
    let (subtables, warnings) = kerx.subtables(&limits);
    assert_eq!(subtables.len(), 2);
    assert_eq!(
        warnings,
        vec![Warning::SubtableBudgetExceeded {
            table: Tag::new(b"kerx")
        }]
    );
    let mut buffer = Buffer::from_glyphs(&[KA, KV]);
    let warnings = kerx.run(&mut buffer, &limits);
    assert_eq!(buffer.glyphs()[0].x_advance, -160, "two subtables of three");
    assert_eq!(
        warnings,
        vec![Warning::SubtableBudgetExceeded {
            table: Tag::new(b"kerx")
        }]
    );
    let first = subtables[0];
    assert_eq!(
        (
            first.index(),
            first.format(),
            first.coverage,
            first.tuple_count
        ),
        (0, 0, 0, 0)
    );
    assert_eq!(first.bytes().len(), good().len());
}

/// A cell the subtable's values point past, and a class table of a format
/// the manual does not define, are named, and kern nothing.
#[test]
fn a_pair_table_pointing_past_itself_is_named() {
    let malformed = Warning::MalformedAatSubtable { site: kerx_site(0) };
    // Format 2 whose `V` column is offset 400 bytes into a three-cell row.
    let mut classes = kerx_class_array(&[(KA, 1)], &[(KO, 1)], &[&[0, 0], &[0, -30]]);
    let right_at = usize::try_from(u32::from_be_bytes(
        classes[8..12].try_into().expect("four bytes"),
    ))
    .expect("small")
        - 12;
    // The right table is a format 6 lookup: its one unit's value sits 14
    // bytes in.
    classes[right_at + 14..right_at + 16].copy_from_slice(&400u16.to_be_bytes());
    let face = kerning_face().with(b"kerx", kerx_table(2, &[kerx_subtable(2, 0, &classes)]));
    let (out, warnings) = kerned(&face, "Ao");
    assert_eq!(out, moved_by(&face, "Ao", &[0, 0]));
    assert_eq!(warnings, vec![malformed]);
    // Format 6 whose `o` column index runs past the two cells of a row.
    let indices = kerx_index_array(&[(KA, 1)], &[(KO, 9)], &[&[0, 0], &[0, -30]], false);
    let face = kerning_face().with(b"kerx", kerx_table(2, &[kerx_subtable(6, 0, &indices)]));
    let (out, warnings) = kerned(&face, "Ao");
    assert_eq!(out, moved_by(&face, "Ao", &[0, 0]));
    assert_eq!(warnings, vec![malformed]);
    // A class table of format 3.
    let mut odd = kerx_class_array(&[(KA, 1)], &[(KO, 1)], &[&[0, 0], &[0, -30]]);
    let left_at = usize::try_from(u32::from_be_bytes(
        odd[4..8].try_into().expect("four bytes"),
    ))
    .expect("small")
        - 12;
    odd[left_at..left_at + 2].copy_from_slice(&3u16.to_be_bytes());
    let face = kerning_face().with(b"kerx", kerx_table(2, &[kerx_subtable(2, 0, &odd)]));
    let (_, warnings) = kerned(&face, "Ao");
    assert_eq!(
        warnings,
        vec![Warning::UnknownLookupFormat {
            site: kerx_site(0),
            format: 3
        }]
    );
}

// --- kerx: contextual kerning (FT-07c.12) ----------------------------------
//
// Format 1's state machine, on the driver every AAT machine runs on: its
// entries clear the kerning stack, push the current glyph on it and start a
// value list, each value popping one glyph and kerning it — along the line
// into its advance, across it into its `y_offset`.

/// Format 1's `push`: the current glyph onto the kerning stack.
const PUSH: u16 = 0x8000;
/// Format 1's `reset`: the kerning stack cleared.
const RESET: u16 = 0x2000;
/// A format 1 value index naming no value list.
const NO_VALUES: u16 = 0xFFFF;

/// A format 1 subtable's body: an extended state table — `classes` giving
/// glyphs their classes, `lines` the state array, and `entries` each entry's
/// new state, flags and value index — whose `valueTable` offset, from the
/// state table's start, names `values`, the value lists one after another.
fn kerx_contextual(
    classes: &[(u16, u16)],
    lines: &[&[u16]],
    entries: &[(u16, u16, u16)],
    values: &[u16],
) -> Vec<u8> {
    let entries: Vec<Vec<u16>> = entries
        .iter()
        .map(|(state, flags, index)| vec![*state, *flags, *index])
        .collect();
    let entries: Vec<&[u16]> = entries.iter().map(Vec::as_slice).collect();
    let class_count = lines.first().map_or(4, |line| line.len());
    let mut out = stx(
        count32(class_count),
        1,
        &lookup_single(classes),
        lines,
        &entries,
    );
    let values_at = count32(out.len());
    put32(&mut out, 16, values_at);
    out.extend(u16s(values));
    out
}

/// A format 1 value list: `values` in font units, then the `0xFFFF` that
/// ends it.
fn value_list(values: &[i16]) -> Vec<u16> {
    values
        .iter()
        .map(|value| u16::from_be_bytes(value.to_be_bytes()))
        .chain([0xFFFF])
        .collect()
}

/// One worked example of the manual's for a `kerx` state machine: where it
/// is, the whole subtable it transcribes, and each case's glyphs with the
/// advance each leaves with, over a left-to-right run whose advances start
/// at zero.
type WorkedKerning = (
    &'static str,
    fn() -> Vec<u8>,
    &'static [(&'static [u16], &'static [i32])],
);

/// Format 1's worked examples: none. The `kerx` chapter gives none, and the
/// `kern` chapter's *Kerning Table Example* is a `kern` format-1 machine
/// whose value lists keep conventions `kerx`'s does not share
/// (`docs/design/shaping.md`, *The manual's worked examples*), so the machine
/// is held by the readings below; H-142's expectations are what the count
/// waits on.
const CONTEXTUAL_KERNING_EXAMPLES: &[WorkedKerning] = &[];

/// Runs each worked example through a `kerx` of its one subtable, and says
/// how many there were.
fn hold_kerning_to_worked_examples(examples: &[WorkedKerning]) -> usize {
    for (source, subtable, cases) in examples {
        let table = kerx_table(2, &[subtable()]);
        let kerx = Kerx::parse(&table).expect("a readable kerx");
        assert!(!cases.is_empty(), "{source} states a case");
        for (input, advances) in *cases {
            let mut buffer = Buffer::from_glyphs(input);
            let warnings = kerx.run(&mut buffer, &Limits::DEFAULT);
            let out: Vec<i32> = buffer.glyphs().iter().map(|g| g.x_advance).collect();
            assert_eq!(out, *advances, "{source}, {input:?}");
            assert!(warnings.is_empty(), "{source}: {warnings:?}");
        }
    }
    examples.len()
}

/// Contextual kerning reproduces each worked example
/// `docs/design/shaping.md` lists for `kerx` format 1, and there are none.
#[test]
fn contextual_kerning_reproduces_the_manuals_worked_examples() {
    assert_eq!(
        hold_kerning_to_worked_examples(CONTEXTUAL_KERNING_EXAMPLES),
        0
    );
}

/// A machine kerning `A V A`, under `coverage` and format 1: an `A` clears
/// the stack and pushes itself, a `V` after it pushes itself, and an `A`
/// after the two pushes itself and pops all three — itself by −30, the `V`
/// by −20 and the first `A` by −10. Classes: `A` 4 and `V` 5. States 0 and 1
/// are the start, 2 after an `A` and 3 after `A V`.
fn triple_machine(coverage: u32) -> Vec<u8> {
    kerx_subtable(
        coverage | 1,
        0,
        &kerx_contextual(
            &[(KA, 4), (KV, 5)],
            &[
                &[0, 0, 0, 0, 1, 0],
                &[0, 0, 0, 0, 1, 0],
                &[0, 0, 0, 0, 1, 2],
                &[0, 0, 0, 0, 3, 0],
            ],
            &[
                (0, 0, NO_VALUES),
                (2, RESET | PUSH, NO_VALUES),
                (3, PUSH, NO_VALUES),
                (0, PUSH, 0),
            ],
            &value_list(&[-30, -20, -10]),
        ),
    )
}

/// A context's glyphs are pushed as the machine meets them and its value
/// list pops them, the last pushed first, each moved by its value; a run
/// holding no whole context is not kerned, and `kern` set to 0 turns the
/// subtable off as it does a pair format.
#[test]
fn a_context_pushes_its_glyphs_and_a_value_list_kerns_them() {
    let face = kerning_face().with(b"kerx", kerx_table(2, &[triple_machine(0)]));
    for (text, moved) in [
        ("AVA", &[-10, -20, -30][..]),
        ("VAVA", &[0, -10, -20, -30]),
        ("AVAVA", &[-10, -20, -30, 0, 0]),
        ("AAVA", &[0, -10, -20, -30]),
        ("AV", &[0, 0]),
        ("AVV", &[0, 0, 0]),
        ("AVTA", &[0, 0, 0, 0]),
    ] {
        let (out, warnings) = kerned(&face, text);
        assert!(warnings.is_empty(), "{text}: {warnings:?}");
        assert_eq!(out, moved_by(&face, text, moved), "{text}");
    }
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let off = [(Tag::new(b"kern"), 0u32)];
    let runs = line_runs(&Shaper::new(&sfnt).with_settings(&off), "AVA");
    assert_eq!(glyphs(&runs), moved_by(&face, "AVA", &[0, 0, 0]));
}

/// `processDirection` walks the run from its last glyph to its first, so the
/// context is met the other way round; a cross-stream subtable moves each
/// popped glyph up by its value, that glyph alone, and changes no advance.
#[test]
fn a_contextual_subtable_walks_and_kerns_the_way_its_coverage_says() {
    let forward = kerning_face().with(b"kerx", kerx_table(2, &[triple_machine(0)]));
    let backward =
        kerning_face().with(b"kerx", kerx_table(2, &[triple_machine(PROCESS_DIRECTION)]));
    let (out, warnings) = kerned(&forward, "AVAA");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(out, moved_by(&forward, "AVAA", &[-10, -20, -30, 0]));
    let (out, warnings) = kerned(&backward, "AVAA");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        out,
        moved_by(&backward, "AVAA", &[-30, -20, -10, 0]),
        "the last `A` starts the context and the first ends it"
    );

    let across = kerning_face().with(b"kerx", kerx_table(2, &[triple_machine(CROSS_STREAM)]));
    let (out, warnings) = kerned(&across, "AVA");
    assert!(warnings.is_empty(), "{warnings:?}");
    let mut expected = moved_by(&across, "AVA", &[0, 0, 0]);
    for (glyph, up) in expected.iter_mut().zip([-10, -20, -30]) {
        glyph.y_offset = up;
    }
    assert_eq!(out, expected);
}

/// A machine whose every entry stays in state 0: an `A` pushes itself, a `T`
/// clears the stack and pushes itself, an `o` pops two glyphs by −7 and −9,
/// and an `x` pushes itself and pops one by +5. Classes: `A` 4, `T` 5, `o` 6
/// and `x` 7.
fn stacker_machine() -> Vec<u8> {
    let line: &[u16] = &[0, 0, 0, 0, 1, 2, 3, 4];
    let mut values = value_list(&[-7, -9]);
    let x_list = count16(values.len());
    values.extend(value_list(&[5]));
    kerx_subtable(
        1,
        0,
        &kerx_contextual(
            &[(KA, 4), (KT, 5), (KO, 6), (KX, 7)],
            &[line, line],
            &[
                (0, 0, NO_VALUES),
                (0, PUSH, NO_VALUES),
                (0, RESET | PUSH, NO_VALUES),
                (0, 0, 0),
                (0, PUSH, x_list),
            ],
            &values,
        ),
    )
}

/// One entry's three actions in the design's order: the stack is cleared,
/// then the current glyph pushed, then the values popped — so a `T`'s reset
/// leaves the `T` it pushes, and an `x` that pushes and kerns kerns itself.
/// A value with no glyph to pop ends its list, named, what it kerned
/// standing.
#[test]
fn an_entry_clears_the_stack_then_pushes_then_pops() {
    let face = kerning_face().with(b"kerx", kerx_table(2, &[stacker_machine()]));
    let empty = Warning::KerningStackEmpty { site: kerx_site(0) };
    for (text, moved, warned) in [
        ("AAo", &[-9, -7, 0][..], false),
        ("AATo", &[0, 0, -7, 0], true),
        ("Ax", &[0, 5], false),
        ("AxAo", &[-9, 5, -7, 0], false),
        ("o", &[0], true),
    ] {
        let (out, warnings) = kerned(&face, text);
        assert_eq!(out, moved_by(&face, text, moved), "{text}");
        let expected = if warned { vec![empty] } else { Vec::new() };
        assert_eq!(warnings, expected, "{text}");
    }
}

/// A machine holding its place once: an `A` met in state 0 pushes itself and
/// moves to state 2 without advancing, and the `A` met again in state 2 pops
/// one glyph by −11 and moves back. Classes: `A` 4.
fn holding_kerner() -> Vec<u8> {
    kerx_subtable(
        1,
        0,
        &kerx_contextual(
            &[(KA, 4)],
            &[&[0, 0, 0, 0, 1], &[0, 0, 0, 0, 1], &[0, 0, 0, 0, 2]],
            &[
                (0, 0, NO_VALUES),
                (2, PUSH | DONT_ADVANCE, NO_VALUES),
                (0, 0, 0),
            ],
            &value_list(&[-11]),
        ),
    )
}

/// `dontAdvance` holds the machine on its glyph, which it meets again in the
/// state the entry names: each `A` is pushed, then kerned by the entry its
/// second meeting takes.
#[test]
fn dont_advance_meets_the_glyph_again_in_its_new_state() {
    let face = kerning_face().with(b"kerx", kerx_table(2, &[holding_kerner()]));
    let (out, warnings) = kerned(&face, "AAV");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(out, moved_by(&face, "AAV", &[-11, -11, 0]));
}

/// A machine pushing every `A` and, at the end of text, popping a list of
/// `pops` values, 1 to `pops`. Classes: `A` 4.
fn ceiling_machine(pops: i16) -> Vec<u8> {
    let values: Vec<i16> = (1..=pops).collect();
    kerx_subtable(
        1,
        0,
        &kerx_contextual(
            &[(KA, 4)],
            &[&[2, 0, 0, 0, 1], &[2, 0, 0, 0, 1]],
            &[(0, 0, NO_VALUES), (0, PUSH, NO_VALUES), (0, 0, 0)],
            &value_list(&values),
        ),
    )
}

/// The kerning stack holds the chapter's eight glyphs: eight are pushed and
/// kerned with nothing said, and a ninth push is refused and named, the glyph
/// not pushed; the list's ninth value then finds no glyph to pop, which ends
/// it, named. Each is named once however often it is met.
#[test]
fn a_kerning_stack_past_its_ceiling_is_refused_and_named() {
    assert_eq!(KERNING_STACK, 8);
    let face = kerning_face().with(b"kerx", kerx_table(2, &[ceiling_machine(8)]));
    let (out, warnings) = kerned(&face, "AAAAAAAA");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        out,
        moved_by(&face, "AAAAAAAA", &[8, 7, 6, 5, 4, 3, 2, 1]),
        "the last pushed popped first"
    );

    let face = kerning_face().with(b"kerx", kerx_table(2, &[ceiling_machine(9)]));
    let (out, warnings) = kerned(&face, "AAAAAAAAAA");
    assert_eq!(
        warnings,
        vec![
            Warning::KerningStackFull { site: kerx_site(0) },
            Warning::KerningStackEmpty { site: kerx_site(0) },
        ]
    );
    assert_eq!(
        out,
        moved_by(&face, "AAAAAAAAAA", &[8, 7, 6, 5, 4, 3, 2, 1, 0, 0]),
        "the ninth and tenth `A` never pushed"
    );
}

/// A contextual machine meeting no glyph its class table names does not run
/// — its end-of-text list, which would find the stack empty, is not taken —
/// and one meeting such a glyph runs over the whole run.
#[test]
fn a_contextual_machine_meeting_no_glyph_it_names_does_not_run() {
    let face = kerning_face().with(b"kerx", kerx_table(2, &[ceiling_machine(1)]));
    let (out, warnings) = kerned(&face, "xoV");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(out, moved_by(&face, "xoV", &[0, 0, 0]));
    let (out, warnings) = kerned(&face, "xAV");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(out, moved_by(&face, "xAV", &[0, 1, 0]));
}

/// A right-to-left run is walked in layout order: the machine pushes a bet
/// and kerns it by −60 where an alef follows it on the line, as the text's
/// alef then bet lays out, and does nothing in the text's own order.
#[test]
fn a_right_to_left_run_is_kerned_contextually_in_layout_order() {
    let machine = kerx_subtable(
        1,
        0,
        &kerx_contextual(
            &[(KBET, 4), (KALEF, 5)],
            &[
                &[0, 0, 0, 0, 1, 0],
                &[0, 0, 0, 0, 1, 0],
                &[0, 0, 0, 0, 1, 2],
            ],
            &[(0, 0, NO_VALUES), (2, RESET | PUSH, NO_VALUES), (0, 0, 0)],
            &value_list(&[-60]),
        ),
    );
    let face = kerning_face().with(b"kerx", kerx_table(2, &[machine]));
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let (_, runs) = Shaper::new(&sfnt).shape_text("\u{5D0}\u{5D1}", BaseDirection::Auto);
    assert!(!runs[0].direction().is_forward());
    assert!(runs[0].warnings().is_empty(), "{:?}", runs[0].warnings());
    assert_eq!(
        glyphs(&runs),
        moved_by(&face, "\u{5D0}\u{5D1}", &[0, -60]),
        "the bet, left of the alef on the line, moved"
    );
}

/// A machine that loops stops at its ceiling, the pushes past the stack's
/// refused on the way; a value list the value table does not hold, a
/// `valueTable` past the subtable and a class table of a format the manual
/// does not define are each named, and kern nothing.
#[test]
fn a_contextual_kerning_subtable_that_loops_or_cannot_be_read_is_named() {
    let site = kerx_site(0);
    let lines: &[&[u16]] = &[&[0, 0, 0, 0, 1], &[0, 0, 0, 0, 1]];

    // An `A` pushed again and again without advancing.
    let looping = kerx_subtable(
        1,
        0,
        &kerx_contextual(
            &[(KA, 4)],
            lines,
            &[(0, 0, NO_VALUES), (0, PUSH | DONT_ADVANCE, NO_VALUES)],
            &value_list(&[]),
        ),
    );
    let table = kerx_table(2, &[looping]);
    let kerx = Kerx::parse(&table).expect("readable");
    let limits = Limits {
        max_transitions: 50,
        ..Limits::DEFAULT
    };
    let mut buffer = Buffer::from_glyphs(&[KA, KA]);
    assert_eq!(
        kerx.run(&mut buffer, &limits),
        vec![
            Warning::KerningStackFull { site },
            Warning::TransitionBudgetExceeded { site },
        ]
    );
    assert!(buffer.glyphs().iter().all(|glyph| glyph.x_advance == 0));

    // A value index forty values into a table of two.
    let past = kerx_contextual(
        &[(KA, 4)],
        lines,
        &[(0, 0, NO_VALUES), (0, PUSH, 40)],
        &value_list(&[-5]),
    );
    // The `valueTable` offset, past the subtable.
    let mut far = kerx_contextual(
        &[(KA, 4)],
        lines,
        &[(0, 0, NO_VALUES), (0, PUSH, 0)],
        &value_list(&[-5]),
    );
    put32(&mut far, 16, 4_000);
    for (name, body) in [("value index", past), ("valueTable", far)] {
        let face = kerning_face().with(b"kerx", kerx_table(2, &[kerx_subtable(1, 0, &body)]));
        let (out, warnings) = kerned(&face, "AA");
        assert_eq!(out, moved_by(&face, "AA", &[0, 0]), "{name}");
        assert_eq!(
            warnings,
            vec![Warning::MalformedAatSubtable { site }],
            "{name}"
        );
    }

    // The class table, after the `STXHeader` and `valueTable` at 20, made a
    // lookup of format 3.
    let mut odd = kerx_contextual(
        &[(KA, 4)],
        lines,
        &[(0, 0, NO_VALUES), (0, PUSH, 0)],
        &value_list(&[-5]),
    );
    odd[20..22].copy_from_slice(&3u16.to_be_bytes());
    let face = kerning_face().with(b"kerx", kerx_table(2, &[kerx_subtable(1, 0, &odd)]));
    let (out, warnings) = kerned(&face, "A");
    assert_eq!(out, moved_by(&face, "A", &[0]));
    assert_eq!(
        warnings,
        vec![Warning::UnknownLookupFormat { site, format: 3 }]
    );
}
