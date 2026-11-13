//! FT-04b.6: `GC`, `SCFS` and `MD` measuring along the vectors, and the five
//! movers — `MDAP`, `MIAP`, `MDRP`, `MIRP` and `MSIRP` — over grids of
//! vectors, round states and flag bits, every expected position, touched flag
//! and reference point computed here from the instructions' semantics.

use super::{machine, model_round, op, program, push};
use crate::hint::state::{GraphicsState, Point, Zone};
use crate::hint::storage::Size;
use crate::hint::vm::{Error, Program, Vm};

/// A vector in 2.14.
type V = (i64, i64);
const X: V = (16_384, 0);
const Y: V = (0, 16_384);
/// 45 degrees: "0x2D41 is the hex equivalent of sqrt(2)/2".
const D45: V = (11_585, 11_585);
/// 135 degrees, orthogonal to [`D45`].
const D135: V = (-11_585, 11_585);
/// Along (300, 420), the direction *GC*'s example measures on.
const SKEW: V = (9_523, 13_332);

/// The freedom and projection vectors the movers are held to, none of them
/// orthogonal.
const VECTORS: [(V, V); 7] = [
    (X, X),
    (Y, Y),
    (D45, D45),
    (Y, D45),
    (D45, X),
    (SKEW, SKEW),
    (X, SKEW),
];

/// Each predefined round state's opcode and its period, phase and threshold
/// in 26.6; `None` for `ROFF`.
const ROUNDS: [(u8, Option<(i64, i64, i64)>); 6] = [
    (op::RTG, Some((64, 0, 32))),
    (op::RTHG, Some((64, 32, 32))),
    (op::RTDG, Some((32, 0, 16))),
    (op::RDTG, Some((64, 0, 0))),
    (op::RUTG, Some((64, 0, 63))),
    (op::ROFF, None),
];

/// `n` rounded by a state of [`ROUNDS`].
fn round(n: i64, state: Option<(i64, i64, i64)>) -> i64 {
    match state {
        None => n,
        Some((period, phase, threshold)) => {
            i64::from(model_round(n as i32, period, phase, threshold))
        }
    }
}

/// `n / d` to the nearest whole, a half away from zero.
fn half_away(n: i128, d: i128) -> i64 {
    let q = (2 * n.abs() + d.abs()) / (2 * d.abs());
    (if (n < 0) != (d < 0) { -q } else { q }) as i64
}

/// `(dx, dy)` projected on `v`, in 26.6.
fn proj(v: V, dx: i64, dy: i64) -> i64 {
    half_away(
        i128::from(dx) * i128::from(v.0) + i128::from(dy) * i128::from(v.1),
        16_384,
    )
}

/// `p` moved along `f` by what changes its projection on `pv` by `d`: `f · d
/// · 16 384 / (f · pv)`, each axis to the nearest whole.
fn shifted(p: (i64, i64), f: V, pv: V, d: i64) -> (i64, i64) {
    let dot = i128::from(f.0 * pv.0 + f.1 * pv.1);
    let scaled = i128::from(d) * 16_384;
    (
        p.0 + half_away(i128::from(f.0) * scaled, dot),
        p.1 + half_away(i128::from(f.1) * scaled, dot),
    )
}

/// The axes a move along `f` touches.
fn touches(f: V) -> (bool, bool) {
    (f.0 != 0, f.1 != 0)
}

/// `d` as the single width `value`, signed as `d`, where its magnitude is
/// within `cut_in` of it.
fn single(d: i64, value: i64, cut_in: i64) -> i64 {
    if (d.abs() - value).abs() < cut_in {
        if d >= 0 {
            value
        } else {
            -value
        }
    } else {
        d
    }
}

/// `d` kept at least `minimum` in its own direction, or `original`'s for a
/// distance of zero.
fn at_least(d: i64, original: i64, minimum: i64) -> i64 {
    let positive = if d == 0 { original >= 0 } else { d > 0 };
    if positive {
        d.max(minimum)
    } else {
        d.min(-minimum)
    }
}

/// A machine at a size that scales font units one to one, its control
/// value table `cvt` (26.6 values), its glyph one contour of `current`
/// points standing over `original` ones.
fn setup(current: &[(i32, i32)], original: &[(i32, i32)], cvt: &[i32]) -> Vm {
    let mut vm = machine();
    vm.set_size(Size {
        ppem: 64,
        points: 64,
        units_per_em: 64,
        monochrome: false,
    });
    let table: Vec<u8> = cvt
        .iter()
        .flat_map(|&v| {
            i16::try_from(v)
                .expect("a CVT value fits a word")
                .to_be_bytes()
        })
        .collect();
    vm.load_cvt(&table);
    let points = |xy: &[(i32, i32)]| xy.iter().map(|&(x, y)| Point { x, y }).collect::<Vec<_>>();
    let ends = if current.is_empty() {
        vec![]
    } else {
        vec![current.len() - 1]
    };
    let zone = Zone::with_positions(
        &points(current),
        &points(original),
        &vec![true; current.len()],
        &ends,
    )
    .expect("a zone");
    vm.set_glyph(zone);
    vm
}

/// The instructions that set the freedom vector to `f` and the projection
/// vector, and the dual with it, to `p`.
fn vectors(f: V, p: V) -> Vec<u8> {
    program(&[
        &push(&[p.0 as i32, p.1 as i32]),
        &[op::SPVFS],
        &push(&[f.0 as i32, f.1 as i32]),
        &[op::SFVFS],
    ])
}

/// Runs `code` as a glyph program from the default graphics state.
fn exec(vm: &mut Vm, code: &[u8]) -> Result<(), Error> {
    vm.set_graphics_state(GraphicsState::default());
    vm.run(code, Program::Glyph, None)
}

/// Glyph point `i`'s current position.
fn at(vm: &Vm, i: usize) -> (i64, i64) {
    let p = vm.zone(1).expect("a glyph").current()[i];
    (i64::from(p.x), i64::from(p.y))
}

/// Glyph point `i`'s touched flags.
fn touched(vm: &Vm, i: usize) -> (bool, bool) {
    vm.zone(1).expect("a glyph").touched(i)
}

/// A pair as 64-bit coordinates.
fn wide((x, y): (i32, i32)) -> (i64, i64) {
    (i64::from(x), i64::from(y))
}

#[test]
fn gc_scfs_and_md_measure_along_the_projection_vector() {
    // *GC*'s example, a point at (300, 420): "First, the projection vector is
    // in the direction of the x axis, and the value 300 is pushed onto the
    // stack. Second ... the y axis, and the value 420 ... Third, the project
    // vector points in the direction of a line from (300,420) to (0,0), and
    // the value -516 is pushed."
    let glyph = [(300, 420), (0, 0)];
    let mut vm = setup(&glyph, &glyph, &[]);
    for (code, expected) in [
        (program(&[&[op::SVTCA + 1], &push(&[0]), &[op::GC]]), 300),
        (program(&[&[op::SVTCA], &push(&[0]), &[op::GC]]), 420),
        (
            program(&[&push(&[1, 0]), &[op::SPVTL], &push(&[0]), &[op::GC]]),
            -516,
        ),
        (
            program(&[&push(&[1, 0]), &[op::SPVTL], &push(&[0]), &[op::GC + 1]]),
            -516,
        ),
    ] {
        assert_eq!(exec(&mut vm, &code), Ok(()));
        assert_eq!(vm.stack(), &[expected], "{code:02X?}");
    }

    // GC[0] reads the current position on the projection vector, GC[1] the
    // original on the dual projection vector; MD[0] the current distance
    // from p2 to p1, MD[1] the original on the dual. SDPVTL from 0 toward 1
    // sets the dual from the original outline, the projection vector from
    // the current one.
    let current = [(0, 0), (640, 64), (-200, 330), (77, -91)];
    let original = [(0, 0), (500, 300), (-150, 280), (90, -60)];
    let mut vm = setup(&current, &original, &[]);
    let line = program(&[&push(&[1, 0]), &[op::SDPVTL]]);
    assert_eq!(exec(&mut vm, &line), Ok(()));
    let pv = vm.graphics_state().projection_vector;
    let dual = vm.graphics_state().dual_projection_vector;
    let (pv, dual) = (
        (i64::from(pv.x), i64::from(pv.y)),
        (i64::from(dual.x), i64::from(dual.y)),
    );
    assert_ne!(pv, dual, "the outlines differ, and so do the vectors");
    for i in 0..4 {
        let (c, o) = (wide(current[i]), wide(original[i]));
        let code = program(&[
            &line,
            &push(&[i as i32]),
            &[op::GC],
            &push(&[i as i32]),
            &[op::GC + 1],
        ]);
        assert_eq!(exec(&mut vm, &code), Ok(()));
        assert_eq!(
            vm.stack(),
            &[proj(pv, c.0, c.1) as i32, proj(dual, o.0, o.1) as i32],
            "GC {i}"
        );
        for j in 0..4 {
            // MD pops p1, then p2: p1 on top.
            let code = program(&[
                &line,
                &push(&[j as i32, i as i32]),
                &[op::MD],
                &push(&[j as i32, i as i32]),
                &[op::MD + 1],
            ]);
            assert_eq!(exec(&mut vm, &code), Ok(()));
            let (cj, oj) = (wide(current[j]), wide(original[j]));
            let grid = proj(pv, c.0 - cj.0, c.1 - cj.1);
            let outline = proj(dual, o.0 - oj.0, o.1 - oj.1);
            assert_eq!(
                vm.stack(),
                &[grid as i32, outline as i32],
                "MD {i} from {j}"
            );
            // "Reversing the order in which the points are listed will change
            // the sign of the result."
            let code = program(&[&line, &push(&[i as i32, j as i32]), &[op::MD]]);
            assert_eq!(exec(&mut vm, &code), Ok(()));
            assert_eq!(vm.stack(), &[-grid as i32], "MD {j} from {i}");
        }
    }
    // Each reads the zone its pointer names: GC zp2's; MD p1 zp1's and p2
    // zp0's. The twilight zone's points stand at the origin.
    let code = program(&[
        &[op::SVTCA + 1],
        &push(&[0]),
        &[op::SZP2],
        &push(&[1]),
        &[op::GC],
    ]);
    assert_eq!(exec(&mut vm, &code), Ok(()));
    assert_eq!(vm.stack(), &[0], "GC of twilight point 1");
    let code = program(&[
        &[op::SVTCA + 1],
        &push(&[0]),
        &[op::SZP0],
        &push(&[3, 1]),
        &[op::MD],
    ]);
    assert_eq!(exec(&mut vm, &code), Ok(()));
    assert_eq!(vm.stack(), &[640], "glyph point 1 from twilight point 3");
    let code = program(&[
        &[op::SVTCA + 1],
        &push(&[0]),
        &[op::SZP1],
        &push(&[1, 3]),
        &[op::MD],
    ]);
    assert_eq!(exec(&mut vm, &code), Ok(()));
    assert_eq!(vm.stack(), &[-640], "twilight point 3 from glyph point 1");

    // SCFS pops a value, then p, and moves p along the freedom vector until
    // its coordinate on the projection vector is the value, touching it.
    for (f, pv) in VECTORS {
        for i in 0..4 {
            for value in [-200, 0, 77, 640, 1000] {
                let mut vm = setup(&current, &original, &[]);
                let code = program(&[&vectors(f, pv), &push(&[i as i32, value]), &[op::SCFS]]);
                assert_eq!(exec(&mut vm, &code), Ok(()));
                let c = wide(current[i]);
                let expected = shifted(c, f, pv, i64::from(value) - proj(pv, c.0, c.1));
                assert_eq!(
                    at(&vm, i),
                    expected,
                    "SCFS {i} to {value} along {f:?} on {pv:?}"
                );
                assert_eq!(touched(&vm, i), touches(f));
                assert_eq!(
                    vm.zone(1).expect("glyph").original()[i],
                    Point {
                        x: original[i].0,
                        y: original[i].1
                    },
                    "a glyph point's original stays"
                );
            }
        }
    }
    // A twilight point SCFS sets takes its new position as its original too.
    let mut vm = setup(&current, &original, &[]);
    let code = program(&[
        &[op::SVTCA + 1],
        &push(&[0]),
        &[op::SZP2],
        &push(&[5, 300]),
        &[op::SCFS],
    ]);
    assert_eq!(exec(&mut vm, &code), Ok(()));
    let twilight = vm.zone(0).expect("twilight");
    assert_eq!(twilight.current()[5], Point { x: 300, y: 0 });
    assert_eq!(twilight.original()[5], Point { x: 300, y: 0 });

    // Errors: a point past its zone, an orthogonal freedom vector at a move,
    // too few values.
    let mut vm = setup(&current, &original, &[]);
    for (code, error) in [
        (program(&[&push(&[4]), &[op::GC]]), Error::PointIndex(4)),
        (
            program(&[&push(&[-1]), &[op::GC + 1]]),
            Error::PointIndex(-1),
        ),
        (program(&[&push(&[4, 0]), &[op::MD]]), Error::PointIndex(4)),
        (
            program(&[&push(&[0, 9]), &[op::MD + 1]]),
            Error::PointIndex(9),
        ),
        (
            program(&[&push(&[4, 10]), &[op::SCFS]]),
            Error::PointIndex(4),
        ),
        (
            program(&[&vectors(Y, X), &push(&[0, 10]), &[op::SCFS]]),
            Error::OrthogonalVectors,
        ),
        (
            program(&[&vectors(D135, D45), &push(&[0, 10]), &[op::SCFS]]),
            Error::OrthogonalVectors,
        ),
        (vec![op::GC], Error::StackUnderflow),
        (program(&[&push(&[0]), &[op::MD]]), Error::StackUnderflow),
        (program(&[&push(&[0]), &[op::SCFS]]), Error::StackUnderflow),
    ] {
        assert_eq!(exec(&mut vm, &code), Err(error), "{code:02X?}");
    }
}

#[test]
fn mdap_moves_as_specified() {
    let points = [(0, 0), (37, 85), (-100, 33), (500, -499), (31, 32)];
    for (f, pv) in VECTORS {
        for (round_op, state) in ROUNDS {
            for flag in [0u8, 1] {
                for (i, &point) in points.iter().enumerate() {
                    let mut vm = setup(&points, &points, &[]);
                    let code = program(&[
                        &vectors(f, pv),
                        &[round_op],
                        &push(&[i as i32]),
                        &[op::MDAP + flag],
                    ]);
                    assert_eq!(exec(&mut vm, &code), Ok(()));
                    let c = wide(point);
                    // MDAP[1]: "the coordinate of point p, as measured against
                    // the projection vector, will be rounded and then moved the
                    // rounded distance"; MDAP[0]: "point p is not moved, but
                    // nonetheless is marked as touched".
                    let expected = if flag == 1 {
                        let coordinate = proj(pv, c.0, c.1);
                        shifted(c, f, pv, round(coordinate, state) - coordinate)
                    } else {
                        c
                    };
                    let case = format!("MDAP[{flag}] {i} along {f:?} on {pv:?}, 0x{round_op:02X}");
                    assert_eq!(at(&vm, i), expected, "{case}");
                    assert_eq!(touched(&vm, i), touches(f), "{case}");
                    // rp0 = rp1 = point p.
                    assert_eq!(vm.graphics_state().rp, [i as u32, i as u32, 0], "{case}");
                    for other in (0..points.len()).filter(|&o| o != i) {
                        assert_eq!(
                            at(&vm, other),
                            wide(points[other]),
                            "{case}: point {other} stays"
                        );
                        assert_eq!(
                            touched(&vm, other),
                            (false, false),
                            "{case}: point {other} untouched"
                        );
                    }
                }
            }
        }
    }
    // Rounding "as if the entire coordinate system has been rotated to be
    // consistent with the projection_vector": on 45 degrees, a point at 2.9
    // pixels along the vector goes to 3.
    let mut vm = setup(&[(131, 131)], &[(131, 131)], &[]);
    let code = program(&[&vectors(D45, D45), &push(&[0]), &[op::MDAP + 1, op::GC]]);
    assert_eq!(exec(&mut vm, &code), Ok(()));
    assert_eq!(vm.stack(), &[192]);
    // It acts on the zone zp0 names.
    let mut vm = setup(&points, &points, &[]);
    let code = program(&[&push(&[0]), &[op::SZP0], &push(&[3]), &[op::MDAP + 1]]);
    assert_eq!(exec(&mut vm, &code), Ok(()));
    assert_eq!(
        at(&vm, 3),
        wide(points[3]),
        "the glyph's point 3 is not the one moved"
    );
    assert_eq!(vm.zone(0).expect("twilight").touched(3), (true, false));
    // An orthogonal freedom vector errs where a point moves, and not where
    // MDAP[0] only touches it.
    assert_eq!(
        exec(
            &mut vm,
            &program(&[&vectors(Y, X), &push(&[1]), &[op::MDAP + 1]])
        ),
        Err(Error::OrthogonalVectors)
    );
    assert_eq!(
        exec(
            &mut vm,
            &program(&[&vectors(Y, X), &push(&[1]), &[op::MDAP]])
        ),
        Ok(())
    );
    assert_eq!(
        exec(&mut vm, &program(&[&push(&[5]), &[op::MDAP]])),
        Err(Error::PointIndex(5))
    );
    assert_eq!(exec(&mut vm, &[op::MDAP + 1]), Err(Error::StackUnderflow));
}

#[test]
fn miap_moves_as_specified() {
    let cvt = [0, 100, -100, 870, 1000, -45];
    // Points standing over originals a little apart, to tell the two apart.
    let current = [(10, 20), (500, -300), (-64, 64), (900, 900)];
    let original = [(10, 20), (480, -320), (0, 0), (870, 870)];
    for (f, pv) in VECTORS {
        for (round_op, state) in ROUNDS {
            for flag in [0u8, 1] {
                for (n, &value) in cvt.iter().enumerate() {
                    for (i, &point) in current.iter().enumerate() {
                        for cut_in in [68, 0, 2000] {
                            let mut vm = setup(&current, &original, &cvt);
                            let code = program(&[
                                &vectors(f, pv),
                                &[round_op],
                                &push(&[cut_in]),
                                &[op::SCVTCI],
                                &push(&[i as i32, n as i32]),
                                &[op::MIAP + flag],
                            ]);
                            assert_eq!(exec(&mut vm, &code), Ok(()));
                            let value = i64::from(value);
                            // a = 1: rounded, and the original position used
                            // "if the device space difference between the CVT
                            // value and the original position is greater than
                            // the control_value_cut_in".
                            let target = if flag == 1 {
                                let o = wide(original[i]);
                                let measured = proj(pv, o.0, o.1);
                                let chosen = if (value - measured).abs() > i64::from(cut_in) {
                                    measured
                                } else {
                                    value
                                };
                                round(chosen, state)
                            } else {
                                value
                            };
                            let c = wide(point);
                            let expected = shifted(c, f, pv, target - proj(pv, c.0, c.1));
                            let case = format!("MIAP[{flag}] {i} to CVT {n} along {f:?} on {pv:?}, 0x{round_op:02X}, cut-in {cut_in}");
                            assert_eq!(at(&vm, i), expected, "{case}");
                            assert_eq!(touched(&vm, i), touches(f), "{case}");
                            assert_eq!(vm.graphics_state().rp, [i as u32, i as u32, 0], "{case}");
                            assert_eq!(
                                vm.zone(1).expect("glyph").original()[i],
                                Point {
                                    x: original[i].0,
                                    y: original[i].1
                                },
                                "{case}"
                            );
                        }
                    }
                }
            }
        }
    }

    // *MIAP*'s example, `MIAP[1] 4 7`, CVT entry 7 at 13.6 pixels: case 1,
    // rounding off, "The point is moved to the position specified in the
    // CVT"; case 2, the cut-in test passing with RTG, "subjected to the
    // rounding rule ... moved to the rounded position", 14; case 3, the test
    // failing with rounding off, "the point is not moved"; case 4, failing
    // with RTG, "moved to the nearest grid position".
    let cvt = [0, 0, 0, 0, 0, 0, 0, 870];
    for (start, round_op, expected) in [
        (832, op::ROFF, 870),
        (832, op::RTG, 896),
        (1299, op::ROFF, 1299),
        (1299, op::RTG, 1280),
    ] {
        let points = [(0, 0), (0, 0), (0, 0), (0, 0), (start, 0)];
        let mut vm = setup(&points, &points, &cvt);
        let code = program(&[&[op::SVTCA + 1, round_op], &push(&[4, 7]), &[op::MIAP + 1]]);
        assert_eq!(exec(&mut vm, &code), Ok(()));
        assert_eq!(at(&vm, 4), (expected, 0), "from {start}, 0x{round_op:02X}");
    }
    // "if round_state is set to 1, and the projection_vector and
    // freedom_vector are at a 45° angle to the x-axis, then a MIAP[1] of a
    // point to 2.9 pixels will round to 3.0 pixels along the
    // projection_vector."
    let mut vm = setup(&[(131, 131)], &[(131, 131)], &[186]);
    let code = program(&[
        &vectors(D45, D45),
        &push(&[0, 0]),
        &[op::MIAP + 1],
        &push(&[0]),
        &[op::GC],
    ]);
    assert_eq!(exec(&mut vm, &code), Ok(()));
    assert_eq!(vm.stack(), &[192]);

    // The twilight zone: a point there has no outline to test the cut-in
    // against, so its measurement is taken to be the CVT's, and the new
    // position is its original too. A glyph point at the origin fails the
    // same test and stays.
    let cvt = [1000];
    let mut vm = setup(&[(0, 0)], &[(0, 0)], &cvt);
    let code = program(&[
        &[op::SVTCA + 1],
        &push(&[0]),
        &[op::SZP0],
        &push(&[2, 0]),
        &[op::MIAP + 1],
    ]);
    assert_eq!(exec(&mut vm, &code), Ok(()));
    let twilight = vm.zone(0).expect("twilight");
    assert_eq!(
        twilight.current()[2],
        Point { x: 1024, y: 0 },
        "the CVT, rounded"
    );
    assert_eq!(twilight.original()[2], Point { x: 1024, y: 0 });
    assert_eq!(twilight.touched(2), (true, false));
    let code = program(&[&[op::SVTCA + 1], &push(&[0, 0]), &[op::MIAP + 1]]);
    assert_eq!(exec(&mut vm, &code), Ok(()));
    assert_eq!(at(&vm, 0), (0, 0), "a glyph point fails the cut-in test");

    // Errors.
    let mut vm = setup(&[(0, 0)], &[(0, 0)], &[5]);
    for (code, error) in [
        (program(&[&push(&[0, 1]), &[op::MIAP]]), Error::CvtIndex(1)),
        (
            program(&[&push(&[0, -1]), &[op::MIAP + 1]]),
            Error::CvtIndex(-1),
        ),
        (
            program(&[&push(&[1, 0]), &[op::MIAP]]),
            Error::PointIndex(1),
        ),
        (
            program(&[&vectors(Y, X), &push(&[0, 0]), &[op::MIAP]]),
            Error::OrthogonalVectors,
        ),
        (program(&[&push(&[0]), &[op::MIAP]]), Error::StackUnderflow),
    ] {
        assert_eq!(exec(&mut vm, &code), Err(error), "{code:02X?}");
    }
}

/// The settings `MDRP` and `MIRP` read beyond the vectors and the round
/// state, as instructions.
fn settings(minimum: i32, single_value: i32, single_cut_in: i32) -> Vec<u8> {
    program(&[
        &push(&[minimum]),
        &[op::SMD],
        &push(&[single_value]),
        &[op::SSW],
        &push(&[single_cut_in]),
        &[op::SSWCI],
    ])
}

/// The movers' points: rp0 (point 0) and three others, each standing over
/// an original apart from it, the last near rp0.
const CURRENT: [(i32, i32); 4] = [(10, 20), (500, 500), (-300, 40), (12, 21)];
const ORIGINAL: [(i32, i32); 4] = [(0, 0), (70, 30), (-130, -60), (3, -2)];

#[test]
fn mdrp_moves_as_specified() {
    for (f, pv) in VECTORS {
        for (round_op, state) in ROUNDS {
            for flags in 0..32u8 {
                for p in 1..4usize {
                    for (minimum, single_value, single_cut_in) in
                        [(64, 0, 0), (0, 128, 40), (100, 64, 100)]
                    {
                        let mut vm = setup(&CURRENT, &ORIGINAL, &[]);
                        let code = program(&[
                            &vectors(f, pv),
                            &[round_op],
                            &settings(minimum, single_value, single_cut_in),
                            &push(&[0]),
                            &[op::SRP0],
                            &push(&[p as i32]),
                            &[op::MDRP + flags],
                        ]);
                        assert_eq!(exec(&mut vm, &code), Ok(()));
                        // The distance in "the original uninstructed outline",
                        // on the dual projection vector (the projection vector
                        // here); the single width where within its cut-in;
                        // rounded where c is set; kept to the minimum where b
                        // is.
                        let (o, r) = (wide(ORIGINAL[p]), wide(ORIGINAL[0]));
                        let original = proj(pv, o.0 - r.0, o.1 - r.1);
                        let mut d =
                            single(original, i64::from(single_value), i64::from(single_cut_in));
                        if flags & 0x04 != 0 {
                            d = round(d, state);
                        }
                        if flags & 0x08 != 0 {
                            d = at_least(d, original, i64::from(minimum));
                        }
                        let (c, rc) = (wide(CURRENT[p]), wide(CURRENT[0]));
                        let expected = shifted(c, f, pv, d - proj(pv, c.0 - rc.0, c.1 - rc.1));
                        let case = format!(
                            "MDRP[{flags:05b}] {p} along {f:?} on {pv:?}, 0x{round_op:02X}, minimum {minimum}, single {single_value} within {single_cut_in}"
                        );
                        assert_eq!(at(&vm, p), expected, "{case}");
                        assert_eq!(touched(&vm, p), touches(f), "{case}");
                        assert_eq!(at(&vm, 0), rc, "{case}: rp0 stays");
                        // "rp1 is set equal to rp0, rp2 is set equal to point p;
                        // if the a flag is set to TRUE, rp0 is set equal to
                        // point p".
                        let rp0 = if flags & 0x10 != 0 { p as u32 } else { 0 };
                        assert_eq!(vm.graphics_state().rp, [rp0, 0, p as u32], "{case}");
                    }
                }
            }
        }
    }

    // *MDRP*'s example 1: rp0 7; `MDRP[00001] 8` leaves rp0 7, rp1 7, rp2 8;
    // `MDRP[10001] 8` leaves rp0 8, rp1 7, rp2 8.
    let points: Vec<(i32, i32)> = (0..9).map(|i| (i * 64, 0)).collect();
    for (flags, rp) in [(0b00001u8, [7, 7, 8]), (0b10001, [8, 7, 8])] {
        let mut vm = setup(&points, &points, &[]);
        let code = program(&[&push(&[7]), &[op::SRP0], &push(&[8]), &[op::MDRP + flags]]);
        assert_eq!(exec(&mut vm, &code), Ok(()));
        assert_eq!(vm.graphics_state().rp, rp, "MDRP[{flags:05b}]");
    }

    // The original distance is measured on the dual projection vector:
    // SDPVTL from point 2 toward point 1 sets it from the original outline.
    let mut vm = setup(&CURRENT, &ORIGINAL, &[]);
    let code = program(&[
        &push(&[1, 2]),
        &[op::SDPVTL, op::SFVTPV],
        &push(&[0]),
        &[op::SRP0],
        &push(&[3]),
        &[op::MDRP],
    ]);
    assert_eq!(exec(&mut vm, &code), Ok(()));
    let gs = *vm.graphics_state();
    let (pv, dual) = (
        (
            i64::from(gs.projection_vector.x),
            i64::from(gs.projection_vector.y),
        ),
        (
            i64::from(gs.dual_projection_vector.x),
            i64::from(gs.dual_projection_vector.y),
        ),
    );
    let (o, r) = (wide(ORIGINAL[3]), wide(ORIGINAL[0]));
    let d = proj(dual, o.0 - r.0, o.1 - r.1);
    let (c, rc) = (wide(CURRENT[3]), wide(CURRENT[0]));
    assert_eq!(
        at(&vm, 3),
        shifted(c, pv, pv, d - proj(pv, c.0 - rc.0, c.1 - rc.1))
    );

    // rp0 is read from zp0's zone and p from zp1's; MDRP creates nothing in
    // the twilight zone, so a twilight point it moves keeps its original.
    let mut vm = setup(&CURRENT, &ORIGINAL, &[]);
    let code = program(&[
        &[op::SVTCA + 1],
        &push(&[0]),
        &[op::SZP1],
        &push(&[0]),
        &[op::SRP0],
        &push(&[4]),
        &[op::MDRP],
    ]);
    assert_eq!(exec(&mut vm, &code), Ok(()));
    let twilight = vm.zone(0).expect("twilight");
    // Twilight point 4's original distance from glyph point 0 is 0 - 0.
    assert_eq!(twilight.current()[4], Point { x: 10, y: 0 }, "at rp0's x");
    assert_eq!(twilight.original()[4], Point::default());

    // Errors: p or rp0 past their zones, an orthogonal freedom vector.
    let mut vm = setup(&CURRENT, &ORIGINAL, &[]);
    for (code, error) in [
        (program(&[&push(&[4]), &[op::MDRP]]), Error::PointIndex(4)),
        (
            program(&[&push(&[9]), &[op::SRP0], &push(&[1]), &[op::MDRP + 0x1F]]),
            Error::PointIndex(9),
        ),
        (
            program(&[&vectors(Y, X), &push(&[1]), &[op::MDRP]]),
            Error::OrthogonalVectors,
        ),
        (vec![op::MDRP], Error::StackUnderflow),
    ] {
        assert_eq!(exec(&mut vm, &code), Err(error), "{code:02X?}");
    }
}

#[test]
fn mirp_moves_as_specified() {
    let cvt = [100, -100, 300, 0];
    let vectors_used = [(X, X), (D45, D45), (Y, D45), (SKEW, SKEW)];
    let rounds_used = [ROUNDS[0], ROUNDS[1], ROUNDS[3], ROUNDS[5]];
    for (f, pv) in vectors_used {
        for (round_op, state) in rounds_used {
            for flags in 0..32u8 {
                for (n, &value) in cvt.iter().enumerate() {
                    for auto_flip in [true, false] {
                        for cut_in in [68, 1000] {
                            for (single_value, single_cut_in) in [(0, 0), (128, 40)] {
                                for p in [1usize, 3] {
                                    let mut vm = setup(&CURRENT, &ORIGINAL, &cvt);
                                    let code = program(&[
                                        &vectors(f, pv),
                                        &[
                                            round_op,
                                            if auto_flip { op::FLIPON } else { op::FLIPOFF },
                                        ],
                                        &push(&[cut_in]),
                                        &[op::SCVTCI],
                                        &settings(64, single_value, single_cut_in),
                                        &push(&[0]),
                                        &[op::SRP0],
                                        &push(&[p as i32, n as i32]),
                                        &[op::MIRP + flags],
                                    ]);
                                    assert_eq!(exec(&mut vm, &code), Ok(()));
                                    let (o, r) = (wide(ORIGINAL[p]), wide(ORIGINAL[0]));
                                    let original = proj(pv, o.0 - r.0, o.1 - r.1);
                                    // The CVT value, the single width where within
                                    // its cut-in, its sign the original distance's
                                    // under auto flip; where c is set, the original
                                    // distance instead past the cut-in, then
                                    // rounded; the minimum where b is set.
                                    let mut c = single(
                                        i64::from(value),
                                        i64::from(single_value),
                                        i64::from(single_cut_in),
                                    );
                                    if auto_flip && (original >= 0) != (c >= 0) {
                                        c = -c;
                                    }
                                    let mut d = c;
                                    if flags & 0x04 != 0 {
                                        if (c - original).abs() > i64::from(cut_in) {
                                            d = original;
                                        }
                                        d = round(d, state);
                                    }
                                    if flags & 0x08 != 0 {
                                        d = at_least(d, original, 64);
                                    }
                                    let (cp, rc) = (wide(CURRENT[p]), wide(CURRENT[0]));
                                    let expected =
                                        shifted(cp, f, pv, d - proj(pv, cp.0 - rc.0, cp.1 - rc.1));
                                    let case = format!(
                                        "MIRP[{flags:05b}] {p} by CVT {n} along {f:?} on {pv:?}, 0x{round_op:02X}, flip {auto_flip}, cut-in {cut_in}, single {single_value} within {single_cut_in}"
                                    );
                                    assert_eq!(at(&vm, p), expected, "{case}");
                                    assert_eq!(touched(&vm, p), touches(f), "{case}");
                                    let rp0 = if flags & 0x10 != 0 { p as u32 } else { 0 };
                                    assert_eq!(
                                        vm.graphics_state().rp,
                                        [rp0, 0, p as u32],
                                        "{case}"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // *MIRP*'s example, `MIRP[00110] 3 17`, CVT entry 17 at 20.7 pixels, the
    // distance in x from rp0 at the origin: case 1, the cut-in passing with
    // rounding off, "the distance from RP0 is equal to that given in CVT
    // entry 17"; case 2, with RTG, "The distance in the CVT is rounded", 21
    // pixels; case 3, the CVT at 13.6 so the test fails, rounding off, "The
    // point is not moved"; case 4, failing with RTG, "The current position of
    // the point is rounded to the grid".
    for (entry, start, round_op, expected) in [
        (1325, 1310, op::ROFF, 1325),
        (1325, 1310, op::RTG, 1344),
        (870, 1325, op::ROFF, 1325),
        (870, 1325, op::RTG, 1344),
    ] {
        let mut cvt = vec![0; 18];
        cvt[17] = entry;
        let points = [(0, 0), (0, 0), (0, 0), (start, 0)];
        let mut vm = setup(&points, &points, &cvt);
        let code = program(&[
            &[op::SVTCA + 1, round_op],
            &push(&[0]),
            &[op::SRP0],
            &push(&[3, 17]),
            &[op::MIRP + 0b00110],
        ]);
        assert_eq!(exec(&mut vm, &code), Ok(()));
        assert_eq!(
            at(&vm, 3),
            (expected, 0),
            "CVT {entry} from {start}, 0x{round_op:02X}"
        );
    }

    // A twilight point MIRP creates: its original distance is taken to be
    // the CVT's, so the cut-in passes whatever its stale original says, and
    // its new position is its original too.
    let mut vm = setup(&CURRENT, &ORIGINAL, &[300]);
    let code = program(&[
        &[op::SVTCA + 1],
        &push(&[0]),
        &[op::SZP1],
        &push(&[0]),
        &[op::SRP0],
        &push(&[6, 0]),
        &[op::MIRP + 0b00100],
    ]);
    assert_eq!(exec(&mut vm, &code), Ok(()));
    let twilight = vm.zone(0).expect("twilight");
    assert_eq!(
        twilight.current()[6],
        Point { x: 10 + 320, y: 0 },
        "rp0's x plus the CVT rounded"
    );
    assert_eq!(twilight.original()[6], twilight.current()[6]);

    // Errors.
    let mut vm = setup(&CURRENT, &ORIGINAL, &[5]);
    for (code, error) in [
        (program(&[&push(&[1, 1]), &[op::MIRP]]), Error::CvtIndex(1)),
        (
            program(&[&push(&[4, 0]), &[op::MIRP]]),
            Error::PointIndex(4),
        ),
        (
            program(&[&push(&[9]), &[op::SRP0], &push(&[1, 0]), &[op::MIRP]]),
            Error::PointIndex(9),
        ),
        (
            program(&[&vectors(Y, X), &push(&[1, 0]), &[op::MIRP]]),
            Error::OrthogonalVectors,
        ),
        (program(&[&push(&[0]), &[op::MIRP]]), Error::StackUnderflow),
    ] {
        assert_eq!(exec(&mut vm, &code), Err(error), "{code:02X?}");
    }
}

#[test]
fn msirp_moves_as_specified() {
    for (f, pv) in VECTORS {
        for flag in [0u8, 1] {
            for d in [0, 64, -100, 333, 1000] {
                for p in 1..4usize {
                    for (round_op, cut_in) in [(op::RTG, 0), (op::RDTG, 68)] {
                        let mut vm = setup(&CURRENT, &ORIGINAL, &[]);
                        let code = program(&[
                            &vectors(f, pv),
                            &[round_op],
                            &push(&[cut_in]),
                            &[op::SCVTCI],
                            &push(&[0]),
                            &[op::SRP0],
                            &push(&[p as i32, d]),
                            &[op::MSIRP + flag],
                        ]);
                        assert_eq!(exec(&mut vm, &code), Ok(()));
                        // "Makes the distance between a point p and rp0 equal to
                        // the value specified on the stack", which the cut-in
                        // and the round state do not touch.
                        let (c, rc) = (wide(CURRENT[p]), wide(CURRENT[0]));
                        let expected =
                            shifted(c, f, pv, i64::from(d) - proj(pv, c.0 - rc.0, c.1 - rc.1));
                        let case = format!("MSIRP[{flag}] {p} by {d} along {f:?} on {pv:?}");
                        assert_eq!(at(&vm, p), expected, "{case}");
                        assert_eq!(touched(&vm, p), touches(f), "{case}");
                        let rp0 = if flag == 1 { p as u32 } else { 0 };
                        assert_eq!(vm.graphics_state().rp, [rp0, 0, p as u32], "{case}");
                    }
                }
            }
        }
    }
    // A twilight point MSIRP sets takes the position as its original.
    let mut vm = setup(&CURRENT, &ORIGINAL, &[]);
    let code = program(&[
        &[op::SVTCA],
        &push(&[0]),
        &[op::SZP1],
        &push(&[1]),
        &[op::SRP0],
        &push(&[7, -64]),
        &[op::MSIRP + 1],
    ]);
    assert_eq!(exec(&mut vm, &code), Ok(()));
    let twilight = vm.zone(0).expect("twilight");
    assert_eq!(twilight.current()[7], Point { x: 0, y: 500 - 64 });
    assert_eq!(twilight.original()[7], twilight.current()[7]);
    assert_eq!(vm.graphics_state().rp, [7, 1, 7]);

    let mut vm = setup(&CURRENT, &ORIGINAL, &[]);
    for (code, error) in [
        (
            program(&[&push(&[4, 0]), &[op::MSIRP]]),
            Error::PointIndex(4),
        ),
        (
            program(&[&push(&[9]), &[op::SRP0], &push(&[1, 0]), &[op::MSIRP + 1]]),
            Error::PointIndex(9),
        ),
        (
            program(&[&vectors(D135, D45), &push(&[1, 0]), &[op::MSIRP]]),
            Error::OrthogonalVectors,
        ),
        (program(&[&push(&[0]), &[op::MSIRP]]), Error::StackUnderflow),
    ] {
        assert_eq!(exec(&mut vm, &code), Err(error), "{code:02X?}");
    }
}
