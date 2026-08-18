//! Paths, brushes, transforms and resources on a fixed page (gap 30,
//! milestone 6).
//!
//! # Why so many of these come in pairs
//!
//! Gap 30's milestones 2 to 5 each closed with the same finding, and milestone
//! 5 stated it as one rule: **when a thing has two independent consequences, a
//! test for one of them is not a test.** Two rules sharing a name (XML 1.0 and
//! Namespaces §5.3 on duplicate attributes), one clause with two consequences
//! (OPC's case fold, on holding a part and on looking one up), one cache with
//! two halves, one mapping with two orders. Each had tests; each had tests for
//! one side.
//!
//! So the pairs this milestone owes are written as pairs from the start:
//!
//! | one rule | its twin |
//! | --- | --- |
//! | `L` names a point | `l` names a displacement |
//! | `Z` returns the pen to the figure's first point | a *relative* command after `Z` starts there |
//! | a `Data` that will not read is **not painted** | a `Clip` that will not read **refuses its element** |
//! | a canvas opacity over overlapping children is a **group** | over children that do not overlap it is **not** |
//! | `{StaticResource}` finds an outer key | an inner key **shadows** an outer one |
//! | a chain past the depth cap is `BrushTooDeep` | a chain that returns to itself is `BrushCyclic` |
//! | a page that will not read is grey | an element whose paint will not read is grey **and the page is not** |
//!
//! # And why the assertions are on the content stream
//!
//! Gap 30's geometry section keeps the flip in **one** `cm` on purpose, *"so
//! the numbers in the synthesised content stream are the numbers in the
//! markup"*, and says in as many words that the property is what *"makes
//! milestone 6's tests assertable against the markup rather than against
//! arithmetic done twice"*. That is taken literally here: `Data="M0,0L200,0"`
//! is asserted to arrive as `0 0 m` and `200 0 l`, and a test that had to
//! multiply by 0.75 to know what to expect would be doing the reader's
//! arithmetic a second time and agreeing with itself.

mod xps_support;

use tinker_pdf::xps::geometry::{self, FillRule, Segment};
use tinker_pdf::xps::markup::Budget;
use tinker_pdf::{ArchiveRefusal, ArchiveWarning, Document, XpsElementDefect};
use xps_support::{
    archive, before_content_types, document, one_page_package, open, part, with, XPS_NS,
};

/// The resource-dictionary-key namespace XPS 1.0 uses, which is what all six
/// `wpf-` packages carry on `xmlns:x`.
const KEY_NS: &str = "http://schemas.microsoft.com/xps/2005/06/resourcedictionary-key";

// ---- helpers ------------------------------------------------------------

/// A package whose one fixed page carries `body` between `<FixedPage>` tags.
fn package(body: &str) -> Vec<u8> {
    let markup = format!(
        r#"<FixedPage xmlns="{XPS_NS}" xmlns:x="{KEY_NS}" Width="816" Height="1056">{body}</FixedPage>"#
    );
    archive(with(
        one_page_package(),
        "Documents/1/Pages/1.fpage",
        &markup,
    ))
}

/// The one page's content stream, as text.
fn stream(document: &Document) -> String {
    let cos = document.cos();
    let pages = tinker_pdf_cos::pages::collect(cos);
    let page = pages.first().expect("one page");
    String::from_utf8_lossy(&tinker_pdf_cos::pages::content_bytes(cos, page)).into_owned()
}

/// Opens a page body and hands back its content stream.
fn drawn(body: &str) -> String {
    let bytes = package(body);
    let document = open(&bytes).unwrap_or_else(|e| panic!("{body}: {e:?}"));
    stream(&document)
}

/// Every element-level defect a page reported, in order.
fn defects(bytes: &[u8]) -> Vec<XpsElementDefect> {
    let document = open(bytes).expect("an XPS");
    document
        .archive()
        .expect("a synthesised document")
        .warnings()
        .iter()
        .filter_map(|warning| match warning {
            ArchiveWarning::XpsElement { defect, .. } => Some(*defect),
            _ => None,
        })
        .collect()
}

/// The defects one page body produces.
fn body_defects(body: &str) -> Vec<XpsElementDefect> {
    defects(&package(body))
}

/// Every drawing operator in a content stream, whitespace collapsed, so an
/// assertion reads like the stream rather than like its line breaks.
fn ops(stream: &str) -> Vec<String> {
    stream
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

/// A geometry from the abbreviated syntax, with a budget nothing here reaches.
fn geometry(data: &str) -> Result<geometry::Geometry, geometry::GeometryError> {
    let mut budget = Budget::new(1 << 20, 1 << 23, 1 << 21);
    geometry::abbreviated(data, &mut budget)
}

/// Every segment of a geometry, flattened.
fn segments(data: &str) -> Vec<Segment> {
    geometry(data)
        .unwrap_or_else(|e| panic!("{data}: {e:?}"))
        .segments()
        .copied()
        .collect()
}

fn corpus(name: &str) -> Vec<u8> {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/xps")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

// ---- 11.2.3, the abbreviated geometry -----------------------------------

/// **`F M L H V C Q S A Z`, in both cases**, which is 11.2.3's whole command
/// set.
///
/// XPS has no smooth *quadratic* — SVG's `T` has no counterpart here — so ten
/// letters is the list rather than eleven, and an eleventh would be a command
/// this reader invented.
#[test]
fn every_abbreviated_command_is_read_in_both_cases() {
    // Upper case: every point is absolute.
    let upper = segments(
        "F1 M10,10 L20,10 H30 V20 C31,21 32,22 33,23 Q34,24 35,25 S36,26 37,27 A5,5 0 0 1 40,30 Z",
    );
    // Lower case: every point displaces from the pen, and the pen starts each
    // command where the last one left it.
    let lower = segments("F1 m10,10 l10,0 h10 v10 c1,1 2,2 3,3 q1,1 2,2 s1,1 2,2 a5,5 0 0 1 3,3 z");
    assert_eq!(
        upper.len(),
        lower.len(),
        "the same ten commands spelled two ways"
    );
    // The two spellings describe the same shape, so every segment lands in the
    // same place — which is the whole claim "both cases" makes.
    for (at, (one, other)) in upper.iter().zip(lower.iter()).enumerate() {
        assert!(same(one, other), "segment {at}: {one:?} against {other:?}");
    }
    assert_eq!(
        geometry("F1 M0,0Z").expect("a geometry").fill_rule,
        FillRule::NonZero
    );
}

/// Within a tenth of a unit, because an arc's cubics come out of trigonometry
/// and two spellings of one arc reach it by different additions.
fn same(one: &Segment, other: &Segment) -> bool {
    fn close(a: &[f64], b: &[f64]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.1)
    }
    match (one, other) {
        (Segment::Move(a), Segment::Move(b)) | (Segment::Line(a), Segment::Line(b)) => close(a, b),
        (Segment::Curve(a), Segment::Curve(b)) => close(a, b),
        (Segment::Close, Segment::Close) => true,
        _ => false,
    }
}

/// **The first half of the pair.** `L` names a point.
#[test]
fn an_absolute_line_names_a_point() {
    assert_eq!(
        segments("M100,100 L10,10"),
        [Segment::Move([100.0, 100.0]), Segment::Line([10.0, 10.0])],
        "`L10,10` is the point 10,10 and not ten units on"
    );
}

/// **The second half.** `l` names a displacement, from wherever the pen is.
///
/// A reader that treated the two as one grammar passes the test above and
/// fails this one, and vice versa — which is why they are two tests over one
/// fixture shape rather than one test that happens to use upper case.
#[test]
fn a_relative_line_names_a_displacement() {
    assert_eq!(
        segments("M100,100 l10,10"),
        [Segment::Move([100.0, 100.0]), Segment::Line([110.0, 110.0])],
        "`l10,10` is ten units on from 100,100"
    );
}

/// A relative **first** `Move` displaces from `0,0`, which is the only
/// position a figure can start from before one is stated.
#[test]
fn a_relative_first_move_displaces_from_the_origin() {
    assert_eq!(segments("m10,20"), [Segment::Move([10.0, 20.0])]);
    // And it is the *first* move that is special: the second displaces from
    // where the first left the pen.
    assert_eq!(
        segments("m10,20 m5,5"),
        [Segment::Move([10.0, 20.0]), Segment::Move([15.0, 25.0])]
    );
}

/// **`Z` has two consequences and this is the first**: the pen returns to the
/// figure's own first point.
#[test]
fn a_close_returns_the_pen_to_the_figures_first_point() {
    // An absolute line after the close proves nothing about the pen, so the
    // proof is that a *second* figure opened by a drawing command with no `M`
    // starts at the closed figure's first point.
    assert_eq!(
        segments("M10,10 L90,90 Z L50,50"),
        [
            Segment::Move([10.0, 10.0]),
            Segment::Line([90.0, 90.0]),
            Segment::Close,
            Segment::Move([10.0, 10.0]),
            Segment::Line([50.0, 50.0]),
        ],
        "the figure re-opens at 10,10, not at 90,90"
    );
}

/// **And the second consequence**: a *relative* command after `Z` displaces
/// from the previous figure's first point.
///
/// The two are independent. A build that reset the current point and not the
/// figure start would pass the test above; one that reset the figure start and
/// not the current point would pass this one. Gap 30's row 6 names both
/// separately for that reason.
#[test]
fn a_relative_command_after_a_close_starts_from_the_previous_figures_first_point() {
    assert_eq!(
        segments("M10,10 L90,90 Z l5,5"),
        [
            Segment::Move([10.0, 10.0]),
            Segment::Line([90.0, 90.0]),
            Segment::Close,
            Segment::Move([10.0, 10.0]),
            Segment::Line([15.0, 15.0]),
        ],
        "`l5,5` after `Z` is five on from 10,10 and not from 90,90"
    );
}

/// 11.2.3's omitted-repeat form: `L 100,200 300,400` is two lines.
#[test]
fn the_omitted_repeat_form_re_applies_the_command() {
    assert_eq!(
        segments("M0,0 L 100,200 300,400"),
        [
            Segment::Move([0.0, 0.0]),
            Segment::Line([100.0, 200.0]),
            Segment::Line([300.0, 400.0]),
        ]
    );
    // Under a relative command each repeat displaces from the point the
    // previous repeat reached, which is a third thing the case of the letter
    // decides.
    assert_eq!(
        segments("M0,0 l 10,10 10,10"),
        [
            Segment::Move([0.0, 0.0]),
            Segment::Line([10.0, 10.0]),
            Segment::Line([20.0, 20.0]),
        ]
    );
    // And a cubic's repeat takes three points at a time rather than one.
    assert_eq!(
        segments("M0,0 C1,1 2,2 3,3 4,4 5,5 6,6").len(),
        3,
        "a move and two curves"
    );
}

/// **`EvenOdd` is the default**, which is the opposite of PDF's `f`.
///
/// A reader that defaulted to non-zero would fill every self-intersecting
/// shape solid and every other shape correctly, which is a defect no fixture
/// without a self-intersection can see.
#[test]
fn the_fill_rule_defaults_to_even_odd_and_the_operators_differ() {
    assert_eq!(
        geometry("M0,0L1,0Z").expect("a geometry").fill_rule,
        FillRule::EvenOdd
    );
    assert_eq!(
        geometry("F0 M0,0L1,0Z").expect("a geometry").fill_rule,
        FillRule::EvenOdd
    );
    assert!(drawn(r##"<Path Fill="#000000" Data="M0,0L1,0Z" />"##).contains("f*\n"));
    assert!(drawn(r##"<Path Fill="#000000" Data="F1 M0,0L1,0Z" />"##).contains("\nf\n"));
}

/// The elliptical arc, which is the one command with no PDF operator at all.
///
/// Two things are asserted rather than one: that it becomes cubics, and that
/// the **last** one ends exactly where the file said. The second matters
/// because the conversion goes through the centre parameterisation and back,
/// and an arc whose end drifted by a thousandth leaves a gap a `Z` then closes
/// with a line nobody wrote.
#[test]
fn an_elliptical_arc_becomes_cubics_that_end_where_the_file_said() {
    let quarter = segments("M100,0 A100,100 0 0 1 0,100");
    assert_eq!(quarter.len(), 2, "a move and one ninety-degree cubic");
    assert_eq!(
        quarter[1],
        match quarter[1] {
            Segment::Curve(p) => Segment::Curve([p[0], p[1], p[2], p[3], 0.0, 100.0]),
            other => other,
        },
        "the arc ends on the point the file stated"
    );

    // A whole ellipse in one arc is more than ninety degrees, so it is split.
    let long = segments("M100,0 A100,100 0 1 1 0,100");
    assert_eq!(long.len(), 4, "a move and three cubics for 270 degrees");

    // The two flags are independent and both change the shape: the same
    // endpoints and radii with `IsLargeArc` set sweep the other way round.
    let short = segments("M100,0 A100,100 0 0 1 0,100");
    assert_ne!(short.len(), long.len());

    // F.6.2: a zero radius is a line rather than an error, and an arc to where
    // the pen already is draws nothing.
    assert_eq!(
        segments("M10,10 A0,50 0 0 1 20,20"),
        [Segment::Move([10.0, 10.0]), Segment::Line([20.0, 20.0])]
    );
    assert_eq!(
        segments("M10,10 A5,5 0 0 1 10,10"),
        [Segment::Move([10.0, 10.0])]
    );
}

/// A smooth cubic reflects the **last cubic's** second control point, and
/// starts at the pen when the last command was not a cubic.
#[test]
fn a_smooth_cubic_reflects_the_last_cubics_control_point() {
    // After `C` ending at 30,30 with a second control at 20,10, the reflection
    // about 30,30 is 40,50.
    assert_eq!(
        segments("M0,0 C10,0 20,10 30,30 S50,50 60,60"),
        [
            Segment::Move([0.0, 0.0]),
            Segment::Curve([10.0, 0.0, 20.0, 10.0, 30.0, 30.0]),
            Segment::Curve([40.0, 50.0, 50.0, 50.0, 60.0, 60.0]),
        ]
    );
    // After a line there is no cubic to reflect, so the first control point is
    // the current point itself.
    assert_eq!(
        segments("M0,0 L10,10 S20,20 30,30"),
        [
            Segment::Move([0.0, 0.0]),
            Segment::Line([10.0, 10.0]),
            Segment::Curve([10.0, 10.0, 20.0, 20.0, 30.0, 30.0]),
        ]
    );
}

/// A quadratic is raised to the cubic that draws the identical curve.
#[test]
fn a_quadratic_is_raised_to_the_cubic_that_draws_it() {
    // From 0,0 through the control 30,60 to 60,0: the cubic controls sit two
    // thirds of the way from each end to the quadratic's own.
    assert_eq!(
        segments("M0,0 Q30,60 60,0"),
        [
            Segment::Move([0.0, 0.0]),
            Segment::Curve([20.0, 40.0, 40.0, 40.0, 60.0, 0.0]),
        ]
    );
}

/// **Both producers' spellings of one rectangle are one geometry.**
///
/// Milestone 1 measured `M0,0L200,0 200,200 0,200Z` from WPF against
/// `M 0,0 L 200,0 200,200 0,200 Z` from the XPS object model, for the same
/// page of the same document. A reader that took either alone refuses half the
/// corpus.
#[test]
fn the_two_producers_spellings_of_one_rectangle_are_one_geometry() {
    assert_eq!(
        segments("M0,0L200,0 200,200 0,200Z"),
        segments("M 0,0 L 200,0 200,200 0,200 Z")
    );
}

/// A `Data` that is not the grammar is refused rather than half read.
#[test]
fn a_data_that_is_not_the_grammar_is_refused() {
    for data in [
        "M0,0 X10,10",           // a command that is not one
        "M0,0 L",                // a command with no operand
        "M0,0 L10",              // half a point
        "M0,0 A5,5 0 2 1 10,10", // a flag that is not 0 or 1
        "M0,0 L10,10 junk",
        "F2 M0,0", // a fill rule that is not 0 or 1
    ] {
        assert_eq!(
            geometry(data),
            Err(geometry::GeometryError::Syntax),
            "{data:?} is not 11.2.3's geometry"
        );
    }

    // **The empty string is not one of them**, and that is the grammar rather
    // than leniency: 11.2.3 makes the move command and every draw command
    // optional, so `Data=""` is a legal geometry describing no shape. It draws
    // nothing and warns about nothing, because a file that said "no shape" got
    // what it asked for — where `Data="X"` said something this reader could not
    // read and is named.
    assert_eq!(geometry("").expect("a legal empty geometry").figures, []);
    assert_eq!(body_defects(r##"<Path Fill="#000000" Data="" />"##), []);
    assert_eq!(
        body_defects(r##"<Path Fill="#000000" Data="X" />"##),
        [XpsElementDefect::GeometryUnreadable]
    );
    // And `M0,0` alone is one segment and reads.
    assert_eq!(segments("M0,0").len(), 1);
}

// ---- 11.2.1, the element syntax -----------------------------------------

/// **The criterion in row 6**: the two spellings produce the identical segment
/// list.
///
/// Asserted through the content stream rather than through the two parsers'
/// return values, because the content stream is what the segment list *is* —
/// two parsers that agreed on a `Vec` and disagreed about which operator each
/// segment becomes would pass a comparison of the values and produce two
/// pictures, which is gap 29's `/Mask` finding exactly.
#[test]
fn the_element_syntax_and_the_abbreviated_syntax_produce_the_identical_segment_list() {
    let abbreviated = drawn(r##"<Path Fill="#000000" Data="M10,20L110,20 110,120 10,120Z" />"##);
    let element = drawn(
        r##"<Path Fill="#000000"><Path.Data><PathGeometry><PathFigure StartPoint="10,20" IsClosed="true"><PolyLineSegment Points="110,20 110,120 10,120" /></PathFigure></PathGeometry></Path.Data></Path>"##,
    );
    assert_eq!(ops(&abbreviated), ops(&element));
    assert!(
        abbreviated.contains("10 20 m"),
        "and the numbers are the markup's own: {abbreviated}"
    );

    // The same three ways again for a curve and an arc, so the agreement is
    // not an accident of straight lines.
    let curves = drawn(r##"<Path Fill="#000000" Data="M0,0C1,2 3,4 5,6" />"##);
    let curve_elements = drawn(
        r##"<Path Fill="#000000"><Path.Data><PathGeometry><PathGeometry.Figures><PathFigure StartPoint="0,0"><PolyBezierSegment Points="1,2 3,4 5,6" /></PathFigure></PathGeometry.Figures></PathGeometry></Path.Data></Path>"##,
    );
    assert_eq!(ops(&curves), ops(&curve_elements));

    let arc = drawn(r##"<Path Fill="#000000" Data="M100,0A100,100 0 0 1 0,100" />"##);
    let arc_element = drawn(
        r##"<Path Fill="#000000"><Path.Data><PathGeometry><PathFigure StartPoint="100,0"><ArcSegment Point="0,100" Size="100,100" SweepDirection="Clockwise" /></PathFigure></PathGeometry></Path.Data></Path>"##,
    );
    assert_eq!(ops(&arc), ops(&arc_element));
}

/// `PathGeometry` carries the abbreviated syntax in its own `Figures`
/// attribute, which is where the two grammars meet.
#[test]
fn a_path_geometrys_figures_attribute_is_the_abbreviated_syntax() {
    let one = drawn(r##"<Path Fill="#000000" Data="F1 M0,0L10,0Z" />"##);
    let other = drawn(
        r##"<Path Fill="#000000"><Path.Data><PathGeometry Figures="M0,0L10,0Z" FillRule="Nonzero" /></Path.Data></Path>"##,
    );
    assert_eq!(ops(&one), ops(&other));
}

/// 11.2.2's `IsFilled="false"` takes a figure out of the fill and leaves it in
/// the stroke.
#[test]
fn a_figure_that_is_not_filled_is_still_stroked() {
    let content = drawn(
        r##"<Path Fill="#000000" Stroke="#FF0000" StrokeThickness="2"><Path.Data><PathGeometry><PathFigure StartPoint="0,0" IsClosed="true"><PolyLineSegment Points="10,0 10,10" /></PathFigure><PathFigure StartPoint="50,50" IsFilled="false" IsClosed="true"><PolyLineSegment Points="60,50 60,60" /></PathFigure></PathGeometry></Path.Data></Path>"##,
    );
    let lines = ops(&content);
    let fill_at = lines.iter().position(|l| l == "f*").expect("a fill");
    let stroke_at = lines.iter().position(|l| l == "S").expect("a stroke");
    let filled: Vec<&String> = lines[..fill_at].iter().collect();
    let stroked: Vec<&String> = lines[fill_at + 1..stroke_at].iter().collect();
    assert!(
        !filled.iter().any(|l| l.as_str() == "50 50 m"),
        "the unfilled figure is not in the fill: {filled:?}"
    );
    assert!(
        stroked.iter().any(|l| l.as_str() == "50 50 m"),
        "and it is in the stroke: {stroked:?}"
    );
}

// ---- 14.4, transforms ---------------------------------------------------

/// A `RenderTransform` reaches the page as the **same six numbers**, in the
/// same order, because 14.4.1's order is PDF's `cm` order.
#[test]
fn a_render_transform_reaches_the_page_as_the_same_six_numbers() {
    let content =
        drawn(r##"<Path Fill="#000000" RenderTransform="2,0,0,3,40,50" Data="M0,0L1,0Z" />"##);
    assert!(content.contains("2 0 0 3 40 50 cm"), "{content}");
}

/// The attribute form and `MatrixTransform` are one value.
#[test]
fn a_matrix_transform_element_and_the_attribute_form_are_one_value() {
    let attribute =
        drawn(r##"<Path Fill="#000000" RenderTransform="2,0,0,3,40,50" Data="M0,0L1,0Z" />"##);
    let element = drawn(
        r##"<Path Fill="#000000" Data="M0,0L1,0Z"><Path.RenderTransform><MatrixTransform Matrix="2,0,0,3,40,50" /></Path.RenderTransform></Path>"##,
    );
    assert_eq!(ops(&attribute), ops(&element));
}

/// A canvas's transform composes with its child's rather than replacing it.
#[test]
fn a_canvas_transform_composes_with_its_childs() {
    let content = drawn(
        r##"<Canvas RenderTransform="1,0,0,1,100,100"><Path Fill="#000000" RenderTransform="2,0,0,2,0,0" Data="M0,0L1,0Z" /></Canvas>"##,
    );
    let lines = ops(&content);
    assert!(
        lines.contains(&"1 0 0 1 100 100 cm".to_string()),
        "the canvas's own: {lines:?}"
    );
    assert!(
        lines.contains(&"2 0 0 2 0 0 cm".to_string()),
        "and the path's, inside it: {lines:?}"
    );
    // Composed by nesting `q`/`Q` rather than by multiplying, so the numbers
    // in the file are the numbers in the stream.
    assert!(
        content.starts_with("q 0.75 0 0 -0.75 0 792 cm"),
        "{content}"
    );
}

/// **A transform that is not six numbers refuses its element**, and everything
/// under it.
///
/// The identity is the plausible wrong answer here: it draws the right content
/// in the wrong place at the right size, which reads as a layout bug in the
/// producer rather than as a reader that could not parse an attribute.
#[test]
fn a_transform_that_is_not_six_numbers_refuses_its_element() {
    let body = r##"<Path Fill="#000000" RenderTransform="1,0,0,1,100" Data="M0,0L10,0Z" />"##;
    assert_eq!(body_defects(body), [XpsElementDefect::TransformUnreadable]);
    assert!(
        !drawn(body).contains(" m\n"),
        "nothing is painted: {}",
        drawn(body)
    );

    // And on a canvas it takes the subtree with it: the child is a shape this
    // reader could otherwise draw perfectly.
    let canvas = r##"<Canvas RenderTransform="not a matrix"><Path Fill="#000000" Data="M0,0L10,0Z" /></Canvas>"##;
    assert_eq!(
        body_defects(canvas),
        [XpsElementDefect::TransformUnreadable],
        "one warning for the canvas and none for what it refused"
    );
    assert!(!drawn(canvas).contains("0 0 m"), "{}", drawn(canvas));
}

// ---- 14.3, canvases, clips and opacity ----------------------------------

/// **The first half of the opacity pair**: children that overlap become a
/// transparency group.
///
/// 14.3 composites a canvas's rendering as a whole, and "as a whole" is only
/// observable where two children cover the same place — one alpha applied to
/// each of two shapes that overlap paints the overlap twice.
#[test]
fn a_canvas_opacity_over_overlapping_children_is_a_transparency_group() {
    let bytes = package(
        r##"<Canvas Opacity="0.5"><Path Fill="#FF0000" Data="M0,0L100,0 100,100 0,100Z" /><Path Fill="#0000FF" Data="M50,50L150,50 150,150 50,150Z" /></Canvas>"##,
    );
    let document = open(&bytes).expect("an XPS");
    let content = stream(&document);
    assert!(content.contains(" Do\n"), "a form is drawn: {content}");

    // And the form really is a transparency group, read out of the object
    // model rather than out of the operator that named it.
    let cos = document.cos();
    let raw = String::from_utf8_lossy(cos.bytes()).into_owned();
    assert!(raw.contains("/Transparency"), "the form carries a /Group");
    assert!(raw.contains("/ca 0.5"), "and the alpha is on the `Do`");
}

/// **The second half.** Children that do not overlap need no group at all.
///
/// A suite that only ever drew overlapping children would pass with this
/// branch deleted, and one that only ever drew separated children would pass
/// with the group deleted. Two rules, two fixtures — which is gap 30's own
/// standing lesson.
#[test]
fn a_canvas_opacity_over_children_that_do_not_overlap_is_not_a_group() {
    let content = drawn(
        r##"<Canvas Opacity="0.5"><Path Fill="#FF0000" Data="M0,0L100,0 100,100 0,100Z" /><Path Fill="#0000FF" Data="M200,200L300,200 300,300 200,300Z" /></Canvas>"##,
    );
    assert!(
        !content.contains(" Do\n"),
        "no form XObject is needed: {content}"
    );
    assert!(content.contains(" gs\n"), "the alpha still applies");
}

/// `Canvas.Clip` and `Path.Clip` both reach the page, in both spellings.
#[test]
fn a_canvas_clip_and_a_path_clip_both_reach_the_page() {
    let path = drawn(
        r##"<Path Fill="#000000" Clip="M0,0L50,0 50,50 0,50Z" Data="M0,0L100,0 100,100 0,100Z" />"##,
    );
    assert!(path.contains("W* n"), "{path}");

    let canvas = drawn(
        r##"<Canvas Clip="M0,0L50,0 50,50 0,50Z"><Path Fill="#000000" Data="M0,0L100,0 100,100 0,100Z" /></Canvas>"##,
    );
    assert!(canvas.contains("W* n"), "{canvas}");

    // And the element spelling of the same value.
    let element = drawn(
        r##"<Canvas><Canvas.Clip><PathGeometry Figures="M0,0L50,0 50,50 0,50Z" /></Canvas.Clip><Path Fill="#000000" Data="M0,0L100,0 100,100 0,100Z" /></Canvas>"##,
    );
    assert_eq!(ops(&canvas), ops(&element));
}

/// **One parser, two consequences.** The same syntax error is *not painted* in
/// a `Data` and *refuses the element* in a `Clip`.
///
/// This is the pair the design section decides before any file exists, and it
/// is the one an implementation is most likely to collapse: both are
/// `geometry::abbreviated` returning `Syntax`, and a build that answered them
/// the same way would pass whichever half its fixtures happened to cover.
#[test]
fn a_data_that_will_not_read_is_not_painted_and_a_clip_that_will_not_read_refuses() {
    let bad_data = r##"<Path Fill="#000000" Data="X" />"##;
    assert_eq!(
        body_defects(bad_data),
        [XpsElementDefect::GeometryUnreadable]
    );

    let bad_clip = r##"<Path Fill="#000000" Clip="X" Data="M0,0L10,0Z" />"##;
    assert_eq!(body_defects(bad_clip), [XpsElementDefect::ClipUnreadable]);

    // Both draw nothing, and they draw nothing for different reasons — which
    // is the whole point of the names being different.
    for body in [bad_data, bad_clip] {
        assert!(!drawn(body).contains(" m\n"), "{body}: {}", drawn(body));
    }
}

/// An `Opacity` that is not a number in `[0, 1]` refuses its element.
#[test]
fn an_opacity_that_is_not_a_number_refuses_its_element() {
    for value in ["half", "2", "-0.5", ""] {
        let body = format!(r##"<Path Fill="#000000" Opacity="{value}" Data="M0,0L10,0Z" />"##);
        assert_eq!(
            body_defects(&body),
            [XpsElementDefect::OpacityUnreadable],
            "Opacity={value:?}"
        );
    }
    // And a good one paints, with the alpha in an `/ExtGState` rather than in
    // the colour — 11.6.4.4's `/ca` is a graphics-state parameter and a writer
    // that folded it into the fill colour would produce a lighter shape rather
    // than a transparent one.
    let content = drawn(r##"<Path Fill="#000000" Opacity="0.25" Data="M0,0L10,0Z" />"##);
    assert!(content.contains(" gs\n"), "{content}");
}

/// An `OpacityMask` is refused rather than ignored (gap 30 milestone 8 owns
/// the brush that would apply it).
///
/// Ignoring a mask draws a whole shape where a sliver was meant, which is the
/// opacity case exactly.
#[test]
fn an_opacity_mask_refuses_its_element_rather_than_being_ignored() {
    let body = r##"<Path Fill="#000000" Data="M0,0L10,0Z"><Path.OpacityMask><SolidColorBrush Color="#80000000" /></Path.OpacityMask></Path>"##;
    assert_eq!(
        body_defects(body),
        [XpsElementDefect::OpacityMaskUnsupported]
    );
    assert!(!drawn(body).contains("0 0 m"), "{}", drawn(body));
}

// ---- 15, brushes --------------------------------------------------------

/// **Four hex lengths and two cases**, because one corpus carries two of them
/// and 15.2.4 defines four.
#[test]
fn a_colour_comes_in_four_hex_lengths_and_two_cases() {
    // `#FFDC143C` from WPF and `#dc143c` from the object model are the same
    // crimson, and milestone 1 measured both on the same page of one document.
    let long = drawn(r##"<Path Fill="#FFDC143C" Data="M0,0L1,0Z" />"##);
    let short = drawn(r##"<Path Fill="#dc143c" Data="M0,0L1,0Z" />"##);
    assert_eq!(ops(&long), ops(&short));
    assert!(long.contains("0.8627 0.0784 0.2353 rg"), "{long}");

    // `#RGB` and `#ARGB` expand each digit by seventeen, so `#F00` is `#FF0000`
    // and not `#F00000`.
    assert_eq!(
        ops(&drawn(r##"<Path Fill="#F00" Data="M0,0L1,0Z" />"##)),
        ops(&drawn(r##"<Path Fill="#FF0000" Data="M0,0L1,0Z" />"##))
    );
    assert_eq!(
        ops(&drawn(r##"<Path Fill="#8F00" Data="M0,0L1,0Z" />"##)),
        ops(&drawn(r##"<Path Fill="#88FF0000" Data="M0,0L1,0Z" />"##))
    );

    // And the alpha of `#AARRGGBB` is an `/ExtGState` rather than a lighter
    // colour.
    let translucent = drawn(r##"<Path Fill="#80FF0000" Data="M0,0L1,0Z" />"##);
    assert!(translucent.contains(" gs\n"), "{translucent}");
    assert!(translucent.contains("1 0 0 rg"), "{translucent}");
}

/// `sc#` is **scRGB**, which is linear light — so it goes through the sRGB
/// transfer function rather than being written out as it stands.
///
/// A reader that wrote the components straight into `rg` would produce a
/// picture that is plausible, systematically too dark, and correct at zero and
/// one — which is the one place a fixture would look.
#[test]
fn an_sc_rgb_colour_goes_through_the_srgb_transfer_function() {
    // Linear 0.5 is sRGB 0.7354, not 0.5.
    let content = drawn(r##"<Path Fill="sc#0.5,0.5,0.5" Data="M0,0L1,0Z" />"##);
    assert!(content.contains("0.7354 0.7354 0.7354 rg"), "{content}");
    // The endpoints are where a wrong transfer function still agrees, which is
    // why they are asserted second rather than first.
    assert!(drawn(r##"<Path Fill="sc#1,1,1" Data="M0,0L1,0Z" />"##).contains("1 1 1 rg"));
    assert!(drawn(r##"<Path Fill="sc#0,0,0" Data="M0,0L1,0Z" />"##).contains("0 0 0 rg"));
    // The four-component form's first number is an alpha and is **not**
    // transferred: it is a coverage and not a light level.
    let alpha = drawn(r##"<Path Fill="sc#0.5,1,1,1" Data="M0,0L1,0Z" />"##);
    assert!(alpha.contains("1 1 1 rg"), "{alpha}");
    assert!(alpha.contains(" gs\n"), "{alpha}");
}

/// The element form of a solid brush is the attribute form.
#[test]
fn a_solid_colour_brush_element_and_the_attribute_form_are_one_value() {
    let attribute = drawn(r##"<Path Fill="#FF0000" Data="M0,0L1,0Z" />"##);
    let element = drawn(
        r##"<Path Data="M0,0L1,0Z"><Path.Fill><SolidColorBrush Color="#FF0000" /></Path.Fill></Path>"##,
    );
    assert_eq!(ops(&attribute), ops(&element));
}

/// A linear gradient is a **type 2** shading and a radial is a **type 3**.
///
/// Read out of the object model rather than out of the operator, because `sh`
/// names a resource and says nothing about what kind it is — and 8.7.4.5.4's
/// radial blends between two *circles*, so a writer sharing one path with type
/// 2 would have nowhere to put the radii.
#[test]
fn a_linear_gradient_is_axial_and_a_radial_is_radial() {
    let bytes = package(
        r##"<Path Data="M0,0L400,0 400,200 0,200Z"><Path.Fill><LinearGradientBrush StartPoint="0,0" EndPoint="400,200"><LinearGradientBrush.GradientStops><GradientStop Color="#FF0000" Offset="0" /><GradientStop Color="#00FF00" Offset="0.5" /><GradientStop Color="#0000FF" Offset="1" /></LinearGradientBrush.GradientStops></LinearGradientBrush></Path.Fill></Path>"##,
    );
    let document = open(&bytes).expect("an XPS");
    let raw = String::from_utf8_lossy(document.cos().bytes()).into_owned();
    assert!(raw.contains("/ShadingType 2"), "an axial shading");
    assert!(raw.contains("/Coords [0 0 400 200]"), "{raw}");
    // Three stops are a stitching function over two ramps, and the bound is
    // the middle stop's own offset.
    assert!(raw.contains("/FunctionType 3"), "{raw}");
    assert!(raw.contains("/Bounds [0.5]"), "{raw}");
    assert!(stream(&document).contains(" sh\n"));

    let bytes = package(
        r##"<Path Data="M0,0L300,0 300,300 0,300Z"><Path.Fill><RadialGradientBrush Center="150,150" RadiusX="150" RadiusY="150" GradientOrigin="120,120"><RadialGradientBrush.GradientStops><GradientStop Color="#FFFFFF" Offset="0" /><GradientStop Color="#191970" Offset="1" /></RadialGradientBrush.GradientStops></RadialGradientBrush></Path.Fill></Path>"##,
    );
    let document = open(&bytes).expect("an XPS");
    let raw = String::from_utf8_lossy(document.cos().bytes()).into_owned();
    assert!(raw.contains("/ShadingType 3"), "a radial shading");
    // The focal circle has radius zero and the outer one is the stated radius.
    assert!(raw.contains("/Coords [120 120 0 150 150 150]"), "{raw}");
    // Two stops are one exponential ramp, not a stitch.
    assert!(raw.contains("/FunctionType 2"), "{raw}");
}

/// **`Pad`, `Reflect` and `Repeat` are three behaviours**, and a test for one
/// is not a test for the others.
///
/// PDF 8.7.4.5.3 has `/Extend` and no repeat, so the two repeating methods are
/// built by replicating the function along an extended axis, and they differ
/// from each other in nothing but which copies run backwards. Three fixtures,
/// three distinct files.
#[test]
fn the_three_spread_methods_are_three_shadings() {
    let gradient = |spread: &str| {
        let bytes = package(&format!(
            r##"<Path Data="M0,0L100,0 100,100 0,100Z"><Path.Fill><LinearGradientBrush StartPoint="0,0" EndPoint="100,0" SpreadMethod="{spread}"><LinearGradientBrush.GradientStops><GradientStop Color="#FF0000" Offset="0" /><GradientStop Color="#0000FF" Offset="1" /></LinearGradientBrush.GradientStops></LinearGradientBrush></Path.Fill></Path>"##
        ));
        let document = open(&bytes).expect("an XPS");
        String::from_utf8_lossy(document.cos().bytes()).into_owned()
    };

    let pad = gradient("Pad");
    let repeat = gradient("Repeat");
    let reflect = gradient("Reflect");

    // Pad is the axis the file stated and one ramp.
    assert!(pad.contains("/Coords [0 0 100 0]"), "{pad}");
    assert!(pad.contains("/FunctionType 2"), "{pad}");

    // Repeat and Reflect extend the axis eight periods each way, which is the
    // same seventeen-copy geometry for both.
    assert!(repeat.contains("/Coords [-800 0 900 0]"), "{repeat}");
    assert!(reflect.contains("/Coords [-800 0 900 0]"), "{reflect}");

    // And they differ in the one thing that makes them two methods: every
    // other copy of a reflected gradient runs backwards, which is an `/Encode`
    // pair of `1 0`.
    assert!(
        !repeat.contains("1 0 0 1 0 1"),
        "a repeated gradient never reverses a copy"
    );
    assert!(
        reflect.contains("1 0 0 1"),
        "a reflected one reverses every other copy: {reflect}"
    );
    assert_ne!(repeat, reflect, "three methods, three files");
    assert_ne!(pad, repeat);

    // A method that is not one of the three is a brush that will not read.
    let bad = package(
        r##"<Path Data="M0,0L1,0Z"><Path.Fill><LinearGradientBrush StartPoint="0,0" EndPoint="1,0" SpreadMethod="Wrap"><LinearGradientBrush.GradientStops><GradientStop Color="#FF0000" Offset="0" /></LinearGradientBrush.GradientStops></LinearGradientBrush></Path.Fill></Path>"##,
    );
    assert_eq!(defects(&bad), [XpsElementDefect::BrushUnreadable]);
}

/// **A gradient's stops are read by offset and not by document order.**
///
/// 15.4.2 orders a gradient by its stops' `Offset` values and says nothing
/// about the order they are written in, so a producer may write them any way
/// round. This survived the injection matrix: every fixture in the corpus and
/// in this file writes them ascending, so a build that took document order
/// passed all of them.
///
/// What it would produce is worse than a refusal. 7.10.4 wants `/Bounds`
/// strictly increasing, and this reader repairs a non-increasing offset by
/// nudging it — a repair that is right for 15.4.2's *hard stop*, two stops at
/// one offset, and that silently flattens a gradient whose stops arrived out
/// of order into a ramp between the wrong two colours.
#[test]
fn a_gradients_stops_are_read_by_offset_and_not_by_document_order() {
    let shading = |stops: &str| {
        let bytes = package(&format!(
            r##"<Path Data="M0,0L100,0 100,100 0,100Z"><Path.Fill><LinearGradientBrush StartPoint="0,0" EndPoint="100,0"><LinearGradientBrush.GradientStops>{stops}</LinearGradientBrush.GradientStops></LinearGradientBrush></Path.Fill></Path>"##
        ));
        let document = open(&bytes).expect("an XPS");
        String::from_utf8_lossy(document.cos().bytes()).into_owned()
    };

    let ascending = shading(concat!(
        r##"<GradientStop Color="#FF0000" Offset="0" />"##,
        r##"<GradientStop Color="#00FF00" Offset="0.25" />"##,
        r##"<GradientStop Color="#0000FF" Offset="1" />"##,
    ));
    let shuffled = shading(concat!(
        r##"<GradientStop Color="#0000FF" Offset="1" />"##,
        r##"<GradientStop Color="#FF0000" Offset="0" />"##,
        r##"<GradientStop Color="#00FF00" Offset="0.25" />"##,
    ));
    assert_eq!(ascending, shuffled, "one gradient, two spellings");
    assert!(ascending.contains("/Bounds [0.25]"), "{ascending}");
    assert!(ascending.contains("/C0 [1 0 0]"), "{ascending}");
}

/// A gradient's own `Transform` reaches the shading's matrix.
#[test]
fn a_gradient_transform_reaches_the_shading() {
    let content = drawn(
        r##"<Path Data="M0,0L100,0 100,100 0,100Z"><Path.Fill><LinearGradientBrush StartPoint="0,0" EndPoint="100,0" Transform="2,0,0,2,10,20"><LinearGradientBrush.GradientStops><GradientStop Color="#FF0000" Offset="0" /><GradientStop Color="#0000FF" Offset="1" /></LinearGradientBrush.GradientStops></LinearGradientBrush></Path.Fill></Path>"##,
    );
    assert!(content.contains("2 0 0 2 10 20 cm"), "{content}");
    assert!(content.contains(" sh\n"), "{content}");
}

/// An elliptical radial gradient is a circle in a scaled space, because a PDF
/// type 3 shading blends between circles and has nowhere to put two radii.
#[test]
fn an_elliptical_radial_gradient_is_a_circle_in_a_scaled_space() {
    let bytes = package(
        r##"<Path Data="M0,0L200,0 200,100 0,100Z"><Path.Fill><RadialGradientBrush Center="100,50" RadiusX="100" RadiusY="50"><RadialGradientBrush.GradientStops><GradientStop Color="#FFFFFF" Offset="0" /><GradientStop Color="#000000" Offset="1" /></RadialGradientBrush.GradientStops></RadialGradientBrush></Path.Fill></Path>"##,
    );
    let document = open(&bytes).expect("an XPS");
    let raw = String::from_utf8_lossy(document.cos().bytes()).into_owned();
    assert!(
        raw.contains("/Coords [100 50 0 100 50 100]"),
        "a circle of the horizontal radius: {raw}"
    );
    // And the space is squashed by the ratio of the two radii, about the
    // centre.
    assert!(
        stream(&document).contains("1 0 0 0.5 0 25 cm"),
        "{}",
        stream(&document)
    );
}

/// `MappingMode="RelativeToBoundingBox"` states a gradient in fractions of the
/// shape it fills.
///
/// No package in milestone 1's corpus uses it — both producers write
/// `Absolute` — so this is a hand-built claim about a path no measured file
/// exercises, and it is named as one in the milestone's progress note. What it
/// can still assert is the arithmetic: a unit axis over a shape from 20,30 to
/// 120,80 is the same shading as an absolute axis from 20,30 to 120,30.
#[test]
fn a_relative_gradient_is_stated_in_fractions_of_the_shape_it_fills() {
    let content = drawn(
        r##"<Path Data="M20,30L120,30 120,80 20,80Z"><Path.Fill><LinearGradientBrush StartPoint="0,0" EndPoint="1,0" MappingMode="RelativeToBoundingBox"><LinearGradientBrush.GradientStops><GradientStop Color="#FF0000" Offset="0" /><GradientStop Color="#0000FF" Offset="1" /></LinearGradientBrush.GradientStops></LinearGradientBrush></Path.Fill></Path>"##,
    );
    // The box is 100 x 50 at 20,30, so the unit square maps through
    // `100 0 0 50 20 30`.
    assert!(content.contains("100 0 0 50 20 30 cm"), "{content}");

    // A gradient stated relatively over a shape with no area has nothing to be
    // a fraction of, and is a brush that will not read rather than a gradient
    // over a box this reader invented.
    let degenerate = r##"<Path Data="M20,30L120,30"><Path.Fill><LinearGradientBrush StartPoint="0,0" EndPoint="1,0" MappingMode="RelativeToBoundingBox"><LinearGradientBrush.GradientStops><GradientStop Color="#FF0000" Offset="0" /></LinearGradientBrush.GradientStops></LinearGradientBrush></Path.Fill></Path>"##;
    assert_eq!(
        body_defects(degenerate),
        [XpsElementDefect::BrushUnreadable]
    );
}

/// **A `ContextColor` is not painted black.**
///
/// 15.2.5 carries a profile part URI and channel floats and **no sRGB
/// fallback**, so there is no cheap approximation available. Gap 07's headline
/// defect was a gradient-stroked rule painting solid black silently, and black
/// is a plausible colour where the placeholder grey is not.
#[test]
fn a_context_colour_is_the_placeholder_grey_and_not_black() {
    let body =
        r##"<Path Fill="ContextColor /Resources/p.icc 1.0,0.1,0.2,0.3,0.4" Data="M0,0L10,0Z" />"##;
    assert_eq!(body_defects(body), [XpsElementDefect::BrushUnsupported]);
    let content = drawn(body);
    assert!(content.contains("0.749 0.749 0.749 rg"), "{content}");
    assert!(!content.contains("0 0 0 rg"), "{content}");
}

/// A colour that is not 15.2.4's syntax is grey and named, and the shape is
/// still there.
#[test]
fn a_fill_that_will_not_read_is_painted_grey_and_the_shape_survives() {
    let body = r##"<Path Fill="#ZZZ" Data="M0,0L10,0 10,10Z" />"##;
    assert_eq!(body_defects(body), [XpsElementDefect::BrushUnreadable]);
    let content = drawn(body);
    assert!(content.contains("0.749 0.749 0.749 rg"), "{content}");
    assert!(content.contains("0 0 m"), "the shape is drawn: {content}");
    assert!(content.contains("10 10 l"), "{content}");
}

/// An `ImageBrush` whose picture is not in the package is grey and named, and
/// the name says **which** thing was missing.
///
/// *Amended, milestone 8.* This used to assert `BrushUnsupported` — "an
/// `ImageBrush` is a brush this build does not paint" — and that was the truth
/// for two milestones. Now the brush is painted and this fixture's
/// `ImageSource` names a part no package here holds, so the answer is
/// `ImageUnresolved`, which is a different sentence about a different fault:
/// one says the reader cannot draw this kind of thing, the other says the file
/// pointed at nothing. A reader of the report needs to be able to tell them
/// apart, which is why they are separate variants rather than one.
///
/// The grey and the shape are unchanged, and that is the half of the old
/// assertion that was never about the milestone.
#[test]
fn an_image_brush_whose_picture_is_missing_is_grey_and_named() {
    let body = r##"<Path Data="M0,0L10,0Z"><Path.Fill><ImageBrush ImageSource="/Resources/x.png" Viewbox="0,0,1,1" Viewport="0,0,1,1" /></Path.Fill></Path>"##;
    assert_eq!(body_defects(body), [XpsElementDefect::ImageUnresolved]);
    assert!(drawn(body).contains("0.749 0.749 0.749 rg"));
}

/// A gradient asked to **stroke** is grey, not the gradient's first colour.
///
/// Gap 07's defect said the other way round: a shading pattern is what a
/// gradient stroke needs, milestone 5 deliberately writes none, and taking the
/// first stop would produce a plausible solid rule where a gradient was asked
/// for.
#[test]
fn a_gradient_stroke_is_grey_rather_than_its_first_colour() {
    let body = r##"<Path Data="M0,0L10,0" StrokeThickness="4"><Path.Stroke><LinearGradientBrush StartPoint="0,0" EndPoint="10,0"><LinearGradientBrush.GradientStops><GradientStop Color="#FF0000" Offset="0" /><GradientStop Color="#0000FF" Offset="1" /></LinearGradientBrush.GradientStops></LinearGradientBrush></Path.Stroke></Path>"##;
    assert_eq!(body_defects(body), [XpsElementDefect::BrushUnsupported]);
    let content = drawn(body);
    assert!(content.contains("0.749 G"), "{content}");
    assert!(!content.contains("1 0 0 RG"), "{content}");
    assert!(content.contains("4 w"), "the width is still the file's");
}

/// **A fill's alpha and a stroke's alpha are two numbers**, and one element may
/// state both.
///
/// 11.6.4.4 gives `/ca` and `/CA` as separate graphics-state parameters and
/// milestone 5 built the writer's API around that distinction. This is where
/// the *reader* has to keep it: an element carries one `Opacity` and **two
/// brushes**, and each brush has an alpha of its own. It survived the injection
/// matrix, because every fixture before it either filled or stroked and never
/// did both at once with two different alphas — which is milestone 5's own
/// `/ca`-and-`/CA` finding arriving one layer up.
#[test]
fn a_fills_alpha_and_a_strokes_alpha_are_two_numbers() {
    let bytes = package(
        r##"<Path Data="M0,0L100,0 100,100Z" Fill="#40FF0000" Stroke="#C00000FF" StrokeThickness="3" />"##,
    );
    let document = open(&bytes).expect("an XPS");
    let raw = String::from_utf8_lossy(document.cos().bytes()).into_owned();
    // 0x40 is 64/255 and 0xC0 is 192/255.
    assert!(raw.contains("/ca 0.2509"), "the fill's own alpha: {raw}");
    assert!(raw.contains("/CA 0.7529"), "the stroke's own alpha: {raw}");

    // And an element `Opacity` multiplies **both**, which is 14.3's rule and
    // not a third alpha.
    let bytes = package(
        r##"<Path Data="M0,0L100,0 100,100Z" Opacity="0.5" Fill="#40FF0000" Stroke="#C00000FF" StrokeThickness="3" />"##,
    );
    let document = open(&bytes).expect("an XPS");
    let raw = String::from_utf8_lossy(document.cos().bytes()).into_owned();
    assert!(raw.contains("/ca 0.1254"), "{raw}");
    assert!(raw.contains("/CA 0.3764"), "{raw}");
}

/// Gradient stops whose alphas differ from each other reach the page
/// approximately, and say so.
#[test]
fn gradient_stops_with_different_alphas_are_named_as_approximate() {
    let body = r##"<Path Data="M0,0L10,0 10,10Z"><Path.Fill><LinearGradientBrush StartPoint="0,0" EndPoint="10,0"><LinearGradientBrush.GradientStops><GradientStop Color="#FFFF0000" Offset="0" /><GradientStop Color="#000000FF" Offset="1" /></LinearGradientBrush.GradientStops></LinearGradientBrush></Path.Fill></Path>"##;
    assert_eq!(body_defects(body), [XpsElementDefect::BrushApproximated]);
    // And it is still painted: an approximation is not a refusal.
    assert!(drawn(body).contains(" sh\n"));
}

// ---- 14.2, resource dictionaries ----------------------------------------

/// A `{StaticResource}` resolves out of `FixedPage.Resources`, which is how
/// every image and every gradient in the corpus is named.
#[test]
fn a_static_resource_resolves_from_the_fixed_pages_dictionary() {
    let content = drawn(
        r##"<FixedPage.Resources><ResourceDictionary><SolidColorBrush x:Key="b0" Color="#FF0000" /></ResourceDictionary></FixedPage.Resources><Path Fill="{StaticResource b0}" Data="M0,0L10,0Z" />"##,
    );
    assert!(content.contains("1 0 0 rg"), "{content}");
}

/// **The lookup half of 14.2.5**: an element inside a canvas finds a key the
/// page declared.
#[test]
fn a_canvas_child_finds_a_key_the_page_declared() {
    let content = drawn(
        r##"<FixedPage.Resources><ResourceDictionary><SolidColorBrush x:Key="b0" Color="#FF0000" /></ResourceDictionary></FixedPage.Resources><Canvas><Canvas.Resources><ResourceDictionary><SolidColorBrush x:Key="other" Color="#00FF00" /></ResourceDictionary></Canvas.Resources><Path Fill="{StaticResource b0}" Data="M0,0L10,0Z" /></Canvas>"##,
    );
    assert!(
        content.contains("1 0 0 rg"),
        "the outer key is still visible: {content}"
    );
}

/// **The shadowing half.** An inner dictionary's key hides an outer one's.
///
/// The two halves are one clause and two behaviours: a reader that searched
/// only the innermost dictionary passes this and fails the test above, and one
/// that searched only the outermost passes that one and fails this. That is
/// milestone 3's `PartName` finding in a second place — a positive assertion
/// cannot catch a weakened check.
#[test]
fn an_inner_dictionary_shadows_an_outer_one() {
    let content = drawn(
        r##"<FixedPage.Resources><ResourceDictionary><SolidColorBrush x:Key="b0" Color="#FF0000" /></ResourceDictionary></FixedPage.Resources><Canvas><Canvas.Resources><ResourceDictionary><SolidColorBrush x:Key="b0" Color="#00FF00" /></ResourceDictionary></Canvas.Resources><Path Fill="{StaticResource b0}" Data="M0,0L10,0Z" /></Canvas>"##,
    );
    assert!(content.contains("0 1 0 rg"), "the inner wins: {content}");
    assert!(!content.contains("1 0 0 rg"), "{content}");

    // And the binding is unbound when the canvas closes, so the same key past
    // it is the page's again.
    let after = drawn(
        r##"<FixedPage.Resources><ResourceDictionary><SolidColorBrush x:Key="b0" Color="#FF0000" /></ResourceDictionary></FixedPage.Resources><Canvas><Canvas.Resources><ResourceDictionary><SolidColorBrush x:Key="b0" Color="#00FF00" /></ResourceDictionary></Canvas.Resources></Canvas><Path Fill="{StaticResource b0}" Data="M0,0L10,0Z" />"##,
    );
    assert!(after.contains("1 0 0 rg"), "the outer is back: {after}");
}

/// A `{StaticResource}` naming nothing is grey and named — never black, and
/// never the first brush in the dictionary.
#[test]
fn a_static_resource_naming_nothing_is_grey_and_named() {
    let body = r##"<FixedPage.Resources><ResourceDictionary><SolidColorBrush x:Key="b0" Color="#FF0000" /></ResourceDictionary></FixedPage.Resources><Path Fill="{StaticResource nothing}" Data="M0,0L10,0Z" />"##;
    assert_eq!(body_defects(body), [XpsElementDefect::BrushUnresolved]);
    let content = drawn(body);
    assert!(content.contains("0.749 0.749 0.749 rg"), "{content}");
    assert!(!content.contains("1 0 0 rg"), "{content}");
}

/// **A part shown on a thousand pages is painted once**, and the element total
/// is the observable that says so.
///
/// Milestone 4 recorded the rule this closes: *"a cache with no observable is a
/// cache a test cannot tell from its own absence"*. The painting cache has one
/// without needing a counter, because `MAX_XPS_ELEMENTS` is a **document
/// total** — a part of two thousand elements shown a thousand times charges two
/// thousand if it is painted once and two million if it is not, and two million
/// is past the cap. So the package opens with the cache and is refused as
/// `TooLarge` without it.
///
/// 12.3.1 permits exactly this: a `FixedDocument` may name one page part as
/// many times as it likes, and each naming is a page.
#[test]
fn a_part_shown_on_a_thousand_pages_is_painted_once() {
    let mut markup = String::with_capacity(16_384);
    markup.push_str(&format!(
        r#"<FixedPage xmlns="{XPS_NS}" Width="816" Height="1056">"#
    ));
    for _ in 0..2_000 {
        markup.push_str("<A/>");
    }
    markup.push_str("</FixedPage>");

    let references: Vec<&str> = vec!["Pages/1.fpage"; 1_000];
    let parts = with(
        with(
            one_page_package(),
            "Documents/1/FixedDocument.fdoc",
            &document(&references),
        ),
        "Documents/1/Pages/1.fpage",
        &markup,
    );
    let bytes = archive(parts);
    let opened = open(&bytes).expect("one part painted once fits under the element total");
    assert_eq!(opened.page_count(), 1_000, "every reference is a page");
}

/// **A cycle is refused rather than recursed**, and it is not reported as a
/// depth.
#[test]
fn a_static_resource_cycle_is_refused_rather_than_recursed() {
    let body = r##"<FixedPage.Resources><ResourceDictionary><SolidColorBrush x:Key="a" Color="{StaticResource b}" /><SolidColorBrush x:Key="b" Color="{StaticResource a}" /></ResourceDictionary></FixedPage.Resources><Path Fill="{StaticResource a}" Data="M0,0L10,0Z" />"##;
    assert_eq!(body_defects(body), [XpsElementDefect::BrushCyclic]);
    assert!(drawn(body).contains("0.749 0.749 0.749 rg"));

    // A one-element cycle is the same rule and the shortest spelling of it.
    let self_reference = r##"<FixedPage.Resources><ResourceDictionary><SolidColorBrush x:Key="a" Color="{StaticResource a}" /></ResourceDictionary></FixedPage.Resources><Path Fill="{StaticResource a}" Data="M0,0L10,0Z" />"##;
    assert_eq!(
        body_defects(self_reference),
        [XpsElementDefect::BrushCyclic]
    );
}

/// **`MAX_XPS_RESOURCE_DEPTH` fires by its own warning**, at the shipped
/// value, and a chain one shorter resolves.
///
/// A chain of distinct keys is **not** a cycle, so the cycle guard cannot
/// answer it and the depth cap cannot be reported as one. Both halves are
/// asserted here because deleting either leaves the other passing every test
/// written for it.
#[test]
fn a_static_resource_chain_past_the_depth_cap_is_named() {
    let chain = |length: usize| {
        let mut entries = String::new();
        for at in 0..length {
            entries.push_str(&format!(
                r##"<SolidColorBrush x:Key="k{at}" Color="{{StaticResource k{}}}" />"##,
                at + 1
            ));
        }
        entries.push_str(&format!(
            r##"<SolidColorBrush x:Key="k{length}" Color="#FF0000" />"##
        ));
        format!(
            r##"<FixedPage.Resources><ResourceDictionary>{entries}</ResourceDictionary></FixedPage.Resources><Path Fill="{{StaticResource k0}}" Data="M0,0L10,0Z" />"##
        )
    };

    // Sixteen hops is the cap and resolves.
    let short = chain(16);
    assert_eq!(body_defects(&short), []);
    assert!(drawn(&short).contains("1 0 0 rg"), "{}", drawn(&short));

    // Seventeen is past it, and is reported as a depth and not as a cycle.
    let long = chain(17);
    assert_eq!(body_defects(&long), [XpsElementDefect::BrushTooDeep]);
    assert!(drawn(&long).contains("0.749 0.749 0.749 rg"));
}

/// A `ResourceDictionary` naming another part is milestone 8's, and says so
/// once rather than once per key.
#[test]
fn a_resource_dictionary_in_another_part_is_named() {
    let body = r##"<FixedPage.Resources><ResourceDictionary Source="/Resources/d.xml" /></FixedPage.Resources><Path Fill="{StaticResource b0}" Data="M0,0L10,0Z" />"##;
    assert_eq!(
        body_defects(body),
        [
            XpsElementDefect::ResourceDictionaryRemote,
            XpsElementDefect::BrushUnresolved
        ]
    );
}

/// A dictionary entry with no `x:Key` can never be referenced, and 14.2.2
/// makes the key mandatory — so it is named rather than silently indexed by
/// position.
#[test]
fn a_dictionary_entry_with_no_key_is_named() {
    let body = r##"<FixedPage.Resources><ResourceDictionary><SolidColorBrush Color="#FF0000" /></ResourceDictionary></FixedPage.Resources><Path Fill="#000000" Data="M0,0L10,0Z" />"##;
    assert_eq!(body_defects(body), [XpsElementDefect::ElementUnknown]);
}

// ---- what is not drawn, by name -----------------------------------------

/// A `Glyphs` run whose font is not in the package is named rather than passed
/// over, because a page whose words are missing and whose report says nothing
/// is gap 17's blank page one element down.
///
/// *Amended by milestone 7.* Until that milestone the answer was
/// `GlyphsNotDrawn` — one name for every run on every page, whatever was wrong
/// with it. That variant is **deleted** rather than kept as a shape nothing
/// produces, which is milestone 6's own argument for deleting `NotDrawn` one
/// level up: a record of old behaviour that does not break when the behaviour
/// changes is not a record.
#[test]
fn a_glyphs_run_is_named_rather_than_silently_skipped() {
    let body = r##"<Glyphs OriginX="10" OriginY="10" FontRenderingEmSize="12" FontUri="/Resources/f.odttf" UnicodeString="hi" Fill="#000000" />"##;
    assert_eq!(body_defects(body), [XpsElementDefect::GlyphsFontUnresolved]);
}

/// **A property element belongs to the element whose name it carries, whole.**
///
/// `Owner.Property` is matched on the **segment** before the dot and not as a
/// prefix, and the injection matrix is why this test exists: a build that
/// compared prefixes failed nothing in the workspace, because every fixture in
/// it spelled the owner exactly. The consequence is not cosmetic — a
/// `<CanvasBackdrop.Clip>` an unknown vocabulary hung on a page would be read
/// as the **canvas's** clip, and a clip that came from markup this build does
/// not understand hides whatever it does.
///
/// Both directions, because the pair is what says the check is a check: the
/// real property applies, and the near miss does not.
#[test]
fn a_property_element_belongs_to_the_element_whose_name_it_carries() {
    let real = drawn(
        r##"<Canvas><Canvas.Clip><PathGeometry Figures="M0,0L1,0 1,1Z" /></Canvas.Clip><Path Fill="#000000" Data="M0,0L100,0 100,100Z" /></Canvas>"##,
    );
    assert!(
        real.contains("W* n"),
        "the canvas's own clip applies: {real}"
    );

    let near_miss = r##"<Canvas><CanvasBackdrop.Clip><PathGeometry Figures="M0,0L1,0 1,1Z" /></CanvasBackdrop.Clip><Path Fill="#000000" Data="M0,0L100,0 100,100Z" /></Canvas>"##;
    let content = drawn(near_miss);
    assert!(
        !content.contains("W* n"),
        "`CanvasBackdrop.Clip` is not a `Canvas`'s clip: {content}"
    );
    assert!(
        content.contains("0 0 0 rg"),
        "and the path is still drawn: {content}"
    );
    assert_eq!(body_defects(near_miss), [XpsElementDefect::ElementUnknown]);
}

/// Markup from another vocabulary is skipped whole, and named.
///
/// Skipped **whole** is the load-bearing half: a reader that dropped the
/// element and kept walking would read its children as siblings of the thing
/// that contained them, and draw a `Path` nested inside a foreign element as
/// though the page had stated it.
#[test]
fn markup_from_another_vocabulary_is_skipped_whole_and_named() {
    let body = r##"<Whatever xmlns="urn:somewhere-else"><Path xmlns="http://schemas.microsoft.com/xps/2005/06" Fill="#FF0000" Data="M0,0L10,0Z" /></Whatever>"##;
    assert_eq!(body_defects(body), [XpsElementDefect::ElementUnknown]);
    assert!(
        !drawn(body).contains("1 0 0 rg"),
        "the nested path is inside the element that was skipped: {}",
        drawn(body)
    );
}

// ---- the criterion this milestone exists for ----------------------------

/// **The whole of the real one-page package in gap 30's design section.**
///
/// `wpf-image-and-text.xps` is the file the plan quotes, and every one of the
/// five things it says are *"not optional, from the first file"* has to work
/// for it to reach a page at all:
///
/// - an image is a `<Path>` filled with an `<ImageBrush>` — there is no image
///   element in XPS;
/// - the brush is named through `FixedPage.Resources` and `{StaticResource}`,
///   so resource dictionaries cannot be deferred;
/// - the geometry is abbreviated — `M0,0L200,0 200,200 0,200Z`;
/// - `RenderTransform` is a six-number list and is present even for a
///   translation;
/// - `Indices=",53"` is 12.1.3's least obvious form.
///
/// Four of the five are milestone 6's and all four work. The fifth is the
/// `Glyphs` run, which is milestone 7, and the raster behind the brush is
/// milestone 8 — so what this asserts is that the page **reads whole**: the
/// path's geometry and transform are the markup's own numbers, the
/// `{StaticResource}` resolved (its brush is grey because it is an
/// `ImageBrush`, which is a different name from a reference that did not
/// resolve), and exactly two things on the page are owed to later milestones.
#[test]
fn the_real_one_page_package_from_the_design_section_renders() {
    let document = Document::open(corpus("wpf-image-and-text.xps")).expect("a fixed document");
    assert_eq!(document.page_count(), 1);
    assert_eq!(document.page(0).map(|p| p.size()), Some((612.0, 792.0)));

    let report = document.archive().expect("a synthesised document");
    // **No page-level warning**: the page read, and it drew.
    assert!(
        !report
            .warnings()
            .iter()
            .any(|w| matches!(w, ArchiveWarning::XpsPage { .. })),
        "{:?}",
        report.warnings()
    );
    // And exactly the two things later milestones own.
    let element: Vec<XpsElementDefect> = report
        .warnings()
        .iter()
        .filter_map(|w| match w {
            ArchiveWarning::XpsElement { defect, .. } => Some(*defect),
            _ => None,
        })
        .collect();
    // **Nothing is owed.** Milestone 6 drew the path, milestone 7 the run and
    // milestone 8 the `ImageBrush` behind the `{{StaticResource}}`, so the
    // whole of the design section's package now reads. This is the assertion
    // row 6 was written for and could not have, and it is stated as an empty
    // list rather than as "no brush warning" so that a list which grew a new
    // complaint would fail whatever the complaint was.
    assert_eq!(
        element,
        [],
        "the design section's package reads whole: {:?}",
        report.warnings()
    );

    // The `Path`'s own numbers, straight out of the markup.
    let content = stream(&document);
    assert!(
        content.starts_with("q 0.75 0 0 -0.75 0 792 cm"),
        "{content}"
    );
    assert!(content.contains("1 0 0 1 100 100 cm"), "{content}");
    for operator in ["0 0 m", "200 0 l", "200 200 l", "0 200 l", "h", "f*"] {
        assert!(content.contains(operator), "{operator} in {content}");
    }

    // The OpenXPS twin of the same document is the same page. Its markup
    // spells the geometry `M 0,0 L 200,0 …`, its colours six digits, and its
    // `ImageSource` relatively — and none of that reaches the content stream.
    let twin = Document::open(corpus("xpsom-image-and-text.oxps")).expect("a fixed document");
    assert_eq!(ops(&stream(&twin)), ops(&content));
}

/// And the rest of the corpus, counted: **all eight packages render with
/// nothing owed at all**, and no page in any of them is a placeholder.
///
/// *Amended, milestone 8.* This said "two of the eight" when milestone 6 wrote
/// it and "four" after milestone 7. The number is kept in the sentence rather
/// than dropped, because it is the one line in this suite that says how much of
/// a format written by somebody else this build actually reads.
#[test]
fn every_package_in_the_corpus_draws_and_says_what_it_does_not() {
    let expected: &[(&str, &[XpsElementDefect])] = &[
        ("wpf-shapes-only.xps", &[]),
        ("wpf-three-pages.xps", &[]),
        ("wpf-gradients.xps", &[]),
        ("xpsom-gradients.oxps", &[]),
        // Milestone 7 drew the `Glyphs` runs and milestone 8 the `ImageBrush`
        // each of these hangs its picture from, so **every package in the
        // corpus now owes nothing**. Eight of eight, where milestone 6 left
        // four and milestone 7 left six.
        ("wpf-image-and-text.xps", &[]),
        ("xpsom-image-and-text.oxps", &[]),
        ("wpf-tiled-brush.xps", &[]),
        ("wpf-jpeg-image.xps", &[]),
    ];
    for (name, owed) in expected {
        let bytes = corpus(name);
        let document = Document::open(bytes).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        let report = document.archive().expect("a synthesised document");
        assert!(
            !report
                .warnings()
                .iter()
                .any(|w| matches!(w, ArchiveWarning::XpsPage { .. })),
            "{name}: no page is a placeholder, and these are: {:?}",
            report.warnings()
        );
        let seen: Vec<XpsElementDefect> = report
            .warnings()
            .iter()
            .filter_map(|w| match w {
                ArchiveWarning::XpsElement { defect, .. } => Some(*defect),
                _ => None,
            })
            .collect();
        assert_eq!(&seen, owed, "{name}");
    }
}

// ---- the bounds ---------------------------------------------------------

/// **`MAX_XPS_ELEMENTS` fires by its own refusal**, at the shipped value,
/// against a real package built past it.
///
/// Three parts rather than one, and that is the finding rather than a fixture
/// detail: `tinker_pdf_xml::limits::MAX_XML_TOKENS` bounds one part at a
/// million events, which is about half a million elements — so **no single
/// part can reach this cap**, and only a total across parts can. That is
/// exactly what a work cap is for, and a per-page cap set to the same number
/// would never fire at all.
///
/// A document one element short opens, so the cap is what stopped it rather
/// than something else about a large package.
#[test]
fn a_page_past_the_element_cap_is_refused_by_name() {
    // Three parts, each a `FixedPage` root plus `count` elements, so the
    // document spends `3 * (1 + count)`. The cap is 1 048 576.
    let stuffed = |count: usize| {
        let mut out = String::with_capacity(count * 4 + 128);
        out.push_str(&format!(
            r#"<FixedPage xmlns="{XPS_NS}" Width="816" Height="1056">"#
        ));
        for _ in 0..count {
            out.push_str("<A/>");
        }
        out.push_str("</FixedPage>");
        out
    };
    let package = |count: usize| {
        let mut parts = with(
            one_page_package(),
            "Documents/1/FixedDocument.fdoc",
            &document(&["Pages/1.fpage", "Pages/2.fpage", "Pages/3.fpage"]),
        );
        parts = with(parts, "Documents/1/Pages/1.fpage", &stuffed(count));
        parts = before_content_types(parts, part("Documents/1/Pages/2.fpage", &stuffed(count)));
        parts = before_content_types(parts, part("Documents/1/Pages/3.fpage", &stuffed(count)));
        archive(parts)
    };

    // 3 * (1 + 349 524) is 1 048 575, one under the cap.
    assert!(
        open(&package(349_524)).is_ok(),
        "one element short of the cap opens"
    );
    // 3 * (1 + 349 525) is 1 048 578, past it.
    assert!(matches!(
        open(&package(349_525)),
        Err(tinker_pdf::OpenError::UnsupportedArchive(
            ArchiveRefusal::TooLarge
        ))
    ));
}

/// **`MAX_XPS_SEGMENTS` fires by its own refusal**, at the shipped value.
///
/// One `Data` attribute, because that is the shape the cap exists for: `H1` is
/// two bytes and one segment, so a 17 MB attribute in a part this build
/// already admits is eight million segments out of an archive that deflates to
/// a hundred kilobytes. A per-path cap would not see it and a per-page cap
/// would not either.
#[test]
fn a_geometry_past_the_segment_cap_is_refused_by_name() {
    let page = |count: usize| {
        let mut data = String::with_capacity(count * 2 + 8);
        data.push_str("M0,0");
        for _ in 0..count {
            data.push_str("H1");
        }
        let markup = format!(
            r##"<FixedPage xmlns="{XPS_NS}" Width="816" Height="1056"><Path Fill="#000000" Data="{data}" /></FixedPage>"##
        );
        archive(with(
            one_page_package(),
            "Documents/1/Pages/1.fpage",
            &markup,
        ))
    };

    // The `M` is one segment, so 8 388 607 `H` commands is exactly the cap.
    assert!(
        open(&page(8_388_607)).is_ok(),
        "a geometry exactly at the cap is drawn"
    );
    assert!(matches!(
        open(&page(8_388_608)),
        Err(tinker_pdf::OpenError::UnsupportedArchive(
            ArchiveRefusal::TooLarge
        ))
    ));
}

/// The two totals are **totals**: a page-sized fixture under the cap and three
/// of them over it is the same test the element cap runs, said as a property.
///
/// `MAX_ZIP_INFLATED` taught this once already and gap 30's own bounds section
/// says it twice — *"a per-element cap times a file-chosen element count is
/// not a bound"*. What this adds is the arithmetic: one part costs a third of
/// what three cost, so a build that reset the budget per part would open the
/// package the test above refuses.
#[test]
fn the_element_total_is_a_total_and_not_a_per_page_cap() {
    let stuffed = |count: usize| {
        let mut out = String::with_capacity(count * 4 + 128);
        out.push_str(&format!(
            r#"<FixedPage xmlns="{XPS_NS}" Width="816" Height="1056">"#
        ));
        for _ in 0..count {
            out.push_str("<A/>");
        }
        out.push_str("</FixedPage>");
        out
    };
    // One part of 349 525 elements is nowhere near the cap and opens.
    let one = archive(with(
        one_page_package(),
        "Documents/1/Pages/1.fpage",
        &stuffed(349_525),
    ));
    assert!(open(&one).is_ok(), "one page of that size is fine");
    // Three of them is not, which is the whole of what "total" means.
}
