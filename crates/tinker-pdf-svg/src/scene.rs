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

use crate::document::Child;
use crate::document::{self, Node, Tree};
use crate::gradient;
use crate::marker::{self, Orient};
use crate::path::{self, Outline, Segment};
use crate::shape::{self, Shape};
use crate::style::{self, PaintSpec, Sheet, Style};
use crate::transform::{self, IDENTITY};
use crate::{Colour, Limits, Paint, Refusal, Scene, Stroke, TextStyle, Warning};
use tinker_pdf_math as math;

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
    /// How deep the *walk* is, which a `<use>` makes different from how deep
    /// the element sits in the tree.
    ///
    /// `Node::depth` is where an element is written; this is where it is being
    /// drawn. A `<use>` of a subtree three deep, from six deep, draws at nine —
    /// and a build that capped on the first number would let a chain of
    /// `<use>`s nest without bound while every element in it looked shallow.
    depth: usize,
}

/// §7.11's object bounding box of one element, as far as this crate can take
/// it — which is everything but text.
///
/// Two facts rather than one box, because "the box is empty" and "the box is
/// text's" are different answers: the first is §13.2.3's zero-area rule, which
/// disables a bounding-box effect, and the second is a font metric this crate
/// does not have (ruling 8), which a `mask`, a `clip-path` or a paint server
/// must not read as a box of nothing and take the ink away with.
#[derive(Clone, Copy, Debug)]
struct Extent {
    /// `[min_x, min_y, max_x, max_y]` of the element's shapes and pictures,
    /// in its own user space.
    measured: [f64; 4],
    /// Whether text was left out of `measured`.
    text: bool,
}

impl Extent {
    /// A run of text, of which nothing is measured.
    const TEXT: Self = Self {
        measured: [0.0; 4],
        text: true,
    };

    /// A shape's own geometry, which has no text in it.
    fn shape(measured: [f64; 4]) -> Self {
        Self {
            measured,
            text: false,
        }
    }

    /// Whether the measured box has an area to take a fraction of.
    fn has_area(self) -> bool {
        let [min_x, min_y, max_x, max_y] = self.measured;
        max_x - min_x > 0.0 && max_y - min_y > 0.0
    }
}

/// What a `<pattern>` comes to for one element.
enum Tiling {
    /// Its tiles.
    Paint(Paint),
    /// Nothing: a tile with no area, which §13.3 says disables the paint.
    Disabled,
    /// No box to take a fraction of — text's, which this crate cannot
    /// measure — so the paint's own fallback stands.
    Unmeasured,
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
    /// §10.4's current text position, as far as it is a fact about the
    /// document rather than about a font. See [`Text`].
    text: Text,
    /// [`Limits::max_uses`], spent across the whole document.
    uses: usize,
    /// The `<use>` targets currently being expanded, innermost last.
    ///
    /// **This is the cycle guard and it is the only one.** A `<use>` whose
    /// target is an ancestor of itself is the classic SVG bomb, and the
    /// obvious check — "is the target an ancestor of the `<use>`?" — is the
    /// *special case*: expanding the target re-reaches the `<use>`, which
    /// names the target again, and the target is already on this stack. An
    /// indirect cycle through three elements has no ancestor relation at all
    /// and is caught here just the same. Milestone 4's injection matrix is why
    /// there is one rule rather than two.
    expanding: Vec<usize>,
    /// `tinker-pdf-css`'s own budget, which bounds selector matching.
    css: CssBudget,
    /// Nodes pushed so far, at every depth.
    ///
    /// Counted rather than read off [`Scene::nodes`], because a
    /// [`crate::Node::Group`] is assembled in a list of its own and moved into
    /// its parent whole — so the length of whichever list is open is not how
    /// much of one picture the walk has built, and [`Limits::max_nodes`] is.
    pushed: usize,
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

    /// Adds one node to the list being built, refusing by name when the
    /// scene is full — and dropping it, named, when composing the transforms
    /// above it carried a number past a double's range.
    fn push(&mut self, node: crate::Node) -> Result<(), Refusal> {
        if !self.admits(&node) {
            return Ok(());
        }
        self.charge()?;
        self.scene.nodes.push(node);
        Ok(())
    }

    /// Whether every number a node carries is finite, naming the node that is
    /// not.
    ///
    /// Every number this crate *reads* is finite — `1e999` is refused where
    /// it is parsed — but a product of finite numbers is not: `scale(1e300)`
    /// inside `scale(1e300)` is two legal transforms and an infinity, and an
    /// infinity in a coordinate is a rasterizer with nothing to draw in a file
    /// that looked ordinary. So the check is on what comes **out**, once per
    /// node: a group's own opacity and clip, since its children were admitted
    /// one by one on the way in.
    fn admits(&mut self, node: &crate::Node) -> bool {
        if finite(node) {
            return true;
        }
        self.warn(Warning::GeometryOverflow);
        false
    }

    /// Spends one node of [`Limits::max_nodes`].
    fn charge(&mut self) -> Result<(), Refusal> {
        if self.pushed >= self.limits.max_nodes {
            return Err(Refusal::TooManyNodes);
        }
        self.pushed += 1;
        Ok(())
    }

    /// Runs `body` with a fresh list open, and hands back what it pushed.
    ///
    /// The list it replaces is put back whatever `body` returned, so a refusal
    /// halfway through a group cannot leave the walk writing into the group's
    /// list — the stack-that-can-be-unwound-wrong shape [`Frame`]'s own note
    /// is about.
    fn collect(
        &mut self,
        body: impl FnOnce(&mut Self) -> Result<(), Refusal>,
    ) -> Result<Vec<crate::Node>, Refusal> {
        let outer = std::mem::take(&mut self.scene.nodes);
        let drawn = body(self);
        let inner = std::mem::replace(&mut self.scene.nodes, outer);
        drawn.map(|()| inner)
    }

    /// An element's rendering as §14.5 and §14.3.5 see it: whatever `body`
    /// draws, faded by the element's own `opacity` and clipped by its own
    /// `clip-path`, **as one**.
    ///
    /// `matrix` is the element's own matrix into the scene, which is the space
    /// a `userSpaceOnUse` clip is in and the one an `objectBoundingBox` clip's
    /// box is measured in. Where neither property is set, `body` draws inline
    /// and no group exists — a group that changes nothing is a transparency
    /// group a reader composites for no reason.
    ///
    /// The box is measured from what `body` drew, which for a container is
    /// §7.11's union of its children's.
    fn group(
        &mut self,
        style: &Style,
        matrix: [f64; 6],
        frame: &Frame,
        body: impl FnOnce(&mut Self) -> Result<(), Refusal>,
    ) -> Result<(), Refusal> {
        self.group_in(style, matrix, frame, None, body)
    }

    /// [`Walk::group`], with the box given where the caller has it: a shape's
    /// own geometry, which is §7.11's box and leaves out the markers `body`
    /// draws with it.
    fn group_in(
        &mut self,
        style: &Style,
        matrix: [f64; 6],
        frame: &Frame,
        own: Option<[f64; 4]>,
        body: impl FnOnce(&mut Self) -> Result<(), Refusal>,
    ) -> Result<(), Refusal> {
        let opacity = style.opacity;
        if opacity >= 1.0 && style.clip_path.is_none() && style.mask.is_none() {
            return body(self);
        }
        let mut nodes = self.collect(body)?;
        // §14.5: an opacity of zero is an element that draws nothing at all.
        // Walked anyway, so that what it would have asked for is still named.
        if nodes.is_empty() || opacity <= 0.0 {
            return Ok(());
        }
        let extent = match own {
            Some(measured) => Extent::shape(measured),
            None => Extent {
                measured: transform::invert(matrix)
                    .map_or([0.0; 4], |inverse| gradient::nodes_bounds(&nodes, inverse)),
                text: gradient::nodes_hold_text(&nodes),
            },
        };
        let source = match &style.clip_path {
            None => None,
            Some(name) => self.clip_of(name, matrix, extent, style)?,
        };
        let (clip, silhouette) = match source {
            Some(source) if !source.text.is_empty() => {
                (None, Some(self.silhouette(source, frame)?))
            }
            Some(source) => (Some(source.clip), None),
            None => (None, None),
        };
        let mask = match &style.mask {
            None => None,
            Some(name) => self.mask_of(name, matrix, extent, frame)?.map(Box::new),
        };
        if let Some(silhouette) = silhouette {
            // A clip that holds text is a mask of its silhouettes, and an
            // element with a mask of its own as well is masked by both — one
            // group inside the other, since a group holds one mask.
            if mask.is_some() {
                self.charge()?;
                nodes = vec![crate::Node::Group {
                    nodes,
                    opacity: 1.0,
                    clip: None,
                    mask,
                }];
            }
            return self.push(crate::Node::Group {
                nodes,
                opacity,
                clip: None,
                mask: Some(Box::new(silhouette)),
            });
        }
        if mask.is_some() {
            return self.push(crate::Node::Group {
                nodes,
                opacity,
                clip,
                mask,
            });
        }
        if clip.is_none() {
            // Every node here was charged when it was pushed into the group's
            // own list, so moving them into the parent's spends nothing.
            if opacity >= 1.0 {
                // The clip named nothing usable, and ruling 2 draws the
                // element unclipped rather than losing it.
                self.scene.nodes.extend(nodes);
                return Ok(());
            }
            if fold(&mut nodes, opacity) {
                self.scene.nodes.extend(nodes);
                return Ok(());
            }
        }
        self.push(crate::Node::Group {
            nodes,
            opacity,
            clip,
            mask: None,
        })
    }

    /// §14.4's `<mask>`, read for one element: its region, and its content
    /// walked into nodes of its own.
    ///
    /// `matrix` and `extent` are the referencing element's, as for a clip:
    /// `maskUnits` (initially `objectBoundingBox`, with the region
    /// −10%/−10%/120%/120%) and `maskContentUnits` (initially
    /// `userSpaceOnUse`) are each a fraction of that box or a length in that
    /// space. `None` is a reference naming no `<mask>`, which ruling 2 draws
    /// unmasked and names — and so is a box that is text's alone, which this
    /// crate cannot measure ([`Walk::measurable`]). A mask whose region has no
    /// area masks everything away, which is §14.4's answer and an empty
    /// [`crate::Mask`] here.
    fn mask_of(
        &mut self,
        name: &str,
        matrix: [f64; 6],
        extent: Extent,
        frame: &Frame,
    ) -> Result<Option<crate::Mask>, Refusal> {
        let tree = self.tree;
        let Some(at) = tree
            .by_id(name)
            .filter(|at| tree.nodes[*at].is_svg() && tree.nodes[*at].name == "mask")
        else {
            self.warn(Warning::MaskUnresolved);
            return Ok(None);
        };
        // A mask whose content wears the same mask is the `<use>` bomb in a
        // third spelling.
        if self.expanding.contains(&at) {
            return Err(Refusal::TooManyUses);
        }
        let element = &tree.nodes[at];
        let user_region = matches!(
            element.attr("maskUnits").map(str::trim),
            Some("userSpaceOnUse")
        );
        let box_content = matches!(
            element.attr("maskContentUnits").map(str::trim),
            Some("objectBoundingBox")
        );
        if (!user_region || box_content) && !self.measurable(extent) {
            return Ok(None);
        }
        let nothing = crate::Mask {
            nodes: Vec::new(),
            region: Some(Outline::default()),
        };
        let [min_x, min_y, max_x, max_y] = extent.measured;
        let (width, height) = (max_x - min_x, max_y - min_y);
        let has_area = extent.has_area();
        let region = if user_region {
            let (vw, vh) = frame.viewport;
            [
                self.length_of(element, "x", Some(vw), -0.1 * vw),
                self.length_of(element, "y", Some(vh), -0.1 * vh),
                self.length_of(element, "width", Some(vw), 1.2 * vw),
                self.length_of(element, "height", Some(vh), 1.2 * vh),
            ]
        } else {
            if !has_area {
                return Ok(Some(nothing));
            }
            let fraction = |walk: &mut Self, name: &str, default: f64| {
                walk.length_of(element, name, Some(1.0), default)
            };
            let (x, y) = (fraction(self, "x", -0.1), fraction(self, "y", -0.1));
            let (w, h) = (fraction(self, "width", 1.2), fraction(self, "height", 1.2));
            [min_x + x * width, min_y + y * height, w * width, h * height]
        };
        let [x, y, w, h] = region;
        if !(w > 0.0 && h > 0.0) {
            return Ok(Some(nothing));
        }
        let content = if box_content {
            if !has_area {
                return Ok(Some(nothing));
            }
            transform::concat([width, 0.0, 0.0, height, min_x, min_y], matrix)
        } else {
            matrix
        };
        // §14.4: *"properties inherit into the 'mask' element from its
        // ancestors; properties do not inherit from the element referencing
        // the 'mask' element"* — the marker's rule, by the marker's code.
        let style = self.style_of(at)?;
        let inner = Frame {
            matrix: content,
            viewport: frame.viewport,
            style,
            depth: frame.depth + 1,
        };
        // A `<text>` inside the mask starts a text position of its own, and
        // the one in force belongs to the run that is being masked.
        let text = std::mem::take(&mut self.text);
        self.expanding.push(at);
        let drawn = self.collect(|walk| walk.children(at, &inner));
        self.expanding.pop();
        self.text = text;
        let nodes = drawn?;
        let rectangle = Outline {
            segments: vec![
                Segment::Move([x, y]),
                Segment::Line([x + w, y]),
                Segment::Line([x + w, y + h]),
                Segment::Line([x, y + h]),
                Segment::Close,
            ],
        };
        Ok(Some(crate::Mask {
            nodes,
            region: Some(rectangle.transformed(matrix)),
        }))
    }

    /// §14.3's `<clipPath>`, read for one element: [`gradient::clip`] with the
    /// walk's segment budget, and its two warnings named.
    ///
    /// `None` draws the element unclipped: a reference naming no `<clipPath>`,
    /// or one in `objectBoundingBox` units on a box that is text's alone
    /// ([`Walk::measurable`]), which would otherwise clip the text away to
    /// nothing.
    fn clip_of(
        &mut self,
        name: &str,
        matrix: [f64; 6],
        extent: Extent,
        style: &Style,
    ) -> Result<Option<gradient::ClipSource>, Refusal> {
        if gradient::clip_measures_box(self.tree, name) && !self.measurable(extent) {
            return Ok(None);
        }
        let source = gradient::clip(
            self.tree,
            name,
            matrix,
            extent.measured,
            style,
            &mut self.segments,
        )?;
        match &source {
            None => self.warn(Warning::ClipPathUnsupported),
            Some(source) if source.ignored => self.warn(Warning::ClipChildIgnored),
            Some(_) => {}
        }
        Ok(source)
    }

    /// Whether a bounding-box effect can be resolved against `extent`, naming
    /// the text left out of it where there is some.
    ///
    /// `false` is a box that is text's alone: the caller draws the element
    /// without the effect, or takes the paint's fallback — ruling 2's answer —
    /// rather than reading the empty box as §13.2.3's zero-area rule. Shapes or
    /// pictures beside the text give the box they span, and that is used.
    fn measurable(&mut self, extent: Extent) -> bool {
        if !extent.text {
            return true;
        }
        self.warn(Warning::TextBoxUnmeasured);
        extent.has_area()
    }

    /// A `<clipPath>` that holds text, as the mask of its silhouettes.
    ///
    /// §14.3.5's clip is *"the raw geometry of each child element exclusive of
    /// rendering properties such as fill, stroke, stroke-width"*, a one-bit
    /// mask. Its shapes are already one outline; its text is walked as text —
    /// styled down the `<clipPath>`'s own ancestry, or the `<use>`'s that
    /// names it, never the clipped element's (§14.3.5) — and every run and
    /// the outline are filled white, unstroked and opaque, on the black a
    /// mask has wherever nothing is drawn. White is the luminance that keeps,
    /// so the mask is the union of the silhouettes.
    fn silhouette(
        &mut self,
        source: gradient::ClipSource,
        frame: &Frame,
    ) -> Result<crate::Mask, Refusal> {
        // A clip whose text wears the same clip is the `<use>` bomb in a fifth
        // spelling.
        if self.expanding.contains(&source.at) {
            return Err(Refusal::TooManyUses);
        }
        let mut nodes = Vec::new();
        if !source.clip.outline.segments.is_empty() {
            let shapes = crate::Node::Path {
                outline: source.clip.outline,
                fill: Paint::Solid(Colour { rgb: [1.0; 3] }),
                rule: source.clip.rule,
                fill_opacity: 1.0,
                stroke: None,
                clip: None,
            };
            if self.admits(&shapes) {
                self.charge()?;
                nodes.push(shapes);
            }
        }
        // The text position belongs to the run being clipped.
        let text = std::mem::take(&mut self.text);
        self.expanding.push(source.at);
        let mut drawn = Ok(());
        for (at, matrix, parent) in source.text {
            let walked = self.style_of(parent).and_then(|style| {
                let inner = Frame {
                    matrix,
                    viewport: frame.viewport,
                    style,
                    depth: frame.depth + 1,
                };
                self.collect(|walk| walk.element(at, &inner))
            });
            match walked {
                Ok(found) => whiten(found, &mut nodes),
                Err(refusal) => {
                    drawn = Err(refusal);
                    break;
                }
            }
        }
        self.expanding.pop();
        self.text = text;
        drawn?;
        Ok(crate::Mask {
            nodes,
            region: None,
        })
    }

    /// One node at an element's own `opacity`: pushed as it is, folded into
    /// its alpha, or wrapped in a group of one — see [`crate::Node::Group`]
    /// for which and why.
    fn emit(&mut self, node: crate::Node, opacity: f64) -> Result<(), Refusal> {
        if opacity >= 1.0 {
            return self.push(node);
        }
        if opacity <= 0.0 || !self.admits(&node) {
            return Ok(());
        }
        let mut nodes = vec![node];
        self.charge()?;
        if fold(&mut nodes, opacity) {
            self.scene.nodes.extend(nodes);
            return Ok(());
        }
        self.push(crate::Node::Group {
            nodes,
            opacity,
            clip: None,
            mask: None,
        })
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
        if outer.depth >= self.limits.max_depth {
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
            "svg" => self.viewport_element(index, node, frame, (None, None)),
            "g" | "a" => {
                let inner = Frame {
                    matrix: self.matrix_of(node, frame.matrix),
                    ..frame.clone()
                };
                self.group(&frame.style, inner.matrix, frame, |walk| {
                    walk.children(index, &inner)
                })
            }
            // §5.5: `<defs>` is never rendered where it stands. Its contents
            // are reached by reference and nowhere else, so walking into it
            // here would draw every gradient's own geometry twice.
            "defs" => Ok(()),
            // §5.4 and §5.12: content for a reader rather than for a raster.
            // Not a warning, because nothing was refused — the specification
            // says these do not paint.
            "title" | "desc" | "metadata" => Ok(()),
            // §6.3: a `<style>` element is never rendered. Its sheet was read
            // before the walk began (`style::sheet_with`), so it is not an
            // unknown element either — reporting it as one named a loss that
            // was not there in every document that styles itself.
            "style" => Ok(()),

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
            // §14.4: a `<mask>` is reached by a `mask` reference and never
            // rendered where it stands — `<clipPath>`'s rule.
            "mask" => Ok(()),
            // §14.3: a `<clipPath>` is never rendered where it stands — it
            // is reached by a `clip-path` reference and nowhere else, so
            // walking into it here would draw every clip's own geometry as
            // though it were a shape. `<defs>`'s reason exactly, and the
            // reason `<linearGradient>` is beside it.
            "clipPath" | "linearGradient" | "radialGradient" | "symbol" => Ok(()),

            // §5.6's `<use>`, which is the one place an SVG grows
            // multiplicatively.
            "use" => self.use_element(node, frame),

            // §10's text.
            "text" => self.text_element(index, node, frame),
            // §10.13's text on a path, and its two relatives. Each is a second
            // layout engine rather than a property, and none has a file behind
            // it here (ruling 3).
            "textPath" | "tref" | "altGlyph" => {
                self.warn(Warning::TextLayoutUnsupported);
                Ok(())
            }
            // §13.3: a `<pattern>` is a paint server, reached by reference and
            // never rendered where it stands.
            "pattern" => Ok(()),
            // §11.6.2: a `<marker>` is drawn at the vertices of whatever
            // references it and never where it stands — `<defs>`'s rule, and
            // the reason `<clipPath>` is beside it.
            "marker" => Ok(()),
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
        let style = frame.style.clone();
        let style = &style;
        // §7.11's object bounding box, in the element's **own** user space —
        // which is what `objectBoundingBox` units are a fraction of, and why
        // it is taken before the matrix rather than after.
        let bounds = gradient::bounds(&outline);
        // §11.5: a hidden element is laid out and not painted, which for a
        // display list means it is not in it. Distinct from `display: none`
        // only in that a descendant could have turned it back on, and a
        // shape has none.
        if !style.visible {
            return Ok(());
        }
        let extent = Extent::shape(bounds);
        let fill = self.paint(&style.fill, style, matrix, extent, frame)?;
        let stroke_paint = self.paint(&style.stroke, style, matrix, extent, frame)?;
        // §14.3's clip, resolved against the same two numbers a gradient uses.
        // A `clip-path` naming nothing is **not** a clip: §14.3.1 makes a
        // reference to a non-existent element an error, and ruling 2 draws the
        // element rather than losing it.
        //
        // A clip that holds text is a mask of silhouettes, which a shape cannot
        // carry: it is a group around the shape, as a `mask` is.
        let text_clip = style
            .clip_path
            .as_deref()
            .is_some_and(|name| gradient::clip_holds_text(self.tree, name));
        let clip = match &style.clip_path {
            Some(name) if !text_clip => self
                .clip_of(name, matrix, extent, style)?
                .map(|source| source.clip),
            _ => None,
        };
        // §11.4: a stroke with no paint, no width or a zero width puts no ink
        // on the page. Answered here rather than carried, so a consumer never
        // has to decide whether a `Stroke` of width zero draws.
        let stroke = if stroke_paint == Paint::None || style.stroke_width <= 0.0 {
            None
        } else {
            Some(Box::new(Stroke {
                paint: stroke_paint,
                width: style.stroke_width,
                cap: style.cap,
                join: style.join,
                miter_limit: style.miter_limit,
                dashes: style.dashes.clone(),
                dash_offset: style.dash_offset,
                opacity: style.stroke_opacity.clamp(0.0, 1.0),
            }))
        };
        // §14.4's mask on a shape is of its whole rendering — fill, stroke and
        // markers — so it is a group around what follows, with the opacity
        // and the clip left to the shape itself, which handles both already.
        // Its box is the shape's own, though: §7.11's object bounding box is
        // the geometry, and the markers drawn inside the group are not in it.
        if style.mask.is_some() || text_clip {
            let masking = Style {
                opacity: 1.0,
                clip_path: if text_clip {
                    style.clip_path.clone()
                } else {
                    None
                },
                ..style.clone()
            };
            let unmasked = Style {
                mask: None,
                ..style.clone()
            };
            return self.group_in(&masking, matrix, frame, Some(bounds), |walk| {
                walk.paint_shape(node, &unmasked, matrix, outline, fill, stroke, clip, frame)
            });
        }
        self.paint_shape(node, style, matrix, outline, fill, stroke, clip, frame)
    }

    /// A shape whose paint is resolved: its node, its markers, and its own
    /// opacity over both.
    #[allow(clippy::too_many_arguments)]
    fn paint_shape(
        &mut self,
        node: &Node,
        style: &Style,
        matrix: [f64; 6],
        outline: Outline,
        fill: Paint,
        stroke: Option<Box<Stroke>>,
        clip: Option<crate::Clip>,
        frame: &Frame,
    ) -> Result<(), Refusal> {
        let markers = self.markers(node, style, matrix, &outline, frame)?;
        let shape = crate::Node::Path {
            outline: outline.transformed(matrix),
            fill,
            rule: style.fill_rule,
            fill_opacity: style.fill_opacity.clamp(0.0, 1.0),
            stroke,
            clip: clip.clone(),
        };
        if markers.is_empty() {
            // §14.5's opacity is the shape's own and applies to its rendering
            // as a whole: exact as an alpha where it paints once, and a group
            // of one where a fill and a stroke would otherwise darken each
            // other.
            return self.emit(shape, style.opacity);
        }
        // §11.6.2: markers are painted after the shape's fill and stroke, and
        // they are part of the **element's** rendering — so its clip and its
        // opacity are theirs too. The marker nodes were charged when the walk
        // pushed them; the shape and any wrapper are charged here.
        let mut nodes = Vec::new();
        if self.admits(&shape) {
            self.charge()?;
            nodes.push(shape);
        }
        match clip {
            Some(clip) => {
                self.charge()?;
                nodes.push(crate::Node::Group {
                    nodes: markers,
                    opacity: 1.0,
                    clip: Some(clip),
                    mask: None,
                });
            }
            None => nodes.extend(markers),
        }
        if style.opacity >= 1.0 {
            self.scene.nodes.extend(nodes);
            return Ok(());
        }
        if style.opacity <= 0.0 {
            return Ok(());
        }
        self.push(crate::Node::Group {
            nodes,
            opacity: style.opacity,
            clip: None,
            mask: None,
        })
    }

    /// §11.6's markers on one shape, as the nodes they draw.
    ///
    /// `outline` is the shape **in its own user space**, which is where its
    /// vertices and their directions are, and where `markerUnits` measures a
    /// stroke width.
    fn markers(
        &mut self,
        node: &Node,
        style: &Style,
        matrix: [f64; 6],
        outline: &Outline,
        frame: &Frame,
    ) -> Result<Vec<crate::Node>, Refusal> {
        // SVG 1.1 §11.6.2: markers apply to `<path>`, `<line>`, `<polyline>`
        // and `<polygon>`. SVG 2 adds the other basic shapes; 1.1 is what
        // this crate reads, and a rectangle with an arrowhead is not a shape
        // any producer in the corpus draws.
        if !matches!(node.name.as_str(), "path" | "line" | "polyline" | "polygon")
            || style.markers.iter().all(Option::is_none)
        {
            return Ok(Vec::new());
        }
        // A path's arc is several cubics, and a vertex is where a *command*
        // ends — so the data is read again for its boundaries, against a
        // budget of what the first reading already spent.
        let ends = match node.attr("d") {
            Some(data) if node.name == "path" => {
                let mut budget = outline.segments.len();
                path::parse_commands(data, &mut budget)
                    .ok()
                    .map(|(_, ends)| ends)
            }
            _ => None,
        };
        let vertices = marker::vertices(outline, ends.as_deref());
        let Some(last) = vertices.len().checked_sub(1) else {
            return Ok(Vec::new());
        };
        let mut chosen: [Option<Marker>; 3] = [None, None, None];
        for (slot, name) in style.markers.iter().enumerate() {
            let Some(name) = name else {
                continue;
            };
            let target = self.tree.by_id(name).filter(|at| {
                self.tree.nodes[*at].is_svg() && self.tree.nodes[*at].name == "marker"
            });
            match target {
                Some(at) => chosen[slot] = self.marker_of(at, frame)?,
                // §11.6.2 makes a reference to nothing an error, and ruling 2
                // draws the path without the decoration rather than losing it.
                None => self.warn(Warning::MarkerUnresolved),
            }
        }
        self.collect(|walk| {
            for (index, vertex) in vertices.iter().enumerate() {
                let slots: &[usize] = match (index == 0, index == last) {
                    (true, true) => &[0, 2],
                    (true, false) => &[0],
                    (false, true) => &[2],
                    (false, false) => &[1],
                };
                for slot in slots {
                    if let Some(marker) = &chosen[*slot] {
                        walk.marker_instance(
                            marker,
                            vertex,
                            index == 0,
                            style,
                            matrix,
                            frame.depth,
                        )?;
                    }
                }
            }
            Ok(())
        })
    }

    /// One `<marker>`'s own attributes, and the style its content starts
    /// from — or `None` for a marker whose rendering is disabled.
    fn marker_of(&mut self, at: usize, frame: &Frame) -> Result<Option<Marker>, Refusal> {
        let tree = self.tree;
        let Some(element) = tree.nodes.get(at) else {
            return Ok(None);
        };
        let width = self.length_of(element, "markerWidth", Some(frame.viewport.0), 3.0);
        let height = self.length_of(element, "markerHeight", Some(frame.viewport.1), 3.0);
        // §11.6.2: a zero `markerWidth` or `markerHeight` disables the
        // marker, and a negative one is an error that does the same.
        if !(width > 0.0 && height > 0.0) {
            return Ok(None);
        }
        let view = match self.view_box_of(element, width, height) {
            Ok(view) => view,
            Err(Disabled) => return Ok(None),
        };
        let reference = [
            self.length_of(element, "refX", Some(width), 0.0),
            self.length_of(element, "refY", Some(height), 0.0),
        ];
        let orient = match marker::orient(element.attr("orient")) {
            Some(orient) => orient,
            None => {
                self.warn(Warning::ValueUnreadable {
                    attribute: "orient".to_owned(),
                });
                Orient::Angle(0.0)
            }
        };
        let stroke_units = !matches!(
            element.attr("markerUnits").map(str::trim),
            Some("userSpaceOnUse")
        );
        // The user agent style sheet's `marker { overflow: hidden }`: the
        // content is clipped to the marker's viewport unless the file says
        // otherwise, in the attribute or in `style=""`.
        let visible = |value: &str| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "visible" | "auto"
            )
        };
        let overflows = element.attr("overflow").is_some_and(visible)
            || element.style.as_deref().is_some_and(|text| {
                text.split(';').any(|piece| {
                    piece.split_once(':').is_some_and(|(name, value)| {
                        name.trim().eq_ignore_ascii_case("overflow") && visible(value)
                    })
                })
            });
        // The viewport a percentage inside the marker resolves against: the
        // view box's own size where there is one, the marker's otherwise.
        let viewport = match element.attr("viewBox").and_then(transform::numbers) {
            Some(numbers) if numbers.len() == 4 && numbers[2] > 0.0 && numbers[3] > 0.0 => {
                (numbers[2], numbers[3])
            }
            _ => (width, height),
        };
        let style = self.style_of(at)?;
        Ok(Some(Marker {
            at,
            size: (width, height),
            view,
            reference,
            orient,
            stroke_units,
            clipped: !overflows,
            viewport,
            style,
        }))
    }

    /// §11.6.2's *"properties inherit into the 'marker' element from its
    /// ancestors; properties do not inherit from the element referencing the
    /// 'marker' element"* — so a marker's style is resolved down its own
    /// ancestry, from the root, and not from the shape that drew it.
    fn style_of(&mut self, at: usize) -> Result<Style, Refusal> {
        let mut chain = vec![at];
        let mut cursor = at;
        while let Some(parent) = self.tree.nodes.get(cursor).and_then(|node| node.parent) {
            // The tree's own depth was capped when it was read; this is the
            // same bound, so a malformed parent chain cannot loop.
            if chain.len() > self.limits.max_depth {
                return Err(Refusal::TooDeep);
            }
            chain.push(parent);
            cursor = parent;
        }
        let mut style = Style::default();
        for index in chain.iter().rev() {
            let resolved = style::resolve(self.tree, *index, &self.sheet, &style, &mut self.css)
                .map_err(|_| Refusal::TooMuchStyle)?;
            for name in resolved.unreadable {
                self.warn(Warning::ValueUnreadable { attribute: name });
            }
            style = resolved.style;
        }
        Ok(style)
    }

    /// One marker at one vertex: §11.6.2's transform, the viewport clip, and
    /// the marker's content walked under both.
    fn marker_instance(
        &mut self,
        marker: &Marker,
        vertex: &marker::Vertex,
        first: bool,
        style: &Style,
        matrix: [f64; 6],
        depth: usize,
    ) -> Result<(), Refusal> {
        // A marker that draws a path carrying the same marker is the `<use>`
        // bomb in another spelling, and is refused by the same rule.
        if self.expanding.contains(&marker.at) {
            return Err(Refusal::TooManyUses);
        }
        let angle = marker::angle(vertex, marker.orient, first);
        let (sin, cos) = (math::sin(angle), math::cos(angle));
        let scale = if marker.stroke_units {
            style.stroke_width
        } else {
            1.0
        };
        // From the vertex outward: the path's own space, the vertex, the
        // turn, the stroke-width scale, and then the reference point — which
        // §11.6.2 states *after* the view box, so it is taken through it.
        let placed = transform::concat(
            [cos, sin, -sin, cos, 0.0, 0.0],
            transform::concat([1.0, 0.0, 0.0, 1.0, vertex.at[0], vertex.at[1]], matrix),
        );
        let placed = transform::concat([scale, 0.0, 0.0, scale, 0.0, 0.0], placed);
        let reference = transform::apply(marker.view, marker.reference);
        let viewport =
            transform::concat([1.0, 0.0, 0.0, 1.0, -reference[0], -reference[1]], placed);
        let content = transform::concat(marker.view, viewport);
        let inner = Frame {
            matrix: content,
            viewport: marker.viewport,
            style: marker.style.clone(),
            // Where the marker is being *drawn*, which is under the shape that
            // referenced it: `Frame::depth`'s reason, for a marker inside a
            // marker inside a marker.
            depth: depth + 1,
        };
        self.expanding.push(marker.at);
        let drawn = self.collect(|walk| {
            walk.group(&marker.style, content, &inner, |walk| {
                walk.children(marker.at, &inner)
            })
        });
        self.expanding.pop();
        let nodes = drawn?;
        if nodes.is_empty() {
            return Ok(());
        }
        if !marker.clipped {
            self.scene.nodes.extend(nodes);
            return Ok(());
        }
        let (width, height) = marker.size;
        let rectangle = Outline {
            segments: vec![
                Segment::Move([0.0, 0.0]),
                Segment::Line([width, 0.0]),
                Segment::Line([width, height]),
                Segment::Line([0.0, height]),
                Segment::Close,
            ],
        };
        self.push(crate::Node::Group {
            nodes,
            opacity: 1.0,
            clip: Some(crate::Clip {
                outline: rectangle.transformed(viewport),
                rule: crate::FillRule::NonZero,
            }),
            mask: None,
        })
    }

    /// §13.2's `<paint>`, with a `url(#name)` resolved against the document.
    ///
    /// A reference that names nothing this build can paint with falls through
    /// to **the paint's own fallback** — which is §13.2's answer and not an
    /// invention: a file that wrote `fill="url(#g) red"` said what to do when
    /// the server is missing, and a build that drew nothing would be ignoring
    /// the half of the value that was for exactly this.
    ///
    /// The fallback stands for one more reason, and it is named: a server in
    /// `objectBoundingBox` units painting text, whose box this crate cannot
    /// measure ([`Walk::measurable`]).
    fn paint(
        &mut self,
        spec: &PaintSpec,
        style: &Style,
        matrix: [f64; 6],
        extent: Extent,
        frame: &Frame,
    ) -> Result<Paint, Refusal> {
        Ok(match spec {
            PaintSpec::None => Paint::None,
            PaintSpec::Solid(colour) => Paint::Solid(*colour),
            PaintSpec::Current => Paint::Solid(style.colour),
            PaintSpec::Reference(name, fallback) => {
                let target = self.tree.by_id(name);
                let kind = target.map(|at| self.tree.nodes[at].name.as_str());
                if let (Some(at), Some("linearGradient" | "radialGradient")) = (target, kind) {
                    if gradient::measures_box(self.tree, at) && !self.measurable(extent) {
                        return self.paint(fallback, style, matrix, extent, frame);
                    }
                    if let Some(resolved) =
                        gradient::resolve(self.tree, at, matrix, extent.measured, style)
                    {
                        return Ok(resolved.paint);
                    }
                    // §13.2.4: a gradient with no stops paints **as if `none`
                    // were specified** — which is not the same as falling
                    // through to the fallback, because the server was found.
                    return Ok(Paint::None);
                }
                if let (Some(at), Some("pattern")) = (target, kind) {
                    // §13.3: a pattern whose tile has no area paints nothing,
                    // which — the server having been found — is `none` and not
                    // the fallback, the gradient's rule.
                    return match self.pattern_of(at, matrix, extent, frame)? {
                        Tiling::Paint(paint) => Ok(paint),
                        Tiling::Disabled => Ok(Paint::None),
                        Tiling::Unmeasured => self.paint(fallback, style, matrix, extent, frame),
                    };
                }
                self.warn(Warning::PaintServerUnresolved);
                return self.paint(fallback, style, matrix, extent, frame);
            }
        })
    }

    /// §13.3's `<pattern>` as a paint, for one element: the tile, and its
    /// content walked into nodes of its own in **pattern space**.
    ///
    /// Pattern space is the referencing element's user space with
    /// `patternTransform` applied, and the tile at `x`, `y`, `width`,
    /// `height` in it repeats at every multiple of its own size — which is
    /// 8.7.3's tiling pattern exactly, so that is what [`crate::Tile`]
    /// describes. Every attribute and the content follow the `xlink:href`
    /// chain as a gradient's do (§13.3: *"any attributes which are defined on
    /// the referenced element which are not defined on this element are
    /// inherited by this element"*, and the children likewise when this one
    /// has none).
    ///
    /// [`Tiling::Disabled`] for a tile with no area, which §13.3 says disables
    /// the paint, and [`Tiling::Unmeasured`] for a tile or content in
    /// `objectBoundingBox` units on a box that is text's alone.
    fn pattern_of(
        &mut self,
        at: usize,
        matrix: [f64; 6],
        extent: Extent,
        frame: &Frame,
    ) -> Result<Tiling, Refusal> {
        let tree = self.tree;
        let mut chain = vec![at];
        while chain.len() < 10 {
            let Some(next) = chain
                .last()
                .and_then(|last| tree.nodes.get(*last))
                .and_then(Node::href)
                .and_then(|href| href.trim().strip_prefix('#'))
                .and_then(|name| tree.by_id(name))
                .filter(|next| tree.nodes[*next].is_svg() && tree.nodes[*next].name == "pattern")
            else {
                break;
            };
            chain.push(next);
        }
        let along = |name: &str| {
            chain
                .iter()
                .find_map(|index| tree.nodes[*index].attr(name))
                .map(str::trim)
        };
        let length = |name: &str, basis: f64, default: f64| {
            along(name)
                .and_then(|text| document::length(text, Some(basis)))
                .unwrap_or(default)
        };
        // §13.3's initial `patternUnits` is `objectBoundingBox`, and its
        // initial `patternContentUnits` is `userSpaceOnUse` — which a
        // `viewBox` makes moot.
        let user_cell = along("patternUnits") == Some("userSpaceOnUse");
        let view = along("viewBox")
            .and_then(transform::numbers)
            .filter(|numbers| numbers.len() == 4);
        let box_content =
            view.is_none() && along("patternContentUnits") == Some("objectBoundingBox");
        if (!user_cell || box_content) && !self.measurable(extent) {
            return Ok(Tiling::Unmeasured);
        }
        let [min_x, min_y, max_x, max_y] = extent.measured;
        let (box_width, box_height) = (max_x - min_x, max_y - min_y);
        let box_area = extent.has_area();
        let cell = if user_cell {
            let (vw, vh) = frame.viewport;
            [
                length("x", vw, 0.0),
                length("y", vh, 0.0),
                length("width", vw, 0.0),
                length("height", vh, 0.0),
            ]
        } else {
            if !box_area {
                return Ok(Tiling::Disabled);
            }
            [
                min_x + length("x", 1.0, 0.0) * box_width,
                min_y + length("y", 1.0, 0.0) * box_height,
                length("width", 1.0, 0.0) * box_width,
                length("height", 1.0, 0.0) * box_height,
            ]
        };
        let [x, y, width, height] = cell;
        if !(width > 0.0 && height > 0.0 && cell.iter().all(|v| v.is_finite())) {
            return Ok(Tiling::Disabled);
        }
        // The content's own space, into pattern space: a `viewBox` fitted into
        // the tile (which makes `patternContentUnits` moot, §13.3 says), or
        // the tile's corner as the origin, scaled by the box under
        // `objectBoundingBox`.
        let content = match view {
            Some(numbers) => {
                let Some(fit) = transform::view_box(
                    [numbers[0], numbers[1], numbers[2], numbers[3]],
                    width,
                    height,
                    along("preserveAspectRatio"),
                ) else {
                    return Ok(Tiling::Disabled);
                };
                transform::concat(fit, [1.0, 0.0, 0.0, 1.0, x, y])
            }
            None if box_content => {
                if !box_area {
                    return Ok(Tiling::Disabled);
                }
                [box_width, 0.0, 0.0, box_height, x, y]
            }
            None => [1.0, 0.0, 0.0, 1.0, x, y],
        };
        let own = along("patternTransform")
            .and_then(transform::list)
            .unwrap_or(IDENTITY);
        let source = chain
            .iter()
            .copied()
            .find(|index| tree.element_children(*index).next().is_some())
            .unwrap_or(at);
        // A pattern whose tile is painted with itself is the `<use>` bomb's
        // fourth spelling.
        if self.expanding.contains(&source) {
            return Err(Refusal::TooManyUses);
        }
        // §13.3: properties inherit into a `<pattern>` from its ancestors and
        // not from the element it paints.
        let style = self.style_of(source)?;
        let inner = Frame {
            matrix: content,
            viewport: frame.viewport,
            style,
            depth: frame.depth + 1,
        };
        // The text position belongs to the run that asked for this paint, and
        // a `<text>` inside the tile would start its own.
        let text = std::mem::take(&mut self.text);
        self.expanding.push(source);
        let drawn = self.collect(|walk| walk.children(source, &inner));
        self.expanding.pop();
        self.text = text;
        let nodes = drawn?;
        Ok(Tiling::Paint(Paint::Pattern(Box::new(crate::Tile {
            nodes,
            cell,
            matrix: transform::concat(own, matrix),
        }))))
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

    /// §5.6's `<use>`: the referenced element, drawn again here.
    ///
    /// **Walked rather than cloned.** §5.6 describes a deep clone into a
    /// shadow tree, and building one would double the memory of every document
    /// that reuses anything — `Tsuru_wiki-1b.svg` reuses its gradients
    /// sixteen times. Walking the original with a different matrix and a
    /// different inherited style produces the same scene, because a scene is
    /// what the clone would have been walked into anyway.
    ///
    /// What that costs is one thing, and it is paid explicitly: the walk's
    /// depth stops being the element's depth, so [`Frame::depth`] exists.
    fn use_element(&mut self, node: &Node, frame: &Frame) -> Result<(), Refusal> {
        let Some(href) = node.href() else {
            // §5.6 makes the reference required; without one there is nothing
            // to instantiate and nothing was refused.
            return Ok(());
        };
        // Only a same-document reference. `other.svg#thing` names a resource
        // this crate cannot fetch — no container, no filesystem, no network —
        // so it is named rather than silently drawing nothing.
        let Some(name) = href.trim().strip_prefix('#') else {
            self.warn(Warning::UseUnresolved);
            return Ok(());
        };
        let Some(target) = self.tree.by_id(name) else {
            self.warn(Warning::UseUnresolved);
            return Ok(());
        };
        // The bomb, refused **by name** rather than recursed into.
        if self.expanding.contains(&target) {
            return Err(Refusal::TooManyUses);
        }
        if self.uses >= self.limits.max_uses {
            return Err(Refusal::TooManyUses);
        }
        self.uses += 1;

        // §5.6: `x` and `y` are *"an additional transformation
        // translate(x, y)"*, appended after the element's own — so a `<use>`
        // that both translates and scales scales first, which is the order a
        // `transform` list would have given it.
        let matrix = self.matrix_of(node, frame.matrix);
        let x = self.length_of(node, "x", Some(frame.viewport.0), 0.0);
        let y = self.length_of(node, "y", Some(frame.viewport.1), 0.0);
        let matrix = transform::concat([1.0, 0.0, 0.0, 1.0, x, y], matrix);

        let inner = Frame {
            matrix,
            viewport: frame.viewport,
            // §5.6: the referenced content inherits from the `<use>`, not from
            // where it was written. A `<use fill="red">` of a shape that
            // states no fill draws it red, and that is the whole reason a
            // symbol library is useful.
            style: frame.style.clone(),
            depth: frame.depth + 1,
        };
        // §5.6: the `<use>` becomes a `<g>` carrying its own attributes, so its
        // `opacity` and `clip-path` are a group around the instance — in the
        // space that includes `x` and `y`, which the generated `<g>`'s
        // transform ends with.
        let style = frame.style.clone();
        self.expanding.push(target);
        let drawn = self.group(&style, matrix, frame, |walk| {
            walk.instance(target, node, &inner)
        });
        self.expanding.pop();
        drawn
    }

    /// One instance of a `<use>`'s target.
    ///
    /// §5.6 gives `<symbol>` and `<svg>` their own rule: the `<use>`'s `width`
    /// and `height` **override** the referenced element's, and a `<symbol>`
    /// becomes an `<svg>`. Every other element is drawn as itself.
    fn instance(&mut self, target: usize, node: &Node, frame: &Frame) -> Result<(), Refusal> {
        let referenced = &self.tree.nodes[target];
        if referenced.is_svg() && matches!(referenced.name.as_str(), "symbol" | "svg") {
            let width = node
                .attr("width")
                .and_then(|text| document::length(text, Some(frame.viewport.0)));
            let height = node
                .attr("height")
                .and_then(|text| document::length(text, Some(frame.viewport.1)));
            let referenced = referenced.clone();
            return self.viewport_element(target, &referenced, frame, (width, height));
        }
        self.element(target, frame)
    }

    /// §10.4's `<text>`, as one or more runs.
    ///
    /// **The pen is not tracked here and that is the milestone's whole
    /// decision.** Where a `<tspan>` with no `x` of its own begins depends on
    /// how wide the text before it was, and a width is a *font metric* — which
    /// this crate cannot have without an edge to `tinker-pdf-font`, and ruling
    /// 8 forbids that edge. So a run that continues carries `anchor: None` and
    /// the caller, which has the metrics because it set the rest of the book
    /// with them, advances the pen. §10.9's `text-anchor` is the same argument
    /// one level up: a chunk cannot be centred until its whole width is known.
    ///
    /// What this *does* do is everything about the document: white space, the
    /// `x`/`y`/`dx`/`dy` grammar, the nesting of `<tspan>`s, and the font
    /// properties through the same §6.4 machinery every other element uses.
    fn text_element(&mut self, index: usize, node: &Node, frame: &Frame) -> Result<(), Refusal> {
        let matrix = self.matrix_of(node, frame.matrix);
        let inner = Frame {
            matrix,
            ..frame.clone()
        };
        // §10.4: a `<text>`'s `x` and `y` are zero where it states none, so
        // every `<text>` opens a chunk at its first character — and the
        // position state starts again with it.
        let mut positions = self.positions(node, frame);
        if positions.x.is_empty() {
            positions.x.push(0.0);
        }
        if positions.y.is_empty() {
            positions.y.push(0.0);
        }
        self.text = Text {
            stack: vec![positions],
            ..Text::default()
        };
        let drawn = self.group(&frame.style, matrix, frame, |walk| {
            walk.text_runs(index, &inner)
        });
        self.text.stack.clear();
        drawn
    }

    /// One element's §10.4 lists, read.
    fn positions(&mut self, node: &Node, frame: &Frame) -> Positions {
        Positions {
            x: self.coordinates(node, "x", frame.viewport.0),
            y: self.coordinates(node, "y", frame.viewport.1),
            dx: self.coordinates(node, "dx", frame.viewport.0),
            dy: self.coordinates(node, "dy", frame.viewport.1),
            rotate: self.coordinates(node, "rotate", 1.0),
            placed: 0,
        }
    }

    /// A `<list-of-coordinates>`, each through §4.2's `<length>` grammar —
    /// which is what `1em` in an `x` means — or the empty list for an
    /// attribute that is absent or not the grammar, the second named.
    fn coordinates(&mut self, node: &Node, name: &str, basis: f64) -> Vec<f64> {
        let Some(text) = node.attr(name) else {
            return Vec::new();
        };
        let read: Option<Vec<f64>> = text
            .split(|c: char| c == ',' || c.is_ascii_whitespace())
            .filter(|piece| !piece.is_empty())
            .map(|piece| document::length(piece, Some(basis)))
            .collect();
        match read {
            Some(list) => list,
            None => {
                self.warn(Warning::ValueUnreadable {
                    attribute: name.to_owned(),
                });
                Vec::new()
            }
        }
    }

    /// The next character's §10.4 positioning, from the innermost element that
    /// has a value for it, and every open element's count advanced past it.
    ///
    /// SVG 1.1 §10.5's rule, per attribute: the `n`th number of an element's
    /// list belongs to the `n`th character **within that element or any of
    /// its descendants**; a character past the end of an element's list takes
    /// the nearest ancestor's number for it, if that one has one. `rotate` is
    /// the exception the clause makes: past the end of a list, its **last**
    /// number goes on applying, and an ancestor's is not consulted.
    fn glyph(&mut self) -> Glyph {
        let stack = &mut self.text.stack;
        let pick = |list: fn(&Positions) -> &Vec<f64>| -> Option<f64> {
            stack
                .iter()
                .rev()
                .find_map(|positions| list(positions).get(positions.placed).copied())
        };
        let glyph = Glyph {
            x: pick(|p| &p.x),
            y: pick(|p| &p.y),
            dx: pick(|p| &p.dx).unwrap_or(0.0),
            dy: pick(|p| &p.dy).unwrap_or(0.0),
            rotate: stack
                .iter()
                .rev()
                .find(|positions| !positions.rotate.is_empty())
                .and_then(|positions| {
                    positions
                        .rotate
                        .get(positions.placed)
                        .or(positions.rotate.last())
                        .copied()
                })
                .unwrap_or(0.0),
        };
        for positions in stack.iter_mut() {
            positions.placed = positions.placed.saturating_add(1);
        }
        glyph
    }

    /// One `<text>` or `<tspan>`, and everything under it, whose §10.4 lists
    /// are already on [`Text::stack`].
    fn text_runs(&mut self, index: usize, frame: &Frame) -> Result<(), Refusal> {
        let children = self.tree.nodes[index].children.clone();
        for child in children {
            match child {
                Child::Text(text) => {
                    // `xml:space="default"` is §10.15's own rule and the one
                    // every producer relies on: newlines and tabs become
                    // spaces, runs of space collapse to one, and the leading
                    // and trailing space of the *element* goes. Applied per
                    // run rather than across the element, which is where this
                    // build differs from a full implementation — named in
                    // `docs/design/svg.md` rather than hidden.
                    let text = collapse(&text);
                    if text.is_empty() {
                        continue;
                    }
                    self.characters(&text, frame)?;
                }
                Child::Element(at) => {
                    let element = self.tree.nodes[at].clone();
                    if !element.is_svg() || element.name != "tspan" {
                        // Anything else inside a `<text>` — a `<textPath>`, an
                        // `<a>`, an `<altGlyph>` — goes through the ordinary
                        // dispatch, which names what it declines.
                        self.element(at, frame)?;
                        continue;
                    }
                    let resolved =
                        style::resolve(self.tree, at, &self.sheet, &frame.style, &mut self.css)
                            .map_err(|_| Refusal::TooMuchStyle)?;
                    for name in resolved.unreadable {
                        self.warn(Warning::ValueUnreadable { attribute: name });
                    }
                    let child_frame = Frame {
                        matrix: self.matrix_of(&element, frame.matrix),
                        style: resolved.style,
                        depth: frame.depth + 1,
                        viewport: frame.viewport,
                    };
                    if child_frame.depth >= self.limits.max_depth {
                        return Err(Refusal::TooDeep);
                    }
                    let style = child_frame.style.clone();
                    let positions = self.positions(&element, &child_frame);
                    self.text.stack.push(positions);
                    let drawn = self.group(&style, child_frame.matrix, &child_frame, |walk| {
                        walk.text_runs(at, &child_frame)
                    });
                    self.text.stack.pop();
                    drawn?;
                }
            }
        }
        Ok(())
    }

    /// One piece of character data, cut into runs wherever §10.4 moves the
    /// current text position or §10.5 turns a glyph.
    ///
    /// A run is as long as nothing interrupts it, which for a `<text>` with
    /// one `x` and one `y` is the whole string and for one with an `x` per
    /// character is one character each. What a run carries is what this crate
    /// knows: an absolute position opens a chunk ([`crate::Node::Text`]'s
    /// `anchor`), a `y` with no `x` opens one whose `x` continues
    /// (`continues_x`), and a `dx` or `dy` with neither moves the run off the
    /// pen by the shifts accumulated since the chunk opened — carried in its
    /// matrix, the one place a shift can live without a metric.
    fn characters(&mut self, text: &str, frame: &Frame) -> Result<(), Refusal> {
        let mut run = Run::default();
        for character in text.chars() {
            let glyph = self.glyph();
            let moved = glyph.x.is_some()
                || glyph.y.is_some()
                || glyph.dx != 0.0
                || glyph.dy != 0.0
                || glyph.rotate != 0.0
                || run.rotate != 0.0;
            if moved && !run.text.is_empty() {
                let done = std::mem::take(&mut run);
                self.push_text(done, frame)?;
            }
            let state = &mut self.text;
            if run.text.is_empty() {
                run.anchor = None;
                run.continues_x = false;
                if let Some(x) = glyph.x {
                    let y = glyph.y.unwrap_or(state.y) + glyph.dy;
                    run.anchor = Some([x + glyph.dx, y]);
                    state.shift = 0.0;
                    state.y = y;
                    state.chunk_y = y;
                } else if let Some(y) = glyph.y {
                    // §10.5's rule (b): no `x` for this character anywhere, so
                    // it starts where the previous glyph left the pen — with
                    // every `dx` since the chunk opened, and this one's.
                    let y = y + glyph.dy;
                    run.anchor = Some([state.shift + glyph.dx, y]);
                    run.continues_x = true;
                    state.shift = 0.0;
                    state.y = y;
                    state.chunk_y = y;
                } else {
                    state.shift += glyph.dx;
                    state.y += glyph.dy;
                }
                run.offset = [state.shift, state.y - state.chunk_y];
                run.rotate = glyph.rotate;
            }
            run.text.push(character);
        }
        if !run.text.is_empty() {
            self.push_text(run, frame)?;
        }
        Ok(())
    }

    /// One run of characters, as a node.
    fn push_text(&mut self, run: Run, frame: &Frame) -> Result<(), Refusal> {
        let Run {
            text,
            anchor,
            continues_x,
            offset: shift,
            rotate,
        } = run;
        let style = &frame.style;
        if !style.visible {
            return Ok(());
        }
        // A `dx`/`dy` on a **continuing** run is an offset from a pen this
        // crate does not have, so it is carried in the *matrix* — the one
        // place a shift can live without a metric. On a run that opens a chunk
        // the shift is already in the anchor, and `text_runs` zeroes it there
        // so that it cannot be counted twice.
        let matrix = if anchor.is_none() && shift != [0.0, 0.0] {
            transform::concat([1.0, 0.0, 0.0, 1.0, shift[0], shift[1]], frame.matrix)
        } else {
            frame.matrix
        };
        // A run's box is its glyph cells, which are a font's: a paint server
        // in `objectBoundingBox` units has nothing here to take a fraction of.
        let fill = self.paint(&style.fill, style, matrix, Extent::TEXT, frame)?;
        let stroke_paint = self.paint(&style.stroke, style, matrix, Extent::TEXT, frame)?;
        let stroke = if stroke_paint == Paint::None || style.stroke_width <= 0.0 {
            None
        } else {
            Some(Box::new(Stroke {
                paint: stroke_paint,
                width: style.stroke_width,
                cap: style.cap,
                join: style.join,
                miter_limit: style.miter_limit,
                dashes: style.dashes.clone(),
                dash_offset: style.dash_offset,
                opacity: style.stroke_opacity.clamp(0.0, 1.0),
            }))
        };
        self.push(crate::Node::Text {
            text,
            anchor,
            continues_x,
            rotate,
            matrix,
            font: TextStyle {
                families: style.families.clone(),
                size: style.font_size,
                weight: style.font_weight,
                italic: style.font_italic,
                anchor: style.text_anchor,
            },
            fill,
            fill_opacity: style.fill_opacity.clamp(0.0, 1.0),
            stroke,
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
        // An image's node has no alpha and no clip of its own, so its
        // `opacity`, its `clip-path` and its `mask` are all a group around it
        // — the first folds nowhere, which makes a translucent picture a group
        // of one. Until the group carried them the clip was dropped without a
        // word, which is the defect `clip-path` on a `<g>` also was.
        let picture = crate::Node::Image {
            href: href.to_owned(),
            rect: [x, y, width, height],
            matrix,
            preserve: node.attr("preserveAspectRatio").map(str::to_owned),
        };
        self.group(&frame.style, matrix, frame, |walk| walk.push(picture))
    }

    /// An `<svg>`, root or nested: §7.9's establishment of a new viewport.
    fn viewport_element(
        &mut self,
        index: usize,
        node: &Node,
        frame: &Frame,
        override_size: (Option<f64>, Option<f64>),
    ) -> Result<(), Refusal> {
        let (outer_width, outer_height) = frame.viewport;
        // A nested `<svg>`'s `x` and `y` place its viewport inside its
        // parent's; the root's are ignored by §7.2 and are zero on every file
        // that has them, so one code path answers both.
        let x = self.length_of(node, "x", Some(outer_width), 0.0);
        let y = self.length_of(node, "y", Some(outer_height), 0.0);
        // §5.6's override, which applies only through a `<use>` and is the
        // reason this is a parameter rather than a second function: a
        // `<symbol>` sized by its instance and one sized by itself must map
        // their view boxes the same way, and two code paths would eventually
        // stop doing so.
        let width = override_size
            .0
            .unwrap_or_else(|| self.length_of(node, "width", Some(outer_width), outer_width));
        let height = override_size
            .1
            .unwrap_or_else(|| self.length_of(node, "height", Some(outer_height), outer_height));
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
            depth: frame.depth,
        };
        // A nested `<svg>`'s own `opacity` and `clip-path` are of its whole
        // rendering, in the space its parent placed it in.
        let style = frame.style.clone();
        self.group(&style, frame.matrix, frame, |walk| {
            walk.children(index, &inner)
        })
    }

    /// Every child element of `index`, in document order.
    fn children(&mut self, index: usize, frame: &Frame) -> Result<(), Refusal> {
        let children: Vec<usize> = self.tree.element_children(index).collect();
        let inner = Frame {
            depth: frame.depth + 1,
            ..frame.clone()
        };
        for child in children {
            self.element(child, &inner)?;
        }
        Ok(())
    }
}

/// §7.7's "rendering of the element is disabled".
struct Disabled;

/// One `<text>` or `<tspan>`'s §10.4 lists, and how many characters within it
/// have been placed.
#[derive(Default)]
struct Positions {
    x: Vec<f64>,
    y: Vec<f64>,
    dx: Vec<f64>,
    dy: Vec<f64>,
    rotate: Vec<f64>,
    placed: usize,
}

/// One character's §10.4 positioning.
struct Glyph {
    x: Option<f64>,
    y: Option<f64>,
    dx: f64,
    dy: f64,
    /// §10.5's supplemental rotation, in degrees.
    rotate: f64,
}

/// The current text position, as far as this crate can know it.
///
/// Everything but the **advance** is a fact about the document: an absolute
/// `x` or `y`, and the `dx` and `dy` added since, are numbers the file states,
/// and in horizontal text only `y`, `dy` and an absolute `x` move the position
/// vertically or put it anywhere at all. What a glyph's width adds is a font
/// metric, which is the caller's (ruling 8). So the state is kept relative to
/// the chunk the caller is advancing: `shift` is the `dx` added since that
/// chunk's anchor, and `y` is exact.
#[derive(Default)]
struct Text {
    /// The lists of the `<text>` and every open `<tspan>`, outermost first.
    stack: Vec<Positions>,
    /// `dx` accumulated since the chunk's anchor.
    shift: f64,
    /// The current text position's `y`.
    y: f64,
    /// The `y` of the chunk's anchor, which is where the caller's pen is.
    chunk_y: f64,
}

/// A run being gathered.
#[derive(Default)]
struct Run {
    text: String,
    anchor: Option<[f64; 2]>,
    continues_x: bool,
    /// Off the caller's pen, for a run that opens no chunk.
    offset: [f64; 2],
    rotate: f64,
}

/// A `<marker>`, read once per shape that uses it.
struct Marker {
    /// The element.
    at: usize,
    /// `markerWidth` and `markerHeight`: the viewport, in the space the
    /// stroke-width scale makes.
    size: (f64, f64),
    /// The view box's mapping into that viewport.
    view: [f64; 6],
    /// `refX` and `refY`, in the view box's coordinates.
    reference: [f64; 2],
    orient: Orient,
    /// `markerUnits="strokeWidth"`, the initial value.
    stroke_units: bool,
    /// Whether the content is clipped to the viewport, which the user agent
    /// style sheet's `overflow: hidden` makes the default.
    clipped: bool,
    /// What a percentage inside the marker is a fraction of.
    viewport: (f64, f64),
    /// The style its content starts from: its own ancestry's.
    style: Style,
}

/// Whether every number one node carries is finite — not counting a group's
/// children, which [`Walk::push`] admitted before they were grouped.
/// A clip's text as silhouettes: every run filled white, opaque and
/// unstroked, at every depth, onto `out`.
///
/// A group inside keeps its own clip and mask — a child of a `<clipPath>`
/// may be clipped itself, §14.3.5 says, and the silhouette is then the
/// intersection — and loses its opacity, which a one-bit mask does not have.
fn whiten(nodes: Vec<crate::Node>, out: &mut Vec<crate::Node>) {
    let white = Paint::Solid(Colour { rgb: [1.0; 3] });
    for node in nodes {
        match node {
            crate::Node::Text {
                text,
                anchor,
                continues_x,
                matrix,
                font,
                rotate,
                ..
            } => out.push(crate::Node::Text {
                text,
                anchor,
                continues_x,
                matrix,
                font,
                rotate,
                fill: white.clone(),
                fill_opacity: 1.0,
                stroke: None,
            }),
            crate::Node::Path {
                outline,
                rule,
                clip,
                ..
            } => out.push(crate::Node::Path {
                outline,
                fill: white.clone(),
                rule,
                fill_opacity: 1.0,
                stroke: None,
                clip,
            }),
            crate::Node::Group {
                nodes, clip, mask, ..
            } => {
                let mut inner = Vec::new();
                whiten(nodes, &mut inner);
                out.push(crate::Node::Group {
                    nodes: inner,
                    opacity: 1.0,
                    clip,
                    mask,
                });
            }
            // A picture has no silhouette in a clip: §14.3.5 admits none.
            crate::Node::Image { .. } => {}
        }
    }
}

fn finite(node: &crate::Node) -> bool {
    fn outline(outline: &Outline) -> bool {
        outline.segments.iter().all(|segment| match *segment {
            Segment::Move(p) | Segment::Line(p) => p.iter().all(|v| v.is_finite()),
            Segment::Cubic(a, b, c) => [a, b, c].iter().flatten().all(|v| v.is_finite()),
            Segment::Close => true,
        })
    }
    fn numbers(values: &[f64]) -> bool {
        values.iter().all(|v| v.is_finite())
    }
    fn paint(paint: &Paint) -> bool {
        match paint {
            Paint::Pattern(tile) => numbers(&tile.cell) && numbers(&tile.matrix),
            Paint::Linear {
                from, to, matrix, ..
            } => numbers(from) && numbers(to) && numbers(matrix),
            Paint::Radial {
                centre,
                radius,
                focus,
                matrix,
                ..
            } => numbers(centre) && radius.is_finite() && numbers(focus) && numbers(matrix),
            _ => true,
        }
    }
    // A stroke's width and dashes are lengths, read finite and never
    // multiplied by a transform here, so only its paint — whose matrix is
    // composed — can overflow.
    fn stroke(stroke: Option<&Stroke>) -> bool {
        stroke.is_none_or(|stroke| paint(&stroke.paint))
    }
    match node {
        crate::Node::Path {
            outline: shape,
            fill,
            stroke: line,
            clip,
            ..
        } => {
            outline(shape)
                && paint(fill)
                && stroke(line.as_deref())
                && clip.as_ref().is_none_or(|clip| outline(&clip.outline))
        }
        crate::Node::Text {
            anchor,
            matrix,
            font,
            fill,
            stroke: line,
            rotate,
            ..
        } => {
            anchor.is_none_or(|anchor| numbers(&anchor))
                && rotate.is_finite()
                && numbers(matrix)
                && font.size.is_finite()
                && paint(fill)
                && stroke(line.as_deref())
        }
        crate::Node::Image { rect, matrix, .. } => numbers(rect) && numbers(matrix),
        crate::Node::Group {
            opacity,
            clip,
            mask,
            ..
        } => {
            opacity.is_finite()
                && clip.as_ref().is_none_or(|clip| outline(&clip.outline))
                && mask
                    .as_ref()
                    .is_none_or(|mask| mask.region.as_ref().is_none_or(outline))
        }
    }
}

/// Folds a group's opacity into its one node, where that is the same picture.
///
/// Exactly one node, painting exactly once: a fill and no stroke, a stroke and
/// no fill, a run of text with no outline, or a group with no clip of its own
/// (two opacities composited one inside the other are their product). Returns
/// whether it folded; a `false` leaves `nodes` untouched for the caller to
/// wrap.
fn fold(nodes: &mut [crate::Node], opacity: f64) -> bool {
    let [only] = nodes else {
        return false;
    };
    match only {
        crate::Node::Path {
            fill,
            fill_opacity,
            stroke,
            ..
        } => match (fill, stroke) {
            (Paint::None, Some(stroke)) => {
                stroke.opacity = (stroke.opacity * opacity).clamp(0.0, 1.0);
                true
            }
            (_, None) => {
                *fill_opacity = (*fill_opacity * opacity).clamp(0.0, 1.0);
                true
            }
            _ => false,
        },
        crate::Node::Text {
            fill_opacity,
            stroke: None,
            ..
        } => {
            *fill_opacity = (*fill_opacity * opacity).clamp(0.0, 1.0);
            true
        }
        crate::Node::Group {
            opacity: inner,
            clip: None,
            mask: None,
            ..
        } => {
            *inner = (*inner * opacity).clamp(0.0, 1.0);
            true
        }
        _ => false,
    }
}

/// §10.15's `xml:space="default"`, which is what a document that says nothing
/// means.
///
/// *"First, it will remove all newline characters. Then it will convert all tab
/// characters into space characters. Then, it will strip off all leading and
/// trailing space characters. Then, all contiguous space characters will be
/// consolidated."* Written in that order because the order matters: a newline
/// removed **before** the collapse joins two words that a newline turned into a
/// space would have kept apart.
fn collapse(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for character in text.chars() {
        match character {
            '\n' | '\r' => {}
            ' ' | '\t' => space = true,
            other => {
                if space && !out.is_empty() {
                    out.push(' ');
                }
                space = false;
                out.push(other);
            }
        }
    }
    out
}

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
    build_with(tree, viewport, limits, &crate::Context::NONE)
}

/// [`build`], with the document's references reaching what `context` names.
///
/// # Errors
/// [`build`]'s.
pub fn build_with(
    tree: &Tree,
    viewport: Option<(f64, f64)>,
    limits: &Limits,
    context: &crate::Context<'_>,
) -> Result<Scene, Refusal> {
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
    let mut sheet = style::sheet_with(
        tree,
        css_limits.max_selector_parts,
        &style::Reach {
            imports: context.imports,
            media: style::print(viewport),
        },
    );
    let font_faces = std::mem::take(&mut sheet.font_faces);
    let imports_unresolved = sheet.imports_unresolved;
    let mut walk = Walk {
        tree,
        limits,
        scene: Scene::default(),
        segments: limits.max_segments,
        css: CssBudget::new(&css_limits),
        sheet,
        uses: 0,
        expanding: Vec::new(),
        text: Text::default(),
        pushed: 0,
    };
    if walk.sheet.at_rules > 0 {
        walk.warn(Warning::AtRuleIgnored);
    }
    if imports_unresolved > 0 {
        walk.warn(Warning::ImportUnresolved);
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
            depth: 0,
        },
    )?;
    let mut scene = walk.scene;
    scene.font_faces = font_faces;
    // The size the root stated, not the size the walk happened to leave
    // behind: a nested `<svg>` sets `Frame::viewport` and must not be able to
    // change what the document says it is.
    scene.size = (width.max(0.0), height.max(0.0));
    Ok(scene)
}
