//! SVG 1.1 §13.2's gradients, and §14.3's clipping paths.
//!
//! # A paint server is two elements, and the second one has the stops
//!
//! Every gradient in the fetched corpus is written twice: once with the stops
//! and the geometry, and again as an empty element with an `xlink:href` to the
//! first and a `gradientTransform` of its own. That is not a quirk of one
//! exporter — §13.2.3 defines it, and it is how a file states the same ramp at
//! two places without repeating a dozen stops. So resolution follows the
//! reference chain for **every** attribute independently: a `<linearGradient>`
//! may take its `x1` from itself, its stops from its parent, and its
//! `spreadMethod` from its grandparent.
//!
//! The chain is depth-capped, and it has to be: `<linearGradient id="a"
//! xlink:href="#b"/><linearGradient id="b" xlink:href="#a"/>` is well-formed
//! XML and a cycle.
//!
//! # Geometry stays in gradient space
//!
//! §13.2.3's `gradientUnits` and `gradientTransform` both become a matrix
//! rather than being multiplied into the coordinates. `objectBoundingBox` on a
//! shape that is not square makes a *circle* into an **ellipse**, which two
//! transformed points cannot express — and neither can PDF's shading, which is
//! why 8.7.4.5.5 puts a `/Matrix` on the pattern instead. Baking would have
//! been the plausible shortcut and it is wrong for exactly the files that use
//! the feature.

use crate::document::{self, Node, Tree};
use crate::path::Outline;
use crate::shape::{self, Shape};
use crate::style::Style;
use crate::transform::{self, IDENTITY};
use crate::{Clip, Paint, Refusal, Spread, Stop};

/// How far an `xlink:href` chain between paint servers may run.
///
/// **This is the cycle guard, and it is the only one.** Two elements
/// referencing each other is two elements and an endless walk, so
/// [`crate::Limits::max_nodes`] cannot bound it. The first draft had a second
/// guard beside this — a `contains` check that stopped a chain returning
/// somewhere it had been — and the injection matrix found it fires **nothing**:
/// with the cap in place a cycle already terminates, and the repeated entries
/// it leaves in the chain resolve to exactly the same attributes and the same
/// stops. A rule enforced twice hides the reachable half, so the second one is
/// gone and this one is asserted directly.
///
/// Ten because no file has ever needed two, and `a_reference_chain_stops_at_the_cap`
/// builds twelve.
const MAX_HREF_CHAIN: usize = 10;

/// The chain of elements an `xlink:href` walk reaches, nearest first.
///
/// Returned as a list rather than resolved attribute by attribute, so that
/// every lookup below walks the *same* chain — a build that re-walked per
/// attribute could take `x1` from one ancestry and `x2` from another if the
/// chain changed shape halfway, which is a picture nobody could explain.
fn chain(tree: &Tree, start: usize) -> Vec<usize> {
    let mut out = vec![start];
    let mut at = start;
    while out.len() < MAX_HREF_CHAIN {
        let Some(href) = tree.nodes[at].href() else {
            break;
        };
        let Some(name) = href.trim().strip_prefix('#') else {
            break;
        };
        let Some(next) = tree.by_id(name) else {
            break;
        };
        if !is_paint_server(&tree.nodes[next]) {
            break;
        }
        out.push(next);
        at = next;
    }
    out
}

/// Whether an element is one of the two gradients this build reads.
fn is_paint_server(node: &Node) -> bool {
    node.is_svg() && matches!(node.name.as_str(), "linearGradient" | "radialGradient")
}

/// The first element of a chain carrying an attribute, and its value.
fn along<'a>(tree: &'a Tree, chain: &[usize], name: &str) -> Option<&'a str> {
    chain
        .iter()
        .find_map(|at| tree.nodes[*at].attr(name))
        .map(str::trim)
}

/// §13.2's units: what a coordinate on a paint server is a coordinate in.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Units {
    /// The user space in force where the *referencing* element is, which is
    /// the element's own matrix.
    UserSpace,
    /// A fraction of the referencing element's bounding box.
    BoundingBox,
}

/// What a gradient resolved to.
pub struct Resolved {
    /// The paint.
    pub paint: Paint,
}

/// Whether resolving the gradient at `at` needs the painted element's box:
/// its `gradientUnits`, along the `xlink:href` chain, are §13.2.3's initial
/// `objectBoundingBox`.
#[must_use]
pub fn measures_box(tree: &Tree, at: usize) -> bool {
    along(tree, &chain(tree, at), "gradientUnits") != Some("userSpaceOnUse")
}

/// Resolves a `<linearGradient>` or `<radialGradient>` into a [`Paint`].
///
/// `matrix` is the referencing element's own matrix into the scene, and
/// `bounds` its bounding box **in its own user space** — which is what
/// `objectBoundingBox` is a fraction of, and why it is the untransformed box
/// rather than the one in the scene.
///
/// `None` when there is nothing to paint with: §13.2.4 makes a gradient with
/// no stops *"as if `none` were specified"*, and one with a single stop a flat
/// colour rather than a ramp.
#[must_use]
pub fn resolve(
    tree: &Tree,
    at: usize,
    matrix: [f64; 6],
    bounds: [f64; 4],
    style: &Style,
) -> Option<Resolved> {
    let chain = chain(tree, at);
    let units = match along(tree, &chain, "gradientUnits") {
        Some("userSpaceOnUse") => Units::UserSpace,
        // §13.2.3's initial value, and the one a file that says nothing means.
        _ => Units::BoundingBox,
    };
    // §13.2.3: inherited along the chain like every other attribute, and
    // `pad` for anything that is not one of the three.
    let spread = match along(tree, &chain, "spreadMethod") {
        Some("reflect") => Spread::Reflect,
        Some("repeat") => Spread::Repeat,
        _ => Spread::Pad,
    };

    // §13.2.3: `gradientTransform` is applied *inside* the units mapping, so
    // a translate on a bounding-box gradient moves it by a fraction of the box
    // rather than by user units.
    let own = along(tree, &chain, "gradientTransform")
        .and_then(transform::list)
        .unwrap_or(IDENTITY);
    let [min_x, min_y, max_x, max_y] = bounds;
    let (width, height) = (max_x - min_x, max_y - min_y);
    let space = match units {
        Units::UserSpace => own,
        Units::BoundingBox => {
            // A box with no area cannot be a fraction of anything, which
            // §13.2.3 answers by not rendering the element at all.
            if !(width > 0.0 && height > 0.0) {
                return None;
            }
            transform::concat(own, [width, 0.0, 0.0, height, min_x, min_y])
        }
    };
    let matrix = transform::concat(space, matrix);

    let stops = stops(tree, &chain, style);
    // §13.2.4: no stops is `none`, and one stop is that colour everywhere.
    if stops.is_empty() {
        return None;
    }

    // A percentage on a paint server is of the unit square under
    // `objectBoundingBox` and of the viewport under `userSpaceOnUse`; the
    // first is the one that matters and it makes `50%` mean `0.5`.
    let basis = match units {
        Units::BoundingBox => 1.0,
        Units::UserSpace => document::diagonal(width.abs(), height.abs()),
    };
    let number = |name: &str, default: f64| -> f64 {
        along(tree, &chain, name)
            .and_then(|text| document::length(text, Some(basis)))
            .unwrap_or(default)
    };

    let paint = if tree.nodes[at].name == "linearGradient" {
        let from = [number("x1", 0.0), number("y1", 0.0)];
        // §13.2.3's initial `x2` is `100%`, which is one in bounding-box units
        // and the viewport's width in user space. Written as the resolved
        // default rather than the string, because a document that states none
        // must not depend on which units it also did not state.
        let to = [number("x2", basis), number("y2", 0.0)];
        Paint::Linear {
            from,
            to,
            matrix,
            stops,
            spread,
        }
    } else {
        let half = basis / 2.0;
        let centre = [number("cx", half), number("cy", half)];
        let radius = number("r", half);
        // §13.2.3: `fx` and `fy` default to `cx` and `cy` — **the resolved
        // ones**, not the initial ones, so a gradient that moves its centre
        // and says nothing about its focus keeps them together.
        let focus = [number("fx", centre[0]), number("fy", centre[1])];
        if radius <= 0.0 {
            // §13.2.3: a zero radius paints the area with the last stop's
            // colour, which is a flat fill rather than nothing.
            let last = stops.last()?;
            return Some(Resolved {
                paint: Paint::Solid(last.colour),
            });
        }
        Paint::Radial {
            centre,
            radius,
            focus,
            matrix,
            stops,
            spread,
        }
    };
    Some(Resolved { paint })
}

/// §13.2.4's stops, from the first element of the chain that has any.
///
/// **All or nothing from one element**, which is §13.2.3's rule and not a
/// simplification: a gradient that references another either states its own
/// complete ramp or takes the other's whole, and a build that merged the two
/// lists would invent a ramp neither file describes.
fn stops(tree: &Tree, chain: &[usize], style: &Style) -> Vec<Stop> {
    let mut out: Vec<Stop> = Vec::new();
    for parent in chain {
        for child in tree.element_children(*parent) {
            let node = &tree.nodes[child];
            if !node.is_svg() || node.name != "stop" {
                continue;
            }
            // A `<stop>`'s own properties, resolved by the same three-source
            // machinery every other element uses — `stop-color` is as likely
            // to be in a `.stN` class as in an attribute.
            let mut own = style.inherit();
            for (name, value) in &node.attributes {
                let lower = name.to_ascii_lowercase();
                if crate::style::PROPERTIES.contains(&lower.as_str()) {
                    own.apply(
                        &lower,
                        &tinker_pdf_css::parser::component_values(
                            tinker_pdf_css::tokenizer::tokenize(value),
                        ),
                    );
                }
            }
            if let Some(text) = node.style.as_deref() {
                for declaration in crate::style::inline_declarations(text) {
                    own.apply(&declaration.name, &declaration.values);
                }
            }
            let offset = node
                .attr("offset")
                .and_then(|text| document::length(text, Some(1.0)))
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
            // §13.2.4: *"each gradient offset value is required to be equal to
            // or greater than the previous"*, and a smaller one is *"adjusted
            // to be equal"*. Not sorted — adjusted, which keeps the stop's
            // position in the list and is a different picture from sorting.
            let offset = out
                .last()
                .map_or(offset, |last: &Stop| offset.max(last.offset));
            out.push(Stop {
                offset,
                colour: own.stop_colour,
                opacity: own.stop_opacity,
            });
        }
        if !out.is_empty() {
            break;
        }
    }
    // §13.2.4: one stop is a flat colour, which is a ramp from it to itself.
    if out.len() == 1 {
        let only = out[0];
        out.push(Stop {
            offset: 1.0,
            ..only
        });
    }
    out
}

/// A `<clipPath>` read for one element: the union of its shapes, and the
/// text that a silhouette has to be drawn for.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipSource {
    /// The `<clipPath>` element.
    pub at: usize,
    /// Its shapes, and the shapes its `<use>` children name, as one outline.
    pub clip: Clip,
    /// Its `<text>` children, and the `<text>` its `<use>` children name:
    /// each as the element to walk, the matrix to walk it under, and the
    /// element whose style it inherits — the `<clipPath>`, or the `<use>`.
    ///
    /// A glyph's outline is a font's, which this crate does not have
    /// (ruling 8), so text cannot join [`ClipSource::clip`]'s outline here; the
    /// walk draws it as a silhouette instead.
    pub text: Vec<(usize, [f64; 6], usize)>,
    /// Whether a child was something §14.3.5 does not let a clipping path
    /// hold — a `<g>`, an `<image>`, or a `<use>` naming anything but a shape
    /// or text — and contributed nothing.
    pub ignored: bool,
}

/// Whether the `<clipPath>` a reference names holds text, directly or
/// through a `<use>` — which decides whether it can be an outline at all.
#[must_use]
pub fn clip_holds_text(tree: &Tree, name: &str) -> bool {
    let Some(at) = tree
        .by_id(name)
        .filter(|at| tree.nodes[*at].is_svg() && tree.nodes[*at].name == "clipPath")
    else {
        return false;
    };
    tree.element_children(at).any(|child| {
        let node = &tree.nodes[child];
        node.is_svg()
            && (node.name == "text"
                || (node.name == "use"
                    && use_target(tree, node)
                        .is_some_and(|target| tree.nodes[target].name == "text")))
    })
}

/// Whether the `<clipPath>` a reference names is in `objectBoundingBox` units,
/// so that resolving it needs the clipped element's box.
#[must_use]
pub fn clip_measures_box(tree: &Tree, name: &str) -> bool {
    tree.by_id(name)
        .map(|at| &tree.nodes[at])
        .filter(|node| node.is_svg() && node.name == "clipPath")
        .and_then(|node| node.attr("clipPathUnits"))
        .is_some_and(|units| units.trim() == "objectBoundingBox")
}

/// The SVG element a `<use>` names, if any.
fn use_target(tree: &Tree, node: &Node) -> Option<usize> {
    node.href()
        .map(str::trim)
        .and_then(|href| href.strip_prefix('#'))
        .and_then(|id| tree.by_id(id))
        .filter(|target| tree.nodes[*target].is_svg())
}

/// An element's own `transform`, applied inside `outer`.
fn own_matrix(node: &Node, outer: [f64; 6]) -> [f64; 6] {
    node.attr("transform")
        .and_then(transform::list)
        .map_or(outer, |own| transform::concat(own, outer))
}

/// §14.3's `<clipPath>`, as one outline in the scene's space and the text it
/// holds.
///
/// `matrix` and `bounds` mean what they mean for a gradient: the referencing
/// element's own matrix, and its bounding box in its own user space for
/// `clipPathUnits="objectBoundingBox"`.
///
/// §14.3.5's content model is shapes, `<text>` and `<use>`, and a `<use>` there
/// *"must directly reference 'path', 'text' or basic shapes elements"*. One
/// that does is that element, placed as §5.6 places it — the `<use>`'s
/// `transform`, then its `x` and `y` — and one that does not, or a child of
/// another kind, contributes nothing and is [`ClipSource::ignored`].
///
/// Every segment of the outline is spent from `segments`, the walk's budget:
/// a clip is rebuilt for each element that names it, and a `<use>` makes one
/// element's geometry count as often as the clips that name it are used.
///
/// `Ok(None)` when the reference names no `<clipPath>` at all. An **empty**
/// `<clipPath>` is `Some` with an empty outline, and the difference matters:
/// §14.3.5 says a clipping path with nothing in it clips everything away, so a
/// build that returned `None` for both would draw the element unclipped.
///
/// # Errors
///
/// [`Refusal::TooManySegments`] when the outline would pass `segments`.
pub fn clip(
    tree: &Tree,
    name: &str,
    matrix: [f64; 6],
    bounds: [f64; 4],
    style: &Style,
    segments: &mut usize,
) -> Result<Option<ClipSource>, Refusal> {
    let Some(at) = tree.by_id(name) else {
        return Ok(None);
    };
    let node = &tree.nodes[at];
    if !node.is_svg() || node.name != "clipPath" {
        return Ok(None);
    }
    let mut source = ClipSource {
        at,
        clip: Clip {
            outline: Outline::default(),
            rule: style.clip_rule,
        },
        text: Vec::new(),
        ignored: false,
    };
    let [min_x, min_y, max_x, max_y] = bounds;
    let (width, height) = (max_x - min_x, max_y - min_y);
    let base = match node.attr("clipPathUnits").map(str::trim) {
        Some("objectBoundingBox") => {
            if !(width > 0.0 && height > 0.0) {
                return Ok(Some(source));
            }
            transform::concat([width, 0.0, 0.0, height, min_x, min_y], matrix)
        }
        // §14.3.4's initial value.
        _ => matrix,
    };
    let base = own_matrix(node, base);

    for child in tree.element_children(at) {
        let element = &tree.nodes[child];
        if !element.is_svg() {
            continue;
        }
        // The viewport a percentage inside a clip path resolves against is the
        // bounding box under `objectBoundingBox` and the user space otherwise;
        // both are already in `base`, so the numbers here are plain.
        let (shape_at, placed) = match element.name.as_str() {
            // Descriptive and animation elements are in the content model and
            // draw nothing — not ignored, because nothing was asked for.
            "title" | "desc" | "metadata" | "animate" | "animateColor" | "animateMotion"
            | "animateTransform" | "set" => continue,
            "text" => {
                source.text.push((child, base, at));
                continue;
            }
            "use" => {
                let Some(target) = use_target(tree, element) else {
                    source.ignored = true;
                    continue;
                };
                let length = |name: &str, basis: f64| {
                    element
                        .attr(name)
                        .and_then(|text| document::length(text, Some(basis)))
                        .unwrap_or(0.0)
                };
                // §5.6: the `<use>`'s own `transform`, then a translation by
                // its `x` and `y`, then the referenced element's `transform`.
                let placed = transform::concat(
                    [1.0, 0.0, 0.0, 1.0, length("x", width), length("y", height)],
                    own_matrix(element, base),
                );
                if tree.nodes[target].name == "text" {
                    source.text.push((target, placed, child));
                    continue;
                }
                (target, placed)
            }
            _ => (child, base),
        };
        let mut degraded = Vec::new();
        let shape_node = &tree.nodes[shape_at];
        match shape::outline(shape_node, (width, height), &mut degraded) {
            Some(Shape::Outline(shape)) => {
                let count = shape.segments.len();
                if count > *segments {
                    return Err(Refusal::TooManySegments);
                }
                *segments -= count;
                source
                    .clip
                    .outline
                    .segments
                    .extend(shape.transformed(own_matrix(shape_node, placed)).segments);
            }
            // A shape with nothing to draw, or an attribute it could not read,
            // is a shape that adds no silhouette: the walk names an unreadable
            // one where it is drawn, and a clip is not where it is drawn.
            Some(Shape::Nothing | Shape::Unreadable(_)) => {}
            None => source.ignored = true,
        }
    }
    Ok(Some(source))
}

/// §7.11's bounding box of a **container**, from what it drew.
///
/// The nodes are already in the scene's space, so `inverse` — the inverse of
/// the container's own matrix — brings every point back into the container's
/// user space, which is what `objectBoundingBox` on a `<g>` is a fraction of.
/// Text is not measured here: its extent is a font metric this crate does not
/// have (ruling 8), so a group of text alone has the empty box. That is
/// **not** §13.2.3's zero-area answer — the text has an extent, this crate
/// cannot take it — so a caller asks [`nodes_hold_text`] as well, and a
/// bounding-box mask, clip or paint on text is not resolved against nothing
/// but named (`Warning::TextBoxUnmeasured`) — unless the walk's caller
/// measured the runs, whose cells then come in through [`nodes_bounds_with`].
///
/// Markers are not in §7.11's box either, and a container's nodes do not say
/// which of them a marker drew: a group holding a marked path measures the
/// markers with it. A shape's own `mask` or text-holding `clip-path` is
/// measured from the shape's geometry alone, where the walk has it.
#[must_use]
pub fn nodes_bounds(nodes: &[crate::Node], inverse: [f64; 6]) -> [f64; 4] {
    nodes_bounds_with(nodes, &[], inverse)
}

/// [`nodes_bounds`], with `cells` — points in the scene's space, the corners
/// of text runs a caller measured — counted with the nodes' own.
#[must_use]
pub fn nodes_bounds_with(nodes: &[crate::Node], cells: &[[f64; 2]], inverse: [f64; 6]) -> [f64; 4] {
    let mut points: Vec<[f64; 2]> = cells.to_vec();
    gather(nodes, &mut points);
    let mut out = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    let mut seen = false;
    for point in points {
        let [x, y] = transform::apply(inverse, point);
        if !(x.is_finite() && y.is_finite()) {
            continue;
        }
        seen = true;
        out[0] = out[0].min(x);
        out[1] = out[1].min(y);
        out[2] = out[2].max(x);
        out[3] = out[3].max(y);
    }
    if seen {
        out
    } else {
        [0.0, 0.0, 0.0, 0.0]
    }
}

/// Whether a list of nodes draws text that [`nodes_bounds`] left out of its
/// box — at any depth of group, as the box itself is gathered.
#[must_use]
pub fn nodes_hold_text(nodes: &[crate::Node]) -> bool {
    nodes.iter().any(|node| match node {
        crate::Node::Text { .. } => true,
        crate::Node::Group { nodes, .. } => nodes_hold_text(nodes),
        crate::Node::Path { .. } | crate::Node::Image { .. } => false,
    })
}

/// Every point a list of nodes visits, in the scene's space.
fn gather(nodes: &[crate::Node], points: &mut Vec<[f64; 2]>) {
    use crate::path::Segment;
    for node in nodes {
        match node {
            crate::Node::Path { outline, .. } => {
                for segment in &outline.segments {
                    match *segment {
                        Segment::Move(p) | Segment::Line(p) => points.push(p),
                        Segment::Cubic(a, b, c) => points.extend([a, b, c]),
                        Segment::Close => {}
                    }
                }
            }
            crate::Node::Image { rect, matrix, .. } => {
                let [x, y, width, height] = *rect;
                for corner in [
                    [x, y],
                    [x + width, y],
                    [x, y + height],
                    [x + width, y + height],
                ] {
                    points.push(transform::apply(*matrix, corner));
                }
            }
            crate::Node::Group { nodes, .. } => gather(nodes, points),
            crate::Node::Text { .. } => {}
        }
    }
}

/// The bounding box of an outline in its own space, as `[min_x, min_y, max_x,
/// max_y]`.
///
/// **The control points' box, not the curve's.** §7.11 defines the object
/// bounding box as the geometry's, which for a cubic is the tighter of the
/// two; taking the control hull makes a bounding-box gradient over a curved
/// shape slightly larger than it should be. It is written down rather than
/// hidden because it is a visible difference on a shape that is mostly curve,
/// and the exact box needs the derivative's roots per segment.
#[must_use]
pub fn bounds(outline: &Outline) -> [f64; 4] {
    use crate::path::Segment;
    let mut out = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    let mut seen = false;
    let mut see = |point: [f64; 2]| {
        seen = true;
        out[0] = out[0].min(point[0]);
        out[1] = out[1].min(point[1]);
        out[2] = out[2].max(point[0]);
        out[3] = out[3].max(point[1]);
    };
    for segment in &outline.segments {
        match *segment {
            Segment::Move(p) | Segment::Line(p) => see(p),
            Segment::Cubic(a, b, c) => {
                see(a);
                see(b);
                see(c);
            }
            Segment::Close => {}
        }
    }
    if seen {
        out
    } else {
        [0.0, 0.0, 0.0, 0.0]
    }
}
