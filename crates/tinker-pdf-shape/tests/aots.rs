//! Adobe's annotated OpenType specification, run as a conformance suite.
//!
//! **aots** (`adobe-type-tools/aots`, Apache-2.0) is the OpenType
//! specification with a test case attached to almost every clause: a tiny
//! font exercising one lookup type in one format, a sequence of glyph indices
//! to feed it, and the sequence — or the positions — that a correct
//! implementation must produce. `docs/design/shaping.md` names it as one of
//! the two conformance bars for this crate, and `docs/rulings.md` ruling 13
//! is what makes it admissible: **the expected output is inside the fixture**,
//! written by the people who wrote the specification, so nothing outside this
//! repository is being asked whether the answer is right.
//!
//! # Provenance, stated exactly, because half of it is second-hand
//!
//! `data/aots/cases.txt` is distilled from `src/opentype.xml` in the aots
//! repository — Adobe's own `<aots:gsub-test>`, `<aots:gpos-test>` and
//! `<aots:context-test>` elements, with their `inputs`, `outputs`, `xdeltas`
//! and `ydeltas` attributes carried over verbatim and nothing else kept. That
//! is a dated measurement, in ruling 13's words: a tool was run once and its
//! output committed.
//!
//! `data/aots/fonts/*.otf` did **not** come from Adobe's repository, and this
//! is the part worth being plain about. aots ships its fonts as XML and
//! compiles them with a Java toolchain — Saxon, plus a compiler generated from
//! the XML by an XSLT stylesheet — which is not present on the machine this
//! was written on, so the fonts could not be built from source here. The 185
//! files are the compiled aots fixtures as redistributed by the HarfBuzz
//! project (`test/shape/data/aots/fonts`, the same Apache-2.0). They are
//! *bytes*, which ruling 13 keeps admissible with provenance recorded; what
//! matters is that **no expected output was taken from HarfBuzz**. Every
//! number this file asserts against comes from Adobe's XML.
//!
//! The risk that leaves is named rather than absorbed: if those fonts differ
//! from what aots's own compiler would emit, this suite is testing something
//! slightly different from what its expectations describe. What bounds it is
//! that the expectations and the fonts have independent origins, so a
//! mismatch shows up as a failure rather than as agreement.
//!
//! # The measurement model, and where it comes from
//!
//! aots states positions as *deltas from an unpositioned run*, and the
//! conversion is defined by `harfbuzz/hb-aots-tester.cpp` **in the aots
//! repository itself** — a file Adobe ships beside the fixtures, not a
//! HarfBuzz artefact. Walking the run, each glyph's reported position is the
//! pen so far plus its own offset, and the pen advances by the glyph's
//! advance less the nominal 1500 units every glyph in these fonts is 1500
//! units wide. Marks do not subtract, because a shaper has zeroed their
//! advances by then. So an unshaped run reports zeroes throughout, and every
//! non-zero number in the fixture is something a lookup did.
//!
//! # What is asserted beyond "the cases pass"
//!
//! A conformance suite whose case count can shrink silently is not a suite
//! ([`docs/verification.md`](../../../docs/verification.md)). Three counts are
//! pinned:
//!
//! - **How many cases ran**, in total and per lookup type.
//! - **How many of those are *discriminating*** — cases whose expected output
//!   differs from their input. A lookup type covered only by cases that
//!   expect nothing to happen is a lookup type whose implementation could be
//!   deleted with the suite still green, so the discriminating count is
//!   asserted separately and is non-zero for every type aots reaches.
//! - **Which lookup types aots does not reach at all**, by name, so that the
//!   gap is a list rather than an absence. `tests/synthetic.rs` covers those.
//!
//! The fixtures ship with the published crate rather than being excluded from
//! it, so a `cargo test` run by whoever downloaded it reaches the same cases
//! this one does. The manifest records why that was not the first answer.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use tinker_pdf_font::Sfnt;
use tinker_pdf_shape::{Buffer, GlyphClass, Layout, LayoutTable, Limits, MarkWidths, Table, Tag};

/// The advance every glyph in every aots fixture carries, and the units per
/// em of every one of them. Asserted per fixture rather than assumed, because
/// the whole delta model is expressed relative to it.
const NOMINAL_ADVANCE: i32 = 1500;

/// The script, language and feature aots shapes every case under; the
/// language is left to the script's default, which is what aots's own runner
/// reaches by naming one no fixture declares.
const SCRIPT: Tag = Tag::new(b"latn");
/// The one feature every aots fixture puts its lookups behind.
const FEATURE: Tag = Tag::new(b"test");

const CASES: &str = include_str!("../data/aots/cases.txt");

/// One line of `cases.txt`.
struct Case {
    kind: Kind,
    id: String,
    font: String,
    input: Vec<u16>,
    output: Option<Vec<u16>>,
    x: Vec<i32>,
    y: Vec<i32>,
    /// Present on the three cases that turn the feature on with a different
    /// *value* at each position, to pick a different alternate glyph each
    /// time. See [`the_three_cases_this_milestone_cannot_run`].
    select: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Kind {
    Gsub,
    Gpos,
}

impl Case {
    /// Whether this case would still pass if the lookup it exercises did
    /// nothing at all.
    fn discriminating(&self) -> bool {
        if let Some(output) = &self.output {
            if output != &self.input {
                return true;
            }
        }
        self.x.iter().chain(self.y.iter()).any(|v| *v != 0)
    }

    fn expected_glyphs(&self) -> &[u16] {
        self.output.as_deref().unwrap_or(&self.input)
    }
}

fn parse_cases() -> Vec<Case> {
    let mut out = Vec::new();
    for line in CASES.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let mut fields = line.split('\t');
        let kind = match fields.next() {
            Some("gsub") => Kind::Gsub,
            Some("gpos") => Kind::Gpos,
            other => panic!("unknown case kind {other:?}"),
        };
        let id = fields.next().expect("a case id").to_string();
        let font = fields.next().expect("a font name").to_string();
        let mut input = Vec::new();
        let mut output = None;
        let mut x = Vec::new();
        let mut y = Vec::new();
        let mut select = false;
        for field in fields {
            let (name, values) = field.split_once('=').expect("a name=values field");
            let numbers: Vec<i64> = values
                .split(',')
                .filter(|v| !v.is_empty())
                .map(|v| v.parse::<i64>().expect("a number"))
                .collect();
            match name {
                "in" => input = numbers.iter().map(|v| *v as u16).collect(),
                "out" => output = Some(numbers.iter().map(|v| *v as u16).collect()),
                "x" => x = numbers.iter().map(|v| *v as i32).collect(),
                "y" => y = numbers.iter().map(|v| *v as i32).collect(),
                "select" => select = true,
                other => panic!("unknown field {other}"),
            }
        }
        out.push(Case {
            kind,
            id,
            font,
            input,
            output,
            x,
            y,
            select,
        });
    }
    out
}

fn font_bytes(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("data/aots/fonts")
        .join(format!("{name}.otf"));
    std::fs::read(&path)
        .unwrap_or_else(|error| panic!("{} could not be read: {error}", path.display()))
}

/// Shapes one case the way aots's own runner does: substitute, then position,
/// under `latn`/default/`test`.
fn shape(face: &Sfnt<'_>, layout: &Layout<'_>, input: &[u16], position: bool) -> Buffer {
    let mut buffer = Buffer::from_glyphs(input);
    for at in 0..buffer.len() {
        let glyph = buffer.glyph(at).expect("in range").glyph;
        let advance = i32::from(face.advance(glyph).unwrap_or(0));
        buffer.glyph_mut(at).expect("in range").x_advance = advance;
    }
    let limits = Limits::for_glyphs(buffer.len());
    if let Some(gsub) = layout.gsub() {
        let lookups = gsub.lookups_for(SCRIPT, None, &[FEATURE]);
        let warnings = layout.substitute(&mut buffer, &lookups, limits);
        assert!(
            warnings.is_empty(),
            "a conforming fixture warned: {warnings:?}"
        );
    }
    if position {
        if let Some(gpos) = layout.gpos() {
            let lookups = gpos.lookups_for(SCRIPT, None, &[FEATURE]);
            let warnings = layout.position(&mut buffer, &lookups, limits, MarkWidths::ZeroByGdef);
            assert!(
                warnings.is_empty(),
                "a conforming fixture warned: {warnings:?}"
            );
        }
    }
    buffer
}

/// The positions aots states, reconstructed from the shaped run.
fn deltas(layout: &Layout<'_>, buffer: &Buffer) -> (Vec<i32>, Vec<i32>) {
    let (mut xs, mut ys) = (Vec::new(), Vec::new());
    let (mut pen_x, mut pen_y) = (0i32, 0i32);
    for glyph in buffer.glyphs() {
        xs.push(pen_x + glyph.x_offset);
        ys.push(pen_y + glyph.y_offset);
        pen_x += glyph.x_advance;
        let class = layout.gdef().map_or(GlyphClass::Unclassified, |gdef| {
            gdef.glyph_class(glyph.glyph)
        });
        if class != GlyphClass::Mark {
            pen_x -= NOMINAL_ADVANCE;
        }
        pen_y += glyph.y_advance;
    }
    (xs, ys)
}

/// Every lookup type a table carries, with extension indirection followed.
///
/// Read through the crate's own public accessors rather than through a second
/// parser, so the count this file asserts is a count of what the code under
/// test can see.
fn lookup_types(table: &LayoutTable<'_>) -> BTreeSet<(Table, u16)> {
    let extension = match table.table() {
        Table::Gsub => 7,
        Table::Gpos => 9,
    };
    let lookups = table.lookups();
    let mut out = BTreeSet::new();
    for index in 0..lookups.len() {
        let Some(lookup) = lookups.get(index) else {
            continue;
        };
        let kind = lookup.kind();
        out.insert((table.table(), kind));
        if kind == extension {
            for sub in 0..lookup.len() {
                if let Some(inner) = lookup.subtable(sub).and_then(|data| data.u16(2)) {
                    out.insert((table.table(), inner));
                }
            }
        }
    }
    out
}

/// The tally this suite is pinned to: `(table, lookup type) -> (cases,
/// discriminating cases)`.
///
/// **Read the second number.** A lookup type reached only by cases that
/// expect nothing to change is a type whose implementation could be deleted
/// with this file still green; the second column is how many of the cases
/// would fail if it were. Every row has a non-zero one.
///
/// Types absent from this table are absent from aots, and are listed by name
/// in [`aots_does_not_reach_these`].
const EXPECTED: &[(Table, u16, usize, usize)] = &[
    (Table::Gsub, 1, 107, 70),
    (Table::Gsub, 2, 96, 61),
    // No `GSUB` type 3 row. The only three cases that reach an alternate
    // substitution are the three this milestone declines to run; see
    // [`the_three_cases_this_milestone_cannot_run`] and `tests/synthetic.rs`.
    (Table::Gsub, 4, 113, 72),
    (Table::Gsub, 5, 42, 35),
    (Table::Gsub, 6, 58, 28),
    (Table::Gsub, 7, 2, 2),
    (Table::Gpos, 1, 103, 65),
    (Table::Gpos, 2, 105, 67),
    (Table::Gpos, 3, 13, 7),
    (Table::Gpos, 4, 8, 4),
    (Table::Gpos, 5, 2, 2),
    (Table::Gpos, 6, 3, 1),
    (Table::Gpos, 7, 36, 28),
    (Table::Gpos, 8, 58, 28),
    (Table::Gpos, 9, 2, 2),
];

/// The seven cursive-attachment cases where aots and text-rendering-tests
/// **cannot both be satisfied**, with the positions this crate produces.
///
/// # The two readings, and which one this crate follows
///
/// A `GPOS` type 3 lookup says an exit anchor and an entry anchor must meet.
/// Both sides agree on where the joined pair ends up relative to each other —
/// aots's own prose states it, *"glyph 19 will have been moved, to have its
/// origin at (99, 99) relative to the origin of the new position of glyph
/// 18"*, and this crate puts it exactly there. They disagree about
/// **everything after the pair**.
///
/// Adobe's reading moves the joined glyph by placement and touches no advance,
/// so the run is exactly as wide as it was. ISO/IEC 14496-22 and OpenType 1.9
/// say the layout engine *"adjusts the advance"*, so the pair overlaps and the
/// run becomes narrower by the overlap — and everything past the join moves
/// with it. The difference in each row below is that overlap, `1500 − 99`.
///
/// Milestone 3 recorded that nothing in the tree could tell the two apart and
/// named text-rendering-tests SHARAN-1 as the case that would. It did, and it
/// chose 14496-22: shaping its six Nasta‘līq words under Adobe's reading puts
/// every glyph identity right and every pen position after a join too far
/// along, by exactly the accumulated overlap. That is not a close call. Under
/// Adobe's reading no Arabic face joins at all — a joined word is as wide as
/// its letters standing apart — which is the opposite of what cursive
/// attachment is for.
///
/// # Why these still run
///
/// They are not skipped. Each is shaped, its glyph count is checked, its `y`
/// deltas are checked against aots unchanged, and its `x` deltas are checked
/// against the array below — so the number this crate produces is pinned just
/// as tightly as an agreeing case, and both readings are written down where a
/// reader can see the one subtracted from the other. What is given up is the
/// claim that this crate agrees with aots on all 272 cases; it agrees on 265,
/// and the other seven are here with the reason.
const CURSIVE_DIVERGENCE: &[(&str, &[i32])] = &[
    ("gpos3_lookupflag_1", &[0, 0, -1300, -1401, -1401]),
    (
        "gpos3_lookupflag_2",
        &[0, 0, -1300, -1300, -1300, -1401, -1401],
    ),
    ("gpos3_test1a", &[0, 0, -1401, -1401]),
    ("gpos3_test3a", &[0, 0, -1400, -1400]),
    ("gpos3_test3b", &[0, 0, -1401, -1401]),
    ("gpos3_test3c", &[0, 0, -1398, -1398]),
    ("gpos3_test3d", &[0, 0, -1399, -1399]),
];

/// How many cases the file holds, in total.
const TOTAL_CASES: usize = 275;
/// How many of them this milestone runs; the difference is
/// [`the_three_cases_this_milestone_cannot_run`].
const RAN_CASES: usize = 272;

#[test]
fn every_case_produces_what_the_specification_says() {
    let cases = parse_cases();
    assert_eq!(
        cases.len(),
        TOTAL_CASES,
        "the case file changed size; if that is intended, the counts below move with it"
    );

    let mut failures: Vec<String> = Vec::new();
    let mut diverged: Vec<String> = Vec::new();
    let mut ran = 0usize;
    for case in &cases {
        if case.select {
            continue;
        }
        ran += 1;
        let bytes = font_bytes(&case.font);
        let face = Sfnt::parse(&bytes).expect("an aots fixture is a valid sfnt");
        assert_eq!(
            i32::from(face.units_per_em),
            NOMINAL_ADVANCE,
            "{}: the delta model assumes 1500 units per em",
            case.id
        );
        let layout = Layout::parse(&face);
        let buffer = shape(&face, &layout, &case.input, case.kind == Kind::Gpos);
        let glyphs: Vec<u16> = buffer.glyphs().iter().map(|g| g.glyph).collect();
        // A positioning case states its expected glyph *count* through the
        // length of its delta arrays, and states the glyphs themselves only
        // where the fixture's own `GSUB` produced them — a context test's
        // `outputs` belongs to the substitution half of the pair, which runs
        // against a different font.
        if case.kind == Kind::Gpos && glyphs.len() != case.x.len() {
            failures.push(format!(
                "{}: {} glyphs, expected {}",
                case.id,
                glyphs.len(),
                case.x.len()
            ));
            continue;
        }
        if case.output.is_some() && glyphs != case.expected_glyphs() {
            failures.push(format!(
                "{}: glyphs {:?}, expected {:?}",
                case.id,
                glyphs,
                case.expected_glyphs()
            ));
            continue;
        }
        if case.kind == Kind::Gpos {
            let (x, y) = deltas(&layout, &buffer);
            // Seven cases read their `x` from `CURSIVE_DIVERGENCE` instead of
            // from aots, because the two published sources disagree there and
            // that constant says which one this crate follows and why. Their
            // `y` is aots's, unchanged: the disagreement is only along the
            // line.
            let diverges = CURSIVE_DIVERGENCE
                .iter()
                .find(|(id, _)| *id == case.id.as_str());
            let wanted = diverges.map_or(case.x.as_slice(), |(_, x)| *x);
            if diverges.is_some() {
                diverged.push(case.id.clone());
            }
            if x != wanted || y != case.y {
                failures.push(format!(
                    "{}: x {x:?} y {y:?}, expected x {wanted:?} y {:?}",
                    case.id, case.y
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {ran} aots cases failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert_eq!(ran, RAN_CASES, "the number of cases that ran moved");
    let expected: Vec<String> = CURSIVE_DIVERGENCE
        .iter()
        .map(|(id, _)| (*id).to_string())
        .collect();
    assert_eq!(
        diverged, expected,
        "the set of cases where aots and text-rendering-tests disagree moved"
    );
}

/// The three cases this milestone declines to run, and why.
///
/// aots turns its one feature on with a *value* — `1`, `2`, `3` — at chosen
/// positions, and a value above one selects that alternate from a `GSUB` type
/// 3 `AlternateSet`. All three of these cases use a different value at each
/// position, which needs a per-position feature-value API this crate does not
/// have: [`Layout::substitute`] takes a lookup list and no values, so the
/// first alternate is always the one taken.
///
/// That is `docs/design/shaping.md`'s milestone 2 — the shaper is what decides
/// which features a run gets — and it is recorded here rather than left as
/// three cases that quietly do not run. The consequence is stated in
/// [`EXPECTED`]: **aots contributes no coverage of `GSUB` type 3 at all**,
/// because these three cases are the only ones that reach a type 3 lookup, so
/// the alternate substitution's evidence is a synthetic fixture in
/// `tests/synthetic.rs` instead.
#[test]
fn the_three_cases_this_milestone_cannot_run() {
    let skipped: Vec<String> = parse_cases()
        .into_iter()
        .filter(|case| case.select)
        .map(|case| case.id)
        .collect();
    assert_eq!(
        skipped,
        vec![
            "gsub3_1_lookupflag_t1".to_string(),
            "gsub3_1_multiple_t1".to_string(),
            "gsub3_1_simple_t1".to_string(),
        ],
        "the set of cases this milestone cannot run changed"
    );
    assert_eq!(TOTAL_CASES - skipped.len(), RAN_CASES);
}

#[test]
fn the_suite_covers_the_lookup_types_it_claims_to() {
    let cases = parse_cases();
    let mut totals: BTreeMap<(Table, u16), (usize, usize)> = BTreeMap::new();
    for case in &cases {
        if case.select {
            continue;
        }
        let bytes = font_bytes(&case.font);
        let face = Sfnt::parse(&bytes).expect("an aots fixture is a valid sfnt");
        let layout = Layout::parse(&face);
        let mut types = BTreeSet::new();
        if let Some(gsub) = layout.gsub() {
            types.extend(lookup_types(gsub));
        }
        // A GSUB case is not evidence about GPOS, even when the fixture
        // carries both tables — only the positioning cases run GPOS at all.
        if case.kind == Kind::Gpos {
            if let Some(gpos) = layout.gpos() {
                types.extend(lookup_types(gpos));
            }
        }
        for key in types {
            let entry = totals.entry(key).or_insert((0, 0));
            entry.0 += 1;
            if case.discriminating() {
                entry.1 += 1;
            }
        }
    }

    let found: Vec<(Table, u16, usize, usize)> = totals
        .iter()
        .map(|((table, kind), (all, discriminating))| (*table, *kind, *all, *discriminating))
        .collect();
    assert_eq!(
        found,
        EXPECTED.to_vec(),
        "the per-lookup-type fixture counts moved"
    );
    for (table, kind, all, discriminating) in EXPECTED {
        assert!(
            *discriminating > 0,
            "{table} type {kind} has {all} cases and not one of them would fail \
             if the implementation were deleted"
        );
    }
}

/// The lookup types and `GDEF` structures aots has no fixture for.
///
/// Named here rather than left as an absence, because a suite's silence about
/// a lookup type reads exactly like coverage of it. Each of these is tested in
/// `tests/synthetic.rs` against a table this repository assembles itself,
/// which is weaker evidence — the same author wrote the table and the
/// reader — and saying so is the point.
#[test]
fn aots_does_not_reach_these() {
    let cases = parse_cases();
    let mut seen = BTreeSet::new();
    for case in &cases {
        let bytes = font_bytes(&case.font);
        let face = Sfnt::parse(&bytes).expect("an aots fixture is a valid sfnt");
        let layout = Layout::parse(&face);
        if let Some(gsub) = layout.gsub() {
            seen.extend(lookup_types(gsub));
        }
        if let Some(gpos) = layout.gpos() {
            seen.extend(lookup_types(gpos));
        }
        if let Some(gdef) = layout.gdef() {
            assert!(
                gdef.attachments().is_none(),
                "an aots fixture grew an AttachList; this list is now wrong"
            );
            assert!(
                gdef.ligature_carets().is_none(),
                "an aots fixture grew a LigCaretList; this list is now wrong"
            );
            assert_eq!(
                gdef.mark_glyph_set_count(),
                0,
                "an aots fixture grew mark glyph sets; this list is now wrong"
            );
        }
    }
    assert!(
        !seen.contains(&(Table::Gsub, 8)),
        "aots grew a reverse chaining fixture; move it out of the synthetic suite"
    );
}
