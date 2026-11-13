//! FT-04b.8: `IUP` and the delta exceptions, `DELTAP1` to `DELTAP3` and
//! `DELTAC1` to `DELTAC3` (`docs/design/hinting.md`, "Interpolation and
//! exceptions").
//!
//! The positions `IUP` gives are worked by hand from *IUP*'s rule — an
//! untouched point between two touched points keeps its place relative to
//! them in the original outline, and one outside them moves as the nearer
//! did — and written into the test as numbers; the delta tests compute each
//! expected move from *DELTAP1*'s step table and the size it names.

use super::super::state::{Point, Zone, TOUCHED_X, TOUCHED_Y};
use super::super::storage::Size;
use super::super::vm::{Error, Program, Vm};
use super::{exec, machine, op, program, push, shifted, touches, vectors, D45, SKEW, X, Y};

/// A glyph zone from each point's original and current position and its
/// touched flags, and each contour's last point. Every point is on the curve.
fn zone(points: &[((i32, i32), (i32, i32), u8)], ends: &[usize]) -> Zone {
    let original: Vec<Point> = points
        .iter()
        .map(|&((x, y), _, _)| Point { x, y })
        .collect();
    let current: Vec<Point> = points
        .iter()
        .map(|&(_, (x, y), _)| Point { x, y })
        .collect();
    let mut zone =
        Zone::with_positions(&current, &original, &vec![true; points.len()], ends).expect("a zone");
    for (i, &(_, _, flags)) in points.iter().enumerate() {
        zone.flags[i] |= flags;
    }
    zone
}

/// The glyph `iup_interpolates_and_shifts_untouched_points_as_specified`
/// runs `IUP` over, along `x` or, `vertical`, along `y`: four contours and the
/// four phantom points.
///
/// - contour 0, points 0 to 3: nothing touched, point 2 standing off its
///   original position;
/// - contour 1, points 4 to 7: point 5 alone touched, moved from 30 to 50;
///   point 6 untouched and standing at 999, off its original 60;
/// - contour 2, points 8 to 15, originals 0 to 700 in steps of 100: points 9
///   (100 to 164), 12 (400 to 448) and 14 (600 to 640) touched;
/// - contour 3, points 16 to 19, originals 10, 50, 50 and 90: points 17 (50
///   to 60) and 18 (50 to 80) touched, at one original coordinate;
/// - points 20 to 23, the phantom points, in no contour, 21 off its original.
///
/// Along the other axis every point stands at 7 times its number, current
/// and original, and every point is touched along it, so the flags of the
/// axis interpolated are the only ones that differ.
fn interpolation_glyph(vertical: bool) -> Zone {
    let (along, across) = if vertical {
        (TOUCHED_Y, TOUCHED_X)
    } else {
        (TOUCHED_X, TOUCHED_Y)
    };
    let rows: [(i32, i32, bool); 24] = [
        (0, 0, false),
        (40, 40, false),
        (80, 85, false),
        (120, 120, false),
        (0, 0, false),
        (30, 50, true),
        (60, 999, false),
        (90, 90, false),
        (0, 0, false),
        (100, 164, true),
        (200, 200, false),
        (300, 300, false),
        (400, 448, true),
        (500, 500, false),
        (600, 640, true),
        (700, 700, false),
        (10, 10, false),
        (50, 60, true),
        (50, 80, true),
        (90, 90, false),
        (0, 0, false),
        (500, 520, false),
        (0, 0, false),
        (0, 0, false),
    ];
    let points: Vec<((i32, i32), (i32, i32), u8)> = rows
        .iter()
        .enumerate()
        .map(|(i, &(original, current, touched))| {
            let other = 7 * i as i32;
            let flags = across | if touched { along } else { 0 };
            if vertical {
                ((other, original), (other, current), flags)
            } else {
                ((original, other), (current, other), flags)
            }
        })
        .collect();
    zone(&points, &[3, 7, 15, 19])
}

/// Where `IUP` puts each of [`interpolation_glyph`]'s points along the axis
/// it interpolates, worked by hand:
///
/// - contour 0 has no touched point, so it stays as it stands, point 2 at 85;
/// - contour 1's one touched point moved by 20, so the rest move from their
///   originals by 20: 0 to 20, 60 to 80 (not from 999), 90 to 110;
/// - in contour 2, point 10 is between 9 and 12: `164 + (200 − 100) · (448 −
///   164) / (400 − 100)`, `164 + 94.67`, so 259; point 11 `164 + 189.33`,
///   353; point 13 between 12 and 14, `448 + 100 · 192 / 200`, 544; and the
///   run after 14, points 15 and 8, between 14 and 9: 15's 700 is past 14's
///   600 and moves as 14 did, by 40, to 740, and 8's 0 is below 9's 100 and
///   moves as 9 did, by 64, to 64;
/// - in contour 3 the run after 18, points 19 and 16, is between 18 and 17,
///   both at 50: 19's 90 is above them and moves as 17, the touched point
///   after the run, did, by 10, to 100; 16's 10 is not above them and moves
///   as 18, the one before the run, did, by 30, to 40;
/// - the phantom points are in no contour and stay, 21 at 520.
const INTERPOLATED: [i32; 24] = [
    0, 40, 85, 120, 20, 50, 80, 110, 64, 164, 259, 353, 448, 544, 640, 740, 40, 60, 80, 100, 0,
    520, 0, 0,
];

#[test]
fn iup_interpolates_and_shifts_untouched_points_as_specified() {
    for vertical in [false, true] {
        let glyph = interpolation_glyph(vertical);
        let mut vm = machine();
        vm.set_glyph(glyph.clone());
        // IUP[a]: a = 0 the y axis, 1 the x axis.
        let opcode = if vertical { op::IUP } else { op::IUP + 1 };
        exec(&mut vm, &[opcode]).expect("IUP runs");
        let after = vm.zone(1).expect("the glyph");
        for (i, &expected) in INTERPOLATED.iter().enumerate() {
            let (point, before) = (after.current()[i], glyph.current()[i]);
            let (moved, other, other_before) = if vertical {
                (point.y, point.x, before.x)
            } else {
                (point.x, point.y, before.y)
            };
            assert_eq!(moved, expected, "point {i}, vertical {vertical}");
            assert_eq!(other, other_before, "point {i} keeps its other coordinate");
            assert_eq!(
                after.touched(i),
                glyph.touched(i),
                "point {i}: IUP touches nothing"
            );
            assert_eq!(
                after.original()[i],
                glyph.original()[i],
                "point {i}'s original"
            );
        }
        // One step for IUP and one for each of the twenty contour points.
        assert_eq!(vm.steps(), 21, "a step a point considered");

        // The same glyph again: from where the first left the touched points
        // nothing moves.
        let mut again = machine();
        again.set_glyph(after.clone());
        exec(&mut again, &[opcode]).expect("IUP runs twice");
        assert_eq!(
            again.zone(1).expect("the glyph").current(),
            after.current(),
            "a second IUP over the same touched points moves nothing"
        );
    }
}

/// Points touched along the other axis are untouched along this one, and a
/// glyph whose every point is untouched along the axis stays as it stands.
#[test]
fn iup_reads_only_its_own_axis_flags() {
    // One contour, points touched across alone: IUP[0], along y, moves none
    // of them, and IUP[1], along x, holds every one where it is.
    let points = [
        ((0, 0), (10, 30), TOUCHED_X),
        ((100, 0), (90, 40), TOUCHED_X),
        ((100, 100), (130, 150), TOUCHED_X),
    ];
    let glyph = zone(&points, &[2]);
    for opcode in [op::IUP, op::IUP + 1] {
        let mut vm = machine();
        vm.set_glyph(glyph.clone());
        exec(&mut vm, &[opcode]).expect("IUP runs");
        assert_eq!(vm.zone(1).expect("the glyph").current(), glyph.current());
    }
    // A glyph with no contour has nothing to interpolate.
    let mut vm = machine();
    vm.set_glyph(Zone::glyph(&[], &[], &[]).expect("an empty glyph"));
    exec(&mut vm, &[op::IUP, op::IUP + 1]).expect("IUP over no contour");
    assert_eq!(vm.steps(), 2, "no point considered");
}

/// `IUP` interpolates the glyph's zone, through `zp2`: with `zp2` at the
/// twilight zone it is an error, and with only `zp0` or `zp1` there it is not.
#[test]
fn iup_over_the_twilight_zone_is_an_error() {
    let glyph = interpolation_glyph(false);
    for (setter, refused) in [(op::SZP2, true), (op::SZP0, false), (op::SZP1, false)] {
        let mut vm = machine();
        vm.set_glyph(glyph.clone());
        let result = exec(&mut vm, &program(&[&push(&[0]), &[setter, op::IUP + 1]]));
        if refused {
            assert_eq!(result, Err(Error::TwilightInterpolation));
        } else {
            assert_eq!(result, Ok(()), "zp2 still at the glyph");
        }
    }
}

/// A delta argument's step, from *DELTAP1*'s table: the low four bits 0 to
/// 7 are −8 to −1 and 8 to 15 are 1 to 8, in steps of `1 / 2^shift` pixels,
/// here for a shift of 0 to 6, where a step is whole sixty-fourths.
fn step(arg: i32, shift: i32) -> i64 {
    let selector = arg & 0xF;
    let step = [-8, -7, -6, -5, -4, -3, -2, -1, 1, 2, 3, 4, 5, 6, 7, 8][selector as usize];
    i64::from(step) * (64 >> shift)
}

/// A machine at `ppem` whole pixels per em, its glyph one contour of
/// `points` and its control value table `cvt`, as 26.6 values.
fn at_size(ppem: i32, points: &[(i32, i32)], cvt: &[i32]) -> Vm {
    let mut vm = machine();
    vm.set_size(Size {
        ppem: ppem * 64,
        points: ppem * 64,
        units_per_em: 2048,
        monochrome: false,
    });
    let positions: Vec<Point> = points.iter().map(|&(x, y)| Point { x, y }).collect();
    let ends = if points.is_empty() {
        vec![]
    } else {
        vec![points.len() - 1]
    };
    vm.set_glyph(Zone::glyph(&positions, &vec![true; points.len()], &ends).expect("a glyph"));
    vm.cvt = cvt.to_vec();
    vm
}

/// The three point exceptions, each with how far above `delta_base` its
/// sizes start.
const DELTAPS: [(u8, i32); 3] = [(op::DELTAP1, 0), (op::DELTAP2, 16), (op::DELTAP3, 32)];
/// The three control value exceptions, the same way.
const DELTACS: [(u8, i32); 3] = [(op::DELTAC1, 0), (op::DELTAC2, 16), (op::DELTAC3, 32)];

#[test]
fn deltap_moves_a_point_only_at_its_size() {
    let points = [(100, 200), (-300, 50), (640, -640)];
    // The default delta base and shift, then others SDB and SDS set.
    for (base, shift) in [(None, None), (Some(4), Some(0)), (Some(20), Some(6))] {
        let delta_base = base.unwrap_or(9);
        let delta_shift = shift.unwrap_or(3);
        let mut settings = Vec::new();
        if let Some(base) = base {
            settings.extend(push(&[base]));
            settings.push(op::SDB);
        }
        if let Some(shift) = shift {
            settings.extend(push(&[shift]));
            settings.push(op::SDS);
        }
        for (opcode, offset) in DELTAPS {
            for (f, p) in [(X, X), (Y, Y), (D45, D45), (Y, D45), (SKEW, X)] {
                for (target, arg) in [(0, 0x0F), (1, 0x30), (2, 0xF7), (1, 0x58)] {
                    let size = delta_base + offset + (arg >> 4);
                    for ppem in [size - 1, size, size + 1, size + 16] {
                        if ppem < 1 {
                            continue;
                        }
                        let mut vm = at_size(ppem, &points, &[]);
                        let code = program(&[
                            &settings,
                            &vectors(f, p),
                            &push(&[arg, target, 1]),
                            &[opcode],
                        ]);
                        exec(&mut vm, &code).expect("the delta runs");
                        let glyph = vm.zone(1).expect("the glyph");
                        let before = points[target as usize];
                        let now = glyph.current()[target as usize];
                        let what = format!(
                            "0x{opcode:02X} arg 0x{arg:02X} at {ppem} ppem, size {size}, \
                             vectors {f:?} {p:?}"
                        );
                        if ppem == size {
                            let d = step(arg, delta_shift);
                            let expected = shifted((before.0.into(), before.1.into()), f, p, d);
                            assert_eq!(
                                (i64::from(now.x), i64::from(now.y)),
                                expected,
                                "{what}: moved by its step"
                            );
                            assert_eq!(
                                glyph.touched(target as usize),
                                touches(f),
                                "{what}: touched"
                            );
                        } else {
                            assert_eq!((now.x, now.y), before, "{what}: not its size");
                            assert_eq!(
                                glyph.touched(target as usize),
                                (false, false),
                                "{what}: untouched"
                            );
                        }
                        // The other points never move.
                        for (other, &position) in points.iter().enumerate() {
                            if other != target as usize {
                                let p = glyph.current()[other];
                                assert_eq!((p.x, p.y), position, "{what}: point {other}");
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Several pairs in one exception, each applied at its own size: the pairs
/// are popped point then argument, and only those whose size is the one
/// hinted move; two on one point add.
#[test]
fn deltap_pairs_apply_each_at_its_own_size() {
    let points = [(0, 0), (64, 64), (128, 128)];
    // At 12 ppem, delta base 9: 0x3F (12, +8 steps), 0x2F (11, not), 0x38
    // (12, +1 step) and 0x30 (12, −8 steps), in steps of 1/8 pixel.
    let pairs = [(0, 0x3F), (1, 0x2F), (2, 0x38), (2, 0x30)];
    let mut stack = Vec::new();
    for &(point, arg) in pairs.iter().rev() {
        stack.extend([arg, point]);
    }
    stack.push(pairs.len() as i32);
    let mut vm = at_size(12, &points, &[]);
    exec(&mut vm, &program(&[&push(&stack), &[op::DELTAP1]])).expect("the deltas run");
    let glyph = vm.zone(1).expect("the glyph");
    assert_eq!(glyph.current()[0], Point { x: 64, y: 0 }, "8 steps of 8");
    assert_eq!(glyph.current()[1], Point { x: 64, y: 64 }, "its size is 11");
    assert_eq!(
        glyph.current()[2],
        Point {
            x: 128 + 8 - 64,
            y: 128
        },
        "one step up and eight down"
    );
    assert!(vm.stack().is_empty(), "every pair popped");
    assert_eq!(vm.steps(), 2 + pairs.len() as u32, "a step a pair");
}

/// A point in the zone `zp0` points at, the twilight zone among them, and
/// every target checked at every size; a pair the stack does not hold, and
/// a negative count, which names more pairs than any stack holds, are
/// errors.
#[test]
fn deltap_reads_zp0_and_refuses_what_it_cannot_find() {
    let points = [(0, 0), (64, 64)];
    // Twilight point 3, at the origin, moved at its size.
    let mut vm = at_size(9, &points, &[]);
    let code = program(&[
        &push(&[0]),
        &[op::SZP0],
        &push(&[0x0F, 3, 1]),
        &[op::DELTAP1],
    ]);
    exec(&mut vm, &code).expect("a twilight point moves");
    assert_eq!(
        vm.zone(0).expect("the twilight zone").current()[3],
        Point { x: 64, y: 0 }
    );
    assert_eq!(
        vm.zone(1).expect("the glyph").current()[0],
        Point { x: 0, y: 0 },
        "the glyph's point 3 is not the twilight zone's"
    );

    for ppem in [9, 10] {
        // Point 5 of a glyph of two, at its size and at another.
        let mut vm = at_size(ppem, &points, &[]);
        let code = program(&[&push(&[0x0F, 5, 1]), &[op::DELTAP1]]);
        assert_eq!(
            exec(&mut vm, &code),
            Err(Error::PointIndex(5)),
            "{ppem} ppem"
        );
    }
    for (opcode, _) in DELTAPS {
        let mut vm = at_size(9, &points, &[]);
        let short = program(&[&push(&[0x0F, 1, 2]), &[opcode]]);
        assert_eq!(
            exec(&mut vm, &short),
            Err(Error::StackUnderflow),
            "one pair of two"
        );
        let mut vm = at_size(9, &points, &[]);
        let negative = program(&[&push(&[0x0F, 1, -1]), &[opcode]]);
        assert_eq!(
            exec(&mut vm, &negative),
            Err(Error::StackUnderflow),
            "a count of 2^32 - 1 pairs"
        );
        let mut vm = at_size(9, &points, &[]);
        let none = program(&[&push(&[0]), &[opcode]]);
        assert_eq!(exec(&mut vm, &none), Ok(()), "no pairs");
    }
}

#[test]
fn deltac_changes_a_cvt_entry_only_at_its_size() {
    let cvt = [100, -200, 0, 32_000];
    for (base, shift) in [(None, None), (Some(0), Some(1)), (Some(30), Some(5))] {
        let delta_base = base.unwrap_or(9);
        let delta_shift = shift.unwrap_or(3);
        let mut settings = Vec::new();
        if let Some(base) = base {
            settings.extend(push(&[base]));
            settings.push(op::SDB);
        }
        if let Some(shift) = shift {
            settings.extend(push(&[shift]));
            settings.push(op::SDS);
        }
        for (opcode, offset) in DELTACS {
            for (entry, arg) in [(0, 0x0F), (1, 0x47), (3, 0xF8), (2, 0x10)] {
                let size = delta_base + offset + (arg >> 4);
                for ppem in [size - 1, size, size + 1] {
                    if ppem < 1 {
                        continue;
                    }
                    let mut vm = at_size(ppem, &[], &cvt);
                    let code = program(&[&settings, &push(&[arg, entry, 1]), &[opcode]]);
                    exec(&mut vm, &code).expect("the delta runs");
                    let what = format!("0x{opcode:02X} arg 0x{arg:02X} at {ppem} ppem");
                    for (i, &value) in cvt.iter().enumerate() {
                        let expected = if i == entry as usize && ppem == size {
                            i64::from(value) + step(arg, delta_shift)
                        } else {
                            i64::from(value)
                        };
                        assert_eq!(i64::from(vm.cvt()[i]), expected, "{what}: entry {i}");
                    }
                }
            }
        }
    }
}

/// Several pairs on one entry add, in the order popped, and what a program
/// reads back with `RCVT` is the changed value; an entry past the table is an
/// error at every size.
#[test]
fn deltac_pairs_add_and_read_back() {
    let cvt = [256, 512];
    // At 9 ppem, delta base 9, steps of 1/8: +8 and +1 steps on entry 0.
    let mut vm = at_size(9, &[], &cvt);
    let code = program(&[
        &push(&[0x0F, 0, 0x08, 0, 2]),
        &[op::DELTAC1],
        &push(&[0]),
        &[op::RCVT],
    ]);
    exec(&mut vm, &code).expect("the deltas run");
    assert_eq!(vm.stack(), [256 + 64 + 8], "RCVT reads the changed value");
    assert_eq!(vm.cvt()[1], 512, "entry 1 untouched");

    for ppem in [9, 10] {
        let mut vm = at_size(ppem, &[], &cvt);
        let code = program(&[&push(&[0x0F, 2, 1]), &[op::DELTAC2]]);
        assert_eq!(exec(&mut vm, &code), Err(Error::CvtIndex(2)), "{ppem} ppem");
    }
    let mut vm = at_size(9, &[], &cvt);
    let code = program(&[&push(&[0x0F, -1, 1]), &[op::DELTAC3]]);
    assert_eq!(exec(&mut vm, &code), Err(Error::CvtIndex(-1)));
}

/// A delta runs wherever a program does — the control value program
/// adjusting a CVT entry for a size is the usual case — and an `IDEF` of its
/// opcode is ignored, as for every instruction the set defines.
#[test]
fn the_deltas_and_iup_are_instructions_of_every_program() {
    for program_kind in [Program::Font, Program::ControlValue, Program::Glyph] {
        let mut vm = at_size(9, &[(0, 0)], &[64]);
        vm.set_graphics_state(Default::default());
        let code = program(&[&push(&[0x0F, 0, 1]), &[op::DELTAC1], &[op::IUP]]);
        vm.run(&code, program_kind, None)
            .expect("runs in each program");
        assert_eq!(vm.cvt(), [128], "{program_kind:?}");
    }
    let mut vm = at_size(9, &[(0, 0)], &[64]);
    let code = program(&[
        &push(&[i32::from(op::DELTAC1)]),
        &[op::IDEF],
        &push(&[0]),
        &[op::POP, op::ENDF],
        &push(&[0x0F, 0, 1]),
        &[op::DELTAC1],
    ]);
    vm.run(&code, Program::Font, None)
        .expect("the IDEF is ignored");
    assert_eq!(vm.cvt(), [128], "DELTAC1 is still DELTAC1");
}
