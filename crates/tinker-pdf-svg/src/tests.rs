//! Geometry, held to numbers this file computes rather than to numbers the
//! crate produced.
//!
//! Every expected value here is either a spec identity (a quadratic raised to a
//! cubic, a half-turn's endpoint) or arithmetic written out beside the
//! assertion. Nothing is a recorded output: a golden that came from the code it
//! checks proves the code has not changed, not that it is right.

use crate::path::{self, PathError, Segment};
use crate::transform;

/// Within a tenth of a millionth of a user unit, which is far below anything a
/// rasterizer resolves and far above `f64`'s noise over these magnitudes.
fn near(left: f64, right: f64, what: &str) {
    assert!((left - right).abs() < 1e-7, "{what}: {left} is not {right}");
}

fn point(left: [f64; 2], right: [f64; 2], what: &str) {
    near(left[0], right[0], what);
    near(left[1], right[1], what);
}

// ---- the number grammar ---------------------------------------------------

/// **A sign begins a new number where a separator would.** `1-2` is two
/// numbers, and a parser that split on whitespace alone reads it as one.
#[test]
fn a_sign_separates_two_numbers_without_any_whitespace() {
    assert_eq!(transform::numbers("1-2").unwrap(), vec![1.0, -2.0]);
    assert_eq!(transform::numbers("1.5.5").unwrap(), vec![1.5, 0.5]);
    assert_eq!(transform::numbers("-.5e-2 +3").unwrap(), vec![-0.005, 3.0]);
}

/// `1e` is the number one followed by rubbish, not a malformed number: the
/// exponent is part of the token only when it has digits.
#[test]
fn an_exponent_with_no_digits_is_not_part_of_the_number() {
    assert_eq!(transform::numbers("1").unwrap(), vec![1.0]);
    assert!(
        transform::numbers("1e").is_none(),
        "trailing `e` is rubbish"
    );
    assert_eq!(transform::numbers("1e3").unwrap(), vec![1000.0]);
}

// ---- transforms -----------------------------------------------------------

/// §7.6 applies a list left to right with the **leftmost outermost**, so
/// `translate(10,0) scale(2)` scales a point and then translates it.
///
/// The two orders are told apart by a point that is not the origin: at the
/// origin both give `(10, 0)` and the test would pass either way.
#[test]
fn a_transform_list_applies_leftmost_outermost() {
    let matrix = transform::list("translate(10,0) scale(2)").unwrap();
    point(
        transform::apply(matrix, [3.0, 5.0]),
        [16.0, 10.0],
        "scale first, then translate",
    );
    // The other order would be (3 + 10) * 2 = 26 in x.
    assert!(
        (transform::apply(matrix, [3.0, 5.0])[0] - 26.0).abs() > 1.0,
        "and not the other way round"
    );
}

/// A rotation about a point leaves that point exactly where it was — which is
/// the whole definition, and the one assertion the three-argument form can be
/// got wrong without failing.
#[test]
fn a_rotation_about_a_point_fixes_that_point() {
    for degrees in [0.0, 30.0, 90.0, 180.0, -45.0] {
        let matrix = transform::list(&format!("rotate({degrees}, 7, -3)")).unwrap();
        point(
            transform::apply(matrix, [7.0, -3.0]),
            [7.0, -3.0],
            "the centre of the rotation",
        );
    }
}

/// A transform that is not the grammar is `None`, so a caller can name it —
/// not an identity, which would move the shape somewhere the file did not.
#[test]
fn an_unreadable_transform_is_refused_rather_than_ignored() {
    assert!(transform::list("translate(").is_none());
    assert!(transform::list("wobble(3)").is_none());
    assert!(transform::list("scale(1,2,3)").is_none(), "wrong arity");
    assert_eq!(transform::list("   ").unwrap(), transform::IDENTITY);
}

// ---- the viewBox mapping --------------------------------------------------

/// `meet` fits the whole box inside the viewport and centres the leftover;
/// `slice` fills the viewport and lets the box overflow. The two differ only
/// in which of the two scales wins, and a square box in a wide viewport is
/// where that is visible.
#[test]
fn meet_takes_the_smaller_scale_and_slice_the_larger() {
    let meet = transform::view_box([0.0, 0.0, 10.0, 10.0], 40.0, 20.0, None).unwrap();
    near(meet[0], 2.0, "meet scales by the smaller, 20/10");
    near(meet[3], 2.0, "and uniformly");
    // 40 wide, 20 of box: 20 left over, centred by the default xMidYMid.
    near(meet[4], 10.0, "the leftover is centred");

    let slice =
        transform::view_box([0.0, 0.0, 10.0, 10.0], 40.0, 20.0, Some("xMidYMid slice")).unwrap();
    near(slice[0], 4.0, "slice scales by the larger, 40/10");

    let none = transform::view_box([0.0, 0.0, 10.0, 10.0], 40.0, 20.0, Some("none")).unwrap();
    near(none[0], 4.0, "none stretches x");
    near(none[3], 2.0, "and y independently");
}

/// §7.7: a degenerate view box disables rendering of the element, which is a
/// different answer from mapping it with an identity.
#[test]
fn a_degenerate_view_box_is_not_an_identity() {
    assert!(transform::view_box([0.0, 0.0, 0.0, 10.0], 40.0, 20.0, None).is_none());
    assert!(transform::view_box([0.0, 0.0, 10.0, -1.0], 40.0, 20.0, None).is_none());
}

// ---- path data ------------------------------------------------------------

fn outline(data: &str) -> Vec<Segment> {
    let mut budget = 4096;
    path::parse(data, &mut budget)
        .expect("the path parses")
        .segments
}

/// The absolute and relative spellings of one square are the same square.
#[test]
fn relative_commands_resolve_against_the_pen() {
    let absolute = outline("M 10 10 L 20 10 L 20 20 L 10 20 Z");
    let relative = outline("m 10 10 l 10 0 l 0 10 l -10 0 z");
    assert_eq!(absolute, relative);
    assert_eq!(absolute.len(), 5);
}

/// §8.3.2: numbers after a `moveto` repeat it as a **lineto**, not as more
/// movetos. A parser that repeated the moveto would draw four subpaths of one
/// point each and no square at all — and would look identical in a `d` that
/// closed after every point.
#[test]
fn numbers_after_a_moveto_are_linetos() {
    let segments = outline("M 0 0 10 0 10 10");
    assert_eq!(
        segments,
        vec![
            Segment::Move([0.0, 0.0]),
            Segment::Line([10.0, 0.0]),
            Segment::Line([10.0, 10.0]),
        ]
    );
}

/// `H` and `V` keep the other coordinate, and their relative forms add to it.
#[test]
fn horizontal_and_vertical_keep_the_other_coordinate() {
    assert_eq!(
        outline("M 5 7 H 9 V 2 h -3 v 1"),
        vec![
            Segment::Move([5.0, 7.0]),
            Segment::Line([9.0, 7.0]),
            Segment::Line([9.0, 2.0]),
            Segment::Line([6.0, 2.0]),
            Segment::Line([6.0, 3.0]),
        ]
    );
}

/// **A quadratic raised to a cubic is exact**, and the identity is checkable
/// without trusting the code: both curves must pass through the same point at
/// the parameter's midpoint.
#[test]
fn a_quadratic_raised_to_a_cubic_is_the_same_curve() {
    let segments = outline("M 0 0 Q 10 20 20 0");
    let Segment::Cubic(a, b, end) = segments[1] else {
        panic!("a cubic: {segments:?}");
    };
    // Degree elevation: each control two thirds of the way from its own end.
    point(a, [20.0 / 3.0, 40.0 / 3.0], "the first control");
    point(b, [40.0 / 3.0, 40.0 / 3.0], "the second control");
    point(end, [20.0, 0.0], "the endpoint");

    // And the midpoints agree, computed here from both formulas.
    let from = [0.0, 0.0];
    let control = [10.0, 20.0];
    let quad_mid = [
        0.25 * from[0] + 0.5 * control[0] + 0.25 * end[0],
        0.25 * from[1] + 0.5 * control[1] + 0.25 * end[1],
    ];
    let cubic_mid = [
        0.125 * from[0] + 0.375 * a[0] + 0.375 * b[0] + 0.125 * end[0],
        0.125 * from[1] + 0.375 * a[1] + 0.375 * b[1] + 0.125 * end[1],
    ];
    point(cubic_mid, quad_mid, "the curves' midpoints");
}

/// §8.3.6: a smooth command whose predecessor was **not** of its family
/// reflects the current point, not the previous control. Getting the fallback
/// wrong makes a smooth curve after a line bulge.
#[test]
fn a_smooth_command_after_a_line_reflects_the_current_point() {
    let segments = outline("M 0 0 L 10 0 S 20 10 30 0");
    let Segment::Cubic(a, _, _) = segments[2] else {
        panic!("a cubic: {segments:?}");
    };
    point(a, [10.0, 0.0], "the reflection is the pen itself");
}

/// **The arc's two flags are one character each**, which is the classic
/// path-parser defect: `a1 1 0 1130` is an arc with both flags set and an x of
/// 30, where a number scan reads `1130` and produces a wildly wrong curve.
#[test]
fn an_arcs_flags_are_single_characters_and_not_numbers() {
    let packed = outline("M 0 0 a1 1 0 1130 0");
    let spaced = outline("M 0 0 a 1 1 0 1 1 30 0");
    assert_eq!(packed, spaced, "the two spellings are one arc");
    assert!(packed.len() > 1, "and it produced curves: {packed:?}");
}

/// An arc ends **exactly** where the command says, whatever the angles
/// reproduce — a rounding error at the join of two arcs is a visible gap.
#[test]
fn an_arc_ends_exactly_where_the_command_says() {
    for data in [
        "M 0 0 A 50 50 0 0 1 100 0",
        "M 0 0 A 50 50 0 1 0 100 0",
        "M 10 10 A 30 60 45 1 1 70 40",
    ] {
        let segments = outline(data);
        let Some(Segment::Cubic(_, _, end)) = segments.last() else {
            panic!("{data}: ends with a cubic: {segments:?}");
        };
        let expected = transform::numbers(
            data.rsplit(|c: char| c.is_ascii_alphabetic())
                .next()
                .unwrap(),
        )
        .expect("the command's own last two numbers");
        point(
            *end,
            [expected[expected.len() - 2], expected[expected.len() - 1]],
            data,
        );
    }
}

/// F.6.6.2: radii too small to span the endpoints are scaled up until they
/// exactly do, rather than producing nothing.
#[test]
fn radii_too_small_are_grown_rather_than_refused() {
    let segments = outline("M 0 0 A 1 1 0 0 1 100 0");
    assert!(segments.len() > 1, "the arc still drew: {segments:?}");
    let Some(Segment::Cubic(_, _, end)) = segments.last() else {
        panic!("{segments:?}");
    };
    point(*end, [100.0, 0.0], "and it still ends where it was told");
}

/// F.6.6.1: a zero radius is a straight line.
#[test]
fn a_zero_radius_arc_is_a_line() {
    let segments = outline("M 0 0 A 0 50 0 0 1 100 0");
    assert_eq!(segments.len(), 2, "one move and one segment: {segments:?}");
}

/// §8.3.2: *"the path data up to the point of the error is rendered"*. Half a
/// path is what a real file that goes wrong halfway should draw.
#[test]
fn data_that_goes_wrong_halfway_keeps_the_half_that_parsed() {
    let segments = outline("M 0 0 L 10 0 L nonsense");
    assert_eq!(
        segments,
        vec![Segment::Move([0.0, 0.0]), Segment::Line([10.0, 0.0])]
    );
}

/// A `d` that does not begin with a moveto has drawn nothing before it went
/// wrong, so there is no prefix to keep.
#[test]
fn data_that_does_not_begin_with_a_moveto_is_refused() {
    let mut budget = 4096;
    assert_eq!(path::parse("L 10 0", &mut budget), Err(PathError::Syntax));
    assert_eq!(
        path::parse("", &mut budget).map(|o| o.segments.len()),
        Ok(0),
        "and empty data is an empty path rather than an error"
    );
}

/// One `d` cannot be the whole document.
#[test]
fn a_path_past_its_budget_is_refused_by_name() {
    let mut data = String::from("M 0 0");
    for index in 0..100 {
        data.push_str(&format!(" L {index} 1"));
    }
    let mut budget = 10;
    assert_eq!(
        path::parse(&data, &mut budget),
        Err(PathError::TooManySegments)
    );
}

/// A transform reaches every point of every segment kind.
#[test]
fn a_transformed_outline_moves_every_control_point() {
    let mut budget = 4096;
    let outline = path::parse("M 1 1 L 2 2 C 3 3 4 4 5 5 Z", &mut budget).unwrap();
    let moved = outline.transformed([2.0, 0.0, 0.0, 2.0, 10.0, 20.0]);
    assert_eq!(
        moved.segments,
        vec![
            Segment::Move([12.0, 22.0]),
            Segment::Line([14.0, 24.0]),
            Segment::Cubic([16.0, 26.0], [18.0, 28.0], [20.0, 30.0]),
            Segment::Close,
        ]
    );
}

/// A path of nothing but moves and closes draws no ink, and says so.
#[test]
fn an_outline_of_moves_alone_is_empty() {
    let mut budget = 4096;
    assert!(path::parse("M 0 0 M 5 5 Z", &mut budget)
        .unwrap()
        .is_empty());
    assert!(!path::parse("M 0 0 L 1 1", &mut budget).unwrap().is_empty());
}

// ---- the fuzz seeds -------------------------------------------------------

/// **Every committed fuzz seed still exercises what it was committed for.**
///
/// `docs/verification.md` records why this test exists: `icc_profile` was the
/// one target whose corpus had no seeds, so its budget went on random bytes
/// against a format gated on a signature — and nothing said so. A seed that
/// quietly stopped parsing sits in the corpus looking like the coverage it no
/// longer is, and only a test that reads it can tell.
///
/// The assertions are the target's own, minus the ones that need libFuzzer:
/// the first byte is the control byte, the rest is the text, nothing escapes
/// that is not a finite number, and an outline begins with a move.
///
/// **Since milestone 1 it drives the document surface too.** A seed that is
/// markup rather than path data parses to nothing here on the geometry half —
/// which is what the `if let Ok` arms are for — and a suite that stopped there
/// would be a corpus of documents proving something about a path parser.
#[test]
fn every_committed_fuzz_seed_still_parses_to_finite_numbers() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fuzz/corpus/svg")
        .canonicalize()
        .expect("the seed corpus is committed beside the target");
    let mut seen = 0usize;
    for entry in std::fs::read_dir(&dir).expect("the seed corpus reads") {
        let file = entry.expect("a seed").path();
        if !file.is_file() {
            continue;
        }
        let bytes = std::fs::read(&file).expect("a seed reads");
        let name = file
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let (control, body) = bytes.split_at(bytes.len().min(1));
        let text = core::str::from_utf8(body).unwrap_or_else(|_| panic!("{name}: seeds are text"));
        let granted = match control.first().copied().unwrap_or(0) & 3 {
            0 => 0,
            1 => 4,
            2 => 512,
            _ => 1 << 20,
        };

        if let Some(matrix) = transform::list(text) {
            assert!(
                matrix.iter().all(|n| n.is_finite()),
                "{name}: a transform produced something that is not a number"
            );
        }
        let mut budget = granted;
        if let Ok(outline) = path::parse(text, &mut budget) {
            assert!(outline.segments.len() <= granted, "{name}: past its budget");
            assert_eq!(
                outline.segments.len() + budget,
                granted,
                "{name}: the budget does not add up"
            );
            if let Some(first) = outline.segments.first() {
                assert!(
                    matches!(first, Segment::Move(_)),
                    "{name}: an outline began with {first:?}"
                );
            }
            for segment in &outline.segments {
                let points = match *segment {
                    Segment::Move(p) | Segment::Line(p) => vec![p],
                    Segment::Cubic(a, b, c) => vec![a, b, c],
                    Segment::Close => Vec::new(),
                };
                for point in points {
                    assert!(
                        point[0].is_finite() && point[1].is_finite(),
                        "{name}: {segment:?} carries something that is not a number"
                    );
                }
            }
        }
        // The document surface, over the same bytes and before the UTF-8 gate
        // the geometry half needs: `read` decides the encoding itself.
        let limits = crate::Limits::DEFAULT;
        if let Ok(scene) = crate::read(body, Some((100.0, 50.0)), &limits) {
            assert!(
                scene.size.0.is_finite() && scene.size.1.is_finite(),
                "{name}: a scene's size is not numbers"
            );
            assert!(
                scene.warnings.len() <= limits.max_warnings,
                "{name}: past the warning cap"
            );
            for (at, warning) in scene.warnings.iter().enumerate() {
                assert!(
                    !scene.warnings[..at].contains(warning),
                    "{name}: {warning:?} was reported twice"
                );
            }
            assert_eq!(
                crate::read(body, Some((100.0, 50.0)), &limits).ok(),
                Some(scene),
                "{name}: reading a document is not deterministic"
            );
        }
        seen += 1;
    }
    // A corpus that emptied itself would pass every assertion above.
    assert!(
        seen >= 18,
        "only {seen} seeds were read from {}",
        dir.display()
    );
}

/// `1e999` is a number a person can type and `f64` cannot hold, and it must
/// never reach a consumer: an infinity in a coordinate is not a wrong picture,
/// it is a rasterizer with nothing to draw and a file that looked ordinary.
#[test]
fn a_coordinate_too_large_for_a_double_does_not_escape() {
    let mut budget = 4096;
    let outline = path::parse("M 0 0 L 1e999 0", &mut budget).expect("the move parsed");
    assert_eq!(
        outline.segments,
        vec![Segment::Move([0.0, 0.0])],
        "the lineto is dropped rather than carried as an infinity"
    );
    assert!(transform::numbers("1e999").is_none());
}
