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

pub mod path;
pub mod transform;

#[cfg(test)]
mod tests;

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
    /// Nodes in the finished scene.
    pub max_nodes: usize,
    /// Path commands across the whole document, so that one `d` attribute
    /// cannot be the document.
    pub max_segments: usize,
    /// `<use>` expansions, which are the one place an SVG can grow
    /// multiplicatively.
    pub max_uses: usize,
}

impl Limits {
    /// The shipped ceilings.
    pub const DEFAULT: Self = Self {
        max_depth: 64,
        max_nodes: 65_536,
        max_segments: 1 << 20,
        max_uses: 4_096,
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
    /// `<clipPath>` and `clip-path=`.
    ClipPathUnsupported,
    /// `<pattern>` used as a paint.
    PatternUnsupported,
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
    /// `<linearGradient>`, with its coordinates already resolved into the
    /// space the path is stated in.
    Linear {
        /// Start point.
        from: [f64; 2],
        /// End point.
        to: [f64; 2],
        /// Stops, in ascending offset order.
        stops: Vec<Stop>,
    },
    /// `<radialGradient>`, likewise resolved.
    Radial {
        /// Centre.
        centre: [f64; 2],
        /// Radius.
        radius: f64,
        /// Focal point, which SVG allows to differ from the centre.
        focus: [f64; 2],
        /// Stops, in ascending offset order.
        stops: Vec<Stop>,
    },
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
        stroke: Option<Stroke>,
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
        /// Where it goes, as `x y width height` after transforms.
        rect: [f64; 4],
        /// The matrix mapping the unit image into `rect`'s space.
        matrix: [f64; 6],
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
