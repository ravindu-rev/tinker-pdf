//! The fonts group: paragraphs shaped, glyph outlines read, and variable
//! faces drawn and written at a named instance.
//!
//! The harness, the sampling and the rules every name here keeps are
//! `support/mod.rs`'s; `baseline.json`'s `fonts` list names these operations
//! in this order. Each input is asserted before its operation is timed: every
//! paragraph shapes to glyphs and no `.notdef`, the column to runs standing
//! upright and sideways, every face has its glyph count, every glyph of
//! every program yields an outline, each variable face's instance stands
//! away from its default and draws every glyph but the space, and each
//! static instance is a face with no variation table.

mod support;

use std::cell::LazyCell;
use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use support::{inputs, Group, Per};
use tinker_pdf_font::cff::Machine;
#[cfg(not(tinker_planning_baseline))]
use tinker_pdf_font::variation::{gvar::Gvar, DesignSpace, Instance};
use tinker_pdf_font::{glyf, Cff, Outline, Sfnt};
#[cfg(not(tinker_planning_baseline))]
use tinker_pdf_font::{instance, Cff2};
use tinker_pdf_shape::bidi::BaseDirection;
use tinker_pdf_shape::{Plans, ShapedRun, Shaper};

/// One call of a shaping operation: a [`Shaper`] over the face made with the
/// face's kept [`Plans`], as every caller makes one, and the paragraph shaped
/// through it. The plans and indexes are built when the paragraph is first
/// asserted, before anything is timed, as a caller's are by its first run.
fn shape(face: &Sfnt<'_>, plans: &Plans, text: &str) -> Vec<ShapedRun> {
    Shaper::new(face)
        .with_plans(plans)
        .shape_text(text, BaseDirection::Auto)
        .1
}

/// `text` shaped in `face` holds glyphs, and none of them is `.notdef`.
fn assert_shapes(face: &Sfnt<'_>, plans: &Plans, text: &str, script: &str) {
    assert_eq!(text.chars().count(), 1000, "the {script} paragraph");
    let runs = shape(face, plans, text);
    let glyphs: Vec<u16> = runs
        .iter()
        .flat_map(|run| run.glyphs().iter().map(|glyph| glyph.glyph))
        .collect();
    assert!(
        !glyphs.is_empty(),
        "the {script} paragraph shapes to glyphs"
    );
    assert!(
        !glyphs.contains(&0),
        "the {script} paragraph shapes to no .notdef"
    );
}

/// One call of the column operation: `text` shaped as a column through the
/// face's kept [`Plans`], as a caller setting a paragraph vertically shapes
/// it — its runs cut by orientation, each upright one top to bottom with the
/// vertical plan and metrics, each sideways one along a line.
#[cfg(not(tinker_planning_baseline))]
fn shape_column(face: &Sfnt<'_>, plans: &Plans, text: &str) -> Vec<ShapedRun> {
    Shaper::new(face)
        .with_plans(plans)
        .shape_text_vertical(text, BaseDirection::Auto)
        .1
}

/// The planning baseline has no column: its arm shapes the paragraph along a
/// line through the same face in its place.
#[cfg(tinker_planning_baseline)]
fn shape_column(face: &Sfnt<'_>, plans: &Plans, text: &str) -> Vec<ShapedRun> {
    shape(face, plans, text)
}

/// The vertical paragraph shaped as a column holds glyphs, none of them
/// `.notdef`, and runs that stand upright and runs that stand sideways.
fn assert_column(face: &Sfnt<'_>, plans: &Plans, text: &str) {
    assert_eq!(text.chars().count(), 1000, "the vertical paragraph");
    let runs = shape_column(face, plans, text);
    let glyphs: Vec<u16> = runs
        .iter()
        .flat_map(|run| run.glyphs().iter().map(|glyph| glyph.glyph))
        .collect();
    assert!(
        !glyphs.is_empty(),
        "the vertical paragraph shapes to glyphs"
    );
    assert!(
        !glyphs.contains(&0),
        "the vertical paragraph shapes to no .notdef"
    );
    #[cfg(not(tinker_planning_baseline))]
    for stands in [
        tinker_pdf_shape::Orientation::Upright,
        tinker_pdf_shape::Orientation::Sideways,
    ] {
        assert!(
            runs.iter().any(|run| run.orientation() == stands),
            "the vertical paragraph has a run standing {stands:?}"
        );
    }
}

/// A TrueType face's glyph count, `maxp`'s `numGlyphs`.
fn glyph_count(face: &Sfnt<'_>) -> u16 {
    let maxp = face
        .table(u32::from_be_bytes(*b"maxp"))
        .expect("a maxp table");
    u16::from_be_bytes([maxp[4], maxp[5]])
}

/// One call of a TrueType outline operation: the face parsed, then every
/// glyph's outline read. How many of them have contours.
fn outline_face(bytes: &[u8]) -> usize {
    let face = Sfnt::parse(bytes).expect("the face parses");
    (0..glyph_count(&face))
        .filter(|&glyph| !black_box(glyf::outline(&face, glyph).expect("an outline")).is_empty())
        .count()
}

/// The face outlined in the setup: its `glyphs` and how many have contours.
fn assert_outlines(bytes: &[u8], glyphs: u16, name: &str) {
    let face = Sfnt::parse(bytes).expect("the face parses");
    assert_eq!(glyph_count(&face), glyphs, "{name}'s glyphs");
    let drawn = outline_face(bytes);
    assert!(
        drawn > usize::from(glyphs) * 9 / 10,
        "{name}'s outlines: {drawn} of {glyphs} have contours"
    );
}

/// One call of the CFF outline operation: each program parsed, then every
/// glyph's outline read into one outline through one machine per program, as
/// a caller reading a whole face keeps them (`Cff::outline_into`). How many
/// outlines that is: a program the parser refuses has none.
fn outline_programs(programs: &[&[u8]]) -> usize {
    let mut outlines = 0;
    for program in programs {
        let Some(cff) = Cff::parse(program) else {
            continue;
        };
        let mut machine = Machine::new();
        let mut outline = Outline::default();
        for glyph in 0..cff.glyph_count() {
            let glyph = u16::try_from(glyph).expect("a CFF glyph index fits 16 bits");
            if black_box(cff.outline_into(glyph, &mut outline, &mut machine)) {
                outlines += 1;
            }
            black_box(&outline);
        }
    }
    outlines
}

/// The named instance the variable operations draw and write, by its index
/// among each face's `fvar` instances: the fifth of both, `Condensed Bold`
/// (`wght` 700, `wdth` 75) in the `glyf` face and `Wide Black` (`wght` 900,
/// `wdth` 125) in the CFF2 face, each away from its face's default on two
/// axes.
const NAMED_INSTANCE: u16 = 4;

/// Calls an iteration of each variable outline operation makes.
const VARIABLE_OUTLINE_CALLS: u64 = 5000;

/// Calls an iteration of each static-instance operation makes.
const INSTANCE_CALLS: u64 = 700;

/// One call of the variable `glyf` outline operation: the face parsed, its
/// design space and `gvar` read and the named instance's location found
/// there, then every glyph's outline read at it. How many of them have
/// contours.
#[cfg(not(tinker_planning_baseline))]
fn outline_glyf_instance(bytes: &[u8], named: u16) -> usize {
    let face = Sfnt::parse(bytes).expect("the face parses");
    let space = DesignSpace::read(&face).expect("the face's design space is read");
    let gvar = Gvar::read(&face, &space).expect("the face's gvar is read");
    let location = space
        .location(&Instance::Named(named))
        .expect("the face names the instance");
    (0..glyph_count(&face))
        .filter(|&glyph| {
            !black_box(
                glyf::outline_at(&face, glyph, gvar.as_ref(), &location).expect("an outline"),
            )
            .is_empty()
        })
        .count()
}

/// The planning baseline reads no variation table: its arm outlines every
/// glyph of the same face at its default, the outlines `glyf` holds.
#[cfg(tinker_planning_baseline)]
fn outline_glyf_instance(bytes: &[u8], _named: u16) -> usize {
    outline_face(bytes)
}

/// One call of the variable CFF2 outline operation: the face parsed, its
/// design space read and the named instance's location found there, its
/// `CFF2` table parsed and the scalars every `blend` reads there computed
/// once, then every glyph's outline read into one outline through one
/// machine (`Cff2::outline_with`), as a caller reading a whole instance
/// keeps them. How many of them have contours.
#[cfg(not(tinker_planning_baseline))]
fn outline_cff2_instance(bytes: &[u8], named: u16) -> usize {
    let face = Sfnt::parse(bytes).expect("the face parses");
    let space = DesignSpace::read(&face).expect("the face's design space is read");
    let location = space
        .location(&Instance::Named(named))
        .expect("the face names the instance");
    let table = face
        .table(u32::from_be_bytes(*b"CFF2"))
        .expect("a CFF2 table");
    let cff2 = Cff2::parse(table).expect("the CFF2 table parses");
    let blends = cff2
        .blends_in(&space, &location)
        .expect("the variation store's axes are fvar's");
    let mut machine = Machine::new();
    let mut outline = Outline::default();
    let mut drawn = 0;
    for glyph in 0..cff2.glyph_count() {
        let glyph = u16::try_from(glyph).expect("a CFF2 glyph index fits 16 bits");
        cff2.outline_with(glyph, &blends, &mut outline, &mut machine)
            .expect("an outline");
        if !black_box(&outline).is_empty() {
            drawn += 1;
        }
    }
    drawn
}

/// The planning baseline reads no `CFF2` table: its arm is the `glyf`
/// operation's, handed the `glyf` face, the operation the gate pairs this
/// one with.
#[cfg(tinker_planning_baseline)]
fn outline_cff2_instance(bytes: &[u8], named: u16) -> usize {
    outline_glyf_instance(bytes, named)
}

/// One call of a static-instance operation: the face written whole at its
/// named instance as a face with no variation table
/// (`instance::instantiate`), its `CFF2` outlines as `CFF ` where it has
/// them.
#[cfg(not(tinker_planning_baseline))]
fn write_instance(bytes: &[u8], named: u16) -> Vec<u8> {
    instance::instantiate(bytes, &Instance::Named(named))
        .expect("the instancer writes the instance")
}

/// The planning baseline has no instancer: its arm writes the same face
/// whole through the one face writer that revision has, its subsetter,
/// keeping every glyph.
#[cfg(tinker_planning_baseline)]
fn write_instance(bytes: &[u8], _named: u16) -> Vec<u8> {
    let face = Sfnt::parse(bytes).expect("the face parses");
    let every: std::collections::BTreeSet<u16> = (0..glyph_count(&face)).collect();
    tinker_pdf_font::subset(bytes, &every).expect("the subsetter writes the face whole")
}

/// A variable face outlined in the setup: its `glyphs`, its named instance
/// away from its default, and every glyph but the space drawing contours
/// there through `outline`, the operation's own call.
#[cfg(not(tinker_planning_baseline))]
fn assert_variable_outlines(
    bytes: &[u8],
    glyphs: u16,
    name: &str,
    outline: fn(&[u8], u16) -> usize,
) {
    let face = Sfnt::parse(bytes).expect("the face parses");
    assert_eq!(glyph_count(&face), glyphs, "{name}'s glyphs");
    let space = DesignSpace::read(&face).expect("the face's design space is read");
    assert!(
        space.is_variable(),
        "{name} varies, so its instance is not the face itself"
    );
    let location = space
        .location(&Instance::Named(NAMED_INSTANCE))
        .unwrap_or_else(|error| panic!("{name} names its instance {NAMED_INSTANCE}: {error}"));
    assert!(
        !location.is_default(),
        "{name}'s instance {NAMED_INSTANCE} stands away from its default"
    );
    assert_eq!(
        outline(bytes, NAMED_INSTANCE),
        usize::from(glyphs) - 1,
        "{name}'s outlines at its instance: every glyph but the space has contours"
    );
}

/// A static instance written in the setup: a face that parses, has the
/// variable face's glyphs, carries its outlines in `outlines` and no table a
/// static instance drops, and is not the variable face's bytes.
#[cfg(not(tinker_planning_baseline))]
fn assert_instance(bytes: &[u8], outlines: [u8; 4], name: &str) {
    let written = write_instance(bytes, NAMED_INSTANCE);
    assert_ne!(written, bytes, "{name}'s instance is written, not copied");
    let face = Sfnt::parse(bytes).expect("the face parses");
    let static_face = Sfnt::parse(&written).unwrap_or_else(|| panic!("{name}'s instance parses"));
    assert_eq!(
        glyph_count(&static_face),
        glyph_count(&face),
        "{name}'s instance keeps every glyph"
    );
    assert!(
        static_face.table(u32::from_be_bytes(outlines)).is_some(),
        "{name}'s instance carries its outlines in {}",
        String::from_utf8_lossy(&outlines)
    );
    for tag in instance::DROPPED.iter().chain([b"CFF2"]) {
        assert!(
            static_face.table(u32::from_be_bytes(*tag)).is_none(),
            "{name}'s instance carries no {}",
            String::from_utf8_lossy(tag)
        );
    }
}

/// One of the faces the group reads, by its file name, from where `read`
/// finds it in the tree.
fn face(name: &str, read: impl FnOnce() -> Vec<u8>) -> Vec<u8> {
    inputs::store().bytes(name, read)
}

fn fonts(c: &mut Criterion) {
    // The faces a routine parses once and shapes through, and the programs it
    // outlines, outlive the routine, so each is kept here.
    let serif = LazyCell::new(|| {
        face("LiberationSerif-Regular.ttf", || {
            inputs::liberation("LiberationSerif-Regular.ttf")
        })
    });
    let arabic = LazyCell::new(|| {
        face("TestShapeAran.ttf", || {
            inputs::shape_face("text-rendering-tests/fonts/TestShapeAran.ttf")
        })
    });
    let devanagari = LazyCell::new(|| {
        face("NotoSansDevanagari-Regular.ttf", || {
            inputs::shape_face("devanagari/NotoSansDevanagari-Regular.ttf")
        })
    });
    let column = LazyCell::new(|| face("column-face.ttf", inputs::column_face));
    let cff = LazyCell::new(|| inputs::store().set("cff-programs.bundle", inputs::cff_programs));
    let variable_glyf = LazyCell::new(|| {
        face("variable-glyf.ttf", || {
            inputs::variable_face("variable-glyf.ttf")
        })
    });
    // The planning baseline reads no `CFF2` table, so its build never reads
    // this face (its operations' arms read the `glyf` face).
    #[cfg_attr(tinker_planning_baseline, allow(unused_variables))]
    let variable_cff2 = LazyCell::new(|| {
        face("variable-cff2.otf", || {
            inputs::variable_face("variable-cff2.otf")
        })
    });

    let mut group = Group::new(c, "fonts");

    // First, because they have no unit: a throughput set on a group stays on
    // it for every operation after, and a shaped paragraph is none of the
    // units.
    group.calibrated("shape 1000 characters of Latin", |op| {
        let face = Sfnt::parse(&serif).expect("Liberation Serif parses");
        let latin_text = inputs::store().text("latin-1000.txt", inputs::latin_paragraph);
        let plans = Plans::new();
        assert_shapes(&face, &plans, &latin_text, "Latin");
        op.iter(move || {
            for _ in 0..125 {
                black_box(shape(&face, &plans, &latin_text));
            }
        });
    });
    group.calibrated("shape 1000 characters of Arabic", |op| {
        let face = Sfnt::parse(&arabic).expect("TestShapeAran parses");
        let arabic_text = inputs::store().text("arabic-1000.txt", inputs::arabic_paragraph);
        let plans = Plans::new();
        assert_shapes(&face, &plans, &arabic_text, "Arabic");
        op.iter(move || {
            for _ in 0..50 {
                black_box(shape(&face, &plans, &arabic_text));
            }
        });
    });
    group.calibrated("shape 1000 characters of Devanagari", |op| {
        let face = Sfnt::parse(&devanagari).expect("Noto Sans Devanagari parses");
        let devanagari_text =
            inputs::store().text("devanagari-1000.txt", inputs::devanagari_paragraph);
        let plans = Plans::new();
        assert_shapes(&face, &plans, &devanagari_text, "Devanagari");
        op.iter(move || {
            for _ in 0..8 {
                black_box(shape(&face, &plans, &devanagari_text));
            }
        });
    });
    // A column of mixed orientation (FT-07b.9a), beside the paragraphs along
    // a line: upright ideographs and kana with the comma's and the stop's
    // vertical forms and a `vkrn` pair, Latin words and a number sideways,
    // through a face generated with its vertical metrics.
    group.operation("shape a vertical paragraph", |op| {
        let face = Sfnt::parse(&column).expect("the column face parses");
        let column_text = inputs::store().text("vertical-1000.txt", inputs::vertical_paragraph);
        let plans = Plans::new();
        assert_column(&face, &plans, &column_text);
        op.iter(move || {
            for _ in 0..50 {
                black_box(shape_column(&face, &plans, &column_text));
            }
        });
    });

    group.operation("outlines of Liberation Serif Regular", |op| {
        assert_outlines(&serif, 2602, "Liberation Serif Regular");
        op.per(Per::Outlines(48 * 2602));
        op.iter(|| {
            for _ in 0..48 {
                black_box(outline_face(&serif));
            }
        });
    });
    group.operation("outlines of Liberation Sans Regular", |op| {
        let sans = face("LiberationSans-Regular.ttf", || {
            inputs::liberation("LiberationSans-Regular.ttf")
        });
        assert_outlines(&sans, 2620, "Liberation Sans Regular");
        op.per(Per::Outlines(49 * 2620));
        op.iter(move || {
            for _ in 0..49 {
                black_box(outline_face(&sans));
            }
        });
    });
    group.operation("outlines of Liberation Mono Regular", |op| {
        let mono = face("LiberationMono-Regular.ttf", || {
            inputs::liberation("LiberationMono-Regular.ttf")
        });
        assert_outlines(&mono, 2423, "Liberation Mono Regular");
        op.per(Per::Outlines(51 * 2423));
        op.iter(move || {
            for _ in 0..51 {
                black_box(outline_face(&mono));
            }
        });
    });
    group.calibrated("outlines of every committed CFF program", |op| {
        let named: &[inputs::Fixture] = &cff;
        assert_eq!(named.len(), 193, "the committed CFF programs");
        let refused: Vec<&str> = named
            .iter()
            .filter(|program| Cff::parse(&program.bytes).is_none())
            .map(|program| program.name.as_str())
            .collect();
        let programs: Vec<&[u8]> = named.iter().map(|program| &program.bytes[..]).collect();
        let glyphs: usize = programs
            .iter()
            .filter_map(|program| Cff::parse(program))
            .map(|cff| cff.glyph_count())
            .sum();
        let outlines = outline_programs(&programs);
        // The `sfnt` target's seed is a fuzz input, not a face: its table is no
        // CFF program the parser accepts, so it costs its refusal and adds no
        // outline. Every glyph of the other 192 outlines.
        assert_eq!(
            refused,
            ["opentype-cff.otf"],
            "the programs the parser refuses"
        );
        assert_eq!(glyphs, 18_529, "the 192 programs' glyphs");
        assert_eq!(outlines, glyphs, "every glyph of them yields an outline");
        op.per(Per::Outlines(2 * outlines as u64));
        op.iter(move || {
            for _ in 0..2 {
                black_box(outline_programs(&programs));
            }
        });
    });

    // The variable faces at a named instance (FT-05.17a): the two faces
    // `tinker-pdf-font` commits for its variation tests, read by path, every
    // glyph outlined at the instance through the readers a page or a book
    // draws it by, and the instance written static as the builder embeds it.
    group.operation("outlines of variable-glyf.ttf at Condensed Bold", |op| {
        let face: &[u8] = &variable_glyf;
        #[cfg(not(tinker_planning_baseline))]
        assert_variable_outlines(face, 8, "variable-glyf.ttf", outline_glyf_instance);
        op.per(Per::Outlines(VARIABLE_OUTLINE_CALLS * 8));
        op.iter(move || {
            for _ in 0..VARIABLE_OUTLINE_CALLS {
                black_box(outline_glyf_instance(face, NAMED_INSTANCE));
            }
        });
    });
    group.operation("outlines of variable-cff2.otf at Wide Black", |op| {
        #[cfg(not(tinker_planning_baseline))]
        let face: &[u8] = &variable_cff2;
        #[cfg(not(tinker_planning_baseline))]
        assert_variable_outlines(face, 7, "variable-cff2.otf", outline_cff2_instance);
        // The planning baseline reads no `CFF2` table: its arm outlines the
        // `glyf` face at its default in this one's place, as the operation
        // its gate pairs this one with does there.
        #[cfg(tinker_planning_baseline)]
        let face: &[u8] = &variable_glyf;
        op.per(Per::Outlines(VARIABLE_OUTLINE_CALLS * 7));
        op.iter(move || {
            for _ in 0..VARIABLE_OUTLINE_CALLS {
                black_box(outline_cff2_instance(face, NAMED_INSTANCE));
            }
        });
    });
    group.operation(
        "a static Condensed Bold instance of variable-glyf.ttf",
        |op| {
            let face: &[u8] = &variable_glyf;
            #[cfg(not(tinker_planning_baseline))]
            assert_instance(face, *b"glyf", "variable-glyf.ttf");
            op.per(Per::Bytes(INSTANCE_CALLS * face.len() as u64));
            op.iter(move || {
                for _ in 0..INSTANCE_CALLS {
                    black_box(write_instance(face, NAMED_INSTANCE));
                }
            });
        },
    );
    group.operation("a static Wide Black instance of variable-cff2.otf", |op| {
        #[cfg(not(tinker_planning_baseline))]
        let face: &[u8] = &variable_cff2;
        #[cfg(not(tinker_planning_baseline))]
        assert_instance(face, *b"CFF ", "variable-cff2.otf");
        // The planning baseline reads no `CFF2` table: its arm writes the
        // `glyf` face whole in this one's place, as the operation its gate
        // pairs this one with does there.
        #[cfg(tinker_planning_baseline)]
        let face: &[u8] = &variable_glyf;
        op.per(Per::Bytes(INSTANCE_CALLS * face.len() as u64));
        op.iter(move || {
            for _ in 0..INSTANCE_CALLS {
                black_box(write_instance(face, NAMED_INSTANCE));
            }
        });
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = support::fine_config();
    targets = fonts
}
criterion_main!(benches);
