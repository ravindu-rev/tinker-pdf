//! The fonts group: paragraphs shaped and glyph outlines read.
//!
//! The harness, the sampling and the rules every name here keeps are
//! `support/mod.rs`'s; `baseline.json`'s `fonts` list names these operations
//! in this order. Each input is asserted before anything is timed: every
//! paragraph shapes to glyphs and no `.notdef`, every face has its glyph
//! count, and every glyph of every program yields an outline.

mod support;

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion, SamplingMode};
use support::{inputs, Per};
use tinker_pdf_font::{glyf, Cff, Sfnt};
use tinker_pdf_shape::bidi::BaseDirection;
use tinker_pdf_shape::{ShapedRun, Shaper};

/// One call of a shaping operation: a fresh [`Shaper`] over the face, as
/// every caller makes one, and the paragraph shaped through it.
fn shape(face: &Sfnt<'_>, text: &str) -> Vec<ShapedRun> {
    Shaper::new(face).shape_text(text, BaseDirection::Auto).1
}

/// `text` shaped in `face` holds glyphs, and none of them is `.notdef`.
fn assert_shapes(face: &Sfnt<'_>, text: &str, script: &str) {
    assert_eq!(text.chars().count(), 1000, "the {script} paragraph");
    let runs = shape(face, text);
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
/// glyph's outline read. How many outlines that is: a program the parser
/// refuses has none.
fn outline_programs(programs: &[&[u8]]) -> usize {
    let mut outlines = 0;
    for program in programs {
        let Some(cff) = Cff::parse(program) else {
            continue;
        };
        for glyph in 0..cff.glyph_count() {
            let glyph = u16::try_from(glyph).expect("a CFF glyph index fits 16 bits");
            if black_box(cff.outline(glyph)).is_some() {
                outlines += 1;
            }
        }
    }
    outlines
}

fn fonts(c: &mut Criterion) {
    let serif = inputs::liberation("LiberationSerif-Regular.ttf");
    let sans = inputs::liberation("LiberationSans-Regular.ttf");
    let mono = inputs::liberation("LiberationMono-Regular.ttf");
    let arabic = inputs::shape_face("text-rendering-tests/fonts/TestShapeAran.ttf");
    let devanagari = inputs::shape_face("devanagari/NotoSansDevanagari-Regular.ttf");
    let serif_face = Sfnt::parse(&serif).expect("Liberation Serif parses");
    let arabic_face = Sfnt::parse(&arabic).expect("TestShapeAran parses");
    let devanagari_face = Sfnt::parse(&devanagari).expect("Noto Sans Devanagari parses");
    let latin_text = inputs::latin_paragraph();
    let arabic_text = inputs::arabic_paragraph();
    let devanagari_text = inputs::devanagari_paragraph();
    assert_shapes(&serif_face, &latin_text, "Latin");
    assert_shapes(&arabic_face, &arabic_text, "Arabic");
    assert_shapes(&devanagari_face, &devanagari_text, "Devanagari");

    assert_outlines(&serif, 2602, "Liberation Serif Regular");
    assert_outlines(&sans, 2620, "Liberation Sans Regular");
    assert_outlines(&mono, 2423, "Liberation Mono Regular");
    let named = inputs::cff_programs();
    assert_eq!(named.len(), 193, "the committed CFF programs");
    let refused: Vec<&str> = named
        .iter()
        .filter(|(_, program)| Cff::parse(program).is_none())
        .map(|(name, _)| name.as_str())
        .collect();
    let programs: Vec<&[u8]> = named.iter().map(|(_, program)| &program[..]).collect();
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

    let mut group = c.benchmark_group("fonts");
    group.sampling_mode(SamplingMode::Flat);

    // First, because they have no unit: a throughput set on a group stays on
    // it for every operation after, and a shaped paragraph is none of the
    // units.
    group.bench_function("shape 1000 characters of Latin", |b| {
        b.iter(|| {
            for _ in 0..125 {
                black_box(shape(&serif_face, &latin_text));
            }
        });
    });
    group.bench_function("shape 1000 characters of Arabic", |b| {
        b.iter(|| {
            for _ in 0..50 {
                black_box(shape(&arabic_face, &arabic_text));
            }
        });
    });
    group.bench_function("shape 1000 characters of Devanagari", |b| {
        b.iter(|| {
            for _ in 0..42 {
                black_box(shape(&devanagari_face, &devanagari_text));
            }
        });
    });

    group
        .throughput(Per::Outlines(12 * 2602).throughput())
        .bench_function("outlines of Liberation Serif Regular", |b| {
            b.iter(|| {
                for _ in 0..12 {
                    black_box(outline_face(&serif));
                }
            });
        });
    group
        .throughput(Per::Outlines(9 * 2620).throughput())
        .bench_function("outlines of Liberation Sans Regular", |b| {
            b.iter(|| {
                for _ in 0..9 {
                    black_box(outline_face(&sans));
                }
            });
        });
    group
        .throughput(Per::Outlines(14 * 2423).throughput())
        .bench_function("outlines of Liberation Mono Regular", |b| {
            b.iter(|| {
                for _ in 0..14 {
                    black_box(outline_face(&mono));
                }
            });
        });
    group
        .throughput(Per::Outlines(2 * outlines as u64).throughput())
        .bench_function("outlines of every committed CFF program", |b| {
            b.iter(|| {
                for _ in 0..2 {
                    black_box(outline_programs(&programs));
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
