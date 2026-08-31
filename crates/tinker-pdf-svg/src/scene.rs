//! The walk that turns an element tree into a [`Scene`].
//!
//! # Document order is paint order
//!
//! SVG 1.1 §3.3: *"Elements … are painted in the order in which they appear"*,
//! and there is no `z-index` to disturb it. So this is a depth-first walk that
//! pushes as it goes, and the vector it produces is already the order a
//! consumer must draw in — which is why [`Scene::nodes`] is a `Vec` and not a
//! tree. A consumer that had to re-derive paint order would be re-deriving
//! something the walk already knew.
//!
//! # Transforms are composed here, once
//!
//! Every point in a finished [`Scene`] is in the space the scene states, with
//! `viewBox`, every ancestor's `transform` and every nested viewport already
//! multiplied in. The alternative — a tree of nodes each carrying its own
//! matrix — moves the multiplication to the consumer, and this repository would
//! then have two of them: this crate's, for its own tests, and the facade's,
//! for the page. Ruling 4 says the one that survives is the one a test reaches.
//!
//! # What is refused is refused **by name**
//!
//! A filter, a mask, an animation, a script and a `<foreignObject>` are each a
//! whole subsystem, and each is a [`Warning`] naming itself rather than an
//! element quietly skipped. The difference is what a caller can report: "this
//! picture is incomplete and here is what is missing" against "this picture".

use tinker_pdf_css::{Budget as CssBudget, Limits as CssLimits};

use crate::document::{self, Node, Tree};
use crate::shape::{self, Shape};
use crate::style::{self, PaintSpec, Sheet, Style};
use crate::transform::{self, IDENTITY};
use crate::{Limits, Paint, Refusal, Scene, Stroke, Warning};

/// The default viewport, in user units, for a document that states no size.
///
/// SVG 1.1 §7.2 leaves the viewport to the referencing context and CSS 2.1
/// §10.3.2 makes a replaced element with no intrinsic size 300 by 150. Every
/// browser uses that pair for a bare `<svg>` with no attributes, so a caller
/// that passes no viewport gets the size everything else would give it.
pub const DEFAULT_VIEWPORT: (f64, f64) = (300.0, 150.0);

/// The state that flows down the tree.
///
/// Copied at each element rather than pushed and popped, because an SVG's
/// inheritance is per-element and a stack that has to be unwound is a stack
/// that can be unwound wrong — the classic defect being an early `continue`
/// that skips the pop.
#[derive(Clone, Debug)]
struct Frame {
    /// The composed matrix from this element's own space to the scene's.
    matrix: [f64; 6],
    /// The viewport this element's percentages resolve against.
    viewport: (f64, f64),
    /// The **resolved** style of the element above, which is what a child
    /// inherits from. Resolved once on the way down and never looked back up.
    style: Style,
}

/// The walk's own state: what it has spent and what it has to say.
struct Walk<'a> {
    tree: &'a Tree,
    limits: &'a Limits,
    scene: Scene,
    /// [`Limits::max_segments`], spent across the **whole document** and never
    /// refunded — so a thousand paths of a thousand segments each is the same
    /// refusal as one path of a million, which is the property a per-path cap
    /// does not have.
    segments: usize,
    /// Every `<style>` element of the document, read once before the walk.
    sheet: Sheet,
    /// `tinker-pdf-css`'s own budget, which bounds selector matching.
    css: CssBudget,
}

impl Walk<'_> {
    /// Charges `count` segments, refusing by name when the document is past
    /// its budget.
    fn spend(&mut self, count: usize) -> Result<(), Refusal> {
        if count > self.segments {
            return Err(Refusal::TooManySegments);
        }
        self.segments -= count;
        Ok(())
    }

    /// Adds one node to the scene, refusing by name when it is full.
    fn push(&mut self, node: crate::Node) -> Result<(), Refusal> {
        if self.scene.nodes.len() >= self.limits.max_nodes {
            return Err(Refusal::TooManyNodes);
        }
        self.scene.nodes.push(node);
        Ok(())
    }

    /// Records a warning once, whatever it names.
    ///
    /// **Deduplicated, and capped.** Ruling 10 wants the fact reported; it does
    /// not want it reported four hundred times, which is
    /// `tinker_pdf_css::parser::Report`'s doctrine one crate over. The cap is
    /// what stops a document of half a million distinct unknown element names
    /// from turning a warning list into the document's memory footprint —
    /// [`Limits::max_warnings`] and its own comment.
    fn warn(&mut self, warning: Warning) {
        if self.scene.warnings.contains(&warning) {
            return;
        }
        if self.scene.warnings.len() >= self.limits.max_warnings {
            return;
        }
        self.scene.warnings.push(warning);
    }

    /// Reads a `transform` attribute, warning by name when it is not §7.6's
    /// grammar.
    fn matrix_of(&mut self, node: &Node, outer: [f64; 6]) -> [f64; 6] {
        let Some(text) = node.attr("transform") else {
            return outer;
        };
        match transform::list(text) {
            Some(matrix) => transform::concat(matrix, outer),
            None => {
                self.warn(Warning::ValueUnreadable {
                    attribute: "transform".to_owned(),
                });
                outer
            }
        }
    }

    /// A `<length>` attribute, or a default, warning by name when it is
    /// present and unreadable.
    fn length_of(&mut self, node: &Node, name: &str, basis: Option<f64>, default: f64) -> f64 {
        let Some(text) = node.attr(name) else {
            return default;
        };
        match document::length(text, basis) {
            Some(value) => value,
            None => {
                self.warn(Warning::ValueUnreadable {
                    attribute: name.to_owned(),
                });
                default
            }
        }
    }

    /// §7.7's `viewBox`, as the matrix mapping it into a viewport.
    ///
    /// Three answers rather than two, and the third is §7.7's own: a view box
    /// that is not four numbers is an unreadable attribute and the element is
    /// drawn unmapped, while a view box whose width or height is zero
    /// *"disables rendering of the element"* — which is `None` here and an
    /// empty subtree at the call site.
    fn view_box_of(&mut self, node: &Node, width: f64, height: f64) -> Result<[f64; 6], Disabled> {
        let Some(text) = node.attr("viewBox") else {
            return Ok(IDENTITY);
        };
        let Some(numbers) = transform::numbers(text) else {
            self.warn(Warning::ValueUnreadable {
                attribute: "viewBox".to_owned(),
            });
            return Ok(IDENTITY);
        };
        let [min_x, min_y, box_width, box_height] = numbers[..] else {
            self.warn(Warning::ValueUnreadable {
                attribute: "viewBox".to_owned(),
            });
            return Ok(IDENTITY);
        };
        let preserve = node.attr("preserveAspectRatio");
        match transform::view_box(
            [min_x, min_y, box_width, box_height],
            width,
            height,
            preserve,
        ) {
            Some(matrix) => Ok(matrix),
            None => Err(Disabled),
        }
    }

    /// Walks one element and everything under it.
    fn element(&mut self, index: usize, outer: &Frame) -> Result<(), Refusal> {
        let Some(node) = self.tree.nodes.get(index) else {
            return Ok(());
        };
        if node.depth >= self.limits.max_depth {
            return Err(Refusal::TooDeep);
        }
        // §11.5's `display` is structural: `none` removes the element **and its
        // children** from the rendering tree, which is a different thing from
        // `visibility: hidden` — that one is a property a child can turn back
        // on, and this one is not. Read as an attribute *and* as a property,
        // because `style="display:none"` is how half the corpus spells it.
        if self.display_none(node) {
            return Ok(());
        }
        if !node.is_svg() {
            self.warn(Warning::ElementUnknown(node.name.clone()));
            return Ok(());
        }

        // §6.4's three sources, resolved against the parent's own resolved
        // style. Done before the element is dispatched, because a container
        // resolves a style for its children even though it paints nothing.
        let resolved = style::resolve(self.tree, index, &self.sheet, &outer.style, &mut self.css)
            .map_err(|_| Refusal::TooMuchStyle)?;
        for name in resolved.unreadable {
            self.warn(Warning::ValueUnreadable { attribute: name });
        }
        let frame = Frame {
            style: resolved.style,
            ..outer.clone()
        };
        let frame = &frame;

        match node.name.as_str() {
            // ---- containers ------------------------------------------------
            "svg" => self.viewport_element(index, node, frame),
            "g" | "a" => {
                let inner = Frame {
                    matrix: self.matrix_of(node, frame.matrix),
                    ..frame.clone()
                };
                self.children(index, &inner)
            }
            // §5.5: `<defs>` is never rendered where it stands. Its contents
            // are reached by reference and nowhere else, so walking into it
            // here would draw every gradient's own geometry twice.
            "defs" => Ok(()),
            // §5.4 and §5.12: content for a reader rather than for a raster.
            // Not a warning, because nothing was refused — the specification
            // says these do not paint.
            "title" | "desc" | "metadata" => Ok(()),

            // ---- §9's basic shapes -----------------------------------------
            "path" | "rect" | "circle" | "ellipse" | "line" | "polyline" | "polygon" => {
                self.shape(node, frame)
            }
            // §5.7's `<image>`, carried unresolved: this crate has no
            // container, no filesystem and no network, which is what makes it
            // a leaf.
            "image" => self.image(node, frame),

            // ---- refused by name -------------------------------------------
            "filter" => {
                self.warn(Warning::FilterUnsupported);
                Ok(())
            }
            "mask" => {
                self.warn(Warning::MaskUnsupported);
                Ok(())
            }
            "clipPath" => {
                self.warn(Warning::ClipPathUnsupported);
                Ok(())
            }
            "pattern" => {
                self.warn(Warning::PatternUnsupported);
                Ok(())
            }
            "marker" => {
                self.warn(Warning::MarkerUnsupported);
                Ok(())
            }
            "foreignObject" => {
                self.warn(Warning::ForeignObjectUnsupported);
                Ok(())
            }
            "animate" | "animateColor" | "animateMotion" | "animateTransform" | "set" | "mpath" => {
                self.warn(Warning::AnimationIgnored);
                Ok(())
            }
            "script" => {
                self.warn(Warning::ScriptIgnored);
                Ok(())
            }
            other => {
                self.warn(Warning::ElementUnknown(other.to_owned()));
                Ok(())
            }
        }
    }

    /// One of §9's basic shapes, as a node in the scene's own space.
    ///
    /// **The transform is applied here rather than carried**, which is the
    /// design's one irreversible decision: a consumer receives points and never
    /// a matrix. See this module's header.
    fn shape(&mut self, node: &Node, frame: &Frame) -> Result<(), Refusal> {
        let matrix = self.matrix_of(node, frame.matrix);
        let mut degraded = Vec::new();
        let read = shape::outline(node, frame.viewport, &mut degraded);
        for attribute in degraded {
            self.warn(Warning::ValueUnreadable {
                attribute: attribute.to_owned(),
            });
        }
        let outline = match read {
            Some(Shape::Outline(outline)) => outline,
            Some(Shape::Nothing) | None => return Ok(()),
            Some(Shape::Unreadable(attribute)) => {
                self.warn(Warning::ValueUnreadable {
                    attribute: attribute.to_owned(),
                });
                return Ok(());
            }
        };
        self.spend(outline.segments.len())?;
        let style = &frame.style;
        // §11.5: a hidden element is laid out and not painted, which for a
        // display list means it is not in it. Distinct from `display: none`
        // only in that a descendant could have turned it back on, and a
        // shape has none.
        if !style.visible {
            return Ok(());
        }
        let fill = self.paint(&style.fill, style);
        let stroke_paint = self.paint(&style.stroke, style);
        // §11.4: a stroke with no paint, no width or a zero width puts no ink
        // on the page. Answered here rather than carried, so a consumer never
        // has to decide whether a `Stroke` of width zero draws.
        let stroke = if stroke_paint == Paint::None || style.stroke_width <= 0.0 {
            None
        } else {
            Some(Stroke {
                paint: stroke_paint,
                width: style.stroke_width,
                cap: style.cap,
                join: style.join,
                miter_limit: style.miter_limit,
                dashes: style.dashes.clone(),
                dash_offset: style.dash_offset,
                opacity: (style.stroke_opacity * style.opacity).clamp(0.0, 1.0),
            })
        };
        // §14.5's group opacity, flattened into each descendant's own alpha.
        // Named where it is observable: a shape painted **twice** — once
        // filled and once stroked — composites the two against each other
        // before the group is faded, so the overlap is darker here than §14.5
        // asks for. A shape painted once is exact and says nothing.
        if style.opacity < 1.0 && fill != Paint::None && stroke.is_some() {
            self.warn(Warning::GroupOpacityFlattened);
        }
        self.push(crate::Node::Path {
            outline: outline.transformed(matrix),
            fill,
            rule: style.fill_rule,
            fill_opacity: (style.fill_opacity * style.opacity).clamp(0.0, 1.0),
            stroke,
        })
    }

    /// §13.2's `<paint>`, with a `url(#name)` resolved against the document.
    ///
    /// A reference that names nothing this build can paint with falls through
    /// to **the paint's own fallback** — which is §13.2's answer and not an
    /// invention: a file that wrote `fill="url(#g) red"` said what to do when
    /// the server is missing, and a build that drew nothing would be ignoring
    /// the half of the value that was for exactly this.
    fn paint(&mut self, spec: &PaintSpec, style: &Style) -> Paint {
        match spec {
            PaintSpec::None => Paint::None,
            PaintSpec::Solid(colour) => Paint::Solid(*colour),
            PaintSpec::Current => Paint::Solid(style.colour),
            PaintSpec::Reference(name, fallback) => {
                let target = self
                    .tree
                    .by_id(name)
                    .map(|at| self.tree.nodes[at].name.clone());
                match target.as_deref() {
                    Some("pattern") => self.warn(Warning::PatternUnsupported),
                    _ => self.warn(Warning::PaintServerUnresolved),
                }
                self.paint(fallback, style)
            }
        }
    }

    /// Whether §11.5's `display: none` applies, from either place it is
    /// written.
    ///
    /// The attribute **and** `style="display:none"`, because both are in the
    /// wild and a build that read only the first would draw what an Inkscape
    /// file hid. It is not resolved through the cascade with the rest, because
    /// `display` decides whether the cascade runs at all for the subtree —
    /// asking for a style in order to find out whether to ask for a style is
    /// the shape a first draft gets wrong.
    fn display_none(&self, node: &Node) -> bool {
        if node
            .attr("display")
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("none"))
        {
            return true;
        }
        node.style.as_deref().is_some_and(|text| {
            text.split(';').any(|piece| {
                let Some((name, value)) = piece.split_once(':') else {
                    return false;
                };
                name.trim().eq_ignore_ascii_case("display")
                    && value.trim().eq_ignore_ascii_case("none")
            })
        })
    }

    /// §5.7's `<image>`.
    fn image(&mut self, node: &Node, frame: &Frame) -> Result<(), Refusal> {
        let Some(href) = node.href() else {
            // §5.7 makes the reference required; without one there is nothing
            // to resolve and nothing was refused.
            return Ok(());
        };
        let matrix = self.matrix_of(node, frame.matrix);
        let x = self.length_of(node, "x", Some(frame.viewport.0), 0.0);
        let y = self.length_of(node, "y", Some(frame.viewport.1), 0.0);
        let width = self.length_of(node, "width", Some(frame.viewport.0), 0.0);
        let height = self.length_of(node, "height", Some(frame.viewport.1), 0.0);
        // §5.7: a zero or negative width or height disables rendering.
        if !(width > 0.0 && height > 0.0) {
            return Ok(());
        }
        self.push(crate::Node::Image {
            href: href.to_owned(),
            rect: [x, y, width, height],
            matrix,
            preserve: node.attr("preserveAspectRatio").map(str::to_owned),
        })
    }

    /// An `<svg>`, root or nested: §7.9's establishment of a new viewport.
    fn viewport_element(
        &mut self,
        index: usize,
        node: &Node,
        frame: &Frame,
    ) -> Result<(), Refusal> {
        let (outer_width, outer_height) = frame.viewport;
        // A nested `<svg>`'s `x` and `y` place its viewport inside its
        // parent's; the root's are ignored by §7.2 and are zero on every file
        // that has them, so one code path answers both.
        let x = self.length_of(node, "x", Some(outer_width), 0.0);
        let y = self.length_of(node, "y", Some(outer_height), 0.0);
        let width = self.length_of(node, "width", Some(outer_width), outer_width);
        let height = self.length_of(node, "height", Some(outer_height), outer_height);
        // §7.7: a viewport with no area draws nothing, and neither does
        // anything inside it.
        if !(width > 0.0 && height > 0.0) {
            return Ok(());
        }
        let mapping = match self.view_box_of(node, width, height) {
            Ok(matrix) => matrix,
            Err(Disabled) => return Ok(()),
        };
        let placed = transform::concat(mapping, [1.0, 0.0, 0.0, 1.0, x, y]);
        let inner = Frame {
            matrix: transform::concat(placed, frame.matrix),
            // §7.9: percentages inside a nested viewport are of **that**
            // viewport. A build that kept the outer one would size a nested
            // `<svg>`'s children against the page.
            viewport: (width, height),
            style: frame.style.clone(),
        };
        self.children(index, &inner)
    }

    /// Every child element of `index`, in document order.
    fn children(&mut self, index: usize, frame: &Frame) -> Result<(), Refusal> {
        let children: Vec<usize> = self.tree.element_children(index).collect();
        for child in children {
            self.element(child, frame)?;
        }
        Ok(())
    }
}

/// §7.7's "rendering of the element is disabled".
struct Disabled;

/// Builds a scene from a tree, against a viewport in user units.
///
/// `viewport` is what a percentage on the root resolves against and what the
/// root falls back to when it states no size of its own — the page box, for the
/// one caller in this repository. `None` is [`DEFAULT_VIEWPORT`].
///
/// # Errors
/// [`Refusal::TooDeep`], [`Refusal::TooManyNodes`] and
/// [`Refusal::TooManySegments`] are the three ceilings a document can cross.
pub fn build(tree: &Tree, viewport: Option<(f64, f64)>, limits: &Limits) -> Result<Scene, Refusal> {
    let viewport = match viewport {
        Some((width, height))
            if width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0 =>
        {
            (width, height)
        }
        // A caller's unusable viewport is the caller's mistake and not the
        // document's, so it is replaced rather than refused — ruling 2, at the
        // one place this crate is handed a number by somebody other than the
        // file.
        _ => DEFAULT_VIEWPORT,
    };
    // §6.2's `<style>` elements, read once for the document rather than once
    // per element: two of them are one author stylesheet in source order, and
    // that order is what `css-cascade-5` §6.1's last criterion compares.
    let css_limits = CssLimits::DEFAULT;
    let sheet = style::sheet(tree, css_limits.max_selector_parts);
    let mut walk = Walk {
        tree,
        limits,
        scene: Scene::default(),
        segments: limits.max_segments,
        css: CssBudget::new(&css_limits),
        sheet,
    };
    if walk.sheet.at_rules > 0 {
        walk.warn(Warning::AtRuleIgnored);
    }
    let root = tree.root;
    let Some(node) = tree.nodes.get(root) else {
        return Err(Refusal::NotAnSvg);
    };
    // The root's own size, which is the scene's — resolved before the walk,
    // because a caller needs it whether or not anything drew.
    let width = walk.length_of(node, "width", Some(viewport.0), viewport.0);
    let height = walk.length_of(node, "height", Some(viewport.1), viewport.1);
    walk.element(
        root,
        &Frame {
            matrix: IDENTITY,
            viewport,
            style: Style::default(),
        },
    )?;
    let mut scene = walk.scene;
    // The size the root stated, not the size the walk happened to leave
    // behind: a nested `<svg>` sets `Frame::viewport` and must not be able to
    // change what the document says it is.
    scene.size = (width.max(0.0), height.max(0.0));
    Ok(scene)
}
