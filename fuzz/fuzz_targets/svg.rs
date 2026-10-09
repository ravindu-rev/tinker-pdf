//! SVG geometry: §8.3.2's path data grammar and §7.6's transform list.
//!
//! The repository's **thirty-third** target. Like `css` and `xml`, the input is
//! *text* rather than a record layout, so there is no offset to corrupt and no
//! length field to lie about. What a mutator finds instead is the grammar's
//! seams, and this grammar has more of them than it looks: a sign that
//! separates two numbers where a space would, an exponent with no digits, a
//! command letter that repeats itself implicitly, a `moveto` whose repetition
//! is a `lineto` and not another `moveto`, and — the one that has broken every
//! path parser ever written — **an arc's two flags, which are one character
//! each and not numbers**, so that `a1 1 0 1130` is a valid arc with an x of
//! thirty rather than an x of eleven hundred and thirty.
//!
//! The control byte picks the **segment budget** rather than the input, for the
//! reason `css`'s header gives: a target whose limits are all at their shipped
//! defaults never explores a refusal, because a million-segment cap cannot fire
//! inside one iteration.
//!
//! What is asserted beyond "it did not panic":
//!
//! - **The budget holds rather than being exceeded and reported.** The
//!   segments produced never outnumber what was granted, and what was spent
//!   plus what is left is what was handed in — an off-by-one in the accounting
//!   is a cap that does not cap.
//! - **Every coordinate is finite.** A NaN or an infinity here does not fail
//!   anything in this crate; it reaches a rasterizer, where it is a page of
//!   nothing, and the file that produced it looked perfectly ordinary. SVG's
//!   number grammar admits `1e999`, so this is reachable from text a person
//!   could type.
//! - **Parsing is deterministic**, which is ruling 4 asserted on a parser
//!   rather than on a rendered page.
//! - **An outline begins with a move.** §8.3.2 requires it, `parse` enforces
//!   it, and a subpath with no start is a rasterizer's problem rather than
//!   this crate's — so it must never leave here.
//! - **A transform preserves the shape of an outline**: the same number of
//!   segments, each of the same kind. A matrix moves points and nothing else,
//!   and a `transformed` that dropped a `Close` would turn a filled shape into
//!   an open one that still fills — visibly wrong only sometimes.
//! - **An unreadable transform is `None` rather than an identity.** That is the
//!   difference between a caller that can name the defect and a shape drawn
//!   somewhere the file did not put it.
//!
//! # The document surface, from milestone 1
//!
//! The same bytes are also read as **markup**, through
//! [`tinker_pdf_svg::read`], which is the whole crate end to end: the XML
//! reader, the element tree, and the walk that composes transforms into a
//! scene. That half is where a mutator finds the things a grammar fuzzer
//! cannot — a `<!DOCTYPE` with an internal subset, an element nested past the
//! cap, a `viewBox` with no area, a `<use>` that reaches its own ancestor.
//!
//! What is asserted over a scene:
//!
//! - **Every number in it is finite**, for the reason a coordinate is: an
//!   infinity reaches a page and the file that produced it looked ordinary.
//! - **Reading is deterministic**, ruling 4 over the whole crate rather than
//!   over one parser.
//! - **The warning list is deduplicated and inside its cap**, so a document
//!   cannot choose how much memory its own diagnostics cost.
//! - **Nothing is past a limit that refused nothing.** A scene came back, so
//!   every ceiling held; the node count is checked against the one that bounds
//!   it rather than assumed.
//! - **A text run's font size is a number.** It reaches a `Tf` operator, and an
//!   infinity there is a content stream a reader refuses outright — which is a
//!   worse failure than a page that looks wrong, not a better one.
//!
//! # A `<style>` element's imports, from the same bytes
//!
//! The document is read a second time through [`tinker_pdf_svg::read_with`]
//! and a resolver that answers **every** `@import` with the input itself,
//! under the address it was asked for — so a mutation that puts an `@import`
//! at the front of the file makes the file its own stylesheet, a chain of
//! distinct names nests it, and a repeated name is a cycle. What is asserted
//! beyond the scene's own invariants above:
//!
//! - **The bytes every import shares hold**: what the resolver handed back
//!   never comes to more than `MAX_CSS_BYTES` and one sheet past it, the one
//!   that crossed it — a comment is no tokens, so the token budget alone
//!   would let a file read itself without end.
//! - **Reading with imports is deterministic**, the same scene and the same
//!   number of fetches twice.
//!
//! # The runs measured, from the same bytes
//!
//! A third read hands the walk a measurer, [`tinker_pdf_svg::Context::with_measure`]:
//! every character a fixed fraction of an em, or — under the control byte's
//! `0x40` — `1e300` ems, so a box at the edge of a double's range is reached.
//! A bounding-box paint on a run waits for its `<text>`'s box as a mark in its
//! place, so what is asserted beyond the scene's own invariants is:
//!
//! - **No mark reaches the caller**: every colour in the scene, at every
//!   depth of a group, a mask or a tile, is in `[0, 1]`.
//! - **A document with nothing to measure reads the same**: where the
//!   unmeasured read named no `TextBoxUnmeasured`, and its warnings were not
//!   capped short of naming one, the measured scene is that scene.
//! - **Reading with a measurer is deterministic.**
//!
//! # What this target cannot find, and what covers it instead
//!
//! Every assertion above is **structural**: a scene carries only finite
//! numbers, the node cap holds, a warning is reported once, and reading a
//! document is deterministic. None of them asks whether the result is the one
//! the input describes, so an answer that is well-formed and *wrong* passes
//! exactly as a correct one does.
//!
//! A path parsed to the wrong coordinates is finite, capped, warned about
//! once and deterministic. Correctness lives in `crates/tinker-pdf-
//! svg/tests/` — `shapes.rs`, `paint.rs`, `gradients.rs`, `text.rs` and
//! `reuse.rs` — which compare the scene that comes out.
//!
#![no_main]
use libfuzzer_sys::fuzz_target;

use std::cell::Cell;

use tinker_pdf_css::ImportResolver;
use tinker_pdf_svg::path::{self, Outline, Segment};
use tinker_pdf_svg::{
    transform, Context, Limits, MeasureText, Node, Paint, RunMetrics, Scene, TextStyle, Warning,
};

/// Every character `.0` ems wide, eight tenths of an em above the baseline
/// and two below.
struct Pitch(f64);

impl MeasureText for Pitch {
    fn measure(&self, text: &str, font: &TextStyle) -> RunMetrics {
        let count = text.chars().count() as f64;
        RunMetrics {
            advance: self.0 * font.size * count,
            ascent: 0.8 * font.size,
            descent: 0.2 * font.size,
        }
    }
}

/// Every colour a list of nodes paints with, at every depth of a group, a
/// mask and a tile, is in `[0, 1]`: no paint left waiting reaches a caller.
fn colours_are_colours(nodes: &[Node]) {
    fn paint(server: &Paint) {
        match server {
            Paint::Solid(colour) => assert!(
                colour.rgb.iter().all(|c| (0.0..=1.0).contains(c)),
                "a colour no document can state reached the caller: {colour:?}"
            ),
            Paint::Linear { stops, .. } | Paint::Radial { stops, .. } => {
                for stop in stops {
                    paint(&Paint::Solid(stop.colour));
                }
            }
            Paint::Pattern(tile) => colours_are_colours(&tile.nodes),
            _ => {}
        }
    }
    for node in nodes {
        match node {
            Node::Path { fill, stroke, .. } | Node::Text { fill, stroke, .. } => {
                paint(fill);
                if let Some(stroke) = stroke {
                    paint(&stroke.paint);
                }
            }
            Node::Group { nodes, mask, .. } => {
                colours_are_colours(nodes);
                if let Some(mask) = mask {
                    colours_are_colours(&mask.nodes);
                }
            }
            _ => {}
        }
    }
}

/// Every `@import` answered with the input, under the address asked for, and
/// what was handed back counted.
struct Itself<'a> {
    body: &'a [u8],
    fetched: Cell<usize>,
    bytes: Cell<usize>,
}

impl ImportResolver for Itself<'_> {
    fn resolve(&self, href: &str, _base: Option<&str>) -> Option<(String, Vec<u8>)> {
        self.fetched.set(self.fetched.get() + 1);
        self.bytes.set(self.bytes.get() + self.body.len());
        Some((href.to_owned(), self.body.to_vec()))
    }
}

/// The scene's own invariants: finite numbers, the node cap, and warnings
/// deduplicated inside theirs.
fn check(scene: &Scene, limits: &Limits) {
    for number in numbers(scene) {
        assert!(
            number.is_finite(),
            "a scene carries something that is not a number: {number}"
        );
    }
    // At every depth: a group is assembled in a list of its own, so the
    // top-level length is not the number the cap is about.
    assert!(
        count(&scene.nodes) <= limits.max_nodes,
        "{} nodes came out of a cap of {}",
        count(&scene.nodes),
        limits.max_nodes
    );
    assert!(
        scene.warnings.len() <= limits.max_warnings,
        "the warning cap did not hold"
    );
    for (index, warning) in scene.warnings.iter().enumerate() {
        assert!(
            !scene.warnings[..index].contains(warning),
            "a warning was reported twice: {warning:?}"
        );
    }
}

/// Every point a segment carries.
fn points(segment: &Segment) -> Vec<[f64; 2]> {
    match *segment {
        Segment::Move(p) | Segment::Line(p) => vec![p],
        Segment::Cubic(a, b, c) => vec![a, b, c],
        Segment::Close => Vec::new(),
        _ => Vec::new(),
    }
}

/// Which variant a segment is, so two outlines can be compared by shape.
fn kind(segment: &Segment) -> u8 {
    match segment {
        Segment::Move(_) => 0,
        Segment::Line(_) => 1,
        Segment::Cubic(..) => 2,
        Segment::Close => 3,
        _ => 4,
    }
}

/// Every coordinate an outline carries, appended.
fn sweep(outline: &Outline, out: &mut Vec<f64>) {
    for segment in &outline.segments {
        for point in points(segment) {
            out.extend_from_slice(&point);
        }
    }
}

/// Every number a paint carries, appended.
fn sweep_paint(paint: &Paint, out: &mut Vec<f64>) {
    match paint {
        Paint::None | Paint::Solid(_) => {}
        Paint::Linear {
            from,
            to,
            matrix,
            stops,
            ..
        } => {
            out.extend_from_slice(&[from[0], from[1], to[0], to[1]]);
            out.extend_from_slice(matrix);
            out.extend(stops.iter().flat_map(|stop| [stop.offset, stop.opacity]));
        }
        Paint::Radial {
            centre,
            radius,
            focus,
            matrix,
            stops,
            ..
        } => {
            out.extend_from_slice(&[centre[0], centre[1], *radius, focus[0], focus[1]]);
            out.extend_from_slice(matrix);
            out.extend(stops.iter().flat_map(|stop| [stop.offset, stop.opacity]));
        }
        // A tile's cell reaches a `/BBox` and `/XStep`, its matrix a
        // `/Matrix`, and its nodes a cell's content stream.
        Paint::Pattern(tile) => {
            out.extend_from_slice(&tile.cell);
            out.extend_from_slice(&tile.matrix);
            sweep_nodes(&tile.nodes, out);
        }
        _ => {}
    }
}

/// The nodes a paint's tile holds, at every depth.
fn count_paint(paint: &Paint) -> usize {
    match paint {
        Paint::Pattern(tile) => count(&tile.nodes),
        _ => 0,
    }
}

/// Every number a scene carries, so one assertion can sweep all of them.
///
/// **Every field, not the interesting ones.** A gradient's `/Matrix` and a
/// clip's outline reach a page exactly as a coordinate does, and an infinity
/// in either is the same failure: a rasterizer with nothing to draw and a file
/// that looked ordinary.
fn numbers(scene: &Scene) -> Vec<f64> {
    let mut out = vec![scene.size.0, scene.size.1];
    sweep_nodes(&scene.nodes, &mut out);
    out
}

/// Every node a list holds, at every depth of `Node::Group`.
fn count(nodes: &[Node]) -> usize {
    nodes
        .iter()
        .map(|node| match node {
            Node::Group { nodes, mask, .. } => {
                1 + count(nodes) + mask.as_ref().map_or(0, |mask| count(&mask.nodes))
            }
            // A pattern's tile is charged as it is built, once per shape it
            // paints.
            Node::Path { fill, stroke, .. } | Node::Text { fill, stroke, .. } => {
                1 + count_paint(fill) + stroke.as_ref().map_or(0, |s| count_paint(&s.paint))
            }
            _ => 1,
        })
        .sum()
}

/// [`numbers`] over one list, groups looked through.
fn sweep_nodes(nodes: &[Node], out: &mut Vec<f64>) {
    for node in nodes {
        match node {
            Node::Path {
                outline,
                fill,
                fill_opacity,
                stroke,
                clip,
                ..
            } => {
                sweep(outline, out);
                if let Some(clip) = clip {
                    sweep(&clip.outline, out);
                }
                sweep_paint(fill, out);
                out.push(*fill_opacity);
                if let Some(stroke) = stroke {
                    sweep_paint(&stroke.paint, out);
                    out.extend_from_slice(&[
                        stroke.width,
                        stroke.miter_limit,
                        stroke.dash_offset,
                        stroke.opacity,
                    ]);
                    out.extend_from_slice(&stroke.dashes);
                    // The stroke's user space reaches a `cm` the facade writes.
                    out.extend_from_slice(&stroke.matrix);
                }
            }
            Node::Image { rect, matrix, .. } => {
                out.extend_from_slice(rect);
                out.extend_from_slice(matrix);
            }
            Node::Text {
                anchor,
                matrix,
                font,
                fill,
                fill_opacity,
                stroke,
                rotate,
                ..
            } => {
                if let Some(anchor) = anchor {
                    out.extend_from_slice(anchor);
                }
                out.push(*rotate);
                out.extend_from_slice(matrix);
                // The size reaches a `Tf` operator, where an infinity is a
                // content stream a reader refuses rather than a page that
                // looks wrong — which is worse, not better.
                out.push(font.size);
                sweep_paint(fill, out);
                out.push(*fill_opacity);
                if let Some(stroke) = stroke {
                    sweep_paint(&stroke.paint, out);
                    out.push(stroke.width);
                }
            }
            // A group's opacity reaches an `/ExtGState` and its clip a `W`,
            // exactly as a shape's do.
            Node::Group {
                nodes,
                opacity,
                clip,
                mask,
            } => {
                out.push(*opacity);
                if let Some(clip) = clip {
                    sweep(&clip.outline, out);
                }
                // A mask's region reaches a `W` and its content a soft mask's
                // form, so both are swept like the group's own.
                if let Some(mask) = mask {
                    if let Some(region) = &mask.region {
                        sweep(region, out);
                    }
                    sweep_nodes(&mask.nodes, out);
                }
                sweep_nodes(nodes, out);
            }
            _ => {}
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let (control, body) = data.split_at(data.len().min(1));
    let knobs = control.first().copied().unwrap_or(0);

    // ---- the whole crate, over the same bytes -------------------------------
    //
    // Before the UTF-8 gate below, deliberately: `tinker_pdf_svg::read` takes
    // bytes and decides the encoding itself, so gating it on `from_utf8` would
    // hide every UTF-16 document and every byte sequence the decoder refuses.
    let mut limits = Limits::DEFAULT;
    // Small enough that the caps are crossable inside one iteration. The
    // shipped values are exercised by the `3` arm.
    match (knobs >> 2) & 3 {
        0 => {
            limits.max_depth = 4;
            limits.max_nodes = 8;
            limits.max_uses = 2;
            limits.max_warnings = 2;
        }
        1 => {
            limits.max_depth = 16;
            limits.max_nodes = 64;
            limits.max_uses = 8;
            limits.max_warnings = 8;
        }
        2 => limits.max_segments = 32,
        _ => {}
    }
    let viewport = if knobs & 0x80 == 0 {
        None
    } else {
        Some((100.0, 50.0))
    };
    let plain = tinker_pdf_svg::read(body, viewport, &limits).ok();
    if let Some(scene) = &plain {
        check(scene, &limits);
        colours_are_colours(&scene.nodes);
        let again = tinker_pdf_svg::read(body, viewport, &limits)
            .expect("the same bytes refused on a second run");
        assert!(again == *scene, "reading a document is not deterministic");
    }

    // ---- the runs measured ---------------------------------------------------
    let pitch = Pitch(if knobs & 0x40 == 0 { 0.5 } else { 1e300 });
    let measuring = Context::NONE.with_measure(&pitch);
    let measured = tinker_pdf_svg::read_with(body, viewport, &limits, &measuring);
    if let Ok(scene) = &measured {
        check(scene, &limits);
        colours_are_colours(&scene.nodes);
        let again = tinker_pdf_svg::read_with(body, viewport, &limits, &measuring)
            .expect("the same bytes refused on a second run");
        assert!(
            again == *scene,
            "reading with a measurer is not deterministic"
        );
    }
    if let Some(plain) = &plain {
        // Only a paint that would be `TextBoxUnmeasured` waits for a box; a
        // capped warning list may have had no room to name one.
        if !plain.warnings.contains(&Warning::TextBoxUnmeasured)
            && plain.warnings.len() < limits.max_warnings
        {
            assert!(
                measured.as_ref() == Ok(plain),
                "a document with nothing to measure read differently with a measurer"
            );
        }
    }

    // ---- the same bytes as their own stylesheet ------------------------------
    let itself = Itself {
        body,
        fetched: Cell::new(0),
        bytes: Cell::new(0),
    };
    let read = tinker_pdf_svg::read_with(body, viewport, &limits, &Context::new(&itself));
    let cap = tinker_pdf_css::limits::MAX_CSS_BYTES;
    assert!(
        itself.bytes.get() <= cap.saturating_add(body.len()),
        "{} bytes were imported past a cap of {cap}",
        itself.bytes.get()
    );
    if let Ok(scene) = read {
        check(&scene, &limits);
        let fetched = itself.fetched.replace(0);
        let again = tinker_pdf_svg::read_with(body, viewport, &limits, &Context::new(&itself))
            .expect("the same bytes refused on a second run");
        assert!(again == scene, "reading with imports is not deterministic");
        assert_eq!(
            itself.fetched.get(),
            fetched,
            "imports were fetched differently"
        );
    }

    let Ok(text) = core::str::from_utf8(body) else {
        return;
    };

    // Small enough that the cap is crossable inside one iteration, and wide
    // enough that the shipped value is also exercised.
    let granted = match knobs & 3 {
        0 => 0,
        1 => 4,
        2 => 512,
        _ => 1 << 20,
    };

    // ---- the transform list ------------------------------------------------
    //
    // Driven first because it is the cheaper half and because a `None` here is
    // a result rather than a dead end: the assertion is that the grammar either
    // yields six finite numbers or refuses, never an identity standing in for a
    // transform the file stated.
    if let Some(matrix) = transform::list(text) {
        assert!(
            matrix.iter().all(|n| n.is_finite()),
            "a transform produced a coordinate that is not a number: {matrix:?}"
        );
        let again = transform::list(text).expect("the same text refused on a second run");
        assert!(matrix == again, "reading a transform is not deterministic");
    }
    if let Some(numbers) = transform::numbers(text) {
        assert!(
            numbers.iter().all(|n| n.is_finite()),
            "the number grammar admitted something that is not a number"
        );
    }
    // §7.7's mapping, over a viewport the input does not choose: a degenerate
    // view box must refuse rather than map, and a legal one must not produce a
    // matrix with a hole in it.
    if let Some(numbers) = transform::numbers(text) {
        if numbers.len() >= 4 {
            let box_ = [numbers[0], numbers[1], numbers[2], numbers[3]];
            if let Some(matrix) = transform::view_box(box_, 100.0, 50.0, Some(text)) {
                assert!(
                    matrix.iter().all(|n| n.is_finite()),
                    "a view box mapped to a matrix that is not numbers: {matrix:?}"
                );
            }
        }
    }

    // ---- the path data -----------------------------------------------------
    let mut budget = granted;
    let Ok(outline) = path::parse(text, &mut budget) else {
        return;
    };

    assert!(
        outline.segments.len() <= granted,
        "{} segments came out of a budget of {granted}",
        outline.segments.len()
    );
    assert_eq!(
        outline.segments.len() + budget,
        granted,
        "what was spent and what is left do not add up to what was granted"
    );

    if let Some(first) = outline.segments.first() {
        assert!(
            matches!(first, Segment::Move(_)),
            "an outline began with something other than a move: {first:?}"
        );
    }

    for segment in &outline.segments {
        for point in points(segment) {
            assert!(
                point[0].is_finite() && point[1].is_finite(),
                "a segment carries a coordinate that is not a number: {segment:?}"
            );
        }
    }

    // Ruling 4, on the parser.
    let mut again_budget = granted;
    let again = path::parse(text, &mut again_budget).expect("the same input refused on a second run");
    assert!(again == outline, "parsing path data is not deterministic");

    // A matrix moves points and nothing else.
    let moved = outline.transformed([2.0, 0.5, -0.25, 3.0, 7.0, -11.0]);
    assert_eq!(
        moved.segments.len(),
        outline.segments.len(),
        "a transform changed how many segments there are"
    );
    for (before, after) in outline.segments.iter().zip(&moved.segments) {
        assert_eq!(
            kind(before),
            kind(after),
            "a transform changed a segment's kind"
        );
    }
    assert_eq!(
        moved.is_empty(),
        outline.is_empty(),
        "a transform changed whether the outline draws anything"
    );
});
