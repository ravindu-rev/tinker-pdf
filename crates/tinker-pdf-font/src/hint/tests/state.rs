//! FT-04b.5: the vectors, the reference points, the zone pointers and the
//! loop, the round states and what rounds by them, the distance and delta
//! settings, and the dropout state `SCANCTRL` and `SCANTYPE` set.

use super::{machine, model_round, nearest, op, program, push};
use crate::hint::state::{Dropout, GraphicsState, Point, Round, Vector, Zone};
use crate::hint::storage::Size;
use crate::hint::vm::{Error, Program, Vm};

/// Points from pairs.
fn points(xy: &[(i32, i32)]) -> Vec<Point> {
    xy.iter().map(|&(x, y)| Point { x, y }).collect()
}

/// A machine whose glyph is one contour of `current` points standing over
/// `original` ones, all on the curve.
fn with_glyph(current: &[(i32, i32)], original: &[(i32, i32)]) -> Vm {
    let mut vm = machine();
    let on = vec![true; current.len()];
    let ends: Vec<usize> = if current.is_empty() {
        vec![]
    } else {
        vec![current.len() - 1]
    };
    let zone =
        Zone::with_positions(&points(current), &points(original), &on, &ends).expect("a zone");
    vm.set_glyph(zone);
    vm
}

/// The graphics state `code`, run as a glyph program on `vm` from the
/// default state, leaves.
fn after(vm: &mut Vm, code: &[u8]) -> Result<GraphicsState, Error> {
    vm.set_graphics_state(GraphicsState::default());
    vm.run(code, Program::Glyph, None)?;
    Ok(*vm.graphics_state())
}

/// The unit vector along `(dx, dy)` as the design states it: each component
/// 16 384 · d / |d| to the nearest whole, the x axis for no difference.
fn unit(dx: i64, dy: i64) -> Vector {
    if dx == 0 && dy == 0 {
        return Vector { x: 16384, y: 0 };
    }
    let length = ((dx * dx + dy * dy) as f64).sqrt();
    let component = |d: i64| (16384.0 * d as f64 / length).round() as i32;
    Vector {
        x: component(dx),
        y: component(dy),
    }
}

#[test]
fn every_vector_instruction_sets_its_specified_unit_vector() {
    let x = Vector { x: 16384, y: 0 };
    let y = Vector { x: 0, y: 16384 };
    assert_eq!(Vector::X, x);
    assert_eq!(Vector::Y, y);

    // SVTCA[a], SPVTCA[a], SFVTCA[a]: a = 0 the y axis, 1 the x axis.
    // SVTCA sets both, "equivalent to SFVTCA[1] followed by SPVTCA[1]".
    let mut vm = machine();
    for (code, freedom, projection) in [
        (vec![op::SVTCA], y, y),
        (vec![op::SVTCA + 1], x, x),
        (vec![op::SPVTCA], x, y),
        (vec![op::SPVTCA + 1], x, x),
        (vec![op::SFVTCA], y, x),
        (vec![op::SFVTCA + 1], x, x),
        (vec![op::SVTCA, op::SFVTCA + 1], x, y),
        (vec![op::SVTCA, op::SPVTCA + 1], y, x),
    ] {
        let state = after(&mut vm, &code).expect("runs");
        assert_eq!(state.freedom_vector, freedom, "{code:02X?}");
        assert_eq!(state.projection_vector, projection, "{code:02X?}");
        assert_eq!(
            state.dual_projection_vector, projection,
            "{code:02X?}: the dual follows"
        );
    }
    assert_eq!(
        GraphicsState::default().projection_vector,
        x,
        "the default is the x axis"
    );
    assert_eq!(GraphicsState::default().freedom_vector, x);
    assert_eq!(GraphicsState::default().dual_projection_vector, x);

    // SPVTL[a] and SFVTL[a] pop p1, then p2; p1 is in the zone zp2 points
    // at and p2 in zp1's; the vector points "from p1 toward p2", and a = 1
    // turns it counter-clockwise.
    let glyph = [(0, 0), (300, 420), (64, 64), (-128, 50), (5, 0), (5, 0)];
    let mut vm = with_glyph(&glyph, &glyph);
    for (p1, p2) in [
        (0usize, 1usize),
        (1, 0),
        (0, 2),
        (2, 0),
        (3, 4),
        (4, 3),
        (1, 3),
        (4, 5),
    ] {
        let (a, b) = (glyph[p1], glyph[p2]);
        let parallel = unit(i64::from(b.0 - a.0), i64::from(b.1 - a.1));
        let perpendicular = Vector {
            x: -parallel.y,
            y: parallel.x,
        };
        let pushed = push(&[p2 as i32, p1 as i32]);
        for (flag, expected) in [(0, parallel), (1, perpendicular)] {
            let state = after(&mut vm, &program(&[&pushed, &[op::SPVTL + flag]])).expect("SPVTL");
            assert_eq!(
                state.projection_vector, expected,
                "SPVTL[{flag}] {p1} to {p2}"
            );
            assert_eq!(
                state.dual_projection_vector, expected,
                "SPVTL[{flag}] sets the dual"
            );
            assert_eq!(
                state.freedom_vector, x,
                "SPVTL[{flag}] leaves the freedom vector"
            );
            let state = after(&mut vm, &program(&[&pushed, &[op::SFVTL + flag]])).expect("SFVTL");
            assert_eq!(state.freedom_vector, expected, "SFVTL[{flag}] {p1} to {p2}");
            assert_eq!(
                state.projection_vector, x,
                "SFVTL[{flag}] leaves the projection vector"
            );
        }
    }
    // *GPV*'s case 3: "0x2D41 is the hex equivalent of sqrt(2)/2", the
    // vector at 45 degrees; and its reverse (*SPVTL*'s case 3: "The order in
    // which the points are specified matters").
    let code = program(&[&push(&[2, 0]), &[op::SPVTL, op::GPV]]);
    vm.set_graphics_state(GraphicsState::default());
    assert_eq!(vm.run(&code, Program::Glyph, None), Ok(()));
    assert_eq!(vm.stack(), &[0x2D41, 0x2D41]);
    let code = program(&[&push(&[0, 2]), &[op::SPVTL, op::GPV]]);
    assert_eq!(vm.run(&code, Program::Glyph, None), Ok(()));
    assert_eq!(vm.stack(), &[-0x2D41, -0x2D41]);
    // Two points at one place give the x axis, this engine's choice.
    let state = after(&mut vm, &program(&[&push(&[5, 4]), &[op::SPVTL + 1]])).expect("runs");
    assert_eq!(
        state.projection_vector,
        Vector { x: 0, y: 16384 },
        "the x axis, turned"
    );
    assert_eq!(Vector::along(0, 0), x);

    // p1 is read from zp2's zone and p2 from zp1's: with zp2 at the twilight
    // zone, whose points are at the origin, p1 = 3 is the origin.
    let code = program(&[&push(&[0]), &[op::SZP2], &push(&[4, 3]), &[op::SPVTL]]);
    let state = after(&mut vm, &code).expect("runs");
    assert_eq!(state.projection_vector, unit(5, 0));
    let code = program(&[&push(&[0]), &[op::SZP1], &push(&[4, 3]), &[op::SPVTL]]);
    let state = after(&mut vm, &code).expect("runs");
    assert_eq!(
        state.projection_vector,
        unit(128, -50),
        "p2 = 4 of the twilight zone is the origin"
    );

    // SDPVTL[a]: the dual vector from the points' original positions, "the
    // scaled outline before any grid-fitting took place", and the projection
    // vector from their current ones; the freedom vector is left.
    let current = [(0, 0), (640, 0), (100, 100)];
    let original = [(0, 0), (600, 64), (-50, 300)];
    let mut vm = with_glyph(&current, &original);
    for (p1, p2) in [(0usize, 1usize), (1, 2), (2, 0)] {
        for flag in [0u8, 1] {
            let code = program(&[&push(&[p2 as i32, p1 as i32]), &[op::SDPVTL + flag]]);
            let state = after(&mut vm, &code).expect("SDPVTL");
            let turn = |v: Vector| {
                if flag == 1 {
                    Vector { x: -v.y, y: v.x }
                } else {
                    v
                }
            };
            let along = |a: (i32, i32), b: (i32, i32)| {
                turn(unit(i64::from(b.0 - a.0), i64::from(b.1 - a.1)))
            };
            assert_eq!(
                state.projection_vector,
                along(current[p1], current[p2]),
                "SDPVTL[{flag}] {p1} {p2}"
            );
            assert_eq!(
                state.dual_projection_vector,
                along(original[p1], original[p2]),
                "SDPVTL[{flag}] {p1} {p2}"
            );
            assert_eq!(state.freedom_vector, x);
        }
    }
    // "The dual_projection_vector will disappear when any other instruction
    // that sets the projection_vector is used."
    for setter in [
        vec![op::SPVTCA + 1],
        vec![op::SVTCA],
        program(&[&push(&[0, 1]), &[op::SPVTL]]),
        program(&[&push(&[16384, 0]), &[op::SPVFS]]),
    ] {
        let code = program(&[&push(&[1, 2]), &[op::SDPVTL], &setter]);
        let state = after(&mut vm, &code).expect("runs");
        assert_eq!(
            state.dual_projection_vector, state.projection_vector,
            "{setter:02X?}"
        );
    }
    // SFVTL leaves the dual as SDPVTL set it: from p1 = 2 toward p2 = 1 in
    // the original outline.
    let code = program(&[&push(&[1, 2]), &[op::SDPVTL], &push(&[0, 1]), &[op::SFVTL]]);
    let state = after(&mut vm, &code).expect("runs");
    assert_eq!(state.dual_projection_vector, unit(650, -236));
    assert_eq!(state.projection_vector, unit(540, -100));

    // SPVFS[ ] and SFVFS[ ] pop y, then x, 2.14 values "padded with zeroes"
    // in the low 16 bits, taken as given; GPV[ ] and GFV[ ] push x, then y.
    let mut vm = machine();
    for (vx, vy) in [
        (16384, 0),
        (0, 16384),
        (11585, 11585),
        (-16384, 0),
        (100, -200),
        (-11596, 11565),
    ] {
        let code = program(&[&push(&[vx, vy]), &[op::SPVFS, op::GPV]]);
        vm.set_graphics_state(GraphicsState::default());
        assert_eq!(vm.run(&code, Program::Glyph, None), Ok(()));
        assert_eq!(vm.stack(), &[vx, vy], "SPVFS {vx} {vy}");
        assert_eq!(
            vm.graphics_state().projection_vector,
            Vector { x: vx, y: vy }
        );
        assert_eq!(
            vm.graphics_state().dual_projection_vector,
            Vector { x: vx, y: vy }
        );
        let code = program(&[&push(&[vx, vy]), &[op::SFVFS, op::GFV]]);
        vm.set_graphics_state(GraphicsState::default());
        assert_eq!(vm.run(&code, Program::Glyph, None), Ok(()));
        assert_eq!(vm.stack(), &[vx, vy], "SFVFS {vx} {vy}");
        assert_eq!(vm.graphics_state().freedom_vector, Vector { x: vx, y: vy });
        assert_eq!(vm.graphics_state().projection_vector, x);
    }
    // The bits above the low 16 are padding.
    vm.set_graphics_state(GraphicsState::default());
    assert_eq!(
        vm.run_on(&[0x0001_4000, 0x7FFF_0000], &[op::SPVFS], Program::Glyph),
        Ok(())
    );
    assert_eq!(
        vm.graphics_state().projection_vector,
        Vector { x: 0x4000, y: 0 }
    );
    vm.set_graphics_state(GraphicsState::default());
    assert_eq!(
        vm.run_on(&[0x0000_C000, 0], &[op::SFVFS], Program::Glyph),
        Ok(())
    );
    assert_eq!(
        vm.graphics_state().freedom_vector,
        Vector { x: -16384, y: 0 }
    );

    // SFVTPV[ ]: the freedom vector set to the projection vector.
    let code = program(&[&push(&[11585, -11585]), &[op::SPVFS, op::SFVTPV, op::GFV]]);
    vm.set_graphics_state(GraphicsState::default());
    assert_eq!(vm.run(&code, Program::Glyph, None), Ok(()));
    assert_eq!(vm.stack(), &[11585, -11585]);

    // A point past its zone, or too few values to pop.
    let mut vm = with_glyph(&glyph, &glyph);
    for (code, p) in [
        (program(&[&push(&[0, 6]), &[op::SPVTL]]), 6),
        (program(&[&push(&[6, 0]), &[op::SFVTL + 1]]), 6),
        (program(&[&push(&[0, -1]), &[op::SDPVTL]]), -1),
    ] {
        assert_eq!(
            after(&mut vm, &code),
            Err(Error::PointIndex(p)),
            "{code:02X?}"
        );
    }
    let mut empty = machine();
    assert_eq!(
        after(&mut empty, &program(&[&push(&[0, 0]), &[op::SPVTL]])),
        Err(Error::PointIndex(0)),
        "no glyph"
    );
    for opcode in [op::SPVTL, op::SFVTL, op::SDPVTL, op::SPVFS, op::SFVFS] {
        assert_eq!(
            vm.run_on(&[0], &[opcode], Program::Glyph),
            Err(Error::StackUnderflow),
            "0x{opcode:02X}"
        );
    }
}

#[test]
fn reference_points_zones_and_the_loop_are_set_as_specified() {
    let defaults = GraphicsState::default();
    assert_eq!(defaults.rp, [0, 0, 0]);
    assert_eq!(defaults.zp, [1, 1, 1], "every zone pointer at the glyph");
    assert_eq!(defaults.loop_count, 1);

    // SRP0[ ] to SRP2[ ] pop a point number, kept as it is popped and
    // checked where it is used.
    let mut vm = machine();
    for (i, opcode) in [op::SRP0, op::SRP1, op::SRP2].into_iter().enumerate() {
        for p in [0, 7, 1000] {
            let state = after(&mut vm, &program(&[&push(&[p]), &[opcode]])).expect("runs");
            let mut expected = [0u32; 3];
            expected[i] = p as u32;
            assert_eq!(state.rp, expected, "SRP{i} {p}");
        }
    }
    vm.set_graphics_state(GraphicsState::default());
    assert_eq!(vm.run_on(&[-1], &[op::SRP1], Program::Glyph), Ok(()));
    assert_eq!(vm.graphics_state().rp, [0, u32::MAX, 0], "a uint32");

    // SZP0[ ] to SZP2[ ] and SZPS[ ]: 0 the twilight zone, 1 the glyph; "Any
    // other value for n is an error."
    for (i, opcode) in [op::SZP0, op::SZP1, op::SZP2].into_iter().enumerate() {
        let state = after(&mut vm, &program(&[&push(&[0]), &[opcode]])).expect("runs");
        let mut expected = [1u8; 3];
        expected[i] = 0;
        assert_eq!(state.zp, expected, "SZP{i} 0");
        let state = after(
            &mut vm,
            &program(&[&push(&[0]), &[opcode], &push(&[1]), &[opcode]]),
        )
        .expect("runs");
        assert_eq!(state.zp, [1; 3], "SZP{i} back to 1");
    }
    let state = after(&mut vm, &program(&[&push(&[0]), &[op::SZPS]])).expect("runs");
    assert_eq!(state.zp, [0; 3]);
    let state = after(
        &mut vm,
        &program(&[&push(&[0]), &[op::SZPS], &push(&[1]), &[op::SZP1]]),
    )
    .expect("runs");
    assert_eq!(state.zp, [0, 1, 0]);
    for opcode in [op::SZP0, op::SZP1, op::SZP2, op::SZPS] {
        for n in [2, -1, 256] {
            let code = program(&[&push(&[n]), &[opcode]]);
            assert_eq!(
                after(&mut vm, &code),
                Err(Error::ZoneIndex(n)),
                "0x{opcode:02X} {n}"
            );
        }
    }

    // SLOOP[ ]: a count of one or more; "Setting the loop variable to zero is
    // an error", and a count below it is too.
    for n in [1, 2, 255, 32767] {
        let state = after(&mut vm, &program(&[&push(&[n]), &[op::SLOOP]])).expect("runs");
        assert_eq!(state.loop_count, n);
    }
    for n in [0, -1] {
        assert_eq!(
            after(&mut vm, &program(&[&push(&[n]), &[op::SLOOP]])),
            Err(Error::LoopCount(n))
        );
    }

    // A zone pointer no instruction could set reads as no zone.
    let mut vm = with_glyph(&[(0, 0), (64, 0)], &[(0, 0), (64, 0)]);
    let mut broken = GraphicsState::default();
    broken.zp[2] = 7;
    vm.set_graphics_state(broken);
    assert_eq!(
        vm.run(
            &program(&[&push(&[1, 0]), &[op::SPVTL]]),
            Program::Glyph,
            None
        ),
        Err(Error::ZoneIndex(7))
    );

    // A zone is built whole or not at all: flags for every point, contours
    // that end in increasing order within the points; phantom points after
    // the last contour belong to none.
    let four = points(&[(0, 0), (1, 1), (2, 2), (3, 3)]);
    assert!(
        Zone::glyph(&four, &[true; 3], &[3]).is_none(),
        "three flags for four points"
    );
    assert!(
        Zone::glyph(&four, &[true; 4], &[2, 2]).is_none(),
        "ends that do not increase"
    );
    assert!(
        Zone::glyph(&four, &[true; 4], &[4]).is_none(),
        "an end past the points"
    );
    assert!(
        Zone::with_positions(&four, &four[..3], &[true; 4], &[3]).is_none(),
        "positions that differ in number"
    );
    let zone = Zone::glyph(&four, &[true, false, true, false], &[1])
        .expect("two contour points, two after");
    assert_eq!(zone.len(), 4);
    assert_eq!(zone.contour_ends(), &[1]);
    assert_eq!(
        (zone.on_curve(0), zone.on_curve(1), zone.on_curve(9)),
        (true, false, false)
    );
    assert_eq!(zone.touched(0), (false, false));
    assert_eq!(zone.current(), zone.original());
    let twilight = Zone::twilight(3);
    assert_eq!(twilight.current(), &[Point::default(); 3]);
    assert_eq!(twilight.original(), &[Point::default(); 3]);
    assert!(twilight.contour_ends().is_empty());
    assert_eq!(
        machine().zone(0).map(Zone::len),
        Some(16),
        "maxTwilightPoints"
    );
    assert_eq!(machine().zone(1).map(Zone::len), Some(0), "no glyph yet");
    assert!(machine().zone(2).is_none());
}

#[test]
fn every_round_state_rounds_as_specified() {
    // The predefined states as Apple's table 19 gives each: period, phase
    // and threshold, in 26.6.
    let states = [
        (op::RTG, Round::Grid, 64, 0, 32),
        (op::RTHG, Round::HalfGrid, 64, 32, 32),
        (op::RTDG, Round::DoubleGrid, 32, 0, 16),
        (op::RDTG, Round::DownToGrid, 64, 0, 0),
        (op::RUTG, Round::UpToGrid, 64, 0, 63),
    ];
    let mut vm = machine();
    for (opcode, state, period, phase, threshold) in states {
        assert_eq!(after(&mut vm, &[opcode]).expect("runs").round_state, state);
        for n in -300..=300 {
            let expected = model_round(n, period, phase, threshold);
            assert_eq!(state.apply(n), expected, "0x{opcode:02X} {n}");
            for ab in 0..4 {
                // ROUND[ab]: compensation for each distance type is zero here,
                // and type 3 reads as grey.
                vm.set_graphics_state(GraphicsState::default());
                assert_eq!(
                    vm.run_on(&[n], &[opcode, op::ROUND + ab], Program::Glyph),
                    Ok(())
                );
                assert_eq!(vm.stack(), &[expected], "0x{opcode:02X} ROUND[{ab}] {n}");
            }
        }
    }
    // ROFF: "rounding is turned off"; NROUND only compensates, so it gives
    // every value back in every state.
    assert_eq!(
        after(&mut vm, &[op::ROFF]).expect("runs").round_state,
        Round::Off
    );
    for n in [-300, -33, -32, -1, 0, 1, 31, 32, 300, i32::MIN, i32::MAX] {
        for (code, expected) in [
            ([op::ROFF, op::ROUND], n),
            ([op::RTG, op::NROUND], n),
            ([op::RUTG, op::NROUND + 3], n),
        ] {
            vm.set_graphics_state(GraphicsState::default());
            assert_eq!(vm.run_on(&[n], &code, Program::Glyph), Ok(()));
            assert_eq!(vm.stack(), &[expected], "{code:02X?} {n}");
        }
    }
    // The prose: to the grid 2.9 is 3, down to the grid -1.9 is -1 and up to
    // it 1.1 is 2, to the half grid 1.2 is 1.5 and 0 is a half, to the
    // double grid 1.3 is 1.5 and 1.2 is 1 — each symmetric about zero.
    let px = |v: f64| (v * 64.0).round() as i32;
    for (state, n, expected) in [
        (Round::Grid, px(2.9), px(3.0)),
        (Round::Grid, px(-2.9), px(-3.0)),
        (Round::Grid, px(0.5), px(1.0)),
        (Round::Grid, px(-0.5), px(-1.0)),
        (Round::DownToGrid, px(-1.9), px(-1.0)),
        (Round::DownToGrid, px(1.9), px(1.0)),
        (Round::UpToGrid, px(1.1), px(2.0)),
        (Round::UpToGrid, px(-1.1), px(-2.0)),
        (Round::UpToGrid, 0, 0),
        (Round::HalfGrid, px(1.2), px(1.5)),
        (Round::HalfGrid, 0, px(0.5)),
        (Round::HalfGrid, px(-1.2), px(-1.5)),
        (Round::DoubleGrid, px(1.3), px(1.5)),
        (Round::DoubleGrid, px(1.2), px(1.0)),
    ] {
        assert_eq!(state.apply(n), expected, "{state:?} {n}");
    }

    // SROUND[ ] at every byte: the period in bits 7 and 6, the phase in bits
    // 5 and 4, the threshold in bits 3 to 0, each from its table; period 3 is
    // reserved, an error.
    for byte in 0..=255i32 {
        let code = program(&[&push(&[byte]), &[op::SROUND]]);
        let period = match byte >> 6 {
            0 => 32i64,
            1 => 64,
            2 => 128,
            _ => {
                assert_eq!(
                    after(&mut vm, &code),
                    Err(Error::ReservedPeriod(byte)),
                    "SROUND {byte:#04x}"
                );
                continue;
            }
        };
        let phase = match (byte >> 4) & 3 {
            0 => 0,
            1 => period / 4,
            2 => period / 2,
            // gridPeriod · 3/4, the grid one pixel.
            _ => 48,
        };
        let threshold = match byte & 0xF {
            0 => period - 1,
            t => i64::from(t - 4) * period / 8,
        };
        let state = after(&mut vm, &code).expect("runs").round_state;
        assert_eq!(
            state,
            Round::Super {
                period: period as i32,
                phase: phase as i32,
                threshold: threshold as i32,
            },
            "SROUND {byte:#04x}"
        );
        for n in (-400..=400).step_by(7) {
            assert_eq!(
                state.apply(n),
                model_round(n, period, phase, threshold),
                "SROUND {byte:#04x} on {n}"
            );
        }
    }
    // *SROUND*'s example, SROUND(01:01:1000): "maps numbers into the values
    // 0.25, 1.25, 2.25 ... [0.75, 1.75) map into 1.25".
    let example = after(&mut vm, &program(&[&push(&[0b0101_1000]), &[op::SROUND]]))
        .expect("runs")
        .round_state;
    for (n, expected) in [
        (0.0, 0.25),
        (0.74, 0.25),
        (0.75, 1.25),
        (1.74, 1.25),
        (1.75, 2.25),
        (2.5, 2.25),
    ] {
        assert_eq!(example.apply(px(n)), px(expected), "{n}");
    }
    // "Only the lower 8 bits of the argument n are used."
    let high = after(&mut vm, &program(&[&push(&[0x0158]), &[op::SROUND]]))
        .expect("runs")
        .round_state;
    assert_eq!(high, example);

    // S45ROUND[ ]: "The gridPeriod is SQRT(2)/2 pixels rather than 1 pixel":
    // its periods √2/4, √2/2 and √2 pixels to the nearest 64th, each part
    // taken from them as SROUND's are and rounded half away from zero.
    let root = 2f64.sqrt();
    let periods: Vec<i64> = [root / 4.0, root / 2.0, root]
        .iter()
        .map(|p| (p * 64.0).round() as i64)
        .collect();
    assert_eq!(periods, [23, 45, 91]);
    let grid = (root / 2.0 * 64.0).round() as i64;
    for byte in 0..=255i32 {
        let code = program(&[&push(&[byte]), &[op::S45ROUND]]);
        if byte >> 6 == 3 {
            assert_eq!(
                after(&mut vm, &code),
                Err(Error::ReservedPeriod(byte)),
                "S45ROUND {byte:#04x}"
            );
            continue;
        }
        let period = periods[(byte >> 6) as usize];
        let phase = match (byte >> 4) & 3 {
            0 => 0,
            1 => nearest(period, 4),
            2 => nearest(period, 2),
            _ => nearest(3 * grid, 4),
        };
        let threshold = match byte & 0xF {
            0 => period - 1,
            t => nearest(i64::from(t - 4) * period, 8),
        };
        let state = after(&mut vm, &code).expect("runs").round_state;
        assert_eq!(
            state,
            Round::Super {
                period: period as i32,
                phase: phase as i32,
                threshold: threshold as i32,
            },
            "S45ROUND {byte:#04x}"
        );
        for n in (-400..=400).step_by(11) {
            assert_eq!(
                state.apply(n),
                model_round(n, period, phase, threshold),
                "S45ROUND {byte:#04x} on {n}"
            );
        }
    }

    // A state no instruction sets: a period of zero rounds nothing.
    let zero = Round::Super {
        period: 0,
        phase: 5,
        threshold: 5,
    };
    assert_eq!(zero.apply(77), 77);
    // The 32-bit edges round without overflow, wrapping where the result
    // leaves the range.
    assert_eq!(Round::Grid.apply(i32::MIN), i32::MIN);
    assert_eq!(Round::Grid.apply(i32::MAX), i32::MIN);
    assert_eq!(Round::DownToGrid.apply(i32::MAX), i32::MAX & !63);

    for opcode in [op::SROUND, op::S45ROUND, op::ROUND, op::NROUND] {
        assert_eq!(
            vm.run_on(&[], &[opcode], Program::Glyph),
            Err(Error::StackUnderflow),
            "0x{opcode:02X}"
        );
    }
}

#[test]
fn odd_and_even_answer_as_specified() {
    // *ODD*'s example, with RTG: "The value 22.7 is popped from the stack,
    // and the value 1 (true) is pushed"; *EVEN*'s, 0.
    let mut vm = machine();
    let v = (22.7f64 * 64.0).round() as i32;
    assert_eq!(vm.run_on(&[v], &[op::RTG, op::ODD], Program::Glyph), Ok(()));
    assert_eq!(vm.stack(), &[1]);
    assert_eq!(
        vm.run_on(&[v], &[op::RTG, op::EVEN], Program::Glyph),
        Ok(())
    );
    assert_eq!(vm.stack(), &[0]);

    // Rounded by the round state, then "shifted from a fixed-point value to
    // an integer value (any fractional values are ignored)": the whole pixel
    // at or below the rounded value, tested for oddness.
    let states = [
        (op::RTG, 64, 0, 32),
        (op::RTHG, 64, 32, 32),
        (op::RTDG, 32, 0, 16),
        (op::RDTG, 64, 0, 0),
        (op::RUTG, 64, 0, 63),
    ];
    for (opcode, period, phase, threshold) in states {
        for n in (-500..=500).step_by(3) {
            let whole = i64::from(model_round(n, period, phase, threshold)).div_euclid(64);
            let odd = whole.rem_euclid(2) == 1;
            vm.set_graphics_state(GraphicsState::default());
            assert_eq!(vm.run_on(&[n], &[opcode, op::ODD], Program::Glyph), Ok(()));
            assert_eq!(vm.stack(), &[i32::from(odd)], "0x{opcode:02X} ODD {n}");
            assert_eq!(vm.run_on(&[n], &[opcode, op::EVEN], Program::Glyph), Ok(()));
            assert_eq!(vm.stack(), &[i32::from(!odd)], "0x{opcode:02X} EVEN {n}");
        }
    }
    // With rounding off, a fraction is dropped toward the pixel below: -0.5
    // is -1, odd, and 1.5 is 1, odd.
    for (n, odd) in [
        (-32, true),
        (96, true),
        (64, true),
        (63, false),
        (-64, true),
        (-65, false),
        (0, false),
    ] {
        vm.set_graphics_state(GraphicsState::default());
        assert_eq!(
            vm.run_on(&[n], &[op::ROFF, op::ODD], Program::Glyph),
            Ok(())
        );
        assert_eq!(vm.stack(), &[i32::from(odd)], "ROFF ODD {n}");
    }
    assert_eq!(
        vm.run_on(&[], &[op::ODD], Program::Glyph),
        Err(Error::StackUnderflow)
    );
    assert_eq!(
        vm.run_on(&[], &[op::EVEN], Program::Glyph),
        Err(Error::StackUnderflow)
    );
}

#[test]
fn the_distance_and_delta_settings_are_stored_as_specified() {
    // The *Graphics State Summary*'s defaults.
    let defaults = GraphicsState::default();
    assert!(defaults.auto_flip);
    assert_eq!(defaults.control_value_cut_in, 68, "17/16 pixels");
    assert_eq!(defaults.delta_base, 9);
    assert_eq!(defaults.delta_shift, 3);
    assert_eq!(defaults.minimum_distance, 64, "one pixel");
    assert_eq!(defaults.round_state, Round::Grid);
    assert_eq!(defaults.single_width_cut_in, 0);
    assert_eq!(defaults.single_width_value, 0);
    assert_eq!(defaults.instruct_control, 0);
    assert_eq!(defaults.scan_control, 0, "FALSE");

    let mut vm = machine();
    let set = |vm: &mut Vm, value: i32, opcode: u8| {
        after(vm, &program(&[&push(&[value]), &[opcode]])).expect("runs")
    };
    // SMD[ ], SCVTCI[ ], SSWCI[ ]: a 26.6 value, as popped.
    for value in [0, 1, 64, 68, 100, -5, 32767] {
        assert_eq!(set(&mut vm, value, op::SMD).minimum_distance, value);
        assert_eq!(set(&mut vm, value, op::SCVTCI).control_value_cut_in, value);
        assert_eq!(set(&mut vm, value, op::SSWCI).single_width_cut_in, value);
        // SDB[ ], SDS[ ]: the delta base and shift, as popped.
        assert_eq!(set(&mut vm, value, op::SDB).delta_base, value);
        assert_eq!(set(&mut vm, value, op::SDS).delta_shift, value);
    }
    // SSW[ ]: font units, converted to 26.6 at the size.
    for (ppem, upem) in [(12 * 64, 2048u16), (9 * 64 + 32, 1000), (64, 16)] {
        vm.set_size(Size {
            ppem,
            points: 0,
            units_per_em: upem,
            monochrome: false,
        });
        for funits in [0, 1, 100, -100, 2048, 32767] {
            let product = i64::from(funits) * i64::from(ppem);
            let expected = nearest(product, i64::from(upem)) as i32;
            assert_eq!(
                set(&mut vm, funits, op::SSW).single_width_value,
                expected,
                "SSW {funits} at {ppem}/{upem}"
            );
        }
    }
    // FLIPON[ ] and FLIPOFF[ ].
    assert!(!after(&mut vm, &[op::FLIPOFF]).expect("runs").auto_flip);
    assert!(
        after(&mut vm, &[op::FLIPOFF, op::FLIPON])
            .expect("runs")
            .auto_flip
    );
    // SANGW[ ]: "Except for popping a single stack element, it has no
    // effect"; the weight is kept and nothing else moves.
    let state = set(&mut vm, 7, op::SANGW);
    assert_eq!(state.angle_weight, 7);
    assert_eq!(
        GraphicsState {
            angle_weight: 0,
            ..state
        },
        GraphicsState::default()
    );
    // AA[ ], which SANGW once weighed, is the next step's.

    for opcode in [
        op::SMD,
        op::SCVTCI,
        op::SSWCI,
        op::SSW,
        op::SANGW,
        op::SDB,
        op::SDS,
    ] {
        assert_eq!(
            vm.run_on(&[], &[opcode], Program::Glyph),
            Err(Error::StackUnderflow),
            "0x{opcode:02X}"
        );
    }
}

#[test]
fn scanctrl_and_scantype_set_the_dropout_state() {
    // SCANCTRL[ ] keeps a 16-bit word; SCANTYPE[ ] its mode.
    let mut vm = machine();
    for flags in [0, 0x01FF, 0x0A10, 0x3FFF, 0xFFFF] {
        let state = after(
            &mut vm,
            &program(&[
                &[op::NPUSHW, 1],
                &(flags as u16).to_be_bytes(),
                &[op::SCANCTRL],
            ]),
        )
        .expect("runs");
        assert_eq!(state.scan_control, flags as u16);
    }
    vm.set_graphics_state(GraphicsState::default());
    assert_eq!(
        vm.run_on(&[0x0002_0110], &[op::SCANCTRL], Program::Glyph),
        Ok(())
    );
    assert_eq!(vm.graphics_state().scan_control, 0x0110, "the low 16 bits");
    for mode in [0, 1, 2, 3, 4, 5, 6, 7, 8, -1] {
        let state = after(&mut vm, &program(&[&push(&[mode]), &[op::SCANTYPE]])).expect("runs");
        assert_eq!(state.scan_type, mode);
    }

    // SCANTYPE's modes: 0 rules 1, 2 and 3; 1 rules 1, 2 and 4; 4 rules 1, 2
    // and 5; 5 rules 1, 2 and 6; 2, 3, 6 and 7 rules 1 and 2 alone.
    let simple = |stubs| {
        Some(Dropout {
            smart: false,
            stubs,
        })
    };
    let smart = |stubs| Some(Dropout { smart: true, stubs });
    let modes = [
        (0, simple(true)),
        (1, simple(false)),
        (2, None),
        (3, None),
        (4, smart(true)),
        (5, smart(false)),
        (6, None),
        (7, None),
        (8, None),
        (-1, None),
    ];
    let on = GraphicsState {
        scan_control: 0x01FF,
        ..GraphicsState::default()
    };
    for (mode, expected) in modes {
        let state = GraphicsState {
            scan_type: mode,
            ..on
        };
        for ppem in [1, 8, 16, 100, 2000] {
            assert_eq!(state.dropout(ppem), expected, "mode {mode} at {ppem}");
        }
    }

    // SCANCTRL's examples: 0x0000 "No dropout control is invoked"; 0x01FF
    // "Always do dropout control"; 0x0A10 "if the glyph is rotated and has
    // less than 16 pixels per-em", and a hinted glyph is never rotated.
    let with = |flags: u16| GraphicsState {
        scan_control: flags,
        scan_type: 4,
        ..GraphicsState::default()
    };
    let control = smart(true);
    for ppem in [1, 9, 16, 17, 255, 256, 1024] {
        assert_eq!(with(0x0000).dropout(ppem), None, "0x0000 at {ppem}");
        assert_eq!(with(0x01FF).dropout(ppem), control, "0x01FF at {ppem}");
        assert_eq!(with(0x0A10).dropout(ppem), None, "0x0A10 at {ppem}");
        // Bit 8: on where the size is at most the threshold.
        let expected = if ppem <= 16 { control } else { None };
        assert_eq!(with(0x0110).dropout(ppem), expected, "0x0110 at {ppem}");
        // Bit 11 with bit 8: off above the threshold, which bit 8 is anyway.
        assert_eq!(with(0x0910).dropout(ppem), expected, "0x0910 at {ppem}");
        // Bit 11 alone turns nothing on.
        assert_eq!(with(0x0810).dropout(ppem), None, "0x0810 at {ppem}");
        // A threshold of 0 holds at no size.
        assert_eq!(with(0x0100).dropout(ppem), None, "0x0100 at {ppem}");
        // Bits 9 and 10 ask for a rotated or stretched glyph; bits 12 and 13
        // turn dropout control off for one that is neither.
        assert_eq!(with(0x06FF).dropout(ppem), None, "0x06FF at {ppem}");
        assert_eq!(with(0x11FF).dropout(ppem), None, "0x11FF at {ppem}");
        assert_eq!(with(0x21FF).dropout(ppem), None, "0x21FF at {ppem}");
        // Bits 14 and 15 are reserved and change nothing.
        assert_eq!(with(0xC1FF).dropout(ppem), control, "0xC1FF at {ppem}");
    }
    assert_eq!(GraphicsState::default().dropout(12), None, "off by default");

    assert_eq!(
        vm.run_on(&[], &[op::SCANCTRL], Program::Glyph),
        Err(Error::StackUnderflow)
    );
    assert_eq!(
        vm.run_on(&[], &[op::SCANTYPE], Program::Glyph),
        Err(Error::StackUnderflow)
    );
}
