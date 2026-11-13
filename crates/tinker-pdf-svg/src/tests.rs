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

/// A rotation by an angle of any size is still a rotation: its matrix is
/// finite, orthonormal, and fixes its centre. An angle of 3·10^37 degrees
/// is a turn far past where a quadrant count fits a double's integers, and
/// its sine and cosine are reduced exactly rather than left as a remainder
/// no polynomial can take.
#[test]
fn a_rotation_by_a_huge_angle_is_still_a_rotation() {
    let matrix = transform::list("rotate(30444444444444444444444444444444444444, 7, -3)").unwrap();
    assert!(
        matrix.iter().all(|n| n.is_finite()),
        "a finite matrix: {matrix:?}"
    );
    assert!(
        (matrix[0] * matrix[0] + matrix[1] * matrix[1] - 1.0).abs() < 1e-12,
        "a turn, not a stretch: {matrix:?}"
    );
    point(
        transform::apply(matrix, [7.0, -3.0]),
        [7.0, -3.0],
        "the centre of the rotation",
    );
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

/// A closepath takes no parameters, so what follows one that is not a command
/// is where the data goes wrong — and every turn of the reader consumes a
/// byte, so a `d` cannot produce more segments than it has characters to
/// spend. `Z%` once repeated the close without reading anything, a close a
/// turn until the budget ran out: with `shape::path`'s unbounded budget, 1.8
/// GB from a 463-byte document (CI's `fuzz-seeds`, 10 October 2026; the input
/// is the corpus's `document-close-then-junk`).
#[test]
fn what_follows_a_closepath_that_is_not_a_command_ends_the_data() {
    for (data, drawn) in [
        ("M 0 0 L 5 0 Z%M 1 1", 3),
        ("M 0 0 L 5 0 Z 7 8", 3),
        ("M 0 0 L 5 0 z,", 3),
        ("M 0 0 L 5 0 Z M 1 1 L 2 2", 5),
    ] {
        // Generous rather than `shape::path`'s unbounded one, so that a reader
        // that repeats the close again fails here by name and does not hang.
        const GRANTED: usize = 1 << 16;
        let mut budget = GRANTED;
        let outline = path::parse(data, &mut budget).expect("the moveto parsed");
        assert_eq!(outline.segments.len(), drawn, "{data:?}: {outline:?}");
        assert_eq!(GRANTED - budget, drawn, "{data:?}: spent what it drew");
    }
    assert_eq!(
        outline("M 0 0 L 5 0 Z%M 1 1"),
        vec![
            Segment::Move([0.0, 0.0]),
            Segment::Line([5.0, 0.0]),
            Segment::Close
        ]
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
/// A target whose seeds reach nothing spends its budget on random bytes, and
/// nothing says so: a seed that quietly stopped parsing sits in the corpus
/// looking like the coverage it no longer is, and only a test that reads it
/// can tell (`docs/verification.md`, *The roster, and whether the seeds reach
/// the code*).
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
    let mut boxes = 0usize;
    let mut elements = 0usize;
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
            let mut numbers = Vec::new();
            sweep_nodes(&scene.nodes, &mut numbers);
            assert!(
                numbers.iter().all(|n| n.is_finite()),
                "{name}: a node carries something that is not a number"
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
        // The runs measured, as the target's third read measures them: no
        // mark reaches the caller, and a document with nothing to measure
        // reads the same.
        let plain = crate::read(body, Some((100.0, 50.0)), &limits).ok();
        let pitch = Pitch(if control.first().copied().unwrap_or(0) & 0x40 == 0 {
            0.5
        } else {
            1e300
        });
        let measured = crate::read_with(
            body,
            Some((100.0, 50.0)),
            &limits,
            &crate::Context::NONE.with_measure(&pitch),
        );
        if let Ok(scene) = &measured {
            let mut numbers = Vec::new();
            sweep_nodes(&scene.nodes, &mut numbers);
            assert!(
                numbers.iter().all(|n| n.is_finite()),
                "{name}: a measured node carries something that is not a number"
            );
            assert!(
                colours_in_range(&scene.nodes),
                "{name}: a colour no document states reached the caller"
            );
        }
        if let Some(plain) = &plain {
            let named = plain.warnings.contains(&crate::Warning::TextBoxUnmeasured);
            if !named && plain.warnings.len() < limits.max_warnings {
                assert_eq!(
                    measured.as_ref().ok(),
                    Some(plain),
                    "{name}: nothing to measure, and it read differently"
                );
            }
            if named
                && measured
                    .as_ref()
                    .is_ok_and(|scene| !scene.warnings.contains(&crate::Warning::TextBoxUnmeasured))
            {
                boxes += 1;
            }
        }
        // One element, by the first id the document has, as the target reads
        // it: the same invariants, and the tree's door and the bytes' door one
        // answer.
        if let Ok(tree) = crate::document::read(body, &limits) {
            if let Some(id) = tree.nodes.iter().find_map(|node| node.id.clone()) {
                let by_id = crate::read_element(body, &id, Some((100.0, 50.0)), &limits);
                if let Ok(scene) = &by_id {
                    let mut numbers = Vec::new();
                    sweep_nodes(&scene.nodes, &mut numbers);
                    assert!(
                        numbers.iter().all(|n| n.is_finite()),
                        "{name}: an element read by its id carries something that is not a number"
                    );
                    assert!(
                        colours_in_range(&scene.nodes),
                        "{name}: a colour no document states reached the caller"
                    );
                    elements += 1;
                }
                assert_eq!(
                    crate::scene::build_element(&tree, &id, Some((100.0, 50.0)), &limits),
                    by_id,
                    "{name}: reading an element by its id is not deterministic"
                );
            }
        }
        seen += 1;
    }
    // A corpus that emptied itself would pass every assertion above.
    assert!(
        seen >= 18,
        "only {seen} seeds were read from {}",
        dir.display()
    );
    assert!(
        boxes >= 1,
        "no seed reaches a box that only a measurer gives"
    );
    assert!(elements >= 1, "no seed reads an element by its id");
}

/// Every character `.0` ems wide, eight tenths of one above the baseline and
/// two below — the fuzz target's measurer.
struct Pitch(f64);

impl crate::MeasureText for Pitch {
    fn measure(&self, text: &str, font: &crate::TextStyle) -> crate::RunMetrics {
        #[allow(clippy::cast_precision_loss)]
        let count = text.chars().count() as f64;
        crate::RunMetrics {
            advance: self.0 * font.size * count,
            ascent: 0.8 * font.size,
            descent: 0.2 * font.size,
        }
    }
}

/// Whether every colour a list of nodes paints with, at every depth of a
/// group, a mask and a tile, is one a document can state.
fn colours_in_range(nodes: &[crate::Node]) -> bool {
    fn paint(server: &crate::Paint) -> bool {
        match server {
            crate::Paint::Solid(colour) => colour.rgb.iter().all(|c| (0.0..=1.0).contains(c)),
            crate::Paint::Pattern(tile) => colours_in_range(&tile.nodes),
            _ => true,
        }
    }
    nodes.iter().all(|node| match node {
        crate::Node::Path { fill, stroke, .. } | crate::Node::Text { fill, stroke, .. } => {
            paint(fill) && stroke.as_ref().is_none_or(|stroke| paint(&stroke.paint))
        }
        crate::Node::Group { nodes, mask, .. } => {
            colours_in_range(nodes) && mask.as_ref().is_none_or(|m| colours_in_range(&m.nodes))
        }
        crate::Node::Image { .. } => true,
    })
}

/// Every number a list of nodes carries, at every depth — the fuzz target's
/// own sweep, so a seed proves on `cargo test` what the target asserts.
fn sweep_nodes(nodes: &[crate::Node], out: &mut Vec<f64>) {
    let outline = |outline: &path::Outline, out: &mut Vec<f64>| {
        for segment in &outline.segments {
            match *segment {
                Segment::Move(p) | Segment::Line(p) => out.extend(p),
                Segment::Cubic(a, b, c) => out.extend([a, b, c].concat()),
                Segment::Close => {}
            }
        }
    };
    let paint = |paint: &crate::Paint, out: &mut Vec<f64>| match paint {
        crate::Paint::Linear {
            from, to, matrix, ..
        } => {
            out.extend(from);
            out.extend(to);
            out.extend(matrix);
        }
        crate::Paint::Radial {
            centre,
            radius,
            focus,
            matrix,
            ..
        } => {
            out.extend(centre);
            out.push(*radius);
            out.extend(focus);
            out.extend(matrix);
        }
        crate::Paint::Pattern(tile) => {
            out.extend(tile.cell);
            out.extend(tile.matrix);
            sweep_nodes(&tile.nodes, out);
        }
        _ => {}
    };
    for node in nodes {
        match node {
            crate::Node::Path {
                outline: shape,
                fill,
                fill_opacity,
                stroke,
                clip,
                ..
            } => {
                outline(shape, out);
                paint(fill, out);
                out.push(*fill_opacity);
                if let Some(clip) = clip {
                    outline(&clip.outline, out);
                }
                if let Some(stroke) = stroke {
                    paint(&stroke.paint, out);
                    out.extend([stroke.width, stroke.opacity, stroke.dash_offset]);
                }
            }
            crate::Node::Text {
                anchor,
                matrix,
                font,
                ..
            } => {
                out.extend(anchor.unwrap_or_default());
                out.extend(matrix);
                out.push(font.size);
            }
            crate::Node::Image { rect, matrix, .. } => {
                out.extend(rect);
                out.extend(matrix);
            }
            crate::Node::Group {
                nodes,
                opacity,
                clip,
                mask,
            } => {
                out.push(*opacity);
                if let Some(clip) = clip {
                    outline(&clip.outline, out);
                }
                if let Some(mask) = mask {
                    if let Some(region) = &mask.region {
                        outline(region, out);
                    }
                    sweep_nodes(&mask.nodes, out);
                }
                sweep_nodes(nodes, out);
            }
        }
    }
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

// ---- one element read by its id -------------------------------------------

/// Two glyph elements in one document, sharing a gradient and a path in
/// `<defs>`: `glyph1` is a `<use>` of the path, moved ten units right, and
/// `glyph2` a rectangle under a `scale(9)` it does not inherit when it is read
/// by its id. The root's view box puts user `y = -1000` at the top of the
/// 1000-unit viewport, so a user point `(x, y)` is the scene's `(x, y + 1000)`;
/// and a green circle stands outside both glyphs.
const GLYPHS: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg"
    xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 -1000 1000 1000">
  <defs>
    <linearGradient id="grad"><stop offset="0" stop-color="#ff0000"/>
      <stop offset="1" stop-color="#0000ff"/></linearGradient>
    <path id="base" d="M100 -100H300V-500H100Z"/>
  </defs>
  <g id="glyph1" transform="translate(10,0)"><use xlink:href="#base" fill="url(#grad)"/></g>
  <g transform="scale(9)">
    <rect id="glyph2" x="500" y="-800" width="200" height="300" fill="url(#grad)"/>
  </g>
  <circle cx="0" cy="0" r="50" fill="green"/>
</svg>"##;

/// The bounds of every point an outline carries, `[min_x, min_y, max_x,
/// max_y]`.
fn bounds(outline: &path::Outline) -> [f64; 4] {
    let mut out = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for segment in &outline.segments {
        let points = match *segment {
            Segment::Move(p) | Segment::Line(p) => vec![p],
            Segment::Cubic(a, b, c) => vec![a, b, c],
            Segment::Close => Vec::new(),
        };
        for [x, y] in points {
            out = [out[0].min(x), out[1].min(y), out[2].max(x), out[3].max(y)];
        }
    }
    out
}

/// The one path a scene read by an id draws: its bounds and its fill.
fn only_path(scene: &crate::Scene) -> ([f64; 4], &crate::Paint) {
    assert_eq!(scene.nodes.len(), 1, "{:?}", scene.nodes);
    match &scene.nodes[0] {
        crate::Node::Path { outline, fill, .. } => (bounds(outline), fill),
        other => panic!("a path, not {other:?}"),
    }
}

/// A fill that is `#grad`, red to blue, its gradient space the unit square
/// mapped onto `box_` — §13.2.3's `objectBoundingBox` — in the scene's space.
fn assert_the_gradient(fill: &crate::Paint, box_: [f64; 4], what: &str) {
    let crate::Paint::Linear { matrix, stops, .. } = fill else {
        panic!("{what}: the gradient, not {fill:?}");
    };
    assert_eq!(stops.len(), 2, "{what}");
    assert_eq!(stops[0].colour.rgb, [1.0, 0.0, 0.0], "{what}");
    assert_eq!(stops[1].colour.rgb, [0.0, 0.0, 1.0], "{what}");
    point(
        transform::apply(*matrix, [0.0, 0.0]),
        [box_[0], box_[1]],
        what,
    );
    point(
        transform::apply(*matrix, [1.0, 1.0]),
        [box_[2], box_[3]],
        what,
    );
}

/// **FT-06.9's exit, the first half.** Each glyph read by its id is its own
/// paths with the gradient they share resolved for each — `glyph1` through the
/// `<use>` it holds, `glyph2` without the `scale(9)` written above it — under
/// the root's view box, and nothing else of the document: not the other
/// glyph, and not the circle.
#[test]
fn an_element_is_read_by_id_with_what_it_references() {
    let limits = crate::Limits::DEFAULT;
    let viewport = Some((1000.0, 1000.0));

    let one = crate::read_element(GLYPHS, "glyph1", viewport, &limits).expect("glyph1 reads");
    let (box_, fill) = only_path(&one);
    // The path's user box, moved ten right by its `<g>` and down 1000 by the
    // view box.
    let expected = [110.0, 500.0, 310.0, 900.0];
    for (got, want) in box_.iter().zip(expected) {
        near(*got, want, "glyph1's path");
    }
    assert_the_gradient(fill, expected, "glyph1");
    assert_eq!(one.size, (1000.0, 1000.0));
    assert!(one.warnings.is_empty(), "{:?}", one.warnings);

    let two = crate::read_element(GLYPHS, "glyph2", viewport, &limits).expect("glyph2 reads");
    let (box_, fill) = only_path(&two);
    let expected = [500.0, 200.0, 700.0, 500.0];
    for (got, want) in box_.iter().zip(expected) {
        near(*got, want, "glyph2's rectangle, unscaled");
    }
    assert_the_gradient(fill, expected, "glyph2");

    // The whole document draws the three, the rectangle under its scale.
    let whole = crate::read(GLYPHS, viewport, &limits).expect("the document reads");
    assert_eq!(whole.nodes.len(), 3, "{:?}", whole.nodes);

    // An element inside `<defs>` reads as a `<use>` of it draws it: the path,
    // painted as its own attributes say — black, there being no fill.
    let base = crate::read_element(GLYPHS, "base", viewport, &limits).expect("base reads");
    let (box_, fill) = only_path(&base);
    for (got, want) in box_.iter().zip([100.0, 500.0, 300.0, 900.0]) {
        near(*got, want, "the defined path");
    }
    assert_eq!(fill, &crate::Paint::Solid(crate::Colour { rgb: [0.0; 3] }));
    // A paint server read by its id draws nothing where it stands, as a
    // `<use>` of one draws nothing.
    let grad = crate::read_element(GLYPHS, "grad", viewport, &limits).expect("grad reads");
    assert!(grad.nodes.is_empty(), "{:?}", grad.nodes);

    // The tree's door agrees with the bytes' door, so a caller reading many
    // glyphs from one document parses it once.
    let tree = crate::document::read(GLYPHS, &limits).expect("the tree reads");
    assert_eq!(
        crate::scene::build_element(&tree, "glyph1", viewport, &limits),
        Ok(one)
    );
}

#[test]
fn an_id_naming_nothing_is_a_refusal() {
    let limits = crate::Limits::DEFAULT;
    for id in ["glyph3", "Glyph1", "glyph01", " glyph1", ""] {
        assert_eq!(
            crate::read_element(GLYPHS, id, Some((1000.0, 1000.0)), &limits),
            Err(crate::Refusal::IdUnresolved),
            "{id:?}"
        );
    }
    // A document that does not read is refused for that first, whatever the
    // id.
    assert_eq!(
        crate::read_element(b"<svg", "glyph1", None, &limits),
        Err(crate::Refusal::Unreadable)
    );
    assert_eq!(
        crate::read_element(b"<html id=\"glyph1\"/>", "glyph1", None, &limits),
        Err(crate::Refusal::NotAnSvg)
    );
}

#[test]
fn a_use_cycle_through_an_id_is_refused() {
    let limits = crate::Limits::DEFAULT;
    // An element that uses itself, and two that use each other.
    let direct = br##"<svg xmlns="http://www.w3.org/2000/svg">
        <g id="glyph1"><rect width="1" height="1"/><use href="#glyph1"/></g></svg>"##;
    assert_eq!(
        crate::read_element(direct, "glyph1", None, &limits),
        Err(crate::Refusal::TooManyUses)
    );
    let indirect = br##"<svg xmlns="http://www.w3.org/2000/svg">
        <g id="glyph2"><use href="#a"/></g><g id="a"><use href="#glyph2"/></g></svg>"##;
    assert_eq!(
        crate::read_element(indirect, "glyph2", None, &limits),
        Err(crate::Refusal::TooManyUses)
    );
    // The root read by its own id, and used from inside itself.
    let rooted = br##"<svg xmlns="http://www.w3.org/2000/svg" id="glyph3">
        <use href="#glyph3"/></svg>"##;
    assert_eq!(
        crate::read_element(rooted, "glyph3", None, &limits),
        Err(crate::Refusal::TooManyUses)
    );
    // A `<use>` of the element from outside it is no cycle, and is not drawn.
    let outside = br##"<svg xmlns="http://www.w3.org/2000/svg">
        <g id="glyph4"><rect width="1" height="1"/></g><use href="#glyph4" x="5"/></svg>"##;
    let scene = crate::read_element(outside, "glyph4", None, &limits).expect("no cycle");
    assert_eq!(scene.nodes.len(), 1, "{:?}", scene.nodes);
    // The reference is no `<use>` the document wrote, so it spends none of
    // the cap; one inside the element still spends its own.
    let mut no_uses = limits;
    no_uses.max_uses = 0;
    assert!(crate::read_element(outside, "glyph4", None, &no_uses).is_ok());
    assert_eq!(
        crate::read_element(GLYPHS, "glyph1", None, &no_uses),
        Err(crate::Refusal::TooManyUses)
    );
}

/// The root's viewport is the read's — its view box mapped onto the caller's
/// viewport — and the transforms and nested viewports between the root and
/// the element are not; the element's own transform is.
#[test]
fn an_element_read_by_id_takes_the_roots_viewport_and_not_its_ancestors() {
    let limits = crate::Limits::DEFAULT;
    // A view box of 500 onto a viewport of 1000 doubles every user unit.
    let document = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 500 500">
        <svg x="100" y="100" viewBox="0 0 10 10" width="20" height="20">
          <g transform="rotate(30)">
            <rect id="glyph1" x="1" y="2" width="3" height="4" transform="translate(5,0)"/>
          </g>
        </svg></svg>"##;
    let scene = crate::read_element(document, "glyph1", Some((1000.0, 1000.0)), &limits)
        .expect("glyph1 reads");
    let (box_, _) = only_path(&scene);
    // (1 + 5, 2) to (4 + 5, 6), doubled.
    for (got, want) in box_.iter().zip([12.0, 4.0, 18.0, 12.0]) {
        near(*got, want, "the rectangle in the root's viewport");
    }
    assert_eq!(scene.size, (1000.0, 1000.0));

    // The root read by its id is the document read whole.
    let named =
        br##"<svg xmlns="http://www.w3.org/2000/svg" id="glyph7" viewBox="0 1000 1000 1000">
        <rect x="100" y="570" width="200" height="430" fill="#00aab3"/>
        <rect x="100" y="365" width="200" height="135" fill="darkblue"/></svg>"##;
    let viewport = Some((1000.0, 1000.0));
    assert_eq!(
        crate::read_element(named, "glyph7", viewport, &limits),
        crate::read(named, viewport, &limits)
    );

    // A `<symbol>` read by its id is the viewport a `<use>` of it makes, its
    // view box mapped onto the whole of the root's.
    let symbol = br##"<svg xmlns="http://www.w3.org/2000/svg">
        <symbol id="glyph5" viewBox="0 0 10 10"><rect width="5" height="5"/></symbol></svg>"##;
    let scene = crate::read_element(symbol, "glyph5", Some((100.0, 100.0)), &limits)
        .expect("the symbol reads");
    let (box_, _) = only_path(&scene);
    for (got, want) in box_.iter().zip([0.0, 0.0, 50.0, 50.0]) {
        near(*got, want, "the symbol's square");
    }
}

/// The element inherits what a `<use>` of it placed in the root inherits —
/// the root's own style — and not the style of the elements between them;
/// a `display: none` on the root or on the element draws nothing.
#[test]
fn an_element_read_by_id_inherits_from_the_root_and_not_from_its_ancestors() {
    let limits = crate::Limits::DEFAULT;
    let document = br##"<svg xmlns="http://www.w3.org/2000/svg" fill="#ff0000">
        <g fill="#0000ff"><rect id="glyph1" width="4" height="4"/></g>
        <rect id="glyph2" width="4" height="4" display="none"/></svg>"##;
    let red = crate::Paint::Solid(crate::Colour {
        rgb: [1.0, 0.0, 0.0],
    });
    let blue = crate::Paint::Solid(crate::Colour {
        rgb: [0.0, 0.0, 1.0],
    });
    let scene = crate::read_element(document, "glyph1", None, &limits).expect("glyph1 reads");
    let (_, fill) = only_path(&scene);
    assert_eq!(fill, &red, "the root's red, not the group's blue");
    // Read whole, the rectangle is the group's blue.
    let whole = crate::read(document, None, &limits).expect("the document reads");
    let (_, fill) = only_path(&whole);
    assert_eq!(fill, &blue);

    let hidden = crate::read_element(document, "glyph2", None, &limits).expect("glyph2 reads");
    assert!(hidden.nodes.is_empty(), "{:?}", hidden.nodes);
    let hidden_root = br##"<svg xmlns="http://www.w3.org/2000/svg" display="none">
        <rect id="glyph3" width="4" height="4"/></svg>"##;
    let scene = crate::read_element(hidden_root, "glyph3", None, &limits).expect("reads");
    assert!(scene.nodes.is_empty(), "{:?}", scene.nodes);
}

/// Every node's fill, in order, descending into groups.
fn fills(nodes: &[crate::Node]) -> Vec<(crate::Paint, f64)> {
    let mut out = Vec::new();
    for node in nodes {
        match node {
            crate::Node::Path {
                fill, fill_opacity, ..
            } => out.push((fill.clone(), *fill_opacity)),
            crate::Node::Group { nodes, .. } => out.extend(fills(nodes)),
            _ => {}
        }
    }
    out
}

/// **An element the caller's profile forbids is drawn as nothing**, its
/// content with it, and named once by its local name; the rest of the
/// document is drawn, and a document read with no profile draws all of it.
#[test]
fn an_element_the_caller_restricts_is_drawn_as_nothing_and_named() {
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
        <a><rect width="10" height="10"/></a>
        <rect x="20" width="10" height="10" fill="#00ff00"/>
        <switch><rect x="40" width="10" height="10"/></switch>
    </svg>"##;
    let limits = crate::Limits::DEFAULT;
    let context = crate::Context::NONE.with_restricted(&["a", "switch"]);
    let scene = crate::read_with(svg, None, &limits, &context).expect("reads");
    let painted = fills(&scene.nodes);
    assert_eq!(painted.len(), 1, "{:?}", scene.nodes);
    assert_eq!(
        painted[0].0,
        crate::Paint::Solid(crate::Colour {
            rgb: [0.0, 1.0, 0.0]
        })
    );
    assert!(scene
        .warnings
        .contains(&crate::Warning::ElementRestricted("a".to_owned())));
    assert!(scene
        .warnings
        .contains(&crate::Warning::ElementRestricted("switch".to_owned())));
    // `<switch>` is no element this build reads; `<a>` is a group.
    let open = crate::read(svg, None, &limits).expect("reads");
    assert_eq!(fills(&open.nodes).len(), 2, "{:?}", open.nodes);
    // An element read by its id that the profile forbids draws nothing.
    let by_id = br##"<svg xmlns="http://www.w3.org/2000/svg">
        <a id="glyph4"><rect width="10" height="10"/></a></svg>"##;
    let scene = crate::read_element_with(by_id, "glyph4", None, &limits, &context).expect("reads");
    assert!(scene.nodes.is_empty(), "{:?}", scene.nodes);
}

/// **`var(--colorN)` reads the caller's palette** in a fill, a stroke, a
/// stop and a `color`: the entry's colour stands for the value and its alpha
/// multiplies the element's own opacity where the paint is used, inherited
/// with the paint; a name past the palette or not of the form takes its
/// fallback, and one with no fallback is an unreadable value. A document read
/// with no palette takes every fallback.
#[test]
fn a_palette_variable_reads_the_callers_entry_and_its_alpha() {
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
        <rect width="10" height="10" fill="var(--color1, #ff0000)" fill-opacity="0.5"/>
        <g fill="var(--color0)"><rect x="20" width="10" height="10"/></g>
        <rect x="40" width="10" height="10" fill="var(--color7, #0000ff)"/>
        <rect x="60" width="10" height="10" fill="var(--other, #ff00ff)"/>
        <rect x="80" width="10" height="10" fill="var(--color9)"/>
    </svg>"##;
    let palette = [[1.0, 1.0, 0.0, 1.0], [0.0, 0.5, 1.0, 0.5]];
    let limits = crate::Limits::DEFAULT;
    let context = crate::Context::NONE.with_palette(&palette);
    let scene = crate::read_with(svg, None, &limits, &context).expect("reads");
    let solid = |rgb| crate::Paint::Solid(crate::Colour { rgb });
    let painted = fills(&scene.nodes);
    assert_eq!(
        painted,
        vec![
            // Entry 1's alpha times the element's own half.
            (solid([0.0, 0.5, 1.0]), 0.25),
            // Entry 0, inherited from the group with its alpha of one.
            (solid([1.0, 1.0, 0.0]), 1.0),
            // Past the palette, and not a palette name: the fallbacks.
            (solid([0.0, 0.0, 1.0]), 1.0),
            (solid([1.0, 0.0, 1.0]), 1.0),
            // No entry and no fallback: the inherited black, named.
            (solid([0.0, 0.0, 0.0]), 1.0),
        ]
    );
    assert!(scene.warnings.contains(&crate::Warning::ValueUnreadable {
        attribute: "fill".to_owned()
    }));
    let unthemed = crate::read(svg, None, &limits).expect("reads");
    assert_eq!(fills(&unthemed.nodes)[0], (solid([1.0, 0.0, 0.0]), 0.5));
}

/// **`currentColor` with no `color` set is the caller's** where it keeps it:
/// [`crate::Paint::Current`]; where a `color` is set, here or above, it is
/// that colour; a `stop-color` of the caller's colour is unreadable, a stop
/// holding no colour it can name. Read without the caller keeping it,
/// `currentColor` is `color`'s initial black, as before.
#[test]
fn current_colour_is_left_to_a_caller_that_keeps_it() {
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
        <defs><linearGradient id="g"><stop offset="0" stop-color="currentColor"/>
            <stop offset="1" stop-color="#ffffff"/></linearGradient></defs>
        <rect width="10" height="10" fill="currentColor"/>
        <g color="#00ff00"><rect x="20" width="10" height="10" fill="currentColor"/></g>
        <rect x="40" width="10" height="10" fill="url(#g)"/>
    </svg>"##;
    let limits = crate::Limits::DEFAULT;
    let context = crate::Context::NONE.with_foreground();
    let scene = crate::read_with(svg, None, &limits, &context).expect("reads");
    let painted = fills(&scene.nodes);
    assert_eq!(painted[0].0, crate::Paint::Current);
    assert_eq!(
        painted[1].0,
        crate::Paint::Solid(crate::Colour {
            rgb: [0.0, 1.0, 0.0]
        })
    );
    assert!(scene.warnings.contains(&crate::Warning::ValueUnreadable {
        attribute: "stop-color".to_owned()
    }));
    let plain = crate::read(svg, None, &limits).expect("reads");
    assert_eq!(
        fills(&plain.nodes)[0].0,
        crate::Paint::Solid(crate::Colour { rgb: [0.0; 3] })
    );
}
