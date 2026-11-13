//! FT-04b.9: a simple glyph hinted end to end (`docs/design/hinting.md`, "The
//! pipeline", "`gasp` and `head`", "Failure").
//!
//! Every face here is built byte by byte, its glyphs' points and programs
//! chosen so each expected position can be worked by hand from the
//! instructions' semantics: scaling by `round(v · ppem / unitsPerEm)`, the
//! grid rounding, the cut-in and `IUP`'s interpolation, each written into the
//! test as numbers.

use super::super::glyph::{Face, Unhinted, RUNS};
use super::super::state::{Dropout, Point};
use super::super::vm::{Error, MaxProfile, Program};
use super::{nearest, op, program, push};
use crate::outline::Segment;
use crate::sfnt::Sfnt;

/// A point of a test glyph: across and up in font units, and on the curve.
type P = (i16, i16, bool);

/// A simple glyph's `glyf` entry: its contours, none empty, then its
/// program; every flag spelled out and every delta a word.
pub(super) fn simple_glyph(contours: &[&[P]], program: &[u8]) -> Vec<u8> {
    let points: Vec<P> = contours.iter().flat_map(|c| c.iter().copied()).collect();
    let xs = || points.iter().map(|p| p.0);
    let ys = || points.iter().map(|p| p.1);
    let mut g = Vec::new();
    g.extend((contours.len() as i16).to_be_bytes());
    for v in [
        xs().min().unwrap_or(0),
        ys().min().unwrap_or(0),
        xs().max().unwrap_or(0),
        ys().max().unwrap_or(0),
    ] {
        g.extend(v.to_be_bytes());
    }
    let mut end = 0usize;
    for contour in contours {
        end += contour.len();
        g.extend(((end - 1) as u16).to_be_bytes());
    }
    g.extend((program.len() as u16).to_be_bytes());
    g.extend(program);
    for &(_, _, on) in &points {
        g.push(u8::from(on));
    }
    let mut previous = 0i16;
    for x in xs() {
        g.extend((x - previous).to_be_bytes());
        previous = x;
    }
    previous = 0;
    for y in ys() {
        g.extend((y - previous).to_be_bytes());
        previous = y;
    }
    g
}

/// A test face: its glyphs, each glyph's advance and left side bearing, and
/// the tables a hinter reads.
pub(super) struct TestFace {
    pub glyphs: Vec<Vec<u8>>,
    pub metrics: Vec<(u16, i16)>,
    pub units_per_em: u16,
    pub head_flags: u16,
    pub cvt: Vec<i16>,
    pub fpgm: Vec<u8>,
    pub prep: Vec<u8>,
    pub gasp: Option<Vec<(u16, u16)>>,
    /// `None` for a face with no `maxp` at all.
    pub maxp: Option<MaxProfile>,
}

impl TestFace {
    /// A face of `glyphs`, each 1 200 units wide at a side bearing of zero,
    /// 2 048 units an em, with room in `maxp` for every test's programs and
    /// no `cvt `, `fpgm`, `prep` or `gasp`.
    pub(super) fn of(glyphs: Vec<Vec<u8>>) -> TestFace {
        let metrics = vec![(1_200, 0); glyphs.len()];
        TestFace {
            glyphs,
            metrics,
            units_per_em: 2_048,
            head_flags: 0,
            cvt: Vec::new(),
            fpgm: Vec::new(),
            prep: Vec::new(),
            gasp: None,
            maxp: Some(MaxProfile {
                max_twilight_points: 8,
                max_storage: 16,
                max_function_defs: 16,
                max_instruction_defs: 4,
                max_stack_elements: 256,
            }),
        }
    }

    /// The face's bytes: `head`, `hhea` (an ascender of 1 600 and a
    /// descender of −400), `hmtx`, `maxp`, a long `loca`, `glyf`, and each of
    /// `cvt `, `fpgm`, `prep` and `gasp` it has.
    pub(super) fn bytes(&self) -> Vec<u8> {
        let count = self.glyphs.len() as u16;
        let mut head = vec![0u8; 54];
        head[16..18].copy_from_slice(&self.head_flags.to_be_bytes());
        head[18..20].copy_from_slice(&self.units_per_em.to_be_bytes());
        head[50..52].copy_from_slice(&1i16.to_be_bytes());
        let mut hhea = vec![0u8; 36];
        hhea[4..6].copy_from_slice(&1_600i16.to_be_bytes());
        hhea[6..8].copy_from_slice(&(-400i16).to_be_bytes());
        hhea[34..36].copy_from_slice(&count.to_be_bytes());
        let mut hmtx = Vec::new();
        for &(advance, lsb) in &self.metrics {
            hmtx.extend(advance.to_be_bytes());
            hmtx.extend(lsb.to_be_bytes());
        }
        let mut loca = vec![0u8; 4];
        let mut glyf = Vec::new();
        for glyph in &self.glyphs {
            glyf.extend(glyph);
            loca.extend((glyf.len() as u32).to_be_bytes());
        }
        let mut tables: Vec<([u8; 4], Vec<u8>)> = vec![
            (*b"head", head),
            (*b"hhea", hhea),
            (*b"hmtx", hmtx),
            (*b"loca", loca),
            (*b"glyf", glyf),
        ];
        if let Some(profile) = self.maxp {
            let mut maxp = vec![0u8; 32];
            maxp[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
            maxp[4..6].copy_from_slice(&count.to_be_bytes());
            for (at, value) in [
                (16, profile.max_twilight_points),
                (18, profile.max_storage),
                (20, profile.max_function_defs),
                (22, profile.max_instruction_defs),
                (24, profile.max_stack_elements),
            ] {
                maxp[at..at + 2].copy_from_slice(&value.to_be_bytes());
            }
            tables.push((*b"maxp", maxp));
        }
        if !self.cvt.is_empty() {
            let cvt: Vec<u8> = self.cvt.iter().flat_map(|v| v.to_be_bytes()).collect();
            tables.push((*b"cvt ", cvt));
        }
        if !self.fpgm.is_empty() {
            tables.push((*b"fpgm", self.fpgm.clone()));
        }
        if !self.prep.is_empty() {
            tables.push((*b"prep", self.prep.clone()));
        }
        if let Some(ranges) = &self.gasp {
            let mut gasp = Vec::new();
            gasp.extend(1u16.to_be_bytes());
            gasp.extend((ranges.len() as u16).to_be_bytes());
            for &(most, behaviour) in ranges {
                gasp.extend(most.to_be_bytes());
                gasp.extend(behaviour.to_be_bytes());
            }
            tables.push((*b"gasp", gasp));
        }
        let mut out = Vec::new();
        out.extend(0x0001_0000u32.to_be_bytes());
        out.extend((tables.len() as u16).to_be_bytes());
        out.extend([0u8; 6]);
        let mut offset = 12 + 16 * tables.len();
        let mut body = Vec::new();
        for (tag, data) in &tables {
            out.extend(tag);
            out.extend(0u32.to_be_bytes());
            out.extend((offset as u32).to_be_bytes());
            out.extend((data.len() as u32).to_be_bytes());
            body.extend(data);
            while body.len() % 4 != 0 {
                body.push(0);
            }
            offset = 12 + 16 * tables.len() + body.len();
        }
        out.extend(body);
        out
    }
}

/// `v` font units at `ppem` (26.6) of 2 048 units an em, in 26.6:
/// `round(v · ppem / 2 048)`, half away from zero.
fn scaled(v: i64, ppem: i64) -> i64 {
    nearest(v * ppem, 2_048)
}

/// A 26.6 value in pixels.
fn px(v: i64) -> f64 {
    v as f64 / 64.0
}

/// The triangle most tests hint: (100, 0), (1 000, 0) and (550, 1 400), all
/// on the curve.
const TRIANGLE: [P; 3] = [(100, 0, true), (1_000, 0, true), (550, 1_400, true)];

/// The segments the contour reading gives a contour of on-curve points
/// `points`, in pixels: a move to the first, a line to each other and back
/// to the first, and a close.
fn polygon(points: &[(f64, f64)]) -> Vec<Segment> {
    let mut out = vec![Segment::MoveTo {
        x: points[0].0,
        y: points[0].1,
    }];
    for &(x, y) in points[1..].iter().chain(&points[..1]) {
        out.push(Segment::LineTo { x, y });
    }
    out.push(Segment::Close);
    out
}

/// Hints glyph `glyph` of `face` at `ppem` (26.6), grey, asking no stop.
fn hint(face: &TestFace, glyph: u16, ppem: i32) -> Result<super::super::Hinted, Unhinted> {
    let bytes = face.bytes();
    let sfnt = Sfnt::parse(&bytes).expect("the test face parses");
    let hinter = Face::new(&sfnt, None)?;
    hinter
        .at_size(ppem, ppem, false, None)?
        .glyph(&sfnt, glyph, None)
}

/// The runs this thread has counted, reset.
fn runs_since() -> [u32; 3] {
    RUNS.with(|runs| runs.replace([0; 3]))
}

#[test]
fn a_glyph_with_no_program_hints_to_its_points_scaled_to_a_64th() {
    // A triangle and a curve with two off-curve points in a row, whose
    // implied on-curve point is half way between them.
    let curve: [P; 4] = [
        (100, 100, true),
        (300, 700, false),
        (700, 700, false),
        (900, 100, true),
    ];
    let face = TestFace::of(vec![simple_glyph(&[&TRIANGLE, &curve], &[])]);
    for ppem in [12 * 64, 12 * 64 + 32, 16 * 64, 9 * 64 + 13] {
        let hinted = hint(&face, 0, ppem).expect("hinted");
        let p = i64::from(ppem);
        let s = |x: i16, y: i16| (scaled(x.into(), p), scaled(y.into(), p));
        let t: Vec<(i64, i64)> = TRIANGLE.iter().map(|&(x, y, _)| s(x, y)).collect();
        let c: Vec<(i64, i64)> = curve.iter().map(|&(x, y, _)| s(x, y)).collect();
        let pt = |(x, y): (i64, i64)| (px(x), px(y));
        let mut expected = polygon(&t.iter().copied().map(pt).collect::<Vec<_>>());
        // The curve: a move to its first point, a quadratic to the midpoint
        // of its two off-curve points, another to its last point, a line back
        // to the first and a close.
        let middle = (px(c[1].0 + c[2].0) / 2.0, px(c[1].1 + c[2].1) / 2.0);
        expected.extend([
            Segment::MoveTo {
                x: px(c[0].0),
                y: px(c[0].1),
            },
            Segment::QuadTo {
                cx: px(c[1].0),
                cy: px(c[1].1),
                x: middle.0,
                y: middle.1,
            },
            Segment::QuadTo {
                cx: px(c[2].0),
                cy: px(c[2].1),
                x: px(c[3].0),
                y: px(c[3].1),
            },
            Segment::LineTo {
                x: px(c[0].0),
                y: px(c[0].1),
            },
            Segment::Close,
        ]);
        assert_eq!(hinted.outline.segments, expected, "at {ppem}/64 ppem");
        // Every point the glyph states stands on a sixty-fourth: the ends of
        // its lines and the control points of its curves.
        for segment in &hinted.outline.segments {
            let stated: Vec<f64> = match *segment {
                Segment::MoveTo { x, y } | Segment::LineTo { x, y } => vec![x, y],
                Segment::QuadTo { cx, cy, .. } => vec![cx, cy],
                _ => Vec::new(),
            };
            for v in stated {
                assert_eq!((v * 64.0).fract(), 0.0, "{segment:?}: a point on a 64th");
            }
        }
        assert!(!hinted.ran_program, "no program to run");
        assert_eq!(hinted.steps, 0);
        // The phantom points: the origin at xMin less the side bearing, 100,
        // the advance 1 200 past it, and the vertical pair at the ascender
        // and the descender, `hhea`'s, the face having no `vmtx`.
        let phantom = |x: i64, y: i64| Point {
            x: scaled(x, p) as i32,
            y: scaled(y, p) as i32,
        };
        assert_eq!(
            hinted.phantom,
            [
                phantom(100, 0),
                phantom(1_300, 0),
                phantom(0, 1_600),
                phantom(0, -400)
            ],
            "at {ppem}/64 ppem"
        );
    }
    // A glyph with no outline, a space, is its four phantom points.
    let face = TestFace::of(vec![Vec::new()]);
    let space = hint(&face, 0, 12 * 64).expect("a space hints");
    assert!(space.outline.is_empty());
    assert_eq!(
        space.phantom[1].x - space.phantom[0].x,
        scaled(1_200, 768) as i32
    );
}

#[test]
fn a_hand_assembled_program_moves_its_glyph_as_computed_in_the_test() {
    // Along x: MDAP[1] rounds points 0 and 1 to the grid, and IUP[1] places
    // point 2 between them. Along y: MIAP[1] puts point 2 at CVT entry 0,
    // rounded, and IUP[0] shifts points 0 and 1, the contour's untouched
    // ones, by point 2's displacement.
    let code = program(&[
        &[op::SVTCA + 1],
        &push(&[0]),
        &[op::MDAP + 1],
        &push(&[1]),
        &[op::MDAP + 1],
        &[op::IUP + 1],
        &[op::SVTCA],
        &push(&[2, 0]),
        &[op::MIAP + 1],
        &[op::IUP],
    ]);
    let mut face = TestFace::of(vec![simple_glyph(&[&TRIANGLE], &code)]);
    face.cvt = vec![1_408];
    let hinted = hint(&face, 0, 12 * 64).expect("hinted");
    // At 12 pixels an em a font unit is 0.375 sixty-fourths: the triangle's
    // points scale to (37.5 → 38, 0), (375, 0) and (206.25 → 206, 525), and
    // CVT entry 0's 1 408 units to 528.
    // - x: 38 rounds to 64 and 375 to 384; point 2 is untouched across, its
    //   original 206 between 38 and 375, so `64 + (206 − 38) · (384 − 64) /
    //   (375 − 38)`, 64 + 159.53, 224.
    // - y: point 2's original 525 is within the cut-in, 68, of the CVT's 528,
    //   so it takes 528, rounded to 512; points 0 and 1 are untouched up, and
    //   move as point 2 did, by −13, to −13.
    let expected = polygon(&[(px(64), px(-13)), (px(384), px(-13)), (px(224), px(512))]);
    assert_eq!(hinted.outline.segments, expected);
    assert!(hinted.ran_program);
    assert_eq!(
        hinted.steps, 16,
        "ten instructions, and IUP's three points twice"
    );
}

#[test]
fn phantom_points_carry_the_hinted_advance() {
    // Points 3 and 4 are the triangle's horizontal origin and advance width
    // point. MDAP[1] rounds the advance point's x to the grid; SHPIX moves the
    // origin by a quarter pixel.
    let face_with = |code: Vec<u8>| TestFace::of(vec![simple_glyph(&[&TRIANGLE], &code)]);
    // At 12 pixels an em the origin, xMin 100 less a side bearing of 0, scales
    // to 38, and the advance point, 1 300, to 487.5 → 488.
    let unhinted = hint(&face_with(Vec::new()), 0, 768).expect("hinted");
    assert_eq!((unhinted.phantom[0].x, unhinted.phantom[1].x), (38, 488));

    let rounded = hint(
        &face_with(program(&[&[op::SVTCA + 1], &push(&[4]), &[op::MDAP + 1]])),
        0,
        768,
    )
    .expect("hinted");
    assert_eq!(rounded.phantom[1].x, 512, "488 rounds to 8 pixels");
    assert_eq!(
        rounded.phantom[1].x - rounded.phantom[0].x,
        512 - 38,
        "the hinted advance"
    );
    assert_eq!(
        rounded.outline.segments, unhinted.outline.segments,
        "the contour points stay"
    );

    let shifted = hint(
        &face_with(program(&[&[op::SVTCA + 1], &push(&[3, 16]), &[op::SHPIX]])),
        0,
        768,
    )
    .expect("hinted");
    assert_eq!(
        shifted.phantom[0].x,
        38 + 16,
        "the origin moved a quarter pixel"
    );
    assert_eq!(shifted.phantom[1].x - shifted.phantom[0].x, 488 - 54);
    // The vertical pair moves on y alone.
    let raised = hint(
        &face_with(program(&[&[op::SVTCA], &push(&[5, 64]), &[op::SHPIX]])),
        0,
        768,
    )
    .expect("hinted");
    assert_eq!(raised.phantom[2].y, scaled(1_600, 768) as i32 + 64);
    assert_eq!(raised.phantom[2].x, 0);
}

#[test]
fn fpgm_runs_once_per_face_and_prep_once_per_size() {
    // fpgm defines function 0, which rounds point 1 to the grid; prep writes 7
    // into storage location 0; glyph 1 overwrites it in its own copy, and
    // glyph 2 moves point 0 by what it reads there.
    let fpgm = program(&[
        &push(&[0]),
        &[op::FDEF],
        &push(&[1]),
        &[op::MDAP + 1, op::ENDF],
    ]);
    let prep = program(&[&push(&[0, 7]), &[op::WS]]);
    let calls = program(&[&push(&[0]), &[op::CALL]]);
    let overwrites = program(&[&push(&[0, 99]), &[op::WS]]);
    let reads = program(&[&push(&[0, 0]), &[op::RS, op::SHPIX]]);
    let mut face = TestFace::of(vec![
        simple_glyph(&[&TRIANGLE], &calls),
        simple_glyph(&[&TRIANGLE], &overwrites),
        simple_glyph(&[&TRIANGLE], &reads),
        simple_glyph(&[&TRIANGLE], &[]),
    ]);
    face.fpgm = fpgm;
    face.prep = prep;
    let bytes = face.bytes();
    let sfnt = Sfnt::parse(&bytes).expect("parses");

    runs_since();
    let hinter = Face::new(&sfnt, None).expect("fpgm runs");
    assert_eq!(runs_since(), [1, 0, 0], "fpgm once");
    let twelve = hinter.at_size(768, 768, false, None).expect("prep runs");
    assert_eq!(runs_since(), [0, 1, 0], "prep once for the size");
    let alone = twelve.glyph(&sfnt, 2, None).expect("glyph 2");
    for glyph in [0, 1, 2, 3, 2] {
        twelve.glyph(&sfnt, glyph, None).expect("each glyph");
    }
    assert_eq!(
        runs_since(),
        [0, 0, 5],
        "a glyph program each for the five with one, neither fpgm nor prep again"
    );
    let after = twelve.glyph(&sfnt, 2, None).expect("glyph 2 again");
    assert_eq!(
        after.outline.segments, alone.outline.segments,
        "glyph 2 reads prep's 7, not glyph 1's 99, whichever glyphs came before"
    );
    // Point 0 at (38, 0) moved by 7 sixty-fourths across.
    assert_eq!(
        alone.outline.segments[0],
        Segment::MoveTo {
            x: px(38 + 7),
            y: 0.0
        }
    );
    // Function 0, defined by fpgm, rounded glyph 0's point 1 from 375 to 384.
    let called = twelve.glyph(&sfnt, 0, None).expect("glyph 0");
    assert_eq!(
        called.outline.segments[1],
        Segment::LineTo { x: px(384), y: 0.0 }
    );
    let sixteen = hinter
        .at_size(1_024, 1_024, false, None)
        .expect("prep runs");
    sixteen.glyph(&sfnt, 0, None).expect("glyph 0 at 16");
    assert_eq!(
        runs_since(),
        [0, 1, 3],
        "prep once for the second size, and a glyph program for each of the three glyphs since"
    );
    assert!(hinter.steps() > 0 && twelve.steps() > 0 && sixteen.steps() > 0);
}

#[test]
fn an_erring_program_leaves_the_glyph_unhinted_and_reports_it() {
    let divides = program(&[&push(&[1, 0]), &[op::DIV]]);
    let face = TestFace::of(vec![
        simple_glyph(&[&TRIANGLE], &divides),
        simple_glyph(&[&TRIANGLE], &[]),
    ]);
    assert_eq!(
        hint(&face, 0, 768).map(|hinted| hinted.outline.segments),
        Err(Unhinted::Failed {
            program: Program::Glyph,
            error: Error::DivideByZero,
            steps: 2,
        }),
        "the glyph's DIV by zero"
    );
    assert!(hint(&face, 1, 768).is_ok(), "the face's other glyph hints");
    let failed = hint(&face, 0, 768).expect_err("unhinted");
    assert_eq!(
        failed.to_string(),
        "the glyph's program failed: DIV by zero",
        "the report names the program and the rule"
    );

    // prep's error leaves the size unhinted, and fpgm's the face.
    let mut face = TestFace::of(vec![simple_glyph(&[&TRIANGLE], &[])]);
    face.prep = vec![op::POP];
    let bytes = face.bytes();
    let sfnt = Sfnt::parse(&bytes).expect("parses");
    let hinter = Face::new(&sfnt, None).expect("fpgm runs");
    assert!(matches!(
        hinter.at_size(768, 768, false, None),
        Err(Unhinted::Failed {
            program: Program::ControlValue,
            error: Error::StackUnderflow,
            ..
        })
    ));
    face.prep = Vec::new();
    face.fpgm = vec![op::ENDF];
    let bytes = face.bytes();
    let sfnt = Sfnt::parse(&bytes).expect("parses");
    assert!(matches!(
        Face::new(&sfnt, None),
        Err(Unhinted::Failed {
            program: Program::Font,
            error: Error::EndfOutsideDefinition,
            ..
        })
    ));

    // A run its caller stops is stopped by name: function 0, a push and a
    // pop, called 5 000 times passes the 4 096 steps at which a run asks.
    let mut face = TestFace::of(vec![simple_glyph(
        &[&TRIANGLE],
        &program(&[&push(&[5_000, 0]), &[op::LOOPCALL]]),
    )]);
    face.fpgm = program(&[&push(&[0]), &[op::FDEF], &push(&[0]), &[op::POP, op::ENDF]]);
    let bytes = face.bytes();
    let sfnt = Sfnt::parse(&bytes).expect("parses");
    let at = Face::new(&sfnt, None)
        .and_then(|face| face.at_size(768, 768, false, None))
        .expect("set up");
    let stop = || true;
    assert!(matches!(
        at.glyph(&sfnt, 0, Some(&stop)),
        Err(Unhinted::Failed {
            program: Program::Glyph,
            error: Error::Stopped,
            ..
        })
    ));
    assert!(at.glyph(&sfnt, 0, None).is_ok(), "unstopped, it ends");
}

#[test]
fn gasp_and_head_decide_whether_and_at_what_size_a_glyph_is_hinted() {
    let mut face = TestFace::of(vec![simple_glyph(&[&TRIANGLE], &[])]);
    // No grid-fitting to 8 pixels, grid-fitting to 16, none past.
    face.gasp = Some(vec![(8, 0), (16, 1), (0xFFFF, 2)]);
    let bytes = face.bytes();
    let sfnt = Sfnt::parse(&bytes).expect("parses");
    let hinter = Face::new(&sfnt, None).expect("fpgm runs");
    for (pixels, hinted) in [(7, false), (8, false), (9, true), (16, true), (17, false)] {
        assert_eq!(hinter.hints_at(pixels * 64), hinted, "{pixels} pixels");
    }
    // The size's whole pixels per em, rounded: 8.5 is 9.
    assert!(hinter.hints_at(8 * 64 + 32));
    assert!(!hinter.hints_at(8 * 64 + 31));
    runs_since();
    assert_eq!(
        hinter.at_size(17 * 64, 17 * 64, false, None).map(|_| ()),
        Err(Unhinted::NotAtThisSize)
    );
    assert_eq!(
        runs_since(),
        [0, 0, 0],
        "prep never ran for a declined size"
    );

    // A table cut short past its last range leaves the sizes past it.
    face.gasp = Some(vec![(12, 1)]);
    let bytes = face.bytes();
    let sfnt = Sfnt::parse(&bytes).expect("parses");
    let hinter = Face::new(&sfnt, None).expect("fpgm runs");
    assert!(hinter.hints_at(12 * 64));
    assert!(!hinter.hints_at(13 * 64));

    // No gasp: every size from a pixel to 1 024.
    face.gasp = None;
    let bytes = face.bytes();
    let sfnt = Sfnt::parse(&bytes).expect("parses");
    let hinter = Face::new(&sfnt, None).expect("fpgm runs");
    for (ppem, hinted) in [(63, false), (64, true), (65_536, true), (65_537, false)] {
        assert_eq!(hinter.hints_at(ppem), hinted, "{ppem}/64 pixels");
    }

    // head's bit 3: a size of 794/64, about 12.4 pixels, is hinted at 12
    // with it, and at 794/64 without.
    let unrounded = hinter.at_size(794, 794, false, None).expect("12.4");
    assert_eq!(unrounded.size().ppem, 794);
    face.head_flags = 1 << 3;
    let bytes = face.bytes();
    let sfnt = Sfnt::parse(&bytes).expect("parses");
    let hinter = Face::new(&sfnt, None).expect("fpgm runs");
    let rounded = hinter.at_size(794, 794, false, None).expect("12.4");
    assert_eq!(rounded.size().ppem, 768, "integer pixels per em");
    let hinted = rounded.glyph(&sfnt, 0, None).expect("hinted");
    assert_eq!(
        hinted.outline.segments[1],
        Segment::LineTo {
            x: px(scaled(1_000, 768)),
            y: 0.0
        },
        "scaled at 12"
    );
}

#[test]
fn instctrl_in_prep_inhibits_programs_or_resets_their_state() {
    // The glyph rounds point 0's x, 38 at 12 pixels: to the grid, 64, from
    // the defaults; down to the grid, 0, from prep's RDTG.
    let rounds = program(&[&[op::SVTCA + 1], &push(&[0]), &[op::MDAP + 1]]);
    let point_0 = |face: &TestFace| {
        let hinted = hint(face, 0, 768).expect("hinted");
        (hinted.outline.segments[0], hinted.ran_program)
    };
    let mut face = TestFace::of(vec![simple_glyph(&[&TRIANGLE], &rounds)]);
    face.prep = vec![op::RDTG];
    assert_eq!(
        point_0(&face),
        (Segment::MoveTo { x: 0.0, y: 0.0 }, true),
        "prep's round state"
    );
    // INSTCTRL pops a selector, then a value: selector 2 with its flag set.
    face.prep = program(&[&[op::RDTG], &push(&[2, 2]), &[op::INSTCTRL]]);
    assert_eq!(
        point_0(&face),
        (Segment::MoveTo { x: 1.0, y: 0.0 }, true),
        "the summary's defaults"
    );
    // Selector 1: every glyph its scaled points, no program run.
    face.prep = program(&[&push(&[1, 1]), &[op::INSTCTRL]]);
    assert_eq!(
        point_0(&face),
        (Segment::MoveTo { x: px(38), y: 0.0 }, false),
        "grid-fitting inhibited"
    );
}

#[test]
fn a_glyph_program_sees_its_size_and_its_on_curve_flags_reach_the_outline() {
    // MPPEM's whole pixels, as a SHPIX of point 0 that many sixty-fourths.
    let moves = program(&[&[op::SVTCA + 1], &push(&[0]), &[op::MPPEM, op::SHPIX]]);
    let face = TestFace::of(vec![simple_glyph(&[&TRIANGLE], &moves)]);
    for pixels in [9i64, 12, 16] {
        let hinted = hint(&face, 0, (pixels * 64) as i32).expect("hinted");
        assert_eq!(
            hinted.outline.segments[0],
            Segment::MoveTo {
                x: px(scaled(100, pixels * 64) + pixels),
                y: 0.0
            },
            "{pixels} pixels"
        );
    }
    // GETINFO's greyscale bit, 4 096, moved by: grey at one size, monochrome
    // at the other.
    let grey = program(&[&[op::SVTCA + 1], &push(&[0, 32]), &[op::GETINFO, op::SHPIX]]);
    let face = TestFace::of(vec![simple_glyph(&[&TRIANGLE], &grey)]);
    let bytes = face.bytes();
    let sfnt = Sfnt::parse(&bytes).expect("parses");
    let hinter = Face::new(&sfnt, None).expect("fpgm runs");
    for (monochrome, moved) in [(false, 4_096), (true, 0)] {
        let at = hinter.at_size(768, 768, monochrome, None).expect("set up");
        let hinted = at.glyph(&sfnt, 0, None).expect("hinted");
        assert_eq!(
            hinted.outline.segments[0],
            Segment::MoveTo {
                x: px(38 + moved),
                y: 0.0
            },
            "monochrome {monochrome}"
        );
    }
    // FLIPPT takes point 1 off the curve: the outline reads it as a control
    // point.
    let flips = program(&[&push(&[1]), &[op::FLIPPT]]);
    let face = TestFace::of(vec![simple_glyph(&[&TRIANGLE], &flips)]);
    let hinted = hint(&face, 0, 768).expect("hinted");
    let s = |x: i64, y: i64| (px(scaled(x, 768)), px(scaled(y, 768)));
    let (p0, p1, p2) = (s(100, 0), s(1_000, 0), s(550, 1_400));
    assert_eq!(
        hinted.outline.segments,
        [
            Segment::MoveTo { x: p0.0, y: p0.1 },
            Segment::QuadTo {
                cx: p1.0,
                cy: p1.1,
                x: p2.0,
                y: p2.1
            },
            Segment::LineTo { x: p0.0, y: p0.1 },
            Segment::Close,
        ]
    );
}

#[test]
fn the_dropout_state_the_programs_leave_travels_with_the_glyph() {
    // prep turns dropout control on at every size with SCANCTRL 0x1FF and
    // SCANTYPE 4, rules 1, 2 and 5; a glyph program may change the mode.
    let mut face = TestFace::of(vec![
        simple_glyph(&[&TRIANGLE], &[]),
        simple_glyph(&[&TRIANGLE], &program(&[&push(&[1]), &[op::SCANTYPE]])),
    ]);
    face.prep = program(&[
        &push(&[0x1FF]),
        &[op::SCANCTRL],
        &push(&[4]),
        &[op::SCANTYPE],
    ]);
    assert_eq!(
        hint(&face, 0, 768).expect("hinted").dropout,
        Some(Dropout {
            smart: true,
            stubs: true
        })
    );
    assert_eq!(
        hint(&face, 1, 768).expect("hinted").dropout,
        Some(Dropout {
            smart: false,
            stubs: false
        }),
        "the glyph's own SCANTYPE 1"
    );
    face.prep = Vec::new();
    assert_eq!(
        hint(&face, 0, 768).expect("hinted").dropout,
        None,
        "off by default"
    );
}

#[test]
fn a_glyph_that_cannot_be_hinted_says_why() {
    // A face with no maxp has no TrueType profile.
    let mut face = TestFace::of(vec![simple_glyph(&[&TRIANGLE], &[])]);
    face.maxp = None;
    assert_eq!(hint(&face, 0, 768).map(|_| ()), Err(Unhinted::NoProfile));
    // A glyph past loca does not read.
    let face = TestFace::of(vec![simple_glyph(&[&TRIANGLE], &[])]);
    assert_eq!(
        hint(&face, 7, 768).map(|_| ()),
        Err(Unhinted::UnreadableGlyph)
    );
    // A composite: numberOfContours of −1, glyph 0 at an offset.
    let mut composite = Vec::new();
    composite.extend((-1i16).to_be_bytes());
    composite.extend([0u8; 8]);
    composite.extend(0x0003u16.to_be_bytes()); // ARG_1_AND_2_ARE_WORDS | ARGS_ARE_XY_VALUES
    composite.extend(0u16.to_be_bytes());
    composite.extend(10i16.to_be_bytes());
    composite.extend(20i16.to_be_bytes());
    let face = TestFace::of(vec![simple_glyph(&[&TRIANGLE], &[]), composite]);
    assert_eq!(hint(&face, 1, 768).map(|_| ()), Err(Unhinted::Composite));
    // A glyph cut short before its points end.
    let mut cut = simple_glyph(&[&TRIANGLE], &[]);
    cut.truncate(cut.len() - 3);
    let face = TestFace::of(vec![cut]);
    assert_eq!(
        hint(&face, 0, 768).map(|_| ()),
        Err(Unhinted::UnreadableGlyph)
    );
}
