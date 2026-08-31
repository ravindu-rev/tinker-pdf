//! SVG 1.1: markup in, a display list out, and no PDF vocabulary anywhere.
//!
//! Feature documentation: `docs/features/epub.md`.
//!
//! The thirteenth leaf, and it is a leaf on ruling 8's *definition* rather than
//! on its shape — bytes and a plain limits struct in, plain structs out. What
//! comes out is a [`Scene`]: filled and stroked paths in user units, with the
//! transforms already composed. Nothing below knows what a page, a content
//! stream or an object is, and the caller that turns a `Scene` into operators
//! is `crates/tinker-pdf/src/epub/`.
//!
//! # Why a display list and not a device
//!
//! The obvious design is a `Device` trait this crate calls back into, which is
//! how `tinker-pdf-content` drives the interpreter (ruling 7). It is the wrong
//! shape here for one reason: ruling 7's seam exists so that *interpretation*
//! happens once and consumers differ, and there is exactly one consumer of an
//! SVG in this repository. A trait would put the facade's vocabulary into this
//! crate's signatures — which is the thing ruling 8 forbids — in exchange for a
//! generality nothing wants. A `Vec` of plain structs costs one allocation per
//! document and keeps the boundary a value.
//!
//! `tinker-pdf-layout` is the precedent: a caller hands it a tree of plain
//! structs and gets boxes back, and ruling 8's amendment names it as the leaf
//! that qualifies on the definition rather than on taking bytes.
//!
//! # What is refused, and why the list is short on purpose
//!
//! An SVG renderer is unbounded if you let it be. Filters, masks, SMIL
//! animation, scripting and `<foreignObject>` are each a whole subsystem, and
//! every one of them is refused **by name and counted** rather than skipped —
//! so a caller learns that a picture was incomplete rather than being handed a
//! plausible one. See [`Warning`].
//!
//! What is *not* refused is the part a book actually uses: geometry, paint,
//! gradients and text.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod document;
pub mod gradient;
pub mod path;
pub mod scene;
pub mod shape;
pub mod style;
pub mod transform;

#[cfg(test)]
mod tests;

/// Reads a document: bytes and a viewport in, a [`Scene`] out.
///
/// `viewport` is the box the document is being placed in, in user units. It is
/// what a percentage on the root resolves against and what a root that states
/// no size of its own falls back to; `None` is [`scene::DEFAULT_VIEWPORT`].
///
/// # Errors
/// [`Refusal`], whose six variants are the whole of what produces no picture
/// at all. Everything else is a [`Scene`] with [`Scene::warnings`] on it.
pub fn read(bytes: &[u8], viewport: Option<(f64, f64)>, limits: &Limits) -> Result<Scene, Refusal> {
    let tree = document::read(bytes, limits)?;
    scene::build(&tree, viewport, limits)
}

/// How much work one document may cost.
///
/// Every field is a ceiling on something an SVG can nest or repeat without
/// bound. They are hardening limits rather than conformance limits: a document
/// past one of them is [`Refusal`], not a smaller picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Limits {
    /// Element nesting, which `<g>` and `<svg>` both add to.
    pub max_depth: usize,
    /// Elements read, **and** nodes in the finished scene.
    ///
    /// One number for two things that a `<use>` makes different — an expansion
    /// draws an element that is already in the tree, so a scene may hold more
    /// nodes than the document has elements. They share a cap because a caller
    /// tuning one and not the other would be tuning half a bound, and because
    /// what the number stands for is the same either way: how much of one
    /// picture a consumer has agreed to hold.
    pub max_nodes: usize,
    /// Path commands across the whole document, so that one `d` attribute
    /// cannot be the document.
    pub max_segments: usize,
    /// `<use>` expansions, which are the one place an SVG can grow
    /// multiplicatively.
    pub max_uses: usize,
    /// Distinct [`Warning`]s one scene may carry.
    ///
    /// Warnings are deduplicated, so this fires only on a document with that
    /// many **different** things to say — which one with half a million
    /// distinct unknown element names has, and which no drawing does. Without
    /// it, [`Warning::ElementUnknown`] would let a file choose how much memory
    /// its own diagnostics cost.
    pub max_warnings: usize,
}

impl Limits {
    /// The shipped ceilings.
    pub const DEFAULT: Self = Self {
        max_depth: 64,
        max_nodes: 65_536,
        max_segments: 1 << 20,
        max_uses: 4_096,
        max_warnings: 256,
    };
}

impl Default for Limits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Why a document produced no scene at all.
///
/// Distinct from [`Warning`], which is a picture that reached the caller with
/// something missing. A `Refusal` means there is no picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refusal {
    /// The bytes are not well-formed XML, or the parser refused them — which
    /// includes `<!DOCTYPE` with an internal subset, refused by name.
    Unreadable,
    /// The root element is not an `<svg>`.
    NotAnSvg,
    /// Past [`Limits::max_depth`].
    TooDeep,
    /// Past [`Limits::max_nodes`].
    TooManyNodes,
    /// Past [`Limits::max_segments`].
    TooManySegments,
    /// Past [`Limits::max_uses`], or a `<use>` that reaches itself.
    TooManyUses,
    /// Selector matching crossed `tinker-pdf-css`'s own budget.
    ///
    /// Named apart from the four above because it is a different half of the
    /// reader and a caller can act on the difference: a document with a million
    /// path segments is [`Refusal::TooManySegments`] and one with a million
    /// selector matches is this. `tinker_pdf::epub::SpineDefect` draws the same
    /// line between `NotStyled` and `NotFragmented`, for the same reason.
    TooMuchStyle,
}

/// Something the picture asked for that this build did not draw.
///
/// Ruling 10: every one names what it affected, and a caller that renders an
/// SVG with warnings has a picture that is incomplete in a way it can report.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Warning {
    /// `<filter>` and every `filter=` that names one. A filter is a raster
    /// pipeline and this crate produces geometry.
    FilterUnsupported,
    /// `<mask>` and `mask=`.
    MaskUnsupported,
    /// A `clip-path` naming something this build cannot turn into an outline.
    ///
    /// **Not the element**: a `<clipPath>` full of shapes is drawn as a clip
    /// since milestone 4. This is the reference that names no `<clipPath>` at
    /// all, or one whose children are `<use>` or `<text>` — geometry that
    /// exists somewhere else. The element is drawn **unclipped**, which is
    /// ruling 2's answer and the one that keeps a picture rather than losing
    /// it; the alternative reading of §14.3.1 would clip everything away.
    ClipPathUnsupported,
    /// `<pattern>` used as a paint.
    PatternUnsupported,
    /// `<marker>`, and the three properties that name one.
    ///
    /// §11.6's vertex decorations: an arrowhead is a whole second rendering of
    /// a referenced subtree at every vertex, rotated to the path's tangent
    /// there. Named rather than folded into [`Warning::ElementUnknown`],
    /// because a `<marker>` is SVG this build declines rather than a
    /// vocabulary it does not read — and thirty-two of them are in the fetched
    /// corpus, every one on a path that also fills.
    MarkerUnsupported,
    /// `<foreignObject>`, whose content is a different document language.
    ForeignObjectUnsupported,
    /// SMIL — `<animate>`, `<set>`, `<animateTransform>` and relatives. A
    /// static rendering is the first frame, and saying so is the point.
    AnimationIgnored,
    /// `<script>`, which is never run.
    ScriptIgnored,
    /// An element from a vocabulary this build does not read.
    ElementUnknown(String),
    /// An attribute value that is not the grammar its property states. The
    /// attribute is dropped and the inherited value stands.
    ValueUnreadable {
        /// The attribute's name.
        attribute: String,
    },
    /// A `<use>` whose `href` names nothing in the document.
    UseUnresolved,
    /// A `fill` or `stroke` of `url(#name)` naming nothing this build can
    /// paint with. The paint's own fallback stands, or `none` when it stated
    /// none — which is §13.2's answer and not an invention here.
    PaintServerUnresolved,
    /// A non-unit `opacity` on something that draws more than once.
    ///
    /// §14.5 makes `opacity` a **group** operation: the subtree is composited
    /// once and the result is faded. This build multiplies it into each
    /// descendant's own fill and stroke alpha instead, which is *exact* for a
    /// single shape painted one way and **too dark where two of them overlap**.
    /// Reported only where it is observable — a lone filled shape at 60 % is
    /// not a warning, because there is nothing wrong with it.
    GroupOpacityFlattened,
    /// An at-rule in a `<style>` element — `@media`, `@import`, `@font-face`.
    /// Skipped by the CSS specification's own recovery, and named.
    AtRuleIgnored,
    /// §10.4's per-glyph positioning: an `x`, `y`, `dx`, `dy` or `rotate`
    /// with **more than one number** in it.
    ///
    /// The first is used and the rest are dropped, which sets the run as one
    /// piece at the right place instead of spreading its letters. Naming it is
    /// the point: a build that took the first number silently would set a
    /// deliberately-spaced line as an ordinary one and look entirely correct.
    TextPositionListIgnored,
    /// `<textPath>`, `<tref>` and `<altGlyph>` — §10.13's text on a path and
    /// its two relatives. Each is a second layout engine.
    TextLayoutUnsupported,
    /// §13.2.3's `spreadMethod` of `reflect` or `repeat`.
    ///
    /// `pad` is drawn instead, which is the initial value and the one every
    /// gradient in the fetched corpus uses. The other two tile the stop list
    /// outside the axis, and doing it honestly means a stitching function over
    /// a repeated domain rather than a wider axis with more stops on it — an
    /// approximation with a chosen number of repeats would be a gradient that
    /// is right in the middle and wrong at the edges.
    SpreadMethodUnsupported,
}

/// A colour, as three components in `[0, 1]`.
///
/// Not an enum over named colours and hex spellings: those are the *grammar*,
/// and by the time a paint reaches a `Scene` the grammar has been read. A
/// consumer that had to know `rebeccapurple` would be reading CSS twice.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Colour {
    /// Red, green, blue, each in `[0, 1]`.
    pub rgb: [f64; 3],
}

/// One stop of a gradient.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stop {
    /// Position along the gradient, in `[0, 1]`.
    pub offset: f64,
    /// The colour there.
    pub colour: Colour,
    /// `stop-opacity`, in `[0, 1]`.
    pub opacity: f64,
}

/// What a shape is painted with.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Paint {
    /// `none`: the shape contributes no ink on this side.
    None,
    /// A flat colour.
    Solid(Colour),
    /// `<linearGradient>`.
    ///
    /// The geometry is in **gradient space** and `matrix` maps that space into
    /// the scene's. It is not baked into the two points, and the reason is a
    /// skew: a linear gradient's iso-lines are perpendicular to its axis *in
    /// its own space*, and an affine transform that is not a similarity does
    /// not keep them perpendicular. Two transformed endpoints cannot say that,
    /// and neither can PDF's axial shading — which is why 8.7.4.5.5 puts a
    /// `/Matrix` on the pattern rather than on the shading's coordinates.
    /// §13.2.3's `gradientUnits` and `gradientTransform` both land here.
    Linear {
        /// Start point, in gradient space.
        from: [f64; 2],
        /// End point, in gradient space.
        to: [f64; 2],
        /// Gradient space to the scene's.
        matrix: [f64; 6],
        /// Stops, in ascending offset order.
        stops: Vec<Stop>,
    },
    /// `<radialGradient>`, on the same terms.
    ///
    /// One circle and a focal point, which is §13.2.3's shape and maps onto
    /// 8.7.4.5.4's two circles with the first one's radius at zero. An
    /// `objectBoundingBox` gradient on a shape that is not square is an
    /// **ellipse**, and it is `matrix` that makes it one.
    Radial {
        /// Centre, in gradient space.
        centre: [f64; 2],
        /// Radius, in gradient space.
        radius: f64,
        /// Focal point, which SVG allows to differ from the centre.
        focus: [f64; 2],
        /// Gradient space to the scene's.
        matrix: [f64; 6],
        /// Stops, in ascending offset order.
        stops: Vec<Stop>,
    },
}

/// §10.9's `text-anchor`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextAnchor {
    /// `start`, the initial value.
    #[default]
    Start,
    /// `middle`.
    Middle,
    /// `end`.
    End,
}

/// The font a run is set in, as **properties rather than a face**.
///
/// Every field is what the document said, and none of them is a font: this
/// crate has no font vocabulary at all, which is ruling 8 and the reason
/// [`Node::Text`] exists in this shape. See that variant's own note.
#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    /// `font-family`, in the author's order, with the generic families left
    /// in — a caller resolving `serif` is answering a question about the faces
    /// it has, which this crate does not have.
    pub families: Vec<String>,
    /// `font-size`, in user units, already resolved through `em` and `%`.
    pub size: f64,
    /// `font-weight`, as a number in `[100, 900]`.
    pub weight: u16,
    /// Whether `font-style` is `italic` or `oblique`. The two are one question
    /// for a caller that has at most one slanted face per family, which is
    /// every caller this repository has.
    pub italic: bool,
    /// `text-anchor`, which decides where the **chunk** sits once its width is
    /// known — and its width is known only to whoever has the metrics.
    pub anchor: TextAnchor,
}

/// §14.3's clipping path, as geometry.
///
/// The **union** of the shapes a `<clipPath>` holds, as one outline of several
/// subpaths, already in the scene's space. A union rather than a list because
/// §14.3.5 says a clipping path is *"the union of the silhouettes"* of its
/// children — a consumer handed a list would have to intersect them, which is
/// the opposite operation and would clip away everything two children did not
/// share.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    /// The outline.
    pub outline: path::Outline,
    /// `clip-rule`, which is a **separate property** from `fill-rule`: one
    /// shape used as a clip and as a fill can want two different rules.
    pub rule: FillRule,
}

/// SVG 1.1 §11.3's `fill-rule`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FillRule {
    /// `nonzero`, the initial value.
    #[default]
    NonZero,
    /// `evenodd`.
    EvenOdd,
}

/// §11.4's `stroke-linecap`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LineCap {
    /// `butt`, the initial value.
    #[default]
    Butt,
    /// `round`.
    Round,
    /// `square`.
    Square,
}

/// §11.4's `stroke-linejoin`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LineJoin {
    /// `miter`, the initial value.
    #[default]
    Miter,
    /// `round`.
    Round,
    /// `bevel`.
    Bevel,
}

/// How a shape's outline is drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    /// The paint.
    pub paint: Paint,
    /// `stroke-width`, in user units.
    pub width: f64,
    /// `stroke-linecap`.
    pub cap: LineCap,
    /// `stroke-linejoin`.
    pub join: LineJoin,
    /// `stroke-miterlimit`.
    pub miter_limit: f64,
    /// `stroke-dasharray`, empty for a solid line.
    pub dashes: Vec<f64>,
    /// `stroke-dashoffset`.
    pub dash_offset: f64,
    /// `stroke-opacity`, in `[0, 1]`.
    pub opacity: f64,
}

/// One drawable thing.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Node {
    /// A path, already in the coordinate space the scene states.
    Path {
        /// The outline, with every transform composed in.
        outline: path::Outline,
        /// How the interior is painted, and by which rule.
        fill: Paint,
        /// The fill rule.
        rule: FillRule,
        /// `fill-opacity`, in `[0, 1]`.
        fill_opacity: f64,
        /// How the outline is drawn, if at all.
        ///
        /// **Boxed**, and it is a size decision rather than a taste one: a
        /// [`Stroke`] carries its own [`Paint`], which for a gradient is a
        /// `Vec` of stops and a matrix, and most shapes in a real drawing only
        /// fill. Inline it made this variant three times the size of
        /// [`Node::Image`] and every node in a ten-thousand-node scene paid
        /// for it.
        stroke: Option<Box<Stroke>>,
        /// §14.3's clip, or `None` for a node that is not clipped.
        clip: Option<Clip>,
    },
    /// A run of text, carried **unshaped**.
    ///
    /// # The seam, and why it is here
    ///
    /// Ruling 8 forbids this crate from learning what a font is, and setting
    /// text needs one: a face has to be matched per character
    /// (`css-fonts-4` §5.3), shaped through `GSUB`/`GPOS`, and turned into
    /// glyph indices. All three live in `tinker-pdf-font` and
    /// `tinker-pdf-shape`, which this crate does not take an edge to and must
    /// not — an SVG reader that carried a shaper would be unfuzzable on its
    /// own and unpublishable apart from them.
    ///
    /// So the split is: **everything about the document happens here** —
    /// reading `<text>` and `<tspan>`, resolving `x`/`y`/`dx`/`dy` into an
    /// anchor, composing the matrix, and resolving the font *properties*
    /// through the same §6.4 machinery every other element uses — and
    /// **everything about a font happens in the caller**, which is
    /// `crates/tinker-pdf/src/epub/`, using the same `choose` and the same
    /// shaper that set the rest of the book. That is the property worth
    /// having: SVG text and XHTML text in one book cannot be set in two
    /// different faces by two different matchers.
    ///
    /// [`Node::Image`] is the same seam read once already — the crate carries
    /// what the document said and the caller resolves it against what it has.
    ///
    /// # Chunks
    ///
    /// §10.9 starts a **text chunk** at every absolute position, and
    /// [`Node::Text::anchor`] is `None` for a run that continues the one
    /// before it. Where a continuing run *begins* depends on how wide the
    /// previous one was, which is a metric — so the pen is the caller's to
    /// track, and so is `text-anchor`, which cannot be applied until a whole
    /// chunk's width is known.
    Text {
        /// The characters, with `xml:space`'s default white-space handling
        /// already applied.
        text: String,
        /// Where this run starts, in the space `matrix` maps out of, or `None`
        /// to continue from where the previous run ended.
        anchor: Option<[f64; 2]>,
        /// The matrix from that space into the scene's.
        matrix: [f64; 6],
        /// The font properties, resolved but not matched.
        font: TextStyle,
        /// How the glyphs are filled.
        fill: Paint,
        /// `fill-opacity` times every `opacity` above it.
        fill_opacity: f64,
        /// How the glyphs are outlined, if at all.
        stroke: Option<Box<Stroke>>,
    },
    /// An `<image>`, carried **unresolved**.
    ///
    /// The href is not fetched here for the reason `tinker-pdf-css`'s
    /// `ImportResolver` exists: this crate has no container, no filesystem and
    /// no network, which is what makes it a leaf. The caller resolves it
    /// against whatever it has.
    Image {
        /// The `href`, verbatim.
        href: String,
        /// Where it goes, as `x y width height`, in the element's **own** user
        /// space — the space `matrix` maps out of.
        rect: [f64; 4],
        /// The matrix from that space into the scene's, every ancestor's
        /// transform and every viewport composed in.
        matrix: [f64; 6],
        /// `preserveAspectRatio`, verbatim, or `None` for §7.8's initial
        /// `xMidYMid meet`.
        ///
        /// **Carried unresolved for the reason `href` is.** Fitting an image
        /// into `rect` needs the image's *intrinsic* size, which is inside
        /// bytes this crate never sees. The caller that decoded it passes both
        /// to [`transform::view_box`], which is where the grammar already
        /// lives — so the string travels and the reading of it does not
        /// happen twice.
        preserve: Option<String>,
    },
}

/// A whole document, read.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Scene {
    /// The `width` and `height` the root states, in user units, after the
    /// viewport mapping.
    pub size: (f64, f64),
    /// What to draw, in document order — which is paint order, because SVG has
    /// no `z-index` and §3.3 makes later elements paint over earlier ones.
    pub nodes: Vec<Node>,
    /// Everything the picture asked for that this build did not draw.
    pub warnings: Vec<Warning>,
}
