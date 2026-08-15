//! The rasterizing `Device`: content interpretation drawn through the
//! rasterizer.
//!
//! This crate is the bridge. The interpreter in `tinker-pdf-content` knows
//! what a page asks for; `tinker-pdf-raster` knows how to put it on pixels;
//! neither knows about the other, and this joins them.
//!
//! Scope, design and exit criteria: `docs/plans/08-rendering-device.md`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use tinker_pdf_raster::blend::BlendMode as RasterBlend;

pub mod mesh;
pub mod shading;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub use mesh::{Mesh, MeshParams};
pub use shading::Shading;
use tinker_pdf_content::{
    Device, Glyph, GraphicsState, ImageRef, LineCap as ContentCap, LineJoin as ContentJoin, Matrix,
    PathSegment,
};

use tinker_pdf_font::Outline;
use tinker_pdf_raster::{
    canvas::{Canvas, Color, MaskKind, PixelFormat},
    fill::{fill, Mask},
    geom::{FillRule, Path},
    image::{draw_image, ImageDraw, ImageSource, Pyramid, Transform},
    mesh::{draw_mesh, MeshDraw},
    stroke::{stroke, LineCap, LineJoin, StrokeStyle},
};

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
    /// The render stopped because it was cancelled.
    Cancelled,
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
    pub format: PixelFormat,
    /// `PaintType 2`'s colour, already resolved to RGB, or `None` for a
    /// coloured pattern (8.7.3.3).
    pub color: Option<(u8, u8, u8)>,
    /// The render's cancellation token, so a cell stops with the page.
    pub cancel: CancelToken,
    /// How many tiling patterns are already open above this one, for the
    /// recursion cap.
    pub depth: u32,
}

/// One rasterized tiling-pattern cell.
#[derive(Clone, Debug)]
pub struct Tile {
    /// The cell's pixels, transparent where it did not paint.
    pub canvas: Canvas,
    /// What drawing the cell had to tolerate. Merged into the page's warnings
    /// rather than dropped — a JPX inside a pattern is exactly as missing as a
    /// JPX anywhere else, and the page is the only place a caller looks.
    pub warnings: Vec<RenderWarning>,
}

/// A decoded image, ready to draw.
#[derive(Clone, Debug)]
pub struct DecodedImage {
    /// Width in samples.
    pub width: u32,
    /// Height in samples.
    pub height: u32,
    /// RGB, three bytes per sample, row-major from the top.
    pub rgb: Vec<u8>,
    /// Per-sample opacity from a mask; empty means fully opaque.
    pub alpha: Vec<u8>,
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
}

/// Where glyph outlines and image data come from, supplied by the caller so
/// this crate stays free of PDF dictionaries.
pub trait GlyphSource {
    /// The outline of one glyph, in a space where one unit is one em.
    ///
    /// Returning `None` means the glyph could not be drawn, which the renderer
    /// reports rather than substituting a shape.
    fn outline(&self, font_id: u64, code: u32) -> Option<Outline>;

    /// A named image XObject, decoded to RGB.
    ///
    /// `Err` carries the codec's name so the warning can say what was missing;
    /// `Ok(None)` means the resource is not an image at all.
    fn image(&self, name: &[u8]) -> Result<Option<DecodedImage>, String> {
        let _ = name;
        Ok(None)
    }

    /// An inline image (8.9.7), from the bytes between `BI`/`ID` and `EI`.
    ///
    /// Passed as bytes because the interpreter has no object parser; the
    /// implementor already reads dictionaries and runs filter chains for
    /// every other image, and an inline one differs only in where it lives.
    fn inline_image(&self, dict: &[u8], data: &[u8]) -> Result<Option<DecodedImage>, String> {
        let _ = (dict, data);
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

/// The most pixels one tiling-pattern fill will composite across its whole
/// lattice.
///
/// [`MAX_TILES`] alone does not bound the work, because `/XStep` may be much
/// smaller than `/BBox` — that is a legal, common way to draw an overlapping
/// weave — and then every cell costs its own area rather than its own step.
/// The product is what runs away: 65 536 cells each covering a hundredth of
/// the page is 650 pages' worth of compositing.
const MAX_TILE_WORK: u64 = 1 << 25;

/// A transparency group being rendered: everything the parent needs back.
struct GroupFrame {
    /// The canvas the group will be composited onto.
    parent: Canvas,
    /// Where the group's buffer sits on it, in the parent's device pixels.
    at: (i32, i32),
    /// `ca` at the `Do`, which applies to the group's result (11.6.6).
    alpha: f64,
    /// `/BM` at the `Do`, likewise.
    blend: RasterBlend,
    /// The clip in force at the `Do`, in the parent's coordinates.
    clip: Option<Mask>,
    /// The soft mask in force at the `Do`, in the parent's coordinates.
    ///
    /// 11.6.6 resets it to None inside the group, so it masks the group's
    /// *result* rather than each element -- which is the whole drop-shadow
    /// idiom: a mask applied element by element shows the seams the group
    /// exists to remove.
    soft: Option<Mask>,
    /// `base` in the parent's coordinates.
    base: Matrix,
    /// `/K`: the buffer as it started, so that each element can be composited
    /// against it rather than against the elements before it (11.4.5).
    ///
    /// `None` for every ordinary group, which is what keeps the second buffer
    /// off the common path — a knockout group pays for it and nothing else
    /// does.
    initial: Option<Canvas>,
    /// How deep the clip stack was when the group opened.
    ///
    /// A content stream may leave a `q` unbalanced; the interpreter drops the
    /// state it saved, but the device has already been told to push. Without
    /// this the stale entry is popped later by a `Q` from outside the group
    /// and installs a clip in the wrong coordinate space.
    clip_depth: usize,
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
    /// which is the default, and is why the bounding-box-sized mask is the
    /// common case and the page-sized one the exception.
    outside: u8,
    /// `/TR`, pre-sampled.
    transfer: Option<[u8; 256]>,
    /// The clip, soft mask and transform to put back afterwards.
    clip: Option<Mask>,
    soft: Option<Mask>,
    base: Matrix,
    clip_depth: usize,
}

/// The rasterizing device.
pub struct Renderer<'g, G: GlyphSource> {
    canvas: Canvas,
    glyphs: &'g G,
    /// The transform from PDF user space to device pixels.
    base: Matrix,
    clip: Option<Mask>,
    /// The clip and the soft mask, saved together by `q` (8.5.4, 11.6.5).
    clip_stack: Vec<(Option<Mask>, Option<Mask>)>,
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
    /// Curve flattening tolerance in device pixels.
    tolerance: f64,
    missing_fonts: u32,
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
    /// Marked-content scopes currently open, innermost last: whether each one
    /// hides what it encloses (8.11.3.2, 14.6.2).
    ///
    /// A stack rather than a counter, because `EMC` has to know whether the
    /// scope it closes was one of the hiding ones. Bounded by the
    /// interpreter's own nesting cap, which is why there is no second cap
    /// here.
    marked_content: Vec<bool>,
    /// How many of `marked_content` hide, so the question every paint asks
    /// is a comparison rather than a scan.
    hidden_depth: u32,
    /// Transparency groups open, innermost last (11.6.6).
    ///
    /// While one is open `canvas` is the group's own buffer and `base` has
    /// been translated so that device coordinates land inside it — which is
    /// what lets the buffer be bounding-box sized rather than page sized
    /// without a second coordinate system running through every paint.
    /// Group buffers opened so far, at any depth, against
    /// [`MAX_GROUP_BUFFERS`]. Spent and never refunded: unwinding a group
    /// returns its memory but not its budget, because the cost this bounds
    /// is the work already done rather than the memory still held.
    group_buffers: u32,
    /// Whether the budget above ever declined a group, reported once at
    /// `finish` rather than per decline: a page that has run out asks
    /// repeatedly, and forty thousand identical warnings is not a report.
    group_budget_spent: bool,
    groups: Vec<GroupFrame>,
    /// The soft mask in force, in the current canvas's coordinates (11.6.5).
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
}

impl<'g, G: GlyphSource> Renderer<'g, G> {
    /// A renderer drawing into a new canvas.
    ///
    /// `base` maps PDF user space to pixels, which is where the y-flip lives:
    /// PDF counts upward from the bottom-left, a raster counts downward from
    /// the top-left.
    pub fn new(canvas: Canvas, base: Matrix, glyphs: &'g G) -> Renderer<'g, G> {
        Renderer {
            canvas,
            glyphs,
            base,
            clip: None,
            clip_stack: Vec::new(),
            warnings: Vec::new(),
            cancel: CancelToken::new(),
            skipped: Arc::new(AtomicBool::new(false)),
            tolerance: 0.2,
            missing_fonts: 0,
            text_clip: None,
            text_clip_requested: false,
            marked_content: Vec::new(),
            hidden_depth: 0,
            group_buffers: 0,
            group_budget_spent: false,
            groups: Vec::new(),
            soft: None,
            mask_frames: Vec::new(),
            pattern_depth: 0,
            #[cfg(test)]
            mask_pixels: 0,
        }
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
        self.hidden_depth > 0
    }

    /// Installs a cancellation token.
    #[must_use]
    pub fn with_cancel(mut self, cancel: CancelToken) -> Self {
        self.cancel = cancel;
        self
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

    /// Record that the group budget declined one, for a single report at
    /// `finish`.
    fn note_group_budget(&mut self) {
        self.group_budget_spent = true;
    }

    /// The canvas and everything the render had to tolerate.
    #[must_use]
    pub fn finish(mut self) -> (Canvas, Vec<RenderWarning>) {
        // A group that was opened and never closed would otherwise hand the
        // caller the *group's* buffer as the page. The interpreter balances
        // its own calls, but `Renderer` is public and this is the one failure
        // that turns a page into a fragment rather than into wrong pixels.
        while !self.groups.is_empty() {
            self.close_group();
        }
        if self.missing_fonts > 0 {
            self.warnings.push(RenderWarning::UnreadableFont);
        }
        if self.group_budget_spent {
            self.warnings.push(RenderWarning::GroupBudgetSpent {
                opened: self.group_buffers,
            });
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
    fn open_group(&mut self, group: tinker_pdf_content::Group, state: &GraphicsState) -> bool {
        if self.groups.len() >= MAX_GROUP_DEPTH {
            return false;
        }
        // Depth is not work once the recursion branches. See
        // `MAX_GROUP_BUFFERS`.
        if self.group_buffers >= MAX_GROUP_BUFFERS {
            self.note_group_budget();
            return false;
        }
        self.group_buffers = self.group_buffers.saturating_add(1);
        let (x0, y0, width, height) =
            self.clip
                .as_ref()
                .map_or((0, 0, self.canvas.width, self.canvas.height), |clip| {
                    let (x0, y0, x1, y1) = clip.overlap(self.canvas.width, self.canvas.height);
                    (x0 as i32, y0 as i32, x1 - x0, y1 - y0)
                });
        if width == 0 || height == 0 {
            return false;
        }

        // A group buffer must carry alpha whatever the page format is: it
        // starts as nothing and accumulates a shape, and a format without an
        // alpha channel has no way to say where the group did not paint.
        let format = match self.canvas.format {
            PixelFormat::Gray8 | PixelFormat::GrayA8 => PixelFormat::GrayA8,
            PixelFormat::Rgb8 | PixelFormat::Rgba8 => PixelFormat::Rgba8,
        };
        let mut buffer = Canvas::new(width, height, format, Color::TRANSPARENT);
        // 11.4.4: an isolated group composites against nothing, so its buffer
        // starts empty; a non-isolated one starts with the backdrop composited
        // in, which is what lets a blend mode inside it see through to the
        // page. The backdrop is then taken out again at close (11.4.7.2), or
        // it is counted twice — once because the group started from it and
        // once because the group is composited back onto it. That failure
        // renders as a group that is *weaker* than it should be, pulled
        // toward whatever was underneath, which is a plausible image.
        if !group.isolated {
            let backdrop = self.canvas.extract((x0, y0), width, height, format);
            buffer.adopt_backdrop(backdrop);
        }

        let initial = group.knockout.then(|| buffer.snapshot());

        let frame = GroupFrame {
            parent: std::mem::replace(&mut self.canvas, buffer),
            at: (x0, y0),
            alpha: state.fill_alpha,
            blend: blend_mode(state.blend),
            clip: self.clip.clone(),
            soft: self.soft.take(),
            base: self.base,
            initial,
            clip_depth: self.clip_stack.len(),
        };

        // Everything inside the group draws in the buffer's coordinates. The
        // translation goes into `base`, which every path, glyph, image and
        // shading already passes through, rather than into a second origin
        // that each of them would have to remember to apply.
        self.base = self
            .base
            .then(&Matrix::translate(-f64::from(x0), -f64::from(y0)));
        self.clip = frame.clip.as_ref().map(|clip| shifted(clip, -x0, -y0));
        self.groups.push(frame);
        true
    }

    /// Whether the innermost open group is a knockout one (11.4.5).
    fn in_knockout(&self) -> bool {
        self.groups
            .last()
            .is_some_and(|frame| frame.initial.is_some())
    }

    /// Discards what earlier elements left where this one is about to paint
    /// (11.4.5).
    ///
    /// Called by every paint that can reach a group buffer, with the coverage
    /// it is about to composite through. Doing it here rather than inside the
    /// composite is what makes "each element" mean an operator: a fill, a
    /// stroke, a glyph, an image, a shading, or a nested group's result.
    fn knockout_restore(&mut self, mask: &Mask) {
        let Some(frame) = self.groups.last_mut() else {
            return;
        };
        // Moved out and put back, because the restore needs the canvas and
        // the frame at once and they are two fields of the same `self`.
        let Some(initial) = frame.initial.take() else {
            return;
        };
        self.canvas.knock_out(&initial, mask);
        if let Some(frame) = self.groups.last_mut() {
            frame.initial = Some(initial);
        }
    }

    /// Closes the innermost group and composites it onto its parent.
    ///
    /// Restoring comes before compositing, unconditionally, so a cancelled
    /// render still hands back the page rather than the group's buffer.
    fn close_group(&mut self) {
        let Some(frame) = self.groups.pop() else {
            return;
        };
        self.clip_stack.truncate(frame.clip_depth);
        let mut buffer = std::mem::replace(&mut self.canvas, frame.parent);
        self.base = frame.base;
        self.clip = frame.clip;
        let soft = frame.soft;

        // 11.4.7.2. A no-op on an isolated group, which kept no backdrop.
        buffer.remove_backdrop();

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
        self.canvas.composite(
            &buffer,
            frame.at,
            frame.alpha,
            frame.blend,
            soft.as_ref(),
            Some(&stop),
        );
        self.soft = soft;
    }

    /// Opens a soft-mask group: a buffer filled with `/BC`, to be read back
    /// as a mask when it closes (11.6.5.2).
    fn open_soft_mask(
        &mut self,
        mask: &tinker_pdf_content::MaskGroup,
        bbox: &[PathSegment],
    ) -> bool {
        if self.mask_frames.len() + self.groups.len() >= MAX_GROUP_DEPTH {
            return false;
        }
        // A mask group is a group buffer and spends from the same budget --
        // and it is the one the campaign's timeout actually recursed through.
        if self.group_buffers >= MAX_GROUP_BUFFERS {
            self.note_group_budget();
            return false;
        }
        self.group_buffers = self.group_buffers.saturating_add(1);
        // The group's own bounding box bounds the buffer, and the current
        // clip deliberately does *not*: the mask applies to whatever is
        // painted after the `gs`, which may be anywhere, and clipping the
        // mask to the clip in force when it was made would leave those paints
        // masked by nothing outside it.
        let built = self.to_path(bbox);
        let (x0, y0, width, height) = if built.is_empty() {
            (0, 0, self.canvas.width, self.canvas.height)
        } else {
            paint_region(&built, None, None, self.canvas.width, self.canvas.height)
        };
        if width == 0 || height == 0 {
            return false;
        }

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
        // through `/TR`. Zero for `/Alpha`, and zero for the `/BC` default,
        // which is why the common mask costs its bounding box.
        let outside = {
            let raw = match kind {
                MaskKind::Luminosity => background.luma(),
                MaskKind::Alpha => 0,
            };
            mask.transfer
                .as_ref()
                .map_or(raw, |lut| lut.get(raw as usize).copied().unwrap_or(raw))
        };

        let format = match self.canvas.format {
            PixelFormat::Gray8 | PixelFormat::GrayA8 => PixelFormat::GrayA8,
            PixelFormat::Rgb8 | PixelFormat::Rgba8 => PixelFormat::Rgba8,
        };
        let buffer = Canvas::new(width, height, format, background);

        let frame = MaskFrame {
            parent: std::mem::replace(&mut self.canvas, buffer),
            at: (x0, y0),
            kind,
            outside,
            transfer: mask.transfer,
            clip: self.clip.take(),
            soft: self.soft.take(),
            base: self.base,
            clip_depth: self.clip_stack.len(),
        };
        // The mask group draws in its own buffer's coordinates, and under no
        // clip and no mask of its own: it is a picture of a mask, not a
        // painting on the page.
        self.base = self
            .base
            .then(&Matrix::translate(-f64::from(x0), -f64::from(y0)));
        self.mask_frames.push(frame);
        true
    }

    /// Closes the innermost soft-mask group and installs what it drew.
    fn close_soft_mask(&mut self) {
        let Some(frame) = self.mask_frames.pop() else {
            return;
        };
        self.clip_stack.truncate(frame.clip_depth);
        let buffer = std::mem::replace(&mut self.canvas, frame.parent);
        self.base = frame.base;
        self.clip = frame.clip;

        let inner = buffer.to_mask(frame.at, frame.kind, frame.transfer.as_ref());
        // A `Mask` reads zero outside its own rectangle, which is exactly
        // right when the backdrop is black and exactly wrong otherwise: with
        // a pale `/BC` the whole page outside the group's box is *unmasked*,
        // and a bounding-box-sized mask would hide it. So the field is only
        // paid for when the file asks for one.
        self.soft = Some(if frame.outside == 0 {
            inner
        } else {
            let mut field =
                Mask::uniform(0, 0, self.canvas.width, self.canvas.height, frame.outside);
            field.overwrite(&inner);
            field
        });
        // The mask the `gs` replaced is gone, not restored: 11.6.5.1 makes
        // `/SMask` a graphics-state parameter that one `gs` overwrites for
        // the next, and only `Q` puts an older one back.
        let _ = frame.soft;
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
        self.paint(
            &path,
            FillRule::NonZero,
            PLACEHOLDER,
            state.fill_alpha,
            blend_mode(state.blend),
        );
    }

    /// Fills a path with a pattern, or reports one that cannot be.
    ///
    /// 8.7.3.1: a pattern's matrix maps pattern space to the *default* space
    /// of the page, not to the space in force when it is used. The CTM at fill
    /// time is therefore not part of it, which is why `base` appears in both
    /// branches below and `state.ctm` appears in neither. A stroke reaches
    /// this through its outline, so that guarantee covers strokes too — the
    /// stroker's transform never enters the pattern.
    ///
    /// `alpha` is a parameter rather than a field of `state` because 8.4.5
    /// gives painting two of them: a fill uses `ca`, a stroke uses `CA`, and
    /// the callers are the only things that know which they are. `color` is a
    /// parameter for exactly the same reason, and it is the same mistake
    /// waiting to be made: 8.7.3.3 gives an uncoloured pattern the colour of
    /// the slot that named it, and a stroke's slot is not a fill's.
    fn fill_with_pattern(
        &mut self,
        path: &Path,
        rule: FillRule,
        name: &[u8],
        state: &GraphicsState,
        alpha: f64,
        color: Color,
    ) {
        match self.glyphs.pattern(name) {
            Some(PatternPaint::Shading(shading, matrix)) => {
                // A mesh too large to paint is reported as the unpainted
                // pattern it is, exactly as an unreadable tiling cell is: the
                // area is left alone rather than filled with black.
                if !self.fill_with_shading(path, rule, &shading, matrix, state, alpha) {
                    self.report_unpainted_pattern(name);
                }
            }
            Some(PatternPaint::Tiling(tiling)) => {
                if !self.fill_with_tiles(path, rule, name, &tiling, state, alpha, color) {
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

    /// Fills a path with a shading pattern.
    ///
    /// The path becomes the clip and the shading is evaluated per pixel inside
    /// it, which is the same machinery `sh` uses — a shading pattern is `sh`
    /// bounded by a path rather than by the current clip.
    ///
    /// False means nothing was painted and the caller should say so: a mesh
    /// past its work budget is the only way that happens.
    fn fill_with_shading(
        &mut self,
        path: &Path,
        rule: FillRule,
        shading: &Shading,
        matrix: Matrix,
        state: &GraphicsState,
        alpha: f64,
    ) -> bool {
        let to_device = matrix.then(&self.base);
        let stop = self.stop_predicate();
        let area = self.coverage(path, rule, Some(&stop));
        let alpha = alpha.clamp(0.0, 1.0);
        let mode = blend_mode(state.blend);

        // A mesh is geometry rather than a function of position, so it takes
        // the other route entirely — and it does its own knockout restore,
        // with its own coverage rather than the shape's.
        if let Some(mesh) = shading.mesh() {
            return self.paint_mesh(mesh, to_device, Some(&area), alpha, mode);
        }

        let Some(inverse) = invert(&to_device) else {
            return true;
        };
        if self.in_knockout() {
            self.knockout_restore(&area);
        }
        // The shape's own rectangle. A shading is evaluated per pixel, so a
        // gradient-filled comma used to run the inverse transform and the
        // function over every pixel of the page to paint two hundred of them.
        let (x_start, y_start, x_end, y_end) = area.overlap(self.canvas.width, self.canvas.height);
        for py in y_start..y_end {
            if self.stopping() {
                return true;
            }
            for px in x_start..x_end {
                let coverage = area.at(px as i32, py as i32);
                if coverage == 0 {
                    continue;
                }
                let (x, y) = inverse.apply(f64::from(px) + 0.5, f64::from(py) + 0.5);
                let Some((r, g, b)) = shading.color_at(x, y) else {
                    continue;
                };
                let weight = alpha * f64::from(coverage) / 255.0;
                self.canvas
                    .blend_pixel_with(px, py, Color { r, g, b, a: 0xFF }, weight, mode);
            }
        }
        true
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
    fn paint_mesh(
        &mut self,
        mesh: &Mesh,
        to_device: Matrix,
        area: Option<&Mask>,
        alpha: f64,
        mode: RasterBlend,
    ) -> bool {
        let Some(tessellation) = mesh.tessellate(&to_device) else {
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
        let (x0, y0, width, height) =
            paint_region(&bbox, area, None, self.canvas.width, self.canvas.height);
        if width == 0 || height == 0 {
            return true;
        }
        #[cfg(test)]
        {
            self.mask_pixels = self
                .mask_pixels
                .saturating_add(u64::from(width) * u64::from(height));
        }

        let stop = self.stop_predicate();
        // 8.7.4.5.5: the vertex values are interpolated and *then* converted,
        // which is what this closure is. Handing the rasteriser RGB at the
        // vertices instead would interpolate in device space and move the
        // midpoint of every non-linear colour space and every non-linear
        // `/Function`.
        let convert = |inputs: &[f64]| {
            let (r, g, b) = mesh.color(inputs);
            Color { r, g, b, a: 0xFF }
        };
        let draw = MeshDraw {
            positions: &tessellation.positions,
            values: &tessellation.values,
            components: mesh.components,
            triangles: &tessellation.triangles,
            color: &convert,
            stop: Some(&stop),
        };
        let Some(mut buffer) = draw_mesh(&draw, x0, y0, width, height) else {
            return false;
        };

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

        let (x_start, y_start, x_end, y_end) = buffer
            .coverage
            .overlap(self.canvas.width, self.canvas.height);
        for py in y_start..y_end {
            if self.stopping() {
                return true;
            }
            for px in x_start..x_end {
                let coverage = buffer.coverage.at(px as i32, py as i32);
                if coverage == 0 {
                    continue;
                }
                let Some(color) = buffer.at(px as i32, py as i32) else {
                    continue;
                };
                let weight = alpha * f64::from(coverage) / 255.0;
                self.canvas.blend_pixel_with(px, py, color, weight, mode);
            }
        }
        true
    }

    /// Fills a path with a tiling pattern: one cell, rasterized once, blitted
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
        path: &Path,
        rule: FillRule,
        name: &[u8],
        tiling: &TilingPattern,
        state: &GraphicsState,
        alpha: f64,
        color: Color,
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
        let (rx, ry, rw, rh) = paint_region(
            path,
            self.clip.as_ref(),
            self.soft.as_ref(),
            self.canvas.width,
            self.canvas.height,
        );
        if rw == 0 || rh == 0 {
            return true;
        }

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
        let canvas_area = u64::from(self.canvas.width) * u64::from(self.canvas.height);
        if tiles.saturating_mul(area.min(canvas_area.max(1))) > MAX_TILE_WORK {
            return false;
        }

        // A cell is a shape over nothing, so its buffer must carry alpha
        // whatever the page's format is.
        let format = match self.canvas.format {
            PixelFormat::Gray8 | PixelFormat::GrayA8 => PixelFormat::GrayA8,
            PixelFormat::Rgb8 | PixelFormat::Rgba8 => PixelFormat::Rgba8,
        };
        let request = TileRequest {
            to_pixels: to_device.then(&Matrix::translate(-tx0, -ty0)),
            bbox: tiling.bbox,
            width,
            height,
            format,
            color: tiling.uncolored.then_some((color.r, color.g, color.b)),
            cancel: self.cancel.clone(),
            depth: self.pattern_depth + 1,
        };
        let Some(tile) = self.glyphs.tile(name, &request) else {
            return false;
        };
        for warning in tile.warnings {
            if !self.warnings.contains(&warning) {
                self.warnings.push(warning);
            }
        }
        let tile = tile.canvas;

        let stop = self.stop_predicate();
        let coverage = self.coverage(path, rule, Some(&stop));
        if self.in_knockout() {
            self.knockout_restore(&coverage);
        }
        let mode = blend_mode(state.blend);

        // The lattice step as a device *vector*: a translation in pattern
        // space, so the matrix's translation cancels and only its linear part
        // survives.
        let origin = to_device.apply(0.0, 0.0);
        for j in j0..=j1 {
            // Gap 15's hook. A large lattice is precisely the long-running
            // work it exists for, and `composite` is asked again every sixteen
            // rows inside each cell.
            if self.stopping() {
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
                self.canvas
                    .composite(&tile, at, alpha, mode, Some(&coverage), Some(&stop));
            }
        }
        true
    }

    /// The coverage a path contributes, over the pixels it can reach and with
    /// the current clip already multiplied in.
    ///
    /// Every paint and every clip goes through here, and this is the only
    /// place that decides how large a mask is. A comma used to allocate and
    /// scan a full page: eight megabytes at 300 dpi, per glyph, twice for
    /// stroked text. It now costs its own bounding box.
    fn coverage(&mut self, path: &Path, rule: FillRule, stop: Option<&dyn Fn() -> bool>) -> Mask {
        let (x0, y0, width, height) = paint_region(
            path,
            self.clip.as_ref(),
            self.soft.as_ref(),
            self.canvas.width,
            self.canvas.height,
        );
        #[cfg(test)]
        {
            self.mask_pixels = self
                .mask_pixels
                .saturating_add(u64::from(width) * u64::from(height));
        }
        let mut mask = fill(path, rule, x0, y0, width, height, self.tolerance, stop);
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
    fn paint(&mut self, path: &Path, rule: FillRule, color: Color, alpha: f64, mode: RasterBlend) {
        if self.stopping() {
            return;
        }
        let stop = self.stop_predicate();
        let mask = self.coverage(path, rule, Some(&stop));
        if self.in_knockout() {
            self.knockout_restore(&mask);
        }
        self.canvas.fill_mask_with(&mask, color, alpha, mode);
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
        // 8.9.6.2: a stencil selects where the *current fill colour* is
        // painted; it has no colour of its own.
        let tint = image.stencil.then(|| fill_color(state));
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
            let area = self.coverage(&quad, FillRule::NonZero, None);
            self.knockout_restore(&area);
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
            },
            unit_to_device,
            interpolate: image.interpolate,
            alpha: state.fill_alpha,
            blend: blend_mode(state.blend),
            clip,
            tint,
            stop: Some(&stop),
        };
        // Built here and dropped here. The levels are the caller's to keep,
        // and keeping them needs an identity for the image rather than a
        // shape — two 512 x 512 photographs are indistinguishable to this
        // function. That identity lives with the resource cache, which is
        // phase 08's, so this deliberately does not invent one.
        let mut pyramid = Pyramid::new();
        draw_image(&mut self.canvas, &draw, &mut pyramid);
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

/// The device pixels a path can put ink on: its bounding box widened by
/// [`SLACK`], clipped to the canvas and to the clip's own rectangle.
///
/// Clipping to the clip's rectangle is safe for the same reason the whole
/// change is: a clip reports zero coverage outside itself, and coverage
/// multiplies, so nothing outside it could ever have been painted.
///
/// A path with no points at all — which is what a text object that selected a
/// clipping mode and drew nothing produces — yields an empty region, and an
/// empty mask reads as zero everywhere. That is the same "clip everything
/// away" a page-sized mask of zeroes gave, at none of the cost.
fn paint_region(
    path: &Path,
    clip: Option<&Mask>,
    soft: Option<&Mask>,
    width: u32,
    height: u32,
) -> (i32, i32, u32, u32) {
    let Some((x0, y0, x1, y1)) = path.bounds() else {
        return (0, 0, 0, 0);
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

    // Both bound the same way and for the same reason: coverage multiplies,
    // and both report zero outside their own rectangle, so nothing out there
    // could ever have been painted.
    for mask in [clip, soft].into_iter().flatten() {
        left = left.max(i64::from(mask.x0));
        top = top.max(i64::from(mask.y0));
        right = right.min(i64::from(mask.x0) + i64::from(mask.width));
        bottom = bottom.min(i64::from(mask.y0) + i64::from(mask.height));
    }

    let left = left.clamp(0, i64::from(width));
    let top = top.clamp(0, i64::from(height));
    let right = right.clamp(left, i64::from(width));
    let bottom = bottom.clamp(top, i64::from(height));

    (
        left as i32,
        top as i32,
        (right - left) as u32,
        (bottom - top) as u32,
    )
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

impl<G: GlyphSource> Device for Renderer<'_, G> {
    fn is_cancelled(&self) -> bool {
        self.stopping()
    }

    fn fill_path(&mut self, path: &[PathSegment], state: &GraphicsState, even_odd: bool) {
        if self.stopping() || self.hidden() {
            return;
        }
        let built = self.to_path(path);
        if built.is_empty() {
            return;
        }
        let rule = if even_odd {
            FillRule::EvenOdd
        } else {
            FillRule::NonZero
        };

        // 8.7.3: a pattern supplies the paint, so the fill colour is not used
        // at all. Painting it would put `/Pattern`'s nominal black over every
        // gradient in the document.
        if let Some(name) = state.fill_pattern.clone() {
            self.fill_with_pattern(
                &built,
                rule,
                &name,
                state,
                state.fill_alpha,
                fill_color(state),
            );
            return;
        }

        let color = fill_color(state);
        self.paint(
            &built,
            rule,
            color,
            state.fill_alpha,
            blend_mode(state.blend),
        );
    }

    fn clip_path(&mut self, path: &[PathSegment], _state: &GraphicsState, even_odd: bool) {
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
        self.clip = Some(self.coverage(&built, rule, Some(&stop)));
    }

    fn end_text(&mut self) {
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
        let stop = self.stop_predicate();
        self.clip = Some(self.coverage(&path, FillRule::NonZero, Some(&stop)));
    }

    fn save_state(&mut self) {
        self.clip_stack.push((self.clip.clone(), self.soft.clone()));
    }

    fn restore_state(&mut self) {
        // Ruling 8: the soft mask is pixels and lives here rather than in the
        // graphics state, so `Q` has to put it back -- otherwise an `/SMask`
        // set inside `q ... Q` keeps masking everything drawn after the `Q`,
        // which is content quietly missing from the rest of the page.
        if let Some((clip, soft)) = self.clip_stack.pop() {
            self.clip = clip;
            self.soft = soft;
        }
    }

    fn stroke_path(&mut self, path: &[PathSegment], state: &GraphicsState) {
        if self.stopping() || self.hidden() {
            return;
        }
        let built = self.to_path(path);
        if built.is_empty() {
            return;
        }

        // 8.4.3.2: the line width is in user space, so the transform scales
        // it. A hairline still has to cover a pixel to be visible.
        //
        // The dashes scale by the same factor, and for the same reason: a
        // pattern measured in user space and applied in device space comes out
        // at the wrong pitch under any zoom.
        let scale = state.ctm.then(&self.base).expansion();
        let width = (state.line_width * scale).max(0.8);
        let style = StrokeStyle {
            width,
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
            dashes: state.dashes.iter().map(|d| d * scale).collect(),
            dash_phase: state.dash_phase * scale,
        };
        // The dash expansion below is the part of a stroke that can run away:
        // 8.4.3.6 puts no floor on a dash length, so a page-long rule under a
        // fine array expands into a hundred thousand pieces per segment before
        // a single row of it is filled.
        let stop = self.stop_predicate();
        let outline = stroke(&built, &style, self.tolerance, Some(&stop));

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
                &outline,
                FillRule::NonZero,
                &name,
                state,
                state.stroke_alpha,
                stroke_color(state),
            );
            return;
        }

        let color = stroke_color(state);
        self.paint(
            &outline,
            FillRule::NonZero,
            color,
            state.stroke_alpha,
            blend_mode(state.blend),
        );
    }

    fn show_glyph(&mut self, glyph: &Glyph, state: &GraphicsState) {
        // Before the clipping mode is recorded, and before the outline is
        // looked for. A glyph in a hidden layer must not add to the text
        // clip -- 9.3.6's modes 4 to 7 would otherwise let an invisible
        // layer clip away the visible ones -- and must not be counted as an
        // unreadable font, which is a warning about a page that is missing
        // something rather than about one that is doing as it was told.
        if self.stopping() || self.hidden() {
            return;
        }
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

        // The glyph's transform maps its em square into user space; the base
        // transform takes that to pixels.
        let combined = glyph.transform.then(&self.base);
        if !combined.is_finite() {
            return;
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
                    &path,
                    FillRule::NonZero,
                    &name,
                    state,
                    state.fill_alpha,
                    fill_color(state),
                );
            } else {
                self.paint(
                    &path,
                    FillRule::NonZero,
                    fill_color(state),
                    state.fill_alpha,
                    blend_mode(state.blend),
                );
            }
        }
        if mode.strokes() {
            let scale = state.ctm.then(&self.base).expansion();
            let style = StrokeStyle {
                width: (state.line_width * scale).max(0.6),
                ..StrokeStyle::default()
            };
            let stop = self.stop_predicate();
            let outlined = stroke(&path, &style, self.tolerance, Some(&stop));
            if let Some(name) = state.stroke_pattern.clone() {
                self.fill_with_pattern(
                    &outlined,
                    FillRule::NonZero,
                    &name,
                    state,
                    state.stroke_alpha,
                    stroke_color(state),
                );
            } else {
                self.paint(
                    &outlined,
                    FillRule::NonZero,
                    stroke_color(state),
                    state.stroke_alpha,
                    blend_mode(state.blend),
                );
            }
        }
        if mode.clips() {
            // 9.3.6: the glyphs of a text object accumulate into one clip that
            // takes effect at `ET`, not glyph by glyph — clipping immediately
            // would leave each glyph clipped to itself, which is empty.
            self.text_clip.get_or_insert_with(Path::new).extend(&path);
        }
    }

    fn draw_image(&mut self, image: &ImageRef, state: &GraphicsState) {
        // Before the decode, not after it. An image in a hidden layer is not
        // drawn, so whether this build could have drawn it is not a fact
        // about the page: decoding first would report `UnsupportedImage` for
        // a JPX nobody was going to see, and paint the grey placeholder that
        // goes with it.
        if self.stopping() || self.hidden() {
            return;
        }

        let decoded = if image.inline {
            match self
                .glyphs
                .inline_image(&image.inline_dict, &image.inline_data)
            {
                Ok(Some(decoded)) => Ok(decoded),
                Ok(None) => Err("inline".to_string()),
                Err(codec) => Err(codec),
            }
        } else {
            match self.glyphs.image(&image.name) {
                Ok(Some(decoded)) => Ok(decoded),
                Ok(None) => Err(String::from_utf8_lossy(&image.name).into_owned()),
                Err(codec) => Err(codec),
            }
        };

        match decoded {
            Ok(decoded) => self.blit(&decoded, state),
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
        if self.stopping() || self.hidden() {
            return;
        }

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

        // A mesh paints its own geometry rather than the whole clip, so it
        // brings its own coverage and its own knockout restore with it.
        if let Some(mesh) = shading.mesh() {
            if !self.paint_mesh(mesh, to_device, area.as_ref(), alpha, mode) {
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
        let (x_start, y_start, x_end, y_end) = area
            .as_ref()
            .map_or((0, 0, self.canvas.width, self.canvas.height), |mask| {
                mask.overlap(self.canvas.width, self.canvas.height)
            });
        // 11.4.5: `sh` paints the whole clip, so the clip *is* this element's
        // coverage — and with no clip in force it really is the whole buffer.
        if self.in_knockout() {
            let restore = area
                .clone()
                .unwrap_or_else(|| Mask::uniform(0, 0, self.canvas.width, self.canvas.height, 255));
            self.knockout_restore(&restore);
        }
        for py in y_start..y_end {
            if self.stopping() {
                return;
            }
            for px in x_start..x_end {
                let clip = area
                    .as_ref()
                    .map_or(255, |mask| mask.at(px as i32, py as i32));
                if clip == 0 {
                    continue;
                }

                let (x, y) = inverse.apply(f64::from(px) + 0.5, f64::from(py) + 0.5);
                let Some((r, g, b)) = shading.color_at(x, y) else {
                    continue;
                };
                let effective = alpha * f64::from(clip) / 255.0;
                self.canvas
                    .blend_pixel_with(px, py, Color::rgb(r, g, b), effective, mode);
            }
        }
    }

    fn begin_marked_content(&mut self, visible: bool, hidden_layer: Option<&str>) {
        self.marked_content.push(!visible);
        if !visible {
            self.hidden_depth = self.hidden_depth.saturating_add(1);
        }
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
        if self.marked_content.pop() == Some(true) {
            self.hidden_depth = self.hidden_depth.saturating_sub(1);
        }
    }

    fn begin_form(&mut self, _id: u64) -> bool {
        self.clip_stack.push((self.clip.clone(), self.soft.clone()));
        !self.stopping()
    }

    fn end_form(&mut self, _id: u64) {
        if let Some((clip, soft)) = self.clip_stack.pop() {
            self.clip = clip;
            self.soft = soft;
        }
    }

    fn begin_group(&mut self, group: tinker_pdf_content::Group, state: &GraphicsState) -> bool {
        // A group inside a layer that is off paints nothing, so there is
        // nothing to buffer. Declining also leaves the interpreter's 11.6.6
        // state reset undone, which is right: nothing is going to paint.
        if self.stopping() || self.hidden() {
            return false;
        }
        self.open_group(group, state)
    }

    fn end_group(&mut self) {
        self.close_group();
    }

    fn begin_soft_mask(
        &mut self,
        mask: &tinker_pdf_content::MaskGroup,
        bbox: &[PathSegment],
        _state: &GraphicsState,
    ) -> bool {
        if self.stopping() {
            return false;
        }
        self.open_soft_mask(mask, bbox)
    }

    fn end_soft_mask(&mut self) {
        self.close_soft_mask();
    }

    fn clear_soft_mask(&mut self) {
        self.soft = None;
    }
}

/// The same mask, addressed from a different origin.
///
/// Group buffers are bounding-box sized, so a clip that came from outside one
/// has to be re-addressed on the way in and back again on the way out. The
/// coverage bytes are untouched; only where they are read from moves.
fn shifted(mask: &Mask, dx: i32, dy: i32) -> Mask {
    Mask {
        x0: mask.x0.saturating_add(dx),
        y0: mask.y0.saturating_add(dy),
        width: mask.width,
        height: mask.height,
        data: mask.data.clone(),
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

/// The largest canvas this will allocate for one page, in pixels.
///
/// About 67 million, which is 201 MB at three bytes a pixel — larger than any
/// legitimate single-page render, and small enough that a hostile file cannot
/// exhaust memory with it. A caller who genuinely wants more renders in tiles,
/// which go through the same path with a translated viewport (ruling 7).
///
/// The cap exists because the page box is attacker-controlled: `/MediaBox
/// [0 0 1e9 1e9]` is four tokens, and without a ceiling it asks for an
/// allocation no machine can serve. A failed allocation **aborts** the process
/// rather than unwinding, so it cannot be caught and reported afterwards —
/// which makes this the one place it has to be prevented rather than handled.
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
    (clamp(w), clamp(h))
}

/// Convenience: a white canvas of the right size for a page.
#[must_use]
pub fn page_canvas(width_pt: f64, height_pt: f64, scale: f64, format: PixelFormat) -> Canvas {
    let (w, h) = page_pixels(width_pt, height_pt, scale);
    Canvas::new(w, h, format, Color::WHITE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tinker_pdf_content::interpret;
    use tinker_pdf_content::FontSource;

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
                let (x0, y0, w, h) = paint_region(&path, None, None, SIDE, SIDE);
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
            paint_region(&path, None, None, 2000, 2000),
            (3, 5, 11, 9),
            "the bounding box, one pixel of slack on each side"
        );

        // Clamped to the canvas rather than reaching outside it.
        let mut over = Path::new();
        over.rect(-50.0, -50.0, 60.0, 60.0);
        assert_eq!(paint_region(&over, None, None, 100, 100), (0, 0, 11, 11));

        // A path that misses the canvas costs nothing at all.
        let mut away = Path::new();
        away.rect(500.0, 500.0, 10.0, 10.0);
        assert_eq!(paint_region(&away, None, None, 100, 100), (100, 100, 0, 0));

        // Neither does one with no points.
        assert_eq!(
            paint_region(&Path::new(), None, None, 100, 100),
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

        let (x0, y0, w, h) = paint_region(&path, Some(&clip), None, 200, 200);
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

        fn image(&self, name: &[u8]) -> Result<Option<DecodedImage>, String> {
            if name != b"Im0" {
                return Ok(None);
            }
            self.cancel.cancel();
            Ok(Some(DecodedImage {
                stencil: false,
                interpolate: false,
                width: 2,
                height: 2,
                rgb: vec![0; 12],
                alpha: Vec::new(),
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

    /// A source that answers with one solid-colour image.
    struct OneImage {
        color: (u8, u8, u8),
        alpha: Vec<u8>,
    }

    impl GlyphSource for OneImage {
        fn outline(&self, _font_id: u64, _code: u32) -> Option<Outline> {
            None
        }
        fn image(&self, name: &[u8]) -> Result<Option<DecodedImage>, String> {
            if name != b"Im0" {
                return Ok(None);
            }
            let (r, g, b) = self.color;
            Ok(Some(DecodedImage {
                stencil: false,
                interpolate: false,
                width: 2,
                height: 2,
                rgb: vec![r, g, b, r, g, b, r, g, b, r, g, b],
                alpha: self.alpha.clone(),
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
        fn image(&self, name: &[u8]) -> Result<Option<DecodedImage>, String> {
            if name != b"Im0" {
                return Ok(None);
            }
            Ok(Some(DecodedImage {
                stencil: false,
                interpolate: self.interpolate,
                width: 2,
                height: 2,
                #[rustfmt::skip]
                rgb: vec![
                    0, 0, 0,       255, 255, 255,
                    255, 255, 255, 0, 0, 0,
                ],
                alpha: Vec::new(),
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
}

#[cfg(test)]
mod page_size_tests {
    use super::{page_pixels, MAX_PAGE_PIXELS};

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
