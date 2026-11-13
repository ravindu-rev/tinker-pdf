//! FT-04b.7: `SHP`, `SHC`, `SHZ` and `SHPIX`; `ALIGNRP` and `ALIGNPTS`; `IP`,
//! `ISECT` and `UTP`; `FLIPPT`, `FLIPRGON`, `FLIPRGOFF` and `AA` — with the
//! loop counter and the zones, every expected point computed here.

use super::{
    at, exec, half_away, machine, op, program, proj, push, setup, shifted, touched, touches,
    vectors, wide, D45, SKEW, V, VECTORS, X, Y,
};
use crate::hint::state::{GraphicsState, Point, Zone};
use crate::hint::vm::{Error, Program, Vm};

/// Two contours, points 0 to 3 and 4 to 6, and two phantom points after
/// them. Point 1 (rp1 in the tests) and point 5 (rp2) stand off their
/// originals; the rest stand on theirs.
const CURRENT: [(i32, i32); 9] = [
    (0, 0),
    (100, 37),
    (50, 50),
    (-20, 10),
    (300, 300),
    (330, 280),
    (310, 350),
    (0, 0),
    (500, 0),
];
const ORIGINAL: [(i32, i32); 9] = [
    (0, 0),
    (64, 0),
    (50, 50),
    (-20, 10),
    (300, 300),
    (300, 300),
    (310, 350),
    (0, 0),
    (500, 0),
];

/// A machine over [`CURRENT`] and [`ORIGINAL`], their contours ending at 3
/// and 6.
fn two_contours() -> Vm {
    let mut vm = machine();
    let points = |xy: &[(i32, i32)]| xy.iter().map(|&(x, y)| Point { x, y }).collect::<Vec<_>>();
    let zone = Zone::with_positions(&points(&CURRENT), &points(&ORIGINAL), &[true; 9], &[3, 6])
        .expect("a zone");
    vm.set_glyph(zone);
    vm
}

/// How far point `i` has moved from its original, on `pv`.
fn moved_by(i: usize, pv: V) -> i64 {
    let (c, o) = (wide(CURRENT[i]), wide(ORIGINAL[i]));
    proj(pv, c.0 - o.0, c.1 - o.1)
}

/// The instructions that set rp1 to 1 and rp2 to 5.
fn references() -> Vec<u8> {
    program(&[&push(&[1]), &[op::SRP1], &push(&[5]), &[op::SRP2]])
}

#[test]
fn shp_shc_shz_and_shpix_shift_as_specified() {
    // SHP[a]: each point the loop count pops shifts "by the same amount that
    // the reference point has been shifted", from its current position; a =
    // 0 reads rp2 in zp1's zone, a = 1 rp1 in zp0's.
    for (f, pv) in VECTORS {
        for flag in [0u8, 1] {
            let shift = moved_by(if flag == 1 { 1 } else { 5 }, pv);
            for points in [vec![2], vec![3, 6], vec![2, 2, 8], vec![0, 4, 7, 3]] {
                let mut vm = two_contours();
                let pushed: Vec<i32> = points.iter().map(|&p| p as i32).collect();
                let code = program(&[
                    &vectors(f, pv),
                    &references(),
                    &push(&[points.len() as i32]),
                    &[op::SLOOP],
                    &push(&pushed),
                    &[op::SHP + flag],
                ]);
                assert_eq!(exec(&mut vm, &code), Ok(()));
                let case = format!("SHP[{flag}] {points:?} along {f:?} on {pv:?}");
                for i in 0..9 {
                    let times = points.iter().filter(|&&p| p == i).count();
                    let mut expected = wide(CURRENT[i]);
                    for _ in 0..times {
                        expected = shifted(expected, f, pv, shift);
                    }
                    assert_eq!(at(&vm, i), expected, "{case}: point {i}");
                    let touch = if times > 0 {
                        touches(f)
                    } else {
                        (false, false)
                    };
                    assert_eq!(touched(&vm, i), touch, "{case}: point {i}");
                }
                assert_eq!(vm.graphics_state().loop_count, 1, "{case}: the loop resets");
            }
        }
    }
    // The reference is read from the zone its pointer names: with zp0 at the
    // twilight zone, rp1 there has not moved, so SHP[1] shifts by nothing,
    // but still touches.
    let mut vm = two_contours();
    let code = program(&[
        &references(),
        &push(&[0]),
        &[op::SZP0],
        &push(&[2]),
        &[op::SHP + 1],
    ]);
    assert_eq!(exec(&mut vm, &code), Ok(()));
    assert_eq!(at(&vm, 2), wide(CURRENT[2]));
    assert_eq!(touched(&vm, 2), (true, false));
    // And the points from zp2's: twilight point 2 shifts, the glyph's stays.
    let mut vm = two_contours();
    let code = program(&[
        &[op::SVTCA + 1],
        &references(),
        &push(&[0]),
        &[op::SZP2],
        &push(&[2]),
        &[op::SHP],
    ]);
    assert_eq!(exec(&mut vm, &code), Ok(()));
    assert_eq!(at(&vm, 2), wide(CURRENT[2]));
    assert_eq!(
        vm.zone(0).expect("twilight").current()[2],
        Point { x: 30, y: 0 }
    );

    // SHC[a]: every point of contour c in zp2's zone shifts, the reference
    // point not where it is one of them.
    for (f, pv) in VECTORS {
        for flag in [0u8, 1] {
            let reference = if flag == 1 { 1 } else { 5 };
            let shift = moved_by(reference, pv);
            for (contour, range) in [(0, 0..=3usize), (1, 4..=6)] {
                let mut vm = two_contours();
                let code = program(&[
                    &vectors(f, pv),
                    &references(),
                    &push(&[contour]),
                    &[op::SHC + flag],
                ]);
                assert_eq!(exec(&mut vm, &code), Ok(()));
                let case = format!("SHC[{flag}] {contour} along {f:?} on {pv:?}");
                for i in 0..9 {
                    let moves = range.contains(&i) && i != reference;
                    let expected = if moves {
                        shifted(wide(CURRENT[i]), f, pv, shift)
                    } else {
                        wide(CURRENT[i])
                    };
                    assert_eq!(at(&vm, i), expected, "{case}: point {i}");
                    let touch = if moves { touches(f) } else { (false, false) };
                    assert_eq!(touched(&vm, i), touch, "{case}: point {i}");
                }
            }
        }
    }
    // SHZ[a]: every point of zone e, the phantom points included and the
    // reference point excepted.
    for (f, pv) in VECTORS {
        for flag in [0u8, 1] {
            let reference = if flag == 1 { 1 } else { 5 };
            let shift = moved_by(reference, pv);
            let mut vm = two_contours();
            let code = program(&[
                &vectors(f, pv),
                &references(),
                &push(&[1]),
                &[op::SHZ + flag],
            ]);
            assert_eq!(exec(&mut vm, &code), Ok(()));
            for i in 0..9 {
                let expected = if i == reference {
                    wide(CURRENT[i])
                } else {
                    shifted(wide(CURRENT[i]), f, pv, shift)
                };
                assert_eq!(
                    at(&vm, i),
                    expected,
                    "SHZ[{flag}] 1 along {f:?} on {pv:?}: point {i}"
                );
            }
            // Zone 0: all sixteen twilight points, from the origin.
            let mut vm = two_contours();
            let code = program(&[
                &vectors(f, pv),
                &references(),
                &push(&[0]),
                &[op::SHZ + flag],
            ]);
            assert_eq!(exec(&mut vm, &code), Ok(()));
            let expected = shifted((0, 0), f, pv, shift);
            let twilight = vm.zone(0).expect("twilight");
            for (i, point) in twilight.current().iter().enumerate() {
                assert_eq!(
                    (i64::from(point.x), i64::from(point.y)),
                    expected,
                    "SHZ[{flag}] 0: twilight {i}"
                );
            }
            for i in 0..9 {
                assert_eq!(
                    at(&vm, i),
                    wide(CURRENT[i]),
                    "SHZ[{flag}] 0 leaves the glyph"
                );
            }
        }
    }

    // SHPIX[ ]: pops an amount, then the loop count's points, and shifts
    // each by the amount along the freedom vector alone, "the projection
    // vector is ignored" — even one orthogonal to it. *SHPIX*'s example
    // shifts three points "by 80/64 or 1.25 pixels".
    for (f, pv) in [(X, X), (Y, X), (D45, X), (SKEW, Y), (X, Y)] {
        for amount in [80, -64, 0, 1000] {
            let mut vm = two_contours();
            let code = program(&[
                &vectors(f, pv),
                &push(&[3]),
                &[op::SLOOP],
                &push(&[2, 3, 4, amount]),
                &[op::SHPIX],
            ]);
            assert_eq!(exec(&mut vm, &code), Ok(()), "SHPIX {amount} along {f:?}");
            let step = (
                half_away(i128::from(amount) * i128::from(f.0), 16_384),
                half_away(i128::from(amount) * i128::from(f.1), 16_384),
            );
            for i in 0..9 {
                let c = wide(CURRENT[i]);
                let expected = if (2..=4).contains(&i) {
                    (c.0 + step.0, c.1 + step.1)
                } else {
                    c
                };
                assert_eq!(
                    at(&vm, i),
                    expected,
                    "SHPIX {amount} along {f:?}: point {i}"
                );
                let touch = if (2..=4).contains(&i) {
                    touches(f)
                } else {
                    (false, false)
                };
                assert_eq!(touched(&vm, i), touch);
            }
            assert_eq!(vm.graphics_state().loop_count, 1);
        }
    }

    // Steps: a loop-counted shift spends one a point, SHC and SHZ one a
    // point they move.
    let mut vm = two_contours();
    let code = program(&[
        &references(),
        &push(&[3]),
        &[op::SLOOP],
        &push(&[0, 2, 3]),
        &[op::SHP],
    ]);
    assert_eq!(exec(&mut vm, &code), Ok(()));
    assert_eq!(
        vm.steps(),
        4 + 2 + 2 + 3,
        "four reference instructions, SLOOP and its push, the push and SHP, three points"
    );
    let mut vm = two_contours();
    let code = program(&[&references(), &push(&[1]), &[op::SHZ + 1]]);
    assert_eq!(exec(&mut vm, &code), Ok(()));
    assert_eq!(vm.steps(), 4 + 2 + 8, "eight of the nine points move");

    // Errors: a contour or a zone the glyph lacks, a point past its zone, a
    // reference past its zone, an orthogonal pair, too few points.
    let mut vm = two_contours();
    for (code, error) in [
        (
            program(&[&references(), &push(&[2]), &[op::SHC]]),
            Error::ContourIndex(2),
        ),
        (
            program(&[&references(), &push(&[-1]), &[op::SHC + 1]]),
            Error::ContourIndex(-1),
        ),
        (
            program(&[
                &references(),
                &push(&[0]),
                &[op::SZP2],
                &push(&[0]),
                &[op::SHC],
            ]),
            Error::ContourIndex(0),
        ),
        (
            program(&[&references(), &push(&[2]), &[op::SHZ]]),
            Error::ZoneIndex(2),
        ),
        (
            program(&[&references(), &push(&[9]), &[op::SHP]]),
            Error::PointIndex(9),
        ),
        (
            program(&[&push(&[20]), &[op::SRP2], &push(&[2]), &[op::SHP]]),
            Error::PointIndex(20),
        ),
        (
            program(&[&references(), &vectors(Y, X), &push(&[2]), &[op::SHP]]),
            Error::OrthogonalVectors,
        ),
        (
            program(&[&push(&[2]), &[op::SLOOP], &push(&[2]), &[op::SHP]]),
            Error::StackUnderflow,
        ),
        (
            program(&[&push(&[9, 64]), &[op::SHPIX]]),
            Error::PointIndex(9),
        ),
        (vec![op::SHPIX], Error::StackUnderflow),
    ] {
        assert_eq!(exec(&mut vm, &code), Err(error), "{code:02X?}");
    }
}

#[test]
fn alignrp_and_alignpts_align_as_specified() {
    // ALIGNRP[ ]: "Reduces the distance between rp0 and point p to zero",
    // each of the loop count's points, in zp1's zone, rp0 in zp0's.
    for (f, pv) in VECTORS {
        for points in [vec![2usize], vec![3, 6, 8]] {
            let mut vm = two_contours();
            let pushed: Vec<i32> = points.iter().map(|&p| p as i32).collect();
            let code = program(&[
                &vectors(f, pv),
                &push(&[1]),
                &[op::SRP0],
                &push(&[points.len() as i32]),
                &[op::SLOOP],
                &push(&pushed),
                &[op::ALIGNRP],
            ]);
            assert_eq!(exec(&mut vm, &code), Ok(()));
            let reference = wide(CURRENT[1]);
            for i in 0..9 {
                let c = wide(CURRENT[i]);
                let expected = if points.contains(&i) {
                    shifted(c, f, pv, -proj(pv, c.0 - reference.0, c.1 - reference.1))
                } else {
                    c
                };
                assert_eq!(
                    at(&vm, i),
                    expected,
                    "ALIGNRP {points:?} along {f:?} on {pv:?}: point {i}"
                );
            }
            assert_eq!(vm.graphics_state().loop_count, 1);
        }
    }
    // Aligned on the x axis, the points take rp0's x and keep their y.
    let mut vm = two_contours();
    let code = program(&[
        &[op::SVTCA + 1],
        &push(&[4]),
        &[op::SRP0],
        &push(&[2]),
        &[op::SLOOP],
        &push(&[2, 6]),
        &[op::ALIGNRP],
    ]);
    assert_eq!(exec(&mut vm, &code), Ok(()));
    assert_eq!((at(&vm, 2), at(&vm, 6)), ((300, 50), (300, 350)));

    // ALIGNPTS[ ]: pops p1, in zp1's zone, then p2, in zp0's, and moves both
    // "along the freedom_vector to the average of both their projections
    // along the projection_vector". *ALIGNPTS*'s example names points 3 and
    // 7.
    for (f, pv) in VECTORS {
        for (p1, p2) in [(3usize, 7usize), (2, 5), (8, 0), (4, 4)] {
            let mut vm = two_contours();
            let code = program(&[
                &vectors(f, pv),
                &push(&[p2 as i32, p1 as i32]),
                &[op::ALIGNPTS],
            ]);
            assert_eq!(exec(&mut vm, &code), Ok(()));
            let (c1, c2) = (wide(CURRENT[p1]), wide(CURRENT[p2]));
            let (on1, on2) = (proj(pv, c1.0, c1.1), proj(pv, c2.0, c2.1));
            let average = half_away(i128::from(on1 + on2), 2);
            let mut expected = [wide(CURRENT[p1]), wide(CURRENT[p2])];
            expected[0] = shifted(expected[0], f, pv, average - on1);
            if p1 == p2 {
                expected[1] = shifted(
                    expected[0],
                    f,
                    pv,
                    average - proj(pv, expected[0].0, expected[0].1),
                );
                assert_eq!(at(&vm, p1), expected[1], "ALIGNPTS {p1} with itself");
            } else {
                expected[1] = shifted(expected[1], f, pv, average - on2);
                assert_eq!(
                    at(&vm, p1),
                    expected[0],
                    "ALIGNPTS {p1} {p2} along {f:?} on {pv:?}"
                );
                assert_eq!(
                    at(&vm, p2),
                    expected[1],
                    "ALIGNPTS {p1} {p2} along {f:?} on {pv:?}"
                );
            }
            assert_eq!(touched(&vm, p1), touches(f));
            assert_eq!(touched(&vm, p2), touches(f));
        }
    }
    // On the x axis the two meet at the average x.
    let mut vm = two_contours();
    let code = program(&[&[op::SVTCA + 1], &push(&[7, 3]), &[op::ALIGNPTS]]);
    assert_eq!(exec(&mut vm, &code), Ok(()));
    assert_eq!((at(&vm, 3), at(&vm, 7)), ((-10, 10), (-10, 0)));
    // p2 is read from zp0's zone: a twilight point at the origin.
    let mut vm = two_contours();
    let code = program(&[
        &[op::SVTCA + 1],
        &push(&[0]),
        &[op::SZP0],
        &push(&[3, 4]),
        &[op::ALIGNPTS],
    ]);
    assert_eq!(exec(&mut vm, &code), Ok(()));
    assert_eq!(at(&vm, 4), (150, 300));
    assert_eq!(
        vm.zone(0).expect("twilight").current()[3],
        Point { x: 150, y: 0 }
    );

    let mut vm = two_contours();
    for (code, error) in [
        (
            program(&[&push(&[20]), &[op::SRP0], &push(&[2]), &[op::ALIGNRP]]),
            Error::PointIndex(20),
        ),
        (
            program(&[&push(&[9]), &[op::ALIGNRP]]),
            Error::PointIndex(9),
        ),
        (
            program(&[&push(&[9, 1]), &[op::ALIGNPTS]]),
            Error::PointIndex(9),
        ),
        (
            program(&[&push(&[1, 9]), &[op::ALIGNPTS]]),
            Error::PointIndex(9),
        ),
        (
            program(&[&vectors(Y, X), &push(&[1, 2]), &[op::ALIGNPTS]]),
            Error::OrthogonalVectors,
        ),
        (vec![op::ALIGNRP], Error::StackUnderflow),
        (
            program(&[&push(&[1]), &[op::ALIGNPTS]]),
            Error::StackUnderflow,
        ),
    ] {
        assert_eq!(exec(&mut vm, &code), Err(error), "{code:02X?}");
    }
}

#[test]
fn ip_isect_and_utp_as_specified() {
    // IP[ ]: each of the loop count's points, in zp2's zone, moves so its
    // distance from rp1 (zp0's) is its original one scaled by how rp1 and rp2
    // (zp1's) now stand against how they stood — "D(p, rp1)/D(p', rp1') =
    // D(p,rp2)/D(p', rp2')" — the original distances on the dual vector.
    let current = [(0, 0), (700, 100), (20, 30), (300, 900), (-50, 64), (64, 0)];
    let original = [(0, 0), (640, 0), (20, 30), (300, 900), (-50, 64), (64, 0)];
    for (f, pv) in VECTORS {
        for points in [vec![2usize], vec![3, 4, 5]] {
            let mut vm = setup(&current, &original, &[]);
            let pushed: Vec<i32> = points.iter().map(|&p| p as i32).collect();
            let code = program(&[
                &vectors(f, pv),
                &push(&[0]),
                &[op::SRP1],
                &push(&[1]),
                &[op::SRP2],
                &push(&[points.len() as i32]),
                &[op::SLOOP],
                &push(&pushed),
                &[op::IP],
            ]);
            let (o1, o2) = (wide(original[0]), wide(original[1]));
            let span = proj(pv, o2.0 - o1.0, o2.1 - o1.1);
            if span == 0 {
                assert_eq!(
                    exec(&mut vm, &code),
                    Err(Error::CoincidentReferences),
                    "IP along {f:?} on {pv:?}"
                );
                continue;
            }
            assert_eq!(exec(&mut vm, &code), Ok(()));
            let (c1, c2) = (wide(current[0]), wide(current[1]));
            let now_span = proj(pv, c2.0 - c1.0, c2.1 - c1.1);
            for i in 0..6 {
                let c = wide(current[i]);
                let expected = if points.contains(&i) {
                    let o = wide(original[i]);
                    let from_rp1 = proj(pv, o.0 - o1.0, o.1 - o1.1);
                    let wanted = half_away(
                        i128::from(from_rp1) * i128::from(now_span),
                        i128::from(span),
                    );
                    shifted(c, f, pv, wanted - proj(pv, c.0 - c1.0, c.1 - c1.1))
                } else {
                    c
                };
                assert_eq!(
                    at(&vm, i),
                    expected,
                    "IP {points:?} along {f:?} on {pv:?}: point {i}"
                );
            }
            assert_eq!(vm.graphics_state().loop_count, 1);
        }
    }
    // On the x axis: rp2 moved from 640 to 700, so a point a quarter of the
    // way in the original, at 160, goes a quarter of the way, to 175.
    let points = [(0, 0), (700, 0), (0, 5)];
    let originals = [(0, 0), (640, 0), (160, 5)];
    let mut vm = setup(&points, &originals, &[]);
    let code = program(&[
        &[op::SVTCA + 1],
        &push(&[0]),
        &[op::SRP1],
        &push(&[1]),
        &[op::SRP2],
        &push(&[2]),
        &[op::IP],
    ]);
    assert_eq!(exec(&mut vm, &code), Ok(()));
    assert_eq!(at(&vm, 2), (175, 5));
    // The original distances are on the dual vector: SDPVTL sets it from the
    // original outline, the projection vector from the current one.
    let mut vm = setup(&current, &original, &[]);
    let code = program(&[
        &push(&[1, 0]),
        &[op::SDPVTL, op::SFVTPV],
        &push(&[0]),
        &[op::SRP1],
        &push(&[1]),
        &[op::SRP2],
        &push(&[3]),
        &[op::IP],
    ]);
    assert_eq!(exec(&mut vm, &code), Ok(()));
    let gs = *vm.graphics_state();
    let pv: V = (
        i64::from(gs.projection_vector.x),
        i64::from(gs.projection_vector.y),
    );
    let dual: V = (
        i64::from(gs.dual_projection_vector.x),
        i64::from(gs.dual_projection_vector.y),
    );
    let (o1, o2, o) = (wide(original[0]), wide(original[1]), wide(original[3]));
    let (c1, c2, c) = (wide(current[0]), wide(current[1]), wide(current[3]));
    let wanted = half_away(
        i128::from(proj(dual, o.0 - o1.0, o.1 - o1.1))
            * i128::from(proj(pv, c2.0 - c1.0, c2.1 - c1.1)),
        i128::from(proj(dual, o2.0 - o1.0, o2.1 - o1.1)),
    );
    assert_eq!(
        at(&vm, 3),
        shifted(c, pv, pv, wanted - proj(pv, c.0 - c1.0, c.1 - c1.1))
    );
    // "This instruction is not valid if rp1 and rp2 have the same position on
    // the projection_vector": here the same original x.
    let mut vm = setup(&[(0, 0), (64, 50), (5, 5)], &[(0, 0), (0, 64), (5, 5)], &[]);
    let code = program(&[
        &[op::SVTCA + 1],
        &push(&[0]),
        &[op::SRP1],
        &push(&[1]),
        &[op::SRP2],
        &push(&[2]),
        &[op::IP],
    ]);
    assert_eq!(exec(&mut vm, &code), Err(Error::CoincidentReferences));
    let code = program(&[
        &[op::SVTCA],
        &push(&[0]),
        &[op::SRP1],
        &push(&[1]),
        &[op::SRP2],
        &push(&[2]),
        &[op::IP],
    ]);
    assert_eq!(exec(&mut vm, &code), Ok(()), "on the y axis they differ");

    // ISECT[ ]: pops b1, b0, a1, a0, then p, and "Puts point p at the
    // intersection of the lines A and B" — A from a0 to a1 in zp1's zone, B
    // in zp0's — ignoring the freedom vector; parallel lines put it "in the
    // middle": px = (a0x + a1x)/4 + (b0x + b1x)/4.
    let corners = [
        (0, 0),
        (640, 640),
        (0, 640),
        (640, 0),
        (0, 100),
        (500, 100),
        (37, 0),
        (37, 999),
        (0, 64),
        (100, 64),
        (100, 0),
        (9, 9),
    ];
    let lines = [
        ((0usize, 1usize), (2usize, 3usize), (320, 320)),
        ((4, 5), (6, 7), (37, 100)),
        ((0, 10), (8, 9), (50, 32)),
        ((0, 1), (0, 1), (320, 320)),
    ];
    for ((a0, a1), (b0, b1), expected) in lines {
        let mut vm = setup(&corners, &corners, &[]);
        let code = program(&[
            &push(&[11, a0 as i32, a1 as i32, b0 as i32, b1 as i32]),
            &[op::ISECT],
        ]);
        assert_eq!(exec(&mut vm, &code), Ok(()));
        assert_eq!(at(&vm, 11), expected, "ISECT of {a0}-{a1} and {b0}-{b1}");
        assert_eq!(touched(&vm, 11), (true, true), "touched on both axes");
    }
    // Lines in general position, against the intersection solved here.
    let mut lcg = super::Lcg(0x15EC7);
    for _ in 0..300 {
        let mut coordinate = || (lcg.below(4001) as i64 - 2000) as i32;
        let quad: Vec<(i32, i32)> = (0..4).map(|_| (coordinate(), coordinate())).collect();
        let mut all = quad.clone();
        all.push((7, 7));
        let mut vm = setup(&all, &all, &[]);
        let code = program(&[&push(&[4, 0, 1, 2, 3]), &[op::ISECT]]);
        assert_eq!(exec(&mut vm, &code), Ok(()));
        let w = |i: usize| (i128::from(quad[i].0), i128::from(quad[i].1));
        let (a0, a1, b0, b1) = (w(0), w(1), w(2), w(3));
        let (ax, ay, bx, by) = (a1.0 - a0.0, a1.1 - a0.1, b1.0 - b0.0, b1.1 - b0.1);
        let denominator = ax * by - ay * bx;
        let (x, y) = if denominator == 0 {
            (
                half_away(a0.0 + a1.0 + b0.0 + b1.0, 4),
                half_away(a0.1 + a1.1 + b0.1 + b1.1, 4),
            )
        } else {
            let t = (b0.0 - a0.0) * by - (b0.1 - a0.1) * bx;
            (
                a0.0 as i64 + half_away(ax * t, denominator),
                a0.1 as i64 + half_away(ay * t, denominator),
            )
        };
        // Lines near parallel meet far off, past 32 bits, which wrap.
        let wrap = |v: i64| i64::from(v as i32);
        assert_eq!(at(&vm, 4), (wrap(x), wrap(y)), "ISECT of {quad:?}");
    }
    // The zones: A from zp1's, B from zp0's, p in zp2's.
    let mut vm = setup(&corners, &corners, &[]);
    let code = program(&[
        &push(&[0]),
        &[op::SZP2],
        &push(&[5, 0, 1, 2, 3]),
        &[op::ISECT],
    ]);
    assert_eq!(exec(&mut vm, &code), Ok(()));
    assert_eq!(
        vm.zone(0).expect("twilight").current()[5],
        Point { x: 320, y: 320 }
    );
    assert_eq!(at(&vm, 5), (500, 100), "the glyph's point 5 stays");
    let mut vm = setup(&corners, &corners, &[]);
    let code = program(&[
        &push(&[0]),
        &[op::SZP0],
        &push(&[11, 4, 5, 6, 7]),
        &[op::ISECT],
    ]);
    assert_eq!(exec(&mut vm, &code), Ok(()));
    // B is the twilight zone's points 6 and 7, both at the origin: no
    // direction, so the middle of the four points, (0 + 500 + 0 + 0) / 4 and
    // (100 + 100 + 0 + 0) / 4.
    assert_eq!(at(&vm, 11), (125, 50));

    // UTP[ ]: untouched in x for a freedom vector on the x axis, in y for one
    // on the y axis, in both otherwise. MDAP[0] along 45 degrees touches both
    // first.
    for (f, untouched) in [
        (X, (false, true)),
        ((-16_384, 0), (false, true)),
        (Y, (true, false)),
        (D45, (false, false)),
        (SKEW, (false, false)),
    ] {
        let mut vm = two_contours();
        let code = program(&[
            &vectors(D45, D45),
            &push(&[2]),
            &[op::MDAP],
            &vectors(f, X),
            &push(&[2]),
            &[op::UTP],
        ]);
        // vectors() resets nothing else: the freedom vector is f for UTP.
        vm.set_graphics_state(GraphicsState::default());
        assert_eq!(
            vm.run(&code, Program::Glyph, None),
            Ok(()),
            "UTP along {f:?}"
        );
        assert_eq!(touched(&vm, 2), untouched, "UTP along {f:?}");
    }
    // UTP reads zp0's zone.
    let mut vm = two_contours();
    assert_eq!(
        exec(
            &mut vm,
            &program(&[&push(&[0]), &[op::SZP0], &push(&[2]), &[op::MDAP]])
        ),
        Ok(())
    );
    assert_eq!(vm.zone(0).expect("twilight").touched(2), (true, false));
    assert_eq!(
        exec(
            &mut vm,
            &program(&[&push(&[0]), &[op::SZP0], &push(&[2]), &[op::UTP]])
        ),
        Ok(())
    );
    assert_eq!(vm.zone(0).expect("twilight").touched(2), (false, false));
    assert_eq!(
        touched(&vm, 2),
        (false, false),
        "the glyph's point 2 was never touched"
    );

    let mut vm = two_contours();
    for (code, error) in [
        (
            program(&[&push(&[9]), &[op::SRP1], &push(&[2]), &[op::IP]]),
            Error::PointIndex(9),
        ),
        (
            program(&[&push(&[1]), &[op::SRP2], &push(&[9]), &[op::IP]]),
            Error::PointIndex(9),
        ),
        (
            program(&[&push(&[9, 0, 1, 2, 3]), &[op::ISECT]]),
            Error::PointIndex(9),
        ),
        (
            program(&[&push(&[0, 0, 1, 2, 9]), &[op::ISECT]]),
            Error::PointIndex(9),
        ),
        (
            program(&[&push(&[0, 1, 2, 3]), &[op::ISECT]]),
            Error::StackUnderflow,
        ),
        (program(&[&push(&[9]), &[op::UTP]]), Error::PointIndex(9)),
        (vec![op::UTP], Error::StackUnderflow),
    ] {
        assert_eq!(exec(&mut vm, &code), Err(error), "{code:02X?}");
    }
}

/// A machine whose glyph is ten points along x, on the curve where `on`
/// says, one contour.
fn flagged(on: [bool; 10]) -> Vm {
    let mut vm = machine();
    let points: Vec<Point> = (0..10).map(|i| Point { x: i * 64, y: 0 }).collect();
    vm.set_glyph(Zone::glyph(&points, &on, &[9]).expect("a zone"));
    vm
}

/// The glyph's on-curve flags.
fn flags(vm: &Vm) -> [bool; 10] {
    let zone = vm.zone(1).expect("a glyph");
    std::array::from_fn(|i| zone.on_curve(i))
}

#[test]
fn the_flip_instructions_toggle_on_curve_as_specified() {
    let alternate = [
        true, false, true, false, true, false, true, false, true, false,
    ];
    // FLIPPT[ ]: each of the loop count's points, in zp0's zone, flips; "The
    // point is not marked as touched", and nothing moves.
    for points in [vec![0usize], vec![1, 2, 3], vec![4, 4], vec![9, 0, 5, 6]] {
        let mut vm = flagged(alternate);
        let pushed: Vec<i32> = points.iter().map(|&p| p as i32).collect();
        let code = program(&[
            &push(&[points.len() as i32]),
            &[op::SLOOP],
            &push(&pushed),
            &[op::FLIPPT],
        ]);
        assert_eq!(exec(&mut vm, &code), Ok(()));
        let mut expected = alternate;
        for &p in &points {
            expected[p] = !expected[p];
        }
        assert_eq!(flags(&vm), expected, "FLIPPT {points:?}");
        for i in 0..10 {
            assert_eq!(touched(&vm, i), (false, false));
            assert_eq!(at(&vm, i), (i as i64 * 64, 0));
        }
        assert_eq!(vm.graphics_state().loop_count, 1);
    }

    // FLIPRGON[ ]: pops the highest point, then the lowest, and puts every
    // point from the lowest to the highest on the curve. Its example: "points
    // 1, 3 and 5 off the curve (before); all points 1 to 5 are on-curve
    // (after)".
    let mut before = [true; 10];
    for p in [1, 3, 5] {
        before[p] = false;
    }
    let mut vm = flagged(before);
    assert_eq!(
        exec(&mut vm, &program(&[&push(&[1, 5]), &[op::FLIPRGON]])),
        Ok(())
    );
    assert_eq!(flags(&vm), [true; 10]);
    // FLIPRGOFF[ ]'s: points 5 to 9 on the curve before; "points 5 and 9 are
    // on-curve and points 6 to 8 are off-curve" after a flip of 6 to 8.
    let mut vm = flagged([true; 10]);
    assert_eq!(
        exec(&mut vm, &program(&[&push(&[6, 8]), &[op::FLIPRGOFF]])),
        Ok(())
    );
    let mut expected = [true; 10];
    for p in 6..=8 {
        expected[p] = false;
    }
    assert_eq!(flags(&vm), expected);
    // Every range over both kinds of point: on puts all of it on, off all of
    // it off, and nothing outside it changes; a range of one point; a highest
    // point below the lowest names none.
    for (low, high) in [(0, 9), (2, 2), (3, 7), (8, 9), (6, 4)] {
        for (opcode, set) in [(op::FLIPRGON, true), (op::FLIPRGOFF, false)] {
            let mut vm = flagged(alternate);
            assert_eq!(
                exec(
                    &mut vm,
                    &program(&[&push(&[low as i32, high as i32]), &[opcode]])
                ),
                Ok(())
            );
            let mut expected = alternate;
            for p in low..=high {
                expected[p] = set;
            }
            assert_eq!(flags(&vm), expected, "0x{opcode:02X} {low} to {high}");
            assert!((0..10).all(|i| touched(&vm, i) == (false, false)));
            assert_eq!(
                vm.steps() as usize,
                2 + (low..=high).count(),
                "a step a point"
            );
        }
    }
    // The flips act on zp0's zone.
    let mut vm = flagged(alternate);
    assert_eq!(
        exec(
            &mut vm,
            &program(&[
                &push(&[0]),
                &[op::SZP0],
                &push(&[3, 5]),
                &[op::FLIPRGON],
                &push(&[1]),
                &[op::FLIPPT]
            ])
        ),
        Ok(())
    );
    assert_eq!(flags(&vm), alternate, "the glyph is untouched");
    let twilight = vm.zone(0).expect("twilight");
    assert_eq!(
        (0..7).map(|i| twilight.on_curve(i)).collect::<Vec<_>>(),
        [false, true, false, true, true, true, false]
    );

    // AA[ ]: "Pops one argument from the stack ... and has no other effect."
    let mut vm = flagged(alternate);
    assert_eq!(
        exec(&mut vm, &program(&[&push(&[7, 3]), &[op::AA]])),
        Ok(())
    );
    assert_eq!(vm.stack(), &[7]);
    assert_eq!(flags(&vm), alternate);
    assert_eq!(*vm.graphics_state(), GraphicsState::default());

    let mut vm = flagged(alternate);
    for (code, error) in [
        (
            program(&[&push(&[10]), &[op::FLIPPT]]),
            Error::PointIndex(10),
        ),
        (
            program(&[&push(&[0, 10]), &[op::FLIPRGON]]),
            Error::PointIndex(10),
        ),
        (
            program(&[&push(&[-1, 3]), &[op::FLIPRGOFF]]),
            Error::PointIndex(-1),
        ),
        (
            program(&[&push(&[2]), &[op::SLOOP], &push(&[1]), &[op::FLIPPT]]),
            Error::StackUnderflow,
        ),
        (
            program(&[&push(&[3]), &[op::FLIPRGON]]),
            Error::StackUnderflow,
        ),
        (vec![op::AA], Error::StackUnderflow),
    ] {
        assert_eq!(exec(&mut vm, &code), Err(error), "{code:02X?}");
    }
}
