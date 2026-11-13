//! The rasterizing `Device`: content interpretation drawn through the
//! rasterizer.
//!
//! This crate is the bridge. The interpreter in `tinker-pdf-content` knows
//! what a page asks for; `tinker-pdf-raster` knows how to put it on pixels;
//! neither knows about the other, and this joins them.
//!
//! Feature documentation: `docs/features/rendering.md`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use core::mem;
use tinker_pdf_raster::blend::BlendMode as RasterBlend;

pub mod display;
mod glyph;
mod glyph_cache;
pub mod mesh;
#[cfg(test)]
mod run_shadows;
pub mod shading;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tinker_pdf_color::icc::Conversion;

pub use display::{Admission, DisplayRecorder, MAX_DISPLAY_LIST_BYTES};
pub use mesh::{Mesh, MeshParams};
pub use shading::{ColorTable, Shading, ShadingScratch, TABLE_ENTRIES};
use tinker_pdf_color::{ColorSpace, Function};
use tinker_pdf_content::{
    ColorFamily, Device, EventKind, Glyph, GraphicsState, ImageRef, LineCap as ContentCap,
    LineJoin as ContentJoin, Matrix, PathSegment, RenderingIntent, StoredParameters, Transfer,
};

use tinker_pdf_font::Outline;
use tinker_pdf_raster::{
    canvas::{Canvas, Color, InkTables, MaskKind, PixelFormat},
    fill::{
        fill_hard_with, fill_outline, fill_outline_hard, fill_with, FillScratch, HardEdge, Mask,
    },
    fragments::Fragments,
    geom::{FillRule, Path},
    image::{
        accumulate_image, draw_image, image_bounds_in, image_coverage, ImageDraw, ImageSource,
        Pyramid, SourceWindow, Transform, TwoColour,
    },
    mesh::{draw_mesh_over, MeshDraw},
    stroke::{
        adjusted_to_pixels, stretches, stroke_mapped_with, stroke_with, whole_pixel_width, LineCap,
        LineJoin, StrokeStyle,
    },
};

/// How far apart a map's two stretches may be, as a fraction of the larger,
/// and still be stroked as a similarity: rounding, and nothing a page states.
/// A rotation composed of several `cm`s is a similarity whose entries no
/// longer cancel exactly; one written with five decimals is not, and is
/// stroked through user space (`Renderer::pen`).
const SIMILARITY_SLACK: f64 = 1e-9;

/// The most a map may stretch one direction over another before a stroke
/// under it is no longer taken back to user space: past this, a point's
/// round trip through the inverse loses more than a thousandth of a pixel
/// at a page's own coordinates, and the map is all but a collapse anyway.
const STROKE_CONDITION: f64 = 1e9;

/// Whether a stroke under `to_device` — user space to the device — has to be
/// drawn in user space, and if so the map's largest stretch.
///
/// 8.4.3.2 measures a pen in user space. Under a similarity that is one
/// device width, the user width times [`Matrix::expansion`], and the answer
/// is `None`: stroke in device space, which every writer of a pen can say
/// with one number. Under any other map no single width is the pen, and the
/// answer is `Some(largest)`, the factor the map stretches its most-stretched
/// direction by — the renderer strokes through user space
/// (`tinker_pdf_raster::stroke_mapped_with`), and the SVG writer states the path
/// in a space this much larger than user space, where the pen is round.
///
/// Also `None` for a map that is not finite or that stretches one direction
/// more than [`STROKE_CONDITION`] times another — a near-collapse, which is
/// stroked as a similarity at its expansion, the only answer left once a
/// round trip through its inverse costs a path its position.
#[must_use]
pub fn user_space_pen(to_device: &Matrix) -> Option<f64> {
    let (largest, smallest) = stretches(&transform(to_device));
    if !(largest.is_finite() && largest > 0.0) {
        return None;
    }
    // A similarity's two stretches are equal; the slack is rounding, so a
    // product of rotations that is a similarity in exact arithmetic is
    // stroked as one.
    let similar = largest - smallest <= largest * SIMILARITY_SLACK;
    (!similar && smallest * STROKE_CONDITION >= largest).then_some(largest)
}

/// `map`'s inverse, or `None` for a map whose inverse is not finite. Unlike
/// `invert`, no determinant is too small by itself: how far a stroke may
/// trust the round trip is [`STROKE_CONDITION`]'s question, and it is a ratio.
fn pen_inverse(map: &Transform) -> Option<Transform> {
    let det = map.a * map.d - map.b * map.c;
    if !det.is_finite() || det == 0.0 {
        return None;
    }
    let inverse = Transform {
        a: map.d / det,
        b: -map.b / det,
        c: -map.c / det,
        d: map.a / det,
        e: (map.c * map.f - map.d * map.e) / det,
        f: (map.b * map.e - map.a * map.f) / det,
    };
    [
        inverse.a, inverse.b, inverse.c, inverse.d, inverse.e, inverse.f,
    ]
    .iter()
    .all(|v| v.is_finite())
    .then_some(inverse)
}

/// Lets a caller stop a render that is taking too long.
///
/// Cloned into whatever holds it; the renderer checks it between operations
/// and between scanline bands, so a cancelled render stops promptly without
/// the interpreter having to unwind.
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// A token that has not been cancelled.
    #[must_use]
    pub fn new() -> CancelToken {
        CancelToken::default()
    }

    /// Asks the render to stop.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Whether cancellation has been requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Something the renderer could not do exactly, reported rather than hidden.
///
/// Ruling 2: a page never fails because one thing on it was unsupported. It
/// renders what it can, substitutes a neutral placeholder for what it cannot,
/// and says so here.
// `Eq` is not derivable now that a variant carries the scale, and a scale is
// an `f64`. `PartialEq` is what the deduplication in the renderer uses.
#[derive(Clone, Debug, PartialEq)]
pub enum RenderWarning {
    /// One of 11.3.5.3's four non-separable blend modes was applied inside a
    /// `/DeviceCMYK` transparency group.
    ///
    /// Those modes reason about hue, saturation and luminosity, which ink
    /// quantities do not have, so the operands convert to light, blend there,
    /// and convert back. The round trip is not free: `rgb_to_cmyk` produces one
    /// particular ink split, so a rich black arrives back as its pure-K
    /// equivalent. The colour is the same and the *next* blend over it is not,
    /// which is why ruling 10 wants it said rather than done quietly.
    ///
    /// Reported once per group, not once per pixel.
    ApproximatedGroupBlend,
    /// An image used a codec that is not built in; a placeholder was drawn.
    UnsupportedImage {
        /// Which codec.
        codec: String,
    },
    /// An image decoded, but something had to be tolerated to get it: a
    /// damaged row replicated, a stream that ended short padded, a decode
    /// ceiling reached.
    ///
    /// Ruling 10 — the image is drawn *and* named, so "the scan decoded" and
    /// "the scan decoded cleanly" stay distinguishable. Without this a fax
    /// with half its rows missing looks exactly like a fax of a mostly blank
    /// page, which is the most common thing a fax is.
    DamagedImage {
        /// The resource name the image was drawn under.
        name: String,
        /// The decoder's own stable identifier for what it tolerated.
        reason: String,
    },
    /// A colour space's parameters said something no space can mean, and
    /// were read the nearest way that means something.
    ///
    /// Ruling 10, for the same reason as [`RenderWarning::DamagedImage`]: the
    /// page is drawn, and "the colours were read" stays distinguishable from
    /// "the colours were read as written". `reason` is a stable identifier:
    /// `LabRangeUnordered` — a `/Lab` space whose `/Range` writes a pair
    /// backwards, `[10 -10 …]`, which 8.6.5.4's Table 65 makes a minimum then
    /// a maximum, read as the span it covers — or, under a default colour
    /// space's own name (`DefaultGray`, `DefaultRGB`, `DefaultCMYK`), a
    /// default 8.6.5.6 does not allow, refused and its device space drawn as
    /// without it: `DefaultSpaceFamily` for a `/Lab`, `/Indexed` or
    /// `/Pattern` one, `DefaultSpaceComponents` for one with another number
    /// of components than its device space, `DefaultSpaceUnreadable` for one
    /// that is no colour space — or, under a JPEG's or a JPEG 2000 image's
    /// name, `ImageSpaceComponents`: a `/ColorSpace` with another number of
    /// components than the codec's samples, refused and the image drawn in
    /// the codec's own colour.
    ///
    /// Reported once per name and repair, wherever the facade reports what
    /// its resources tolerated — a space read through the page's resources
    /// or a tiling cell's — and not yet through a form XObject's or an
    /// annotation appearance's own `/Resources`, which keep what they
    /// tolerate to themselves (`rendering.md`'s refusal table).
    RepairedColorSpace {
        /// The resource name the space was reached under: a `/ColorSpace`
        /// entry's for `cs`, or the image's, shading's, pattern's, form's or
        /// graphics state's whose dictionary names the space — `Group` for a
        /// page's own `/Group`, and the default's own for a refused default.
        name: String,
        /// What was repaired.
        reason: String,
    },
    /// The page was too large to render at the requested resolution, so it
    /// was rendered smaller.
    ///
    /// Ruling 2: the caller gets a whole page rather than a fragment of one,
    /// and is told the scale is not the one they asked for. Without this the
    /// only signal is a bitmap whose dimensions do not match the arithmetic
    /// the caller just did.
    PageScaledDown {
        /// The scale that was asked for.
        requested: f64,
        /// The scale that was used.
        applied: f64,
    },
    /// A text object selected a clipping mode and produced no glyphs, so it
    /// clipped everything away.
    ///
    /// Spec-correct and almost never intended: it usually means the glyphs
    /// could not be resolved, and the visible result is a blank region rather
    /// than missing text.
    EmptyTextClip,
    /// A font program could not be read, so its glyphs were not drawn.
    UnreadableFont,
    /// A shading type that is not implemented was skipped.
    UnsupportedShading {
        /// The shading type number.
        kind: i64,
    },
    /// A pattern this build cannot paint was left unpainted.
    ///
    /// Reported rather than filled with the black that `/Pattern` nominally
    /// reports as its colour: an unpainted area reads as missing, whereas a
    /// black one reads as content and hides the gap.
    UnsupportedPattern {
        /// The resource name.
        name: String,
    },
    /// An optional-content layer the document's default configuration turns
    /// off was not painted (8.11).
    ///
    /// Correct behaviour rather than a degradation, and reported all the
    /// same, because it is the one leniency a reader cannot see for
    /// themselves: content that is missing because a layer is off looks
    /// exactly like content that was never there. Ruling 10 — the layer is
    /// named, so a caller can say *which* one and offer to turn it on.
    HiddenOptionalContent {
        /// The layer's `/Name` (8.11.2.1), which is the string a viewer's
        /// layer panel shows, falling back to the resource name.
        layer: String,
    },
    /// A page asked for more transparency groups than one render will open,
    /// and the excess were declined (11.4.7, and `MAX_GROUP_BUFFERS`).
    ///
    /// This is a *budget*, not a nesting limit: a group whose soft mask names
    /// a stream that opens further groups branches rather than descends, so a
    /// page can stay inside the depth cap and still ask for exponentially
    /// many buffers. What follows the decline is drawn without the group it
    /// asked for, which is ruling 2's answer -- the page is a little wrong
    /// rather than never finishing.
    GroupBudgetSpent {
        /// How many buffers were opened before the budget ran out.
        opened: u32,
    },
    /// A form, a soft mask's group or a Type 3 glyph procedure named itself
    /// while it was running, directly or through others, and was drawn once
    /// (8.10).
    ///
    /// A cycle has no last run: drawn as written it never finishes, and a
    /// depth cap ends it only after two or more runs to the sixteenth when it
    /// branches. The repeat is passed over at its first appearance, which is
    /// the page every reader can draw.
    RecursiveContent,
    /// A page ran its forms, soft masks' groups and Type 3 glyph procedures
    /// more times over than one interpretation runs, and the runs past the
    /// budget were not drawn (`NESTED_WORK_FLOOR`, `NESTED_WORK_RATE`).
    ///
    /// A budget scaled to the content the page holds: every stream the page
    /// reaches still runs once, and what is passed over is further runs of
    /// content already drawn — a tree of forms each drawing the next many
    /// times, which no depth cap bounds.
    NestedBudgetSpent,
    /// The render stopped because it was cancelled.
    Cancelled,
    /// More distinct sets of transfer functions than one page keeps per
    /// point (11.7.5.2): past 255, a further set's paints are kept, and
    /// drawn, as the device's own (ruling 2).
    DeviceParametersSpent,
    /// A [`PixelRegion`] reached past the edge of the page, so the part that
    /// is on the page was rendered and the rest was trimmed away.
    ///
    /// Ruling 2 and ruling 10 together: the caller gets the pixels that exist
    /// rather than an error, and is told the bitmap is not the size they asked
    /// for — which they would otherwise discover by arithmetic, or not at all.
    /// `applied` is the intersection of the ask with the page, so an `applied`
    /// of zero width or height means the region missed the page entirely and
    /// the bitmap has no pixels.
    RegionClamped {
        /// The region the caller asked for.
        requested: PixelRegion,
        /// The part of it that is on the page, which is what was rendered.
        applied: PixelRegion,
    },
    /// The render named an optional-content configuration the document does
    /// not have — an index past `/OCProperties /Configs`, or of an entry that
    /// is no dictionary (8.11.4.3) — and drew the default configuration's
    /// states instead.
    ///
    /// Ruling 2 and ruling 10 together: the page is drawn, in the states every
    /// reader starts from, and the caller is told its choice was not the one
    /// drawn, since a page in `/D`'s states looks like a page in any other.
    UnreadableLayerConfiguration {
        /// The index the render named.
        index: usize,
    },
}

/// What a pattern name paints with (8.7.3).
#[derive(Clone, Debug)]
pub enum PatternPaint {
    /// A shading pattern, and the matrix mapping pattern space to the page's
    /// default space.
    Shading(Box<Shading>, Matrix),
    /// A tiling pattern's geometry (8.7.3.2). The cell's *pixels* come
    /// separately, from [`GlyphSource::tile`], because rasterizing them means
    /// running a content stream and this crate has no interpreter.
    Tiling(Box<TilingPattern>),
    /// A pattern this build does not paint — a tiling pattern whose cell could
    /// not be read, a shading whose dictionary says nothing usable.
    ///
    /// The area is left alone and reported, rather than filled with the black
    /// that `/Pattern` nominally reports as its colour: an unpainted area
    /// reads as missing, a black one reads as content.
    Unsupported,
}

/// A tiling pattern's geometry (8.7.3.2), without its pixels.
///
/// Everything here is decided before a single pixel is rasterized, which is
/// the point of the split: the lattice, and therefore whether the lattice is
/// affordable at all, is known from these six numbers, so a pathological
/// `/XStep` costs a division rather than a cell.
#[derive(Clone, Copy, Debug)]
pub struct TilingPattern {
    /// `/Matrix`: pattern space to the *default* space of the pattern's parent
    /// content stream (8.7.3.1) — not to the CTM in force when it is used.
    pub matrix: Matrix,
    /// `/BBox` in pattern space, as `[x0, y0, x1, y1]` with `x0 < x1`.
    pub bbox: [f64; 4],
    /// `/XStep`: how far apart neighbouring cells sit horizontally in pattern
    /// space. May be smaller than the bounding box (cells overlap) or larger
    /// (cells leave gaps).
    pub xstep: f64,
    /// `/YStep`, vertically.
    pub ystep: f64,
    /// `/PaintType 2`: the cell is a shape, and takes the colour the `SCN`
    /// operands supplied rather than any colour of its own (8.7.3.3).
    pub uncolored: bool,
}

/// What one cell of a tiling pattern has to be drawn into.
///
/// The renderer decides all of this — it is the only thing that knows `base`,
/// the canvas format and the lattice — and the resource layer only runs the
/// content stream, because that is the half this crate cannot do.
#[derive(Clone, Debug)]
pub struct TileRequest {
    /// Pattern space to the tile buffer's own pixels: `/Matrix`, then the
    /// page's `base`, then the translation that puts the cell's device
    /// rectangle at the buffer's origin.
    ///
    /// `base` and not the paint-time CTM, which is 8.7.3.1 and the whole
    /// correctness question in a tiling pattern.
    pub to_pixels: Matrix,
    /// `/BBox` in pattern space, so the cell can be clipped to it by the same
    /// numbers the buffer was sized from. Passed rather than re-read: two
    /// spellings of one rectangle is how a buffer and its clip come to
    /// disagree about where the cell ends.
    pub bbox: [f64; 4],
    /// The buffer's width in pixels.
    pub width: u32,
    /// The buffer's height in pixels.
    pub height: u32,
    /// The buffer's format, always one carrying alpha: a cell is a shape over
    /// nothing, and a format without alpha cannot say where it did not paint.
    ///
    /// No colour rides with the request. A `PaintType 2` cell is a shape
    /// (8.7.3.3), drawn as its content stream draws it, and the renderer
    /// composites its coverage in the `SCN` colour
    /// ([`Canvas::composite_tinted`]), so what a cell's pixels hold never
    /// depends on the colour it is filled in and one cell serves every
    /// colour.
    pub format: PixelFormat,
    /// The render's cancellation token, so a cell stops with the page.
    pub cancel: CancelToken,
    /// How many tiling patterns are already open above this one, for the
    /// recursion cap.
    pub depth: u32,
    /// Whether the cell's own edges are anti-aliased, which is the page's
    /// answer: a cell drawn soft and composited onto a hard page would put
    /// partial pixels on a page that asked for none. See
    /// [`Renderer::with_antialias`].
    pub antialias: bool,
    /// The page's graphics level, for the cell's own graphics
    /// ([`Renderer::with_levels`]).
    pub graphics_bits: u8,
    /// The page's text level, for text in the cell.
    pub text_bits: u8,
    /// The page's rule at level 0 ([`Renderer::with_hard_edge`]).
    pub hard_edge: HardEdge,
    /// The page's thinnest stroke, in device pixels, which a cell drawn at
    /// the page's resolution keeps ([`Renderer::with_thinnest_line`]).
    pub thinnest_line: Option<f64>,
}

/// One rasterized tiling-pattern cell.
#[derive(Clone, Debug)]
pub struct Tile {
    /// The cell's pixels, transparent where it did not paint.
    pub canvas: Canvas,
    /// Whether every paint of the cell was fully opaque, which 11.7.5.2
    /// asks of a tiling pattern's cell before an object painted with it is
    /// ([`Renderer::painted_opaquely`]).
    pub opaque: bool,
    /// What drawing the cell had to tolerate. Merged into the page's warnings
    /// rather than dropped — a JPX inside a pattern is exactly as missing as a
    /// JPX anywhere else, and the page is the only place a caller looks.
    pub warnings: Vec<RenderWarning>,
}

/// A decoded image, ready to draw.
///
/// Held one of two ways: as RGB, three bytes a sample and a byte of opacity
/// where a mask gives one, or — for an image whose every sample is one of two
/// colours, a one-component one-bit image — as those two colours and one byte
/// a sample ([`DecodedImage::two_colour`]), which the rasterizer samples in
/// one channel ([`tinker_pdf_raster::TwoColour`]). A 2480 by 3508 scan is
/// then held at one byte a sample rather than three or four, and so is every
/// copy a cache hands out.
#[derive(Clone, Debug)]
pub struct DecodedImage {
    /// Width in samples.
    pub width: u32,
    /// Height in samples.
    pub height: u32,
    /// RGB, three bytes per sample, row-major from the top; empty where
    /// [`Self::two_colour`] holds the samples.
    pub rgb: Vec<u8>,
    /// Per-sample opacity from a mask; empty means fully opaque. Empty
    /// where [`Self::two_colour`] holds the samples, whose colours carry
    /// their own.
    pub alpha: Vec<u8>,
    /// The image as its two colours and one byte a sample, in place of
    /// `rgb` and `alpha`; `None` for an image held as RGB.
    pub two_colour: Option<TwoColourSamples>,
    /// Whether this is a stencil mask (8.9.6.2).
    ///
    /// A stencil carries no colour of its own: it selects where the *current
    /// fill colour* is painted. Baking black in at decode time — which is
    /// what happens without this flag — paints every stencil black whatever
    /// the page asked for.
    pub stencil: bool,
    /// The image dictionary's `/Interpolate` (Table 89).
    ///
    /// Opt-*in* smoothing when the image is magnified. It is the file's own
    /// statement about its content — a photograph says yes, a barcode or a
    /// screenshot says nothing and means it — so it is carried here rather
    /// than guessed at from the sample count.
    pub interpolate: bool,
    /// Where the decode made the image smaller than it is, the image it
    /// stands for (`docs/design/jpeg-speed.md`, "Placing the reduced grid");
    /// `None` for an image decoded at its full size, which is every image but
    /// a JPEG or a JPEG 2000 codestream a draw asked for at a fraction of its
    /// size.
    pub reduced: Option<Reduced>,
    /// The device space family the samples are in ([`ColorFamily`]): what
    /// black generation and undercolour removal (11.7.5.3) and the grey
    /// transfer function (10.5) ask. A stencil's colour is the fill's, which
    /// the renderer reads from the state, so a stencil says `Other`.
    pub family: ColorFamily,
    /// Whether `alpha` is opacity — an `/SMask`, or a JPEG 2000 image's own
    /// under `/SMaskInData` — rather than a `/Mask`'s shape. 11.7.5.2 counts
    /// an image with a soft mask as not fully opaque, and its shape is its
    /// whole square (11.6.4.2).
    pub soft_masked: bool,
    /// The rectangle of the image the samples hold, where the decode held
    /// less than the whole of it (`docs/design/jpx-speed.md`, "The visible
    /// window"): `rgb` and `alpha` are that rectangle's rows, and
    /// [`Self::width`] and [`Self::height`] stay the whole image's, which the
    /// draw is placed and sampled by. `None` where every sample is held,
    /// which is every image but a JPEG 2000 codestream a draw showed part of.
    pub window: Option<SourceWindow>,
}

/// A rectangle of an image's unit square, `(0, 0)` its bottom-left corner
/// and `(1, 1)` its top-right (8.9.5.2): `u` across the image's columns and
/// `v` up its rows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnitArea {
    /// The left edge.
    pub u0: f64,
    /// The bottom edge.
    pub v0: f64,
    /// The right edge.
    pub u1: f64,
    /// The top edge.
    pub v1: f64,
}

impl UnitArea {
    /// The part of the unit square `to_device` puts on the device pixels of
    /// `rect`, `(x0, y0, x1, y1)` with the far edges exclusive: the
    /// rectangle's corners mapped back into the square, their bounding box,
    /// clamped to it — empty, with `u0 == u1` or `v0 == v1`, where the square
    /// misses the rectangle. `None` where the map has no inverse or the
    /// corners are no numbers, which asks for the whole image.
    #[must_use]
    pub fn of_device(to_device: &Matrix, rect: (f64, f64, f64, f64)) -> Option<UnitArea> {
        let inverse = invert(to_device)?;
        let (x0, y0, x1, y1) = rect;
        let corners = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)].map(|(x, y)| inverse.apply(x, y));
        if corners
            .iter()
            .any(|(u, v)| !u.is_finite() || !v.is_finite())
        {
            return None;
        }
        let clamp = |value: f64| value.clamp(0.0, 1.0);
        let fold = |pick: fn(&(f64, f64)) -> f64, start: f64, keep: fn(f64, f64) -> f64| {
            corners.iter().map(pick).fold(start, keep)
        };
        let u0 = clamp(fold(|c| c.0, f64::INFINITY, f64::min));
        let u1 = clamp(fold(|c| c.0, f64::NEG_INFINITY, f64::max));
        let v0 = clamp(fold(|c| c.1, f64::INFINITY, f64::min));
        let v1 = clamp(fold(|c| c.1, f64::NEG_INFINITY, f64::max));
        if x1 <= x0 || y1 <= y0 {
            return Some(UnitArea {
                u0,
                v0,
                u1: u0,
                v1: v0,
            });
        }
        Some(UnitArea { u0, v0, u1, v1 })
    }

    /// Whether it holds no part of the square.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.u1 <= self.u0 || self.v1 <= self.v0
    }
}

/// A reduced decode's grid: [`DecodedImage::width`] by
/// [`DecodedImage::height`] samples standing for an image of
/// `full_width` by `full_height`, each sample for `factor` of the image's own
/// samples each way, the first from `skip` of them in.
///
/// Sample `X` covers the image's columns `skip + factor·X` up to
/// `skip + factor·(X + 1)`, the last cut where the image ends, and rows
/// alike. So the draw maps the unit square onto `full_width / factor` by
/// `full_height / factor` samples — the last column and row cut at the
/// square's edge rather than stretched to it — and the resampler draws that,
/// so the reduced image covers what the image covers. A column the first
/// sample's cell starts after takes that sample, as an image's own edge
/// does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Reduced {
    /// The image's own width, which the unit square spans.
    pub full_width: u32,
    /// The image's own height.
    pub full_height: u32,
    /// How many of the image's samples each way one decoded sample stands
    /// for: 2, 4 or 8 for a JPEG decoded at a half, a quarter or an eighth,
    /// and `2^d` for a JPEG 2000 codestream with `d` levels dropped.
    pub factor: u32,
    /// How many of the image's own columns and rows stand before the first
    /// decoded sample's cell, less than `factor`: none for a JPEG, whose
    /// blocks start at its corner, and none for a JPEG 2000 image whose
    /// origin on the reference grid is a multiple of `factor` — the reduced
    /// grid's cells are `factor` of the full grid's from the grid's own
    /// origin (T.800 B.5), and an image that starts inside one starts that
    /// far from the next.
    pub skip: (u32, u32),
}

/// What a draw asks of an image's decode (`docs/design/jpeg-speed.md`, "The
/// drawn-size request"; `docs/design/jpx-speed.md`, "The visible window"):
/// the size the page draws it at, and the parts of it the draw can reach and
/// show.
///
/// The size is computed from the image's transform in the page frame — the
/// frame a tile and a band share with the page (ruling 5), and the one a
/// display list's replay computes from the transform it replays at — so every
/// part of a page asks for the size the page asks for. The parts are the
/// canvas's own: a tile reaches less of an image than the page does, and a
/// decoder that holds only what a draw reaches — widened by the sampler's
/// reach — hands every pixel of the canvas the samples the page's decode
/// hands it, so the tile is still the page under it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageRequest {
    /// The device pixels the image's unit square spans along each of the
    /// image's own axes, its width first: `√(a² + b²)` and `√(c² + d²)` of its
    /// transform. A size that is not finite asks for the image at full size.
    pub drawn: (f64, f64),
    /// The part of the unit square the draw can put a pixel on: the canvas's
    /// device pixels mapped back into it ([`UnitArea::of_device`]) — the
    /// page's, a tile's or a band's, a group's buffer inside one or a
    /// pattern cell's — and the clip's and the soft mask's rectangles too
    /// where the draw composites through them on its own, an element of a
    /// knockout group or an image on a page too large to hold its images in
    /// runs. Elsewhere a draw joins a run of images that holds what it
    /// accumulates wherever the clip hides it, and a later draw joins the run
    /// only where it does not overlap what the run holds, so what the run
    /// holds under the clip still decides pixels. `None` reaches all of it.
    pub reach: Option<UnitArea>,
    /// The part of [`Self::reach`] the clip and the soft mask leave visible.
    /// A decoder may hold only the samples under it, widened by the sampler's
    /// reach, for an image every sample of which is opaque: a window reads its
    /// nearest sample where it holds none (`ImageSource::window`), so every
    /// pixel of the square is still covered, wholly where the image is, and a
    /// run decides what it overlaps as it would over the whole image — which
    /// a stand-in's opacity would not settle for an image with opacity of its
    /// own. `None` shows all of it.
    pub visible: Option<UnitArea>,
}

impl ImageRequest {
    /// The image at its full size, whatever it is drawn at: what the SVG
    /// writer, `Page::images` and recoding ask, and what an image with a mask
    /// is decoded at.
    pub const FULL: ImageRequest = ImageRequest {
        drawn: (f64::INFINITY, f64::INFINITY),
        reach: None,
        visible: None,
    };

    /// The request a draw makes whose unit square `to_device` maps into the
    /// page frame: the length of each of its two columns, and every part of
    /// it. `sqrt` is correctly rounded, so every target asks for the same
    /// size (ruling 4).
    #[must_use]
    pub fn drawn_under(to_device: &Matrix) -> ImageRequest {
        let across = (to_device.a * to_device.a + to_device.b * to_device.b).sqrt();
        let down = (to_device.c * to_device.c + to_device.d * to_device.d).sqrt();
        ImageRequest {
            drawn: (across, down),
            reach: None,
            visible: None,
        }
    }
}

/// The transform of the square a reduced image's samples span, from the
/// image's own unit square `unit` (`docs/design/jpeg-speed.md`, "Placing the
/// reduced grid"), or `None` where the grid says nothing that can be drawn.
///
/// The `width` samples, each `factor` of the image's own, span
/// `width · factor / full_width` of the unit square across from
/// `skip / full_width` in from its left edge, and `height · factor /
/// full_height` down from `skip / full_height` below its top edge, the
/// image's first row (8.9.5.2). Each ratio is one division of two integers
/// `f64` holds exactly, so it is correctly rounded on every target. With no
/// skip the samples reach at least the square's far edges; a skip of a
/// JPEG 2000 image whose origin is inside a reduced cell can leave them a
/// part of a sample short of it, and the draw's clamp to its samples' edge
/// paints that part from the last sample, as it paints the skip from the
/// first.
fn reduced_square(unit: &Transform, width: u32, height: u32, grid: Reduced) -> Option<Transform> {
    if grid.full_width == 0 || grid.full_height == 0 || grid.factor == 0 {
        return None;
    }
    let span =
        |samples: u32, full: u32| (f64::from(samples) * f64::from(grid.factor)) / f64::from(full);
    let (across, down) = (span(width, grid.full_width), span(height, grid.full_height));
    let lead = |skip: u32, full: u32| f64::from(skip) / f64::from(full);
    let (left, top) = (
        lead(grid.skip.0, grid.full_width),
        lead(grid.skip.1, grid.full_height),
    );
    // The samples' square, `(s, t)` with `t` upward as the unit square's is,
    // is the unit square's `(left + across · s, (1 − top) − down · (1 − t))`:
    // anchored where its first sample is, at the image's top-left corner but
    // for the skip. With none, `1 − 0 − down` is `1 − down` exactly and the
    // `left` term is not added, so a JPEG's square is the one it always was.
    let bottom = (1.0 - top) - down;
    let (mut e, mut f) = (unit.c * bottom + unit.e, unit.d * bottom + unit.f);
    if grid.skip.0 > 0 {
        e += unit.a * left;
        f += unit.b * left;
    }
    Some(Transform {
        a: unit.a * across,
        b: unit.b * across,
        c: unit.c * down,
        d: unit.d * down,
        e,
        f,
    })
}

/// An image of two colours, decoded: one byte a sample and the two colours
/// they choose between, the owned form of [`tinker_pdf_raster::TwoColour`].
///
/// What a one-component, one-bit image is once its two possible samples have
/// been converted — a fax, a JBIG2 region, a one-bit Flate or uncompressed
/// stream, an `/ImageMask` (8.9.6.2), whose paint is opaque and whose rest is
/// transparent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TwoColourSamples {
    /// One byte per sample, row-major from the top: zero takes the first of
    /// [`Self::colours`], any other value the second.
    pub samples: Vec<u8>,
    /// Red, green, blue and alpha of a sample of zero, then of any other.
    pub colours: [[u8; 4]; 2],
}

/// Where glyph outlines and image data come from, supplied by the caller so
/// this crate stays free of PDF dictionaries.
pub trait GlyphSource {
    /// The outline of one glyph, in a space where one unit is one em.
    ///
    /// Returning `None` means the glyph could not be drawn, which the renderer
    /// reports rather than substituting a shape.
    fn outline(&self, font_id: u64, code: u32) -> Option<Outline>;

    /// A named image XObject, decoded to RGB, for a draw that asks for it as
    /// `request` says: an implementor may decode it smaller, down to the
    /// drawn size and never below it, and says so in
    /// [`DecodedImage::reduced`].
    ///
    /// `intent` is the rendering intent in force at the draw (8.6.5.8): the
    /// implementor converts the image's colours in the intent its own
    /// dictionary names, where it names one, and in `intent` otherwise, so a
    /// decode it keeps for one intent is never handed to a draw in another.
    ///
    /// `Err` carries the codec's name so the warning can say what was missing;
    /// `Ok(None)` means the resource is not an image at all.
    fn image(
        &self,
        name: &[u8],
        request: &ImageRequest,
        intent: RenderingIntent,
    ) -> Result<Option<DecodedImage>, String> {
        let _ = (name, request, intent);
        Ok(None)
    }

    /// An inline image (8.9.7), from the bytes between `BI`/`ID` and `EI`,
    /// asked for as [`GlyphSource::image`] is, in the same intent.
    ///
    /// Passed as bytes because the interpreter has no object parser; the
    /// implementor already reads dictionaries and runs filter chains for
    /// every other image, and an inline one differs only in where it lives.
    fn inline_image(
        &self,
        dict: &[u8],
        data: &[u8],
        request: &ImageRequest,
        intent: RenderingIntent,
    ) -> Result<Option<DecodedImage>, String> {
        let _ = (dict, data, request, intent);
        Ok(None)
    }

    /// A named shading, and the type number when it is one this engine does
    /// not paint.
    fn shading(&self, name: &[u8]) -> Result<Option<Shading>, i64> {
        let _ = name;
        Ok(None)
    }

    /// A named pattern (8.7.3).
    ///
    /// `None` means the name resolves to nothing at all.
    fn pattern(&self, name: &[u8]) -> Option<PatternPaint> {
        let _ = name;
        None
    }

    /// Rasterizes one cell of a tiling pattern (8.7.3.2).
    ///
    /// Split from [`GlyphSource::pattern`] because a cell is a *content
    /// stream*: running it needs an interpreter and a resource dictionary,
    /// neither of which this crate has. The renderer supplies the geometry it
    /// has already decided; this only has to draw into the buffer described.
    ///
    /// `None` declines, and the renderer then reports the pattern as
    /// unpainted rather than leaving a hole nobody is told about — which is
    /// what a `GlyphSource` that cannot run content streams should produce.
    fn tile(&self, name: &[u8], request: &TileRequest) -> Option<Tile> {
        let _ = (name, request);
        None
    }
    /// The resources a form XObject brings with it, if it has its own
    /// (8.10.1).
    ///
    /// The device's half of `FontSource::form_scope`. Both are asked about the
    /// same form, by the same name, at the same moment, so the two scopes stay
    /// in step without either seam handing the other a resource dictionary.
    /// Shared and cached by the implementor, for `FontSource::form_scope`'s
    /// reason: a page invoking one form many times must not rebuild it.
    fn form_scope(&self, name: &[u8]) -> Option<Arc<Self>>
    where
        Self: Sized,
    {
        let _ = name;
        None
    }

    /// What font `font_id` selects in this scope, for the render's glyph-mask
    /// cache (`docs/design/performance.md`, P-06). The id is the name the
    /// scope's content selects the font by, and a form's scope may bind the
    /// same name to another font, so the id alone cannot key a mask.
    ///
    /// `None` keeps the glyph's masks out of the cache: each is rasterised at
    /// every request, to the same pixels.
    fn font_identity(&self, font_id: u64) -> Option<FontIdentity> {
        let _ = font_id;
        None
    }

    /// The glyph mask this source keeps beyond one render under `key`, for a
    /// source that holds its masks itself — a document under a stated budget
    /// (`docs/design/performance.md`, P-16): `Ok` with the mask kept, or `Err`
    /// with the number of the request that missed it, for
    /// [`GlyphSource::keep_mask`] to keep the mask the miss rasterises under.
    ///
    /// `None`, the default, keeps no masks here: the render keeps its own for
    /// itself (`glyph_cache`), as it does without one. The renderer asks only
    /// for a glyph whose font has an identity ([`GlyphSource::font_identity`]),
    /// and asks the scope it reads that identity in.
    fn kept_mask(&self, key: &MaskKey) -> Option<Result<Arc<Mask>, u64>> {
        let _ = key;
        None
    }

    /// Keeps `mask`, rasterised to the end for `key` by a render that is not
    /// cancelled, after [`GlyphSource::kept_mask`] missed it at request
    /// `asked`. A hit is therefore the mask a miss rasterises.
    fn keep_mask(&self, key: &MaskKey, mask: &Arc<Mask>, asked: u64) {
        let _ = (key, mask, asked);
    }
}

/// What a glyph's mask is a function of, as a source that keeps masks beyond
/// one render is asked for it ([`GlyphSource::kept_mask`]): P-06's key — the
/// font, the code, the quantised 2x2, the phase and the flattening tolerance —
/// with the font as the id its glyphs carry in the scope asked, which reads
/// the font's identity, and the render's anti-aliasing beside them, which one
/// render holds fixed and two renders need not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MaskKey {
    /// The id the glyph's font carries in the scope asked.
    pub font_id: u64,
    /// The code the glyph was shown by.
    pub code: u32,
    /// The quantised em-to-device 2x2.
    pub matrix: [i32; 4],
    /// The origin's phase on each axis, in sixteenths of a pixel.
    pub phase: [u8; 2],
    /// The bits of coverage the mask keeps: the render's text level, 0 with
    /// anti-aliasing off ([`Renderer::with_levels`]).
    pub bits: u8,
    /// The rule it is whole or empty by at 0 bits
    /// ([`Renderer::with_hard_edge`]), and [`HardEdge::Half`] at any other.
    pub edge: HardEdge,
    /// The bits of the tolerance the glyph's outline is flattened to: the
    /// flatness in force when it is shown (10.7.2).
    pub tolerance: u64,
}

/// What a glyph-mask cache keys a font by: one font wherever a scope names
/// it, or one only within the scope's turn the renderer holds it in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontIdentity {
    /// The font's object, `num << 16 | gen`: one font wherever a scope names
    /// it, so its masks serve every scope.
    Object(u64),
    /// A font written into the scope's own resource dictionary, which has no
    /// object: one font only within the scope the renderer holds it in, so
    /// its masks serve that scope's turn alone.
    Scoped,
}

/// Where a glyph's mask that missed is kept once it is rasterised.
enum MaskMiss {
    /// Nowhere: its font has no identity in the scope.
    Unkept,
    /// In the render's own cache, under its key, for the request that missed.
    Own(glyph_cache::Key, u64),
    /// By the source, for the request that missed
    /// ([`GlyphSource::keep_mask`]).
    Source(u64),
}

/// A `GlyphSource` that has nothing, for callers that only want geometry.
pub struct NoGlyphs;

impl GlyphSource for NoGlyphs {
    fn outline(&self, _font_id: u64, _code: u32) -> Option<Outline> {
        None
    }
}

/// How deep transparency groups may nest before one is declined (11.6.6).
///
/// The interpreter's form-recursion cap already bounds this, because a group
/// is a form; the number is restated here because this is where the *memory*
/// is, and because `Renderer` is public and can be driven by something other
/// than that interpreter. Each buffer is bounded by the clip in force, which
/// only ever shrinks as groups nest, so the cap is a backstop rather than the
/// primary bound.
const MAX_GROUP_DEPTH: usize = 16;

/// How many group buffers one page may open in total, at any depth.
///
/// [`MAX_GROUP_DEPTH`] bounds how deep the nesting goes and not how much work
/// it is, which are different numbers the moment the recursion *branches*.
/// The first real fuzzing campaign found the difference in forty-seven
/// seconds: an 1 851-byte page whose ExtGState `/SMask` names `/G 4 0 R`,
/// where object 4 is the page's own content stream — so every mask group runs
/// the three `gs` operators that each open another one. Depth stays inside
/// sixteen the whole time and the count of buffers is three to the sixteenth.
/// It rendered in 19.3 seconds at 120 by 80 points, which is 9 600 pixels.
///
/// [`MAX_TILE_WORK`] exists for exactly this reason one layer down, and its
/// comment says the same thing: a per-item cap is not a total. This is the
/// group lineage's version, and like that one it is a budget rather than a
/// depth — spent, never refunded, so a page cannot recover budget by
/// unwinding and going again.
///
/// Two thousand is far above any real document. A page that genuinely wants
/// more gets its excess groups declined and reported, which is ruling 2's
/// answer rather than a hang.
const MAX_GROUP_BUFFERS: u32 = 2000;

/// How deep tiling patterns may nest before one is declined (8.7.3.2).
///
/// A cell is a content stream, so it may fill with a pattern of its own —
/// including itself. The interpreter's own form-recursion cap does **not**
/// bound that: each cell is a fresh `interpret` call, so its depth counter
/// starts at zero again, and a pattern that fills with itself would recurse
/// until the stack ran out. So the mechanism is the same counter threaded
/// through, and the number is its own.
///
/// Much smaller than the form cap of sixteen, deliberately. A form nested `n`
/// deep costs `n` renders; a *pattern* nested `n` deep costs the product of
/// the lattice counts, because every cell of every level draws the whole of
/// the level below it. Sixteen levels of a four-cell lattice is four billion
/// cells inside a budget that only ever looks at one level at a time.
const MAX_PATTERN_DEPTH: u32 = 4;

/// The most lattice positions one tiling-pattern fill will composite
/// (8.7.3.2).
///
/// `/XStep` has no floor in the spec, so a one-unit step across A4 asks for
/// half a million cells and a step of `1e-6` asks for more than there are
/// pixels in the universe. Past the cap the fill paints nothing and says so
/// (ruling 2), which is the same degradation an unpaintable pattern has
/// always had — a partial lattice would be worse, because a hatch that covers
/// the top third of a shape reads as a rendering artefact rather than as a
/// gap.
///
/// 65 536 is generous against real files: a 10 pt hatch over A4 at scale 1 is
/// about 5 000 cells, and a 2 pt one about 125 000, which is over.
const MAX_TILES: u64 = 1 << 16;

/// The most pixels one cell's offscreen buffer may hold.
///
/// The buffer is allocated, so this is a memory bound rather than a time one.
/// 16.7 Mpx is a full-page cell on A4 at roughly 400 dpi; past that the cell
/// is not a cell, and the allocation is the thing that would fail rather than
/// the render.
const MAX_TILE_AREA: u64 = 1 << 24;

/// The most one tiling-pattern fill will composite across its whole lattice,
/// in square units of the default space the page is drawn from: pixels at
/// 72 dpi.
///
/// [`MAX_TILES`] alone does not bound the work, because `/XStep` may be much
/// smaller than `/BBox` — that is a legal, common way to draw an overlapping
/// weave — and then every cell costs its own area rather than its own step.
/// The product is what runs away: 65 536 cells each covering a hundredth of
/// the page is 650 pages' worth of compositing.
///
/// Counted in that space rather than in device pixels, because the pixels a
/// lattice covers grow with the square of the render's scale while the
/// pattern stays the same pattern: counted in pixels, a page painted its
/// pattern at 72 dpi and left it out at 144, so whether a document showed
/// its pattern was decided by the resolution it was asked for. A render at a
/// higher scale composites more pixels for the same decision, as every paint
/// does.
const MAX_TILE_WORK: u64 = 1 << 25;

/// What a forced-colour render does differently
/// ([`Renderer::with_forced_colours`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Forced {
    /// Whether a path fill outside a text object paints the stroke of its
    /// path at the thinnest width in place of its interior.
    fill_to_stroke: bool,
}

/// The clip and the soft mask, with their page-frame rectangles, as `q` saves
/// them (8.5.4, 11.6.5).
struct Held {
    clip: Option<Mask>,
    soft: Option<Mask>,
    clip_bounds: Option<Bounds>,
    soft_bounds: Option<Bounds>,
}

/// A transparency group being rendered: everything the parent needs back.
///
/// The parent canvas itself is not here: the group's buffer holds it while
/// the group is open (`Canvas::adopt_backdrop`, `Canvas::hold_parent`), and
/// hands it back first thing at close (`Canvas::release_parent`).
struct GroupFrame {
    /// Where the group's buffer sits on its parent, in device pixels.
    at: (i32, i32),
    /// `ca` at the `Do`, which applies to the group's result (11.6.6).
    alpha: f64,
    /// `/BM` at the `Do`, likewise.
    blend: RasterBlend,
    /// The clip in force at the `Do`, in device pixels.
    clip: Option<Mask>,
    /// The soft mask in force at the `Do`, in device pixels.
    ///
    /// 11.6.6 resets it to None inside the group, so it masks the group's
    /// *result* rather than each element -- which is the whole drop-shadow
    /// idiom: a mask applied element by element shows the seams the group
    /// exists to remove.
    soft: Option<Mask>,
    /// The page-frame rectangles of the two masks above, and of the parent's
    /// canvas. See [`Bounds`].
    clip_bounds: Option<Bounds>,
    soft_bounds: Option<Bounds>,
    frame: Bounds,
    /// `/K`: each element composites against the buffer as it started rather
    /// than against the elements before it (11.4.5). The buffer reads that
    /// state where it lies — nothing for an isolated group, its parent's
    /// colour at no alpha for a non-isolated one — so a knockout group keeps
    /// no second buffer (`Canvas::knock_out`).
    knockout: bool,
    /// Whether the group was invoked fully opaque — `ca` 1, `Normal`, no soft
    /// mask at the `Do` — which 11.7.5.2 asks of every group enclosing an
    /// object before the object counts as fully opaque.
    opaque: bool,
    /// The black generation and undercolour removal in force at the `Do` of
    /// a group compositing in RGB, which take its result into ink on an ink
    /// parent (11.7.5.3); `None` for every other group.
    removal: Option<Arc<InkTables>>,
    /// How deep the clip stack was when the group opened.
    ///
    /// A content stream may leave a `q` unbalanced; the interpreter drops the
    /// state it saved, but the device has already been told to push. Without
    /// this the stale entry is popped later by a `Q` from outside the group
    /// and installs a clip in the wrong coordinate space.
    clip_depth: usize,
    /// How many soft masks' groups were open when the group opened: one
    /// opened inside it and never closed is closed before it is, so that the
    /// canvas it closes over is its own buffer, holding its parent.
    masks_open: usize,
}

/// A soft-mask group being rendered (11.6.5.2).
struct MaskFrame {
    /// The canvas the render was drawing into.
    parent: Canvas,
    /// Where the mask's buffer sits in it.
    at: (i32, i32),
    /// Whether the result is read as luminosity or as alpha.
    kind: MaskKind,
    /// What the mask reads *outside* the group's bounding box.
    ///
    /// For `/Luminosity` that is the luminosity of `/BC` alone, because the
    /// group painted nothing out there and 11.6.5.2 composites it against a
    /// fully opaque `/BC` backdrop. It is zero only when `/BC` is black --
    /// which is the default -- and for `/Alpha`. The installed mask carries it
    /// as its `Mask::outside`, beside the box it was read from.
    outside: u8,
    /// `/TR`, pre-sampled.
    transfer: Option<[u8; 256]>,
    /// The clip and soft mask to put back afterwards, with their page-frame
    /// rectangles and the parent canvas's.
    clip: Option<Mask>,
    soft: Option<Mask>,
    clip_bounds: Option<Bounds>,
    soft_bounds: Option<Bounds>,
    frame: Bounds,
    clip_depth: usize,
    /// How many transparency groups were open when the mask's group opened:
    /// one opened inside it and never closed is closed before it is, so that
    /// the canvas it closes over is its own buffer.
    groups_open: usize,
}

/// The most pixels a run of images may be held over: a quarter of a page.
///
/// A run buffers four bytes a pixel, so this is 64 MB at the bound — against
/// the 268 MB an `Rgba8` canvas of [`MAX_PAGE_PIXELS`] would need, which is
/// why the two are related rather than chosen apart. An A4 page at 300 dpi is
/// 8.7 Mpx and well inside it.
///
/// Past the bound, images composite one at a time exactly as they did before
/// runs existed: abutting ones conflate again. That is a quality loss and not
/// a correctness one, and it is bounded memory rather than a page that will
/// not render (ruling 1).
///
/// **It is still not a `bounds_ledger.rs` row, and the reason it used to give
/// is no longer available.** That reason was that this and [`MAX_PAGE_PIXELS`]
/// are both properties of the scale the caller asked for rather than counts
/// read out of a document, and that ledger measures documents. Tier 0's memory
/// row put `MAX_PAGE_PIXELS` in that table and amended its `Bound` contract to
/// admit a caller-parameterised cap, so the premise is now false and the
/// conclusion drawn from it proves nothing.
///
/// The reason that survives is the one that file's header already makes twice
/// — for `MAX_EPUB_PAGES` at gap 31's milestone 4, and for `MAX_LAYOUT_WORK`
/// at its milestone 7: **a cap set under another cap, over a quantity that
/// other cap has already bounded, is the other cap wearing a second name.** A
/// run buffers part of one page; a page is already bounded by
/// [`MAX_PAGE_PIXELS`]; and this number is literally a quarter of it. Nothing
/// unbounded becomes bounded here, and what changes past the threshold is how
/// the same bounded memory is spent rather than how much of it there is — so
/// `every_bound_can_fire`, which asks whether a cap sits under what its own
/// inputs can allocate, would be asking a question this constant does not
/// answer.
const MAX_IMAGE_RUN_PIXELS: u64 = MAX_PAGE_PIXELS / 4;

/// A mask's identity: where it is, whose coverage it holds and what it reads
/// outside itself — two masks of no pixels hold no coverage of their own to
/// tell them apart by.
type MaskId = (i32, i32, u32, u32, usize, u8);

/// A run of consecutive image draws, accumulated before any of it is
/// composited.
struct ImageRun {
    /// Coverage and premultiplied colour so far.
    fragments: Fragments,
    /// The constant alpha every draw in the run was made with.
    alpha: f64,
    /// The blend mode every draw in the run was made with.
    blend: RasterBlend,
    /// The clip and soft mask in force when the run was opened.
    ///
    /// A run survives `q`, `Q` and a form boundary, because none of those
    /// draws anything — but any of them can put a *different* clip in force,
    /// and a run composited under the wrong one would paint through it. Each
    /// mask is identified by its rectangle, the address of its coverage and
    /// the value it reads outside itself, which is cheap and cannot collide
    /// while the mask is alive.
    masks: (Option<MaskId>, Option<MaskId>),
    /// The ink every draw in the run painted, when the canvas composites
    /// ink: a stencil's fill colour's own components (8.9.6.2), handed to
    /// the canvas as a fill's are. A draw of another ink, or none, ends the
    /// run, since the run reaches the canvas as one colour per pixel. `None`
    /// on every canvas that does not composite ink, so no light page's runs
    /// are split by it.
    ink: Option<[u8; 4]>,
    /// The device rectangle the run has put coverage in, as
    /// `(x0, y0, x1, y1)` with the far edges exclusive.
    ///
    /// Kept so that asking whether a new draw *overlaps* the run costs the
    /// band where the two meet rather than either of them whole. Abutting
    /// strips share one row; a picture laid over another shares its area, and
    /// only the second ends the run.
    covered: (i32, i32, i32, i32),
    /// The bounds of the run's images within the frame, which a canvas
    /// smaller than the frame does not cut: where a render of the whole page
    /// would ask whether a new draw overlaps the run ([`RunReach`]).
    framed: Bounds,
}

/// Whichever resource dictionary is in force: the caller's, or a form's own.
///
/// The device's half of `FontSource`'s scope. Both are asked for the same form
/// by the same name at the same moment, so they change together without either
/// crate handing the other a resource dictionary it should not know about.
enum Scope<'g, G> {
    /// The caller's, borrowed for the whole render.
    Borrowed(&'g G),
    /// A form's own, shared with whatever cached it.
    Owned(Arc<G>),
}

impl<G> core::ops::Deref for Scope<'_, G> {
    type Target = G;

    fn deref(&self) -> &G {
        match self {
            Scope::Borrowed(glyphs) => glyphs,
            Scope::Owned(glyphs) => glyphs,
        }
    }
}

/// What one renderer counted: the renderer's part of the counted-work tally
/// (`docs/design/performance.md`), handed back by [`Renderer::finish_counted`].
///
/// Plain numbers the renderer keeps whether or not anyone asked, as it keeps
/// its count of glyphs nothing was drawn for: two additions a glyph cost less
/// than asking. This crate has no edge to the one the tally lives in, so the
/// caller adds them to it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct RenderCounts {
    /// Glyphs the interpreter showed this renderer: every
    /// [`Device::show_glyph`] call, whether or not anything was drawn for it.
    pub glyphs_shown: u64,
    /// Glyph outlines this renderer built into a path and painted — filled,
    /// stroked or both, once a glyph: on the path route, and for a filled
    /// glyph's mask its cache did not hold. A glyph drawn from a mask the
    /// cache held builds none, and a glyph with no outline, an empty one and
    /// one a clipping-only mode adds to the clip are not counted either.
    pub outlines_filled: u64,
    /// A shading's function evaluated for a colour: once a table entry where
    /// a sweep reads it through a [`ColorTable`](shading::ColorTable), and
    /// once a colour asked of it otherwise — each pixel of a sweep no table
    /// stands in for, and each input a table does not vouch for.
    pub shading_functions_evaluated: u64,
    /// Glyph masks this renderer rasterised (`docs/design/performance.md`,
    /// P-06): each filled glyph drawn from a mask its cache did not hold, or
    /// whose font the glyph source gave no identity for.
    pub glyph_masks_rasterised: u64,
    /// Filled glyphs drawn from a mask this renderer's cache held.
    pub glyph_mask_hits: u64,
    /// Pixels of the buffers a transparency group or a soft-mask group
    /// allocated for its own pixels while it was open
    /// (`docs/design/performance.md`, P-25): its canvas, counted where it is
    /// allocated as its width times its height. A group reads its backdrop
    /// and its initial state from the parent canvas it holds, which was
    /// allocated before it opened and is not counted. A quantity rather than
    /// a count of steps.
    pub group_buffer_pixels: u64,
    /// Pixels of the regions those groups opened over, once a group: the
    /// rectangle each one's canvas is allocated over. What
    /// `group_buffer_pixels` is held to.
    pub group_bound_pixels: u64,
    /// Pixels of the soft masks this renderer installed: each mask a
    /// soft-mask group's buffer was read back as, its width times its height.
    pub soft_mask_pixels: u64,
    /// Pixels of the regions of the soft-mask groups those masks were read
    /// from, once a mask installed. What `soft_mask_pixels` is held to.
    pub soft_mask_bound_pixels: u64,
    /// Pixels of the canvas this renderer drew a page onto: the whole page,
    /// or the region of it a render asked for. A renderer drawing a tiling
    /// pattern's cell counts none, its cell drawn once for the page to
    /// repeat. A quantity, the canvas's size, rather than a count of steps:
    /// what a render of part of a page saves is the pixels it does not draw.
    pub pixels_rendered: u64,
    // The calls drawn by class, the calls hidden, and the images drawn from
    // their samples (`docs/design/page-analysis.md`, "Per-render
    // statistics"): each counted once a call, never once a pixel, so a
    // replay counts what a direct render counts. A paint skipped because
    // nothing would read it (forced colours inside a soft mask's group) is
    // not drawn.
    /// Glyphs this renderer painted: a fill or stroke mode and an outline to
    /// paint, outside hidden content.
    pub drawn_glyphs: u64,
    /// [`Device::fill_path`] calls this renderer painted, with a path to
    /// fill, a pattern's fill among them.
    pub drawn_fills: u64,
    /// [`Device::stroke_path`] calls this renderer painted, with a path to
    /// stroke.
    pub drawn_strokes: u64,
    /// Clips this renderer installed: a `W` or `W*` path, a form's or a mask
    /// group's `/BBox`, which arrive as one, and a text object's clip at `ET`.
    pub drawn_clips: u64,
    /// [`Device::draw_image`] calls this renderer drew, from decoded samples
    /// or as the placeholder of an image that would not decode.
    pub drawn_images: u64,
    /// `sh` calls whose shading this renderer painted.
    pub drawn_shadings: u64,
    /// Transparency groups this renderer accepted.
    pub drawn_groups: u64,
    /// Soft-mask groups this renderer accepted.
    pub drawn_soft_masks: u64,
    /// Glyphs, fills, strokes, images, `sh` calls and groups that hidden
    /// optional content kept from painting (8.11.3.2).
    pub hidden_calls: u64,
    /// Images this renderer drew from decoded samples.
    pub decoded_images_drawn: u64,
    /// Samples of those images as the glyph source decoded them, width by
    /// height, at every draw: fewer than `/Width` by `/Height` where it
    /// decoded one at the size the page draws it.
    pub decoded_image_samples: u64,
}

/// A canvas's pixels, counted as `RenderCounts` counts a buffer.
fn pixels_of(canvas: &Canvas) -> u64 {
    u64::from(canvas.width) * u64::from(canvas.height)
}

/// One more of a count, saturating.
fn tally(slot: &mut u64) {
    *slot = slot.saturating_add(1);
}

/// What a render of part of a page could not see of the page's image runs,
/// handed back by [`Renderer::run_reach`] (`docs/design/performance.md`,
/// P-17).
///
/// Whether an image joins the run held back so that abutting images do not
/// conflate is decided by whether it overlaps what the run holds, and a
/// canvas holds the run's coverage only over itself — so where an image and
/// the run it could join meet outside the canvas, a render of the whole page
/// can end the run where this one kept it (ruling 5's named exception). Each
/// rectangle is in the page's pixels, `(x, y, width, height)`, and lies
/// inside the page; both are `None` on a render of the whole page, whose
/// canvas holds everything.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct RunReach {
    /// The hull of every place, on a render of the whole page, where an
    /// image met the run it could join outside this canvas: the bounds the
    /// two share, the image's within its frame and the run's images'. A
    /// render whose canvas holds all of them decides every run as the page
    /// does.
    pub unseen: Option<(i32, i32, u32, u32)>,
    /// The hull of every image that could join a run whose bounds within its
    /// frame this canvas does not hold whole, whether or not it met one. A
    /// canvas that holds them all holds every place an image meets a run.
    pub cut: Option<(i32, i32, u32, u32)>,
}

/// The device's own flattening tolerance, in device pixels: what a curve is
/// flattened to where the content sets no flatness (10.7.2).
const DEFAULT_TOLERANCE: f64 = 0.2;

/// The coarsest flatness a paint may ask for: Table 56's "a number in the
/// range 0 to 100".
const COARSEST_TOLERANCE: f64 = 100.0;

/// The finest: 1/256 of a pixel, the grid the filler places an edge's
/// crossing on, below which a closer chord cannot change a coverage.
const FINEST_TOLERANCE: f64 = 1.0 / 256.0;

/// The flattening tolerance a paint in `state` takes (10.7.2): the flatness
/// in force, in device pixels, no coarser than 100 and no finer than 1/256;
/// 0 — Table 56's "the output device's default flatness tolerance" — a
/// negative value, and none set are the device's own,
/// [`DEFAULT_TOLERANCE`] (`docs/design/device-dependent-state.md`).
fn flatness_tolerance(state: &GraphicsState) -> f64 {
    match state.stored_parameters().flatness {
        Some(flatness) if flatness.is_finite() && flatness > 0.0 => {
            flatness.clamp(FINEST_TOLERANCE, COARSEST_TOLERANCE)
        }
        _ => DEFAULT_TOLERANCE,
    }
}

/// `path` flattened at `tolerance` into straight segments, one subpath a
/// polyline: a text clip's glyph kept at the flatness it was shown at, once
/// the clip's glyphs no longer share one.
fn polyline(path: &Path, tolerance: f64) -> Path {
    let mut out = Path::new();
    for subpath in tinker_pdf_raster::flatten(path, tolerance) {
        let mut points = subpath.iter();
        if let Some(first) = points.next() {
            out.move_to(first.x, first.y);
            for point in points {
                out.line_to(point.x, point.y);
            }
            out.close();
        }
    }
    out
}

/// The rasterizing device.
pub struct Renderer<'g, G: GlyphSource> {
    canvas: Canvas,
    glyphs: Scope<'g, G>,
    /// The transform from PDF user space to device pixels.
    ///
    /// **Never translated after construction.** A tile, a transparency group's
    /// buffer and a soft mask's buffer are each a canvas standing at an origin
    /// in this one frame ([`Canvas::origin`]), so every coordinate computed
    /// here — a path point, a sampled pixel centre, an image's inverse — is the
    /// same floating-point number whichever canvas it lands on (ruling 5).
    base: Matrix,
    /// The pixels the current canvas would hold on a render of the **whole**
    /// page: the page itself, or the rectangle a group's or a soft mask's
    /// buffer would have had there.
    ///
    /// Equal to the canvas's own rectangle whenever the render is of the whole
    /// page, and larger than it in a tile. Decisions about *how* something is
    /// drawn — whether an image joins a run, how much a tiling lattice may
    /// cost, which of its cells are composited, which pixels a mesh spreads
    /// colour from — are taken against this and never against the canvas, so
    /// that a tile takes the decision the page under it took (ruling 5).
    frame: Bounds,
    /// The pixels of the canvas this renderer was made with: the page, or
    /// the region of it a tile draws. A group's or a soft mask's buffer stands
    /// inside it, so it is what [`RunReach`] measures a render's reach by.
    region: Bounds,
    /// Where an image met the run it could join outside `region`, and every
    /// image that could join a run whose bounds `region` does not hold whole:
    /// [`Renderer::run_reach`].
    unseen: Option<Bounds>,
    cut: Option<Bounds>,
    clip: Option<Mask>,
    /// The clip's rectangle on a render of the whole page. See [`Bounds`].
    clip_bounds: Option<Bounds>,
    /// The soft mask's rectangle on a render of the whole page.
    soft_bounds: Option<Bounds>,
    /// The clip and the soft mask, saved together by `q` (8.5.4, 11.6.5).
    clip_stack: Vec<Held>,
    warnings: Vec<RenderWarning>,
    cancel: CancelToken,
    /// Whether a cancellation check has ever answered yes — that is, whether
    /// any work was actually skipped.
    ///
    /// Separate from the token because the two answer different questions.
    /// The token says a caller *asked*; this says the render *obeyed*. A
    /// caller that cancels after the last operator has run gets a complete
    /// page, and reporting `Cancelled` for it would make the warning mean
    /// "somebody pressed the button" rather than "this page is short".
    ///
    /// Shared rather than owned, because the predicate handed to the
    /// rasterizer outlives the borrow of `self` that every rasterizing call
    /// takes. `Relaxed` for the same reason [`CancelToken`] is: nothing is
    /// published through this flag, only observed.
    skipped: Arc<AtomicBool>,
    /// Curve flattening tolerance in device pixels: the flatness of the paint
    /// being drawn (10.7.2), set from its state at the paint.
    tolerance: f64,
    /// Whether coverage is anti-aliased, or every pixel is whole or empty.
    /// See [`Renderer::with_antialias`].
    antialias: bool,
    /// The bits of coverage graphics keep. See [`Renderer::with_levels`].
    graphics_bits: u8,
    /// The bits of coverage text keeps. See [`Renderer::with_levels`].
    text_bits: u8,
    /// The rule a pixel is whole by at level 0. See
    /// [`Renderer::with_hard_edge`].
    hard_edge: HardEdge,
    /// The thinnest stroke, in device pixels, in place of the table's. See
    /// [`Renderer::with_thinnest_line`].
    thinnest_line: Option<f64>,
    /// Glyphs being shown: what they paint is text however it is painted.
    showing: u32,
    /// The count of open text objects at each save, innermost last: what its
    /// restore lowers the count to, at most.
    text_saves: Vec<u32>,
    /// The count of open text objects at each form, group and mask's group
    /// entered, innermost last: what its end puts back.
    text_streams: Vec<u32>,
    missing_fonts: u32,
    /// Whether the interpreter refused a nested stream that named itself, for
    /// [`RenderWarning::RecursiveContent`].
    recursion_refused: bool,
    /// Whether the interpreter spent its nested-content budget, for
    /// [`RenderWarning::NestedBudgetSpent`].
    nested_budget_spent: bool,
    /// Glyphs shown and outlines painted, for [`Renderer::finish_counted`].
    counts: RenderCounts,
    /// Glyph outlines accumulated by text clipping modes 4–7, applied at `ET`.
    text_clip: Option<Path>,
    /// Whether any glyph in this text object *selected* a clipping mode,
    /// whether or not its outline could be found.
    ///
    /// Separate from `text_clip` because the two answer different questions.
    /// A text object that clips and shows nothing must clip everything away;
    /// keying that off the accumulated path alone cannot tell "clipped to
    /// nothing" from "never clipped".
    text_clip_requested: bool,
    /// The flatness every glyph in `text_clip` was shown at, while they share
    /// one: the accumulated outlines are flattened at it at `ET`, as one fill.
    text_clip_tolerance: Option<f64>,
    /// Whether `text_clip` already holds its glyphs flattened, each at its
    /// own flatness, because two of them were shown at different ones.
    text_clip_flat: bool,
    /// The answers to the interpreter's three questions, and what they
    /// depend on: marked-content visibility, how many groups and masks are
    /// open, and the group-buffer budget.
    ///
    /// Its own type rather than fields here because a page recorded for a
    /// retained display list has to be recorded with *these* answers, and
    /// [`DisplayRecorder`] asks the same type rather than a copy of it. See
    /// [`display`].
    admission: Admission,
    /// Transparency groups open, innermost last (11.6.6).
    ///
    /// While one is open `canvas` is the group's own buffer, standing at its
    /// corner of the page ([`Canvas::origin`]) — which is what lets the buffer
    /// be bounding-box sized rather than page sized without a second
    /// coordinate system running through every paint. `base` is untouched.
    groups: Vec<GroupFrame>,
    /// Resource scopes pushed by the caller rather than by a form, innermost
    /// last.
    ///
    /// An annotation's appearance stream carries its own `/Resources` and is
    /// reached by *reference*, not by a name in anybody's dictionary — so the
    /// interpreter cannot announce it the way it announces a form, and the
    /// caller says so instead. Until this existed the appearance's resources
    /// reached the interpreter and not the device, so a name that resolved for
    /// text did not resolve for an image.
    pushed_scopes: Vec<(Scope<'g, G>, u64)>,
    /// The resource scope each open form displaced, innermost last, with the
    /// serial it held.
    ///
    /// `None` for a form that brought no `/Resources` of its own, which is the
    /// common case and costs one `Option` rather than a clone of the page's.
    form_scopes: Vec<Option<(Scope<'g, G>, u64)>>,
    /// The serial of the scope `glyphs` is: 0 for the one the renderer was
    /// made with, and the next number at each scope a form brings or a caller
    /// pushes. A font written into a scope's dictionary keys its glyph masks
    /// by it, so two scopes that name two fonts alike never share one.
    scope: u64,
    /// The serials handed out so far.
    scopes: u64,
    /// This render's glyph masks (`glyph_cache`).
    glyph_cache: glyph_cache::GlyphCache,
    /// Consecutive image draws held back so that abutting ones do not
    /// conflate (`tinker_pdf_raster::fragments`).
    ///
    /// A run of images composited one at a time lets the page through
    /// wherever two of them share an edge, which is what every scanned
    /// document assembled from strips is made of. Held here rather than in
    /// the rasterizer because *when a run ends* is a question about the
    /// content stream, and only this layer sees that.
    run: Option<ImageRun>,
    /// The soft mask in force, in device pixels (11.6.5).
    ///
    /// On the device rather than in the graphics state, because it is pixels
    /// and the graphics state is PDF-level (ruling 8). It follows the clip
    /// stack's precedent exactly: `save_state` pushes it and `restore_state`
    /// pops it, so an `/SMask` set inside `q ... Q` does not survive the `Q`.
    soft: Option<Mask>,
    /// Soft-mask groups being rendered, innermost last.
    mask_frames: Vec<MaskFrame>,
    /// How many tiling-pattern cells this renderer is drawing inside
    /// (8.7.3.2), so a pattern that fills with itself terminates.
    ///
    /// On the renderer rather than on the resource seam because a cell is
    /// drawn by a `Renderer` of its own, and passing the depth down through
    /// the request is what makes the two ends agree without either of them
    /// keeping mutable state a second fill could see.
    pattern_depth: u32,
    /// Mask pixels rasterized so far, summed over every paint and every clip.
    ///
    /// The instrument behind `a_small_fill_on_a_large_page_stays_small`. A
    /// paint that goes back to asking for the whole canvas is a performance
    /// defect that no output comparison can see — the pixels are identical,
    /// which is the entire point of this work — so the only way it fails
    /// rather than merely costs is if something counts.
    #[cfg(test)]
    mask_pixels: u64,
    /// Pixels the run-membership question read (`docs/design/image-edges.md`,
    /// RC-21): for each question `run_would_overlap` asked, the band it
    /// filled the draw's square over and read the run's fragments at. The
    /// instrument behind `a_run_reads_only_the_band_where_a_draw_meets_it`.
    #[cfg(test)]
    run_pixels: u64,
    /// The test-only shadows of RC-21's three mechanisms, answering each
    /// run-membership question in the page's frame beside the canvas's
    /// answer, when a test switches them on (`run_shadows`). They decide
    /// nothing the render uses.
    #[cfg(test)]
    shadows: Option<Box<run_shadows::Shadows>>,
    /// The memory every fill of this renderer works in — each path, clip and
    /// glyph, each image's unit square and each mesh's coverage, on the page,
    /// in a group's or a soft mask's buffer and in a tile alike — grown for
    /// the largest and reused by the rest. A tiling cell is drawn by a
    /// renderer of its own, with its own.
    scratch: FillScratch,
    /// 11.7.5.2's rendering parameters at each device pixel of the page
    /// canvas — an index into `sets`, 0 the device's own — or `None` until a
    /// paint sets one. See [`DeviceParameters`].
    plane: Option<Vec<u8>>,
    /// The page canvas's device rectangle, which the plane covers whatever
    /// buffer a paint lands in.
    page_rect: (i32, i32, u32, u32),
    /// The parameter sets the plane names, entry `i + 1` at `sets[i]`.
    sets: Vec<ParameterSet>,
    /// Each transfer function met, sampled once, by the functions' identity.
    transfers: Vec<([usize; 4], Arc<[[u8; 256]; 4]>)>,
    /// Each black generation and undercolour removal pair met, sampled once.
    removals: Vec<((usize, usize), Arc<InkTables>)>,
    /// Whether a page met more parameter sets than the plane names.
    parameters_spent: bool,
    /// Whether every paint was fully opaque (11.7.5.2), for a tiling cell's
    /// [`Tile::opaque`].
    every_paint_opaque: bool,
    /// Forced colours ([`Renderer::with_forced_colours`]); `None` for an
    /// ordinary render.
    forced: Option<Forced>,
    /// Text objects open, each `BT` without its `ET` yet (9.4.1). A fill
    /// inside one is a Type 3 glyph's procedure painting (9.6.5), which
    /// fill-to-stroke leaves a glyph. No text object outlives the stream it
    /// was opened in: one a form, a transparency group or a mask's group
    /// leaves open ends with its stream, and a restore ends every one opened
    /// since its save, as `tinker_pdf_content::cull` counts them.
    text_depth: u32,
}

/// One set of the rendering parameters 11.7.5.2 keeps per point: the
/// transfer functions, red or cyan, green or magenta, blue or yellow, grey or
/// black (10.5), and whether the topmost object was DeviceGray, which on a
/// CMYK output takes the grey function alone.
#[derive(Clone, Debug)]
struct ParameterSet {
    transfer: Arc<[[u8; 256]; 4]>,
    gray: bool,
}

/// What a paint takes from the device-dependent state: the plane entry it
/// writes (0 where it is not fully opaque, or keeps the device's own) and the
/// ink tables its light becomes ink by on an ink buffer.
#[derive(Clone, Debug, Default)]
struct DeviceSet {
    entry: u8,
    removal: Option<Arc<InkTables>>,
}

/// **11.7.5.2's rendering parameters, per point, for a finished page.**
///
/// "The halftone and transfer function to be used at any given point on the
/// page shall be those in effect at the time of painting the last (topmost)
/// elementary graphics object enclosing that point, but only if the object is
/// fully opaque"; elsewhere, the page's defaults. Each paint writes its set
/// where its shape is not zero, so this holds the topmost object's at every
/// device pixel of the page canvas (`docs/design/device-dependent-state.md`).
/// A render whose paints set no transfer function keeps none.
#[derive(Clone, Debug)]
pub struct DeviceParameters {
    x0: i32,
    y0: i32,
    width: u32,
    height: u32,
    plane: Vec<u8>,
    sets: Vec<ParameterSet>,
}

impl DeviceParameters {
    /// Maps every pixel of `canvas` — the page's output, standing where the
    /// page canvas stood — through its topmost object's transfer functions,
    /// on the output's own components (10.5): red, green and blue through the
    /// first three on a light page, in either order; grey through the fourth;
    /// cyan, magenta, yellow and black through all four, each complemented on
    /// the way in and out ("converted to additive form"), and black alone
    /// through the fourth where the object was DeviceGray. Alpha is untouched.
    pub fn apply(&self, canvas: &mut Canvas) {
        let n = canvas.format.components();
        let (ox, oy) = canvas.origin();
        let stride = canvas.stride;
        for row in 0..canvas.height {
            let y = i64::from(oy) + i64::from(row) - i64::from(self.y0);
            if y < 0 || y >= i64::from(self.height) {
                continue;
            }
            for col in 0..canvas.width {
                let x = i64::from(ox) + i64::from(col) - i64::from(self.x0);
                if x < 0 || x >= i64::from(self.width) {
                    continue;
                }
                let at = y as usize * self.width as usize + x as usize;
                let entry = usize::from(self.plane.get(at).copied().unwrap_or(0));
                let Some(set) = entry.checked_sub(1).and_then(|i| self.sets.get(i)) else {
                    continue;
                };
                let base = row as usize * stride + col as usize * n;
                let Some(pixel) = canvas.data.get_mut(base..base + n) else {
                    continue;
                };
                let tables = &set.transfer;
                let additive = |slot: &mut u8, table: &[u8; 256]| {
                    *slot = table[usize::from(*slot)];
                };
                let subtractive = |slot: &mut u8, table: &[u8; 256]| {
                    *slot = 255 - table[usize::from(255 - *slot)];
                };
                match canvas.format {
                    PixelFormat::Gray8 | PixelFormat::GrayA8 => {
                        if let Some(slot) = pixel.first_mut() {
                            additive(slot, &tables[3]);
                        }
                    }
                    PixelFormat::Rgb8 | PixelFormat::Rgba8 => {
                        for (slot, table) in pixel.iter_mut().zip(tables.iter()).take(3) {
                            additive(slot, table);
                        }
                    }
                    PixelFormat::Bgr8 | PixelFormat::Bgra8 => {
                        for (slot, table) in pixel.iter_mut().zip([2, 1, 0]) {
                            additive(slot, &tables[table]);
                        }
                    }
                    PixelFormat::CmykA8 => {
                        if set.gray {
                            if let Some(slot) = pixel.get_mut(3) {
                                subtractive(slot, &tables[3]);
                            }
                        } else {
                            for (slot, table) in pixel.iter_mut().zip(tables.iter()).take(4) {
                                subtractive(slot, table);
                            }
                        }
                    }
                    // A page is never handed back in Lab, nor in plates.
                    PixelFormat::LabA8 | PixelFormat::Plates { .. } => {}
                }
            }
        }
    }
}

/// The device space family `space`'s colours are in (`ColorFamily`): a
/// device space's own, an `/Indexed` space's base's, a `/Separation` or
/// `/DeviceN` space's alternate's and an uncoloured pattern space's base's;
/// `Other` for every CIE-based and ICC space.
#[must_use]
pub fn color_family(space: &ColorSpace) -> ColorFamily {
    match space {
        ColorSpace::DeviceGray => ColorFamily::Gray,
        ColorSpace::DeviceRgb => ColorFamily::Rgb,
        ColorSpace::DeviceCmyk => ColorFamily::Cmyk,
        ColorSpace::Indexed { base, .. } => color_family(base),
        ColorSpace::Separation { alternate, .. } => color_family(alternate),
        ColorSpace::Pattern { base: Some(base) } => color_family(base),
        _ => ColorFamily::Other,
    }
}

/// The family a shading's colours are in.
fn shading_family(shading: &Shading) -> ColorFamily {
    match shading {
        Shading::FunctionBased { space, .. }
        | Shading::Axial { space, .. }
        | Shading::Radial { space, .. } => color_family(space),
        Shading::Mesh(mesh) => color_family(&mesh.space),
    }
}

/// A function of one input over `[0 1]`, sampled to 256 entries and read in
/// `[low 1]`: a transfer function's (10.5) or a black generation's, from 0,
/// and an undercolour removal's, from −1 (Table 57). Evaluated once, never
/// per pixel (ruling 4); a value that is not finite reads as the input.
fn sample_function(function: &Function, low: f64) -> [f64; 256] {
    let mut table = [0.0; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        let input = f64::from(i as u8) / 255.0;
        let value = function.eval(&[input]).first().copied().unwrap_or(input);
        *slot = if value.is_finite() { value } else { input }.clamp(low, 1.0);
    }
    table
}

/// [`sample_function`] in bytes, for a transfer function.
fn transfer_table(function: &Function) -> [u8; 256] {
    sample_function(function, 0.0).map(|v| (v * 255.0).round() as u8)
}

impl<'g, G: GlyphSource> Renderer<'g, G> {
    /// A renderer drawing into a new canvas.
    ///
    /// `base` maps PDF user space to pixels, which is where the y-flip lives:
    /// PDF counts upward from the bottom-left, a raster counts downward from
    /// the top-left.
    pub fn new(canvas: Canvas, base: Matrix, glyphs: &'g G) -> Renderer<'g, G> {
        let frame = Bounds::of_rect(canvas.device_rect());
        let canvas_rect = canvas.device_rect();
        Renderer {
            canvas,
            glyphs: Scope::Borrowed(glyphs),
            base,
            frame,
            region: frame,
            unseen: None,
            cut: None,
            clip: None,
            clip_bounds: None,
            soft_bounds: None,
            clip_stack: Vec::new(),
            warnings: Vec::new(),
            cancel: CancelToken::new(),
            skipped: Arc::new(AtomicBool::new(false)),
            tolerance: DEFAULT_TOLERANCE,
            antialias: true,
            graphics_bits: 8,
            text_bits: 8,
            hard_edge: HardEdge::Half,
            thinnest_line: None,
            showing: 0,
            text_saves: Vec::new(),
            text_streams: Vec::new(),
            missing_fonts: 0,
            recursion_refused: false,
            nested_budget_spent: false,
            counts: RenderCounts::default(),
            text_clip: None,
            text_clip_requested: false,
            text_clip_tolerance: None,
            text_clip_flat: false,
            run: None,
            pushed_scopes: Vec::new(),
            form_scopes: Vec::new(),
            scope: 0,
            scopes: 0,
            glyph_cache: glyph_cache::GlyphCache::new(),
            admission: Admission::new(),
            groups: Vec::new(),
            soft: None,
            mask_frames: Vec::new(),
            pattern_depth: 0,
            #[cfg(test)]
            mask_pixels: 0,
            #[cfg(test)]
            run_pixels: 0,
            #[cfg(test)]
            shadows: None,
            scratch: FillScratch::new(),
            plane: None,
            page_rect: canvas_rect,
            sets: Vec::new(),
            transfers: Vec::new(),
            removals: Vec::new(),
            parameters_spent: false,
            every_paint_opaque: true,
            forced: None,
            text_depth: 0,
        }
    }

    /// Says the canvas is a region of a page `width` by `height` pixels.
    ///
    /// The canvas carries where in the page it stands ([`Canvas::origin`]);
    /// this carries how large the page is, which is what every decision about
    /// *how* to draw rather than *what* to draw is measured against. Without
    /// it a tile would measure them against itself and could take the other
    /// branch from the page it is part of. A renderer whose canvas is the
    /// whole page needs no call: the canvas is the page.
    #[must_use]
    pub fn with_page_size(mut self, width: u32, height: u32) -> Self {
        self.frame = Bounds::of_rect((0, 0, width, height));
        self
    }

    /// How many pixels the current canvas would hold on a render of the whole
    /// page. See the `frame` field.
    fn frame_area(&self) -> u64 {
        self.frame.area()
    }

    /// How many device pixels one square unit of the space `base` maps from
    /// covers: the render's scale, squared, whatever it turns or mirrors.
    /// One where `base` collapses the plane, which no page draws through.
    fn pixels_per_square_unit(&self) -> f64 {
        let det = (self.base.a * self.base.d - self.base.b * self.base.c).abs();
        if det.is_finite() && det > 0.0 {
            det
        } else {
            1.0
        }
    }

    /// The rectangle a shape's mask would have on a render of the whole page:
    /// [`paint_region`] over the page-frame rectangles rather than over the
    /// masks and the canvas.
    fn page_region(&self, shape: Shape<'_>) -> Bounds {
        region_within(
            self.bounds_of(shape),
            &[self.clip_bounds, self.soft_bounds],
            self.frame,
        )
    }

    /// A shape's bounding box in device pixels, control points included:
    /// a path's own, or the bounds of the stroke's outline in the scratch,
    /// which are the ones that outline's path would have.
    fn bounds_of(&self, shape: Shape<'_>) -> Option<(f64, f64, f64, f64)> {
        match shape {
            Shape::Fill(path, _) => path.bounds(),
            Shape::Stroke => self.scratch.outline_bounds(),
        }
    }

    /// What the clip and the soft mask leave of the frame, on a render of the
    /// whole page: the rectangle `sh` paints.
    fn page_limit(&self) -> Bounds {
        [self.clip_bounds, self.soft_bounds]
            .into_iter()
            .flatten()
            .fold(self.frame, Bounds::meet)
    }

    /// The clip and the soft mask as `q` saves them.
    fn held(&self) -> Held {
        Held {
            clip: self.clip.clone(),
            soft: self.soft.clone(),
            clip_bounds: self.clip_bounds,
            soft_bounds: self.soft_bounds,
        }
    }

    /// Puts back what [`Renderer::held`] saved.
    fn restore_held(&mut self, held: Held) {
        self.clip = held.clip;
        self.soft = held.soft;
        self.clip_bounds = held.clip_bounds;
        self.soft_bounds = held.soft_bounds;
    }

    /// The device pixels the current canvas holds.
    fn device(&self) -> (i32, i32, u32, u32) {
        self.canvas.device_rect()
    }

    /// Records that this renderer is drawing a tiling pattern's cell, `depth`
    /// levels inside the page (8.7.3.2).
    ///
    /// Set by whoever rasterizes a cell, from [`TileRequest::depth`]. Without
    /// it the cell's own pattern fills start counting from zero again and a
    /// pattern that fills with itself never terminates.
    #[must_use]
    pub fn with_pattern_depth(mut self, depth: u32) -> Self {
        self.pattern_depth = depth;
        self
    }

    /// Whether an optional-content layer is currently hiding what is drawn.
    ///
    /// 8.11.3.2: hidden content is not *skipped* — every operator inside the
    /// scope still runs, the text pen still advances, `q` and `Q` still
    /// balance, and text extraction still sees every glyph. Only painting
    /// stops, and it stops here, at the paint, so that a device which does
    /// not draw and one which does cannot disagree about what a page
    /// contains.
    fn hidden(&self) -> bool {
        self.admission.hidden()
    }

    /// Turns anti-aliasing off, or back on: `false` makes every pixel of
    /// every shape either wholly covered or not covered at all.
    ///
    /// **One threshold, applied wherever this crate decides coverage**, so
    /// that no path through the rasterizer is left soft on a page that asked
    /// for hard edges:
    ///
    /// - every fill, stroke, clip, text clip, pattern shape and glyph drawn as
    ///   a path goes through `Renderer::coverage`, which hardens the mask
    ///   `fill` returns before the clip multiplies into it, and a glyph's own
    ///   mask is hardened as it is made;
    /// - an image's edge is hardened inside the rasterizer
    ///   ([`ImageDraw::edge_bits`] at 0);
    /// - a mesh shading's silhouette is hardened inside the rasterizer
    ///   ([`MeshDraw::edge_bits`] at 0), after its colours are spread;
    /// - a tiling pattern's cell is drawn by a renderer of its own, which is
    ///   handed the same answer through [`TileRequest::antialias`];
    /// - `sh` and a shading pattern paint through the clip or the shape, which
    ///   were hardened when they were made.
    ///
    /// The threshold is [`Mask::harden`]'s — half a pixel's coverage — and the
    /// doc there says why. What is **not** coverage is left alone: a soft
    /// mask's levels, a constant alpha, an image's own alpha channel and the
    /// colours of its samples are the document's, not the rasterizer's
    /// approximation of an edge.
    ///
    /// A stroke is at least a whole pixel wide when this is off under
    /// [`HardEdge::Half`], where it is eight tenths of one otherwise. 8.4.3.2
    /// asks for the thinnest line the device can render and a hard-edged
    /// device can render a pixel; a line narrower than that, straddling two
    /// pixels, would leave each less than half covered and vanish. The other
    /// rules and levels take the floors `Renderer::thinnest` tables.
    ///
    /// Off, every coverage is at level 0 whatever [`Renderer::with_levels`]
    /// says, under the rule [`Renderer::with_hard_edge`] names, whose
    /// default, [`HardEdge::Half`], is the threshold above.
    #[must_use]
    pub fn with_antialias(mut self, antialias: bool) -> Self {
        self.antialias = antialias;
        self
    }

    /// The bits of coverage graphics and text keep, 0 to 8
    /// (`docs/design/anti-aliasing.md`): 8, the default, anti-aliased; 1 to 7
    /// each pixel's coverage taken to the nearest of `2^bits` values
    /// ([`Mask::quantise`]); 0 whole or empty by the rule
    /// [`Renderer::with_hard_edge`] names. A number above 8 reads as 8.
    ///
    /// Text is every glyph shown, however it is painted — from its mask, as a
    /// path, stroked or through a pattern — and everything a text object
    /// paints or clips: a Type 3 glyph's procedure and the clip modes 4 to 7
    /// add at `ET`. Graphics is everything else: paths, clips, image edges and
    /// mesh silhouettes. A tiling cell is drawn by a renderer of its own,
    /// handed both levels and the rule through [`TileRequest`], so text in a
    /// cell is text. Each level is applied where the coverage is made, before
    /// the clip and the soft mask multiply into it. Read only with
    /// anti-aliasing on: [`Renderer::with_antialias`]`(false)` reads both as
    /// 0.
    #[must_use]
    pub fn with_levels(mut self, graphics: u8, text: u8) -> Self {
        self.graphics_bits = graphics.min(8);
        self.text_bits = text.min(8);
        self
    }

    /// The rule a pixel is whole by at level 0: [`HardEdge::Half`], the
    /// default, the coverage hardened at half a pixel; [`HardEdge::Centre`]
    /// the pixels whose centres a shape holds; [`HardEdge::AnyPart`] every
    /// pixel it meets. Under the last the renderer's scratch keeps each
    /// stroke's pieces, which that rule reads where an edge passes
    /// ([`FillScratch::keep_outline_points`]).
    #[must_use]
    pub fn with_hard_edge(mut self, edge: HardEdge) -> Self {
        self.hard_edge = edge;
        self.scratch.keep_outline_points(edge == HardEdge::AnyPart);
        self
    }

    /// Whether what is painted now is text: a glyph being shown, or anything
    /// a text object paints or clips.
    fn painting_text(&self) -> bool {
        self.showing > 0 || self.text_depth > 0
    }

    /// The level coverage made now keeps: 0 with anti-aliasing off, the
    /// text level while text is painted, the graphics level otherwise.
    fn level(&self) -> u8 {
        if !self.antialias {
            0
        } else if self.painting_text() {
            self.text_bits
        } else {
            self.graphics_bits
        }
    }

    /// The rule a fill asks the filler for: the hard edge's, at level 0
    /// under a rule that reads the shape; `None` where the coverage is read.
    fn hard_rule(&self, level: u8) -> Option<HardEdge> {
        (level == 0 && self.hard_edge != HardEdge::Half).then_some(self.hard_edge)
    }

    /// Notes a form, group or mask's group the interpreter `entered`, so that
    /// a text object its stream leaves open ends with it.
    fn entered_stream(&mut self, entered: bool) {
        if entered {
            self.text_streams.push(self.text_depth);
        }
    }

    /// Notes that the innermost entered stream ended: the text objects open
    /// are the ones open when it began.
    fn left_stream(&mut self) {
        if let Some(at) = self.text_streams.pop() {
            self.text_depth = at;
        }
    }

    /// The thinnest stroke this render draws, in device pixels, in the page's
    /// frame: 8.4.3.2's "thinnest line that can be rendered" at the level
    /// and rule of the stroke's own class (`docs/design/anti-aliasing.md`),
    /// graphics for a path and text for a stroked glyph —
    ///
    /// | Level | Rule | A path | A stroked glyph |
    /// | --- | --- | --- | --- |
    /// | 8 | — | 0.8 | 0.6 |
    /// | 1 to 7 | — | 1 | 1 |
    /// | 0 | `Half` | 1 | 1 |
    /// | 0 | `Centre` | 65/64 | 65/64 |
    /// | 0 | `AnyPart` | 1/256 | 1/256 |
    ///
    /// — each the narrowest width at which a hairline leaves no column it
    /// crosses (no row, past one in one) empty, or the width
    /// [`Renderer::with_thinnest_line`] asks for. So the default, level 8,
    /// and anti-aliasing off, level 0 under `Half`, keep the floors they had.
    fn thinnest(&self, glyph: bool) -> f64 {
        if let Some(width) = self.thinnest_line {
            return width;
        }
        match (self.level(), self.hard_edge) {
            (8.., _) if glyph => 0.6,
            (8.., _) => 0.8,
            (1..=7, _) | (0, HardEdge::Half) => 1.0,
            (0, HardEdge::Centre) => 65.0 / 64.0,
            (0, HardEdge::AnyPart) => 1.0 / 256.0,
        }
    }

    /// The thinnest stroke the render draws, in device pixels, for every
    /// stroke — a path's and a stroked glyph's at every level and rule — in
    /// place of `Renderer::thinnest`'s table; `None`, the default, takes
    /// the table. A width that is not a finite number above zero is read as
    /// `None`. A floor in device pixels of the page's frame, so a tile draws
    /// the page's strokes (ruling 5).
    #[must_use]
    pub fn with_thinnest_line(mut self, width: Option<f64>) -> Self {
        self.thinnest_line = width.filter(|width| width.is_finite() && *width > 0.0);
        self
    }

    /// Installs a cancellation token.
    #[must_use]
    pub fn with_cancel(mut self, cancel: CancelToken) -> Self {
        self.cancel = cancel;
        self
    }

    /// Draws the page in **forced colours** (`docs/design/bitmap-toolbox.md`):
    /// as its coverage, black on white, for the caller to tint into two
    /// colours afterwards.
    ///
    /// Every fill, stroke and glyph paints opaque black by its own coverage —
    /// whatever its colour, its DeviceCMYK ink, its pattern or a stencil's
    /// fill colour — and an image or a shading paints its own colours, which
    /// the white `Gray8` canvas the caller hands over holds as their
    /// lightness, so its darkness is its coverage too. No constant alpha,
    /// blend mode, soft mask, knockout or group colour space applies: every
    /// paint is opaque and `Normal`, so the canvas ends as what the page's
    /// marks make of white.
    ///
    /// The interpreter's three questions are answered as an ordinary render
    /// answers them — a group is opened and composited at full strength, a
    /// soft mask's group is accepted, drawn into nothing and never installed
    /// — so a display list recorded for an ordinary render replays into this
    /// one without a disagreement.
    ///
    /// With `fill_to_stroke`, each path fill paints, in place of its
    /// interior, the stroke of the same path at 8.4.3.2's thinnest line,
    /// solid, under the same clip, with the caps, joins and miter limit in
    /// force: exactly what `0 w [] 0 d` and `S` would paint there. A fill
    /// inside a text object is a Type 3 glyph's (9.6.5) and stays a fill, as
    /// every glyph does.
    #[must_use]
    pub fn with_forced_colours(mut self, fill_to_stroke: bool) -> Self {
        self.forced = Some(Forced { fill_to_stroke });
        self
    }

    /// The state a paint is made with: `state` itself, or under forced
    /// colours a copy whose paint is opaque black and `Normal`, with no ink
    /// and no pattern ([`Renderer::with_forced_colours`]). Everything else —
    /// the transform, the pen, the text state — is the page's.
    fn painting<'s>(&self, state: &'s GraphicsState) -> std::borrow::Cow<'s, GraphicsState> {
        if self.forced.is_none() {
            return std::borrow::Cow::Borrowed(state);
        }
        let mut forced = state.clone();
        forced.fill_color = tinker_pdf_content::Rgb::BLACK;
        forced.stroke_color = tinker_pdf_content::Rgb::BLACK;
        forced.fill_wide = tinker_pdf_content::Rgb16::BLACK;
        forced.stroke_wide = tinker_pdf_content::Rgb16::BLACK;
        forced.fill_ink = None;
        forced.stroke_ink = None;
        forced.fill_alpha = 1.0;
        forced.stroke_alpha = 1.0;
        forced.blend = tinker_pdf_content::BlendMode::Normal;
        forced.fill_pattern = None;
        forced.stroke_pattern = None;
        std::borrow::Cow::Owned(forced)
    }

    /// Whether what is drawn now would be thrown away unread: the inside of a
    /// soft mask's group under forced colours, which install no soft mask, so
    /// its paints are skipped rather than drawn into a buffer nothing reads.
    fn discarding(&self) -> bool {
        self.forced.is_some() && !self.mask_frames.is_empty()
    }

    /// Whether a paint is skipped rather than drawn: the render is stopping,
    /// hidden content holds it — which counts it among the calls optional
    /// content hid, as it is wherever it lies — or nothing would read it
    /// ([`Renderer::discarding`]).
    fn skips_paint(&mut self) -> bool {
        if self.stopping() {
            return true;
        }
        if self.hidden() {
            tally(&mut self.counts.hidden_calls);
            return true;
        }
        self.discarding()
    }

    /// Whether to stop now — and, if so, a record that this check is one that
    /// fired.
    ///
    /// Every check in this crate asks through here rather than the token
    /// directly, so that `Cancelled` can mean "work was skipped" instead of
    /// "the token was set". The two come apart at the end of a render: a
    /// caller that cancels a page which has already finished drawing gets a
    /// complete page, no check ever answers yes, and nothing is reported —
    /// which is the truthful answer, and the one a caller can act on.
    ///
    /// A check that answers yes always skips something. There is no site
    /// where the answer is consulted and the work runs anyway: each one
    /// returns, and the row loops return with what they have.
    fn stopping(&self) -> bool {
        if self.cancel.is_cancelled() {
            self.skipped.store(true, Ordering::Relaxed);
            return true;
        }
        false
    }

    /// How the clip and the soft mask in force can be told apart cheaply.
    fn mask_ids(&self) -> (Option<MaskId>, Option<MaskId>) {
        let id = |mask: &Mask| {
            (
                mask.x0,
                mask.y0,
                mask.width,
                mask.height,
                mask.data.as_ptr() as usize,
                mask.outside,
            )
        };
        (self.clip.as_ref().map(id), self.soft.as_ref().map(id))
    }

    /// Resolves names against `resources` until [`Renderer::pop_resources`].
    ///
    /// For a content stream the caller reached by reference — an annotation's
    /// appearance — where there is no name for the interpreter to announce.
    pub fn push_resources(&mut self, resources: Arc<G>) {
        let outer = mem::replace(&mut self.glyphs, Scope::Owned(resources));
        let serial = self.next_scope();
        self.pushed_scopes.push((outer, serial));
    }

    /// Restores the scope [`Renderer::push_resources`] displaced.
    pub fn pop_resources(&mut self) {
        if let Some((outer, serial)) = self.pushed_scopes.pop() {
            self.glyphs = outer;
            self.scope = serial;
        }
    }

    /// Gives the scope just made current the next serial, and hands back the
    /// one it displaced.
    fn next_scope(&mut self) -> u64 {
        self.scopes = self.scopes.saturating_add(1);
        mem::replace(&mut self.scope, self.scopes)
    }

    /// What a glyph's mask is asked for by in a source that keeps masks
    /// beyond this render ([`GlyphSource::kept_mask`]).
    fn mask_key(&self, glyph: &Glyph, placement: &glyph::Placement) -> MaskKey {
        MaskKey {
            font_id: glyph.font_id,
            code: glyph.code,
            matrix: placement.matrix,
            phase: placement.phase,
            bits: self.level(),
            edge: self.mask_rule(),
            tolerance: self.tolerance.to_bits(),
        }
    }

    /// The rule a glyph's mask is made under, as its keys hold it: the hard
    /// edge's at level 0, and [`HardEdge::Half`] at any other level, where
    /// no rule is read, so that one mask serves them all.
    fn mask_rule(&self) -> HardEdge {
        if self.level() == 0 {
            self.hard_edge
        } else {
            HardEdge::Half
        }
    }

    /// The key a glyph's mask is kept under, or `None` when its font has no
    /// identity in the current scope and its masks are not kept.
    fn glyph_key(&self, glyph: &Glyph, placement: &glyph::Placement) -> Option<glyph_cache::Key> {
        let font = match self.glyphs.font_identity(glyph.font_id)? {
            FontIdentity::Object(object) => glyph_cache::KeyFont::Object(object),
            FontIdentity::Scoped => glyph_cache::KeyFont::Scoped {
                font_id: glyph.font_id,
                scope: self.scope,
            },
        };
        Some(glyph_cache::Key {
            font,
            code: glyph.code,
            matrix: placement.matrix,
            phase: placement.phase,
            tolerance: self.tolerance.to_bits(),
            bits: self.level(),
            edge: self.mask_rule(),
        })
    }

    /// Composites the run of images held back so far, if any.
    ///
    /// Called at the top of every device operation that is not an image draw,
    /// which is what makes the run's graphics state constant: the clip and the
    /// soft mask can only change through one of those, so the state in force
    /// now is the state every draw in the run was made with.
    fn flush_run(&mut self) {
        // What ends the real run for anything but an overlap ends the page's,
        // so it ends every shadow's too (`run_shadows`).
        #[cfg(test)]
        if let Some(shadows) = self.shadows.as_mut() {
            shadows.end_runs();
        }
        self.composite_run();
    }

    /// Composites the run held back so far, if any: [`Renderer::flush_run`]
    /// less the shadows, for the one call site that has told them already.
    fn composite_run(&mut self) {
        let Some(run) = self.run.take() else {
            return;
        };
        let stop = self.stop_predicate();
        let combined = self.combined_mask();
        let clip = combined
            .as_ref()
            .or(self.clip.as_ref())
            .or(self.soft.as_ref());
        run.fragments.composite_region_inked(
            &mut self.canvas,
            run.covered,
            run.alpha,
            run.blend,
            clip,
            run.ink,
            Some(&stop),
        );
    }

    /// What this render could not see of the page's image runs so far
    /// ([`RunReach`]): nothing, for a canvas that is the whole page.
    ///
    /// Asked when the content has been drawn: every question a run is asked
    /// is asked when an image is drawn, and [`Renderer::finish`] only
    /// composites the run it holds.
    #[must_use]
    pub fn run_reach(&self) -> RunReach {
        RunReach {
            unseen: self.unseen.map(Bounds::rect),
            cut: self.cut.map(Bounds::rect),
        }
    }

    /// [`Renderer::finish`], and what the render counted: the glyphs it was
    /// shown and the outlines it painted, for a caller keeping a tally.
    #[must_use]
    pub fn finish_counted(self) -> (Canvas, Vec<RenderWarning>, RenderCounts) {
        let (canvas, warnings, counts, _) = self.finish_with_parameters();
        (canvas, warnings, counts)
    }

    /// What the render has met so far, in the order it first met each: the
    /// list [`Renderer::finish`] hands back, before the warnings it adds at
    /// the end of the render.
    ///
    /// What a caller that delivers the calls one at a time reads after each,
    /// to say which call a warning was first met at: the bands of one page
    /// merge their lists by that into the page's own
    /// (`docs/design/parallelism.md`, "Warnings equal the serial render's").
    #[must_use]
    pub fn met(&self) -> &[RenderWarning] {
        &self.warnings
    }

    /// [`Renderer::finish_counted`], and the page's rendering parameters per
    /// point (11.7.5.2), for the caller to apply once the canvas is in its
    /// output format ([`DeviceParameters::apply`]); `None` for a render whose
    /// paints set no transfer function.
    #[must_use]
    pub fn finish_with_parameters(
        mut self,
    ) -> (
        Canvas,
        Vec<RenderWarning>,
        RenderCounts,
        Option<DeviceParameters>,
    ) {
        let mut counts = self.counts;
        // The page's canvas, the region of it a tile draws; a cell's renderer
        // draws its cell once, for the page to repeat.
        if self.pattern_depth == 0 {
            counts.pixels_rendered = self.region.area();
        }
        let plane = self.plane.take();
        let sets = std::mem::take(&mut self.sets);
        let (x0, y0, width, height) = self.page_rect;
        let spent = self.parameters_spent;
        let (canvas, mut warnings) = self.finish();
        if spent && !warnings.contains(&RenderWarning::DeviceParametersSpent) {
            warnings.push(RenderWarning::DeviceParametersSpent);
        }
        let parameters = plane.map(|plane| DeviceParameters {
            x0,
            y0,
            width,
            height,
            plane,
            sets,
        });
        (canvas, warnings, counts, parameters)
    }

    /// Whether every paint so far was fully opaque in 11.7.5.2's sense — the
    /// answer a tiling cell's renderer gives its [`Tile::opaque`].
    #[must_use]
    pub fn painted_opaquely(&self) -> bool {
        self.every_paint_opaque
    }

    /// Whether a paint made now is fully opaque (11.7.5.2): its own alpha
    /// constant 1, the blend mode `Normal`, no soft mask, its source opaque
    /// (an image with no soft mask, a cell all of whose paints were), and
    /// every group around it invoked the same way.
    fn fully_opaque(&self, state: &GraphicsState, alpha: f64, source_opaque: bool) -> bool {
        source_opaque
            && alpha >= 1.0
            && state.blend == tinker_pdf_content::BlendMode::Normal
            && self.soft.is_none()
            && self.groups.iter().all(|frame| frame.opaque)
    }

    /// What a paint takes from the device-dependent state: its plane entry,
    /// and the ink tables its light becomes ink by — only for a colour of
    /// DeviceRGB's family, the one conversion 11.7.5.3 applies them to.
    fn device_set(
        &mut self,
        state: &GraphicsState,
        alpha: f64,
        family: ColorFamily,
        source_opaque: bool,
    ) -> DeviceSet {
        let opaque = self.fully_opaque(state, alpha, source_opaque);
        if !opaque {
            self.every_paint_opaque = false;
        }
        let stored = state.stored_parameters();
        let entry = if opaque {
            self.entry_for(stored.transfer.as_ref(), family)
        } else {
            0
        };
        let removal = if family == ColorFamily::Rgb {
            self.removal_for(stored)
        } else {
            None
        };
        DeviceSet { entry, removal }
    }

    /// The plane entry of a transfer, 0 for the device's own: `/Identity`,
    /// `/Default` and an unset key alike (10.5).
    fn entry_for(&mut self, transfer: Option<&Transfer>, family: ColorFamily) -> u8 {
        let functions: [&Arc<Function>; 4] = match transfer {
            None | Some(Transfer::Identity) => return 0,
            Some(Transfer::One(one)) => [one, one, one, one],
            Some(Transfer::Four([red, green, blue, gray])) => [red, green, blue, gray],
        };
        let key = functions.map(|function| Arc::as_ptr(function) as usize);
        let tables = match self.transfers.iter().find(|(known, _)| *known == key) {
            Some((_, tables)) => Arc::clone(tables),
            None => {
                let tables = Arc::new(functions.map(|function| transfer_table(function)));
                // A function written into the dictionary itself is read anew
                // at every `gs`, so the identities are bounded rather than the
                // tables, which the sets below share by value.
                if self.transfers.len() < 64 {
                    self.transfers.push((key, Arc::clone(&tables)));
                }
                tables
            }
        };
        let gray = family == ColorFamily::Gray;
        if let Some(at) = self
            .sets
            .iter()
            .position(|set| set.gray == gray && *set.transfer == *tables)
        {
            return u8::try_from(at + 1).unwrap_or(0);
        }
        if self.sets.len() >= usize::from(u8::MAX) {
            self.parameters_spent = true;
            return 0;
        }
        self.sets.push(ParameterSet {
            transfer: tables,
            gray,
        });
        u8::try_from(self.sets.len()).unwrap_or(0)
    }

    /// The ink tables black generation and undercolour removal in force make
    /// (10.4.2.4), or `None` with neither set — the device's own, maximum
    /// removal. With one set, the other is `k` itself.
    fn removal_for(&mut self, stored: &StoredParameters) -> Option<Arc<InkTables>> {
        let (black, removal) = (
            stored.black_generation.as_ref(),
            stored.undercolor_removal.as_ref(),
        );
        if black.is_none() && removal.is_none() {
            return None;
        }
        let identity = |function: Option<&Arc<Function>>| {
            function.map_or(0, |function| Arc::as_ptr(function) as usize)
        };
        let key = (identity(black), identity(removal));
        if let Some((_, tables)) = self.removals.iter().find(|(known, _)| *known == key) {
            return Some(Arc::clone(tables));
        }
        let unit: [f64; 256] = core::array::from_fn(|k| f64::from(k as u8) / 255.0);
        let black = black.map_or(unit, |function| sample_function(function, 0.0));
        let removal = removal.map_or(unit, |function| sample_function(function, -1.0));
        let tables = Arc::new(InkTables {
            black: black.map(|v| (v * 255.0).round() as u8),
            removal: removal.map(|v| (v * 255.0).round() as i16),
        });
        if self.removals.len() < 64 {
            self.removals.push((key, Arc::clone(&tables)));
        }
        Some(tables)
    }

    /// Puts `removal` in force on an ink canvas for one paint, handing back
    /// what was, or does nothing on any other canvas.
    fn hold_removal(&mut self, removal: &Option<Arc<InkTables>>) -> Option<Option<Arc<InkTables>>> {
        (removal.is_some() && self.canvas.format == PixelFormat::CmykA8)
            .then(|| self.canvas.set_ink_tables(removal.clone()))
    }

    /// Puts back what [`Renderer::hold_removal`] displaced.
    fn release_removal(&mut self, held: Option<Option<Arc<InkTables>>>) {
        if let Some(previous) = held {
            self.canvas.set_ink_tables(previous);
        }
    }

    /// Whether a paint with this entry writes the plane at all: a soft
    /// mask's group paints no page, and an entry of 0 changes nothing while
    /// there is no plane.
    fn records(&self, entry: u8) -> bool {
        self.mask_frames.is_empty() && (entry != 0 || self.plane.is_some())
    }

    /// Writes `entry` into the plane wherever `shape` is not zero.
    fn record(&mut self, shape: &Mask, entry: u8) {
        if !self.records(entry) {
            return;
        }
        let (px0, py0, width, height) = self.page_rect;
        let plane = self
            .plane
            .get_or_insert_with(|| vec![0; width as usize * height as usize]);
        let (x0, y0, x1, y1) = shape.overlap_at((px0, py0), width, height);
        for row in y0..y1 {
            for col in x0..x1 {
                let (x, y) = (
                    px0.saturating_add(col as i32),
                    py0.saturating_add(row as i32),
                );
                if shape.at(x, y) == 0 {
                    continue;
                }
                if let Some(slot) = plane.get_mut(row as usize * width as usize + col as usize) {
                    *slot = entry;
                }
            }
        }
    }

    /// Records a path's or a stroke's shape: the coverage it composites
    /// through, or, under a soft mask, the coverage again without it — the
    /// mask is opacity, not shape (11.6.4.3).
    fn record_shape(&mut self, shape: Shape<'_>, painted: &Mask, entry: u8) {
        if !self.records(entry) {
            return;
        }
        if self.soft.is_none() {
            self.record(painted, entry);
            return;
        }
        let soft = self.soft.take();
        let unmasked = self.coverage(shape, None);
        self.soft = soft;
        self.record(&unmasked, entry);
    }

    /// The coverage of `shape` without the soft mask, for the plane, where a
    /// soft mask is in force and the paint records; `None` otherwise, when
    /// the coverage the paint composites through is its shape.
    fn unmasked(&mut self, shape: Shape<'_>, entry: u8) -> Option<Mask> {
        if !self.records(entry) || self.soft.is_none() {
            return None;
        }
        let soft = self.soft.take();
        let unmasked = self.coverage(shape, None);
        self.soft = soft;
        Some(unmasked)
    }

    /// Records where a shading defines a colour over `area`, its shape for
    /// the plane (11.6.4.2: `sh`'s shape is its painting geometry).
    fn record_shading(
        &mut self,
        shading: &Shading,
        inverse: &Matrix,
        area: Option<&Mask>,
        entry: u8,
    ) {
        if !self.records(entry) {
            return;
        }
        let (ox, oy, width, height) = self.device();
        let (x0, y0, x1, y1) = area.map_or((0, 0, width, height), |mask| {
            mask.overlap_at((ox, oy), width, height)
        });
        let mut shape = Mask::empty(
            ox.saturating_add(x0 as i32),
            oy.saturating_add(y0 as i32),
            x1 - x0,
            y1 - y0,
        );
        let mut scratch = ShadingScratch::default();
        for row in y0..y1 {
            let py = oy.saturating_add(row as i32);
            let mut walk = shading.row(inverse, f64::from(py) + 0.5);
            for col in x0..x1 {
                let px = ox.saturating_add(col as i32);
                if area.is_some_and(|mask| mask.at(px, py) == 0) {
                    continue;
                }
                if walk.color_at(i64::from(px), &mut scratch).is_some() {
                    let at = (row - y0) as usize * (x1 - x0) as usize + (col - x0) as usize;
                    if let Some(slot) = shape.data.get_mut(at) {
                        *slot = 255;
                    }
                }
            }
        }
        self.count_evaluations(&scratch);
        self.record(&shape, entry);
    }

    /// The canvas and everything the render had to tolerate.
    #[must_use]
    pub fn finish(mut self) -> (Canvas, Vec<RenderWarning>) {
        // Before anything reads the canvas: a page whose last operation was an
        // image would otherwise hand back a canvas the run never reached.
        self.flush_run();
        // A group or a soft mask's group that was opened and never closed
        // would otherwise hand the caller its buffer as the page — and a
        // group's buffer holds the page itself until it closes. The
        // interpreter balances its own calls, but `Renderer` is public and
        // this is the one failure that turns a page into a fragment rather
        // than into wrong pixels. Each closes what was opened inside it first.
        while !self.groups.is_empty() {
            self.close_group();
        }
        while !self.mask_frames.is_empty() {
            self.close_soft_mask();
        }
        if self.missing_fonts > 0 {
            self.warnings.push(RenderWarning::UnreadableFont);
        }
        if self.admission.budget_spent() {
            self.warnings.push(RenderWarning::GroupBudgetSpent {
                opened: self.admission.buffers(),
            });
        }
        if self.recursion_refused {
            self.warnings.push(RenderWarning::RecursiveContent);
        }
        if self.nested_budget_spent {
            self.warnings.push(RenderWarning::NestedBudgetSpent);
        }
        // The flag, not the token. Asking the token here reports a page that
        // was drawn in full as cancelled whenever the caller's timeout lost
        // the race by a millisecond, which is the case where the caller most
        // wants to keep what it has.
        if self.skipped.load(Ordering::Relaxed) {
            self.warnings.push(RenderWarning::Cancelled);
        }
        (self.canvas, self.warnings)
    }

    /// What the render has drawn so far, as [`Renderer::finish`] would hand
    /// it back were the render to stop here, built on copies so that nothing
    /// the render goes on to draw changes (`docs/design/progressive-render.md`):
    ///
    /// - the canvas this renderer was made with — the page, or a tile of it at
    ///   its corner of the page frame — copied;
    /// - the run of images held back so that abutting ones do not conflate,
    ///   composited onto the copy under the clip and soft mask in force, as
    ///   its end will composite it, the run itself left accumulating;
    /// - each open transparency group closed onto a copy of what is under it,
    ///   innermost first, as its end closes it: its backdrop taken out, a
    ///   knockout group outside it restored where it painted, and the copy
    ///   composited at the group's own alpha, blend mode and soft mask;
    /// - each open soft mask's group dropped, a group opened inside it with
    ///   it: what it drew is a mask in the making and no page content, so what
    ///   is under it shows as it stood when its `gs` opened it.
    ///
    /// The brackets are walked in the order they opened, which each frame
    /// records as the other kind's count when it opened, the order the end of
    /// a render closes them in; a group's buffer holds what is under it, so
    /// the copy holds every canvas the walk closes onto. With no bracket open
    /// a snapshot is the canvas `finish` returns for the same calls. It counts
    /// nothing and reports nothing; the warnings are `finish`'s.
    #[must_use]
    pub fn snapshot(&self) -> Canvas {
        let mut canvas = self.canvas.clone();
        if let Some(run) = &self.run {
            let combined = self.combined_mask();
            let clip = combined
                .as_ref()
                .or(self.clip.as_ref())
                .or(self.soft.as_ref());
            run.fragments.composite_region_inked(
                &mut canvas,
                run.covered,
                run.alpha,
                run.blend,
                clip,
                run.ink,
                None,
            );
        }
        let (mut groups, mut masks) = (self.groups.len(), self.mask_frames.len());
        loop {
            // The innermost bracket: the last group, where every soft mask's
            // group open now was open when it opened, and the last soft
            // mask's group otherwise.
            let group = groups
                .checked_sub(1)
                .and_then(|at| self.groups.get(at))
                .filter(|frame| frame.masks_open == masks);
            if let Some(frame) = group {
                groups -= 1;
                let Some(mut parent) = canvas.release_parent() else {
                    break;
                };
                let mut buffer = canvas;
                buffer.remove_backdrop(&parent);
                // 11.4.5, as `close_group` takes it: the group outside this
                // one, where it knocks out, restored where this one painted.
                let outer = groups.checked_sub(1).and_then(|at| self.groups.get(at));
                if outer.is_some_and(|outer| outer.knockout) {
                    let area = buffer.to_mask(frame.at, MaskKind::Alpha, None);
                    parent.knock_out(&area);
                }
                // An RGB group's result reaches an ink parent through the
                // black generation and undercolour removal at its `Do`.
                let held = (frame.removal.is_some() && parent.format == PixelFormat::CmykA8)
                    .then(|| parent.set_ink_tables(frame.removal.clone()));
                parent.composite(
                    &buffer,
                    frame.at,
                    frame.alpha,
                    frame.blend,
                    frame.soft.as_ref(),
                    None,
                );
                if let Some(previous) = held {
                    parent.set_ink_tables(previous);
                }
                canvas = parent;
            } else if let Some(frame) = masks.checked_sub(1).and_then(|at| self.mask_frames.get(at))
            {
                masks -= 1;
                canvas = frame.parent.clone();
            } else {
                break;
            }
        }
        canvas
    }

    /// Opens a transparency group: a buffer of its own, bounded to the
    /// clip in force (11.6.6).
    ///
    /// The clip *is* the bound, and that is not a shortcut. 8.10.2 clips a
    /// form to its `/BBox`, the interpreter has already installed that clip
    /// by the time this is called, and the clip in force is the bounding box
    /// intersected with whatever encloses it — which is tighter than the
    /// bounding box and never larger. Sizing the buffer to the page instead
    /// is the memory blowup the plan's risk table names, multiplied by the
    /// nesting depth.
    /// The same colours as `parent`, but carrying alpha.
    ///
    /// What a buffer needs whatever it is for: a group, a soft mask and a
    /// tiling cell are all shapes over nothing, so each must record where it
    /// painted as well as what it painted.
    fn alpha_format(parent: PixelFormat) -> PixelFormat {
        match parent {
            PixelFormat::Gray8 | PixelFormat::GrayA8 => PixelFormat::GrayA8,
            PixelFormat::Rgb8 | PixelFormat::Rgba8 => PixelFormat::Rgba8,
            PixelFormat::CmykA8 => PixelFormat::CmykA8,
            PixelFormat::LabA8 => PixelFormat::LabA8,
            // A page never composites in a blue-first order (`canvas_format`),
            // but a caller handing this renderer such a canvas gets buffers in
            // its order, which the canvas composites right.
            PixelFormat::Bgr8 | PixelFormat::Bgra8 => PixelFormat::Bgra8,
            plates @ PixelFormat::Plates { .. } => plates,
        }
    }

    /// The buffer a transparency group declaring `space` composites in
    /// (11.6.6).
    ///
    /// Keyed by the shape of the space rather than by its identity, which is
    /// all a blend formula needs: how many components, whether they are
    /// subtractive, and — for `/Lab` — whether they need encoding into 0..1
    /// before 11.3.5's formulas can say anything about them at all.
    ///
    /// **Every space `GroupSpace` can name now has a buffer**, which is why
    /// this returns a format rather than an `Option` and why
    /// `RenderWarning::UnsupportedGroupSpace` no longer exists: a variant
    /// nothing can reach is a claim rather than a check.
    fn group_format_of(space: tinker_pdf_content::GroupSpace) -> PixelFormat {
        group_format(space)
    }

    fn open_group(&mut self, group: tinker_pdf_content::Group, state: &GraphicsState) -> bool {
        // Hidden content, `MAX_GROUP_DEPTH` and — because depth is not work
        // once the recursion branches — `MAX_GROUP_BUFFERS`. Asked of the
        // admission rather than decided here, so that a page recorded for a
        // display list was recorded with this answer.
        if !self.admission.begin_group() {
            return false;
        }
        let (x0, y0, width, height) = self.clip.as_ref().map_or(self.device(), |clip| {
            let (ox, oy) = self.canvas.origin();
            let (x0, y0, x1, y1) = clip.overlap_at((ox, oy), self.canvas.width, self.canvas.height);
            (
                ox.saturating_add(x0 as i32),
                oy.saturating_add(y0 as i32),
                x1 - x0,
                y1 - y0,
            )
        });
        // A group whose clip misses this canvas is still **accepted**, over a
        // buffer of no pixels. Declining it would make the answer the
        // interpreter acts on — reset the alphas or not — depend on which
        // pixels were asked for, and a display list recorded at one scale is
        // replayed at another: every answer this device gives has to be one
        // the whole page would give. Nothing painted into an empty buffer is
        // seen, which is what declining achieved.

        // A group buffer must carry alpha whatever the page format is: it
        // starts as nothing and accumulates a shape, and a format without an
        // alpha channel has no way to say where the group did not paint.
        // 11.6.6: the group composites in the space it declared, so its buffer
        // holds that space's components. Where it declared none it inherits the
        // one it will be composited into, which is what "the group's colour
        // space is the one it is painted onto" comes to in practice.
        //
        // Forced colours composite every group in the page's grey, whatever
        // space it declared: a paint there is black or a lightness, and a
        // trip through ink or Lab and back would round a grey that needs none.
        let format = match group.space {
            Some(space) if self.forced.is_none() => Self::group_format_of(space),
            _ => Self::alpha_format(self.canvas.format),
        };
        let buffer = Canvas::new(width, height, format, Color::TRANSPARENT).at_origin(x0, y0);
        self.count_group_buffer(&buffer);
        self.counts.group_bound_pixels = self
            .counts
            .group_bound_pixels
            .saturating_add(pixels_of(&buffer));
        // The buffer becomes the canvas everything paints into, and the canvas
        // it will be composited onto moves into it until the group closes: a
        // move of the canvas, never of its pixels. Nothing paints into the
        // parent while the group is open, so its own pixels are the backdrop.
        //
        // 11.4.4: an isolated group composites against nothing, so its buffer
        // starts empty and reads nothing of its parent; a non-isolated one
        // starts with the backdrop composited in, which is what lets a blend
        // mode inside it see through to the page. The backdrop is then taken
        // out again at close (11.4.7.2), or it is counted twice — once because
        // the group started from it and once because the group is composited
        // back onto it. That failure renders as a group that is *weaker* than
        // it should be, pulled toward whatever was underneath, which is a
        // plausible image.
        let parent = std::mem::replace(&mut self.canvas, buffer);
        if group.isolated {
            self.canvas.hold_parent(parent);
        } else {
            self.canvas.adopt_backdrop(parent);
        }
        // 11.7.5.2's conditions at the `Do`, and 11.7.5.3's functions for a
        // group whose result is RGB light reaching an ink parent.
        let opaque = state.fill_alpha >= 1.0
            && state.blend == tinker_pdf_content::BlendMode::Normal
            && self.soft.is_none();
        let removal = if format == PixelFormat::Rgba8 {
            self.removal_for(state.stored_parameters())
        } else {
            None
        };

        let frame = GroupFrame {
            at: (x0, y0),
            alpha: state.fill_alpha,
            blend: blend_mode(state.blend),
            clip: self.clip.clone(),
            soft: self.soft.take(),
            clip_bounds: self.clip_bounds,
            soft_bounds: self.soft_bounds.take(),
            frame: self.frame,
            // No knockout under forced colours: every element's coverage adds
            // to what is under it, inside a group as on the page.
            knockout: group.knockout && self.forced.is_none(),
            opaque,
            removal,
            clip_depth: self.clip_stack.len(),
            masks_open: self.mask_frames.len(),
        };
        // The buffer a whole-page render would have made: the clip's
        // rectangle on the page, within the parent's.
        self.frame = self
            .clip_bounds
            .map_or(self.frame, |clip| clip.within(self.frame));

        // Everything inside the group draws in the page's own frame, onto a
        // buffer that stands at `(x0, y0)` in it. The buffer used to carry a
        // frame of its own — `base` translated by the whole pixels to its
        // corner — which is exact as arithmetic and not as floating point, and
        // put a page and a tile over it an ulp apart (ruling 5). The clip is in
        // the same frame, so it is untouched.
        self.groups.push(frame);
        true
    }

    /// Counts a buffer a group allocated for its own pixels while it is open
    /// ([`RenderCounts::group_buffer_pixels`]).
    fn count_group_buffer(&mut self, buffer: &Canvas) {
        self.counts.group_buffer_pixels = self
            .counts
            .group_buffer_pixels
            .saturating_add(pixels_of(buffer));
    }

    /// Whether the innermost open group is a knockout one (11.4.5).
    fn in_knockout(&self) -> bool {
        self.groups.last().is_some_and(|frame| frame.knockout)
    }

    /// Discards what earlier elements left where this one is about to paint
    /// (11.4.5).
    ///
    /// Called by every paint that can reach a group buffer, with the coverage
    /// it is about to composite through. Doing it here rather than inside the
    /// composite is what makes "each element" mean an operator: a fill, a
    /// stroke, a glyph, an image, a shading, or a nested group's result.
    fn knockout_restore(&mut self, mask: &Mask) {
        if self.in_knockout() {
            self.canvas.knock_out(mask);
        }
    }

    /// Closes the innermost group and composites it onto its parent.
    ///
    /// Taking the parent back out of the buffer comes before compositing,
    /// unconditionally, so a cancelled render still hands back the page
    /// rather than the group's buffer; and a soft mask's group opened inside
    /// this one and never closed is closed first, so that the canvas this
    /// closes over is this group's buffer, holding its parent.
    fn close_group(&mut self) {
        let Some(masks_open) = self.groups.last().map(|frame| frame.masks_open) else {
            return;
        };
        while self.mask_frames.len() > masks_open {
            self.close_soft_mask();
        }
        let Some(frame) = self.groups.pop() else {
            return;
        };
        self.admission.end_group();
        self.clip_stack.truncate(frame.clip_depth);
        self.clip = frame.clip;
        self.clip_bounds = frame.clip_bounds;
        self.frame = frame.frame;
        let soft = frame.soft;
        let Some(parent) = self.canvas.release_parent() else {
            // Every open group's buffer holds its parent, and whatever was
            // opened inside it has just been closed, so this has nothing to
            // composite onto only if that stopped holding: the state the frame
            // kept goes back, and the canvas stays as it stands.
            self.soft = soft;
            self.soft_bounds = frame.soft_bounds;
            return;
        };
        let mut buffer = std::mem::replace(&mut self.canvas, parent);

        // 11.3.5.3 over a subtractive buffer: reported once per group rather
        // than once per pixel, which is what makes it a leniency record and not
        // a log. The comment beside `blend` in `tinker-pdf-raster` has claimed
        // since it was written that this was reported; until now it was not.
        if buffer.approximated_blends() > 0 {
            let warning = RenderWarning::ApproximatedGroupBlend;
            if !self.warnings.contains(&warning) {
                self.warnings.push(warning);
            }
        }

        // 11.4.7.2, `a0` and `C0` read from the parent, which nothing has
        // painted into since the group adopted it. A no-op on an isolated
        // group, which has no backdrop.
        buffer.remove_backdrop(&self.canvas);

        // 11.4.5: a nested group is one element of whatever encloses it, and
        // its shape is its own accumulated alpha. This is the only element
        // whose coverage comes from a buffer rather than from a path.
        if self.in_knockout() {
            let area = buffer.to_mask(frame.at, tinker_pdf_raster::MaskKind::Alpha, None);
            self.knockout_restore(&area);
        }

        // 11.6.6: the soft mask in force at the `Do` shapes the group's
        // *result*. Applying it to each element instead is what a drop shadow
        // looks like when it has been done element by element -- the mask
        // shows through every seam the group exists to remove.
        let stop = self.stop_predicate();
        let held = self.hold_removal(&frame.removal);
        self.canvas.composite(
            &buffer,
            frame.at,
            frame.alpha,
            frame.blend,
            soft.as_ref(),
            Some(&stop),
        );
        self.release_removal(held);
        self.soft = soft;
        self.soft_bounds = frame.soft_bounds;
    }

    /// Opens a soft-mask group: a buffer filled with `/BC`, to be read back
    /// as a mask when it closes (11.6.5.2).
    fn open_soft_mask(
        &mut self,
        mask: &tinker_pdf_content::MaskGroup,
        bbox: &[PathSegment],
    ) -> bool {
        // Depth, counting the groups open as well as the masks, and the
        // group-buffer budget: a mask group is a group buffer and spends from
        // the same budget -- and it is the one the campaign's timeout actually
        // recursed through.
        if !self.admission.begin_soft_mask() {
            return false;
        }
        // The group's own bounding box bounds the buffer, and the current
        // clip deliberately does *not*: the mask applies to whatever is
        // painted after the `gs`, which may be anywhere, and clipping the
        // mask to the clip in force when it was made would leave those paints
        // masked by nothing outside it.
        let built = self.to_path(bbox);
        let (x0, y0, width, height) = if built.is_empty() {
            self.device()
        } else {
            paint_region(built.bounds(), None, None, self.device())
        };
        // A box that misses this canvas is accepted over a buffer of no
        // pixels, for the reason `open_group` gives: declining depends on the
        // pixels asked for, and so did the *mask*, because a declined mask
        // left the previous one in force. A tile beside the box then painted
        // unmasked what the page, whose canvas the box did meet, masked by
        // `/BC`. Accepted, the mask is `/BC`'s value everywhere, which is
        // 11.6.5.2's answer outside the group.

        // 11.6.5.2. For `/Luminosity` the group is composited against a fully
        // opaque backdrop of `/BC`, and `/BC` absent is the **black** of the
        // group's colour space -- fully masked. For `/Alpha` there is no
        // backdrop at all and `/BC` does not apply.
        let (kind, background) = if mask.luminosity {
            let backdrop = mask.backdrop.unwrap_or(tinker_pdf_content::Rgb::BLACK);
            (
                MaskKind::Luminosity,
                Color::rgb(backdrop.r, backdrop.g, backdrop.b),
            )
        } else {
            (MaskKind::Alpha, Color::TRANSPARENT)
        };
        // What the mask reads outside the group's box: the backdrop alone,
        // through `/TR`. Zero for `/Alpha`, and zero for the `/BC` default.
        let outside = {
            let raw = match kind {
                // The identical function `to_mask` uses inside the box. Two
                // weightings here would put a step exactly at the mask's
                // bounding-box edge — a halo on a drop shadow, which reads as
                // a design choice rather than a defect.
                MaskKind::Luminosity => background.luminosity(),
                MaskKind::Alpha => 0,
            };
            mask.transfer
                .as_ref()
                .map_or(raw, |lut| lut.get(raw as usize).copied().unwrap_or(raw))
        };

        let format = Self::alpha_format(self.canvas.format);
        let buffer = Canvas::new(width, height, format, background).at_origin(x0, y0);
        self.count_group_buffer(&buffer);
        self.counts.group_bound_pixels = self
            .counts
            .group_bound_pixels
            .saturating_add(pixels_of(&buffer));

        let frame = MaskFrame {
            parent: std::mem::replace(&mut self.canvas, buffer),
            at: (x0, y0),
            kind,
            outside,
            transfer: mask.transfer,
            clip: self.clip.take(),
            soft: self.soft.take(),
            clip_bounds: self.clip_bounds.take(),
            soft_bounds: self.soft_bounds.take(),
            frame: self.frame,
            clip_depth: self.clip_stack.len(),
            groups_open: self.groups.len(),
        };
        // The buffer a whole-page render would have made, from the same
        // arithmetic as the one above.
        if !built.is_empty() {
            self.frame = region_within(built.bounds(), &[], self.frame);
        }
        // The mask group draws under no clip and no mask of its own: it is a
        // picture of a mask, not a painting on the page. It draws in the
        // page's frame, onto a buffer standing at `(x0, y0)` in it.
        self.mask_frames.push(frame);
        true
    }

    /// Closes the innermost soft-mask group and installs what it drew.
    ///
    /// A transparency group opened inside the mask's group and never closed
    /// is closed first, so that the canvas this closes over is the mask's
    /// buffer.
    fn close_soft_mask(&mut self) {
        let Some(groups_open) = self.mask_frames.last().map(|frame| frame.groups_open) else {
            return;
        };
        while self.groups.len() > groups_open {
            self.close_group();
        }
        let Some(frame) = self.mask_frames.pop() else {
            return;
        };
        self.admission.end_soft_mask();
        self.clip_stack.truncate(frame.clip_depth);
        let buffer = std::mem::replace(&mut self.canvas, frame.parent);
        self.clip = frame.clip;
        self.clip_bounds = frame.clip_bounds;
        // The mask's own buffer on the page, before the parent's is restored.
        let inner_bounds = mem::replace(&mut self.frame, frame.frame);

        // Forced colours install no soft mask: the group was accepted for the
        // answer an ordinary render gives, and nothing in it was drawn.
        if self.forced.is_some() {
            self.soft = None;
            self.soft_bounds = None;
            return;
        }

        // The group's box, read back, and beyond it the backdrop's value
        // (11.6.5.2). With a pale `/BC` the page outside the box is
        // *unmasked*, by that value, and a mask reading zero there would hide
        // it; carried as the mask's outside value rather than as a field of it
        // the size of the canvas, so a pale mask costs its box as a black one
        // does (`docs/design/performance.md`, P-25).
        let mask = buffer
            .to_mask(frame.at, frame.kind, frame.transfer.as_ref())
            .with_outside(frame.outside);
        self.counts.soft_mask_pixels = self
            .counts
            .soft_mask_pixels
            .saturating_add(u64::from(mask.width) * u64::from(mask.height));
        self.counts.soft_mask_bound_pixels = self
            .counts
            .soft_mask_bound_pixels
            .saturating_add(pixels_of(&buffer));
        self.soft = Some(mask);
        // A mask reading a value outside its box confines no paint, so on the
        // whole page its rectangle is the whole frame (`Bounds::limit_of`).
        self.soft_bounds = Some(if frame.outside == 0 {
            inner_bounds
        } else {
            self.frame
        });
        // The mask the `gs` replaced is gone, not restored: 11.6.5.1 makes
        // `/SMask` a graphics-state parameter that one `gs` overwrites for
        // the next, and only `Q` puts an older one back.
        let _ = (frame.soft, frame.soft_bounds);
    }

    /// The clip and the soft mask as one mask, for a consumer that can take
    /// only one.
    ///
    /// Returns an owned product when both are in force and nothing at all
    /// when neither is; with one of the two the caller uses it directly, so
    /// the common paths still allocate nothing.
    fn combined_mask(&self) -> Option<Mask> {
        match (&self.clip, &self.soft) {
            (Some(clip), Some(soft)) => Some(clip.intersect(soft)),
            _ => None,
        }
    }

    /// What an image draw whose unit square `to_device` maps into the page
    /// frame asks of its decode ([`ImageRequest`]): the size it is drawn at,
    /// the part of it this canvas can reach — under the clip and the soft mask
    /// too where the draw composites through them on its own, as
    /// [`Renderer::blit`] decides — and the part they leave visible.
    fn image_request(&self, to_device: &Matrix) -> ImageRequest {
        let canvas = Bounds::of_rect(self.device());
        let shown = [self.clip.as_ref(), self.soft.as_ref()]
            .into_iter()
            .flatten()
            .fold(canvas, |shown, mask| shown.meet(Bounds::of_mask(mask)));
        // `blit`'s own test for a draw that is no member of a run.
        let alone = self.in_knockout() || self.frame_area() > MAX_IMAGE_RUN_PIXELS;
        let area = |bounds: Bounds| {
            UnitArea::of_device(
                to_device,
                (
                    bounds.left as f64,
                    bounds.top as f64,
                    bounds.right as f64,
                    bounds.bottom as f64,
                ),
            )
        };
        ImageRequest {
            reach: area(if alone { shown } else { canvas }),
            visible: area(shown),
            ..ImageRequest::drawn_under(to_device)
        }
    }

    /// Fills an undecodable image's area with a neutral grey.
    ///
    /// Ruling 2 asks for a placeholder, not merely a warning, and the
    /// difference is what a reader sees: an image that silently occupies no
    /// space lets the page look complete and wrong. Grey rather than a
    /// diagonal-cross convention because it composites predictably over
    /// whatever is beneath it and never reads as content.
    ///
    /// 8.9.5.2: the image occupies the unit square of the current transform,
    /// so the placeholder is that square — exactly the area the real image
    /// would have covered.
    fn draw_image_placeholder(&mut self, state: &GraphicsState) {
        let unit_to_device = state.ctm.then(&self.base);
        let corners = [
            unit_to_device.apply(0.0, 0.0),
            unit_to_device.apply(1.0, 0.0),
            unit_to_device.apply(1.0, 1.0),
            unit_to_device.apply(0.0, 1.0),
        ];
        if corners
            .iter()
            .any(|(x, y)| !x.is_finite() || !y.is_finite())
        {
            return;
        }

        let mut path = Path::new();
        path.move_to(corners[0].0, corners[0].1);
        for (x, y) in &corners[1..] {
            path.line_to(*x, *y);
        }
        path.close();

        const PLACEHOLDER: Color = Color {
            r: 0xBF,
            g: 0xBF,
            b: 0xBF,
            a: 0xFF,
        };
        // It stands where the image would have, as an opaque fill of its
        // square under the state's alpha.
        let set = self.device_set(state, state.fill_alpha, ColorFamily::Other, true);
        self.paint(
            &path,
            FillRule::NonZero,
            PLACEHOLDER,
            state.fill_alpha,
            blend_mode(state.blend),
            &set,
        );
    }

    /// Fills a shape with a pattern, or reports one that cannot be.
    ///
    /// 8.7.3.1: a pattern's matrix maps pattern space to the *default* space
    /// of the page, not to the space in force when it is used. The CTM at fill
    /// time is therefore not part of it, which is why `base` appears in both
    /// branches below and `state.ctm` appears in neither. A stroke reaches
    /// this through its outline ([`Shape::Stroke`]), so that guarantee covers
    /// strokes too — the stroker's transform never enters the pattern.
    ///
    /// `alpha` is a parameter rather than a field of `state` because 8.4.5
    /// gives painting two of them: a fill uses `ca`, a stroke uses `CA`, and
    /// the callers are the only things that know which they are. `color` is a
    /// parameter for exactly the same reason, and it is the same mistake
    /// waiting to be made: 8.7.3.3 gives an uncoloured pattern the colour of
    /// the slot that named it, and a stroke's slot is not a fill's.
    fn fill_with_pattern(
        &mut self,
        shape: Shape<'_>,
        name: &[u8],
        state: &GraphicsState,
        alpha: f64,
        color: Color,
        family: ColorFamily,
    ) {
        match self.glyphs.pattern(name) {
            Some(PatternPaint::Shading(shading, matrix)) => {
                // A mesh too large to paint is reported as the unpainted
                // pattern it is, exactly as an unreadable tiling cell is: the
                // area is left alone rather than filled with black.
                if !self.fill_with_shading(shape, &shading, matrix, state, alpha) {
                    self.report_unpainted_pattern(name);
                }
            }
            Some(PatternPaint::Tiling(tiling)) => {
                if !self.fill_with_tiles(shape, name, &tiling, state, alpha, color, family) {
                    self.report_unpainted_pattern(name);
                }
            }
            Some(PatternPaint::Unsupported) => self.report_unpainted_pattern(name),
            None => (),
        }
    }

    /// Records that a named pattern was left unpainted (ruling 2, ruling 10).
    fn report_unpainted_pattern(&mut self, name: &[u8]) {
        let warning = RenderWarning::UnsupportedPattern {
            name: String::from_utf8_lossy(name).into_owned(),
        };
        if !self.warnings.contains(&warning) {
            self.warnings.push(warning);
        }
    }

    /// Fills a shape with a shading pattern.
    ///
    /// The shape becomes the clip and the shading is evaluated per pixel
    /// inside it, which is the same machinery `sh` uses — a shading pattern is
    /// `sh` bounded by a shape rather than by the current clip.
    ///
    /// False means nothing was painted and the caller should say so: a mesh
    /// past its work budget is the only way that happens.
    fn fill_with_shading(
        &mut self,
        shape: Shape<'_>,
        shading: &Shading,
        matrix: Matrix,
        state: &GraphicsState,
        alpha: f64,
    ) -> bool {
        let to_device = matrix.then(&self.base);
        let stop = self.stop_predicate();
        let area = self.coverage(shape, Some(&stop));
        let alpha = alpha.clamp(0.0, 1.0);
        let mode = blend_mode(state.blend);
        let set = self.device_set(state, alpha, shading_family(shading), true);
        // The shape for the plane: the fill's coverage, without the soft mask
        // where one is in force (11.6.4.3).
        let unmasked = self.unmasked(shape, set.entry);
        // 11.7.5.3: a shading pattern's colours convert in the intent in
        // force at the fill or stroke that paints it.
        let conversion = state.stored_parameters().rendering_intent.conversion();

        // A mesh is geometry rather than a function of position, so it takes
        // the other route entirely — and it does its own knockout restore,
        // with its own coverage rather than the shape's.
        if let Some(mesh) = shading.mesh() {
            let limit = self.page_region(shape);
            let shape_area = unmasked.as_ref().unwrap_or(&area);
            return self.paint_mesh(
                mesh,
                to_device,
                Some(&area),
                Some(shape_area),
                limit,
                alpha,
                mode,
                &set,
                state.stored_parameters().smoothness,
                conversion,
            );
        }

        let Some(inverse) = invert(&to_device) else {
            return true;
        };
        self.record_shading(
            shading,
            &inverse,
            unmasked.as_ref().or(Some(&area)),
            set.entry,
        );
        if self.in_knockout() {
            self.knockout_restore(&area);
        }
        let held = self.hold_removal(&set.removal);
        // The shape's own rectangle. A shading is evaluated per pixel, so a
        // gradient-filled comma used to run the inverse transform and the
        // function over every pixel of the page to paint two hundred of them.
        let (ox, oy) = self.canvas.origin();
        let (x_start, y_start, x_end, y_end) =
            area.overlap_at((ox, oy), self.canvas.width, self.canvas.height);
        let mut scratch = ShadingScratch::converting(conversion);
        // P-14.5: through a table, where the shape's rectangle on the whole
        // page asks for more colours than the table holds.
        if let Some((space, function)) = shading.parametric() {
            scratch.tabulate(space, function, 0.0, 1.0, self.page_region(shape).area());
        }
        for row in y_start..y_end {
            if self.stopping() {
                self.release_removal(held);
                self.count_evaluations(&scratch);
                return true;
            }
            // The device pixel, in the page's frame: the one number a tile
            // and the page under it both sample at, and the frame a row's
            // steps restart in (ruling 5).
            let py = oy.saturating_add(row as i32);
            let mut walk = shading.row(&inverse, f64::from(py) + 0.5);
            for col in x_start..x_end {
                let px = ox.saturating_add(col as i32);
                let coverage = area.at(px, py);
                if coverage == 0 {
                    continue;
                }
                let Some((r, g, b)) = walk.color_at(i64::from(px), &mut scratch) else {
                    continue;
                };
                let weight = alpha * f64::from(coverage) / 255.0;
                self.canvas
                    .blend_pixel_with(col, row, Color { r, g, b, a: 0xFF }, weight, mode);
            }
        }
        self.release_removal(held);
        self.count_evaluations(&scratch);
        true
    }

    /// Adds the function evaluations a sweep made to the render's count.
    fn count_evaluations(&mut self, scratch: &ShadingScratch) {
        self.counts.shading_functions_evaluated = self
            .counts
            .shading_functions_evaluated
            .saturating_add(scratch.evaluations());
    }

    /// Paints a mesh shading through a coverage mask (8.7.4.5.5 to 8.7.4.5.8).
    ///
    /// False means the mesh was refused: the caller reports it, naming the
    /// shading type. The two ways that happens are a tessellation larger than
    /// `MAX_MESH_TRIANGLES` and a rasterisation past `MAX_MESH_WORK`, and both
    /// paint nothing at all rather than part of a mesh — the same answer
    /// `fill_with_tiles` gives to a lattice that will not fit, and for the same
    /// reason: a fragment reads as an artefact where a gap reads as a gap.
    ///
    /// **The whole mesh goes through one buffer.** Compositing per triangle
    /// would anti-alias every shared edge against the canvas and leave a
    /// lattice of pale seams; `draw_mesh` returns colour and coverage over one
    /// rectangle and this composites that rectangle once.
    ///
    /// `limit` is `area`'s rectangle on a render of the whole page, which is
    /// what the mesh's region is taken from: see [`draw_mesh_over`].
    ///
    /// `shape_area` is `area` without the soft mask, the region the mesh's
    /// shape is recorded over for the plane (11.6.4.3), and `set` what the
    /// paint takes from the device-dependent state. The vertices' colours
    /// convert as `conversion` says, in the intent in force at the operator
    /// that paints the mesh.
    ///
    /// `smoothness` is the `/SM` in force (10.7.3): a patch is subdivided until
    /// its colour is within it as well as by its device length
    /// ([`Mesh::tessellate`]). The count is a function of the patch and the
    /// page's transform alone, so a tile cuts the page's mesh (ruling 5).
    #[allow(clippy::too_many_arguments)]
    fn paint_mesh(
        &mut self,
        mesh: &Mesh,
        to_device: Matrix,
        area: Option<&Mask>,
        shape_area: Option<&Mask>,
        limit: Bounds,
        alpha: f64,
        mode: RasterBlend,
        set: &DeviceSet,
        smoothness: Option<f64>,
        conversion: Conversion,
    ) -> bool {
        let Some(tessellation) = mesh.tessellate(&to_device, smoothness) else {
            return false;
        };
        if tessellation.triangles.is_empty() {
            return true; // An empty mesh paints nothing, and that is not a refusal.
        }

        // The mesh's own device rectangle, bounded by the clip exactly as a
        // fill's is: a mesh written far off the page must cost the page rather
        // than the coordinates.
        let mut bbox = Path::new();
        for (x, y) in &tessellation.positions {
            if bbox.is_empty() {
                bbox.move_to(*x, *y);
            } else {
                bbox.line_to(*x, *y);
            }
        }
        //
        // Taken on the whole page and then narrowed to this canvas by the
        // rasterizer, which knows how far past the canvas a silhouette pixel
        // looks for its colour (ruling 5).
        let whole = region_within(bbox.bounds(), &[Some(limit)], self.frame);
        if whole.is_empty() {
            return true;
        }
        let wanted = Bounds::of_rect(self.device()).meet(whole);

        let stop = self.stop_predicate();
        // 8.7.4.5.5: the vertex values are interpolated and *then* converted,
        // which is what this closure is. Handing the rasteriser RGB at the
        // vertices instead would interpolate in device space and move the
        // midpoint of every non-linear colour space and every non-linear
        // `/Function`.
        //
        // One shading scratch for the whole colour pass, taken out of its
        // cell for each pixel and put back, so the pass allocates nothing per
        // pixel; a mesh carrying a function reads it through a table over
        // the inputs its vertices span (P-14.5), decided on the mesh's
        // rectangle on the whole page. Its colours convert in the intent in
        // force at the paint, the table's entries among them.
        let mut kept = ShadingScratch::converting(conversion);
        if let (Some(function), 1) = (&mesh.function, mesh.components) {
            let (low, high) = tessellation
                .values
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), value| {
                    (low.min(*value), high.max(*value))
                });
            kept.tabulate(&mesh.space, function, low, high, whole.area());
        }
        let shading = core::cell::Cell::new(kept);
        let convert = |inputs: &[f64]| {
            let mut taken = shading.take();
            let (r, g, b) = mesh.color(inputs, &mut taken);
            shading.set(taken);
            Color { r, g, b, a: 0xFF }
        };
        let draw = MeshDraw {
            positions: &tessellation.positions,
            values: &tessellation.values,
            components: mesh.components,
            triangles: &tessellation.triangles,
            color: &convert,
            stop: Some(&stop),
            scale: self.pixels_per_square_unit().sqrt(),
            // The silhouette at its level, the colours spread before it.
            edge_bits: self.level(),
            edge_rule: self.hard_edge,
        };
        let drawn = draw_mesh_over(&draw, wanted.rect(), whole.rect(), &mut self.scratch);
        let kept = shading.take();
        self.count_evaluations(&kept);
        let Some(mut buffer) = drawn else {
            return false;
        };
        #[cfg(test)]
        {
            self.mask_pixels = self.mask_pixels.saturating_add(
                u64::from(buffer.coverage.width) * u64::from(buffer.coverage.height),
            );
        }

        // The mesh's shape for the plane: its coverage over the area the
        // paint is shaped by, without the soft mask.
        if self.records(set.entry) {
            let mut shape = buffer.coverage.clone();
            if let Some(shape_area) = shape_area {
                shape.intersect_in_place(shape_area);
            }
            self.record(&shape, set.entry);
        }
        // The element's coverage is the mesh's own, not the clip's: 11.4.5's
        // restore has to know where this element actually painted, and `sh`
        // with a mesh does not paint the whole clip the way `sh` with a
        // gradient does.
        if let Some(area) = area {
            buffer.coverage.intersect_in_place(area);
        }
        if self.in_knockout() {
            self.knockout_restore(&buffer.coverage);
        }

        let (ox, oy) = self.canvas.origin();
        let (x_start, y_start, x_end, y_end) =
            buffer
                .coverage
                .overlap_at((ox, oy), self.canvas.width, self.canvas.height);
        let held = self.hold_removal(&set.removal);
        for row in y_start..y_end {
            if self.stopping() {
                self.release_removal(held);
                return true;
            }
            for col in x_start..x_end {
                let (px, py) = (ox.saturating_add(col as i32), oy.saturating_add(row as i32));
                let coverage = buffer.coverage.at(px, py);
                if coverage == 0 {
                    continue;
                }
                let Some(color) = buffer.at(px, py) else {
                    continue;
                };
                let weight = alpha * f64::from(coverage) / 255.0;
                self.canvas.blend_pixel_with(col, row, color, weight, mode);
            }
        }
        self.release_removal(held);
        true
    }

    /// Fills a shape with a tiling pattern: one cell, rasterized once, blitted
    /// across the lattice (8.7.3.2).
    ///
    /// Returns false when the pattern could not be painted at all, which the
    /// caller turns into a warning. Painting *some* of a lattice is not one of
    /// the answers: a hatch covering part of a shape reads as a rendering
    /// artefact, and an unpainted area reads as the gap it is.
    ///
    /// **Why the cell is rasterized once.** The obvious implementation replays
    /// the cell's content stream per lattice position with a translated CTM.
    /// It is correct and it is unusable: each position needs its own bounding
    /// box clip, every clip goes through `save_state`, and `save_state` clones
    /// a page-sized mask. A 10 pt hatch over A4 is about 4 800 cells and
    /// several gigabytes of memcpy. Dropping the per-cell clip to avoid that is
    /// how the anchoring above gets lost.
    ///
    /// **Why the buffer is `/BBox`-sized.** A cell whose bounding box is
    /// smaller than its step leaves gaps between cells, and a neighbour must
    /// not spill into them. With the buffer sized to the box there is nowhere
    /// for a neighbour's ink to be — the spill is impossible by construction
    /// rather than prevented by a clip that could be forgotten.
    #[allow(clippy::too_many_arguments)]
    fn fill_with_tiles(
        &mut self,
        shape: Shape<'_>,
        name: &[u8],
        tiling: &TilingPattern,
        state: &GraphicsState,
        alpha: f64,
        color: Color,
        family: ColorFamily,
    ) -> bool {
        if self.pattern_depth >= MAX_PATTERN_DEPTH {
            return false;
        }

        // 8.7.3.1, and the whole of this function's correctness: `base`, the
        // parent stream's default space, and not `state.ctm`. Anchoring to the
        // paint-time CTM makes the lattice slide under every transform, which
        // reads as a small offset rather than as a defect and is therefore
        // never reported.
        let to_device = tiling.matrix.then(&self.base);
        if !to_device.is_finite() {
            return false;
        }
        let Some(inverse) = invert(&to_device) else {
            return false;
        };

        let [bx0, by0, bx1, by1] = tiling.bbox;
        if ![bx0, by0, bx1, by1].iter().all(|v| v.is_finite()) || bx1 <= bx0 || by1 <= by0 {
            return false;
        }
        // 8.7.3.2 makes both steps required and neither may be zero. A file
        // that says zero anyway gets the cell's own size, which is the only
        // reading that tiles at all; the alternative is a division by zero and
        // an infinite lattice.
        let step = |given: f64, extent: f64| {
            if given.is_finite() && given.abs() > 1e-9 {
                given.abs()
            } else {
                extent
            }
        };
        let xstep = step(tiling.xstep, bx1 - bx0);
        let ystep = step(tiling.ystep, by1 - by0);
        if !xstep.is_finite() || !ystep.is_finite() || xstep <= 0.0 || ystep <= 0.0 {
            return false;
        }

        // The device pixels this fill can reach, without rasterizing anything:
        // the lattice has to be bounded *before* a cell is drawn, or a
        // pathological `/XStep` has already cost a rasterization by the time it
        // is refused.
        //
        // **On the whole page, not on this canvas** (ruling 5). A cell's buffer
        // is its box rounded outward and it lands at a rounded offset, so a
        // cell whose box stops just short of a rectangle still puts a pixel of
        // anti-aliased edge inside it. Indexed from a tile's own rectangle,
        // the lattice left out a cell the page's included, and the tile lost
        // that pixel; measured on `render_regions.rs`'s tiling fixture, ten
        // levels at 1x. Indexed from the page's, every canvas composites the
        // same cells in the same order, and the budgets below refuse the same
        // lattices.
        let reach = self.page_region(shape);
        if reach.is_empty() {
            return true;
        }
        let (rx, ry, rw, rh) = reach.rect();

        // That rectangle back in pattern space. A rotated or skewed `/Matrix`
        // turns it into a parallelogram, so the axis-aligned hull of the four
        // corners is what indexes the lattice — larger than needed, never
        // smaller, and a cell that lands off the canvas costs nothing because
        // `composite` walks the overlap.
        let (rx1, ry1) = (f64::from(rx) + f64::from(rw), f64::from(ry) + f64::from(rh));
        let corners = [
            inverse.apply(f64::from(rx), f64::from(ry)),
            inverse.apply(rx1, f64::from(ry)),
            inverse.apply(rx1, ry1),
            inverse.apply(f64::from(rx), ry1),
        ];
        if corners
            .iter()
            .any(|(x, y)| !x.is_finite() || !y.is_finite())
        {
            return false;
        }
        let hull = |pick: fn(&(f64, f64)) -> f64| {
            let values = corners.iter().map(pick);
            let lo = values.clone().fold(f64::INFINITY, f64::min);
            let hi = values.fold(f64::NEG_INFINITY, f64::max);
            (lo, hi)
        };
        let (px0, px1) = hull(|c| c.0);
        let (py0, py1) = hull(|c| c.1);

        // Cell `(i, j)` covers `[bx0 + i*xstep, bx1 + i*xstep]`, so it meets
        // the region exactly when `i` is between these two. The far edge uses
        // the *near* edge of the box and vice versa, which is the off-by-one
        // that shows as a missing row of cells down one side of a shape.
        let Some((i0, i1)) = lattice_range(px0, px1, bx0, bx1, xstep) else {
            return false;
        };
        let Some((j0, j1)) = lattice_range(py0, py1, by0, by1, ystep) else {
            return false;
        };
        let across = i1 - i0 + 1;
        let down = j1 - j0 + 1;
        let tiles = (across as u64).saturating_mul(down as u64);
        if tiles > MAX_TILES {
            return false;
        }
        if tiles == 0 {
            // The shape sits entirely in a gap between cells, which a step
            // larger than the box makes legal. Nothing to paint, and nothing
            // to report.
            return true;
        }

        // The cell's own device rectangle, rounded outward. No slack: the
        // buffer is the bounding box and nothing else, so a neighbour cannot
        // reach into a gap the file asked for.
        let cell = [
            to_device.apply(bx0, by0),
            to_device.apply(bx1, by0),
            to_device.apply(bx1, by1),
            to_device.apply(bx0, by1),
        ];
        if cell.iter().any(|(x, y)| !x.is_finite() || !y.is_finite()) {
            return false;
        }
        let span = |pick: fn(&(f64, f64)) -> f64| {
            let values = cell.iter().map(pick);
            let lo = values.clone().fold(f64::INFINITY, f64::min).floor();
            let hi = values.fold(f64::NEG_INFINITY, f64::max).ceil();
            (lo, hi)
        };
        let (tx0, tx1) = span(|c| c.0);
        let (ty0, ty1) = span(|c| c.1);
        let extent = |lo: f64, hi: f64| -> Option<u32> {
            let size = hi - lo;
            (size.is_finite() && size >= 1.0 && size <= f64::from(u32::MAX)).then_some(size as u32)
        };
        let (Some(width), Some(height)) = (extent(tx0, tx1), extent(ty0, ty1)) else {
            return false;
        };
        let area = u64::from(width) * u64::from(height);
        if area > MAX_TILE_AREA {
            return false;
        }
        // The page's frame rather than the canvas, so a tile of a page and
        // the page take this decision against the same number; and in square
        // units of the default space rather than in pixels, so a render at
        // another scale takes it against the same number too.
        let canvas_area = self.frame_area();
        let work = tiles.saturating_mul(area.min(canvas_area.max(1)));
        if work as f64 / self.pixels_per_square_unit() > MAX_TILE_WORK as f64 {
            return false;
        }
        // Every decision above is the page's. Whether any of it lands on
        // *this* canvas is not a decision, and a fill that misses it costs no
        // cell: the coverage below is zero everywhere off the page-frame
        // rectangle.
        if reach.meet(Bounds::of_rect(self.device())).is_empty() {
            return true;
        }

        // A cell is a shape over nothing, so its buffer must carry alpha
        // whatever the page's format is.
        let format = Self::alpha_format(self.canvas.format);
        let request = TileRequest {
            to_pixels: to_device.then(&Matrix::translate(-tx0, -ty0)),
            bbox: tiling.bbox,
            width,
            height,
            format,
            cancel: self.cancel.clone(),
            depth: self.pattern_depth + 1,
            antialias: self.antialias,
            graphics_bits: self.graphics_bits,
            text_bits: self.text_bits,
            hard_edge: self.hard_edge,
            thinnest_line: self.thinnest_line,
        };
        let Some(tile) = self.glyphs.tile(name, &request) else {
            return false;
        };
        for warning in tile.warnings {
            if !self.warnings.contains(&warning) {
                self.warnings.push(warning);
            }
        }
        // 11.7.5.2: an object painted with a tiling pattern is fully opaque
        // only if every object of its cell is.
        let set = self.device_set(state, alpha, family, tile.opaque);
        let tile = tile.canvas;

        let stop = self.stop_predicate();
        let coverage = self.coverage(shape, Some(&stop));
        let unmasked = self.unmasked(shape, set.entry);
        if self.in_knockout() {
            self.knockout_restore(&coverage);
        }
        let mode = blend_mode(state.blend);
        // 8.7.3.3: a `PaintType 2` cell is a shape and the `SCN` operands its
        // colour, so whatever the cell's content stream said about colour is
        // discarded here, at the blit, rather than intercepted at every paint
        // inside it -- which is what makes "colour operators inside it are
        // ignored" true of operators this engine has not implemented yet as
        // well. Opaque, as the colour slot's light is: the fill's `ca` is
        // `alpha`.
        let tint = tiling
            .uncolored
            .then(|| Color::rgb(color.r, color.g, color.b));
        // That colour is light of the state's family, and on an ink canvas it
        // becomes ink by the black generation and undercolour removal the
        // state holds (11.7.5.3); a coloured cell's paints took their own.
        let held = if tint.is_some() {
            self.hold_removal(&set.removal)
        } else {
            None
        };
        // The fill's shape for the plane: the shape where the cells painted
        // (11.6.4.2, 11.6.7), gathered as they are placed.
        let mut cells = Vec::new();

        // The lattice step as a device *vector*: a translation in pattern
        // space, so the matrix's translation cancels and only its linear part
        // survives.
        let origin = to_device.apply(0.0, 0.0);
        for j in j0..=j1 {
            // Gap 15's hook. A large lattice is precisely the long-running
            // work it exists for, and `composite` is asked again every sixteen
            // rows inside each cell.
            if self.stopping() {
                self.release_removal(held);
                return true;
            }
            for i in i0..=i1 {
                let (dx, dy) = to_device.apply(i as f64 * xstep, j as f64 * ystep);
                let (x, y) = (tx0 + dx - origin.0, ty0 + dy - origin.1);
                if !x.is_finite() || !y.is_finite() {
                    continue;
                }
                let at = (
                    x.round().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32,
                    y.round().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32,
                );
                match tint {
                    Some(tint) => self.canvas.composite_tinted(
                        &tile,
                        tint,
                        at,
                        alpha,
                        mode,
                        Some(&coverage),
                        Some(&stop),
                    ),
                    None => {
                        self.canvas
                            .composite(&tile, at, alpha, mode, Some(&coverage), Some(&stop));
                    }
                }
                if self.records(set.entry) {
                    cells.push(at);
                }
            }
        }
        self.release_removal(held);
        if self.records(set.entry) {
            let shape_area = unmasked.as_ref().unwrap_or(&coverage);
            let mut painted = Mask::empty(
                shape_area.x0,
                shape_area.y0,
                shape_area.width,
                shape_area.height,
            );
            for (cx, cy) in cells {
                for sy in 0..tile.height {
                    for sx in 0..tile.width {
                        if tile.pixel(sx, sy).is_none_or(|pixel| pixel.a == 0) {
                            continue;
                        }
                        let (x, y) = (cx.saturating_add(sx as i32), cy.saturating_add(sy as i32));
                        if shape_area.at(x, y) == 0 {
                            continue;
                        }
                        let (lx, ly) = (
                            i64::from(x) - i64::from(painted.x0),
                            i64::from(y) - i64::from(painted.y0),
                        );
                        if lx < 0 || ly < 0 {
                            continue;
                        }
                        let at = ly as usize * painted.width as usize + lx as usize;
                        if let Some(slot) = painted.data.get_mut(at) {
                            *slot = 255;
                        }
                    }
                }
            }
            self.record(&painted, set.entry);
        }
        true
    }

    /// A **new clip**: a path's coverage times the clip in force — and not the
    /// soft mask.
    ///
    /// 8.5.4's clip and 11.6.5's soft mask are two parameters of the graphics
    /// state, and a clip installed while a mask is in force is not masked: the
    /// mask goes on being applied to what is painted, by [`Renderer::coverage`]
    /// and by a group's composite, as long as it is in force and no longer.
    /// Folding it into the clip applied it twice to everything painted under
    /// both — a transparency group under a luminosity mask of grey 0.5 drew
    /// at a quarter, because the interpreter installs the form's `/BBox` clip
    /// before the group takes the mask, so the group's content was masked
    /// through its clip and its composite masked again — and went on applying
    /// it after an `/SMask /None` had turned it off, for as long as the clip
    /// lasted.
    ///
    /// Returns the clip and its page-frame rectangle, both made with the mask
    /// out of the way: the rectangle is the clip mask's, and a rectangle
    /// narrowed by the mask's would stand beside a mask that is not.
    fn clip_coverage(
        &mut self,
        path: &Path,
        rule: FillRule,
        stop: Option<&dyn Fn() -> bool>,
    ) -> (Mask, Bounds) {
        let soft = self.soft.take();
        let soft_bounds = self.soft_bounds.take();
        let bounds = self.page_region(Shape::Fill(path, rule));
        let mask = self.coverage(Shape::Fill(path, rule), stop);
        self.soft = soft;
        self.soft_bounds = soft_bounds;
        (mask, bounds)
    }

    /// The coverage a shape contributes, over the pixels it can reach and with
    /// the current clip already multiplied in.
    ///
    /// Every paint and every clip goes through here, and this is the only
    /// place that decides how large a mask is. A comma used to allocate and
    /// scan a full page: eight megabytes at 300 dpi, per glyph, twice for
    /// stroked text. It now costs its own bounding box. A path is filled; a
    /// stroke's outline, which the pen wrote into the scratch as edges, is
    /// swept as it stands, over the region its path would have had.
    fn coverage(&mut self, shape: Shape<'_>, stop: Option<&dyn Fn() -> bool>) -> Mask {
        let (x0, y0, width, height) = paint_region(
            self.bounds_of(shape),
            self.clip.as_ref(),
            self.soft.as_ref(),
            self.device(),
        );
        #[cfg(test)]
        {
            self.mask_pixels = self
                .mask_pixels
                .saturating_add(u64::from(width) * u64::from(height));
        }
        let level = self.level();
        let mut mask = match (shape, self.hard_rule(level)) {
            (Shape::Fill(path, rule), None) => fill_with(
                &mut self.scratch,
                path,
                rule,
                x0,
                y0,
                width,
                height,
                self.tolerance,
                stop,
            ),
            (Shape::Fill(path, rule), Some(edge)) => fill_hard_with(
                &mut self.scratch,
                path,
                rule,
                x0,
                y0,
                width,
                height,
                self.tolerance,
                stop,
                edge,
            ),
            (Shape::Stroke, None) => fill_outline(&mut self.scratch, x0, y0, width, height, stop),
            (Shape::Stroke, Some(edge)) => {
                fill_outline_hard(&mut self.scratch, x0, y0, width, height, stop, edge)
            }
        };
        // Before the clip, so the clip multiplies into a shape at its level
        // and the product of two hard masks stays hard: 8 leaves the
        // coverage, 1 to 7 quantise it, and 0 hardens it, which a mask the
        // filler made whole or empty already is. See `with_levels`.
        mask.quantise(level);
        // 8.5.4: the clip multiplies rather than replaces, so an anti-aliased
        // edge clipped by another stays soft on both. In place, because the
        // region above is already no larger than the clip: the destination
        // `intersect` would allocate is the mask that is already here.
        if let Some(clip) = &self.clip {
            mask.intersect_in_place(clip);
        }
        // 11.6.5: the soft mask multiplies exactly as the clip does -- it *is*
        // a clip with more than two levels -- so it goes through the same
        // product rather than through a second mechanism beside it. The
        // region above already excludes everything outside its rectangle,
        // where it reads zero.
        if let Some(soft) = &self.soft {
            mask.intersect_in_place(soft);
        }
        mask
    }

    /// Rasterizes a path and composites it, unless the render has been
    /// stopped.
    ///
    /// The check is here rather than only in the callers, and the difference
    /// is not pedantry: `show_glyph` checks once and then fills *and* strokes,
    /// `draw_image` checks before a decode that can take longer than the paint
    /// that follows it, and every one of them checks before building a path
    /// whose rasterization is the expensive part. A caller's entry is a
    /// different moment from this one, and this is the moment the work starts.
    fn paint(
        &mut self,
        path: &Path,
        rule: FillRule,
        color: Color,
        alpha: f64,
        mode: RasterBlend,
        set: &DeviceSet,
    ) {
        self.paint_inked(Shape::Fill(path, rule), color, None, alpha, mode, set);
    }

    /// As [`Renderer::paint`], for any shape, with the colour's DeviceCMYK
    /// components beside it (`GraphicsState::fill_ink`): a canvas compositing
    /// in ink — the page asked for in ink, or a `/DeviceCMYK` group's buffer —
    /// composites the document's own components rather than the light turned
    /// back into ink, which would lose every separation the file chose but
    /// one.
    ///
    /// `set` is what the paint takes from the device-dependent state: the
    /// plane entry its shape records (11.7.5.2), and the ink tables a light
    /// colour of DeviceRGB's family becomes ink by on an ink buffer
    /// (11.7.5.3).
    fn paint_inked(
        &mut self,
        shape: Shape<'_>,
        color: Color,
        ink: Option<[u8; 4]>,
        alpha: f64,
        mode: RasterBlend,
        set: &DeviceSet,
    ) {
        if self.stopping() {
            return;
        }
        let stop = self.stop_predicate();
        let mask = self.coverage(shape, Some(&stop));
        self.record_shape(shape, &mask, set.entry);
        if self.in_knockout() {
            self.knockout_restore(&mask);
        }
        let held = self.hold_removal(&set.removal);
        self.canvas.fill_mask_inked(&mask, color, ink, alpha, mode);
        self.release_removal(held);
    }

    /// Composites a glyph's mask, rasterised in its own frame
    /// (`glyph::rasterise`), with its pixel (0, 0) at the page-frame pixel
    /// `origin` — through the clip, the soft mask and a knockout group's
    /// restore, as [`Renderer::paint_inked`] composites a path's coverage,
    /// and taking `set`, the paint's device-dependent state, as that does:
    /// the glyph's shape for the plane is its placed mask times the clip,
    /// without the soft mask, which is opacity (11.6.4.3).
    #[allow(clippy::too_many_arguments)]
    fn paint_glyph_mask(
        &mut self,
        local: &Mask,
        origin: [i32; 2],
        color: Color,
        ink: Option<[u8; 4]>,
        alpha: f64,
        mode: RasterBlend,
        set: &DeviceSet,
    ) {
        let mask = self.placed_glyph(local, origin, true);
        if self.records(set.entry) {
            if self.soft.is_some() {
                let shape = self.placed_glyph(local, origin, false);
                self.record(&shape, set.entry);
            } else {
                self.record(&mask, set.entry);
            }
        }
        if self.in_knockout() {
            self.knockout_restore(&mask);
        }
        let held = self.hold_removal(&set.removal);
        self.canvas.fill_mask_inked(&mask, color, ink, alpha, mode);
        self.release_removal(held);
    }

    /// A glyph's mask placed with its pixel (0, 0) at the page-frame pixel
    /// `origin`, multiplied by the clip and, with `soft`, by the soft mask:
    /// the coverage the glyph composites through, or without the soft mask
    /// its shape.
    ///
    /// The mask is placed by an integer offset, which cannot round, and is
    /// windowed to the pixels the canvas, the clip and the soft mask can show
    /// before either multiplies into it: the region `coverage` would have
    /// asked `fill` for. So a tile composites exactly the page's pixels under
    /// it (ruling 5), and the clip multiplies into a mask already hardened
    /// when anti-aliasing is off, as it does on the path route.
    fn placed_glyph(&self, local: &Mask, origin: [i32; 2], soft: bool) -> Mask {
        let soft = if soft { self.soft.as_ref() } else { None };
        let left = i64::from(origin[0]) + i64::from(local.x0);
        let top = i64::from(origin[1]) + i64::from(local.y0);
        let placed = Bounds {
            left,
            top,
            right: left + i64::from(local.width),
            bottom: top + i64::from(local.height),
        };
        let mut region = placed.meet(Bounds::of_rect(self.device()));
        for limit in [self.clip.as_ref(), soft]
            .into_iter()
            .flatten()
            .filter_map(Bounds::limit_of)
        {
            region = region.meet(limit);
        }
        let (x0, y0, width, height) = region.rect();
        let mut mask = Mask::empty(x0, y0, width, height);
        if width > 0 {
            // `region` lies inside `placed`, so every row and column below is
            // one of the local mask's.
            let column = usize::try_from(i64::from(x0) - left).unwrap_or(0);
            let stride = local.width as usize;
            let span = width as usize;
            for row in 0..height as usize {
                let Ok(source) = usize::try_from(i64::from(y0) + row as i64 - top) else {
                    continue;
                };
                let from = source * stride + column;
                let (Some(source), Some(target)) = (
                    local.data.get(from..from + span),
                    mask.data.get_mut(row * span..(row + 1) * span),
                ) else {
                    continue;
                };
                target.copy_from_slice(source);
            }
        }
        if let Some(clip) = &self.clip {
            mask.intersect_in_place(clip);
        }
        if let Some(soft) = soft {
            mask.intersect_in_place(soft);
        }
        mask
    }

    /// The question the rasterizer asks while it is working.
    ///
    /// Both handles are **cloned** rather than borrowed, because every
    /// rasterizing call takes `&mut self` and a closure borrowing them could
    /// not survive it. What crosses the crate boundary is a bare
    /// `&dyn Fn() -> bool`: ruling 8 keeps `tinker-pdf-raster` free of PDF and
    /// of this crate alike, so it may not name `CancelToken`, and this is the
    /// same seam `ImageDraw::stop` has used since gap 12 rather than a second
    /// one invented beside it.
    ///
    /// It sets the same flag [`Renderer::stopping`] does, and it has to: a
    /// fill that stopped halfway down a shape skipped work as surely as one
    /// that never started, and it is the only kind of skipping the layer above
    /// cannot see for itself.
    fn stop_predicate(&self) -> impl Fn() -> bool {
        let cancel = self.cancel.clone();
        let skipped = Arc::clone(&self.skipped);
        move || {
            if cancel.is_cancelled() {
                skipped.store(true, Ordering::Relaxed);
                return true;
            }
            false
        }
    }

    /// Draws a decoded image into the unit square of the current transform.
    ///
    /// 8.9.5.2: an image occupies the unit square in user space whatever its
    /// pixel dimensions, so the transform carries the placement. Everything
    /// after that placement is the rasterizer's: this maps PDF's vocabulary
    /// onto [`ImageDraw`] and gets out of the way.
    ///
    /// The sampling itself used to be twenty lines of loop here, which is why
    /// it had no filtering — there is nowhere in a bridge crate to put a
    /// pyramid, and nothing here can be tested without building a page.
    fn blit(&mut self, image: &DecodedImage, state: &GraphicsState) {
        let unit_to_device = transform(&state.ctm.then(&self.base));
        // A reduced decode's samples span a square of their own, which reaches
        // past the unit square where the last column and row stand for less
        // than a whole sample; they are drawn over it and cut at the unit
        // square, the image they stand for. Everything else here — the run,
        // the knockout's shape — is the unit square's.
        let samples_to_device = image
            .reduced
            .and_then(|grid| reduced_square(&unit_to_device, image.width, image.height, grid));
        // 11.7.5.2: an image with a soft mask is not fully opaque; a stencil
        // is painted in the fill colour, of the fill's family.
        let family = if image.stencil {
            state.fill_family
        } else {
            image.family
        };
        let set = self.device_set(state, state.fill_alpha, family, !image.soft_masked);
        // The image's shape for the plane, drawn once more through the
        // sampler over nothing: its square, a `/Mask` or a stencil's samples,
        // and the clip — not a soft mask, which is opacity.
        if self.records(set.entry) {
            if let Some((bx, by, bw, bh)) = image_bounds_in(&unit_to_device, self.device()) {
                let mut probe =
                    Canvas::new(bw, bh, PixelFormat::GrayA8, Color::TRANSPARENT).at_origin(bx, by);
                let shape_alpha: &[u8] = if image.soft_masked { &[] } else { &image.alpha };
                // A two-colour image's colours carry its alpha, which is its
                // shape unless it is a soft mask's opacity.
                let shape_two = image.two_colour.as_ref().map(|two| TwoColour {
                    samples: &two.samples,
                    colours: if image.soft_masked {
                        two.colours.map(|[r, g, b, _]| [r, g, b, 255])
                    } else {
                        two.colours
                    },
                });
                let probe_draw = ImageDraw {
                    image: ImageSource {
                        width: image.width,
                        height: image.height,
                        rgb: &image.rgb,
                        alpha: shape_alpha,
                        two_colour: shape_two,
                        // The rectangle the samples hold, as the draw below
                        // reads them.
                        window: image.window,
                    },
                    // Placed as the draw below places it: a reduced decode's
                    // samples over their own square, cut at the unit square.
                    unit_to_device: samples_to_device.unwrap_or(unit_to_device),
                    cut: samples_to_device.map(|_| unit_to_device),
                    interpolate: image.interpolate,
                    alpha: 1.0,
                    blend: RasterBlend::Normal,
                    clip: self.clip.as_ref(),
                    tint: image.stencil.then_some(Color::WHITE),
                    ink: None,
                    stop: None,
                    edge_bits: self.level(),
                    edge_rule: self.hard_edge,
                };
                draw_image(
                    &mut probe,
                    &probe_draw,
                    &mut Pyramid::new(),
                    &mut self.scratch,
                );
                let shape = probe.to_mask((bx, by), MaskKind::Alpha, None);
                self.record(&shape, set.entry);
            }
        }
        // Black generation on an ink buffer takes each sample's light into
        // ink as it is drawn, so such an image composites on its own rather
        // than through a run, which holds no tables.
        let removing = set.removal.is_some() && self.canvas.format == PixelFormat::CmykA8;
        // 8.9.6.2: a stencil selects where the *current fill colour* is
        // painted; it has no colour of its own. That colour is a flat fill
        // like any other, so a canvas compositing in ink is handed its own
        // ink beside its light, as a fill is (`GraphicsState::fill_ink`).
        let tint = image.stencil.then(|| fill_color(state));
        let ink = if image.stencil && self.canvas.format == PixelFormat::CmykA8 {
            state.fill_ink
        } else {
            None
        };
        // 11.4.5: an image is one element, and its shape is the unit square of
        // its transform (8.9.5.2). Rasterised only inside a knockout group,
        // where it is the coverage the restore needs; everywhere else this
        // costs nothing at all.
        if self.in_knockout() {
            let mut quad = Path::new();
            let corners = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
            let to_device = state.ctm.then(&self.base);
            for (i, (x, y)) in corners.iter().enumerate() {
                let (x, y) = to_device.apply(*x, *y);
                if i == 0 {
                    quad.move_to(x, y);
                } else {
                    quad.line_to(x, y);
                }
            }
            quad.close();
            let area = self.coverage(Shape::Fill(&quad, FillRule::NonZero), None);
            self.knockout_restore(&area);
        }
        // Whether this draw joins the run held back so that abutting images do
        // not conflate, or goes straight to the canvas. Decided here, before
        // the clip is borrowed, because ending a run needs `&mut self`.
        let bounds = image_bounds_in(&unit_to_device, self.device());
        // The page frame's pixels, not this canvas's: a tile, or a group's
        // buffer inside one, must take the branch the whole page takes,
        // because a run composites through different rounding from a direct
        // draw.
        let pixels = self.frame_area();
        // 11.4.5 gives every element inside a knockout group its own shape, so
        // a run there would restore the wrong one.
        let direct = self.in_knockout() || pixels > MAX_IMAGE_RUN_PIXELS || removing;
        let masks = self.mask_ids();
        let state_changed = self.run.as_ref().is_some_and(|run| {
            run.alpha.to_bits() != state.fill_alpha.to_bits()
                || run.blend != blend_mode(state.blend)
                || run.masks != masks
                || run.ink != ink
        });
        // The draw's bounds within the frame, which this canvas may cut: what
        // a render of the whole page asks the run about (`RunReach`).
        let framed = image_bounds_in(&unit_to_device, self.frame.rect()).map(Bounds::of_rect);
        if !direct {
            self.reach(framed, state_changed);
        }
        // A different graphics state, or a picture laid *over* another rather
        // than beside it: both end the run. Fragments are added, so an overlap
        // added rather than composited would show both pictures at once.
        let ends = direct || state_changed;
        #[cfg(test)]
        let asked = self.run_pixels;
        let overlaps = !ends && self.run_would_overlap(&unit_to_device);
        #[cfg(test)]
        self.shadow_question(
            &unit_to_device,
            framed,
            ends,
            overlaps,
            self.run_pixels - asked,
        );
        if ends || overlaps {
            self.composite_run();
        }

        // The same predicate the fill and the stroker are given. It was a
        // second hand-rolled closure here until gap 15 gave the other two
        // hooks one, and two spellings of the same question is how one of them
        // ends up not recording that it fired.
        let stop = self.stop_predicate();

        // 11.6.5: the sampler takes one mask, and there may be two. The
        // product is built here rather than passed as a second argument
        // because a soft mask *is* a clip with more than two levels, and the
        // rasterizer has no reason to learn the difference (ruling 8).
        let combined = self.combined_mask();
        let clip = combined
            .as_ref()
            .or(self.clip.as_ref())
            .or(self.soft.as_ref());
        let draw = ImageDraw {
            image: ImageSource {
                width: image.width,
                height: image.height,
                rgb: &image.rgb,
                alpha: &image.alpha,
                // A bilevel image is handed over as it is held, and sampled in
                // one channel (`TwoColour`).
                two_colour: image.two_colour.as_ref().map(|two| TwoColour {
                    samples: &two.samples,
                    colours: two.colours,
                }),
                // What the decode held of it: the sampler works over the whole
                // image and reads the held rectangle's rows.
                window: image.window,
            },
            unit_to_device: samples_to_device.unwrap_or(unit_to_device),
            cut: samples_to_device.map(|_| unit_to_device),
            interpolate: image.interpolate,
            alpha: state.fill_alpha,
            blend: blend_mode(state.blend),
            clip,
            tint,
            ink,
            stop: Some(&stop),
            edge_bits: self.level(),
            edge_rule: self.hard_edge,
        };
        // Built here and dropped here. The levels are the caller's to keep,
        // and keeping them needs an identity for the image rather than a
        // shape — two 512 x 512 photographs are indistinguishable to this
        // function. That identity lives with the resource cache, which is
        // phase 08's, so this deliberately does not invent one.
        let mut pyramid = Pyramid::new();
        if direct {
            let held = removing.then(|| self.canvas.set_ink_tables(set.removal.clone()));
            draw_image(&mut self.canvas, &draw, &mut pyramid, &mut self.scratch);
            if let Some(previous) = held {
                self.canvas.set_ink_tables(previous);
            }
            return;
        }

        let extent = self.device();
        let run = self.run.get_or_insert_with(|| ImageRun {
            fragments: Fragments::new(extent.0, extent.1, extent.2, extent.3),
            alpha: state.fill_alpha,
            blend: blend_mode(state.blend),
            masks,
            ink,
            covered: (i32::MAX, i32::MAX, i32::MIN, i32::MIN),
            framed: Bounds::NONE,
        });
        accumulate_image(
            &mut run.fragments,
            &draw,
            &mut pyramid,
            extent,
            &mut self.scratch,
        );
        if let Some((bx0, by0, bw, bh)) = bounds {
            run.covered = (
                run.covered.0.min(bx0),
                run.covered.1.min(by0),
                run.covered.2.max(bx0.saturating_add(bw as i32)),
                run.covered.3.max(by0.saturating_add(bh as i32)),
            );
        }
        if let Some(framed) = framed {
            run.framed = run.framed.hull(framed);
        }
        #[cfg(test)]
        if let Some(shadows) = self.shadows.as_mut() {
            shadows.join(
                &draw,
                image,
                framed,
                self.frame,
                Bounds::of_rect(extent),
                &mut self.scratch,
            );
        }
    }

    /// Hands a run-membership question, and the canvas's answer to it, to
    /// the shadows a test switched on (`run_shadows`): a draw that ends the
    /// run is no question, and ends every shadow's run too.
    #[cfg(test)]
    fn shadow_question(
        &mut self,
        t: &Transform,
        framed: Option<Bounds>,
        ends: bool,
        overlaps: bool,
        band: u64,
    ) {
        let canvas = Bounds::of_rect(self.device());
        let Some(shadows) = self.shadows.as_mut() else {
            return;
        };
        if ends {
            shadows.end_runs();
        } else {
            shadows.question(t, framed, canvas, overlaps, band, &mut self.scratch);
        }
    }

    /// Records what this canvas cannot see of the run an image could join
    /// ([`RunReach`]), before the run is asked about it.
    ///
    /// `framed` is the image's bounds within the frame. Where the run is not
    /// ended by a change of state, a render of the whole page asks whether the
    /// image overlaps the run over the bounds the two share; this canvas asks
    /// over its own pixels only, so where those bounds reach outside
    /// `region` the two may answer apart, and they are recorded. Within
    /// `region` they cannot: every pixel either would find covered is one
    /// this canvas holds for both. The image is recorded whole too wherever
    /// `region` does not hold it, met a run or not.
    fn reach(&mut self, framed: Option<Bounds>, state_changed: bool) {
        let Some(framed) = framed else {
            return;
        };
        let region = self.region;
        let record = |slot: &mut Option<Bounds>, bounds: Bounds| {
            if !bounds.is_empty() && !region.holds(bounds) {
                *slot = Some(slot.map_or(bounds, |held| held.hull(bounds)));
            }
        };
        record(&mut self.cut, framed);
        if state_changed {
            return;
        }
        if let Some(run) = &self.run {
            record(&mut self.unseen, framed.meet(run.framed));
        }
    }

    /// Whether a draw would land on coverage a run is already holding.
    ///
    /// Abutment is not overlap. Two strips share one row of pixels, and the
    /// question is asked only of that row — the intersection of the draw's own
    /// rectangle with the rectangle the run has painted — rather than of
    /// either draw whole, so the common case costs a band and not a page.
    fn run_would_overlap(&mut self, t: &Transform) -> bool {
        let Some(run) = &self.run else {
            return false;
        };
        let Some((bx0, by0, bw, bh)) = image_bounds_in(t, self.device()) else {
            return false;
        };
        let (cx0, cy0, cx1, cy1) = run.covered;
        let x0 = bx0.max(cx0);
        let y0 = by0.max(cy0);
        let x1 = bx0.saturating_add(bw as i32).min(cx1);
        let y1 = by0.saturating_add(bh as i32).min(cy1);
        if x1 <= x0 || y1 <= y0 {
            return false;
        }
        #[cfg(test)]
        {
            self.run_pixels = self
                .run_pixels
                .saturating_add(u64::from(x1.abs_diff(x0)) * u64::from(y1.abs_diff(y0)));
        }
        let shape = image_coverage(
            t,
            x0,
            y0,
            (x1 - x0) as u32,
            (y1 - y0) as u32,
            None,
            &mut self.scratch,
        );
        run.fragments.would_overlap(&shape)
    }

    /// Strokes `path`, which is in device space, with a pen whose width, dash
    /// array and phase are in **user space** (8.4.3.2, 8.4.3.6), at least
    /// `floor` device pixels wide — writing the outline into the scratch as
    /// the edges of a fill, and handing back the [`Shape`] that fills it.
    ///
    /// Under a similarity — a uniform scale with any rotation or reflection,
    /// which is nearly every page — a user-space pen is a device-space pen
    /// of the width times the scale, and that is how it is stroked: in device
    /// space, the dashes scaled alike, the width no thinner than `floor`.
    /// Under any other map no one device width is the pen: `scale(1, 3)`
    /// makes a pen two units wide six device units across the stretch and two
    /// along it, and cuts a dash square in user space and sheared on the
    /// device. So the path is taken back to user space, stroked there, and
    /// the outline carried out again (`stroke_mapped_with`), with `floor`
    /// held in device pixels in every direction.
    ///
    /// The outline is never a path: the stroker turns each of its pieces into
    /// edges as it makes them, so the path is flattened once, here, and the
    /// outline not at all, and its mask is the one filling the outline's path
    /// would give, byte for byte (`tinker_pdf_raster::fill_outline`). It stays
    /// in the scratch until the next stroke writes another, and a path filled
    /// through the scratch meanwhile does not touch it.
    ///
    /// Which of the two is [`user_space_pen`]'s decision, shared with the
    /// SVG writer so the two state one pen.
    ///
    /// **Stroke adjustment** (`/SA`, 10.7.5), where the state turns it on:
    /// under a similarity the device width is made whole, the nearest whole
    /// number of pixels and never under one ([`whole_pixel_width`]), and
    /// every point of the path — vertices and control points alike — goes to
    /// the grid that puts the stroke's edges on pixel boundaries
    /// ([`adjusted_to_pixels`]), so a thin line covers whole pixels across its
    /// width at any offset and between its copies. Under any other map no one
    /// device width is the pen: its width stays the user-space pen's, uniform
    /// along the line already, and each axis's points go to the grid of the
    /// pen's extent along it made whole — `width·√(a² + c²)` across `x` and
    /// `width·√(b² + d²)` across `y`. The path is in device pixels of the
    /// page, the frame every canvas stands in, so a tile adjusts the page's
    /// stroke (ruling 5). Off or unset, strokes draw as before.
    fn pen(
        &mut self,
        path: &Path,
        state: &GraphicsState,
        style: StrokeStyle,
        floor: f64,
        stop: &dyn Fn() -> bool,
    ) -> Shape<'static> {
        let m = state.ctm.then(&self.base);
        let adjust = state.stored_parameters().stroke_adjustment == Some(true);
        if user_space_pen(&m).is_some() {
            let map = transform(&m);
            if let Some(back) = pen_inverse(&map) {
                // Each axis's extent is the stroke's as drawn, the pen's
                // raised to the floor the outline is united with.
                let adjusted = adjust.then(|| {
                    let across = |p: f64, q: f64| {
                        whole_pixel_width((style.width * (p * p + q * q).sqrt()).max(floor))
                    };
                    adjusted_to_pixels(path, (across(map.a, map.c), across(map.b, map.d)))
                });
                let user = adjusted.as_ref().unwrap_or(path).mapped(&back);
                stroke_mapped_with(
                    &mut self.scratch,
                    &user,
                    &style,
                    &map,
                    floor,
                    self.tolerance,
                    Some(stop),
                );
                return Shape::Stroke;
            }
        }
        let scale = m.expansion();
        // Adjusted, the width made whole is the one the stroke would draw at,
        // the asked width raised to the thinnest line. Every floor of the
        // table is under a pixel and a half, so there it is the asked width's.
        let (width, adjusted) = if adjust {
            let whole = whole_pixel_width((style.width * scale).max(floor));
            (whole, Some(adjusted_to_pixels(path, (whole, whole))))
        } else {
            ((style.width * scale).max(floor), None)
        };
        let style = StrokeStyle {
            width,
            dashes: style.dashes.iter().map(|d| d * scale).collect(),
            dash_phase: style.dash_phase * scale,
            ..style
        };
        stroke_with(
            &mut self.scratch,
            adjusted.as_ref().unwrap_or(path),
            &style,
            self.tolerance,
            Some(stop),
        );
        Shape::Stroke
    }

    /// Converts interpreter path segments into a rasterizer path.
    ///
    /// The segments arrive already in user space with the content stream's own
    /// transform applied, so only the page-to-device transform remains.
    fn to_path(&self, segments: &[PathSegment]) -> Path {
        let mut path = Path::new();
        let map = |x: f64, y: f64| self.base.apply(x, y);

        for segment in segments {
            match *segment {
                PathSegment::MoveTo { x, y } => {
                    let (x, y) = map(x, y);
                    path.move_to(x, y);
                }
                PathSegment::LineTo { x, y } => {
                    let (x, y) = map(x, y);
                    path.line_to(x, y);
                }
                PathSegment::CurveTo {
                    x1,
                    y1,
                    x2,
                    y2,
                    x3,
                    y3,
                } => {
                    let (x1, y1) = map(x1, y1);
                    let (x2, y2) = map(x2, y2);
                    let (x3, y3) = map(x3, y3);
                    path.curve_to(x1, y1, x2, y2, x3, y3);
                }
                PathSegment::Close => path.close(),
            }
        }
        path
    }
}

/// One pixel of slack on every side of a path's bounding box.
///
/// Not superstition, and not theory either: it was measured. A rasterized
/// edge can deposit coverage in the pixel *outside* the box its own control
/// points describe, because the flattener evaluates a Bézier as a float sum
/// that may round an ulp past the extreme control point, and because the
/// scanline walks a crossing as `(x * 256) as i64` stepped by a slope
/// truncated to 1/256 of a pixel. Both are sub-pixel, and both can cross an
/// integer boundary when the shape's extreme lands exactly on one — which for
/// glyphs and rectangles is the common case, not the odd one.
///
/// Two million random paths over a 20x20 canvas, compared pixel for pixel
/// against the full-canvas mask: an exact `floor`/`ceil` box lost 24 pixels,
/// the worst of them 16 levels out of 255. With one pixel of slack, none.
/// That is the whole justification — a perimeter's worth of work for the
/// invariant that bounding changes no pixel.
const SLACK: i64 = 1;

/// A rectangle of device pixels in the page's frame, `[left, right)` by
/// `[top, bottom)`.
///
/// **What makes a tile's decisions the page's** (ruling 5). Every mask this
/// crate draws is only as large as the canvas it was drawn on, so a clip in a
/// tile knows nothing of the page past the tile's edge — and three things are
/// decided by a rectangle rather than pixel by pixel: which cells of a tiling
/// lattice are composited, which pixels a mesh spreads its colour from, and
/// whether a lattice or a mesh is past its budget. Decided from the masks,
/// each came out differently in a tile than in the page under it. So the
/// renderer keeps, beside each mask, the rectangle that mask *would* have had
/// on a render of the whole page, computed from nothing but the path in device
/// pixels and the rectangles before it — numbers that are the same in every
/// frame, because the frame is one frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Bounds {
    left: i64,
    top: i64,
    right: i64,
    bottom: i64,
}

impl Bounds {
    /// No pixels at all.
    const NONE: Bounds = Bounds {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };

    /// The smallest rectangle holding both, an empty one holding nothing.
    fn hull(self, other: Bounds) -> Bounds {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        Bounds {
            left: self.left.min(other.left),
            top: self.top.min(other.top),
            right: self.right.max(other.right),
            bottom: self.bottom.max(other.bottom),
        }
    }

    /// Whether every pixel of `inner` is one of this rectangle's.
    fn holds(self, inner: Bounds) -> bool {
        inner.is_empty()
            || (self.left <= inner.left
                && self.top <= inner.top
                && inner.right <= self.right
                && inner.bottom <= self.bottom)
    }

    /// A canvas's `(x, y, width, height)`.
    fn of_rect(rect: (i32, i32, u32, u32)) -> Bounds {
        let (x, y, width, height) = rect;
        Bounds {
            left: i64::from(x),
            top: i64::from(y),
            right: i64::from(x) + i64::from(width),
            bottom: i64::from(y) + i64::from(height),
        }
    }

    /// A mask's own rectangle.
    fn of_mask(mask: &Mask) -> Bounds {
        Bounds::of_rect((mask.x0, mask.y0, mask.width, mask.height))
    }

    /// The rectangle a mask confines a paint to: its own, or none for a mask
    /// that reads a value other than zero outside itself — a soft mask whose
    /// `/BC` is pale, which leaves every pixel of the canvas some coverage
    /// (11.6.5.2).
    fn limit_of(mask: &Mask) -> Option<Bounds> {
        (mask.outside == 0).then(|| Bounds::of_mask(mask))
    }

    /// Where the two overlap: empty, never negative, when they do not.
    fn meet(self, other: Bounds) -> Bounds {
        let left = self.left.max(other.left);
        let top = self.top.max(other.top);
        Bounds {
            left,
            top,
            right: self.right.min(other.right).max(left),
            bottom: self.bottom.min(other.bottom).max(top),
        }
    }

    /// This rectangle clamped into `outer`, corner by corner: the same
    /// arithmetic as [`Mask::overlap_at`], so a rectangle that misses `outer`
    /// comes back empty at the edge it missed by, exactly where a canvas's
    /// overlap with it would have been placed.
    fn within(self, outer: Bounds) -> Bounds {
        let span = |v: i64, lo: i64, hi: i64| v.clamp(lo, hi.max(lo));
        let left = span(self.left, outer.left, outer.right);
        let top = span(self.top, outer.top, outer.bottom);
        Bounds {
            left,
            top,
            right: span(self.right, outer.left, outer.right).max(left),
            bottom: span(self.bottom, outer.top, outer.bottom).max(top),
        }
    }

    fn is_empty(self) -> bool {
        self.right <= self.left || self.bottom <= self.top
    }

    fn area(self) -> u64 {
        if self.is_empty() {
            return 0;
        }
        (self.right.abs_diff(self.left)).saturating_mul(self.bottom.abs_diff(self.top))
    }

    /// As `(x, y, width, height)`. Every `Bounds` this crate makes lies
    /// inside a canvas or a page, both of which are far inside `i32` and
    /// `u32`; the conversion saturates rather than trusting that.
    fn rect(self) -> (i32, i32, u32, u32) {
        let at = |v: i64| v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
        let span = |lo: i64, hi: i64| hi.saturating_sub(lo).clamp(0, i64::from(u32::MAX)) as u32;
        (
            at(self.left),
            at(self.top),
            span(self.left, self.right),
            span(self.top, self.bottom),
        )
    }
}

/// The device pixels a shape can put ink on: its bounding box, `bounds` as
/// [`Path::bounds`] gives it, widened by [`SLACK`], clipped to `within` and
/// to every rectangle in `limits`.
///
/// The one computation behind both [`paint_region`], which clips to masks and
/// a canvas, and the renderer's page-frame rectangles, which clip to the
/// rectangles those masks would have had on the whole page. One function, so
/// the two cannot drift: a page-frame rectangle is the region a render of the
/// whole page computes, by construction rather than by a second spelling.
fn region_within(
    bounds: Option<(f64, f64, f64, f64)>,
    limits: &[Option<Bounds>],
    within: Bounds,
) -> Bounds {
    let Some((x0, y0, x1, y1)) = bounds else {
        return Bounds {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
    };

    // `as` saturates at the integer bounds for floats, and `Path` refuses
    // non-finite points at construction — but a *finite* 1e300 saturates to
    // `i64::MAX`, and the slack then has to be added saturating or a form
    // XObject with an absurd `/BBox` panics where it used to render nothing
    // (ruling 1).
    let (mut left, mut top) = (
        (x0.floor() as i64).saturating_sub(SLACK),
        (y0.floor() as i64).saturating_sub(SLACK),
    );
    let (mut right, mut bottom) = (
        (x1.ceil() as i64).saturating_add(SLACK),
        (y1.ceil() as i64).saturating_add(SLACK),
    );

    // Every limit bounds the same way and for the same reason: coverage
    // multiplies, and each reports zero outside its own rectangle, so nothing
    // out there could ever have been painted.
    for limit in limits.iter().flatten() {
        left = left.max(limit.left);
        top = top.max(limit.top);
        right = right.min(limit.right);
        bottom = bottom.min(limit.bottom);
    }

    let left = left.clamp(within.left, within.right.max(within.left));
    let top = top.clamp(within.top, within.bottom.max(within.top));
    let right = right.clamp(left, within.right.max(left));
    let bottom = bottom.clamp(top, within.bottom.max(top));
    Bounds {
        left,
        top,
        right,
        bottom,
    }
}

/// The device pixels a shape can put ink on: its bounding box, `bounds` as
/// [`Path::bounds`] gives it, widened by [`SLACK`], clipped to the canvas and
/// to the rectangles the clip and the soft mask confine a paint to
/// ([`Bounds::limit_of`]).
///
/// Clipping to the clip's rectangle is safe for the same reason the whole
/// change is: a clip reports zero coverage outside itself, and coverage
/// multiplies, so nothing outside it could ever have been painted. A soft
/// mask that reads a value outside itself confines nothing.
///
/// A shape with no points at all — which is what a text object that selected
/// a clipping mode and drew nothing produces — yields an empty region, and an
/// empty mask reads as zero everywhere. That is the same "clip everything
/// away" a page-sized mask of zeroes gave, at none of the cost.
fn paint_region(
    bounds: Option<(f64, f64, f64, f64)>,
    clip: Option<&Mask>,
    soft: Option<&Mask>,
    canvas: (i32, i32, u32, u32),
) -> (i32, i32, u32, u32) {
    region_within(
        bounds,
        &[
            clip.and_then(Bounds::limit_of),
            soft.and_then(Bounds::limit_of),
        ],
        Bounds::of_rect(canvas),
    )
    .rect()
}

/// What a paint covers: a path filled under a rule, or a stroke's outline.
///
/// A stroke's outline is never a path. [`Renderer::pen`] writes it into the
/// scratch as the edges of a fill, piece by piece as the stroker makes them,
/// and [`Shape::Stroke`] names it there: its bounds are the ones its path
/// would have had (`FillScratch::outline_bounds`) and its coverage the mask
/// filling that path would give (`tinker_pdf_raster::fill_outline`), so every
/// route a path takes — a solid paint, a shading or tiling pattern, the
/// page-frame rectangle a lattice or a mesh is decided on — a stroke takes
/// with the same numbers.
#[derive(Clone, Copy)]
enum Shape<'a> {
    /// A path, filled under a rule.
    Fill(&'a Path, FillRule),
    /// The outline the last [`Renderer::pen`] wrote into the scratch, filled
    /// under the non-zero rule.
    Stroke,
}

/// Which lattice indices of a tiling pattern reach `[lo, hi]` (8.7.3.2).
///
/// Cell `n` covers `[cell_lo + n*step, cell_hi + n*step]`, so it meets the
/// range exactly when `cell_lo + n*step <= hi` and `cell_hi + n*step >= lo`.
/// The two bounds each pair the *opposite* edges, and swapping them is the
/// off-by-one that drops a row of cells down one side of every shape — which
/// looks like a margin rather than like a missing cell.
///
/// `None` when the arithmetic is not finite. An empty range comes back as
/// `Some` with `hi < lo` never happening: the caller gets `n1 < n0` only when
/// the region falls in a gap between cells, which is legal and paints nothing.
fn lattice_range(lo: f64, hi: f64, cell_lo: f64, cell_hi: f64, step: f64) -> Option<(i64, i64)> {
    let first = ((lo - cell_hi) / step).ceil();
    let last = ((hi - cell_lo) / step).floor();
    if !first.is_finite() || !last.is_finite() {
        return None;
    }
    // Clamped rather than converted: `as` saturates, but a range of
    // `i64::MIN..=i64::MAX` is a loop nobody survives, and the count below has
    // to be able to overflow into the cap rather than wrap.
    const LIMIT: f64 = 1e15;
    let first = first.clamp(-LIMIT, LIMIT) as i64;
    let last = last.clamp(-LIMIT, LIMIT) as i64;
    Some((first, last.max(first - 1)))
}

fn fill_color(state: &GraphicsState) -> Color {
    Color::rgb(state.fill_color.r, state.fill_color.g, state.fill_color.b)
}

fn stroke_color(state: &GraphicsState) -> Color {
    Color::rgb(
        state.stroke_color.r,
        state.stroke_color.g,
        state.stroke_color.b,
    )
}

/// The pen `state` strokes a path with (8.4.3): the line width, cap, join,
/// miter limit and dashes in force, all in user space.
fn stroke_style(state: &GraphicsState) -> StrokeStyle {
    StrokeStyle {
        width: state.line_width,
        cap: match state.line_cap {
            ContentCap::Butt => LineCap::Butt,
            ContentCap::Round => LineCap::Round,
            ContentCap::Square => LineCap::Square,
        },
        join: match state.line_join {
            ContentJoin::Miter => LineJoin::Miter,
            ContentJoin::Round => LineJoin::Round,
            ContentJoin::Bevel => LineJoin::Bevel,
        },
        miter_limit: state.miter_limit,
        dashes: state.dashes.clone(),
        dash_phase: state.dash_phase,
    }
}

impl<G: GlyphSource> Renderer<'_, G> {
    /// A glyph shown ([`Device::show_glyph`]), with the count of glyphs being
    /// shown already raised.
    fn show_glyph_shown(&mut self, glyph: &Glyph, state: &GraphicsState) {
        self.counts.glyphs_shown = self.counts.glyphs_shown.saturating_add(1);
        // Ends the image run: a run only holds while nothing else draws,
        // which is what keeps its clip, soft mask and alpha constant.
        self.flush_run();
        // Before the clipping mode is recorded, and before the outline is
        // looked for. A glyph in a hidden layer must not add to the text
        // clip -- 9.3.6's modes 4 to 7 would otherwise let an invisible
        // layer clip away the visible ones -- and must not be counted as an
        // unreadable font, which is a warning about a page that is missing
        // something rather than about one that is doing as it was told.
        if self.skips_paint() {
            return;
        }
        let painting = self.painting(state);
        let state: &GraphicsState = &painting;
        // 9.3.6: mode 3 paints nothing, which is exactly what a scanned page's
        // invisible OCR layer relies on. It is still extracted, just not drawn.
        //
        // Mode 7 is clip-only and paints nothing either, but it does add to the
        // clip, so it cannot return here — the accumulation happens below.
        let mode = state.text.render_mode;
        if matches!(mode, tinker_pdf_content::TextRenderMode::Invisible) {
            return;
        }
        if !mode.paints() && !mode.clips() {
            return;
        }
        // Recorded before anything can fail. Every early return below — a
        // font whose program will not parse, a glyph with no outline — used
        // to leave the clip unrecorded, so a text object that asked to clip
        // and could not resolve one glyph clipped *nothing* and the rest of
        // the page painted at full strength. With no bundled faces, that is
        // the default configuration rather than an edge case.
        if mode.clips() {
            self.text_clip_requested = true;
        }

        let Some(outline) = self.glyphs.outline(glyph.font_id, glyph.code) else {
            if !glyph.text.trim().is_empty() {
                self.missing_fonts = self.missing_fonts.saturating_add(1);
            }
            return;
        };
        if outline.is_empty() {
            return; // A space, legitimately.
        }

        // 10.7.2: a glyph's outline, filled, stroked or clipped by, is
        // flattened at the flatness in force when it is shown.
        self.tolerance = flatness_tolerance(state);

        // The glyph's transform maps its em square into user space; the base
        // transform takes that to pixels.
        let combined = glyph.transform.then(&self.base);
        if !combined.is_finite() {
            return;
        }
        // From here the glyph is painted, by its mask or by its path, unless
        // its mode only clips.
        if mode.paints() {
            tally(&mut self.counts.drawn_glyphs);
        }

        // 9.3.6 mode 0 in a solid colour: drawn from a mask rasterised in the
        // glyph's own frame and placed at the whole pixel its origin falls in
        // (`glyph`), kept for the rest of the render under the key it is a
        // function of (`glyph_cache`) — or, where the source keeps masks
        // beyond the render (`GlyphSource::kept_mask`), kept by the source and
        // not here. Strokes, pattern fills and the clipping modes keep the
        // path route below, and so does a glyph whose mask would be too large
        // to be worth one or will not place.
        if matches!(mode, tinker_pdf_content::TextRenderMode::Fill) && state.fill_pattern.is_none()
        {
            if let Some(placement) = glyph::Placement::of(&combined) {
                if self.stopping() {
                    return;
                }
                let shared = self.mask_key(glyph, &placement);
                let asked = match self.glyph_key(glyph, &placement) {
                    None => Err(MaskMiss::Unkept),
                    Some(key) => match self.glyphs.kept_mask(&shared) {
                        Some(Ok(kept)) => Ok(kept),
                        Some(Err(request)) => Err(MaskMiss::Source(request)),
                        None => match self.glyph_cache.get(&key) {
                            Ok(kept) => Ok(kept),
                            Err(request) => Err(MaskMiss::Own(key, request)),
                        },
                    },
                };
                let local = match asked {
                    Ok(kept) => {
                        self.counts.glyph_mask_hits = self.counts.glyph_mask_hits.saturating_add(1);
                        Some(kept)
                    }
                    Err(miss) => {
                        let stop = self.stop_predicate();
                        let made = glyph::rasterise(
                            &outline,
                            &placement,
                            self.tolerance,
                            self.level(),
                            self.mask_rule(),
                            Some(&stop),
                        )
                        .map(Arc::new);
                        if let Some(made) = &made {
                            #[cfg(test)]
                            {
                                self.mask_pixels = self
                                    .mask_pixels
                                    .saturating_add(u64::from(made.width) * u64::from(made.height));
                            }
                            self.counts.outlines_filled =
                                self.counts.outlines_filled.saturating_add(1);
                            self.counts.glyph_masks_rasterised =
                                self.counts.glyph_masks_rasterised.saturating_add(1);
                            // A mask a cancelled render stopped short is drawn
                            // and never kept, so a hit is always the mask a
                            // miss would rasterise.
                            if !self.stopping() {
                                match miss {
                                    MaskMiss::Own(key, request) => {
                                        self.glyph_cache.insert(key, Arc::clone(made), request);
                                    }
                                    MaskMiss::Source(request) => {
                                        self.glyphs.keep_mask(&shared, made, request);
                                    }
                                    MaskMiss::Unkept => {}
                                }
                            }
                        }
                        made
                    }
                };
                if let Some(local) = local {
                    let set = self.device_set(state, state.fill_alpha, state.fill_family, true);
                    self.paint_glyph_mask(
                        &local,
                        placement.origin,
                        fill_color(state),
                        state.fill_ink,
                        state.fill_alpha,
                        blend_mode(state.blend),
                        &set,
                    );
                    return;
                }
            }
        }

        let mut path = Path::new();
        for segment in &outline.segments {
            use tinker_pdf_font::Segment;
            match *segment {
                Segment::MoveTo { x, y } => {
                    let (x, y) = combined.apply(x, y);
                    path.move_to(x, y);
                }
                Segment::LineTo { x, y } => {
                    let (x, y) = combined.apply(x, y);
                    path.line_to(x, y);
                }
                Segment::QuadTo { cx, cy, x, y } => {
                    // Straight through, as the curve the font wrote. Raising
                    // it to a cubic here needed the current point, and the
                    // path is the wrong place to ask: after a `Close` the last
                    // verb carries no point, and the pen is back at the start
                    // of the subpath rather than anywhere the verbs say.
                    let (cx, cy) = combined.apply(cx, cy);
                    let (x, y) = combined.apply(x, y);
                    path.quad_to(cx, cy, x, y);
                }
                Segment::CurveTo {
                    c1x,
                    c1y,
                    c2x,
                    c2y,
                    x,
                    y,
                } => {
                    let (c1x, c1y) = combined.apply(c1x, c1y);
                    let (c2x, c2y) = combined.apply(c2x, c2y);
                    let (x, y) = combined.apply(x, y);
                    path.curve_to(c1x, c1y, c2x, c2y, x, y);
                }
                Segment::Close => path.close(),
            }
        }
        if mode.paints() {
            self.counts.outlines_filled = self.counts.outlines_filled.saturating_add(1);
        }

        // 9.3.6, Table 106: the mode decides which of fill, stroke and clip
        // happen, and they are independent. Filling regardless — which is what
        // this did — paints stroke-only text solid, in the wrong colour.
        // A glyph is a shape like any other, so 8.7.3 applies to it unchanged:
        // a pattern selected as the text colour paints the glyph. Both halves
        // are routed, because a mode 2 glyph fills *and* strokes and a
        // half-routed one would paint a black body under a patterned edge —
        // which reads as a rendering bug rather than as a missing capability.
        if mode.fills() {
            if let Some(name) = state.fill_pattern.clone() {
                self.fill_with_pattern(
                    Shape::Fill(&path, FillRule::NonZero),
                    &name,
                    state,
                    state.fill_alpha,
                    fill_color(state),
                    state.fill_family,
                );
            } else {
                let set = self.device_set(state, state.fill_alpha, state.fill_family, true);
                self.paint_inked(
                    Shape::Fill(&path, FillRule::NonZero),
                    fill_color(state),
                    state.fill_ink,
                    state.fill_alpha,
                    blend_mode(state.blend),
                    &set,
                );
            }
        }
        if mode.strokes() {
            // 9.3.6 strokes a glyph as a path, so its pen is 8.4.3.2's: the
            // line width in user space — the CTM's, not the text matrix's.
            let style = StrokeStyle {
                width: state.line_width,
                ..StrokeStyle::default()
            };
            let stop = self.stop_predicate();
            let outlined = self.pen(&path, state, style, self.thinnest(true), &stop);
            if let Some(name) = state.stroke_pattern.clone() {
                self.fill_with_pattern(
                    outlined,
                    &name,
                    state,
                    state.stroke_alpha,
                    stroke_color(state),
                    state.stroke_family,
                );
            } else {
                let set = self.device_set(state, state.stroke_alpha, state.stroke_family, true);
                self.paint_inked(
                    outlined,
                    stroke_color(state),
                    state.stroke_ink,
                    state.stroke_alpha,
                    blend_mode(state.blend),
                    &set,
                );
            }
        }
        if mode.clips() {
            // 9.3.6: the glyphs of a text object accumulate into one clip that
            // takes effect at `ET`, not glyph by glyph — clipping immediately
            // would leave each glyph clipped to itself, which is empty.
            //
            // Each at its own flatness: while every glyph shares one, the
            // outlines are kept as curves and flattened together at `ET`,
            // exactly as one fill; once two differ, what is held is
            // flattened at its own and every glyph after at its own.
            let tolerance = self.tolerance;
            let shared = self
                .text_clip_tolerance
                .is_none_or(|held| held.to_bits() == tolerance.to_bits());
            if shared && !self.text_clip_flat {
                self.text_clip_tolerance = Some(tolerance);
                self.text_clip.get_or_insert_with(Path::new).extend(&path);
            } else {
                if !self.text_clip_flat {
                    if let (Some(held), Some(at)) =
                        (self.text_clip.take(), self.text_clip_tolerance.take())
                    {
                        self.text_clip = Some(polyline(&held, at));
                    }
                    self.text_clip_flat = true;
                }
                self.text_clip
                    .get_or_insert_with(Path::new)
                    .extend(&polyline(&path, tolerance));
            }
        }
    }
}

impl<G: GlyphSource> Device for Renderer<'_, G> {
    fn is_cancelled(&self) -> bool {
        self.stopping()
    }

    /// The admission's answers ([`Admission::answers_alike`]), which are
    /// every answer this renderer gives that does not depend on cancellation;
    /// and a renderer cancelled declines everything, so it is handed no
    /// replay.
    fn answers_alike(&self, events: &[tinker_pdf_content::record::Event]) -> bool {
        !self.cancel.is_cancelled() && self.admission.answers_alike(events)
    }

    fn fill_path(&mut self, path: &[PathSegment], state: &GraphicsState, even_odd: bool) {
        // Ends the image run: a run only holds while nothing else draws,
        // which is what keeps its clip, soft mask and alpha constant.
        self.flush_run();
        if self.skips_paint() {
            return;
        }
        let built = self.to_path(path);
        if built.is_empty() {
            return;
        }
        tally(&mut self.counts.drawn_fills);
        self.tolerance = flatness_tolerance(state);
        let rule = if even_odd {
            FillRule::EvenOdd
        } else {
            FillRule::NonZero
        };
        let painting = self.painting(state);
        let state: &GraphicsState = &painting;

        // Forced colours' fill-to-stroke: the path's stroke at the thinnest
        // width, solid, through the same pen `0 w [] 0 d S` takes, so a fill
        // so drawn is that stroke byte for byte. A fill inside a text object
        // is a Type 3 glyph's, and a glyph stays filled.
        if self.forced.is_some_and(|forced| forced.fill_to_stroke) && self.text_depth == 0 {
            let style = StrokeStyle {
                width: 0.0,
                dashes: Vec::new(),
                dash_phase: 0.0,
                ..stroke_style(state)
            };
            let stop = self.stop_predicate();
            let outline = self.pen(&built, state, style, self.thinnest(false), &stop);
            let set = self.device_set(state, state.stroke_alpha, state.stroke_family, true);
            self.paint_inked(
                outline,
                stroke_color(state),
                state.stroke_ink,
                state.stroke_alpha,
                blend_mode(state.blend),
                &set,
            );
            return;
        }

        // 8.7.3: a pattern supplies the paint, so the fill colour is not used
        // at all. Painting it would put `/Pattern`'s nominal black over every
        // gradient in the document.
        if let Some(name) = state.fill_pattern.clone() {
            self.fill_with_pattern(
                Shape::Fill(&built, rule),
                &name,
                state,
                state.fill_alpha,
                fill_color(state),
                state.fill_family,
            );
            return;
        }

        let color = fill_color(state);
        let set = self.device_set(state, state.fill_alpha, state.fill_family, true);
        self.paint_inked(
            Shape::Fill(&built, rule),
            color,
            state.fill_ink,
            state.fill_alpha,
            blend_mode(state.blend),
            &set,
        );
    }

    fn clip_path(&mut self, path: &[PathSegment], state: &GraphicsState, even_odd: bool) {
        // Ends the image run: a run only holds while nothing else draws,
        // which is what keeps its clip, soft mask and alpha constant.
        self.flush_run();
        // A clip is a full fill of its own path plus an intersect, and it had
        // no check at all — so a render cancelled while a clip operator was
        // pending still paid for both. Skipping it cannot leak ink: every
        // paint is gated too, so a clip that is never installed is never the
        // reason something is drawn.
        if self.stopping() {
            return;
        }
        let built = self.to_path(path);
        if built.is_empty() {
            return;
        }
        // 10.7.4: the clip is the pixels a fill of the path would paint, so
        // it is flattened at the flatness a fill there would be.
        self.tolerance = flatness_tolerance(state);
        let rule = if even_odd {
            FillRule::EvenOdd
        } else {
            FillRule::NonZero
        };
        // 8.5.4: the new clip is the intersection with whatever is in force,
        // which is a multiply rather than a replacement — an anti-aliased edge
        // clipped by another must stay soft on both. `coverage` does that, and
        // the region it picks is the two rectangles' overlap, so a clip stack
        // shrinks as it nests instead of carrying a page apiece.
        let stop = self.stop_predicate();
        let (clip, bounds) = self.clip_coverage(&built, rule, Some(&stop));
        self.clip = Some(clip);
        self.clip_bounds = Some(bounds);
        tally(&mut self.counts.drawn_clips);
    }

    fn begin_text(&mut self) {
        self.text_depth = self.text_depth.saturating_add(1);
    }

    fn end_text(&mut self) {
        self.text_depth = self.text_depth.saturating_sub(1);
        // Ends the image run: a run only holds while nothing else draws,
        // which is what keeps its clip, soft mask and alpha constant.
        self.flush_run();
        // Before the accumulated outline is rasterized, for the same reason as
        // `clip_path`: a text clip is a fill over every glyph of the object at
        // once, which is the largest single clip a page can ask for.
        if self.stopping() {
            return;
        }
        // 9.3.6: text clipping modes accumulate every glyph of the text
        // object and intersect the result at `ET`, not glyph by glyph —
        // clipping each glyph as it arrived would leave the next one clipped
        // to the previous, which is empty.
        //
        // A text object that selects a clipping mode and then shows no glyphs
        // at all should clip everything away. It does not here: nothing
        // records the mode unless a glyph reaches the device, so the clip
        // stays absent rather than becoming empty.
        let requested = std::mem::take(&mut self.text_clip_requested);
        let path = match self.text_clip.take() {
            Some(path) => path,
            // 9.3.6: a text object that selected a clipping mode and produced
            // no glyphs clips everything away. An empty path is the honest
            // answer; leaving the clip alone would paint the whole page.
            None if requested => {
                self.warnings.push(RenderWarning::EmptyTextClip);
                Path::new()
            }
            None => return,
        };
        // Each glyph at the flatness it was shown at: the one they share, or
        // already flattened each at its own.
        if let Some(tolerance) = self.text_clip_tolerance.take() {
            self.tolerance = tolerance;
        }
        self.text_clip_flat = false;
        let stop = self.stop_predicate();
        // The outlines the clip modes added are text, at the text level, as
        // the object they were shown in is.
        self.showing = self.showing.saturating_add(1);
        let (clip, bounds) = self.clip_coverage(&path, FillRule::NonZero, Some(&stop));
        self.showing = self.showing.saturating_sub(1);
        self.clip = Some(clip);
        self.clip_bounds = Some(bounds);
        tally(&mut self.counts.drawn_clips);
    }

    fn save_state(&mut self) {
        let held = self.held();
        self.clip_stack.push(held);
        self.text_saves.push(self.text_depth);
    }

    fn restore_state(&mut self) {
        if let Some(at) = self.text_saves.pop() {
            self.text_depth = self.text_depth.min(at);
        }
        // Ruling 8: the soft mask is pixels and lives here rather than in the
        // graphics state, so `Q` has to put it back -- otherwise an `/SMask`
        // set inside `q ... Q` keeps masking everything drawn after the `Q`,
        // which is content quietly missing from the rest of the page.
        if let Some(held) = self.clip_stack.pop() {
            self.restore_held(held);
        }
    }

    fn stroke_path(&mut self, path: &[PathSegment], state: &GraphicsState) {
        // Ends the image run: a run only holds while nothing else draws,
        // which is what keeps its clip, soft mask and alpha constant.
        self.flush_run();
        if self.skips_paint() {
            return;
        }
        let built = self.to_path(path);
        if built.is_empty() {
            return;
        }
        tally(&mut self.counts.drawn_strokes);
        // 10.7.2: the path, and the stroker's round joins and caps, are
        // flattened at the flatness in force.
        self.tolerance = flatness_tolerance(state);
        let painting = self.painting(state);
        let state: &GraphicsState = &painting;

        // 8.4.3.2: the line width is in user space, and so are the dashes —
        // the pen is stated in user units and `pen` carries it to the device.
        // A hairline still has to cover a pixel to be visible.
        let style = stroke_style(state);
        // The dash expansion below is the part of a stroke that can run away:
        // 8.4.3.6 puts no floor on a dash length, so a page-long rule under a
        // fine array expands into a hundred thousand pieces per segment before
        // a single row of it is filled.
        let stop = self.stop_predicate();
        let outline = self.pen(&built, state, style, self.thinnest(false), &stop);

        // 8.7.3: a stroke painted with a pattern takes its paint from the
        // pattern, exactly as a fill does — and a stroke *is* a fill, of the
        // outline just computed. So this needs no pattern machinery of its
        // own; what it needed was to stop asking for `stroke_color`, which for
        // `/Pattern` is the nominal black the colour crate reports because a
        // pattern space has no colour of its own. Every gradient-stroked rule
        // in every document came out solid black, silently.
        //
        // The rule is non-zero and is stated rather than defaulted: a stroked
        // outline overlaps itself at joins, at caps and wherever a path
        // crosses, and under even-odd every one of those overlaps would be
        // punched back out into a hole.
        if let Some(name) = state.stroke_pattern.clone() {
            // The *stroke* slot's colour, and 8.7.3.3 is why that has to be
            // said out loud: an uncoloured pattern paints in the colour its
            // `SCN` operands gave, and this operator's operands went to the
            // stroking slot. Handing the fill slot's colour instead is a
            // defect nothing else in the engine can see -- every "the pattern
            // paints" assertion is satisfied by a cell that paints in *some*
            // colour.
            self.fill_with_pattern(
                outline,
                &name,
                state,
                state.stroke_alpha,
                stroke_color(state),
                state.stroke_family,
            );
            return;
        }

        let color = stroke_color(state);
        let set = self.device_set(state, state.stroke_alpha, state.stroke_family, true);
        self.paint_inked(
            outline,
            color,
            state.stroke_ink,
            state.stroke_alpha,
            blend_mode(state.blend),
            &set,
        );
    }

    fn show_glyph(&mut self, glyph: &Glyph, state: &GraphicsState) {
        // Everything a glyph paints is text, at the text level, however it is
        // painted: from its mask, as a path, stroked or through a pattern.
        self.showing = self.showing.saturating_add(1);
        self.show_glyph_shown(glyph, state);
        self.showing = self.showing.saturating_sub(1);
    }

    fn draw_image(&mut self, image: &ImageRef, state: &GraphicsState) {
        // Before the decode, not after it. An image in a hidden layer is not
        // drawn, so whether this build could have drawn it is not a fact
        // about the page: decoding first would report `UnsupportedImage` for
        // a JPX nobody was going to see, and paint the grey placeholder that
        // goes with it.
        if self.skips_paint() {
            return;
        }
        let painting = self.painting(state);
        let state: &GraphicsState = &painting;

        // The size the page draws it at, in the page frame whatever canvas
        // this is (ruling 5), so a tile, a band and a replay ask alike; the
        // parts of it this canvas reaches and shows; and the intent in force
        // at `Do` or `EI`, which the image's own `/Intent` overrides where it
        // names one (8.6.5.8, Table 89).
        let request = self.image_request(&state.ctm.then(&self.base));
        let intent = state.stored_parameters().rendering_intent;
        let decoded = if image.inline {
            match self
                .glyphs
                .inline_image(&image.inline_dict, &image.inline_data, &request, intent)
            {
                Ok(Some(decoded)) => Ok(decoded),
                Ok(None) => Err("inline".to_string()),
                Err(codec) => Err(codec),
            }
        } else {
            match self.glyphs.image(&image.name, &request, intent) {
                Ok(Some(decoded)) => Ok(decoded),
                Ok(None) => Err(String::from_utf8_lossy(&image.name).into_owned()),
                Err(codec) => Err(codec),
            }
        };

        tally(&mut self.counts.drawn_images);
        match decoded {
            Ok(decoded) => {
                tally(&mut self.counts.decoded_images_drawn);
                let samples = u64::from(decoded.width).saturating_mul(u64::from(decoded.height));
                self.counts.decoded_image_samples =
                    self.counts.decoded_image_samples.saturating_add(samples);
                self.blit(&decoded, state);
            }
            Err(codec) => {
                // Ruling 2: the page renders without it and says so.
                let warning = RenderWarning::UnsupportedImage { codec };
                if !self.warnings.contains(&warning) {
                    self.warnings.push(warning);
                }
                self.draw_image_placeholder(state);
            }
        }
    }

    fn draw_shading(&mut self, name: &[u8], state: &GraphicsState) {
        // Ends the image run: a run only holds while nothing else draws,
        // which is what keeps its clip, soft mask and alpha constant.
        self.flush_run();
        if self.skips_paint() {
            return;
        }
        let painting = self.painting(state);
        let state: &GraphicsState = &painting;

        let shading = match self.glyphs.shading(name) {
            Ok(Some(shading)) => shading,
            Ok(None) => return,
            Err(kind) => {
                let warning = RenderWarning::UnsupportedShading { kind };
                if !self.warnings.contains(&warning) {
                    self.warnings.push(warning);
                }
                return;
            }
        };

        // 8.7.4.2: `sh` fills the current clip, so a page without one paints
        // everywhere — and the shading's own extent decides the rest.
        let to_device = state.ctm.then(&self.base);

        let alpha = state.fill_alpha.clamp(0.0, 1.0);
        let mode = blend_mode(state.blend);
        // 11.7.5.3: the shading's colours convert in the intent in force at
        // `sh`.
        let conversion = state.stored_parameters().rendering_intent.conversion();
        // 8.7.4.2 again: `sh` paints the current clip, so the clip's own
        // rectangle bounds the sweep. Without one it really is the whole
        // page, which is what the operator means.
        // Owned rather than borrowed, because the knockout restore below
        // needs `self` mutably and this has to outlive it. A clone per `sh`
        // against a per-pixel sweep over the same rectangle is nothing.
        let area: Option<Mask> = match (&self.clip, &self.soft) {
            (Some(clip), Some(soft)) => Some(clip.intersect(soft)),
            (Some(mask), None) | (None, Some(mask)) => Some(mask.clone()),
            (None, None) => None,
        };

        let set = self.device_set(state, alpha, shading_family(&shading), true);
        // `sh`'s shape for the plane is its geometry within the clip, the soft
        // mask aside (11.6.4.2, 11.6.4.3).
        let shape_area = if self.records(set.entry) {
            self.clip.clone()
        } else {
            None
        };

        // A mesh paints its own geometry rather than the whole clip, so it
        // brings its own coverage and its own knockout restore with it.
        if let Some(mesh) = shading.mesh() {
            let limit = self.page_limit();
            if self.paint_mesh(
                mesh,
                to_device,
                area.as_ref(),
                shape_area.as_ref(),
                limit,
                alpha,
                mode,
                &set,
                state.stored_parameters().smoothness,
                conversion,
            ) {
                tally(&mut self.counts.drawn_shadings);
            } else {
                let warning = RenderWarning::UnsupportedShading { kind: mesh.kind };
                if !self.warnings.contains(&warning) {
                    self.warnings.push(warning);
                }
            }
            return;
        }

        let Some(inverse) = invert(&to_device) else {
            return;
        };
        tally(&mut self.counts.drawn_shadings);
        self.record_shading(&shading, &inverse, shape_area.as_ref(), set.entry);
        let (ox, oy, width, height) = self.device();
        let (x_start, y_start, x_end, y_end) =
            area.as_ref().map_or((0, 0, width, height), |mask| {
                mask.overlap_at((ox, oy), width, height)
            });
        // 11.4.5: `sh` paints the whole clip, so the clip *is* this element's
        // coverage — and with no clip in force it really is the whole buffer.
        if self.in_knockout() {
            let restore = area
                .clone()
                .unwrap_or_else(|| Mask::uniform(ox, oy, width, height, 255));
            self.knockout_restore(&restore);
        }
        let mut scratch = ShadingScratch::converting(conversion);
        // P-14.5, decided on the clip's rectangle on the whole page.
        if let Some((space, function)) = shading.parametric() {
            scratch.tabulate(space, function, 0.0, 1.0, self.page_limit().area());
        }
        let held = self.hold_removal(&set.removal);
        for row in y_start..y_end {
            if self.stopping() {
                self.release_removal(held);
                self.count_evaluations(&scratch);
                return;
            }
            // The device pixel, in the page's frame: see
            // `fill_with_shading`.
            let py = oy.saturating_add(row as i32);
            let mut walk = shading.row(&inverse, f64::from(py) + 0.5);
            for col in x_start..x_end {
                let px = ox.saturating_add(col as i32);
                let clip = area.as_ref().map_or(255, |mask| mask.at(px, py));
                if clip == 0 {
                    continue;
                }

                let Some((r, g, b)) = walk.color_at(i64::from(px), &mut scratch) else {
                    continue;
                };
                let effective = alpha * f64::from(clip) / 255.0;
                self.canvas
                    .blend_pixel_with(col, row, Color::rgb(r, g, b), effective, mode);
            }
        }
        self.release_removal(held);
        self.count_evaluations(&scratch);
    }

    /// The property list is ignored here on purpose. 14.7.4.2's `/MCID` ties
    /// content to the structure tree, and a rasterizer paints the same pixels
    /// whether or not a paragraph knows its own number; the text device is
    /// the one that keeps it.
    fn begin_marked_content(
        &mut self,
        _tag: &[u8],
        visible: bool,
        hidden_layer: Option<&str>,
        _props: Option<&tinker_pdf_content::MarkedProps>,
    ) {
        self.admission.begin_marked_content(visible);
        // Ruling 10. Raised once per layer rather than once per scope: a CAD
        // drawing marks every construction line, and a hundred identical
        // warnings say nothing the first one did not.
        if let Some(layer) = hidden_layer {
            let warning = RenderWarning::HiddenOptionalContent {
                layer: layer.to_string(),
            };
            if !self.warnings.contains(&warning) {
                self.warnings.push(warning);
            }
        }
    }

    fn end_marked_content(&mut self) {
        self.admission.end_marked_content();
    }

    fn begin_form(&mut self, _id: u64, name: &[u8]) -> bool {
        let held = self.held();
        self.clip_stack.push(held);
        // 8.10.1: a form's own `/Resources`, if it brought any. The
        // interpreter asks its own seam the same question about the same name,
        // so the two scopes open and close together.
        let nested = self.glyphs.form_scope(name);
        let displaced = nested.map(|scope| {
            let outer = mem::replace(&mut self.glyphs, Scope::Owned(scope));
            (outer, self.next_scope())
        });
        self.form_scopes.push(displaced);
        let entered = !self.stopping();
        self.entered_stream(entered);
        entered
    }

    fn end_form(&mut self, _id: u64) {
        self.left_stream();
        if let Some(Some((outer, serial))) = self.form_scopes.pop() {
            self.glyphs = outer;
            self.scope = serial;
        }
        if let Some(held) = self.clip_stack.pop() {
            self.restore_held(held);
        }
    }

    fn begin_group(&mut self, group: tinker_pdf_content::Group, state: &GraphicsState) -> bool {
        // Ends the image run: a run only holds while nothing else draws,
        // which is what keeps its clip, soft mask and alpha constant.
        self.flush_run();
        // A group inside a layer that is off paints nothing, so there is
        // nothing to buffer. Declining also leaves the interpreter's 11.6.6
        // state reset undone, which is right: nothing is going to paint.
        if self.stopping() {
            return false;
        }
        if self.hidden() {
            tally(&mut self.counts.hidden_calls);
            return false;
        }
        // Under forced colours a group's `ca` and `/BM` at the `Do` are its
        // paint's too, and composite it opaque and `Normal`.
        let painting = self.painting(state);
        let entered = self.open_group(group, &painting);
        if entered {
            tally(&mut self.counts.drawn_groups);
        }
        self.entered_stream(entered);
        entered
    }

    fn end_group(&mut self) {
        self.left_stream();
        // Ends the image run: a run only holds while nothing else draws,
        // which is what keeps its clip, soft mask and alpha constant.
        self.flush_run();
        self.close_group();
    }

    fn begin_soft_mask(
        &mut self,
        mask: &tinker_pdf_content::MaskGroup,
        bbox: &[PathSegment],
        _state: &GraphicsState,
    ) -> bool {
        // Ends the image run: a run only holds while nothing else draws,
        // which is what keeps its clip, soft mask and alpha constant.
        self.flush_run();
        if self.stopping() {
            return false;
        }
        let entered = self.open_soft_mask(mask, bbox);
        if entered {
            tally(&mut self.counts.drawn_soft_masks);
        }
        self.entered_stream(entered);
        entered
    }

    fn end_soft_mask(&mut self) {
        self.left_stream();
        // Ends the image run: a run only holds while nothing else draws,
        // which is what keeps its clip, soft mask and alpha constant.
        self.flush_run();
        self.close_soft_mask();
    }

    fn clear_soft_mask(&mut self) {
        // Ends the image run: a run only holds while nothing else draws,
        // which is what keeps its clip, soft mask and alpha constant.
        self.flush_run();
        self.soft = None;
        self.soft_bounds = None;
    }

    /// The interpreter passed groups over for its budget: the page says so
    /// as [`RenderWarning::GroupBudgetSpent`], as it does for its own.
    fn group_budget_spent(&mut self) {
        self.admission.note_budget_spent();
    }

    /// Said at the end as [`RenderWarning::RecursiveContent`].
    fn recursion_refused(&mut self) {
        self.recursion_refused = true;
    }

    /// Said at the end as [`RenderWarning::NestedBudgetSpent`].
    fn nested_budget_spent(&mut self) {
        self.nested_budget_spent = true;
    }
}

/// Inverts an affine transform, or `None` when it is degenerate.
/// The same six numbers, in the rasterizer's spelling.
///
/// Two types for one matrix because the rasterizer may not name a PDF type
/// (ruling 8) and the interpreter's `Matrix` is one. The conversion is a copy;
/// nothing is recomputed, so a placement is bit-identical either side of it.
fn transform(m: &Matrix) -> Transform {
    Transform {
        a: m.a,
        b: m.b,
        c: m.c,
        d: m.d,
        e: m.e,
        f: m.f,
    }
}

fn invert(m: &Matrix) -> Option<Matrix> {
    let determinant = m.a * m.d - m.b * m.c;
    if !determinant.is_finite() || determinant.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / determinant;
    Some(Matrix {
        a: m.d * inv,
        b: -m.b * inv,
        c: -m.c * inv,
        d: m.a * inv,
        e: (m.c * m.f - m.d * m.e) * inv,
        f: (m.b * m.e - m.a * m.f) * inv,
    })
}

/// The transform from PDF user space to a device of the given size.
///
/// PDF's y axis points up from the bottom-left of the page; a raster's points
/// down from the top-left, so the flip belongs here rather than in every
/// caller.
///
/// Ignores `/Rotate` and any crop-box origin; [`page_view_transform`] is what
/// a page should actually be rendered through. Kept because it is the honest
/// primitive — a raw flip for a box that starts at the origin — and because
/// the tests that pin the flip should not have to state a rotation.
#[must_use]
pub fn page_transform(page_height: f64, scale: f64) -> Matrix {
    Matrix {
        a: scale,
        b: 0.0,
        c: 0.0,
        d: -scale,
        e: 0.0,
        f: page_height * scale,
    }
}

/// Whether the renderer composites the run of images it is holding back when
/// it is handed a call of `kind` (`Renderer::flush_run`): every call that
/// draws anything but an image, a clip, `ET`, and either end of a group or a
/// soft mask, and `/SMask /None`; never `BT`, a `q`, a `Q`, a form's bracket,
/// a marked-content scope, or what the interpreter reports of its budgets. An
/// image draw decides for itself whether it joins the run, and is no end of
/// one here.
///
/// The rule the bands of one page are planned by: a run of image draws is
/// decided by what its canvas holds (ruling 5's named exception), so no band
/// is cut through the rows of one (`docs/design/parallelism.md`, "Bands of one
/// page"). `the_planners_rule_is_the_renderers_flush` holds it to the renderer
/// one kind at a time.
#[must_use]
pub fn ends_image_run(kind: EventKind) -> bool {
    match kind {
        EventKind::ShowGlyph
        | EventKind::EndText
        | EventKind::FillPath
        | EventKind::StrokePath
        | EventKind::ClipPath
        | EventKind::DrawShading
        | EventKind::BeginGroup
        | EventKind::EndGroup
        | EventKind::BeginSoftMask
        | EventKind::EndSoftMask
        | EventKind::ClearSoftMask => true,
        EventKind::BeginText
        | EventKind::SaveState
        | EventKind::RestoreState
        | EventKind::DrawImage
        | EventKind::BeginMarkedContent
        | EventKind::EndMarkedContent
        | EventKind::BeginForm
        | EventKind::EndForm
        | EventKind::GroupBudgetSpent
        | EventKind::RecursionRefused
        | EventKind::NestedBudgetSpent => false,
    }
}

/// The transform that maps a page's visible area onto its canvas.
///
/// Three things the raw flip does not do, each of which silently misplaces
/// content when it is missing:
///
/// - **`/Rotate`** (7.7.3.3) turns the page clockwise when displayed. The
///   canvas is sized from the *rotated* extent, so without the matching
///   rotation here the content is drawn upright inside a sideways canvas and
///   clipped away.
/// - **The crop box's origin.** `/CropBox [20 30 »]` means the visible area
///   starts at (20, 30) in user space and must land at the canvas origin.
///   A box that does not start at (0, 0) otherwise shifts everything.
/// - **The y flip**, as before.
///
/// `crop` is the page's visible box in user space, `rotation` its normalized
/// `/Rotate` (0, 90, 180 or 270).
#[must_use]
pub fn page_view_transform(crop: (f64, f64, f64, f64), rotation: u16, scale: f64) -> Matrix {
    let (x0, y0, x1, y1) = crop;
    let (w, h) = ((x1 - x0).max(0.0), (y1 - y0).max(0.0));

    // Move the visible box's lower-left corner to the origin first; every
    // case below then reasons about a box at (0, 0).
    let to_origin = Matrix::translate(-x0, -y0);

    // 7.7.3.3: `/Rotate` turns the page **clockwise** when it is displayed.
    //
    // Each case below is derived from where the corners land, because the sign
    // is easy to get backwards and looks plausible either way — a page rotated
    // the wrong way is still a rotated page. Turning a sheet clockwise sends
    // its bottom-left corner to the top-left; anticlockwise sends it to the
    // bottom-right. These matrices are stated in user space (y still up), and
    // the y flip happens afterwards in `page_transform`.
    let rotate = match rotation % 360 {
        // Clockwise: (x, y) -> (y, w - x), so (0, 0) ends up at the top-left
        // once the flip is applied.
        90 => Matrix {
            a: 0.0,
            b: -1.0,
            c: 1.0,
            d: 0.0,
            e: 0.0,
            f: w,
        },
        // (x, y) -> (w - x, h - y): the opposite corner.
        180 => Matrix {
            a: -1.0,
            b: 0.0,
            c: 0.0,
            d: -1.0,
            e: w,
            f: h,
        },
        // Anticlockwise: (x, y) -> (h - y, x), bottom-left to bottom-right.
        270 => Matrix {
            a: 0.0,
            b: 1.0,
            c: -1.0,
            d: 0.0,
            e: h,
            f: 0.0,
        },
        _ => Matrix::IDENTITY,
    };

    // After rotating, the extent swaps for the quarter turns — which is the
    // same swap `Page::display_size` makes when it sizes the canvas.
    let rotated_height = if rotation % 180 == 90 { w } else { h };

    to_origin
        .then(&rotate)
        .then(&page_transform(rotated_height, scale))
}

/// A rectangle of a page's **rendered pixels**: the unit the bitmap is in, not
/// the unit the page is in.
///
/// # Why pixels and not points, which is the decision this type exists to state
///
/// A region could as defensibly be a rectangle of PDF points, and both answers
/// are useful: a caller tiling a 4x render thinks in pixels, a caller asking
/// for "the top-left quarter of this page" thinks in points. The two differ,
/// and a bare four-number rectangle that could be either is exactly the API
/// defect this type refuses — the name and the `u32`s say which, at every call
/// site, without a doc comment having to be read.
///
/// Pixels win because of ruling 5. The property the whole feature owes is that
/// **a tile is byte-equal to the corresponding sub-rectangle of the full-page
/// render**, and a sub-rectangle of a bitmap is a rectangle of whole pixels. A
/// region in points has to be multiplied by the scale and rounded before it can
/// become a canvas, and no rounding rule makes an arbitrary point rectangle land
/// on pixel boundaries: `page_pixels` already rounds the page itself *outward*,
/// so a caller who asked for a half-open point range would get a tile that
/// straddles a source pixel and could not be compared to anything exactly. The
/// guard would then need a tolerance, and a tolerance is precisely what ruling 5
/// says this must not have.
///
/// A caller who is thinking in points converts once, with the page's own
/// rounding rule, through `tinker_pdf::Page::pixel_size`. A caller who is
/// thinking in pixels — which is every caller that tiles, which is the use this
/// row exists for — writes what they mean.
///
/// # Where the origin is
///
/// `x` and `y` count from the **top-left of the rendered bitmap, downward**,
/// like the bitmap's own rows and unlike PDF user space, which counts up from
/// the bottom-left. Stated because it is the second thing a region API gets
/// wrong and the picture still looks plausible: a region whose y runs the other
/// way returns a real part of a real page, mirrored about the middle.
///
/// The consequence for the page's own geometry follows from this and is not a
/// separate decision: the bitmap is already `/Rotate`d and already cropped to
/// the crop box, so the region indexes the picture *after* both. Asking for
/// `(0, 0, 32, 32)` of a `/Rotate 90` page gives the top-left 32x32 of the
/// sideways picture a reader sees, never the corner of the upright sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRegion {
    /// Distance from the bitmap's left edge, in pixels.
    pub x: u32,
    /// Distance from the bitmap's top edge, in pixels, counting downward.
    pub y: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

impl PixelRegion {
    /// A region at `(x, y)` of the given size.
    #[must_use]
    pub fn new(x: u32, y: u32, width: u32, height: u32) -> PixelRegion {
        PixelRegion {
            x,
            y,
            width,
            height,
        }
    }

    /// Whether this region asks for no pixels at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// This region **intersected** with a bitmap of `width` by `height`.
    ///
    /// # Why an intersection, and why nothing more forgiving
    ///
    /// Ruling 2 says degrade rather than fail, and a region hanging off the
    /// edge of the page is a degradation waiting to be chosen: clamp it, refuse
    /// it, return nothing, or panic. Panicking is out under ruling 1 — the
    /// region is a caller's number, but a page's pixel size is the *file's*,
    /// and a tiler that is correct for one document must not abort on another.
    ///
    /// Of the three that remain, the intersection is the only one that keeps
    /// ruling 5's guard true. Any clamp that moves the rectangle rather than
    /// trimming it — sliding an off-page tile back onto the page, or growing an
    /// empty one to a single pixel so that the result is always "a picture" —
    /// returns pixels the caller did not ask for, and those pixels are not the
    /// sub-rectangle of the full render that the caller's own coordinates name.
    /// The guard would hold for tiles inside the page and quietly not hold at
    /// the edge, which is the one place a tiler actually needs it.
    ///
    /// So the far edges are trimmed and the near ones are not moved, and a
    /// region that does not meet the page trims to nothing: width or height
    /// zero, which is an honest empty answer rather than an invented pixel.
    /// `tinker_pdf::Page::render` reports the trim as
    /// [`RenderWarning::RegionClamped`] rather than performing it silently
    /// (ruling 10), so "I got the tile I asked for" and "I got what was left of
    /// it" stay distinguishable.
    #[must_use]
    pub fn clamped_to(&self, width: u32, height: u32) -> PixelRegion {
        let x = self.x.min(width);
        let y = self.y.min(height);
        PixelRegion {
            x,
            y,
            width: self.width.min(width - x),
            height: self.height.min(height - y),
        }
    }
}

/// A white canvas holding `region`'s pixels, standing at `region`'s corner in
/// the page's frame, in exactly the format asked for.
///
/// This is ruling 5's mechanism: **a clipped render is the same pipeline over
/// a smaller canvas**, never a second implementation. The page is drawn through
/// [`page_view_transform`] whatever part of it is asked for, and the canvas says
/// which pixels of that picture it holds ([`Canvas::origin`]).
///
/// # Why the canvas moves and the transform does not
///
/// Until September 2026 a region was drawn through the page's transform moved
/// by the region's corner — `page_view_transform(..)` then a translation by
/// `(-x, -y)` in device space. The translation is exact as arithmetic and it
/// changed only `e` and `f`, but a point's device coordinate is `a·x + c·y +
/// e`, and `fl(u + e)` and `fl(u + e − x)` are two roundings at two
/// magnitudes. Where the exact value sat on one of the rasterizer's grids — a
/// sub-scanline, a 1/256 step, a colour step of a shading — the last ulp put a
/// tile and the page under it a level apart: one shading pixel at 3x, four
/// miter corners of a stroked rectangle by fifteen levels at 1x. Keeping one
/// frame and moving the canvas makes every coordinate the same number in both,
/// so the only arithmetic that differs is an integer subtraction when a pixel is
/// written.
///
/// The composition order the old spelling had to get right — the shift after
/// the crop-box origin, `/Rotate` and the y flip, so it subtracts bitmap pixels
/// from bitmap coordinates — is now the canvas's: `region` is a rectangle of the
/// rendered bitmap, and so is the origin.
#[must_use]
pub fn region_canvas_in(region: PixelRegion, format: PixelFormat) -> Canvas {
    region_canvas(region, format, Color::WHITE)
}

/// [`region_canvas_in`] with nothing painted on it: every pixel transparent.
///
/// For a page rendered as a layer rather than onto paper. Only a format with
/// alpha can hold nothing, so the caller asks for this only with one; a format
/// without it starts opaque whatever it is handed, and would come back black.
#[must_use]
pub fn region_canvas_clear(region: PixelRegion, format: PixelFormat) -> Canvas {
    region_canvas(region, format, Color::TRANSPARENT)
}

/// The canvas both of the above make.
fn region_canvas(region: PixelRegion, format: PixelFormat, background: Color) -> Canvas {
    // A region is a rectangle of a page's pixels, and a page is bounded by
    // `MAX_PAGE_PIXELS` on each side long before `i32::MAX`; saturating keeps a
    // caller's absurd corner from wrapping rather than trusting that.
    let at = |value: u32| i32::try_from(value).unwrap_or(i32::MAX);
    Canvas::new(region.width, region.height, format, background)
        .at_origin(at(region.x), at(region.y))
}

/// The largest canvas this will allocate for one page, in pixels.
///
/// About 67 million, which is 201 MB at three bytes a pixel — larger than any
/// legitimate single-page render, and small enough that a hostile file cannot
/// exhaust memory with it. A caller who genuinely wants more renders in tiles,
/// which go through the same path over a smaller canvas (ruling 5).
///
/// The cap exists because the page box is attacker-controlled: `/MediaBox
/// [0 0 1e9 1e9]` is four tokens, and without a ceiling it asks for an
/// allocation no machine can serve. A failed allocation **aborts** the process
/// rather than unwinding, so it cannot be caught and reported afterwards —
/// which makes this the one place it has to be prevented rather than handled.
///
/// | | Pixels, at 150 dpi |
/// | --- | --- |
/// | The most any fixture in this repository spends | 67 108 864 |
/// | A 200-page comic, whose largest page is a 2000 x 3000 scan | 26 043 750 |
/// | A dense 200-page fixed document, US Letter at 612 x 792 pt | 2 103 750 |
/// | A 300-page reflowable book, at `epub::DEFAULT_PAGE`'s 432 x 648 pt | 1 215 000 |
/// | **This cap** | **67 108 864** |
///
/// The fixture figure is the cap itself and is allowed, because this bound
/// **degrades rather than refuses**: `an_absurd_page_box_is_clamped_rather_than_allocated`
/// asks [`page_pixels`] for `1e9 x 1e9` and gets a canvas under the cap, and
/// `an_enormous_page_is_scaled_rather_than_cropped` renders a 20 000-point
/// page at scale 4 and gets [`RenderWarning::PageScaledDown`]. Nothing here
/// ever allocates more than this number, so the most any fixture *spends* is
/// this number.
///
/// **Those three yardsticks are undefined without a resolution, so one is
/// fixed: 150 dpi.** A page's pixel count is its box in points times the
/// square of the caller's scale, which makes this the row in
/// `bounds_ledger.rs` whose columns are a property of the *ask* as much as of
/// the file — and a column that did not name the ask would be three numbers
/// nobody could check.
///
/// 150 rather than either neighbour, and both neighbours are wrong for a
/// stated reason. **72 dpi** is `RenderOptions::default()`, one device pixel
/// per point, and it is the floor; a yardstick that takes the floor measures
/// nothing. **300 dpi** is what two rows of that table already use, and it is
/// right where they use it, because their input is a scan — but
/// `tinker_pdf::cbz::synthesise` makes one image pixel exactly one PDF point,
/// so a 2000 x 3000 comic page is a 2000 x 3000 *point* box, 27.8 by 41.7
/// inches, and asking 300 dpi of it asks to upsample a scan four-fold. That
/// crosses this cap at **241 dpi**, and the crossing is written down here
/// rather than left to be rediscovered: past it the page is scaled to fit and
/// says so, which is a degradation under ruling 2 rather than a refusal, and
/// makes this the one row in that table whose over-yardstick behaviour is a
/// warning instead of a `Refused`.
///
/// **150 dpi** is `tpdf render`'s own default — the resolution a caller who
/// does not choose gets from this engine's front end — and twice the library's
/// floor. All three yardsticks clear the cap at it: by 2.6x, 32x and 55x.
///
/// Reachable: [`page_pixels`] rounds each side outward and clamps each at
/// `u32::MAX`, so four tokens of `/MediaBox` and any finite scale may ask for
/// `u32::MAX` squared — 18 446 744 065 119 617 025 pixels, 275 billion times
/// this cap.
pub const MAX_PAGE_PIXELS: u64 = 1 << 26;

/// The pixel size of a page at a given scale.
///
/// **Rounded outward**, so a page never loses its last row or column to
/// rounding: A4 at 150 dpi is 1240×1755, not 1240×1754. This is an API
/// guarantee, pinned by tests, because the engine being replaced left it as
/// folklore that callers rediscovered.
///
/// A page whose area would exceed [`MAX_PAGE_PIXELS`] is scaled down to fit,
/// keeping its aspect ratio — degraded rather than refused (ruling 2). Pass
/// the result of [`page_scale`] rather than the caller's own scale, or the
/// canvas shrinks and the content does not.
///
/// The scale a page will actually be rendered at.
///
/// [`page_pixels`] clamps an enormous page to a bounded canvas. The canvas was
/// clamped and the *transform* was not, so the content was drawn at full size
/// onto a smaller surface — which crops rather than scales. On real paper:
/// ANSI E at 300 dpi lost three quarters of the sheet, silently.
///
/// Both the canvas and the transform take this, so they cannot disagree.
#[must_use]
pub fn page_scale(width_pt: f64, height_pt: f64, scale: f64) -> f64 {
    if !scale.is_finite() || scale <= 0.0 {
        return 1.0;
    }
    let side = |v: f64| {
        if !v.is_finite() || v <= 0.0 {
            1.0
        } else {
            (v * scale).ceil().max(1.0)
        }
    };
    let area = side(width_pt) * side(height_pt);
    if area <= MAX_PAGE_PIXELS as f64 {
        return scale;
    }
    // `sqrt` is correctly rounded by IEEE 754, so this stays target-stable
    // (ruling 4).
    scale * (MAX_PAGE_PIXELS as f64 / area).sqrt()
}

/// The pixel size of a page at a given scale, rounded outward.
#[must_use]
pub fn page_pixels(width_pt: f64, height_pt: f64, scale: f64) -> (u32, u32) {
    let round_out = |v: f64| {
        if !v.is_finite() || v <= 0.0 {
            return 1u32;
        }
        (v * scale).ceil().max(1.0).min(f64::from(u32::MAX)) as u32
    };
    let (w, h) = (round_out(width_pt), round_out(height_pt));

    let area = u64::from(w) * u64::from(h);
    if area <= MAX_PAGE_PIXELS {
        return (w, h);
    }

    // Both dimensions shrink by the same factor, so the page keeps its shape
    // and stays recognisable rather than becoming a stripe of itself.
    let shrink = (MAX_PAGE_PIXELS as f64 / area as f64).sqrt();
    let clamp = |v: u32| (f64::from(v) * shrink).floor().max(1.0) as u32;
    let (w, h) = (clamp(w), clamp(h));

    // **A side raised back to the one-pixel floor did not shrink by `shrink`,
    // and the product is over the cap by however much it was raised.**
    // `w * shrink` and `h * shrink` multiply to exactly the cap only while
    // both are above 1. A `/MediaBox` of `[0 0 20 9.2e18]` at 12 dpi reaches
    // the two lines above as 1 x 4 294 967 295 — the height having already
    // taken the `u32::MAX` clamp — whose `shrink` is 0.125; the width shrinks
    // to 0.125 and the floor raises it back to 1, a factor of eight the
    // height never gives back, and the pair comes back at 1 x 536 870 911,
    // eight times the ceiling. The `render_page` fuzz target found it as a
    // single 1.6 GB allocation from 755 bytes.
    //
    // Spend what is left on the other side rather than leaving it over. The
    // aspect ratio is already gone at this point — a side is at the floor —
    // so there is nothing left to keep, and the cap is what must hold.
    let area = u64::from(w) * u64::from(h);
    if area <= MAX_PAGE_PIXELS {
        return (w, h);
    }
    let fit =
        |keep: u32| (MAX_PAGE_PIXELS / u64::from(keep).max(1)).clamp(1, u64::from(u32::MAX)) as u32;
    if w >= h {
        (fit(h), h)
    } else {
        (w, fit(w))
    }
}

/// Convenience: a white canvas of the right size for a page.
#[must_use]
pub fn page_canvas(width_pt: f64, height_pt: f64, scale: f64, format: PixelFormat) -> Canvas {
    page_canvas_in(width_pt, height_pt, scale, page_format(format))
}

/// [`page_canvas`], in exactly the format asked for.
///
/// What a page declaring its own transparency group needs (11.4.7): the page
/// composites in the group's space and is converted for the caller afterwards,
/// so this one does *not* coerce.
#[must_use]
pub fn page_canvas_in(width_pt: f64, height_pt: f64, scale: f64, format: PixelFormat) -> Canvas {
    let (w, h) = page_pixels(width_pt, height_pt, scale);
    Canvas::new(w, h, format, Color::WHITE)
}

/// The buffer a transparency group declaring `space` composites in (11.6.6),
/// or `None` for a space this build does not blend in.
#[must_use]
pub fn group_format(space: tinker_pdf_content::GroupSpace) -> PixelFormat {
    match space {
        tinker_pdf_content::GroupSpace::Gray => PixelFormat::GrayA8,
        tinker_pdf_content::GroupSpace::Rgb => PixelFormat::Rgba8,
        tinker_pdf_content::GroupSpace::Cmyk => PixelFormat::CmykA8,
        tinker_pdf_content::GroupSpace::Lab => PixelFormat::LabA8,
    }
}

/// The format a *page* may be handed back in.
///
/// `CmykA8` exists so a transparency group can composite over ink (11.6.6), and
/// `LabA8` so one can composite in Lab. Neither is a format a page comes back
/// in, and the reason is that `Bitmap` says how many components it has and
/// nothing about what they mean: every consumer that reads three bytes and
/// calls them red, green and blue — this repository's own `examples/render.rs`
/// writes a PPM that way — would emit cyan, magenta and yellow, or lightness
/// and two opponent axes, under those names and look almost right.
///
/// So a caller asking for either gets `Rgba8`, which carries the same alpha and
/// the colours the name promises. Silently, because there is nothing for the
/// caller to do about it: the request was for pixels, and pixels are what comes
/// back. The facade's `RenderOptions::allow_cmyk` is the one way past this for
/// ink, and it is a second, explicit field rather than a reading of this one,
/// so that a caller has to say they know the bytes are ink.
///
/// `Bgr8` and `Bgra8` store light, blue first, and come back as asked; they
/// are composited red first ([`canvas_format`]) and reordered at the end.
///
/// *`LabA8` was let through until September 2026*, while its own documentation
/// and this function's both said a page never came back in it: the `match`
/// named `CmykA8` alone, so `format: LabA8` handed a caller four bytes of
/// encoded `L*a*b*` under a format that stores no light at all.
///
/// `Plates` is ink too, and of colorants whose colours a `Bitmap` does not
/// carry, so a caller asking for it is handed `Rgba8` as well.
#[must_use]
pub fn page_format(format: PixelFormat) -> PixelFormat {
    match format {
        PixelFormat::CmykA8 | PixelFormat::LabA8 | PixelFormat::Plates { .. } => PixelFormat::Rgba8,
        other => other,
    }
}

/// The format a page asked for in `format` composites in.
///
/// The blue-first orders are **output orders**: a page asked for in `Bgr8` or
/// `Bgra8` composites in `Rgb8` or `Rgba8` and is reordered once, at the end,
/// by `Canvas::extract` (`docs/design/output-pixel-models.md`). So every
/// buffer a render opens — a group's, a soft mask's, a tiling cell's, a
/// mesh's, an image run's — keeps the formats it has, and nothing in the
/// renderer meets a second channel order. Every other format composites as
/// itself, as it always has.
#[must_use]
pub fn canvas_format(format: PixelFormat) -> PixelFormat {
    match format {
        PixelFormat::Bgr8 => PixelFormat::Rgb8,
        PixelFormat::Bgra8 => PixelFormat::Rgba8,
        PixelFormat::Gray8
        | PixelFormat::GrayA8
        | PixelFormat::Rgb8
        | PixelFormat::Rgba8
        | PixelFormat::CmykA8
        | PixelFormat::LabA8
        | PixelFormat::Plates { .. } => format,
    }
}

#[cfg(test)]
mod tests {

    /// Every format a page may come back in, and the three it may not.
    ///
    /// Exhaustive over `PixelFormat` on purpose: a format added to the
    /// rasterizer fails to compile here until somebody decides which side of
    /// this line it is on.
    #[test]
    fn only_formats_that_store_light_are_page_formats() {
        let spots = |count| PixelFormat::Plates {
            spots: tinker_pdf_raster::Spots::new(count).expect("under the cap"),
        };
        for format in [
            PixelFormat::Gray8,
            PixelFormat::GrayA8,
            PixelFormat::Rgb8,
            PixelFormat::Rgba8,
            PixelFormat::CmykA8,
            PixelFormat::LabA8,
            PixelFormat::Bgr8,
            PixelFormat::Bgra8,
            spots(0),
            spots(3),
            spots(tinker_pdf_raster::MAX_SPOT_PLATES),
        ] {
            // The blue-first orders store light, in another order: a page
            // comes back in them as asked.
            let expected = match format {
                PixelFormat::Gray8
                | PixelFormat::GrayA8
                | PixelFormat::Rgb8
                | PixelFormat::Rgba8
                | PixelFormat::Bgr8
                | PixelFormat::Bgra8 => format,
                PixelFormat::CmykA8 | PixelFormat::LabA8 | PixelFormat::Plates { .. } => {
                    PixelFormat::Rgba8
                }
            };
            assert_eq!(page_format(format), expected, "{format:?}");
        }
    }

    /// **A blue-first request composites red-first**, and every other
    /// format composites as itself — exhaustive for the reason the test above
    /// is: a format added to the rasterizer does not compile here until it is
    /// placed. The composite format stores the same light as the asked one,
    /// with as many components and alpha alike, so the one reorder at the end
    /// is all that parts them.
    #[test]
    fn a_blue_first_request_composites_red_first() {
        for format in [
            PixelFormat::Gray8,
            PixelFormat::GrayA8,
            PixelFormat::Rgb8,
            PixelFormat::Rgba8,
            PixelFormat::CmykA8,
            PixelFormat::LabA8,
            PixelFormat::Bgr8,
            PixelFormat::Bgra8,
            PixelFormat::Plates {
                spots: tinker_pdf_raster::Spots::new(3).expect("under the cap"),
            },
        ] {
            let expected = match format {
                PixelFormat::Bgr8 => PixelFormat::Rgb8,
                PixelFormat::Bgra8 => PixelFormat::Rgba8,
                PixelFormat::Gray8
                | PixelFormat::GrayA8
                | PixelFormat::Rgb8
                | PixelFormat::Rgba8
                | PixelFormat::CmykA8
                | PixelFormat::LabA8
                | PixelFormat::Plates { .. } => format,
            };
            let composited = canvas_format(format);
            assert_eq!(composited, expected, "{format:?}");
            assert!(!composited.is_blue_first(), "{format:?}");
            assert_eq!(composited.components(), format.components(), "{format:?}");
            assert_eq!(composited.has_alpha(), format.has_alpha(), "{format:?}");
        }
    }

    /// **The two crates agree about ink and light.**
    ///
    /// `tinker-pdf-raster` carries its own byte-level copy of 8.6.4.4's device
    /// relation rather than depending on `tinker-pdf-color` for it — the same
    /// call the crate graph makes for `Color::luma`'s coefficients, and the
    /// alternative was a fifth leaf-to-leaf edge for eight lines of
    /// arithmetic. A duplicate that can drift is worth nothing, so this holds
    /// the copy to the original over every combination that matters.
    ///
    /// This crate is where it lives because this crate already depends on both.
    #[test]
    fn the_two_crates_agree_on_the_device_relation() {
        let space = tinker_pdf_color::ColorSpace::DeviceCmyk;
        let levels = [0u8, 1, 17, 64, 128, 191, 254, 255];
        for &c in &levels {
            for &m in &levels {
                for &y in &levels {
                    for &k in &levels {
                        let ours = tinker_pdf_raster::canvas::cmyk_to_rgb(c, m, y, k);
                        let unit = |v: u8| f64::from(v) / 255.0;
                        let theirs = space.to_rgb(&[unit(c), unit(m), unit(y), unit(k)]);
                        assert_eq!(
                            ours, theirs,
                            "({c}, {m}, {y}, {k}): the rasterizer and the colour \
                             crate disagree about 8.6.4.4"
                        );
                    }
                }
            }
        }
    }
    use super::*;
    use tinker_pdf_content::interpret;
    use tinker_pdf_content::FontSource;
    use tinker_pdf_raster::fill::fill;

    struct NoFonts;
    impl FontSource for NoFonts {
        fn decode(&self, _font: &[u8], _bytes: &[u8]) -> Vec<(u32, String, f64)> {
            Vec::new()
        }
        /// Nothing decodes, so nothing is shown and nothing asks. Spelled out
        /// rather than defaulted because 9.7.4.3's metrics have no safe
        /// default: the trait makes every source answer.
        fn vertical_metrics(&self, _font: &[u8], _code: u32) -> (f64, f64, f64) {
            (0.0, 880.0, -1000.0)
        }
    }

    fn render(content: &[u8], size: u32) -> (Canvas, Vec<RenderWarning>) {
        let canvas = Canvas::new(size, size, PixelFormat::Rgb8, Color::WHITE);
        let base = page_transform(f64::from(size), 1.0);
        let mut renderer = Renderer::new(canvas, base, &NoGlyphs);
        interpret(content, Matrix::IDENTITY, &mut renderer, &NoFonts);
        renderer.finish()
    }

    #[test]
    fn a_filled_rectangle_lands_where_pdf_coordinates_say() {
        // PDF's origin is bottom-left, so a rectangle at y=0 paints the
        // *bottom* of the image.
        let (canvas, _) = render(b"0 0 10 4 re f", 20);

        assert_eq!(
            canvas.pixel(5, 18).map(|c| c.r),
            Some(0),
            "the bottom rows are painted"
        );
        assert_eq!(
            canvas.pixel(5, 2).map(|c| c.r),
            Some(255),
            "the top rows are not"
        );
    }

    #[test]
    fn a_stroke_draws_a_line() {
        let (canvas, _) = render(b"2 10 m 18 10 l S", 20);
        let painted = (0..20)
            .filter(|&x| canvas.pixel(x, 10).is_some_and(|c| c.r < 200))
            .count();
        assert!(painted > 10, "the stroke covers the line, got {painted}");
    }

    /// Shapes that between them reach every way a bounding box can be wrong:
    /// integer coordinates, sub-pixel offsets, curves whose control points sit
    /// outside the ink, a shape hanging off each edge of the canvas, and one
    /// that misses the canvas entirely.
    fn shapes() -> Vec<(&'static str, Path)> {
        let mut out = Vec::new();

        let mut rect = Path::new();
        rect.rect(4.0, 6.0, 9.0, 7.0);
        out.push(("integer rectangle", rect));

        let mut offset = Path::new();
        offset.rect(4.3, 6.7, 9.4, 7.1);
        out.push(("sub-pixel rectangle", offset));

        let mut triangle = Path::new();
        triangle.move_to(2.5, 2.0);
        triangle.line_to(17.5, 9.25);
        triangle.line_to(6.0, 18.75);
        triangle.close();
        out.push(("triangle", triangle));

        let mut curve = Path::new();
        curve.move_to(3.0, 10.0);
        curve.curve_to(3.0, 1.0, 17.0, 1.0, 17.0, 10.0);
        curve.quad_to(10.0, 19.0, 3.0, 10.0);
        curve.close();
        out.push(("curves", curve));

        for (name, (x, y)) in [
            ("over the left edge", (-6.5, 5.0)),
            ("over the right edge", (14.5, 5.0)),
            ("over the top edge", (5.0, -6.5)),
            ("over the bottom edge", (5.0, 14.5)),
        ] {
            let mut path = Path::new();
            path.rect(x, y, 11.25, 11.25);
            out.push((name, path));
        }

        let mut away = Path::new();
        away.rect(100.0, 100.0, 5.0, 5.0);
        out.push(("entirely off the canvas", away));

        let mut hairline = Path::new();
        hairline.move_to(10.0, 0.5);
        hairline.line_to(10.4, 19.5);
        hairline.line_to(10.6, 19.5);
        hairline.line_to(10.2, 0.5);
        hairline.close();
        out.push(("a near-vertical sliver", hairline));

        out
    }

    /// The invariant the whole of gap 14 rests on, at the level that decides
    /// it: a mask over the path's own rectangle carries the same coverage, at
    /// every pixel of the canvas, as one over the whole page.
    ///
    /// This is what a fingerprint would report as a hash mismatch with no clue
    /// where; here a failure names the shape, the pixel and both values. A
    /// bound one pixel short on any single edge fails it.
    #[test]
    fn a_bounded_mask_and_a_page_sized_one_agree_everywhere() {
        const SIDE: u32 = 20;
        for (name, path) in shapes() {
            for rule in [FillRule::NonZero, FillRule::EvenOdd] {
                let whole = fill(&path, rule, 0, 0, SIDE, SIDE, 0.2, None);
                let (x0, y0, w, h) = paint_region(path.bounds(), None, None, (0, 0, SIDE, SIDE));
                let bounded = fill(&path, rule, x0, y0, w, h, 0.2, None);

                for y in 0..SIDE as i32 {
                    for x in 0..SIDE as i32 {
                        assert_eq!(
                            whole.at(x, y),
                            bounded.at(x, y),
                            "{name} under {rule:?} differs at ({x}, {y}); \
                             the bounded mask is {w}x{h} at ({x0}, {y0})"
                        );
                    }
                }
            }
        }
    }

    /// And the region is actually small, or the test above would pass on a
    /// build that had changed nothing.
    #[test]
    fn the_region_is_the_shape_rather_than_the_page() {
        let mut path = Path::new();
        path.rect(4.0, 6.0, 9.0, 7.0);
        assert_eq!(
            paint_region(path.bounds(), None, None, (0, 0, 2000, 2000)),
            (3, 5, 11, 9),
            "the bounding box, one pixel of slack on each side"
        );

        // Clamped to the canvas rather than reaching outside it.
        let mut over = Path::new();
        over.rect(-50.0, -50.0, 60.0, 60.0);
        assert_eq!(
            paint_region(over.bounds(), None, None, (0, 0, 100, 100)),
            (0, 0, 11, 11)
        );

        // A path that misses the canvas costs nothing at all.
        let mut away = Path::new();
        away.rect(500.0, 500.0, 10.0, 10.0);
        assert_eq!(
            paint_region(away.bounds(), None, None, (0, 0, 100, 100)),
            (100, 100, 0, 0)
        );

        // Neither does one with no points.
        assert_eq!(
            paint_region(Path::new().bounds(), None, None, (0, 0, 100, 100)),
            (0, 0, 0, 0)
        );
    }

    /// A clip bounds the region as well, because coverage multiplies and the
    /// clip is zero outside itself.
    #[test]
    fn a_clip_bounds_the_region_it_cannot_paint_outside_of() {
        let mut path = Path::new();
        path.rect(10.0, 10.0, 60.0, 60.0);
        let mut clip_path = Path::new();
        clip_path.rect(30.0, 40.0, 10.0, 10.0);
        let clip = fill(&clip_path, FillRule::NonZero, 29, 39, 12, 12, 0.2, None);

        let (x0, y0, w, h) = paint_region(path.bounds(), Some(&clip), None, (0, 0, 200, 200));
        assert_eq!((x0, y0, w, h), (29, 39, 12, 12), "the clip is the smaller");

        // And the clipped coverage is unchanged by that: compare against the
        // full-page mask multiplied by the same clip.
        let whole = fill(&path, FillRule::NonZero, 0, 0, 200, 200, 0.2, None).intersect(&clip);
        let bounded = fill(&path, FillRule::NonZero, x0, y0, w, h, 0.2, None).intersect(&clip);
        for y in 0..200 {
            for x in 0..200 {
                assert_eq!(whole.at(x, y), bounded.at(x, y), "at ({x}, {y})");
            }
        }
    }

    /// Renders onto a square page and reports how many mask pixels the paints
    /// between them asked `fill` for.
    fn mask_cost<G: GlyphSource>(content: &[u8], size: u32, glyphs: &G) -> (Canvas, u64) {
        let canvas = Canvas::new(size, size, PixelFormat::Rgb8, Color::WHITE);
        let base = page_transform(f64::from(size), 1.0);
        let mut renderer = Renderer::new(canvas, base, glyphs);
        interpret(content, Matrix::IDENTITY, &mut renderer, &OneChar);
        let cost = renderer.mask_pixels;
        (renderer.finish().0, cost)
    }

    /// Milestone 4 of gap 14: the guard that makes a regression *fail*.
    ///
    /// Everything else in this gap is invisible to a test, by design — the
    /// pixels are identical before and after, which is the whole point. So a
    /// paint that went back to asking for the whole canvas would cost two
    /// hundred times what it should and pass every assertion in the
    /// repository, including the fingerprints. It would be found, eventually,
    /// by somebody wondering why a page was slow.
    ///
    /// Each case draws a shape a few tens of pixels across on a page of four
    /// million, through a different one of the four `fill` call sites, and
    /// names the pixels it may ask for. A full-canvas call is 4 000 000, so
    /// the budgets do not need to be tight to be decisive; they are tight
    /// anyway, because a budget with room in it stops being a statement about
    /// anything.
    #[test]
    fn a_small_fill_on_a_large_page_stays_small() {
        const SIDE: u32 = 2000;
        let whole_page = u64::from(SIDE) * u64::from(SIDE);

        // (what it draws, which call site, the region it may ask for)
        let cases: &[(&str, &[u8], u64)] = &[
            // A 20 x 20 rectangle: 22 x 22 with the slack.
            ("a fill", b"0 0 0 rg 10 10 20 20 re f", 484),
            // A 20 px line stroked 4 wide, butt caps: 20 x 4 of outline plus
            // the slack, so 22 x 6.
            ("a stroke", b"4 w 10 10 m 30 10 l S", 140),
            // The clip is its own fill, and the fill inside it is bounded by
            // both: 22 x 22 for the clip, and the same again for the paint,
            // which is why this one is twice the first.
            (
                "a clip and a fill inside it",
                b"q 10 10 20 20 re W n 0 0 0 rg 0 0 2000 2000 re f Q",
                968,
            ),
            // A glyph at 40 pt, filled: about 40 x 40 of em.
            ("a glyph", b"BT /F0 40 Tf 10 10 Td (A) Tj ET", 1500),
            // The same glyph filled *and* stroked (9.3.6 mode 2), which is
            // two masks, the second slightly larger for the pen.
            (
                "a glyph filled and stroked",
                b"BT /F0 40 Tf 2 Tr 1 w 10 10 Td (A) Tj ET",
                2400,
            ),
        ];

        for (name, content, budget) in cases {
            let (canvas, cost) = mask_cost(content, SIDE, &ClosedThenCurved);
            let ink = (0..SIDE)
                .flat_map(|y| (0..SIDE).map(move |x| (x, y)))
                .filter(|(x, y)| canvas.pixel(*x, *y) != Some(Color::WHITE))
                .count();
            assert!(ink > 20, "{name} painted almost nothing: {ink} pixels");
            assert!(
                cost <= *budget,
                "{name} asked `fill` for {cost} mask pixels on a {SIDE}x{SIDE} \
                 page, more than the {budget} it covers. A full-canvas paint \
                 is {whole_page}: if this is that, the paint stopped bounding \
                 itself and nothing else in the workspace will say so, because \
                 the pixels are identical either way."
            );
        }
    }

    #[test]
    fn page_pixels_round_outward() {
        // The A4-at-150-dpi case the previous engine left as folklore.
        let scale = 150.0 / 72.0;
        assert_eq!(page_pixels(595.0, 842.0, scale), (1240, 1755));
        // Whole numbers stay whole.
        assert_eq!(page_pixels(100.0, 100.0, 1.0), (100, 100));
        assert_eq!(page_pixels(595.0, 842.0, 1.0), (595, 842));
        assert_eq!(page_pixels(595.0, 842.0, 2.0), (1190, 1684));
        // Nonsense produces a usable canvas rather than a panic.
        assert_eq!(page_pixels(0.0, -5.0, 1.0), (1, 1));
        assert_eq!(page_pixels(f64::NAN, 10.0, 1.0), (1, 10));
    }

    #[test]
    fn the_page_transform_flips_the_y_axis() {
        let m = page_transform(100.0, 1.0);
        assert_eq!(m.apply(0.0, 0.0), (0.0, 100.0), "PDF origin is at the foot");
        assert_eq!(m.apply(0.0, 100.0), (0.0, 0.0), "the top of the page");
    }

    #[test]
    fn an_unsupported_image_warns_rather_than_failing() {
        let (canvas, warnings) = render(b"q 10 0 0 10 0 0 cm /Im0 Do Q 0 0 5 5 re f", 20);

        assert!(
            warnings
                .iter()
                .any(|w| matches!(w, RenderWarning::UnsupportedImage { .. })),
            "the image is reported"
        );
        assert_eq!(
            canvas.pixel(2, 18).map(|c| c.r),
            Some(0),
            "and the rest of the page still renders"
        );
    }

    #[test]
    fn cancellation_stops_the_render() {
        let canvas = Canvas::new(20, 20, PixelFormat::Rgb8, Color::WHITE);
        let cancel = CancelToken::new();
        cancel.cancel();

        let mut renderer =
            Renderer::new(canvas, page_transform(20.0, 1.0), &NoGlyphs).with_cancel(cancel);
        interpret(b"0 0 20 20 re f", Matrix::IDENTITY, &mut renderer, &NoFonts);
        let (canvas, warnings) = renderer.finish();

        assert_eq!(
            canvas.pixel(10, 10).map(|c| c.r),
            Some(255),
            "nothing was painted"
        );
        assert!(warnings.contains(&RenderWarning::Cancelled));
    }

    // -----------------------------------------------------------------------
    // Cancellation (gap 15).
    // -----------------------------------------------------------------------

    /// A square, as the interpreter would hand one over.
    fn square(side: f64) -> Vec<PathSegment> {
        vec![
            PathSegment::MoveTo { x: 10.0, y: 10.0 },
            PathSegment::LineTo { x: side, y: 10.0 },
            PathSegment::LineTo { x: side, y: side },
            PathSegment::LineTo { x: 10.0, y: side },
            PathSegment::Close,
        ]
    }

    /// **The thinnest stroke is the design's at each level and rule**
    /// (RC-11.6): 0.8 for a path and 0.6 for a stroked glyph at 8 bits, 1 at
    /// 1 to 7 bits and at 0 under `Half`, 65/64 under `Centre` and 1/256 under
    /// `AnyPart`, each read at the level of the stroke's own class — a path's
    /// the graphics level, a glyph's the text level — and 0 bits for both
    /// with anti-aliasing off. The default and anti-aliasing off keep the
    /// floors they had, 0.8 and 0.6, and 1. An explicit width replaces the
    /// table for every stroke, and one that is not a finite number above zero
    /// is no width.
    #[test]
    fn the_thinnest_stroke_is_the_designs_at_each_level_and_rule() {
        let make = || {
            Renderer::new(
                Canvas::new(4, 4, PixelFormat::Rgb8, Color::WHITE),
                page_transform(4.0, 1.0),
                &NoGlyphs,
            )
        };
        let floors = |r: &Renderer<'_, NoGlyphs>| (r.thinnest(false), r.thinnest(true));
        assert_eq!(floors(&make()), (0.8, 0.6), "the default");
        assert_eq!(
            floors(&make().with_antialias(false)),
            (1.0, 1.0),
            "anti-aliasing off"
        );
        for bits in 1..=7u8 {
            assert_eq!(
                floors(&make().with_levels(bits, bits)),
                (1.0, 1.0),
                "{bits} bits"
            );
        }
        for (edge, floor) in [
            (HardEdge::Half, 1.0),
            (HardEdge::Centre, 65.0 / 64.0),
            (HardEdge::AnyPart, 1.0 / 256.0),
        ] {
            let at_zero = make().with_levels(0, 0).with_hard_edge(edge);
            assert_eq!(floors(&at_zero), (floor, floor), "{edge:?} at 0 bits");
            let off = make().with_antialias(false).with_hard_edge(edge);
            assert_eq!(
                floors(&off),
                (floor, floor),
                "{edge:?} with anti-aliasing off"
            );
            // A rule at 8 bits is not read.
            assert_eq!(
                floors(&make().with_hard_edge(edge)),
                (0.8, 0.6),
                "{edge:?} at 8 bits"
            );
        }
        // Each stroke at its own class's level: a path at the graphics level,
        // a stroked glyph at the text level while it is shown.
        let mut split = make().with_levels(8, 0).with_hard_edge(HardEdge::Centre);
        assert_eq!(split.thinnest(false), 0.8, "a path at 8 bits");
        split.showing = 1;
        assert_eq!(split.thinnest(true), 65.0 / 64.0, "a glyph at 0 bits");
        split.showing = 0;
        split.begin_text();
        assert_eq!(
            split.thinnest(false),
            65.0 / 64.0,
            "a path inside a text object is text's"
        );
        for width in [Some(2.5), Some(0.01)] {
            let fixed = make()
                .with_levels(3, 0)
                .with_hard_edge(HardEdge::AnyPart)
                .with_thinnest_line(width);
            assert_eq!(floors(&fixed), (width.unwrap_or(0.0), width.unwrap_or(0.0)));
        }
        for width in [
            Some(0.0),
            Some(-1.0),
            Some(f64::NAN),
            Some(f64::INFINITY),
            None,
        ] {
            assert_eq!(
                floors(&make().with_thinnest_line(width)),
                (0.8, 0.6),
                "{width:?} is no width"
            );
        }
    }

    /// **What a text object paints is text, and no text object outlives its
    /// save or its stream** (RC-11.5). With graphics at 0 bits and text at 8,
    /// a turned triangle filled outside a text object is whole or empty; the
    /// same fill inside one — a Type 3 glyph's procedure painting — keeps
    /// its partial pixels; and a fill after a text object that a restore, or
    /// an entered form's end, leaves open is graphics again, as
    /// `tinker_pdf_content::cull` counts text objects.
    #[test]
    fn a_text_object_left_open_ends_with_its_save_and_its_stream() {
        let triangle = vec![
            PathSegment::MoveTo { x: 3.3, y: 4.1 },
            PathSegment::LineTo { x: 27.6, y: 9.8 },
            PathSegment::LineTo { x: 12.2, y: 26.4 },
            PathSegment::Close,
        ];
        let soft = |drive: &dyn Fn(&mut Renderer<'_, NoGlyphs>)| {
            let canvas = Canvas::new(32, 32, PixelFormat::Rgb8, Color::WHITE);
            let mut renderer =
                Renderer::new(canvas, page_transform(32.0, 1.0), &NoGlyphs).with_levels(0, 8);
            drive(&mut renderer);
            renderer.fill_path(&triangle, &GraphicsState::default(), false);
            let (canvas, _) = renderer.finish();
            canvas.data.iter().any(|v| *v != 0 && *v != 255)
        };
        assert!(!soft(&|_| {}), "a fill outside a text object is graphics");
        assert!(
            soft(&|r: &mut Renderer<'_, NoGlyphs>| r.begin_text()),
            "a fill inside one is text"
        );
        assert!(
            !soft(&|r: &mut Renderer<'_, NoGlyphs>| {
                r.begin_text();
                r.end_text();
            }),
            "an ended one is over"
        );
        assert!(
            !soft(&|r: &mut Renderer<'_, NoGlyphs>| {
                r.save_state();
                r.begin_text();
                r.restore_state();
            }),
            "a restore ends what its save did not hold"
        );
        assert!(
            soft(&|r: &mut Renderer<'_, NoGlyphs>| {
                r.begin_text();
                r.save_state();
                r.restore_state();
            }),
            "and keeps what it did"
        );
        assert!(
            !soft(&|r: &mut Renderer<'_, NoGlyphs>| {
                assert!(r.begin_form(1, b"Fm0"));
                r.begin_text();
                r.end_form(1);
            }),
            "a form's stream ends the text it left open"
        );
    }

    /// Milestone 1: a clip and a text clip check before they rasterize.
    ///
    /// Both are a full fill of a path — a clip is `fill` plus an intersect,
    /// and a text clip is one fill over every glyph the object accumulated —
    /// and neither had a check of any kind, on entry or inside.
    ///
    /// The instrument is `mask_pixels`, the count of pixels `fill` was *asked
    /// for*, because the page cannot tell the two cases apart: a clip that ran
    /// and a clip that did not leave the same canvas, since a clip paints
    /// nothing. Only the counter says which happened.
    ///
    /// The operators are driven directly rather than through `interpret`,
    /// which is the honest way to reach this: the interpreter checks the token
    /// before every token it dispatches, so through it these two entries are
    /// reachable only in the window between that check and the call — which is
    /// real on a second thread and not reproducible in a test. `Device` is a
    /// public trait and `Renderer` implements it, so a host can arrive here by
    /// any route it likes.
    #[test]
    fn a_cancelled_clip_and_text_clip_rasterize_nothing() {
        const SIDE: u32 = 400;
        let cost = |cancelled: bool| {
            let canvas = Canvas::new(SIDE, SIDE, PixelFormat::Rgb8, Color::WHITE);
            let cancel = CancelToken::new();
            if cancelled {
                cancel.cancel();
            }
            let mut renderer =
                Renderer::new(canvas, page_transform(400.0, 1.0), &NoGlyphs).with_cancel(cancel);
            let state = GraphicsState::default();

            renderer.clip_path(&square(300.0), &state, false);
            let clip = renderer.mask_pixels;

            // A text clip: the glyph outline accumulates and `ET` applies it.
            renderer.text_clip_requested = true;
            renderer.text_clip = Some(renderer.to_path(&square(200.0)));
            renderer.end_text();
            (clip, renderer.mask_pixels - clip)
        };

        let (clip, text) = cost(false);
        assert!(clip > 0, "a clip that is not cancelled rasterizes: {clip}");
        assert!(text > 0, "and so does a text clip: {text}");

        assert_eq!(
            cost(true),
            (0, 0),
            "a token set before the operator arrived means neither fill runs"
        );
    }

    /// A source that cancels the render the first time it is asked for an
    /// outline, and counts how many times it was asked.
    ///
    /// This is the counting token milestone 1 needs, and it is deterministic:
    /// it fires on the Nth *call*, never on a clock. A time-based cancellation
    /// test is flaky by construction — the render either got there first or it
    /// did not — and one in CI would fail for reasons nobody could reproduce.
    struct CancelOnOutline {
        cancel: CancelToken,
        calls: std::cell::Cell<u32>,
        /// Which call cancels, counting from one.
        at: u32,
    }

    impl GlyphSource for CancelOnOutline {
        fn outline(&self, _font_id: u64, _code: u32) -> Option<Outline> {
            self.calls.set(self.calls.get() + 1);
            if self.calls.get() == self.at {
                self.cancel.cancel();
            }
            use tinker_pdf_font::Segment;
            Some(Outline {
                segments: vec![
                    Segment::MoveTo { x: 0.1, y: 0.0 },
                    Segment::LineTo { x: 0.6, y: 0.0 },
                    Segment::LineTo { x: 0.6, y: 0.7 },
                    Segment::LineTo { x: 0.1, y: 0.7 },
                    Segment::Close,
                ],
            })
        }
    }

    /// Milestone 1, and the reason `paint` needed a check of its own: its
    /// callers check on *their* entry, which is a different moment.
    ///
    /// `show_glyph` checks the token, then asks the source for an outline —
    /// which is where a host reads a file, decompresses a font program or
    /// waits on a lock — and only then paints. The source here cancels during
    /// exactly that call, so `show_glyph`'s own check has already passed and
    /// the token is set by the time the paint begins. Without a check in
    /// `paint` the glyph is drawn in full.
    ///
    /// Two glyphs, and the second is the one that cancels, so the assertion is
    /// that the page has ink on it and stops — a build that painted nothing at
    /// all would pass a "nothing after the cancel" test on its own.
    #[test]
    fn a_paint_checks_on_its_own_entry_and_not_only_its_callers() {
        let ink_after = |at: u32| {
            let canvas = Canvas::new(120, 60, PixelFormat::Rgb8, Color::WHITE);
            let source = CancelOnOutline {
                cancel: CancelToken::new(),
                calls: std::cell::Cell::new(0),
                at,
            };
            let mut renderer = Renderer::new(canvas, page_transform(60.0, 1.0), &source)
                .with_cancel(source.cancel.clone());
            interpret(
                b"BT /F0 40 Tf 2 4 Td (AB) Tj ET",
                Matrix::IDENTITY,
                &mut renderer,
                &OneChar,
            );
            let (canvas, _) = renderer.finish();
            let column = |x: u32| {
                (0..60)
                    .filter(|y| canvas.pixel(x, *y) != Some(Color::WHITE))
                    .count()
            };
            (column(10), column(30))
        };

        // Nothing cancels: both glyphs paint. The second sits half an em past
        // the first, which for this outline puts its left bar at x = 30.
        let (first, second) = ink_after(99);
        assert!(first > 0 && second > 0, "both glyphs: {first}, {second}");

        // The second glyph's outline lookup cancels. The first is already on
        // the page; the second must not reach it.
        assert_eq!(
            ink_after(2),
            (first, 0),
            "the glyph whose own lookup set the token is not painted"
        );
    }

    /// Milestone 3, and it is a pair: `Cancelled` has to mean work was
    /// skipped, not that somebody set the token.
    ///
    /// The two halves are one test because either alone is satisfied by a
    /// wrong answer. Reporting from the token passes the first; reporting
    /// nothing ever passes the second.
    ///
    /// The second half is the case a caller actually meets: a timeout that
    /// fires a millisecond after the last operator drew. The page is whole,
    /// and telling its owner it was cancelled makes them throw away a
    /// complete render — or, worse, teaches them to ignore the warning.
    #[test]
    fn cancelled_reports_skipped_work_rather_than_a_token_that_was_set() {
        let page = |set_before: bool| {
            let canvas = Canvas::new(20, 20, PixelFormat::Rgb8, Color::WHITE);
            let cancel = CancelToken::new();
            if set_before {
                cancel.cancel();
            }
            let mut renderer = Renderer::new(canvas, page_transform(20.0, 1.0), &NoGlyphs)
                .with_cancel(cancel.clone());
            interpret(b"0 0 20 20 re f", Matrix::IDENTITY, &mut renderer, &NoFonts);
            if !set_before {
                // The render is over; the token arrives too late to stop
                // anything.
                cancel.cancel();
            }
            let (canvas, warnings) = renderer.finish();
            (
                canvas.pixel(10, 10).map(|c| c.r),
                warnings.contains(&RenderWarning::Cancelled),
            )
        };

        assert_eq!(
            page(true),
            (Some(255), true),
            "a render that stopped early: nothing painted, and it says so"
        );
        assert_eq!(
            page(false),
            (Some(0), false),
            "a render that finished first: the whole page, and no warning"
        );
    }

    /// A source that cancels the render inside the image lookup, and answers
    /// with an image anyway.
    ///
    /// That is the one place a token can be set between `draw_image`'s entry
    /// check and the pixels: a host decoding an image is where a render spends
    /// its longest uninterrupted stretch.
    struct CancelOnImage {
        cancel: CancelToken,
    }

    impl GlyphSource for CancelOnImage {
        fn outline(&self, _font_id: u64, _code: u32) -> Option<Outline> {
            None
        }

        fn image(
            &self,
            name: &[u8],
            _request: &ImageRequest,
            _intent: RenderingIntent,
        ) -> Result<Option<DecodedImage>, String> {
            if name != b"Im0" {
                return Ok(None);
            }
            self.cancel.cancel();
            Ok(Some(DecodedImage {
                stencil: false,
                family: ColorFamily::Rgb,
                soft_masked: false,
                interpolate: false,
                width: 2,
                height: 2,
                rgb: vec![0; 12],
                alpha: Vec::new(),
                two_colour: None,
                reduced: None,
                window: None,
            }))
        }
    }

    /// Milestone 3, from the other side: a stop that fires *inside the
    /// rasterizer* is reported too.
    ///
    /// The predicate handed across the crate boundary is the only thing that
    /// knows a fill stopped halfway down a shape or a draw stopped halfway
    /// down an image — this crate cannot see it happen — so the predicate has
    /// to set the flag itself. One predicate serves the fill, the stroker and
    /// the image draw, and this is the route that can be triggered without a
    /// second thread.
    ///
    /// `Do` is the **last** token on purpose. The interpreter asks the token
    /// before every operator it dispatches, so anything after the image would
    /// set the flag by itself and the test would pass with a predicate that
    /// recorded nothing.
    #[test]
    fn a_stop_inside_the_rasterizer_is_reported_as_cancelled() {
        let canvas = Canvas::new(20, 20, PixelFormat::Rgb8, Color::WHITE);
        let source = CancelOnImage {
            cancel: CancelToken::new(),
        };
        let mut renderer = Renderer::new(canvas, page_transform(20.0, 1.0), &source)
            .with_cancel(source.cancel.clone());
        interpret(
            b"20 0 0 20 0 0 cm /Im0 Do",
            Matrix::IDENTITY,
            &mut renderer,
            &NoFonts,
        );
        let (canvas, warnings) = renderer.finish();

        assert_eq!(
            canvas.pixel(10, 10).map(|c| c.r),
            Some(255),
            "the draw stopped on its first destination row"
        );
        assert!(
            warnings.contains(&RenderWarning::Cancelled),
            "and the predicate that stopped it recorded that it had: {warnings:?}"
        );
    }

    #[test]
    fn colour_operators_reach_the_canvas() {
        // Pure red, then pure blue, each in its own rectangle.
        let (canvas, _) = render(b"1 0 0 rg 0 0 8 20 re f  0 0 1 rg 12 0 8 20 re f", 20);

        assert_eq!(canvas.pixel(4, 10), Some(Color::rgb(255, 0, 0)), "red half");
        assert_eq!(
            canvas.pixel(16, 10),
            Some(Color::rgb(0, 0, 255)),
            "blue half"
        );
    }

    #[test]
    fn grey_and_cmyk_operators_convert_correctly() {
        let (grey, _) = render(b"0.5 g 0 0 20 20 re f", 20);
        let px = grey.pixel(10, 10).expect("a pixel");
        assert!(
            (126..=130).contains(&px.r) && px.r == px.g && px.g == px.b,
            "0.5 g is mid grey, got {px:?}"
        );

        // Cyan ink alone: no red, full green and blue.
        let (cyan, _) = render(b"1 0 0 0 k 0 0 20 20 re f", 20);
        assert_eq!(cyan.pixel(10, 10), Some(Color::rgb(0, 255, 255)));

        // Full black ink.
        let (black, _) = render(b"0 0 0 1 k 0 0 20 20 re f", 20);
        assert_eq!(black.pixel(10, 10), Some(Color::BLACK));
    }

    #[test]
    fn a_stroke_uses_the_stroking_colour_not_the_fill() {
        let (canvas, _) = render(b"1 0 0 rg 0 1 0 RG 2 10 m 18 10 l S", 20);
        let px = canvas.pixel(10, 10).expect("a pixel");
        assert!(
            px.g > px.r && px.g > px.b,
            "the stroke should be green, got {px:?}"
        );
    }

    #[test]
    fn a_clipping_path_confines_what_follows() {
        // Clip to the left half, then fill the whole page.
        let (canvas, _) = render(b"0 0 10 20 re W n 0 0 20 20 re f", 20);

        assert_eq!(
            canvas.pixel(5, 10).map(|c| c.r),
            Some(0),
            "inside the clip is painted"
        );
        assert_eq!(
            canvas.pixel(15, 10).map(|c| c.r),
            Some(255),
            "outside it is not"
        );
    }

    #[test]
    fn a_clip_is_restored_with_the_graphics_state() {
        // Clip inside q/Q, then paint after Q: the clip must be gone.
        let (canvas, _) = render(b"q 0 0 4 20 re W n Q 0 0 20 20 re f", 20);
        assert_eq!(
            canvas.pixel(15, 10).map(|c| c.r),
            Some(0),
            "the clip should not survive Q"
        );
    }

    #[test]
    fn the_even_odd_rule_reaches_the_fill() {
        // Two nested rectangles: even-odd leaves the middle empty.
        let nonzero = render(b"0 0 20 20 re 5 5 10 10 re f", 20).0;
        let evenodd = render(b"0 0 20 20 re 5 5 10 10 re f*", 20).0;

        assert_eq!(nonzero.pixel(10, 10).map(|c| c.r), Some(0), "filled");
        assert_eq!(evenodd.pixel(10, 10).map(|c| c.r), Some(255), "a hole");
    }

    /// Which side of the triangle `a b c` a point is on, with a margin.
    ///
    /// `Some(true)` when the point is inside by more than `margin` from every
    /// edge line, `Some(false)` when it is outside by more than `margin` from
    /// at least one, and `None` in the band along the outline where
    /// anti-aliasing decides. Half-plane arithmetic and nothing else: the
    /// answer is the geometry's, not the rasterizer's.
    fn in_triangle(p: (f64, f64), tri: [(f64, f64); 3], margin: f64) -> Option<bool> {
        let [a, b, c] = tri;
        let orient = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
        let sign = if orient < 0.0 { -1.0 } else { 1.0 };
        let mut inside = true;
        for (u, v) in [(a, b), (b, c), (c, a)] {
            let (dx, dy) = (v.0 - u.0, v.1 - u.1);
            let distance =
                sign * (dx * (p.1 - u.1) - dy * (p.0 - u.0)) / (dx * dx + dy * dy).sqrt();
            if distance < -margin {
                return Some(false);
            }
            if distance <= margin {
                inside = false;
            }
        }
        inside.then_some(true)
    }

    /// **ISO 32000-1 8.5.3.1: before filling, each open subpath is implicitly
    /// closed** — and 8.5.4 says the same of a clipping path.
    ///
    /// `90 10 m 150 30 l 110 60 l f` names two segments and no `h`. Both of its
    /// edges run the same way in `y`, so a filler that only builds the edges a
    /// path states holds one crossing per sub-scanline and paints **nothing**,
    /// which is what this engine did until September 2026. Every pixel whose
    /// centre is more than a pixel inside the triangle the implicit close
    /// makes must be painted and every pixel more than a pixel outside it must
    /// not, decided by half-plane arithmetic rather than by comparing with a
    /// closed render (which would pass a filler that ignored `h` as well). The
    /// same holds for `f*`, for a clip, and for the first of two subpaths,
    /// which a second `m` ends without closing.
    #[test]
    fn an_open_subpath_is_closed_before_it_is_filled_or_clipped() {
        const SIDE: u32 = 160;
        let triangle = [(90.0, 10.0), (150.0, 30.0), (110.0, 60.0)];
        let other = [(10.0, 100.0), (60.0, 150.0), (20.0, 140.0)];
        type Triangle = [(f64, f64); 3];
        let cases: [(&str, &[u8], &[Triangle]); 4] = [
            ("f", b"90 10 m 150 30 l 110 60 l f", &[triangle]),
            ("f*", b"90 10 m 150 30 l 110 60 l f*", &[triangle]),
            (
                "W n",
                b"90 10 m 150 30 l 110 60 l W n 0 0 160 160 re f",
                &[triangle],
            ),
            (
                "two subpaths",
                b"10 100 m 60 150 l 20 140 l 90 10 m 150 30 l 110 60 l f",
                &[other, triangle],
            ),
        ];
        for (what, content, shapes) in cases {
            let (canvas, _) = render(content, SIDE);
            let (mut inside, mut outside) = (0, 0);
            for y in 0..SIDE {
                for x in 0..SIDE {
                    // The pixel's centre in PDF user space: `render` flips y.
                    let p = (f64::from(x) + 0.5, f64::from(SIDE - y) - 0.5);
                    let verdicts: Vec<_> = shapes.iter().map(|t| in_triangle(p, *t, 1.0)).collect();
                    let red = canvas.pixel(x, y).map(|c| c.r);
                    if verdicts.contains(&Some(true)) {
                        inside += 1;
                        assert_eq!(red, Some(0), "{what}: ({x}, {y}) is inside and unpainted");
                    } else if verdicts.iter().all(|v| *v == Some(false)) {
                        outside += 1;
                        assert_eq!(red, Some(255), "{what}: ({x}, {y}) is outside and painted");
                    }
                }
            }
            assert!(
                inside > 700,
                "{what}: only {inside} pixels were deep inside"
            );
            assert!(
                outside > 20_000,
                "{what}: only {outside} pixels were outside"
            );
        }
    }

    /// A source that answers with one solid-colour image.
    struct OneImage {
        color: (u8, u8, u8),
        alpha: Vec<u8>,
    }

    impl GlyphSource for OneImage {
        fn outline(&self, _font_id: u64, _code: u32) -> Option<Outline> {
            None
        }
        fn image(
            &self,
            name: &[u8],
            _request: &ImageRequest,
            _intent: RenderingIntent,
        ) -> Result<Option<DecodedImage>, String> {
            if name != b"Im0" {
                return Ok(None);
            }
            let (r, g, b) = self.color;
            Ok(Some(DecodedImage {
                stencil: false,
                family: ColorFamily::Rgb,
                soft_masked: false,
                interpolate: false,
                width: 2,
                height: 2,
                rgb: vec![r, g, b, r, g, b, r, g, b, r, g, b],
                alpha: self.alpha.clone(),
                two_colour: None,
                reduced: None,
                window: None,
            }))
        }
    }

    /// A source whose image has four different samples, so a filter change is
    /// visible, and whose `/Interpolate` the test chooses.
    struct FourSamples {
        interpolate: bool,
    }

    impl GlyphSource for FourSamples {
        fn outline(&self, _font_id: u64, _code: u32) -> Option<Outline> {
            None
        }
        fn image(
            &self,
            name: &[u8],
            _request: &ImageRequest,
            _intent: RenderingIntent,
        ) -> Result<Option<DecodedImage>, String> {
            if name != b"Im0" {
                return Ok(None);
            }
            Ok(Some(DecodedImage {
                stencil: false,
                family: ColorFamily::Rgb,
                soft_masked: false,
                interpolate: self.interpolate,
                width: 2,
                height: 2,
                #[rustfmt::skip]
                rgb: vec![
                    0, 0, 0,       255, 255, 255,
                    255, 255, 255, 0, 0, 0,
                ],
                alpha: Vec::new(),
                two_colour: None,
                reduced: None,
                window: None,
            }))
        }
    }

    /// Table 89: `/Interpolate` is the file's own request for smoothing, and
    /// it must reach the sampler. The two renders differ in nothing else, so
    /// any difference in the pixels is the flag.
    #[test]
    fn interpolate_chooses_a_different_filter() {
        let render_flag = |interpolate: bool| {
            let canvas = Canvas::new(20, 20, PixelFormat::Rgb8, Color::WHITE);
            let source = FourSamples { interpolate };
            let mut renderer = Renderer::new(canvas, page_transform(20.0, 1.0), &source);
            interpret(
                b"q 20 0 0 20 0 0 cm /Im0 Do Q",
                Matrix::IDENTITY,
                &mut renderer,
                &NoFonts,
            );
            let (canvas, _) = renderer.finish();
            let mut values: Vec<u8> = canvas.data.iter().step_by(3).copied().collect();
            values.sort_unstable();
            values.dedup();
            values
        };

        assert_eq!(
            render_flag(false),
            vec![0, 255],
            "without the key the samples are copied, hard edge and all"
        );
        let smoothed = render_flag(true);
        assert!(
            smoothed.len() > 20,
            "with it they are blended: {} distinct values",
            smoothed.len()
        );
    }

    /// A source that records every image request it is asked, with the
    /// intent it is asked in, and answers each with one grey sample.
    #[derive(Default)]
    struct Requests {
        asked: std::sync::Mutex<Vec<(Vec<u8>, ImageRequest, RenderingIntent)>>,
    }

    impl Requests {
        fn answer(
            &self,
            name: &[u8],
            request: &ImageRequest,
            intent: RenderingIntent,
        ) -> Option<DecodedImage> {
            if let Ok(mut asked) = self.asked.lock() {
                asked.push((name.to_vec(), *request, intent));
            }
            Some(DecodedImage {
                width: 1,
                height: 1,
                rgb: vec![90, 90, 90],
                alpha: Vec::new(),
                two_colour: None,
                stencil: false,
                interpolate: false,
                reduced: None,
                family: ColorFamily::Rgb,
                soft_masked: false,
                window: None,
            })
        }
    }

    impl GlyphSource for Requests {
        fn outline(&self, _font_id: u64, _code: u32) -> Option<Outline> {
            None
        }
        fn image(
            &self,
            name: &[u8],
            request: &ImageRequest,
            intent: RenderingIntent,
        ) -> Result<Option<DecodedImage>, String> {
            Ok(self.answer(name, request, intent))
        }
        fn inline_image(
            &self,
            _dict: &[u8],
            _data: &[u8],
            request: &ImageRequest,
            intent: RenderingIntent,
        ) -> Result<Option<DecodedImage>, String> {
            Ok(self.answer(b"inline", request, intent))
        }
    }

    /// **A draw asks for an image at the size the page draws it**
    /// (`docs/design/jpeg-speed.md`, "The drawn-size request"): the length of
    /// each column of the image's transform in the page frame — scaled,
    /// quarter-turned and mirrored, an image XObject and an inline image alike
    /// — and a tile of the page, its canvas standing at a corner of the page,
    /// asks exactly what the page asks.
    #[test]
    fn a_draw_asks_for_the_size_the_page_draws_it() {
        let content: &[u8] = b"q 40 0 0 20 5 5 cm /Im0 Do Q \
            q 0 30 -10 0 50 5 cm /Im1 Do Q \
            q -12 0 0 9 30 30 cm /Im0 Do Q \
            q 8 0 0 4 2 40 cm BI /W 1 /H 1 /CS /G /BPC 8 ID \x5a EI Q";
        let asked = |canvas: Canvas| {
            let source = Requests::default();
            let mut renderer =
                Renderer::new(canvas, page_transform(50.0, 2.0), &source).with_page_size(120, 100);
            interpret(content, Matrix::IDENTITY, &mut renderer, &NoFonts);
            let _ = renderer.finish();
            source.asked.into_inner().expect("the record")
        };
        let page = asked(Canvas::new(120, 100, PixelFormat::Rgb8, Color::WHITE));
        let expected: Vec<(Vec<u8>, (f64, f64))> = vec![
            (b"Im0".to_vec(), (80.0, 40.0)),
            (b"Im1".to_vec(), (60.0, 20.0)),
            (b"Im0".to_vec(), (24.0, 18.0)),
            (b"inline".to_vec(), (16.0, 8.0)),
        ];
        let drawn: Vec<(Vec<u8>, (f64, f64))> = page
            .iter()
            .map(|(name, request, _)| (name.clone(), request.drawn))
            .collect();
        assert_eq!(drawn, expected, "each draw's size in the page frame");
        let tile = asked(Canvas::new(37, 23, PixelFormat::Rgb8, Color::WHITE).at_origin(41, 19));
        let sizes =
            |asked: &[(Vec<u8>, ImageRequest, RenderingIntent)]| -> Vec<(Vec<u8>, (f64, f64))> {
                asked
                    .iter()
                    .map(|(name, request, _)| (name.clone(), request.drawn))
                    .collect()
            };
        assert_eq!(
            sizes(&tile),
            sizes(&page),
            "a tile asks for the size the page asks for"
        );

        let infinite = ImageRequest::drawn_under(&Matrix {
            a: f64::MAX,
            b: f64::MAX,
            c: 1.0,
            d: 0.0,
            e: 0.0,
            f: 0.0,
        });
        assert!(
            !infinite.drawn.0.is_finite() && infinite.drawn.1 == 1.0,
            "a column past f64 is no finite size: {infinite:?}"
        );
        assert!(!ImageRequest::FULL.drawn.0.is_finite() && !ImageRequest::FULL.drawn.1.is_finite());
    }

    /// **A draw asks for the parts of its image the canvas reaches and the
    /// clip shows** (`docs/design/jpx-speed.md`, "The visible window"), each
    /// mapped back into the image's unit square. On the whole page an image
    /// wholly on it is reached and shown whole; a tile reaches the part of it
    /// under the tile, and none of an image it misses; a clip narrows what is
    /// shown and not what a draw that joins a run reaches, and on a page too
    /// large to hold its images in runs, where each draw composites through
    /// the clip on its own, narrows both. A quarter-turned image's square is
    /// turned with it.
    #[test]
    fn a_draw_asks_for_the_parts_of_its_image_the_canvas_reaches_and_shows() {
        // `Im0` over user (5, 5) to (45, 25), device (10, 50) to (90, 90) at
        // twice the scale; the clip, user (10, 10) to (30, 20), device (20,
        // 60) to (60, 80), held a pixel wider each way as every mask is;
        // `Im1` quarter-turned over user (20, 30) to (50, 40), device (40, 20)
        // to (100, 40).
        let content: &[u8] = b"q 40 0 0 20 5 5 cm /Im0 Do Q \
            q 10 10 20 10 re W n 40 0 0 20 5 5 cm /Im0 Do Q \
            q 0 10 -30 0 50 30 cm /Im1 Do Q";
        let asked = |canvas: Canvas, page: (u32, u32)| {
            let source = Requests::default();
            let mut renderer = Renderer::new(canvas, page_transform(50.0, 2.0), &source)
                .with_page_size(page.0, page.1);
            interpret(content, Matrix::IDENTITY, &mut renderer, &NoFonts);
            let _ = renderer.finish();
            source.asked.into_inner().expect("the record")
        };
        let area =
            |got: Option<UnitArea>, what: &str| got.unwrap_or_else(|| panic!("{what}: an area"));
        let near = |got: Option<UnitArea>, want: (f64, f64, f64, f64), what: &str| {
            let got = area(got, what);
            let (u0, v0, u1, v1) = want;
            for (g, w) in [(got.u0, u0), (got.v0, v0), (got.u1, u1), (got.v1, v1)] {
                assert!((g - w).abs() < 1e-9, "{what}: {got:?} where {want:?}");
            }
        };
        // Holds `inner` and lies inside `outer`, to the last bits of the
        // inverse map.
        let between = |got: Option<UnitArea>,
                       inner: (f64, f64, f64, f64),
                       outer: (f64, f64, f64, f64),
                       what: &str| {
            let got = area(got, what);
            let e = 1e-9;
            assert!(
                got.u0 <= inner.0 + e
                    && got.v0 <= inner.1 + e
                    && got.u1 >= inner.2 - e
                    && got.v1 >= inner.3 - e
                    && got.u0 >= outer.0 - e
                    && got.v0 >= outer.1 - e
                    && got.u1 <= outer.2 + e
                    && got.v1 <= outer.3 + e,
                "{what}: {got:?} between {inner:?} and {outer:?}"
            );
        };
        let whole = (0.0, 0.0, 1.0, 1.0);
        // The clip's own part of `Im0`, and that part a pixel wider each way.
        let (clip, slack) = ((0.125, 0.25, 0.625, 0.75), (0.1125, 0.225, 0.6375, 0.775));

        let page = asked(
            Canvas::new(120, 100, PixelFormat::Rgb8, Color::WHITE),
            (120, 100),
        );
        assert_eq!(page.len(), 3);
        near(page[0].1.reach, whole, "the page reaches all of Im0");
        near(page[0].1.visible, whole, "and shows all of it");
        near(
            page[1].1.reach,
            whole,
            "a run's member reaches past the clip",
        );
        between(page[1].1.visible, clip, slack, "the clip shows its part");
        near(page[2].1.reach, whole, "the page reaches all of Im1");

        // Device (41, 60) to (78, 83): user (20.5, 8.5) to (39, 20), so u
        // from 0.3875 to 0.85 and v from 0.175 to 0.75 of Im0; Im1, from
        // device y 20 to 40, it misses.
        let tile = asked(
            Canvas::new(37, 23, PixelFormat::Rgb8, Color::WHITE).at_origin(41, 60),
            (120, 100),
        );
        near(
            tile[0].1.reach,
            (0.3875, 0.175, 0.85, 0.75),
            "the tile's part",
        );
        near(tile[0].1.visible, (0.3875, 0.175, 0.85, 0.75), "shown");
        between(
            tile[1].1.visible,
            (0.3875, 0.25, 0.625, 0.75),
            (0.3875, 0.225, 0.6375, 0.75),
            "the tile and the clip",
        );
        assert!(
            tile[2].1.reach.is_some_and(|area| area.is_empty()),
            "the tile reaches none of Im1: {:?}",
            tile[2].1.reach
        );

        // A page past `MAX_IMAGE_RUN_PIXELS` draws each image on its own.
        let large = asked(
            Canvas::new(120, 100, PixelFormat::Rgb8, Color::WHITE),
            (5_000, 4_000),
        );
        between(
            large[1].1.reach,
            clip,
            slack,
            "a draw alone reaches what it shows",
        );
        between(large[1].1.visible, clip, slack, "and shows its part");

        // Im1's square turned a quarter: a tile over device (40, 20) to (70,
        // 30), user (20, 35) to (35, 40), is u from a half to one along the
        // turned height and v from a half to one along the turned width.
        let turned = asked(
            Canvas::new(30, 10, PixelFormat::Rgb8, Color::WHITE).at_origin(40, 20),
            (120, 100),
        );
        near(
            turned[2].1.reach,
            (0.5, 0.5, 1.0, 1.0),
            "turned with the image",
        );
        assert!(UnitArea::of_device(
            &Matrix {
                a: 0.0,
                b: 0.0,
                c: 0.0,
                d: 0.0,
                e: 0.0,
                f: 0.0,
            },
            (0.0, 0.0, 1.0, 1.0)
        )
        .is_none());
    }

    /// **A draw hands the image seam the intent in force** (8.6.5.8): the
    /// initial relative colorimetric, then each intent `ri` names, an inline
    /// image as an image XObject, `Q` restoring the intent `q` saved, and a
    /// name 8.6.5.8 does not list kept as unrecognised for the source to read.
    /// A tile asks in the intents the page asks in.
    #[test]
    fn a_draw_hands_the_image_seam_the_intent_in_force() {
        let content: &[u8] = b"q 10 0 0 10 0 0 cm /Im0 Do Q \
            /Perceptual ri q 10 0 0 10 10 0 cm /Im0 Do Q \
            q /Saturation ri q 10 0 0 10 20 0 cm BI /W 1 /H 1 /CS /G /BPC 8 ID \x5a EI Q \
            q 10 0 0 10 30 0 cm /Im1 Do Q Q \
            q 10 0 0 10 40 0 cm /Im0 Do Q \
            /AbsoluteColorimetric ri q 10 0 0 10 50 0 cm /Im1 Do Q \
            /Ultraviolet ri q 10 0 0 10 60 0 cm BI /W 1 /H 1 /CS /G /BPC 8 ID \x5a EI Q";
        let asked = |canvas: Canvas| {
            let source = Requests::default();
            let mut renderer =
                Renderer::new(canvas, page_transform(20.0, 1.0), &source).with_page_size(80, 20);
            interpret(content, Matrix::IDENTITY, &mut renderer, &NoFonts);
            let _ = renderer.finish();
            source.asked.into_inner().expect("the record")
        };
        let page = asked(Canvas::new(80, 20, PixelFormat::Rgb8, Color::WHITE));
        let intents: Vec<(Vec<u8>, RenderingIntent)> = page
            .iter()
            .map(|(name, _, intent)| (name.clone(), *intent))
            .collect();
        assert_eq!(
            intents,
            vec![
                (b"Im0".to_vec(), RenderingIntent::RelativeColorimetric),
                (b"Im0".to_vec(), RenderingIntent::Perceptual),
                (b"inline".to_vec(), RenderingIntent::Saturation),
                (b"Im1".to_vec(), RenderingIntent::Saturation),
                (b"Im0".to_vec(), RenderingIntent::Perceptual),
                (b"Im1".to_vec(), RenderingIntent::AbsoluteColorimetric),
                (b"inline".to_vec(), RenderingIntent::Unrecognised),
            ],
            "each draw in the intent in force at it"
        );
        let tile = asked(Canvas::new(23, 11, PixelFormat::Rgb8, Color::WHITE).at_origin(17, 5));
        assert_eq!(tile, page, "a tile asks in the intents the page asks in");
    }

    /// One image under two names: `Im0` three by two samples standing for a
    /// five by three image, two of its samples a sample, and `Im1` the same
    /// samples as an image of their own.
    struct Halved;

    impl GlyphSource for Halved {
        fn outline(&self, _font_id: u64, _code: u32) -> Option<Outline> {
            None
        }
        fn image(
            &self,
            name: &[u8],
            _request: &ImageRequest,
            _intent: RenderingIntent,
        ) -> Result<Option<DecodedImage>, String> {
            let (reduced, width, height) = match name {
                b"Im0" => (
                    Some(Reduced {
                        full_width: 5,
                        full_height: 3,
                        factor: 2,
                        skip: (0, 0),
                    }),
                    3,
                    2,
                ),
                b"Im1" => (None, 3, 2),
                // Two by one samples, a quarter each, standing for an image
                // of eight by four whose first cell starts two columns and
                // two rows in: a JPEG 2000 image at an origin inside a cell.
                b"Im2" => (
                    Some(Reduced {
                        full_width: 8,
                        full_height: 4,
                        factor: 4,
                        skip: (2, 2),
                    }),
                    2,
                    1,
                ),
                b"Im3" => (None, 2, 1),
                _ => return Ok(None),
            };
            Ok(Some(DecodedImage {
                width,
                height,
                rgb: (0..width * height * 3)
                    .map(|i| (i * 37 % 256) as u8)
                    .collect(),
                alpha: Vec::new(),
                two_colour: None,
                stencil: false,
                interpolate: false,
                reduced,
                family: ColorFamily::Rgb,
                soft_masked: false,
                window: None,
            }))
        }
    }

    /// **A reduced image is drawn over the square its samples span and cut at
    /// the image it stands for** (`docs/design/jpeg-speed.md`, "Placing the
    /// reduced grid"). Three samples standing for five, two of the image's a
    /// sample, span six of the image's five columns: drawn over its 50 by 30
    /// pixel square they are the same samples drawn as an image of their own
    /// over 60 by 40 pixels from the same top-left corner, under a clip of the
    /// 50 by 30 square — pixel for pixel, the square's edges being whole
    /// pixels — and nothing past the image's square is painted.
    #[test]
    fn a_reduced_image_is_drawn_over_its_samples_and_cut_at_the_image() {
        let draw = |content: &[u8]| {
            let canvas = Canvas::new(80, 60, PixelFormat::Rgb8, Color::WHITE);
            let mut renderer = Renderer::new(canvas, page_transform(60.0, 1.0), &Halved);
            interpret(content, Matrix::IDENTITY, &mut renderer, &NoFonts);
            let (canvas, warnings) = renderer.finish();
            assert!(warnings.is_empty(), "{warnings:?}");
            canvas
        };
        let reduced = draw(b"q 50 0 0 30 4 24 cm /Im0 Do Q");
        let twin = draw(b"q 4 24 50 30 re W n 60 0 0 40 4 14 cm /Im1 Do Q");
        assert!(
            reduced.data == twin.data,
            "the reduced grid is its samples over their own square, cut at the image"
        );
        for y in 0..60usize {
            for x in 0..80usize {
                let at = (y * 80 + x) * 3;
                let inside = (4..54).contains(&x) && (6..36).contains(&y);
                assert_eq!(
                    reduced.data[at..at + 3] != [255, 255, 255],
                    inside,
                    "({x}, {y}): painted exactly where the image is"
                );
            }
        }

        // The grid the samples stand on, and a grid that says nothing.
        let unit = Transform {
            a: 50.0,
            b: 0.0,
            c: 0.0,
            d: -30.0,
            e: 4.0,
            f: 36.0,
        };
        let grid = Reduced {
            full_width: 5,
            full_height: 3,
            factor: 2,
            skip: (0, 0),
        };
        assert_eq!(
            reduced_square(&unit, 3, 2, grid),
            Some(Transform {
                a: 60.0,
                b: 0.0,
                c: 0.0,
                d: -40.0,
                e: 4.0,
                f: 46.0,
            }),
            "six of five columns across from the left, four of three rows down from the top"
        );
        // Two samples standing for eight columns, a quarter each, the first
        // cell two columns and two rows in: the square moved right two of the
        // eight columns, a quarter of fifty pixels, and down two of the four
        // rows, half of thirty, its size the samples' own.
        assert_eq!(
            reduced_square(
                &unit,
                2,
                1,
                Reduced {
                    full_width: 8,
                    full_height: 4,
                    factor: 4,
                    skip: (2, 2),
                }
            ),
            Some(Transform {
                a: 50.0,
                b: 0.0,
                c: 0.0,
                d: -30.0,
                e: 16.5,
                f: 51.0,
            }),
            "the skip moves the square and stretches nothing"
        );
        for broken in [
            Reduced {
                full_width: 0,
                ..grid
            },
            Reduced {
                full_height: 0,
                ..grid
            },
            Reduced { factor: 0, ..grid },
        ] {
            assert_eq!(reduced_square(&unit, 3, 2, broken), None, "{broken:?}");
        }
    }

    /// **A reduced grid whose first cell starts inside the image is drawn from
    /// there** (`Reduced::skip`): two samples standing for eight columns, the
    /// first cell two columns and two rows in, drawn over a 40 by 30 pixel
    /// square, are their own image drawn over 40 by 30 pixels from a quarter
    /// of the square's width right of its top-left corner and half its height
    /// below it, wherever the two overlap; the columns and rows of the image
    /// before the first cell take the first column's and the first row's
    /// colours, as an image's own edge does; and the draw paints the unit
    /// square and nothing past it. Every figure is a whole pixel, so the two
    /// squares are one transform.
    #[test]
    fn a_reduced_grid_that_starts_inside_the_image_is_drawn_from_there() {
        let draw = |content: &[u8]| {
            let canvas = Canvas::new(80, 60, PixelFormat::Rgb8, Color::WHITE);
            let mut renderer = Renderer::new(canvas, page_transform(60.0, 1.0), &Halved);
            interpret(content, Matrix::IDENTITY, &mut renderer, &NoFonts);
            let (canvas, warnings) = renderer.finish();
            assert!(warnings.is_empty(), "{warnings:?}");
            canvas
        };
        let skipped = draw(b"q 40 0 0 30 4 24 cm /Im2 Do Q");
        let twin = draw(b"q 4 24 40 30 re W n 40 0 0 30 14 9 cm /Im3 Do Q");
        let at = |canvas: &Canvas, x: usize, y: usize| {
            let at = (y * 80 + x) * 3;
            [canvas.data[at], canvas.data[at + 1], canvas.data[at + 2]]
        };
        let mut compared = 0;
        for y in 0..60usize {
            for x in 0..80usize {
                let inside = (4..44).contains(&x) && (6..36).contains(&y);
                assert_eq!(
                    at(&skipped, x, y) != [255, 255, 255],
                    inside,
                    "({x}, {y}): painted exactly where the image is"
                );
                if !inside {
                    continue;
                }
                // Where the samples' own square is, the twin; before it, the
                // twin's first column or row.
                let want = at(&twin, x.max(14), y.max(21));
                assert_eq!(at(&skipped, x, y), want, "({x}, {y})");
                compared += 1;
            }
        }
        assert_eq!(compared, 40 * 30);
    }

    /// Every kind of recorded call, so a test of each holds them all.
    const EVERY_KIND: [EventKind; 22] = [
        EventKind::BeginText,
        EventKind::ShowGlyph,
        EventKind::EndText,
        EventKind::FillPath,
        EventKind::StrokePath,
        EventKind::ClipPath,
        EventKind::SaveState,
        EventKind::RestoreState,
        EventKind::DrawImage,
        EventKind::DrawShading,
        EventKind::BeginMarkedContent,
        EventKind::EndMarkedContent,
        EventKind::BeginForm,
        EventKind::EndForm,
        EventKind::BeginGroup,
        EventKind::EndGroup,
        EventKind::BeginSoftMask,
        EventKind::EndSoftMask,
        EventKind::ClearSoftMask,
        EventKind::GroupBudgetSpent,
        EventKind::RecursionRefused,
        EventKind::NestedBudgetSpent,
    ];

    /// `OneImage`'s image drawn ten pixels square at `(x, 10)`: a draw that
    /// joins a run, the page being far below the size a run is refused at.
    fn image_draw(x: f64) -> tinker_pdf_content::Event {
        tinker_pdf_content::Event::DrawImage {
            image: ImageRef {
                name: b"Im0".to_vec(),
                inline: false,
                inline_dict: Vec::new(),
                inline_data: Vec::new(),
            },
            state: Some(Box::new(GraphicsState::new(Matrix {
                a: 10.0,
                b: 0.0,
                c: 0.0,
                d: 10.0,
                e: x,
                f: 10.0,
            }))),
        }
    }

    /// The calls that hand the renderer one call of `kind` while it holds a
    /// run of images: an image draw, then the call — inside the bracket it
    /// ends, for a kind that ends one.
    fn holding_a_run_then(kind: EventKind) -> Vec<tinker_pdf_content::Event> {
        use tinker_pdf_content::{Event, Form, Group, MarkedScope, MaskGroup};

        let state = || Some(Box::new(GraphicsState::new(Matrix::IDENTITY)));
        let square = || {
            vec![
                PathSegment::MoveTo { x: 2.0, y: 2.0 },
                PathSegment::LineTo { x: 8.0, y: 2.0 },
                PathSegment::LineTo { x: 8.0, y: 8.0 },
                PathSegment::Close,
            ]
        };
        let group = || Event::BeginGroup {
            group: Group {
                isolated: true,
                knockout: false,
                space: None,
            },
            entered: true,
            state: state(),
        };
        let mask = || Event::BeginSoftMask {
            mask: Box::new(MaskGroup {
                form: Form {
                    content: Vec::new(),
                    matrix: Matrix::IDENTITY,
                    bbox: None,
                    group: None,
                    stream: 0,
                },
                luminosity: true,
                backdrop: None,
                transfer: None,
            }),
            bbox: Vec::new(),
            entered: true,
            state: state(),
        };
        let marked = || {
            Event::BeginMarkedContent(MarkedScope {
                tag: b"P".to_vec(),
                visible: true,
                hidden_layer: None,
                props: None,
            })
        };
        let form = || Event::BeginForm {
            id: 1,
            name: b"Fm0".to_vec(),
            entered: true,
        };
        let call = match kind {
            EventKind::BeginText => Event::BeginText,
            EventKind::ShowGlyph => Event::ShowGlyph {
                glyph: Glyph {
                    code: 65,
                    text: "A".to_string(),
                    transform: Matrix {
                        a: 12.0,
                        b: 0.0,
                        c: 0.0,
                        d: 12.0,
                        e: 30.0,
                        f: 30.0,
                    },
                    baseline: None,
                    advance: 0.6,
                    size: 12.0,
                    vertical: false,
                    font_id: 1,
                    font_name: None,
                },
                state: state(),
            },
            EventKind::FillPath => Event::FillPath {
                path: square(),
                even_odd: false,
                state: state(),
            },
            EventKind::StrokePath => Event::StrokePath {
                path: square(),
                state: state(),
            },
            EventKind::ClipPath => Event::ClipPath {
                path: square(),
                even_odd: false,
                state: state(),
            },
            EventKind::SaveState => Event::SaveState,
            EventKind::RestoreState => Event::RestoreState,
            EventKind::DrawImage => image_draw(30.0),
            EventKind::DrawShading => Event::DrawShading {
                name: b"Sh0".to_vec(),
                state: state(),
            },
            EventKind::BeginMarkedContent => marked(),
            EventKind::BeginForm => form(),
            EventKind::BeginGroup => group(),
            EventKind::BeginSoftMask => mask(),
            EventKind::ClearSoftMask => Event::ClearSoftMask,
            EventKind::GroupBudgetSpent => Event::GroupBudgetSpent,
            EventKind::RecursionRefused => Event::RecursionRefused,
            EventKind::NestedBudgetSpent => Event::NestedBudgetSpent,
            // The end of a bracket, the run opened inside it.
            EventKind::EndText => return vec![Event::BeginText, image_draw(10.0), Event::EndText],
            EventKind::EndMarkedContent => {
                return vec![marked(), image_draw(10.0), Event::EndMarkedContent]
            }
            EventKind::EndForm => return vec![form(), image_draw(10.0), Event::EndForm { id: 1 }],
            EventKind::EndGroup => return vec![group(), image_draw(10.0), Event::EndGroup],
            EventKind::EndSoftMask => return vec![mask(), image_draw(10.0), Event::EndSoftMask],
        };
        vec![image_draw(10.0), call]
    }

    /// **The band planner's rule is the renderer's flush** (`ends_image_run`):
    /// for every kind of call, a renderer holding a run of images — one drawn
    /// just before, inside the bracket a closing kind ends — has composited it
    /// after the call exactly when the rule says the kind ends a run. So the
    /// planner, which reads runs off a display list by the rule, cannot drift
    /// from the renderer, which decides them.
    #[test]
    fn the_planners_rule_is_the_renderers_flush() {
        let source = OneImage {
            color: (40, 90, 160),
            alpha: Vec::new(),
        };
        for kind in EVERY_KIND {
            let events = holding_a_run_then(kind);
            assert_eq!(
                events.last().map(tinker_pdf_content::Event::kind),
                Some(kind),
                "the case ends on its kind"
            );
            let canvas = Canvas::new(60, 60, PixelFormat::Rgb8, Color::WHITE);
            let mut renderer = Renderer::new(canvas, page_transform(60.0, 1.0), &source);
            let replayed = tinker_pdf_content::replay(&events, &mut renderer);
            assert_eq!(
                replayed.delivered,
                events.len(),
                "{kind:?}: every call delivered"
            );
            assert_eq!(
                renderer.run.is_none(),
                ends_image_run(kind),
                "{kind:?}: the renderer {} the run, and the rule says it {}",
                if renderer.run.is_none() {
                    "composited"
                } else {
                    "still holds"
                },
                if ends_image_run(kind) {
                    "ends it"
                } else {
                    "does not"
                }
            );
        }
    }

    /// A source whose `Im0` and `Im1` are one bilevel image, held as its two
    /// colours or as their RGB expansion.
    struct Bilevel {
        two_colour: bool,
        stencil: bool,
        interpolate: bool,
        colours: [[u8; 4]; 2],
        width: u32,
        height: u32,
        samples: Vec<u8>,
    }

    impl GlyphSource for Bilevel {
        fn outline(&self, _font_id: u64, _code: u32) -> Option<Outline> {
            None
        }
        fn image(
            &self,
            name: &[u8],
            _request: &ImageRequest,
            _intent: RenderingIntent,
        ) -> Result<Option<DecodedImage>, String> {
            if name != b"Im0" && name != b"Im1" {
                return Ok(None);
            }
            let (width, height) = (self.width, self.height);
            let (stencil, interpolate) = (self.stencil, self.interpolate);
            if self.two_colour {
                return Ok(Some(DecodedImage {
                    width,
                    height,
                    rgb: Vec::new(),
                    alpha: Vec::new(),
                    two_colour: Some(TwoColourSamples {
                        samples: self.samples.clone(),
                        colours: self.colours,
                    }),
                    stencil,
                    interpolate,
                    reduced: None,
                    family: if stencil {
                        ColorFamily::Other
                    } else {
                        ColorFamily::Rgb
                    },
                    soft_masked: false,
                    window: None,
                }));
            }
            let mut rgb = Vec::new();
            let mut alpha = Vec::new();
            for &sample in &self.samples {
                let [r, g, b, a] = self.colours[usize::from(sample != 0)];
                rgb.extend_from_slice(&[r, g, b]);
                alpha.push(a);
            }
            Ok(Some(DecodedImage {
                width,
                height,
                rgb,
                alpha,
                two_colour: None,
                stencil,
                interpolate,
                reduced: None,
                family: if stencil {
                    ColorFamily::Other
                } else {
                    ColorFamily::Rgb
                },
                soft_masked: false,
                window: None,
            }))
        }
    }

    /// **`blit` draws a bilevel image held as two colours as its RGB
    /// expansion.**
    ///
    /// One 23 by 17 image of runs and noise — some of its bytes neither 0 nor
    /// 1, and its buffer cut short in one case — drawn magnified, minified,
    /// as a run of two abutting strips, rotated and past 128:1, as an image
    /// and as a stencil in the fill colour and its ink, onto an RGB canvas
    /// and an ink one, for four colour pairs — one with a transparent colour
    /// each way round — and both values of `/Interpolate`: the canvases are
    /// byte for byte the same, and so are the warnings.
    #[test]
    fn a_two_colour_image_blits_as_its_rgb_expansion() {
        let (width, height) = (23u32, 17u32);
        let mut state = 7u32;
        let full: Vec<u8> = (0..width * height)
            .map(|i| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                match i % 9 {
                    0..=3 => u8::from((i / 4) % 3 == 0),
                    4 => (state >> 24) as u8,
                    _ => u8::from(state >> 31 == 1),
                }
            })
            .collect();
        let content: &[u8] = b"0.1 0.9 0.6 0.05 k \
            q 46 0 0 34 2.3 61.1 cm /Im0 Do Q \
            q 11.5 0 0 8.5 51.7 80.2 cm /Im0 Do Q \
            q 7 0 0 5 60 60 cm /Im1 Do Q q 7 0 0 5 67 60 cm /Im1 Do Q \
            q 20 11 -11 20 30 5 cm /Im0 Do Q \
            q 0.1 0 0 0.1 70.3 30.4 cm /Im0 Do Q \
            q 30 0 0 20 50 2 cm /Im0 Do Q";
        let pairs = [
            [[0, 0, 0, 255], [255, 255, 255, 255]],
            [[31, 58, 147, 255], [242, 201, 76, 255]],
            [[0, 0, 0, 255], [0, 0, 0, 0]],
            [[0, 0, 0, 0], [250, 9, 128, 255]],
        ];
        let mut renders = 0;
        for stencil in [false, true] {
            for interpolate in [false, true] {
                for colours in pairs {
                    for samples in [&full[..], &full[..full.len() * 2 / 3]] {
                        for format in [PixelFormat::Rgb8, PixelFormat::CmykA8] {
                            let render = |two_colour: bool| {
                                let source = Bilevel {
                                    two_colour,
                                    stencil,
                                    interpolate,
                                    colours,
                                    width,
                                    height,
                                    samples: samples.to_vec(),
                                };
                                let background = if format == PixelFormat::CmykA8 {
                                    Color::TRANSPARENT
                                } else {
                                    Color::WHITE
                                };
                                let canvas = Canvas::new(96, 96, format, background);
                                let mut renderer =
                                    Renderer::new(canvas, page_transform(96.0, 1.0), &source);
                                interpret(content, Matrix::IDENTITY, &mut renderer, &NoFonts);
                                let (canvas, warnings) = renderer.finish();
                                (canvas.data, warnings)
                            };
                            let (two, rgb) = (render(true), render(false));
                            assert!(
                                two == rgb,
                                "stencil {stencil}, interpolate {interpolate}, colours \
                                 {colours:?}, {} samples on {format:?}: the two forms part",
                                samples.len()
                            );
                            assert!(
                                two.0.iter().any(|byte| *byte != two.0[0]),
                                "the draws painted something"
                            );
                            renders += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(renders, 64);
    }

    fn render_with(content: &[u8], size: u32, source: &OneImage) -> (Canvas, Vec<RenderWarning>) {
        let canvas = Canvas::new(size, size, PixelFormat::Rgb8, Color::WHITE);
        let base = page_transform(f64::from(size), 1.0);
        let mut renderer = Renderer::new(canvas, base, source);
        interpret(content, Matrix::IDENTITY, &mut renderer, &NoFonts);
        renderer.finish()
    }

    #[test]
    fn an_image_lands_in_the_unit_square_of_its_transform() {
        let source = OneImage {
            color: (255, 0, 0),
            alpha: Vec::new(),
        };
        // Ten by ten at the origin: the bottom-left corner in PDF space.
        let (canvas, warnings) = render_with(b"q 10 0 0 10 0 0 cm /Im0 Do Q", 20, &source);

        assert!(warnings.is_empty(), "a decodable image warns about nothing");
        assert_eq!(
            canvas.pixel(5, 15),
            Some(Color::rgb(255, 0, 0)),
            "inside the placement"
        );
        assert_eq!(
            canvas.pixel(15, 5),
            Some(Color::WHITE),
            "outside it, untouched"
        );
    }

    #[test]
    fn an_images_alpha_channel_is_honoured() {
        let source = OneImage {
            color: (0, 0, 0),
            // The two left samples transparent, the two right opaque.
            alpha: vec![0, 255, 0, 255],
        };
        let (canvas, _) = render_with(b"q 20 0 0 20 0 0 cm /Im0 Do Q", 20, &source);

        assert_eq!(
            canvas.pixel(2, 10),
            Some(Color::WHITE),
            "a transparent sample leaves the page alone"
        );
        assert_eq!(
            canvas.pixel(17, 10),
            Some(Color::BLACK),
            "an opaque one paints"
        );
    }

    #[test]
    fn a_degenerate_image_transform_draws_nothing() {
        let source = OneImage {
            color: (255, 0, 0),
            alpha: Vec::new(),
        };
        // A zero-area transform is not invertible.
        let (canvas, _) = render_with(b"q 0 0 0 0 5 5 cm /Im0 Do Q", 20, &source);
        assert_eq!(canvas.pixel(5, 15), Some(Color::WHITE));
    }

    #[test]
    fn an_unknown_image_still_warns() {
        let source = OneImage {
            color: (255, 0, 0),
            alpha: Vec::new(),
        };
        let (_, warnings) = render_with(b"q 10 0 0 10 0 0 cm /Missing Do Q", 20, &source);
        assert!(warnings
            .iter()
            .any(|w| matches!(w, RenderWarning::UnsupportedImage { .. })));
    }

    struct OneShading;

    impl GlyphSource for OneShading {
        fn outline(&self, _font_id: u64, _code: u32) -> Option<Outline> {
            None
        }
        fn shading(&self, name: &[u8]) -> Result<Option<Shading>, i64> {
            match name {
                b"Sh0" => Ok(Some(Shading::Axial {
                    space: tinker_pdf_color::ColorSpace::DeviceGray,
                    function: tinker_pdf_color::Function::Exponential {
                        domain: (0.0, 1.0),
                        c0: vec![0.0],
                        c1: vec![1.0],
                        n: 1.0,
                    },
                    coords: [0.0, 0.0, 20.0, 0.0],
                    extend: (true, true),
                })),
                b"Mesh" => Err(4),
                _ => Ok(None),
            }
        }
    }

    #[test]
    fn a_shading_paints_a_gradient() {
        let canvas = Canvas::new(20, 20, PixelFormat::Rgb8, Color::WHITE);
        let mut renderer = Renderer::new(canvas, page_transform(20.0, 1.0), &OneShading);
        interpret(b"/Sh0 sh", Matrix::IDENTITY, &mut renderer, &NoFonts);
        let (canvas, warnings) = renderer.finish();

        assert!(warnings.is_empty());
        let left = canvas.pixel(1, 10).expect("a pixel").r;
        let right = canvas.pixel(18, 10).expect("a pixel").r;
        assert!(
            right > left + 100,
            "the gradient should run dark to light, got {left} then {right}"
        );
    }

    #[test]
    fn a_shading_is_confined_by_the_clip() {
        let canvas = Canvas::new(20, 20, PixelFormat::Rgb8, Color::WHITE);
        let mut renderer = Renderer::new(canvas, page_transform(20.0, 1.0), &OneShading);
        interpret(
            b"0 0 10 20 re W n /Sh0 sh",
            Matrix::IDENTITY,
            &mut renderer,
            &NoFonts,
        );
        let (canvas, _) = renderer.finish();

        assert_ne!(canvas.pixel(5, 10), Some(Color::WHITE), "inside the clip");
        assert_eq!(canvas.pixel(15, 10), Some(Color::WHITE), "outside it");
    }

    #[test]
    fn an_unpaintable_shading_type_warns() {
        let canvas = Canvas::new(8, 8, PixelFormat::Rgb8, Color::WHITE);
        let mut renderer = Renderer::new(canvas, page_transform(8.0, 1.0), &OneShading);
        interpret(b"/Mesh sh", Matrix::IDENTITY, &mut renderer, &NoFonts);
        let (_, warnings) = renderer.finish();

        assert!(warnings
            .iter()
            .any(|w| matches!(w, RenderWarning::UnsupportedShading { kind: 4 })));
    }

    /// A red-to-blue ramp across a 40 pt page, and one glyph shaped like a
    /// tall box, so a stroked glyph has two vertical bars far enough apart for
    /// the ramp to have moved between them.
    ///
    /// `Tile` stands for everything `PageResources` reports as unpaintable —
    /// today every tiling pattern, and a mesh inside a shading pattern.
    struct Patterns;

    impl GlyphSource for Patterns {
        fn outline(&self, _font_id: u64, _code: u32) -> Option<Outline> {
            use tinker_pdf_font::Segment;
            // One em is one unit here, which is the contract of this method.
            Some(Outline {
                segments: vec![
                    Segment::MoveTo { x: 0.1, y: 0.0 },
                    Segment::LineTo { x: 0.6, y: 0.0 },
                    Segment::LineTo { x: 0.6, y: 0.7 },
                    Segment::LineTo { x: 0.1, y: 0.7 },
                    Segment::Close,
                ],
            })
        }

        fn pattern(&self, name: &[u8]) -> Option<PatternPaint> {
            match name {
                b"Grad" => Some(PatternPaint::Shading(
                    Box::new(Shading::Axial {
                        space: tinker_pdf_color::ColorSpace::DeviceRgb,
                        function: tinker_pdf_color::Function::Exponential {
                            domain: (0.0, 1.0),
                            c0: vec![1.0, 0.0, 0.0],
                            c1: vec![0.0, 0.0, 1.0],
                            n: 1.0,
                        },
                        coords: [0.0, 0.0, 40.0, 0.0],
                        extend: (true, true),
                    }),
                    Matrix::IDENTITY,
                )),
                b"Tile" => Some(PatternPaint::Unsupported),
                _ => None,
            }
        }
    }

    /// A tiling pattern whose cell is painted straight into the buffer
    /// instead of by a content stream.
    ///
    /// This crate has no interpreter and cannot run one — that is the whole
    /// reason [`GlyphSource::tile`] exists — and it is not what these tests
    /// are about anyway. What they are about is the lattice: which cells are
    /// asked for, where each one lands, and how many times the cell is drawn.
    struct Tiles {
        pattern: TilingPattern,
        /// The colour the cell fills its whole buffer with, so that a step
        /// wider than the box leaves a visible gap and a narrower one does
        /// not.
        cell: Color,
        /// How many times the cell was rasterized, which for a whole lattice
        /// must be one.
        drawn: std::cell::Cell<u32>,
        /// The size of the last buffer asked for.
        size: std::cell::Cell<(u32, u32)>,
    }

    impl Tiles {
        fn new(bbox: [f64; 4], xstep: f64, ystep: f64) -> Tiles {
            Tiles {
                pattern: TilingPattern {
                    matrix: Matrix::IDENTITY,
                    bbox,
                    xstep,
                    ystep,
                    uncolored: false,
                },
                cell: Color::rgb(0, 0, 255),
                drawn: std::cell::Cell::new(0),
                size: std::cell::Cell::new((0, 0)),
            }
        }
    }

    impl GlyphSource for Tiles {
        fn outline(&self, _font_id: u64, _code: u32) -> Option<Outline> {
            None
        }

        fn pattern(&self, name: &[u8]) -> Option<PatternPaint> {
            (name == b"T0").then(|| PatternPaint::Tiling(Box::new(self.pattern)))
        }

        fn tile(&self, name: &[u8], request: &TileRequest) -> Option<Tile> {
            if name != b"T0" {
                return None;
            }
            self.drawn.set(self.drawn.get() + 1);
            self.size.set((request.width, request.height));
            Some(Tile {
                canvas: Canvas::new(request.width, request.height, request.format, self.cell),
                opaque: true,
                warnings: Vec::new(),
            })
        }
    }

    fn with_tiles(content: &[u8], tiles: &Tiles) -> (Canvas, Vec<RenderWarning>) {
        let canvas = Canvas::new(40, 40, PixelFormat::Rgb8, Color::WHITE);
        let mut renderer = Renderer::new(canvas, page_transform(40.0, 1.0), tiles);
        interpret(content, Matrix::IDENTITY, &mut renderer, &OneChar);
        renderer.finish()
    }

    /// A `PaintType 2` pattern whose cells a test watches being asked for:
    /// each request kept, and each cell drawn in a green of its own on its
    /// left half and left empty on its right.
    struct Uncoloured {
        requests: std::cell::RefCell<Vec<TileRequest>>,
    }

    impl GlyphSource for Uncoloured {
        fn outline(&self, _font_id: u64, _code: u32) -> Option<Outline> {
            None
        }

        fn pattern(&self, name: &[u8]) -> Option<PatternPaint> {
            (name == b"U0").then(|| {
                PatternPaint::Tiling(Box::new(TilingPattern {
                    matrix: Matrix::IDENTITY,
                    bbox: [0.0, 0.0, 4.0, 4.0],
                    xstep: 4.0,
                    ystep: 4.0,
                    uncolored: true,
                }))
            })
        }

        fn tile(&self, name: &[u8], request: &TileRequest) -> Option<Tile> {
            if name != b"U0" {
                return None;
            }
            self.requests.borrow_mut().push(request.clone());
            let mut canvas = Canvas::new(
                request.width,
                request.height,
                request.format,
                Color::TRANSPARENT,
            );
            for y in 0..request.height {
                for x in 0..request.width / 2 {
                    canvas.blend_pixel(x, y, Color::rgb(0, 255, 0), 1.0);
                }
            }
            Some(Tile {
                canvas,
                opaque: true,
                warnings: Vec::new(),
            })
        }
    }

    /// **An uncoloured cell is asked for without its colour.** One
    /// `PaintType 2` pattern filled in red over the page's lower half and in
    /// blue over its upper half asks for two cells whose requests differ in
    /// no field, since a cell is a shape and its colour is the blit's; and the
    /// page still takes each fill's colour where the cells painted, white
    /// where they did not, and the cells' own green nowhere.
    #[test]
    fn an_uncoloured_cell_is_asked_for_without_its_colour() {
        let source = Uncoloured {
            requests: std::cell::RefCell::new(Vec::new()),
        };
        let canvas = Canvas::new(40, 40, PixelFormat::Rgb8, Color::WHITE);
        let mut renderer = Renderer::new(canvas, page_transform(40.0, 1.0), &source);
        interpret(
            b"1 0 0 rg /U0 scn 0 0 40 20 re f 0 0 1 rg /U0 scn 0 20 40 20 re f",
            Matrix::IDENTITY,
            &mut renderer,
            &OneChar,
        );
        let (canvas, warnings) = renderer.finish();
        assert!(warnings.is_empty(), "{warnings:?}");

        let requests = source.requests.borrow();
        assert_eq!(requests.len(), 2, "a cell for each fill");
        let fields = |r: &TileRequest| {
            (
                r.to_pixels,
                r.bbox,
                r.width,
                r.height,
                r.format,
                r.depth,
                r.antialias,
            )
        };
        assert_eq!(
            fields(&requests[0]),
            fields(&requests[1]),
            "the red fill's cell and the blue fill's"
        );

        // Each cell paints the left half of its four pixels across.
        let red = Color::rgb(255, 0, 0);
        let blue = Color::rgb(0, 0, 255);
        for x in (0..40).step_by(4) {
            assert_eq!(canvas.pixel(x, 30), Some(red), "the lower fill at x {x}");
            assert_eq!(canvas.pixel(x + 1, 30), Some(red), "x {}", x + 1);
            assert_eq!(canvas.pixel(x + 2, 30), Some(Color::WHITE), "x {}", x + 2);
            assert_eq!(canvas.pixel(x, 10), Some(blue), "the upper fill at x {x}");
            assert_eq!(canvas.pixel(x + 3, 10), Some(Color::WHITE), "x {}", x + 3);
        }
        for y in 0..40 {
            for x in 0..40 {
                assert_ne!(
                    canvas.pixel(x, y).map(|c| (c.r, c.g, c.b)),
                    Some((0, 255, 0)),
                    "the cell's own colour shows at ({x}, {y})"
                );
            }
        }
    }

    /// The point of the whole design: one rasterization, however many lattice
    /// positions it lands on.
    ///
    /// Replaying the cell's content stream per position is correct and
    /// unusable — each position needs its own bounding-box clip, every clip
    /// clones a page-sized mask, and a 10 pt hatch over A4 is about 4 800 of
    /// them. Nothing about the *pixels* can tell the two apart, so this counts
    /// instead.
    #[test]
    fn a_cell_is_rasterised_once_for_the_whole_lattice() {
        let tiles = Tiles::new([0.0, 0.0, 4.0, 4.0], 8.0, 8.0);
        let (canvas, warnings) = with_tiles(b"/Pattern cs /T0 scn 0 0 40 40 re f", &tiles);

        assert_eq!(
            tiles.drawn.get(),
            1,
            "twenty-five lattice positions, one rasterization"
        );
        assert_eq!(
            tiles.size.get(),
            (4, 4),
            "and the buffer is the cell's own bounding box, not the page"
        );
        assert_eq!(
            canvas.pixel(2, 37),
            Some(Color::rgb(0, 0, 255)),
            "the cell at the pattern origin lands at the page's bottom-left"
        );
        assert!(warnings.is_empty(), "nothing to report: {warnings:?}");
    }

    /// A `GlyphSource` that reports a tiling pattern and cannot draw one is
    /// the default implementation of [`GlyphSource::tile`], and it has to
    /// degrade the way an unpaintable pattern always has.
    #[test]
    fn a_tiling_pattern_nobody_can_rasterise_is_reported() {
        struct Geometry;
        impl GlyphSource for Geometry {
            fn outline(&self, _font_id: u64, _code: u32) -> Option<Outline> {
                None
            }
            fn pattern(&self, _name: &[u8]) -> Option<PatternPaint> {
                Some(PatternPaint::Tiling(Box::new(TilingPattern {
                    matrix: Matrix::IDENTITY,
                    bbox: [0.0, 0.0, 8.0, 8.0],
                    xstep: 8.0,
                    ystep: 8.0,
                    uncolored: false,
                })))
            }
        }

        let canvas = Canvas::new(40, 40, PixelFormat::Rgb8, Color::WHITE);
        let mut renderer = Renderer::new(canvas, page_transform(40.0, 1.0), &Geometry);
        interpret(
            b"/Pattern cs /T0 scn 0 0 40 40 re f",
            Matrix::IDENTITY,
            &mut renderer,
            &OneChar,
        );
        let (canvas, warnings) = renderer.finish();

        assert_eq!(
            canvas.pixel(20, 20),
            Some(Color::WHITE),
            "the area is left alone"
        );
        assert!(
            warnings.iter().any(|w| matches!(
                w,
                RenderWarning::UnsupportedPattern { name } if name == "T0"
            )),
            "and named: {warnings:?}"
        );
    }

    /// 8.7.3.2: the lattice steps by `/XStep` and `/YStep`, which are not the
    /// bounding box. A step wider than the box leaves a gap, and the gap is
    /// what the file asked for — filling it is the failure a cell sized to its
    /// step instead of to its box produces.
    ///
    /// The page is 40 pt; a 10 pt cell every 20 pt puts cells at device x 0,
    /// 20 and 40, and at device y 30 and 10 once the y flip is applied.
    #[test]
    fn the_lattice_repeats_at_its_steps_and_leaves_their_gaps() {
        let tiles = Tiles::new([0.0, 0.0, 10.0, 10.0], 20.0, 20.0);
        let (canvas, _) = with_tiles(b"/Pattern cs /T0 scn 0 0 40 40 re f", &tiles);

        let blue = Some(Color::rgb(0, 0, 255));
        for (x, y) in [(5, 35), (25, 35), (5, 15), (25, 15)] {
            assert_eq!(canvas.pixel(x, y), blue, "a cell covers ({x}, {y})");
        }
        for (x, y) in [(15, 35), (35, 35), (15, 15), (5, 25)] {
            assert_eq!(
                canvas.pixel(x, y),
                Some(Color::WHITE),
                "and the step leaves ({x}, {y}) alone"
            );
        }
    }

    /// The opposite arrangement: a step *narrower* than the box, which is how
    /// a weave or an overlapping scale pattern is drawn. Every pixel of the
    /// shape is covered, and the cells overlapping does not punch holes.
    #[test]
    fn a_step_narrower_than_the_box_overlaps_rather_than_tearing() {
        let tiles = Tiles::new([0.0, 0.0, 10.0, 10.0], 5.0, 5.0);
        let (canvas, _) = with_tiles(b"/Pattern cs /T0 scn 0 0 40 40 re f", &tiles);

        for y in 0..40 {
            for x in 0..40 {
                assert_eq!(
                    canvas.pixel(x, y),
                    Some(Color::rgb(0, 0, 255)),
                    "({x}, {y}) is covered by an overlapping lattice"
                );
            }
        }
    }

    /// The lattice is indexed from the *pattern's* origin, which may be
    /// anywhere — including outside the shape, so the indices run negative.
    ///
    /// This is also where an off-by-one in the range shows. Cell zero here
    /// spans device x -5 to 5, so it only half overlaps the shape; a range
    /// that pairs the near edge of the shape with the near edge of the cell
    /// rather than with its far edge starts at cell one instead, and the
    /// leftmost five columns of every shape in the document go blank. That
    /// reads as a margin, not as a missing cell.
    #[test]
    fn a_cell_that_only_half_overlaps_the_shape_is_still_drawn() {
        let mut tiles = Tiles::new([0.0, 0.0, 10.0, 10.0], 20.0, 20.0);
        tiles.pattern.matrix = Matrix::translate(-5.0, -5.0);
        let (canvas, _) = with_tiles(b"/Pattern cs /T0 scn 0 0 40 40 re f", &tiles);

        let blue = Some(Color::rgb(0, 0, 255));
        assert_eq!(
            canvas.pixel(2, 37),
            blue,
            "the half of cell zero that is on the page is drawn"
        );
        assert_eq!(
            canvas.pixel(37, 2),
            blue,
            "and so is the far corner, five steps out"
        );
    }

    /// Nothing paints outside the filled path. The lattice covers the path's
    /// bounding *box*, so a shape that is not a rectangle is the case that
    /// matters: the coverage mask is what shapes each blit, and a blit that
    /// ignored it would paint the box.
    #[test]
    fn the_lattice_paints_only_where_the_path_covers() {
        let tiles = Tiles::new([0.0, 0.0, 8.0, 8.0], 8.0, 8.0);
        // A triangle across the page: the top-left corner is well outside it.
        let (canvas, _) = with_tiles(b"/Pattern cs /T0 scn 0 0 m 40 0 l 40 40 l h f", &tiles);

        assert_eq!(
            canvas.pixel(35, 35),
            Some(Color::rgb(0, 0, 255)),
            "inside the triangle"
        );
        assert_eq!(
            canvas.pixel(4, 4),
            Some(Color::WHITE),
            "outside it, inside its bounding box — a blit that ignored the \
             coverage mask would paint here"
        );
    }

    /// A clip bounds the lattice exactly as the path does, because both reach
    /// it through the one coverage mask.
    #[test]
    fn a_clip_bounds_the_lattice_too() {
        let tiles = Tiles::new([0.0, 0.0, 8.0, 8.0], 8.0, 8.0);
        let (canvas, _) = with_tiles(
            b"q 0 0 20 20 re W n /Pattern cs /T0 scn 0 0 40 40 re f Q",
            &tiles,
        );

        assert_eq!(
            canvas.pixel(10, 30),
            Some(Color::rgb(0, 0, 255)),
            "inside the clip"
        );
        assert_eq!(canvas.pixel(30, 10), Some(Color::WHITE), "outside it");
    }

    /// 8.7.3.2 puts no floor under `/XStep`, so a file may ask for a lattice
    /// nobody can draw. A thousandth-of-a-point step across 40 pt is 1.6
    /// billion cells; on A4 at 300 dpi it is very much worse.
    ///
    /// Past the cap the fill paints nothing and says so (ruling 2). Painting
    /// *some* of the lattice is not one of the answers: a hatch that covers
    /// the top third of a shape reads as a rendering artefact, where an
    /// unpainted area reads as the gap it is.
    ///
    /// The cell is never rasterized, which is the half that makes the cap a
    /// bound rather than a consolation — the lattice is counted from six
    /// numbers, before anything is drawn.
    #[test]
    fn a_step_too_small_to_draw_warns_rather_than_hanging() {
        let tiles = Tiles::new([0.0, 0.0, 1.0, 1.0], 0.001, 0.001);
        let (canvas, warnings) = with_tiles(b"/Pattern cs /T0 scn 0 0 40 40 re f", &tiles);

        assert_eq!(tiles.drawn.get(), 0, "no cell was rasterized");
        assert_eq!(
            canvas.pixel(20, 20),
            Some(Color::WHITE),
            "and nothing was painted"
        );
        assert!(
            warnings.iter().any(|w| matches!(
                w,
                RenderWarning::UnsupportedPattern { name } if name == "T0"
            )),
            "and the pattern is named: {warnings:?}"
        );
    }

    /// A cell whose bounding box is enormous is a memory bound rather than a
    /// time one: the buffer is allocated before a single lattice position is
    /// composited.
    #[test]
    fn a_cell_too_large_to_buffer_warns() {
        let tiles = Tiles::new([0.0, 0.0, 200_000.0, 200_000.0], 200_000.0, 200_000.0);
        let (canvas, warnings) = with_tiles(b"/Pattern cs /T0 scn 0 0 40 40 re f", &tiles);

        assert_eq!(tiles.drawn.get(), 0, "the buffer was never allocated");
        assert_eq!(canvas.pixel(20, 20), Some(Color::WHITE));
        assert!(
            warnings
                .iter()
                .any(|w| matches!(w, RenderWarning::UnsupportedPattern { .. })),
            "and it is reported: {warnings:?}"
        );
    }

    /// The lattice count alone does not bound the work, and this is the case
    /// that shows it: a cell as large as the page, stepped a hundredth of a
    /// point, is about 24 000 positions — comfortably inside the cell cap —
    /// each of which costs a whole page of compositing rather than a step's
    /// worth. That is 39 megapixels for one fill, and the file may have
    /// thousands.
    #[test]
    fn a_lattice_inside_the_cell_cap_can_still_be_too_much_work() {
        let tiles = Tiles::new([0.0, 0.0, 40.0, 40.0], 0.01, 40.0);
        let (canvas, warnings) = with_tiles(b"/Pattern cs /T0 scn 0 0 40 40 re f", &tiles);

        assert_eq!(tiles.drawn.get(), 0);
        assert_eq!(canvas.pixel(20, 20), Some(Color::WHITE));
        assert!(
            warnings
                .iter()
                .any(|w| matches!(w, RenderWarning::UnsupportedPattern { .. })),
            "the work budget refuses it: {warnings:?}"
        );
    }

    /// A lattice just inside the caps still paints, so the guards above are
    /// not passing by refusing everything.
    #[test]
    fn a_lattice_inside_the_caps_still_paints() {
        let tiles = Tiles::new([0.0, 0.0, 2.0, 2.0], 2.0, 2.0);
        let (canvas, warnings) = with_tiles(b"/Pattern cs /T0 scn 0 0 40 40 re f", &tiles);

        assert_eq!(tiles.drawn.get(), 1, "441 positions, one rasterization");
        assert_eq!(canvas.pixel(20, 20), Some(Color::rgb(0, 0, 255)));
        assert!(warnings.is_empty(), "nothing to report: {warnings:?}");
    }

    /// Whether a lattice is affordable is a question about the pattern, so
    /// every scale answers it alike. A lattice of 1 503 positions, each a
    /// whole page, fits the work budget at 72 dpi and is drawn at 288 too,
    /// where its pixels are sixteen times as many — counted in pixels, the
    /// same page lost its pattern from twice its scale up — and the lattice
    /// of 24 000 that the budget refuses is refused at both.
    #[test]
    fn a_lattice_is_drawn_or_refused_alike_at_every_scale() {
        for scale in [1.0, 4.0] {
            let side = (40.0 * scale) as u32;
            for (xstep, drawn) in [(0.16, true), (0.01, false)] {
                let tiles = Tiles::new([0.0, 0.0, 40.0, 40.0], xstep, 40.0);
                let canvas = Canvas::new(side, side, PixelFormat::Rgb8, Color::WHITE);
                let mut renderer = Renderer::new(canvas, page_transform(40.0, scale), &tiles);
                interpret(
                    b"/Pattern cs /T0 scn 0 0 40 40 re f",
                    Matrix::IDENTITY,
                    &mut renderer,
                    &OneChar,
                );
                let (canvas, warnings) = renderer.finish();
                let refused = warnings
                    .iter()
                    .any(|w| matches!(w, RenderWarning::UnsupportedPattern { .. }));
                assert_eq!(refused, !drawn, "step {xstep} at {scale}x: {warnings:?}");
                let ink = if drawn {
                    Color::rgb(0, 0, 255)
                } else {
                    Color::WHITE
                };
                assert_eq!(
                    canvas.pixel(side / 2, side / 2),
                    Some(ink),
                    "step {xstep} at {scale}x"
                );
            }
        }
    }

    /// A stroke painted with a tiling pattern takes its lattice and its
    /// coverage from the outline the pen wrote into the scratch, which is
    /// never a path: the cell is drawn once, the line is the cell's colour
    /// from end to end, and nothing beside it is painted — under a
    /// similarity, stroked in device space, and under a stretch, stroked in
    /// user space and carried out.
    #[test]
    fn a_tiling_pattern_stroke_paints_its_outline_and_nothing_else() {
        for (content, what) in [
            (
                &b"/Pattern CS /T0 SCN 6 w 4 20 m 36 20 l S"[..],
                "a similarity",
            ),
            (
                &b"/Pattern CS /T0 SCN 1 0 0 3 0 0 cm 2 w 4 6.6666667 m 36 6.6666667 l S"[..],
                "a stretch",
            ),
        ] {
            let tiles = Tiles::new([0.0, 0.0, 2.0, 2.0], 2.0, 2.0);
            let (canvas, warnings) = with_tiles(content, &tiles);
            assert_eq!(
                tiles.drawn.get(),
                1,
                "{what}: one rasterization of the cell"
            );
            for x in [6, 20, 34] {
                assert_eq!(
                    canvas.pixel(x, 20),
                    Some(Color::rgb(0, 0, 255)),
                    "{what}: the line at x = {x}"
                );
            }
            assert_eq!(canvas.pixel(20, 12), Some(Color::WHITE), "{what}: above it");
            assert_eq!(canvas.pixel(20, 28), Some(Color::WHITE), "{what}: below it");
            assert_eq!(canvas.pixel(2, 20), Some(Color::WHITE), "{what}: before it");
            assert!(
                warnings.is_empty(),
                "{what}: nothing to report: {warnings:?}"
            );
        }
    }

    /// One byte, one code, half an em wide.
    struct OneChar;

    impl tinker_pdf_content::FontSource for OneChar {
        fn decode(&self, _font: &[u8], bytes: &[u8]) -> Vec<(u32, String, f64)> {
            bytes
                .iter()
                .map(|&b| (u32::from(b), char::from(b).to_string(), 500.0))
                .collect()
        }
        fn vertical_metrics(&self, _font: &[u8], _code: u32) -> (f64, f64, f64) {
            (0.0, 880.0, -1000.0)
        }
    }

    fn with_patterns(content: &[u8]) -> (Canvas, Vec<RenderWarning>) {
        let canvas = Canvas::new(40, 40, PixelFormat::Rgb8, Color::WHITE);
        let mut renderer = Renderer::new(canvas, page_transform(40.0, 1.0), &Patterns);
        interpret(content, Matrix::IDENTITY, &mut renderer, &OneChar);
        renderer.finish()
    }

    /// 9.3.6 mode 1 strokes the glyph outline, and 8.7.3 says what with. The
    /// stroking colour for `/Pattern` is the nominal black the colour crate
    /// reports, so a fallback to it is unmistakable here: the ramp is red at
    /// one end and blue at the other, and neither is black.
    #[test]
    fn a_pattern_stroked_glyph_shows_the_pattern() {
        // The box spans 0.1..0.6 em; at 40 pt from x = 2 that puts its two
        // vertical bars at device x = 6 and x = 26.
        let (canvas, warnings) =
            with_patterns(b"/Pattern CS /Grad SCN 4 w BT /F0 40 Tf 1 Tr 2 4 Td (A) Tj ET");

        let left = canvas.pixel(6, 22).expect("a pixel");
        let right = canvas.pixel(26, 22).expect("a pixel");
        assert!(
            left.r > 150 && left.b < 110,
            "the left bar takes the red end of the ramp, got {left:?} — \
             black is the stroking colour of a pattern space"
        );
        assert!(
            right.b > 150 && right.r < 110,
            "and the right bar the blue end, got {right:?}"
        );
        assert!(warnings.is_empty(), "nothing to report: {warnings:?}");
    }

    /// The same glyph with a pattern this build cannot paint. It must degrade
    /// the way a fill does — nothing painted, and a warning naming it — rather
    /// than stroking the black that `/Pattern` nominally reports.
    #[test]
    fn a_glyph_stroked_with_an_unpaintable_pattern_warns() {
        let (canvas, warnings) =
            with_patterns(b"/Pattern CS /Tile SCN 4 w BT /F0 40 Tf 1 Tr 2 4 Td (A) Tj ET");

        assert_eq!(
            canvas.pixel(6, 22),
            Some(Color::WHITE),
            "the glyph is left alone rather than stroked black"
        );
        assert!(
            warnings.iter().any(|w| matches!(
                w,
                RenderWarning::UnsupportedPattern { name } if name == "Tile"
            )),
            "and the gap is named: {warnings:?}"
        );
    }

    /// Mode 2 fills *and* strokes one glyph. Routing only the stroke would
    /// paint a black body under a patterned edge, which reads as a rendering
    /// bug rather than as a missing capability.
    #[test]
    fn a_pattern_filled_and_stroked_glyph_has_no_black_in_it() {
        let (canvas, _) = with_patterns(
            b"/Pattern cs /Grad scn /Pattern CS /Grad SCN 4 w \
              BT /F0 40 Tf 2 Tr 2 4 Td (A) Tj ET",
        );

        // Inside the box, away from both bars.
        let body = canvas.pixel(16, 22).expect("a pixel");
        assert!(
            body.r + body.b > 150,
            "the glyph body carries the ramp, got {body:?} — \
             (0, 0, 0) is the fill colour of a pattern space"
        );
    }

    /// A stroked outline is filled non-zero, always. Two subpaths of one path
    /// crossing at right angles overlap where they meet; under the even-odd
    /// rule that overlap is a hole, and a wide-stroked cross comes out with a
    /// white square in the middle of it.
    ///
    /// Asserted rather than left to the default, because a default that
    /// happens to be right is not a guarantee.
    #[test]
    fn a_self_crossing_pattern_stroke_fills_its_overlap() {
        let (canvas, _) = with_patterns(b"/Pattern CS /Grad SCN 8 w 5 5 m 35 35 l 5 35 m 35 5 l S");

        let centre = canvas.pixel(20, 20).expect("a pixel");
        assert_ne!(
            centre,
            Color::WHITE,
            "the crossing is painted, not punched out"
        );
        // The ramp is halfway across at the centre, so both ends contribute.
        assert!(
            centre.r > 90 && centre.b > 90,
            "and it is the pattern that painted it, got {centre:?}"
        );
        // The arms too, so a canvas painted edge to edge cannot pass.
        assert_ne!(
            canvas.pixel(8, 31),
            Some(Color::WHITE),
            "the lower left arm"
        );
        assert_eq!(canvas.pixel(20, 4), Some(Color::WHITE), "and nothing above");
    }

    // -----------------------------------------------------------------------
    // Quadratic glyph outlines (gap 13).
    // -----------------------------------------------------------------------

    /// A glyph whose second contour opens with a quadratic straight after a
    /// close, which is the case the deleted current-point helper got wrong.
    ///
    /// A close returns the pen to where the subpath began, so the curve runs
    /// from (0.05, 0.05) through the control at (0.05, 0.95) to (0.95, 0.05):
    /// an arch leaning hard against the left edge of the em. The tiny triangle
    /// exists only to put a `Close` in front of the curve; it lies inside the
    /// arch and is wound the same way, so under the non-zero rule it adds no
    /// ink and takes none away. Wound the other way it punches a hole, which
    /// is how the first draft of this fixture was written and what the
    /// comparison below caught.
    struct ClosedThenCurved;

    impl GlyphSource for ClosedThenCurved {
        fn outline(&self, _font_id: u64, _code: u32) -> Option<Outline> {
            use tinker_pdf_font::Segment;
            Some(Outline {
                segments: vec![
                    Segment::MoveTo { x: 0.05, y: 0.05 },
                    Segment::LineTo { x: 0.12, y: 0.09 },
                    Segment::LineTo { x: 0.12, y: 0.05 },
                    Segment::Close,
                    Segment::QuadTo {
                        cx: 0.05,
                        cy: 0.95,
                        x: 0.95,
                        y: 0.05,
                    },
                    Segment::Close,
                ],
            })
        }
    }

    fn glyph_canvas() -> Canvas {
        let canvas = Canvas::new(40, 40, PixelFormat::Rgb8, Color::WHITE);
        let mut renderer = Renderer::new(canvas, page_transform(40.0, 1.0), &ClosedThenCurved);
        interpret(
            b"BT /F0 40 Tf 0 0 Td (A) Tj ET",
            Matrix::IDENTITY,
            &mut renderer,
            &OneChar,
        );
        renderer.finish().0
    }

    fn painted(canvas: &Canvas) -> usize {
        (0..40)
            .flat_map(|y| (0..40).map(move |x| (x, y)))
            .filter(|(x, y)| canvas.pixel(*x, *y) != Some(Color::WHITE))
            .count()
    }

    /// Milestone 2 of gap 13, and the reason the verb removes a bug rather
    /// than only some arithmetic.
    ///
    /// `show_glyph` used to raise each quadratic to a cubic itself, which
    /// needs the current point, which it looked up from the last verb already
    /// on the path. After a `Close` there is no such verb, and it fell back to
    /// the curve's *own* endpoint — so both control points collapsed onto one
    /// another and the arch leaned right instead of left.
    ///
    /// The probe is where the two shapes are furthest apart — nine pixels, at
    /// device x = 4. The arch this outline describes tops out there at
    /// y = 24.3 and the one the fallback produced at y = 33.3, so device
    /// (4, 28) is four pixels inside the first and five pixels above the
    /// second. It was white before this change.
    #[test]
    fn a_glyph_quadratic_after_a_close_starts_at_the_subpath_start() {
        let canvas = glyph_canvas();

        assert_ne!(
            canvas.pixel(4, 28),
            Some(Color::WHITE),
            "the arch leans against the left of the em, so this column is \
             filled from y = 24 down to the baseline"
        );
        // Both shapes cover this one, so a glyph that drew nothing at all
        // cannot pass the assertion above by drawing nothing here either.
        assert_ne!(
            canvas.pixel(4, 36),
            Some(Color::WHITE),
            "close to the chord, which both readings of the curve fill"
        );
        assert_eq!(
            canvas.pixel(30, 10),
            Some(Color::WHITE),
            "and well above the arch, nothing"
        );
    }

    /// The same shape written as an explicit cubic in a content stream: start
    /// (2, 2), control points two thirds of the way toward (2, 38), end
    /// (38, 2). If the glyph path and this one do not agree, the quadratic arm
    /// of the flattener and the cubic arm have drifted apart — which is what
    /// gives one typeface two smoothness characters.
    #[test]
    fn a_quadratic_glyph_matches_the_same_curve_drawn_as_a_cubic() {
        let glyph = glyph_canvas();
        let (reference, _) = render(b"2 2 m 2 26 14 26 38 2 c h f", 40);

        assert_eq!(
            painted(&glyph),
            painted(&reference),
            "the two cover the same pixels"
        );

        let mut differing = 0usize;
        let mut worst = 0u8;
        for y in 0..40 {
            for x in 0..40 {
                let (Some(a), Some(b)) = (glyph.pixel(x, y), reference.pixel(x, y)) else {
                    continue;
                };
                if a != b {
                    differing += 1;
                    for (p, q) in [(a.r, b.r), (a.g, b.g), (a.b, b.b)] {
                        worst = worst.max(p.abs_diff(q));
                    }
                }
            }
        }
        // Byte for byte, not within a budget. The two evaluations are the same
        // algebraic expression over `+ - * /`, which IEEE 754 rounds
        // identically on every target and which the `mul_add` ban keeps an
        // optimizer from contracting, so agreeing here means agreeing on all
        // four. Before the up-conversion was deleted, 138 of these 1600 pixels
        // differed.
        assert_eq!(
            differing, 0,
            "{differing} of 1600 pixels differ, the worst by {worst} levels"
        );
    }

    /// A ring of two cubic contours wound opposite ways, centred in its em:
    /// curved edges on every side, so its coverage depends on where it falls
    /// inside a pixel.
    struct Ring;

    impl GlyphSource for Ring {
        fn outline(&self, _font_id: u64, _code: u32) -> Option<Outline> {
            use tinker_pdf_font::Segment;
            // A quarter of a circle as a cubic: the control points sit this
            // fraction of the radius out along each tangent.
            const K: f64 = 0.552_284_749_830_793_4;
            let circle = |r: f64, forward: bool| {
                let (cx, cy) = (0.5, 0.45);
                // The points a quarter turn apart, and the unit tangent at
                // each in the direction the contour runs.
                let at = |quarter: usize| {
                    let (cos, sin) = [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)][quarter];
                    let tangent = if forward { (-sin, cos) } else { (sin, -cos) };
                    ((cx + r * cos, cy + r * sin), tangent)
                };
                let order = if forward {
                    [0, 1, 2, 3, 0]
                } else {
                    [0, 3, 2, 1, 0]
                };
                let mut segments = vec![Segment::MoveTo { x: cx + r, y: cy }];
                for pair in order.windows(2) {
                    let ((p, tp), (q, tq)) = (at(pair[0]), at(pair[1]));
                    segments.push(Segment::CurveTo {
                        c1x: p.0 + K * r * tp.0,
                        c1y: p.1 + K * r * tp.1,
                        c2x: q.0 - K * r * tq.0,
                        c2y: q.1 - K * r * tq.1,
                        x: q.0,
                        y: q.1,
                    });
                }
                segments.push(Segment::Close);
                segments
            };
            let mut segments = circle(0.4, true);
            segments.extend(circle(0.25, false));
            Some(Outline { segments })
        }
    }

    /// The pixels a canvas holds that are not white, each keyed from the whole
    /// pixel `origin`.
    fn ink_from(canvas: &Canvas, side: u32, origin: (i64, i64)) -> Vec<(i64, i64, Color)> {
        let mut ink = Vec::new();
        for y in 0..side {
            for x in 0..side {
                let Some(colour) = canvas.pixel(x, y) else {
                    continue;
                };
                if colour != Color::WHITE {
                    ink.push((i64::from(x) - origin.0, i64::from(y) - origin.1, colour));
                }
            }
        }
        ink
    }

    /// **A glyph moved by whole pixels draws the same mask** (P-06.2).
    ///
    /// A filled glyph is rasterised in its own frame, under its 2x2 and its
    /// origin's phase quantised to a sixteenth of a pixel, and placed at the
    /// whole pixel its origin falls in. So the same glyph is the same pixels
    /// wherever it stands at one phase — moved by whole pixels in any
    /// direction, and moved inside one sixteenth of a pixel, which the path
    /// route would have drawn differently — and a move to another sixteenth
    /// draws another mask.
    #[test]
    fn a_glyph_moved_by_whole_pixels_draws_the_same_mask() {
        const SIDE: u32 = 160;
        // The glyph at user (x, y) on a 160-point page at 72 dpi: its device
        // origin is (x, 160 - y), and its ink keyed from that origin's whole
        // pixel.
        let ink = |x: f64, y: f64| {
            let content = format!("BT /F0 30 Tf {x} {y} Td (o) Tj ET");
            let canvas = Canvas::new(SIDE, SIDE, PixelFormat::Rgb8, Color::WHITE);
            let mut renderer = Renderer::new(canvas, page_transform(f64::from(SIDE), 1.0), &Ring);
            interpret(
                content.as_bytes(),
                Matrix::IDENTITY,
                &mut renderer,
                &OneChar,
            );
            let canvas = renderer.finish().0;
            let origin = (x.floor() as i64, (f64::from(SIDE) - y).floor() as i64);
            ink_from(&canvas, SIDE, origin)
        };

        let here = ink(40.3, 70.6);
        assert!(here.len() > 100, "the ring draws: {} pixels", here.len());
        assert!(
            here.iter().any(|(_, _, c)| *c != Color::BLACK),
            "and its edges are anti-aliased, so its phase reaches the pixels"
        );
        for (dx, dy) in [(37.0, 11.0), (-29.0, 23.0), (61.0, -40.0)] {
            assert_eq!(
                ink(40.3 + dx, 70.6 + dy),
                here,
                "moved by ({dx}, {dy}) whole pixels, the same mask"
            );
        }
        // 0.3 and 0.33 are one sixteenth, the fifth, as are the device
        // fractions 0.4 and 0.38 the two heights give, the sixth.
        assert_eq!(
            ink(40.33, 70.62),
            here,
            "moved inside one sixteenth, the same mask"
        );
        // 0.37 is the sixth sixteenth across, one past 0.3's.
        assert_ne!(
            ink(40.37, 70.6),
            here,
            "moved to the next sixteenth, another mask"
        );
    }

    #[test]
    fn rendering_is_bit_identical_across_runs() {
        let content = b"1 0 0 1 3 3 cm 0 0 m 12 1 l 6 13 l h f 2 2 m 14 14 l S";
        let (a, _) = render(content, 24);
        let (b, _) = render(content, 24);
        assert_eq!(a.data, b.data, "ruling 4");
    }

    // -----------------------------------------------------------------------
    // Optional content (gap 06, 8.11.3.2).
    // -----------------------------------------------------------------------

    /// `/Off` is hidden, `/On` is not, and `/Plain` is not a layer at all.
    struct HiddenLayer;

    impl tinker_pdf_content::FontSource for HiddenLayer {
        fn decode(&self, _font: &[u8], bytes: &[u8]) -> Vec<(u32, String, f64)> {
            bytes
                .iter()
                .map(|&b| (u32::from(b), char::from(b).to_string(), 500.0))
                .collect()
        }
        fn vertical_metrics(&self, _font: &[u8], _code: u32) -> (f64, f64, f64) {
            (0.0, 880.0, -1000.0)
        }
        fn optional_content(&self, name: &[u8]) -> Option<tinker_pdf_content::Layer> {
            match name {
                b"Off" => Some(tinker_pdf_content::Layer {
                    visible: false,
                    label: "Construction lines".to_string(),
                }),
                b"On" => Some(tinker_pdf_content::Layer {
                    visible: true,
                    label: "Base".to_string(),
                }),
                _ => None,
            }
        }
    }

    /// Renders against a source that has one hidden layer, one visible one,
    /// one glyph outline and one image.
    fn with_layers(content: &[u8]) -> (Canvas, Vec<RenderWarning>) {
        let canvas = Canvas::new(40, 40, PixelFormat::Rgb8, Color::WHITE);
        let mut renderer = Renderer::new(canvas, page_transform(40.0, 1.0), &Patterns);
        interpret(content, Matrix::IDENTITY, &mut renderer, &HiddenLayer);
        renderer.finish()
    }

    /// **The per-render statistics count each call once, by its class**: a
    /// fill (a patterned one among them), a stroke, a glyph and an image drawn,
    /// a clip installed by `W` and one by a text object at `ET`; a glyph in
    /// mode 3 and one that only clips are not drawn; a fill, a stroke, a glyph
    /// and an image in a hidden layer are hidden; an image this source has no
    /// samples for is drawn as its placeholder, and one it has is drawn from
    /// its samples, two by two; and a render cancelled before it starts counts
    /// nothing.
    #[test]
    fn the_counts_classify_what_the_render_drew() {
        let canvas = Canvas::new(40, 40, PixelFormat::Rgb8, Color::WHITE);
        let mut renderer = Renderer::new(canvas, page_transform(40.0, 1.0), &Patterns);
        interpret(
            b"0 0 10 10 re f \
              /Pattern cs /Grad scn 20 0 10 10 re f \
              12 w 0 20 m 40 20 l S \
              BT /F0 10 Tf 1 4 Td (A) Tj ET \
              BT 3 Tr /F0 10 Tf 1 4 Td (A) Tj ET \
              q 10 0 0 10 0 0 cm /Im0 Do Q \
              q 0 0 5 5 re W n Q \
              q BT 7 Tr /F0 10 Tf 1 4 Td (A) Tj ET Q \
              /OC /Off BDC 0 0 40 40 re f 0 20 m 40 20 l S \
                BT 0 Tr /F0 10 Tf 1 4 Td (A) Tj ET q 40 0 0 40 0 0 cm /Im0 Do Q EMC",
            Matrix::IDENTITY,
            &mut renderer,
            &HiddenLayer,
        );
        let (_, _, counts) = renderer.finish_counted();
        assert_eq!(
            (
                counts.drawn_fills,
                counts.drawn_strokes,
                counts.drawn_glyphs,
                counts.drawn_images,
                counts.drawn_clips,
                counts.hidden_calls,
            ),
            (2, 1, 1, 1, 2, 4),
            "{counts:?}"
        );
        assert_eq!(
            (
                counts.decoded_images_drawn,
                counts.decoded_image_samples,
                counts.drawn_shadings,
                counts.drawn_groups,
                counts.drawn_soft_masks,
            ),
            (0, 0, 0, 0, 0),
            "{counts:?}"
        );

        let source = OneImage {
            color: (255, 0, 0),
            alpha: Vec::new(),
        };
        let canvas = Canvas::new(40, 40, PixelFormat::Rgb8, Color::WHITE);
        let mut renderer = Renderer::new(canvas, page_transform(40.0, 1.0), &source);
        interpret(
            b"q 10 0 0 10 0 0 cm /Im0 Do Q q 10 0 0 10 20 20 cm /Im0 Do Q",
            Matrix::IDENTITY,
            &mut renderer,
            &NoFonts,
        );
        let (_, _, counts) = renderer.finish_counted();
        assert_eq!(
            (
                counts.drawn_images,
                counts.decoded_images_drawn,
                counts.decoded_image_samples
            ),
            (2, 2, 8),
            "{counts:?}"
        );

        let cancel = CancelToken::new();
        cancel.cancel();
        let canvas = Canvas::new(20, 20, PixelFormat::Rgb8, Color::WHITE);
        let mut renderer =
            Renderer::new(canvas, page_transform(20.0, 1.0), &Patterns).with_cancel(cancel);
        interpret(
            b"0 0 20 20 re f /OC /Off BDC 0 0 20 20 re f EMC",
            Matrix::IDENTITY,
            &mut renderer,
            &HiddenLayer,
        );
        let (_, _, counts) = renderer.finish_counted();
        assert_eq!(counts, RenderCounts::default(), "a cancelled render");
    }

    /// M3: a rectangle inside an `/OFF` layer paints nothing, and the same
    /// rectangle outside one paints.
    ///
    /// Both halves in one page, because "renders white" is what a render that
    /// failed outright also produces. The decoy is the green rectangle: a
    /// build that suppressed everything, or that never painted at all, fails
    /// on it rather than passing this test by accident.
    #[test]
    fn a_hidden_layer_paints_nothing_and_its_neighbour_paints() {
        let (canvas, warnings) = with_layers(
            b"0 1 0 rg 0 0 10 40 re f \
              /OC /Off BDC 1 0 0 rg 20 0 10 40 re f EMC",
        );

        assert_eq!(
            canvas.pixel(5, 20),
            Some(Color::rgb(0, 255, 0)),
            "the layer-less rectangle painted"
        );
        assert_eq!(
            canvas.pixel(25, 20),
            Some(Color::WHITE),
            "and the one in the /OFF layer did not"
        );
        assert!(
            warnings.contains(&RenderWarning::HiddenOptionalContent {
                layer: "Construction lines".to_string()
            }),
            "the layer is named: {warnings:?}"
        );
    }

    /// The same content with the layer on. Exit criterion of M3, and the half
    /// that makes the other half mean something.
    #[test]
    fn the_same_content_in_a_visible_layer_paints() {
        let (canvas, warnings) = with_layers(
            b"0 1 0 rg 0 0 10 40 re f \
              /OC /On BDC 1 0 0 rg 20 0 10 40 re f EMC",
        );

        assert_eq!(canvas.pixel(5, 20), Some(Color::rgb(0, 255, 0)));
        assert_eq!(
            canvas.pixel(25, 20),
            Some(Color::rgb(255, 0, 0)),
            "an /ON layer paints exactly as if it were not marked at all"
        );
        assert!(warnings.is_empty(), "and reports nothing: {warnings:?}");
    }

    /// The nesting the device has to keep. The inner scope is a layer that is
    /// *on*, and closing it must not un-hide the outer one — which is the
    /// defect a counter-free `bool` would have.
    #[test]
    fn an_inner_scope_closing_does_not_un_hide_the_outer_one() {
        let (canvas, _) = with_layers(
            b"/OC /Off BDC \
                /OC /On BDC EMC \
                1 0 0 rg 0 0 40 40 re f \
              EMC",
        );
        assert_eq!(canvas.pixel(20, 20), Some(Color::WHITE));
    }

    /// Every painting operator, not only `f`. A guard on one of them and not
    /// the rest is a layer that half disappears, which reads as a rendering
    /// bug rather than as a hidden layer.
    #[test]
    fn every_paint_is_suppressed_inside_a_hidden_layer() {
        for content in [
            b"1 0 0 rg 0 0 40 40 re f".as_slice(),
            b"1 0 0 RG 12 w 0 20 m 40 20 l S",
            b"/Pattern cs /Grad scn 0 0 40 40 re f",
            b"BT /F0 40 Tf 1 4 Td (A) Tj ET",
            b"q 40 0 0 40 0 0 cm /Im0 Do Q",
        ] {
            let mut hidden = Vec::from(b"/OC /Off BDC ".as_slice());
            hidden.extend_from_slice(content);
            hidden.extend_from_slice(b" EMC");

            let (shown, _) = with_layers(content);
            let (suppressed, _) = with_layers(&hidden);
            assert_ne!(
                shown.pixel(20, 20),
                Some(Color::WHITE),
                "{} draws when it is not hidden",
                String::from_utf8_lossy(content)
            );
            assert!(
                suppressed.data.iter().all(|byte| *byte == 255),
                "{} put ink on the page inside an /OFF layer",
                String::from_utf8_lossy(content)
            );
        }
    }

    /// An image in a hidden layer is not reported as an unsupported codec.
    ///
    /// The decode never happens, so whether this build could have drawn it is
    /// not a fact about the page. `/Missing` is a name the source resolves to
    /// nothing, which outside a layer produces `UnsupportedImage` and a grey
    /// placeholder — asserted here so the test cannot pass because the image
    /// path is broken generally.
    #[test]
    fn a_hidden_image_neither_draws_nor_reports_a_codec() {
        let (_, warnings) = with_layers(b"q 40 0 0 40 0 0 cm /Missing Do Q");
        assert!(
            warnings
                .iter()
                .any(|w| matches!(w, RenderWarning::UnsupportedImage { .. })),
            "the unhidden case reports: {warnings:?}"
        );

        let (canvas, warnings) = with_layers(b"/OC /Off BDC q 40 0 0 40 0 0 cm /Missing Do Q EMC");
        assert!(
            !warnings
                .iter()
                .any(|w| matches!(w, RenderWarning::UnsupportedImage { .. })),
            "a hidden image is not a missing codec: {warnings:?}"
        );
        assert_eq!(
            canvas.pixel(20, 20),
            Some(Color::WHITE),
            "and no placeholder was painted"
        );
    }

    /// 8.5.4: a clip is graphics state, not a paint, so a hidden layer that
    /// sets one still sets it. Getting this backwards is invisible on the
    /// page that hid the layer and wrong on every page after it.
    #[test]
    fn a_clip_set_inside_a_hidden_layer_still_applies() {
        let (canvas, _) = with_layers(
            b"/OC /Off BDC 0 0 20 40 re W n EMC \
              1 0 0 rg 0 0 40 40 re f",
        );

        assert_eq!(
            canvas.pixel(10, 20),
            Some(Color::rgb(255, 0, 0)),
            "inside the clip the hidden scope set"
        );
        assert_eq!(canvas.pixel(30, 20), Some(Color::WHITE), "outside it");
    }

    /// 9.3.6: a clipping text mode inside a hidden layer must not clip. The
    /// glyph is not painted, so it accumulates nothing, and `ET` must not
    /// then read that as "clipped to nothing" and erase the page.
    #[test]
    fn a_clipping_text_mode_inside_a_hidden_layer_clips_nothing() {
        let (canvas, warnings) = with_layers(
            b"/OC /Off BDC BT /F0 40 Tf 7 Tr 1 4 Td (A) Tj ET EMC \
              1 0 0 rg 0 0 40 40 re f",
        );

        assert_eq!(
            canvas.pixel(20, 20),
            Some(Color::rgb(255, 0, 0)),
            "the page after the hidden text object is not clipped away"
        );
        assert!(
            !warnings.contains(&RenderWarning::EmptyTextClip),
            "and an empty clip was never requested: {warnings:?}"
        );
    }

    /// One warning per layer however many scopes it opens: a CAD drawing
    /// marks every construction line.
    #[test]
    fn a_layer_is_reported_once_however_often_it_is_marked() {
        let (_, warnings) = with_layers(
            b"/OC /Off BDC 0 0 4 4 re f EMC \
              /OC /Off BDC 0 0 5 5 re f EMC \
              /OC /Off BDC 0 0 6 6 re f EMC",
        );
        assert_eq!(warnings.len(), 1, "{warnings:?}");
    }

    #[test]
    fn an_empty_or_broken_page_renders_blank_rather_than_failing() {
        for content in [
            b"".as_slice(),
            b"garbage operators here",
            b"f S n W",
            b"0 0 0 0 re f",
            &[0xFF; 128],
        ] {
            let (canvas, _) = render(content, 8);
            assert_eq!(canvas.data.len(), 8 * 8 * 3);
        }
    }

    /// **On a whole page, every page-frame rectangle is the rectangle of the
    /// mask it stands beside, and `frame` is the canvas's own** — after every
    /// call, through clips and `q`/`Q`, a group under a clip and one under
    /// none, soft masks with a black and a pale backdrop, `/SMask /None`, a
    /// text clip that clips everything away, and a clip wholly off the page.
    /// A mask's rectangle is the one it confines a paint to: its own, and for
    /// a mask that reads a value outside itself — the pale backdrop's — the
    /// whole canvas, every pixel of which it leaves some coverage
    /// (`Bounds::limit_of`).
    ///
    /// This is the invariant that makes a tile's decisions the page's
    /// (ruling 5): the rectangles are computed from device coordinates alone,
    /// so they are the same numbers in a tile as here, and *here* they are
    /// provably what the masks were — which is what the page decided with
    /// before they existed. A tile cannot check it about itself, because in a
    /// tile the masks are smaller than the rectangles by design; so it is
    /// checked on the one render where the two must agree.
    #[test]
    fn page_frame_rectangles_are_the_masks_rectangles_on_a_whole_page() {
        #[track_caller]
        fn check(r: &Renderer<'_, NoGlyphs>, step: &str) {
            assert_eq!(
                r.frame,
                Bounds::of_rect(r.canvas.device_rect()),
                "{step}: the frame is not the canvas"
            );
            assert_eq!(
                r.clip_bounds,
                r.clip.as_ref().map(Bounds::of_mask),
                "{step}: the clip's rectangle"
            );
            assert_eq!(r.clip.as_ref().map_or(0, |clip| clip.outside), 0, "{step}");
            assert_eq!(
                r.soft_bounds,
                r.soft
                    .as_ref()
                    .map(|soft| Bounds::limit_of(soft).unwrap_or(r.frame)),
                "{step}: the soft mask's rectangle"
            );
        }
        let rect = |x0: f64, y0: f64, x1: f64, y1: f64| {
            vec![
                PathSegment::MoveTo { x: x0, y: y0 },
                PathSegment::LineTo { x: x1, y: y0 },
                PathSegment::LineTo { x: x1, y: y1 },
                PathSegment::LineTo { x: x0, y: y1 },
                PathSegment::Close,
            ]
        };
        let mask = |backdrop: Option<tinker_pdf_content::Rgb>| tinker_pdf_content::MaskGroup {
            form: tinker_pdf_content::Form {
                content: Vec::new(),
                matrix: Matrix::IDENTITY,
                bbox: None,
                group: None,
                stream: 0,
            },
            luminosity: true,
            backdrop,
            transfer: None,
        };
        let group = |isolated| tinker_pdf_content::Group {
            isolated,
            knockout: false,
            space: None,
        };
        let state = GraphicsState::default();
        let canvas = Canvas::new(120, 90, PixelFormat::Rgb8, Color::WHITE);
        let mut r = Renderer::new(canvas, page_transform(90.0, 1.3), &NoGlyphs);
        check(&r, "a new renderer");

        r.save_state();
        r.clip_path(&rect(5.5, 7.25, 60.0, 50.5), &state, false);
        check(&r, "a clip");
        assert!(r.begin_group(group(false), &state));
        check(&r, "a group under the clip");
        r.clip_path(&rect(20.0, 10.0, 90.0, 80.0), &state, true);
        check(&r, "a clip inside the group");
        assert!(r.begin_soft_mask(&mask(None), &rect(0.0, 30.0, 40.0, 95.0), &state));
        check(&r, "a black-backdrop mask group");
        r.fill_path(&rect(1.0, 1.0, 30.0, 30.0), &state, false);
        r.end_soft_mask();
        check(&r, "the black-backdrop mask installed");
        r.clip_path(&rect(10.0, 12.0, 44.0, 40.0), &state, false);
        check(&r, "a clip under the soft mask");
        r.end_group();
        check(&r, "the group closed");
        r.restore_state();
        check(&r, "Q");

        assert!(r.begin_soft_mask(
            &mask(Some(tinker_pdf_content::Rgb {
                r: 200,
                g: 200,
                b: 200
            })),
            &rect(50.0, -20.0, 200.0, 40.0),
            &state
        ));
        check(&r, "a pale-backdrop mask group off the page's edge");
        r.end_soft_mask();
        check(&r, "the pale-backdrop mask installed");
        let pale = r.soft.as_ref().expect("the pale mask is in force");
        assert_ne!(pale.outside, 0, "it reads its backdrop outside its box");
        assert!(
            u64::from(pale.width) * u64::from(pale.height) < pixels_of(&r.canvas),
            "and it is kept at its box: {}x{}",
            pale.width,
            pale.height
        );
        assert!(r.begin_group(group(true), &state));
        check(&r, "an isolated group under no clip");
        r.end_group();
        r.clear_soft_mask();
        check(&r, "/SMask /None");

        r.save_state();
        r.clip_path(&rect(500.0, 500.0, 520.0, 520.0), &state, false);
        check(&r, "a clip wholly off the page");
        assert!(r.begin_group(group(false), &state));
        check(&r, "a group whose clip misses the page");
        r.end_group();
        r.restore_state();
        r.begin_text();
        r.text_clip_requested = true;
        r.end_text();
        check(&r, "a text clip that clips everything away");
    }

    /// A rectangle in user space, as path segments.
    fn rect_path(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<PathSegment> {
        vec![
            PathSegment::MoveTo { x: x0, y: y0 },
            PathSegment::LineTo { x: x1, y: y0 },
            PathSegment::LineTo { x: x1, y: y1 },
            PathSegment::LineTo { x: x0, y: y1 },
            PathSegment::Close,
        ]
    }

    /// A luminosity mask group whose backdrop is `backdrop`, with no `/TR`.
    fn luminosity_mask(backdrop: Option<tinker_pdf_content::Rgb>) -> tinker_pdf_content::MaskGroup {
        tinker_pdf_content::MaskGroup {
            form: tinker_pdf_content::Form {
                content: Vec::new(),
                matrix: Matrix::IDENTITY,
                bbox: None,
                group: None,
                stream: 0,
            },
            luminosity: true,
            backdrop,
            transfer: None,
        }
    }

    /// The graphics state with a fill colour of `grey`.
    fn filled(grey: u8) -> GraphicsState {
        let mut state = GraphicsState::default();
        state.fill_color = tinker_pdf_content::Rgb {
            r: grey,
            g: grey,
            b: grey,
        };
        state
    }

    /// **A soft mask with a pale backdrop is kept at its box and masks the
    /// page beyond it by its backdrop's value** (P-25.2).
    ///
    /// A luminosity mask whose `/BC` is grey 200, its box the middle of a
    /// 60 by 40 page and its group painting the box's left half white, then a
    /// black fill over the whole page: the mask in force is the box's
    /// region, not the canvas, and reads 200 outside it; the fill is black
    /// where the group painted white, and black at 200 levels over white —
    /// 55 — in the box's right half and everywhere outside the box. The
    /// counts say the same: the installed mask's pixels are its group's
    /// region's, and the mask group's buffer is that region too. Whole and in
    /// a tile that the box misses, which installs a mask of no pixels that
    /// still reads 200 everywhere, and in one that it straddles.
    #[test]
    fn a_pale_soft_mask_is_kept_at_its_box_and_masks_the_page_beyond_it() {
        let pale = Some(tinker_pdf_content::Rgb {
            r: 200,
            g: 200,
            b: 200,
        });
        let draw = |canvas: Canvas| {
            let mut r =
                Renderer::new(canvas, page_transform(40.0, 1.0), &NoGlyphs).with_page_size(60, 40);
            let plain = GraphicsState::default();
            assert!(r.begin_soft_mask(
                &luminosity_mask(pale),
                &rect_path(15.0, 10.0, 45.0, 30.0),
                &plain
            ));
            r.fill_path(&rect_path(15.0, 10.0, 30.0, 30.0), &filled(255), false);
            r.end_soft_mask();
            let soft = r.soft.clone().expect("the mask is in force");
            r.fill_path(&rect_path(0.0, 0.0, 60.0, 40.0), &filled(0), false);
            let (canvas, _, counts) = r.finish_counted();
            (canvas, soft, counts)
        };

        let (page, soft, counts) = draw(Canvas::new(60, 40, PixelFormat::Rgb8, Color::WHITE));
        // The box is 15..45 by 10..30 on the page, its rows flipped, widened
        // by the slack every region carries.
        let region = paint_region(Some((15.0, 10.0, 45.0, 30.0)), None, None, (0, 0, 60, 40));
        assert_eq!(
            (soft.x0, soft.y0, soft.width, soft.height),
            region,
            "the mask is its box's region"
        );
        assert_eq!(soft.outside, 200, "and reads the backdrop beyond it");
        let area = u64::from(region.2) * u64::from(region.3);
        assert!(area < 60 * 40, "a box smaller than the page");
        assert_eq!(
            (counts.soft_mask_pixels, counts.soft_mask_bound_pixels),
            (area, area)
        );
        assert_eq!(
            (counts.group_buffer_pixels, counts.group_bound_pixels),
            (area, area)
        );
        let grey = |canvas: &Canvas, x: i32, y: i32| {
            let (x, y) = canvas.local(x, y).expect("on the canvas");
            canvas.pixel(x, y).expect("a pixel").r
        };
        assert_eq!(grey(&page, 20, 20), 0, "where the group painted white");
        assert_eq!(
            grey(&page, 40, 20),
            55,
            "in the box, where it painted nothing"
        );
        for (x, y) in [(2, 2), (57, 37), (5, 20), (50, 20), (30, 3), (30, 36)] {
            assert_eq!(grey(&page, x, y), 55, "outside the box at ({x}, {y})");
        }

        // Tiles: one the box misses, one it straddles, each the page's pixels.
        for (x, y, width, height) in [(0, 0, 12, 7), (25, 5, 30, 20)] {
            let tile = Canvas::new(width, height, PixelFormat::Rgb8, Color::WHITE).at_origin(x, y);
            let (tile, soft, counts) = draw(tile);
            for row in 0..height {
                for col in 0..width {
                    let (px, py) = (x + col as i32, y + row as i32);
                    assert_eq!(
                        tile.pixel(col, row),
                        page.pixel(px as u32, py as u32),
                        "the tile at ({x}, {y}), pixel ({px}, {py})"
                    );
                }
            }
            assert_eq!(soft.outside, 200, "the tile at ({x}, {y})");
            let clipped = Bounds::of_rect(region).within(Bounds::of_rect((x, y, width, height)));
            assert_eq!(
                Bounds::of_mask(&soft),
                clipped,
                "the tile at ({x}, {y}): the box clipped to the tile"
            );
            assert_eq!(
                counts.soft_mask_pixels,
                clipped.area(),
                "the tile at ({x}, {y})"
            );
            assert_eq!(
                counts.soft_mask_bound_pixels,
                clipped.area(),
                "the tile at ({x}, {y})"
            );
        }
    }

    /// What a renderer is asked to do, step by step, for
    /// [`a_group_left_open_hands_back_the_page`].
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Step {
        /// Open a transparency group, isolated or not, over the clip in force.
        Group(bool),
        /// Close the innermost group.
        EndGroup,
        /// Open a luminosity soft mask's group over a box.
        Mask,
        /// Close the innermost soft mask's group.
        EndMask,
        /// Set the cancellation token.
        Cancel,
    }

    /// A grey page, a clip, then `steps`, each opening step followed by a
    /// fill at half alpha inside what it opened, and then `finish`.
    fn drawn_with(steps: &[Step]) -> (Canvas, Vec<RenderWarning>) {
        let canvas = Canvas::new(40, 30, PixelFormat::Rgb8, Color::WHITE);
        let cancel = CancelToken::new();
        let mut r =
            Renderer::new(canvas, page_transform(30.0, 1.0), &NoGlyphs).with_cancel(cancel.clone());
        let plain = GraphicsState::default();
        r.fill_path(&rect_path(0.0, 0.0, 40.0, 30.0), &filled(128), false);
        r.save_state();
        r.clip_path(&rect_path(4.0, 3.0, 34.0, 27.0), &plain, false);
        let mut half = filled(20);
        half.fill_alpha = 0.5;
        for (at, step) in steps.iter().enumerate() {
            let shift = at as f64 * 3.0;
            match step {
                Step::Group(isolated) => {
                    let group = tinker_pdf_content::Group {
                        isolated: *isolated,
                        knockout: false,
                        space: None,
                    };
                    assert!(r.begin_group(group, &plain), "{steps:?}: the group opens");
                }
                Step::Mask => {
                    assert!(
                        r.begin_soft_mask(
                            &luminosity_mask(None),
                            &rect_path(2.0, 2.0, 38.0, 28.0),
                            &plain
                        ),
                        "{steps:?}: the mask opens"
                    );
                }
                Step::EndGroup => r.end_group(),
                Step::EndMask => r.end_soft_mask(),
                Step::Cancel => cancel.cancel(),
            }
            if matches!(step, Step::Group(_) | Step::Mask) {
                r.fill_path(
                    &rect_path(8.0 + shift, 6.0, 22.0 + shift, 20.0),
                    &half,
                    false,
                );
            }
        }
        r.finish()
    }

    /// **A group left open hands back the page** (P-25.3): a group's buffer
    /// holds the page while the group is open, and a renderer finished inside
    /// a non-isolated group, an isolated one, two nested, a soft mask's group
    /// inside a group and a group inside a soft mask's group hands back the
    /// page — its size, its frame and the pixels closing each in turn would
    /// have left. A render cancelled inside a group, left open or closed,
    /// hands back the page with what was painted before it and nothing of
    /// the group, and says it was cancelled.
    #[test]
    fn a_group_left_open_hands_back_the_page() {
        use Step::{Cancel, EndGroup, EndMask, Group, Mask};
        let opened: [&[Step]; 6] = [
            &[Group(false)],
            &[Group(true)],
            &[Group(false), Group(true)],
            &[Group(true), Group(false)],
            &[Group(false), Mask],
            &[Mask, Group(false)],
        ];
        for steps in opened {
            // Each opened thing closed, innermost first.
            let mut closing = steps.to_vec();
            for step in steps.iter().rev() {
                closing.push(if *step == Mask { EndMask } else { EndGroup });
            }
            let (closed, closed_warnings) = drawn_with(&closing);
            let (open, warnings) = drawn_with(steps);
            assert_eq!(
                (open.width, open.height, open.format, open.origin()),
                (40, 30, PixelFormat::Rgb8, (0, 0)),
                "{steps:?}: the page, not a buffer"
            );
            assert!(
                open.data == closed.data,
                "{steps:?}: the pixels closing would leave"
            );
            assert_eq!(warnings, closed_warnings, "{steps:?}");
            assert!(!warnings.contains(&RenderWarning::Cancelled), "{steps:?}");
            if steps.first() != Some(&Mask) {
                assert_ne!(
                    open.pixel(12, 15),
                    Some(Color::rgb(128, 128, 128)),
                    "{steps:?}: the group was composited"
                );
            }
        }

        let grey = Canvas::new(40, 30, PixelFormat::Rgb8, Color::rgb(128, 128, 128));
        for isolated in [false, true] {
            for steps in [
                vec![Group(isolated), Cancel],
                vec![Group(isolated), Cancel, EndGroup],
                vec![Group(isolated), Group(!isolated), Cancel],
            ] {
                let (page, warnings) = drawn_with(&steps);
                assert_eq!(
                    (page.width, page.height, page.origin()),
                    (40, 30, (0, 0)),
                    "{steps:?}: the page"
                );
                assert!(
                    page.data == grey.data,
                    "{steps:?}: what was painted before the group, and nothing of it"
                );
                assert!(warnings.contains(&RenderWarning::Cancelled), "{steps:?}");
            }
        }
    }

    /// Every image a one-sample black picture, whatever it is called.
    struct BlackSamples;

    impl GlyphSource for BlackSamples {
        fn outline(&self, _font_id: u64, _code: u32) -> Option<Outline> {
            None
        }

        fn image(
            &self,
            _name: &[u8],
            _request: &ImageRequest,
            _intent: RenderingIntent,
        ) -> Result<Option<DecodedImage>, String> {
            Ok(Some(DecodedImage {
                stencil: false,
                family: ColorFamily::Rgb,
                soft_masked: false,
                interpolate: false,
                width: 1,
                height: 1,
                rgb: vec![0, 0, 0],
                alpha: Vec::new(),
                two_colour: None,
                reduced: None,
                window: None,
            }))
        }
    }

    /// The three images of ruling 5's named exception on a 100 x 100 page:
    /// the first and the third abut along `x = 40.5` in the top forty rows,
    /// and the third overlaps the second in rows 60 to 69.
    const THREE_IMAGES: &[u8] = b"q 40.5 0 0 40 0 60 cm /I Do Q \
        q 40 0 0 40 40.5 0 cm /I Do Q \
        q 40 0 0 70 40.5 30 cm /I Do Q";

    /// The page of [`THREE_IMAGES`] drawn over `region`, or whole: its
    /// canvas, its reach and what it counted.
    fn three_images(region: Option<PixelRegion>) -> (Canvas, RunReach, RenderCounts) {
        let base = page_transform(100.0, 1.0);
        let canvas = match region {
            Some(region) => region_canvas_in(region, PixelFormat::Rgb8),
            None => Canvas::new(100, 100, PixelFormat::Rgb8, Color::WHITE),
        };
        let mut renderer = Renderer::new(canvas, base, &BlackSamples).with_page_size(100, 100);
        interpret(THREE_IMAGES, Matrix::IDENTITY, &mut renderer, &NoFonts);
        let reach = renderer.run_reach();
        let (canvas, _, counts) = renderer.finish_counted();
        (canvas, reach, counts)
    }

    /// **What a tile cannot see of an image run is reported** (`RunReach`).
    /// The whole page sees everything and reports nothing. A tile over the
    /// top half cannot see where the third image meets the run of the first
    /// two — the bounds they share run to row 70 — and reports that, and
    /// both images whose rows run past it; a tile grown to row 70 sees all of
    /// it, reports only the second image as cut, and is the page's top
    /// seventy rows pixel for pixel, the run ended where the page ends it.
    #[test]
    fn a_tile_reports_where_an_image_meets_a_run_outside_it() {
        let (page, whole, _) = three_images(None);
        assert_eq!(whole, RunReach::default(), "the page holds everything");

        let (_, top, _) = three_images(Some(PixelRegion::new(0, 0, 100, 50)));
        assert_eq!(
            top.unseen,
            Some((40, 0, 41, 70)),
            "the third image and the run's bounds meet down to row 70"
        );
        assert_eq!(
            top.cut,
            Some((40, 0, 41, 100)),
            "the second and the third image run past row 50"
        );

        let grown = PixelRegion::new(0, 0, 100, 70);
        let (tile, reach, _) = three_images(Some(grown));
        assert_eq!(reach.unseen, None, "every meeting is inside the tile");
        assert_eq!(reach.cut, Some((40, 60, 41, 40)), "the second image alone");
        for y in 0..70 {
            for x in 0..100 {
                assert_eq!(
                    tile.pixel(x, y),
                    page.pixel(x, y),
                    "({x}, {y}): the grown tile is the page under it"
                );
            }
        }
    }

    /// An image drawn under another clip than the run's ends the run whatever
    /// the tile sees, so where it meets the run is no question the tile could
    /// answer otherwise, and is not reported; the image is still cut. The same
    /// two images under one clip meet outside the tile, and that is reported.
    #[test]
    fn a_change_of_state_ends_the_run_and_reports_no_meeting() {
        let reach = |content: &[u8]| {
            let base = page_transform(100.0, 1.0);
            let canvas = region_canvas_in(PixelRegion::new(0, 0, 100, 50), PixelFormat::Rgb8);
            let mut renderer = Renderer::new(canvas, base, &BlackSamples).with_page_size(100, 100);
            interpret(content, Matrix::IDENTITY, &mut renderer, &NoFonts);
            renderer.run_reach()
        };
        // The first image under a clip the `Q` takes away, which a run
        // survives; the second, with no clip, cannot join it.
        let apart = reach(
            b"q 0 0 100 100 re W n q 40 0 0 70 0 30 cm /I Do Q Q \
              q 40 0 0 70 0 30 cm /I Do Q",
        );
        assert_eq!(apart.unseen, None, "the clip ends the run on the page too");
        assert_eq!(apart.cut, Some((0, 0, 40, 70)), "both images are cut");
        let together = reach(b"q 40 0 0 70 0 30 cm /I Do Q q 40 0 0 70 0 30 cm /I Do Q");
        assert_eq!(
            together.unseen,
            Some((0, 0, 40, 70)),
            "one clip: the second meets the run down to row 70"
        );
        assert_eq!(together.cut, Some((0, 0, 40, 70)));
    }

    /// A renderer counts its canvas's pixels: the page's whole, a tile's own.
    #[test]
    fn a_renderer_counts_the_pixels_of_its_canvas() {
        let (_, _, whole) = three_images(None);
        assert_eq!(whole.pixels_rendered, 100 * 100);
        let (_, _, tile) = three_images(Some(PixelRegion::new(5, 10, 30, 20)));
        assert_eq!(tile.pixels_rendered, 30 * 20);
    }
}

/// Forced colours (`Renderer::with_forced_colours`), driven through the
/// device calls themselves: each paint black by its coverage whatever its
/// state asked, fill-to-stroke as the thinnest stroke, and the interpreter's
/// questions answered as an ordinary render answers them.
#[cfg(test)]
mod forced_colour_tests {
    use super::*;
    use tinker_pdf_content::{BlendMode as ContentBlend, Form, Group, GroupSpace, MaskGroup, Rgb};

    const SIZE: u32 = 24;

    static GLYPHS: NoGlyphs = NoGlyphs;

    /// A square in user space with whole-pixel corners, so it covers each
    /// pixel wholly or not at all and a composite has no edge to round.
    fn square(x: f64, y: f64, side: f64) -> Vec<PathSegment> {
        vec![
            PathSegment::MoveTo { x, y },
            PathSegment::LineTo { x: x + side, y },
            PathSegment::LineTo {
                x: x + side,
                y: y + side,
            },
            PathSegment::LineTo { x, y: y + side },
            PathSegment::Close,
        ]
    }

    /// A renderer over a white grey canvas: forced, with or without
    /// fill-to-stroke, or ordinary.
    fn renderer(forced: Option<bool>) -> Renderer<'static, NoGlyphs> {
        let canvas = Canvas::new(SIZE, SIZE, PixelFormat::Gray8, Color::WHITE);
        let renderer = Renderer::new(canvas, page_transform(f64::from(SIZE), 1.0), &GLYPHS);
        match forced {
            Some(fill_to_stroke) => renderer.with_forced_colours(fill_to_stroke),
            None => renderer,
        }
    }

    /// The plain state: opaque black, `Normal`.
    fn plain() -> GraphicsState {
        GraphicsState::new(Matrix::IDENTITY)
    }

    /// A state asking for everything forced colours do not apply: colours,
    /// ink, constant alphas, a blend mode, a pattern, and a pen that is wide
    /// and dashed.
    fn loud() -> GraphicsState {
        let mut state = plain();
        state.fill_color = Rgb { r: 255, g: 0, b: 0 };
        state.stroke_color = Rgb { r: 0, g: 0, b: 255 };
        state.fill_ink = Some([0, 255, 255, 0]);
        state.stroke_ink = Some([255, 255, 0, 0]);
        state.fill_alpha = 0.4;
        state.stroke_alpha = 0.6;
        state.blend = ContentBlend::Multiply;
        state.line_width = 3.0;
        state.dashes = vec![2.0, 1.0];
        state.dash_phase = 0.5;
        state
    }

    fn pixels(renderer: Renderer<'static, NoGlyphs>) -> Vec<u8> {
        let (canvas, warnings) = renderer.finish();
        assert!(warnings.is_empty(), "{warnings:?}");
        canvas.data
    }

    /// **Every paint is opaque black by its coverage**: a fill and a stroke
    /// whose state asks for colour, ink, alpha and a blend mode paint what
    /// the plain state paints, and a fill naming a pattern no scope holds
    /// paints its shape rather than nothing.
    #[test]
    fn every_paint_is_opaque_black_by_its_coverage() {
        let mut forced = renderer(Some(false));
        forced.fill_path(&square(2.0, 2.0, 8.0), &loud(), false);
        forced.stroke_path(&square(12.0, 12.0, 8.0), &loud());
        let mut pattern = loud();
        pattern.fill_pattern = Some(b"P0".to_vec());
        forced.fill_path(&square(14.0, 2.0, 6.0), &pattern, false);

        let mut ordinary = renderer(None);
        let mut pen = plain();
        pen.line_width = 3.0;
        pen.dashes = vec![2.0, 1.0];
        pen.dash_phase = 0.5;
        ordinary.fill_path(&square(2.0, 2.0, 8.0), &plain(), false);
        ordinary.stroke_path(&square(12.0, 12.0, 8.0), &pen);
        ordinary.fill_path(&square(14.0, 2.0, 6.0), &plain(), false);
        let forced = pixels(forced);
        assert_eq!(forced, pixels(ordinary));
        assert!(forced.contains(&0), "black was painted");
    }

    /// **Fill-to-stroke is the thinnest solid stroke of the path**: a fill
    /// under a wide dashed pen paints exactly what stroking the same path
    /// with `0 w [] 0 d` paints, and not its interior.
    #[test]
    fn fill_to_stroke_is_the_thinnest_solid_stroke_of_the_path() {
        let mut fill = renderer(Some(true));
        fill.fill_path(&square(4.0, 4.0, 14.0), &loud(), true);
        let mut stroke = renderer(Some(false));
        let mut thin = plain();
        thin.line_width = 0.0;
        stroke.stroke_path(&square(4.0, 4.0, 14.0), &thin);
        let (fill, stroke) = (pixels(fill), pixels(stroke));
        assert_eq!(fill, stroke);
        let centre = (SIZE as usize / 2) * SIZE as usize + SIZE as usize / 2;
        assert_eq!(fill[centre], 255, "the interior is not painted");
        assert!(fill.iter().any(|&v| v < 255), "the outline is");
    }

    /// A fill inside a text object is a Type 3 glyph's, and a glyph stays
    /// filled; after the text object a fill is a stroke again.
    #[test]
    fn a_fill_inside_a_text_object_stays_a_fill() {
        let mut glyph = renderer(Some(true));
        glyph.begin_text();
        glyph.fill_path(&square(4.0, 4.0, 8.0), &plain(), false);
        glyph.end_text();
        let mut filled = renderer(Some(false));
        filled.fill_path(&square(4.0, 4.0, 8.0), &plain(), false);
        assert_eq!(pixels(glyph), pixels(filled));

        let mut after = renderer(Some(true));
        after.begin_text();
        after.end_text();
        after.fill_path(&square(4.0, 4.0, 8.0), &plain(), false);
        let mut stroked = renderer(Some(true));
        stroked.fill_path(&square(4.0, 4.0, 8.0), &plain(), false);
        assert_eq!(pixels(after), pixels(stroked));
    }

    /// **A paint forced colours skip is not counted as drawn**: inside a soft
    /// mask's group, which forced colours install nothing of, a fill is
    /// skipped and neither drawn nor hidden, while the mask is accepted as an
    /// ordinary render accepts it and the fills on either side of it are
    /// drawn; an ordinary render draws all three.
    #[test]
    fn a_paint_forced_colours_skip_is_not_counted_as_drawn() {
        let mask = MaskGroup {
            form: Form {
                content: Vec::new(),
                matrix: Matrix::IDENTITY,
                bbox: Some([0.0, 0.0, 24.0, 24.0]),
                group: None,
                stream: 0,
            },
            luminosity: true,
            backdrop: None,
            transfer: None,
        };
        let draw = |renderer: &mut Renderer<'static, NoGlyphs>| {
            renderer.fill_path(&square(2.0, 2.0, 8.0), &plain(), false);
            if renderer.begin_soft_mask(&mask, &square(0.0, 0.0, 24.0), &plain()) {
                renderer.fill_path(&square(0.0, 0.0, 24.0), &plain(), false);
                renderer.end_soft_mask();
            }
            renderer.fill_path(&square(14.0, 14.0, 6.0), &plain(), false);
        };
        let mut ordinary = renderer(None);
        draw(&mut ordinary);
        let (_, _, ordinary) = ordinary.finish_counted();
        assert_eq!(
            (
                ordinary.drawn_fills,
                ordinary.drawn_soft_masks,
                ordinary.hidden_calls
            ),
            (3, 1, 0),
            "an ordinary render"
        );
        let mut forced = renderer(Some(false));
        draw(&mut forced);
        let (_, _, forced) = forced.finish_counted();
        assert_eq!(
            (
                forced.drawn_fills,
                forced.drawn_soft_masks,
                forced.hidden_calls
            ),
            (2, 1, 0),
            "forced colours"
        );
    }

    /// The interpreter's questions are answered as an ordinary render answers
    /// them, so a display list recorded for one replays into the other; a
    /// knockout group declaring ink composites opaque and `Normal`, and a
    /// soft mask's group is drawn into nothing and never installed.
    #[test]
    fn forced_colours_answer_as_an_ordinary_render_and_install_no_soft_mask() {
        let group = Group {
            isolated: true,
            knockout: true,
            space: Some(GroupSpace::Cmyk),
        };
        let mask = MaskGroup {
            form: Form {
                content: Vec::new(),
                matrix: Matrix::IDENTITY,
                bbox: Some([0.0, 0.0, 24.0, 24.0]),
                group: None,
                stream: 0,
            },
            luminosity: true,
            backdrop: None,
            transfer: None,
        };
        let draw = |renderer: &mut Renderer<'static, NoGlyphs>| -> (bool, bool) {
            let grouped = renderer.begin_group(group, &loud());
            renderer.fill_path(&square(2.0, 2.0, 8.0), &loud(), false);
            renderer.fill_path(&square(6.0, 6.0, 8.0), &loud(), false);
            if grouped {
                renderer.end_group();
            }
            let masked = renderer.begin_soft_mask(&mask, &square(0.0, 0.0, 24.0), &plain());
            renderer.fill_path(&square(0.0, 0.0, 24.0), &plain(), false);
            if masked {
                renderer.end_soft_mask();
            }
            renderer.fill_path(&square(14.0, 14.0, 6.0), &loud(), false);
            (grouped, masked)
        };
        let mut ordinary = renderer(None);
        let mut forced = renderer(Some(false));
        let answers = draw(&mut ordinary);
        assert_eq!(draw(&mut forced), answers, "the same answers");
        assert_eq!(answers, (true, true));

        let mut direct = renderer(Some(false));
        direct.fill_path(&square(2.0, 2.0, 8.0), &plain(), false);
        direct.fill_path(&square(6.0, 6.0, 8.0), &plain(), false);
        direct.fill_path(&square(14.0, 14.0, 6.0), &plain(), false);
        let (forced, direct) = (pixels(forced), pixels(direct));
        assert_eq!(forced, direct, "drawn as if no group or mask were there");
        // An ordinary render masks the last square away under a black /BC.
        let ordinary = pixels(ordinary);
        let at = (SIZE as usize - 17) * SIZE as usize + 17;
        assert_eq!((forced[at], ordinary[at]), (0, 255));
    }
}

/// `Renderer::snapshot` (`docs/design/progressive-render.md`), over calls a
/// test writes out: snapshotted after every call, the render goes on to draw
/// what it would have drawn, and with no bracket open a snapshot is what the
/// same calls finish with.
#[cfg(test)]
mod snapshot_tests {
    use super::*;
    use tinker_pdf_content::{
        BlendMode as ContentBlend, Cursor, Event, Form, Group, ImageRef, MaskGroup, Rgb,
    };

    const SIZE: u32 = 32;

    /// Two images, `A` and `B`, four samples each of a colour of their own.
    struct Images;

    impl GlyphSource for Images {
        fn outline(&self, _font_id: u64, _code: u32) -> Option<Outline> {
            None
        }
        fn image(
            &self,
            name: &[u8],
            _request: &ImageRequest,
            _intent: RenderingIntent,
        ) -> Result<Option<DecodedImage>, String> {
            let (r, g, b) = match name {
                b"A" => (200, 40, 40),
                b"B" => (40, 200, 40),
                _ => return Ok(None),
            };
            Ok(Some(DecodedImage {
                stencil: false,
                family: ColorFamily::Rgb,
                soft_masked: false,
                interpolate: false,
                width: 2,
                height: 2,
                rgb: [r, g, b].repeat(4),
                alpha: Vec::new(),
                two_colour: None,
                reduced: None,
                window: None,
            }))
        }
    }

    static IMAGES: Images = Images;

    fn renderer() -> Renderer<'static, Images> {
        let canvas = Canvas::new(SIZE, SIZE, PixelFormat::Rgb8, Color::WHITE);
        Renderer::new(canvas, page_transform(f64::from(SIZE), 1.0), &IMAGES)
    }

    /// A state filling in `(r, g, b)` at `alpha` under `ctm`.
    fn state(ctm: Matrix, (r, g, b): (u8, u8, u8), alpha: f64) -> Option<Box<GraphicsState>> {
        let mut state = GraphicsState::new(ctm);
        state.fill_color = Rgb { r, g, b };
        state.fill_alpha = alpha;
        Some(Box::new(state))
    }

    /// A rectangle in user space, whose edges are not on whole pixels.
    fn rect(x: f64, y: f64, w: f64, h: f64) -> Vec<PathSegment> {
        vec![
            PathSegment::MoveTo { x, y },
            PathSegment::LineTo { x: x + w, y },
            PathSegment::LineTo { x: x + w, y: y + h },
            PathSegment::LineTo { x, y: y + h },
            PathSegment::Close,
        ]
    }

    fn fill(x: f64, y: f64, w: f64, h: f64, colour: (u8, u8, u8), alpha: f64) -> Event {
        Event::FillPath {
            path: rect(x, y, w, h),
            even_odd: false,
            state: state(Matrix::IDENTITY, colour, alpha),
        }
    }

    fn image(name: &[u8], ctm: Matrix) -> Event {
        Event::DrawImage {
            image: ImageRef {
                name: name.to_vec(),
                inline: false,
                inline_dict: Vec::new(),
                inline_data: Vec::new(),
            },
            state: state(ctm, (0, 0, 0), 1.0),
        }
    }

    fn group(isolated: bool, knockout: bool, alpha: f64) -> Event {
        Event::BeginGroup {
            group: Group {
                isolated,
                knockout,
                space: None,
            },
            entered: true,
            state: state(Matrix::IDENTITY, (0, 0, 0), alpha),
        }
    }

    /// An abutting run of two images on the page and one inside a group, a
    /// knockout group under a blend of its own with a group nested in it, a
    /// luminosity soft mask whose group draws a fill and a group of its own,
    /// fills and an image under the mask, and the mask cleared.
    fn calls() -> Vec<Event> {
        let at = |x: f64, y: f64| Matrix {
            a: 10.3,
            b: 0.0,
            c: 0.0,
            d: 6.0,
            e: x,
            f: y,
        };
        let mut knockout = group(false, true, 0.7);
        if let Event::BeginGroup {
            state: Some(state), ..
        } = &mut knockout
        {
            state.blend = ContentBlend::Multiply;
        }
        let mask = MaskGroup {
            form: Form {
                content: Vec::new(),
                matrix: Matrix::IDENTITY,
                bbox: Some([0.0, 0.0, 32.0, 32.0]),
                group: None,
                stream: 0,
            },
            luminosity: true,
            backdrop: None,
            transfer: None,
        };
        vec![
            fill(0.5, 0.5, 31.0, 31.0, (180, 180, 220), 1.0),
            image(b"A", at(2.25, 2.0)),
            image(b"B", at(2.25, 8.0)),
            knockout,
            fill(6.5, 4.5, 14.0, 9.0, (230, 30, 30), 0.8),
            image(b"A", at(14.75, 10.0)),
            group(true, false, 0.5),
            fill(10.5, 6.5, 12.0, 12.0, (20, 20, 230), 1.0),
            Event::EndGroup,
            Event::EndGroup,
            Event::BeginSoftMask {
                mask: Box::new(mask),
                bbox: rect(0.0, 0.0, 32.0, 32.0),
                entered: true,
                state: state(Matrix::IDENTITY, (0, 0, 0), 1.0),
            },
            fill(3.0, 3.0, 20.5, 26.0, (255, 255, 255), 1.0),
            group(true, false, 1.0),
            fill(16.0, 1.0, 10.0, 10.0, (128, 128, 128), 1.0),
            Event::EndGroup,
            Event::EndSoftMask,
            fill(1.5, 18.5, 28.0, 9.0, (20, 120, 20), 1.0),
            image(b"B", at(18.0, 20.0)),
            Event::ClearSoftMask,
            fill(24.5, 1.5, 6.0, 28.0, (90, 20, 90), 0.6),
        ]
    }

    /// **The exit criterion: a snapshot changes nothing the render goes on
    /// to draw.** The calls stepped one at a time and snapshotted after every
    /// one — inside the image run, inside a knockout group and the group in
    /// it, inside the soft mask's group and the group in that — finish
    /// byte-identical, warnings included, to the same calls never
    /// snapshotted; and each snapshot taken with no bracket open is what a
    /// renderer fed the calls up to it finishes with.
    #[test]
    fn a_snapshot_changes_nothing_the_render_goes_on_to_draw() {
        let events = calls();
        let mut snapshotted = renderer();
        let mut cursor = Cursor::new();
        let mut taken = Vec::new();
        let mut inside = 0;
        while !cursor.is_finished(&events) {
            let step = cursor.step(&events, &mut snapshotted, 1);
            assert_eq!(step.delivered, 1);
            let open = !(snapshotted.groups.is_empty() && snapshotted.mask_frames.is_empty());
            inside += usize::from(open);
            taken.push((cursor.position(), open, snapshotted.snapshot()));
        }
        assert!(inside >= 10, "most snapshots are taken inside a bracket");
        assert!(taken.iter().any(|(_, open, _)| !open));

        let mut plain = renderer();
        assert_eq!(
            tinker_pdf_content::replay(&events, &mut plain).delivered,
            events.len()
        );
        let (drawn, warnings) = snapshotted.finish();
        let (expected, expected_warnings) = plain.finish();
        assert_eq!(
            drawn.data, expected.data,
            "the snapshots changed the render"
        );
        assert_eq!(warnings, expected_warnings);

        for (position, open, snapshot) in &taken {
            if *open {
                continue;
            }
            let mut fed = renderer();
            tinker_pdf_content::replay(&events[..*position], &mut fed);
            let (finished, _) = fed.finish();
            assert_eq!(
                snapshot.data, finished.data,
                "the snapshot after {position} calls is not what they finish with"
            );
        }
        // The page's last snapshot is the page.
        let (_, _, last) = taken.last().expect("a snapshot");
        assert_eq!(last.data, expected.data);
    }

    /// Inside a bracket a snapshot shows the page as the design says: a run
    /// held back is in it, an open group composited as if it closed now, and
    /// an open soft mask's group not at all — the page under it as it stood.
    #[test]
    fn a_snapshot_shows_a_run_and_an_open_group_and_not_a_mask_being_drawn() {
        let events = calls();
        let mut renderer = renderer();
        let mut cursor = Cursor::new();
        // The background and the two abutting images, the run still held.
        cursor.step(&events, &mut renderer, 3);
        assert!(renderer.run.is_some(), "the images are held as a run");
        let held = renderer.snapshot();
        assert_ne!(
            held.data, renderer.canvas.data,
            "the run is in the snapshot"
        );
        // Into the knockout group and its first fill: the snapshot is the
        // page with the group closed onto a copy of it.
        cursor.step(&events, &mut renderer, 2);
        assert_eq!((renderer.groups.len(), renderer.mask_frames.len()), (1, 0));
        let in_group = renderer.snapshot();
        assert_ne!(in_group.data, held.data, "the open group shows");
        // Into the soft mask's group: the page shows as it stood at the `gs`.
        cursor.step(&events, &mut renderer, 5);
        assert_eq!((renderer.groups.len(), renderer.mask_frames.len()), (0, 0));
        let before_mask = renderer.snapshot();
        cursor.step(&events, &mut renderer, 3);
        assert_eq!((renderer.groups.len(), renderer.mask_frames.len()), (1, 1));
        assert_eq!(
            renderer.groups[0].masks_open, 1,
            "the group opened inside the mask's"
        );
        assert_eq!(
            renderer.snapshot().data,
            before_mask.data,
            "a soft mask being drawn is no page content"
        );
    }

    /// A render stopped with brackets open — a replay cancelled inside them —
    /// ends with each closed innermost first, the open group composited and
    /// what the soft mask's group drew kept off the page: the canvas the
    /// page's, as a snapshot there shows it.
    #[test]
    fn a_render_ended_inside_brackets_closes_them_as_a_snapshot_shows() {
        let events = calls();
        for stop in 1..events.len() {
            let mut renderer = renderer();
            tinker_pdf_content::replay(&events[..stop], &mut renderer);
            let snapshot = renderer.snapshot();
            let (finished, _) = renderer.finish();
            assert_eq!(
                (finished.width, finished.height, finished.format),
                (SIZE, SIZE, PixelFormat::Rgb8),
                "stopped after {stop}: the page, not a buffer"
            );
            assert_eq!(finished.data, snapshot.data, "stopped after {stop}");
        }
    }
}

#[cfg(test)]
mod page_size_tests {
    use super::{page_pixels, page_scale, MAX_PAGE_PIXELS};

    /// The pinned contract, which callers depend on.
    #[test]
    fn a4_at_150_dpi_rounds_outward() {
        assert_eq!(page_pixels(595.0, 842.0, 150.0 / 72.0), (1240, 1755));
    }

    /// `/MediaBox [0 0 1e9 1e9]` is four tokens and, without a ceiling, asks
    /// for an allocation that aborts the process instead of failing politely.
    #[test]
    fn an_absurd_page_box_is_clamped_rather_than_allocated() {
        let (w, h) = page_pixels(1e9, 1e9, 1.0);
        let area = u64::from(w) * u64::from(h);
        assert!(
            area <= MAX_PAGE_PIXELS,
            "a hostile page box asked for {area} pixels"
        );
        assert_eq!(w, h, "and it keeps its shape");
    }

    /// A scale can be hostile even when the page box is ordinary.
    #[test]
    fn an_absurd_scale_is_clamped_too() {
        let (w, h) = page_pixels(595.0, 842.0, 100_000.0);
        assert!(u64::from(w) * u64::from(h) <= MAX_PAGE_PIXELS);
    }

    #[test]
    fn a_clamped_page_keeps_its_aspect_ratio() {
        // Twice as wide as it is tall, at a size far past the ceiling.
        let (w, h) = page_pixels(200_000.0, 100_000.0, 1.0);
        let ratio = f64::from(w) / f64::from(h);
        assert!(
            (ratio - 2.0).abs() < 0.01,
            "expected a 2:1 page, got {w}x{h}"
        );
    }

    /// **A page that is nearly all one dimension is clamped too.**
    ///
    /// `page_pixels` shrinks both sides by `sqrt(cap / area)`, which lands on
    /// the cap only while both sides stay above the one-pixel floor. A
    /// `/MediaBox` twenty points wide and 9.2e18 tall shrinks to a width of
    /// 0.125; the floor raises it back to 1 — a factor of eight the height
    /// never gave back — and the canvas came back at **eight times** the
    /// ceiling, 1 x 536 870 911, which is one 1.6 GB allocation in `Rgb8`.
    /// The `render_page` fuzz target found it as an OOM on 755 bytes.
    ///
    /// Nothing else in this module could see it: every other fixture here is
    /// square or 2:1, and both sides of those shrink well clear of the floor.
    #[test]
    fn a_page_that_is_nearly_all_one_dimension_is_clamped_too() {
        for (w, h) in [
            (20.0, 9.223_372_036_854_776e18),
            (9.223_372_036_854_776e18, 20.0),
            (1.0, 1e300),
            (1e300, 1.0),
            (3.0, 1e12),
        ] {
            for scale in [1.0, 12.0 / 72.0, 150.0 / 72.0, 4.0] {
                let used = page_scale(w, h, scale);
                let (pw, ph) = page_pixels(w, h, used);
                let area = u64::from(pw) * u64::from(ph);
                assert!(
                    area <= MAX_PAGE_PIXELS,
                    "{w}x{h} at {scale} gave {pw}x{ph} = {area} pixels,                      {}x the {MAX_PAGE_PIXELS}-pixel ceiling",
                    area / MAX_PAGE_PIXELS
                );
                assert!(pw >= 1 && ph >= 1, "{w}x{h} at {scale} gave {pw}x{ph}");
            }
        }
    }

    /// The ceiling over a lattice of shapes rather than over a handful, so a
    /// future change to the shrink cannot leave one aspect ratio over it.
    #[test]
    fn no_shape_of_page_passes_the_ceiling() {
        let sides = [
            1.0,
            2.0,
            17.0,
            595.0,
            9_000.0,
            1e6,
            1e9,
            1e15,
            1e18,
            f64::from(u32::MAX),
        ];
        for w in sides {
            for h in sides {
                for scale in [1e-6, 12.0 / 72.0, 1.0, 150.0 / 72.0, 1e6] {
                    let (pw, ph) = page_pixels(w, h, page_scale(w, h, scale));
                    let area = u64::from(pw) * u64::from(ph);
                    assert!(
                        area <= MAX_PAGE_PIXELS,
                        "{w}x{h} at {scale} gave {pw}x{ph} = {area} pixels"
                    );
                    assert!(pw >= 1 && ph >= 1);
                }
            }
        }
    }

    #[test]
    fn a_degenerate_or_infinite_box_still_yields_a_page() {
        for (w, h) in [(0.0, 0.0), (f64::NAN, 10.0), (f64::INFINITY, f64::INFINITY)] {
            let (pw, ph) = page_pixels(w, h, 1.0);
            assert!(pw >= 1 && ph >= 1, "{w}x{h} gave {pw}x{ph}");
            assert!(u64::from(pw) * u64::from(ph) <= MAX_PAGE_PIXELS);
        }
    }
}

/// Maps the content layer's blend mode onto the rasteriser's.
///
/// The two enums are deliberately separate — ruling 8 keeps the content crate
/// free of any dependency on how pixels are made — so this function is the
/// single seam between them, and the exhaustive match is what makes adding a
/// mode to one a compile error until it is added to the other.
fn blend_mode(mode: tinker_pdf_content::BlendMode) -> RasterBlend {
    use tinker_pdf_content::BlendMode as Pdf;
    match mode {
        Pdf::Normal => RasterBlend::Normal,
        Pdf::Multiply => RasterBlend::Multiply,
        Pdf::Screen => RasterBlend::Screen,
        Pdf::Overlay => RasterBlend::Overlay,
        Pdf::Darken => RasterBlend::Darken,
        Pdf::Lighten => RasterBlend::Lighten,
        Pdf::ColorDodge => RasterBlend::ColorDodge,
        Pdf::ColorBurn => RasterBlend::ColorBurn,
        Pdf::HardLight => RasterBlend::HardLight,
        Pdf::SoftLight => RasterBlend::SoftLight,
        Pdf::Difference => RasterBlend::Difference,
        Pdf::Exclusion => RasterBlend::Exclusion,
        Pdf::Hue => RasterBlend::Hue,
        Pdf::Saturation => RasterBlend::Saturation,
        Pdf::Color => RasterBlend::Color,
        Pdf::Luminosity => RasterBlend::Luminosity,
    }
}
