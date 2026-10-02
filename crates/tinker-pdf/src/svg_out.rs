//! A page written as SVG 1.1: [`Page::to_svg`] and [`DisplayList::to_svg`].
//!
//! # A device, fed from the retained page
//!
//! The writer is a [`Device`] like the renderer and the text extractor
//! (ruling 7), and it is fed by replaying the page's display list rather
//! than by interpreting the page a third time: the list's calls, its
//! resources and its annotations' resource scopes are exactly what a device
//! needs, and a caller who already holds a [`DisplayList`] for rendering pays
//! nothing more to write it out.
//!
//! # What the output is, decided
//!
//! - **Coordinates are the page's displayed points**, crop box and `/Rotate`
//!   applied and `y` running down — `page_view_transform` at scale 1 — with a
//!   root `width` and `height` in `pt` and a `viewBox` of the same numbers,
//!   so one user unit is one point and the picture has the page's physical
//!   size. Every path is written with its transform already applied; only an
//!   image and a gradient carry a `transform`, because a unit square and a
//!   gradient axis are not points.
//! - **Numbers are written to four decimal places** — a ten-thousandth of a
//!   point — rounded, with trailing zeros dropped. Deterministic on every
//!   target (ruling 4): the rounding is `f64::round` and the printing is the
//!   shortest representation, both exact.
//! - **Text is written as paths**, one `<path>` per glyph, from the outline
//!   the renderer draws. Not as `<text>`: a positioned glyph needs its font
//!   in the SVG to look like the page, SVG 1.1's way of carrying one is
//!   `@font-face` or `<font>`, and `tinker-pdf-svg` — the reader this writer
//!   is held to — refuses the first and reads neither. Paths are exact and
//!   need nothing; the trade, named, is that the text in the file is not
//!   text any more: not selectable and not searchable.
//! - **Fills and strokes** carry colour, opacity, the fill rule, and a
//!   stroke's width, caps, joins, miter limit and dashes. A stroke's width and
//!   dashes are scaled by the transform's expansion into page space, which is
//!   what the renderer does; a zero-width line, 8.4.3.2's "thinnest line", is
//!   written at the renderer's own thinnest, 0.8 of a point.
//! - **A clip is a `<clipPath>`** in page space, referenced by every element
//!   drawn under it. A clip inside a clip names its parent with `clip-path` on
//!   the `<clipPath>` element, which is SVG 1.1 §14.3.5's intersection.
//!   `tinker-pdf-svg` reads one clip per element and not that chain, so the
//!   reader sees the innermost clip of a nested pair; the file is right and
//!   the reader is one clip short, which `docs/features/rendering.md` records.
//! - **An image is an `<image>` with a PNG `data:` URI**, its samples as
//!   decoded — a stencil in the fill colour, a soft-masked image with its
//!   alpha — placed on the unit square by the image's own transform. A
//!   clipped image sits inside a `<g>` that names the clip, never with
//!   `clip-path` on the `<image>`: §14.3.5 reads a `userSpaceOnUse` clip in
//!   the naming element's user space, which includes its own `transform`, so
//!   the page-space clip would be carried through the unit square with the
//!   picture. (`tinker-pdf-svg` carries no clip on an image node at all, and
//!   reads a `<g>`'s clip on nothing under it, so the reader sees a clipped
//!   image unclipped; the file is right and the reader is short, as with
//!   nested clips below.)
//! - **A picture drawn again is not embedded again.** An image's PNG, or a
//!   rasterised paint's, of `REUSE_AT` (512 bytes) or more is written once, as
//!   an `<image id>` on the unit square in `<defs>`, and every draw of the
//!   same bytes is a `<use>` of it carrying that draw's transform and opacity
//!   (SVG 1.1 §5.6). Without it a short content stream drawing one large
//!   image four hundred times is four hundred copies of the image in the
//!   markup. Smaller pictures are written in place, where a reference would
//!   save less than it costs, and so is every picture past the
//!   [`tinker_pdf_svg::Limits::DEFAULT`] number of expansions, so a file this
//!   writes is never one its reader refuses for having too many.
//! - **A number is never `inf` or `NaN`.** A non-finite value is written as
//!   0 and one past 10^11 as a whole number, so a hostile page's coordinates
//!   still make a document a reader parses.
//! - **An axial or radial shading is a gradient where that is exact**: the
//!   colour space is DeviceRGB or DeviceGray, the function is piecewise
//!   linear (type 2 with `N` 1, stitched or arrayed), its values stay inside
//!   0 to 1, both ends extend, and — radial — the first circle is a point
//!   inside the second. Stops sit at the function's breakpoints, a
//!   discontinuity is two stops at one offset. Anything else is **rasterised**
//!   at [`SvgOptions::raster_scale`] through the renderer, clipped as the page
//!   clips it, and embedded as an image — a mesh, a function-based shading, a
//!   CMYK ramp, a tiling pattern — and each is named ([`SvgWarning::Rasterised`]).
//! - **A transparency group is a `<g>`** with the group's constant alpha as
//!   `opacity`, which is 11.4's group compositing for the normal blend mode.
//!
//! # What is refused, by name
//!
//! The writer never emits a `<mask>`, a `<pattern>` or a `<filter>`, the
//! three elements `tinker-pdf-svg` refuses — so a file this writes is one this
//! repository can read back whole. Where a page needs one, the writer says so
//! instead of dropping it silently (ruling 10):
//!
//! - a **soft mask** (SVG 1.1's `<mask>`) is declined, and what it masked is
//!   drawn unmasked — [`SvgWarning::SoftMaskRefused`];
//! - a **blend mode** other than `Normal` needs `<filter>`'s `feBlend`; the
//!   element is drawn with the normal one — [`SvgWarning::BlendModeRefused`];
//! - a **knockout group** has no SVG spelling at all; it is drawn as an
//!   ordinary group — [`SvgWarning::KnockoutRefused`];
//! - a **tiling pattern** would be a `<pattern>`; it is rasterised instead,
//!   which is the fallback and not a refusal, and named as one.
//!
//! What a render of the same page would report — an image codec this build
//! lacks (drawn as the renderer's grey placeholder), a shading or pattern it
//! does not paint, a font with no outline, a damaged image, a hidden layer,
//! a text clip with no glyphs, and anything the renderer said while drawing a
//! rasterised paint — is reported in the renderer's own words, as
//! [`SvgWarning::Render`], so one vocabulary names one fact.
//!
//! # How large the output may be
//!
//! **At most [`MAX_SVG_BYTES`] of elements**, or the smaller
//! [`SvgOptions::max_bytes`] a caller asks for. Markup is the one thing the
//! writer allocates in proportion to what the page *does* rather than to what
//! the file holds — every operator is an element, every image draw a picture,
//! every rasterised paint a page-sized raster — so a short content stream,
//! and a shorter one through a fan of forms, asks for as much as it likes. An
//! element that would take the markup past the budget is not written, and
//! nothing after it is: the writer reports [`SvgWarning::Truncated`] and
//! tells the replay to stop, and the document it hands back is well-formed
//! and ends where the budget did. The root element, `<defs>`' own tags and
//! the `</g>` of each group still open are outside the count, a few hundred
//! bytes in all.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Arc;

use tinker_pdf_color::{ColorSpace, Function};
use tinker_pdf_content::{
    replay, BlendMode, Device, Glyph, GraphicsState, Group, ImageRef, LineCap, LineJoin,
    MarkedProps, MaskGroup, Matrix, PathSegment, TextRenderMode,
};
use tinker_pdf_render::{
    page_pixels, page_scale, page_view_transform, region_canvas_clear, DecodedImage, GlyphSource,
    PatternPaint, PixelRegion, Renderer, Shading,
};

use crate::resources::PageResources;
use crate::{DisplayList, Page, PixelFormat, RenderWarning};

/// How a page is written as SVG. See [`Page::to_svg`].
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct SvgOptions {
    /// Whether annotations' appearances are written after the content, as
    /// [`crate::RenderOptions::annotations`] decides for a render. On by
    /// default.
    pub annotations: bool,
    /// Pixels per point for what is rasterised — a shading no gradient can
    /// state exactly, a tiling pattern. 2 by default, which is 144 dpi;
    /// clamped to `0.25..=16`, and a value that is not a finite positive
    /// number is read as the default.
    pub raster_scale: f64,
    /// The most markup the writer may produce, in bytes of elements: see the
    /// module documentation's *How large the output may be*.
    /// [`MAX_SVG_BYTES`] by default, and never more — a larger value is read
    /// as that cap, so this lowers the ceiling and cannot raise it.
    pub max_bytes: usize,
}

impl Default for SvgOptions {
    fn default() -> Self {
        SvgOptions {
            annotations: true,
            raster_scale: 2.0,
            max_bytes: MAX_SVG_BYTES,
        }
    }
}

/// The most markup one page's SVG may hold, in bytes of elements.
///
/// What a page's markup costs is what the page *does*: every operator an
/// element, every image `Do` a picture, every rasterised paint a raster the
/// size of the region it paints. So a short content stream — and a shorter
/// one through a fan of forms — asks for as much as it likes, which a render
/// of the same page, holding one canvas, never does. Past this the writer
/// stops, says so ([`SvgWarning::Truncated`]) and hands back the well-formed
/// document it has.
///
/// | | Bytes of markup |
/// | --- | --- |
/// | The most any fixture in this repository spends | 266 127 |
/// | A 200-page comic, whose largest page is a 2000 x 3000 scan | 32 100 000 |
/// | A dense 200-page fixed document, a 300 dpi US Letter scan on the page | 46 000 000 |
/// | A 300-page reflowable book, the same plate on a page of text | 46 000 000 |
/// | **This cap** | **256 MiB** |
///
/// The yardsticks are pictures, because a picture is the largest thing one
/// element can be: every image is embedded as an RGBA PNG, so the worst case
/// is its samples incompressible — four bytes a pixel and one a row, in
/// base64's four characters for three. A 2000 x 3000 comic scan is 24 003 000
/// bytes of PNG and 32 004 000 of markup, rounded up for the chunks; a
/// 2 550 x 3 300 full-page scan at 300 dpi is 33 663 300 and 44 884 400, and a
/// fixed page's two thousand elements and forty thousand segments, or a
/// book page's text as glyph outlines, add about a megabyte. The cap clears
/// the largest by 5.8x. The fixtures' figure is the largest page any suite
/// writes at this cap, measured 2 October 2026: `svg_output.rs`'s 4 100 draws
/// of one picture, 4 096 of them references. The test that fires this cap
/// lowers [`SvgOptions::max_bytes`] rather than writing a quarter of a
/// gigabyte, and a unit test holds the lowering to never being a raising.
///
/// Reachable: a content stream as long as `MAX_DECODED_STREAM` allows of
/// `0 0 m 1 1 l S`, fourteen bytes an operator, writes a stroked `<path>` of
/// about seventy bytes for each — 640 MiB, two and a half times this cap,
/// before any form multiplies it.
pub const MAX_SVG_BYTES: usize = 256 << 20;

/// The size from which a picture — an image's PNG, or a rasterised paint's —
/// is written once and referenced after, in bytes of PNG.
///
/// A `<use>` and its transform are about a hundred bytes, and a picture
/// written in place is its PNG in base64 — a third larger — and a hundred and
/// fifty bytes of attributes. From half a kilobyte, a reference costs an
/// eighth of a copy.
const REUSE_AT: usize = 512;

/// How many `<use>` references one page's markup may make: as many as
/// `tinker-pdf-svg` expands, so the reader never refuses a file for them.
const MAX_REFERENCES: usize = tinker_pdf_svg::Limits::DEFAULT.max_uses;

/// The budget a write runs under: the caller's, and never more than the cap.
fn budget(options: &SvgOptions) -> usize {
    options.max_bytes.min(MAX_SVG_BYTES)
}

/// A page as SVG 1.1.
#[derive(Clone, Debug, PartialEq)]
pub struct Svg {
    /// The document, UTF-8.
    pub markup: String,
    /// The page's displayed width in points, which is the root's `width` and
    /// the first `viewBox` extent.
    pub width: f64,
    /// Its height, likewise.
    pub height: f64,
    /// Everything the page asked for that SVG 1.1, as this writer and
    /// `tinker-pdf-svg` understand it, could not say.
    pub warnings: Vec<SvgWarning>,
}

/// Something the page drew that the SVG says differently or not at all.
///
/// Deduplicated: each distinct warning once per page, however many times the
/// page asked.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum SvgWarning {
    /// Drawn as an embedded raster at [`SvgOptions::raster_scale`] rather
    /// than as vectors.
    Rasterised {
        /// What was.
        what: Rasterised,
    },
    /// A soft mask (11.6.5) was declined — SVG's `<mask>` is refused by
    /// `tinker-pdf-svg` — and what it masked is drawn unmasked.
    SoftMaskRefused,
    /// A blend mode other than `Normal` (11.3.5) — `feBlend`, a `<filter>` —
    /// drawn with the normal one.
    BlendModeRefused {
        /// The mode's name as ISO 32000 writes it.
        mode: &'static str,
    },
    /// A knockout transparency group (11.4.5), drawn as an ordinary one.
    KnockoutRefused,
    /// The markup reached its budget — [`SvgOptions::max_bytes`], at most
    /// [`MAX_SVG_BYTES`] — and what the page drew from that point on is not
    /// in it. The document is well-formed and ends there.
    Truncated {
        /// The budget, in bytes of elements.
        limit: usize,
    },
    /// What a render of the page says, in the renderer's own words, about
    /// something the writer met too: a font with no outline, an image this
    /// build cannot decode (a grey placeholder stands in for it, as on a
    /// render), a shading or pattern it does not paint, an image decoded
    /// with damage tolerated, a hidden layer, a text clip with no glyphs —
    /// and whatever the renderer said while drawing a rasterised paint.
    Render(RenderWarning),
}

/// What [`SvgWarning::Rasterised`] rasterised.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Rasterised {
    /// A shading painted by `sh` or as a shading pattern, whose function,
    /// colour space, extension or geometry no SVG gradient states exactly.
    Shading,
    /// A tiling pattern (8.7.3), whose SVG spelling is the `<pattern>` that
    /// `tinker-pdf-svg` refuses.
    TilingPattern,
    /// A stroke painted with a pattern.
    PatternedStroke,
}

impl Page {
    /// The page as SVG 1.1. See the module documentation of `svg_out` for
    /// every decision the output makes, and [`SvgWarning`] for what it says
    /// when it cannot.
    ///
    /// Records the page as [`Page::display_list`] does and writes the
    /// recording; a caller that already holds a list calls
    /// [`DisplayList::to_svg`] and interprets nothing.
    #[must_use]
    pub fn to_svg(&self, options: &SvgOptions) -> Svg {
        self.display_list().to_svg(options)
    }
}

impl DisplayList {
    /// The recorded page as SVG 1.1. See [`Page::to_svg`].
    #[must_use]
    pub fn to_svg(&self, options: &SvgOptions) -> Svg {
        let page = self.page();
        let (width, height) = page.size();
        let base = page_view_transform(page.crop_box(), page.rotation(), 1.0);
        // Resources of this write's own over the list's caches, so what it
        // reports is what it met — `DisplayList::render`'s reason exactly.
        let resources = self.resources().for_one_render();
        resources.note_missing_fonts(self.interpreted_missing());
        let mut writer = Writer::new(&resources, base, page, options);
        replay(self.content(), &mut writer);
        if options.annotations {
            for (scope, events) in self.annotation_layers() {
                writer.push_resources(Arc::clone(scope));
                replay(events, &mut writer);
                writer.pop_resources();
            }
        }
        writer.finish_with(&resources, width, height)
    }
}

/// The resource scope names resolve in: the page's, a form's, an
/// annotation's. The renderer's `Scope`, for the same reason.
enum Scope<'a> {
    Borrowed(&'a PageResources),
    Owned(Arc<PageResources>),
}

impl core::ops::Deref for Scope<'_> {
    type Target = PageResources;

    fn deref(&self) -> &PageResources {
        match self {
            Scope::Borrowed(resources) => resources,
            Scope::Owned(resources) => resources,
        }
    }
}

/// One clip in force: its `<clipPath>`, and the path again in page space so
/// a rasterised paint can be clipped as the page clips it.
#[derive(Clone)]
struct ClipEntry {
    id: u32,
    path: Vec<PathSegment>,
    even_odd: bool,
    /// The clip's bounding box in SVG units, `[x0, y0, x1, y1]`.
    bounds: [f64; 4],
}

/// The page's geometry, for a rasterised paint to be drawn in.
struct Geometry {
    crop: (f64, f64, f64, f64),
    rotation: u16,
    width: f64,
    height: f64,
}

/// The device.
struct Writer<'a> {
    glyphs: Scope<'a>,
    form_scopes: Vec<Option<Scope<'a>>>,
    pushed: Vec<Scope<'a>>,
    /// PDF default space to SVG units: displayed points, `y` down.
    base: Matrix,
    geometry: Geometry,
    raster_scale: f64,
    defs: String,
    body: String,
    next_id: u32,
    clips: Vec<ClipEntry>,
    saved: Vec<Vec<ClipEntry>>,
    text_clip: Vec<PathSegment>,
    text_clip_requested: bool,
    marked: Vec<bool>,
    hidden: u32,
    groups: usize,
    warnings: Vec<SvgWarning>,
    /// The most bytes `defs` and `body` may hold together.
    limit: usize,
    /// Whether an element was refused for the budget, after which nothing
    /// more is written and the replay is told to stop.
    spent: bool,
    /// Every picture written once into `defs`, by its PNG and its
    /// `image-rendering`, and the id it was written under.
    pictures: HashMap<(Vec<u8>, Option<&'static str>), u32>,
    /// `<use>` references written, against [`MAX_REFERENCES`].
    references: usize,
}

impl<'a> Writer<'a> {
    fn new(
        resources: &'a PageResources,
        base: Matrix,
        page: &Page,
        options: &SvgOptions,
    ) -> Writer<'a> {
        let (width, height) = page.size();
        let raster_scale = if options.raster_scale.is_finite() && options.raster_scale > 0.0 {
            options.raster_scale.clamp(0.25, 16.0)
        } else {
            SvgOptions::default().raster_scale
        };
        Writer {
            glyphs: Scope::Borrowed(resources),
            form_scopes: Vec::new(),
            pushed: Vec::new(),
            base,
            geometry: Geometry {
                crop: page.crop_box(),
                rotation: page.rotation(),
                width,
                height,
            },
            raster_scale,
            defs: String::new(),
            body: String::new(),
            next_id: 0,
            clips: Vec::new(),
            saved: Vec::new(),
            text_clip: Vec::new(),
            text_clip_requested: false,
            marked: Vec::new(),
            hidden: 0,
            groups: 0,
            warnings: Vec::new(),
            limit: budget(options),
            spent: false,
            pictures: HashMap::new(),
            references: 0,
        }
    }

    /// Whether `more` bytes of elements fit in the budget. The first time
    /// they do not, the writer is spent: it says so once, writes nothing
    /// more, and answers the replay's `is_cancelled` with yes.
    fn room(&mut self, more: usize) -> bool {
        if self.spent {
            return false;
        }
        let total = self
            .defs
            .len()
            .saturating_add(self.body.len())
            .saturating_add(more);
        if total > self.limit {
            self.spent = true;
            let limit = self.limit;
            self.warn(SvgWarning::Truncated { limit });
            return false;
        }
        true
    }

    /// Writes an element into the body, if the budget has room for it.
    fn put(&mut self, element: &str) -> bool {
        if !self.room(element.len()) {
            return false;
        }
        self.body.push_str(element);
        true
    }

    /// Writes a definition into `<defs>`, if the budget has room for it.
    fn define(&mut self, element: &str) -> bool {
        if !self.room(element.len()) {
            return false;
        }
        self.defs.push_str(element);
        true
    }

    /// A picture — PNG bytes on the unit square — placed by `to_page`, as the
    /// element that draws it: an `<image>` in place, or a `<use>` of the one
    /// written into `<defs>` the first time these bytes were drawn (see the
    /// module documentation for which). `extra` is the draw's own
    /// attributes. `None` when the budget had no room for the definition.
    fn picture(
        &mut self,
        png: &[u8],
        rendering: Option<&'static str>,
        to_page: &Matrix,
        extra: &str,
    ) -> Option<String> {
        let rendering_attr = rendering
            .map(|r| format!(" image-rendering=\"{r}\""))
            .unwrap_or_default();
        let placed = matrix_attr(to_page);
        if png.len() < REUSE_AT || self.references >= MAX_REFERENCES {
            // Checked before the base64 is made, which is a third larger
            // than what it encodes and pointless to build past the budget.
            if !self.room(png.len().saturating_mul(4) / 3) {
                return None;
            }
            return Some(format!(
                "<image x=\"0\" y=\"0\" width=\"1\" height=\"1\" preserveAspectRatio=\"none\"\
                 {rendering_attr} transform=\"{placed}\"{extra} \
                 xlink:href=\"data:image/png;base64,{}\"/>",
                base64(png)
            ));
        }
        let key = (png.to_vec(), rendering);
        let id = match self.pictures.get(&key) {
            Some(&id) => id,
            None => {
                if !self.room(png.len().saturating_mul(4) / 3) {
                    return None;
                }
                let id = self.id();
                let definition = format!(
                    "<image id=\"p{id}\" x=\"0\" y=\"0\" width=\"1\" height=\"1\" \
                     preserveAspectRatio=\"none\"{rendering_attr} \
                     xlink:href=\"data:image/png;base64,{}\"/>\n",
                    base64(png)
                );
                if !self.define(&definition) {
                    return None;
                }
                self.pictures.insert(key, id);
                id
            }
        };
        self.references += 1;
        Some(format!(
            "<use xlink:href=\"#p{id}\" transform=\"{placed}\"{extra}/>"
        ))
    }

    fn push_resources(&mut self, resources: Arc<PageResources>) {
        let outer = std::mem::replace(&mut self.glyphs, Scope::Owned(resources));
        self.pushed.push(outer);
    }

    fn pop_resources(&mut self) {
        if let Some(outer) = self.pushed.pop() {
            self.glyphs = outer;
        }
    }

    /// [`Writer::finish`], after what `Page::render` adds once its renderer
    /// finishes, for the same two reasons: a glyph a font could not name is a
    /// `.notdef` outline and never reaches the device's own count, and a
    /// damaged image is drawn and named (ruling 10).
    fn finish_with(mut self, resources: &PageResources, width: f64, height: f64) -> Svg {
        if !resources.missing_fonts().is_empty() {
            self.warn(SvgWarning::Render(RenderWarning::UnreadableFont));
        }
        for (name, reason) in resources.damaged_images() {
            self.warn(SvgWarning::Render(RenderWarning::DamagedImage {
                name,
                reason,
            }));
        }
        self.finish(width, height)
    }

    fn finish(mut self, width: f64, height: f64) -> Svg {
        // A group left open by a stream the interpreter could not balance is
        // closed, so the document is well-formed whatever the page did.
        for _ in 0..self.groups {
            self.body.push_str("</g>\n");
        }
        let (w, h) = (num(width), num(height));
        let mut markup = String::with_capacity(self.defs.len() + self.body.len() + 256);
        markup.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        let _ = writeln!(
            markup,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" \
             version=\"1.1\" width=\"{w}pt\" height=\"{h}pt\" viewBox=\"0 0 {w} {h}\">"
        );
        if !self.defs.is_empty() {
            markup.push_str("<defs>\n");
            markup.push_str(&self.defs);
            markup.push_str("</defs>\n");
        }
        markup.push_str(&self.body);
        markup.push_str("</svg>\n");
        Svg {
            markup,
            width,
            height,
            warnings: self.warnings,
        }
    }

    fn warn(&mut self, warning: SvgWarning) {
        if !self.warnings.contains(&warning) {
            self.warnings.push(warning);
        }
    }

    fn id(&mut self) -> u32 {
        self.next_id = self.next_id.saturating_add(1);
        self.next_id
    }

    /// The `clip-path` attribute for what is in force.
    ///
    /// With the clip's rule beside it when that is even-odd. SVG 1.1 reads
    /// `clip-rule` on the `<clipPath>`'s own children, which is where
    /// [`Writer::add_clip`] writes it; `tinker-pdf-svg` reads it on the
    /// element that refers to the clip. It is written in both places, which
    /// is harmless to the first reading and necessary for the second.
    fn clip_attr(&self) -> String {
        match self.clips.last() {
            Some(clip) if clip.even_odd => {
                format!(" clip-path=\"url(#c{})\" clip-rule=\"evenodd\"", clip.id)
            }
            Some(clip) => format!(" clip-path=\"url(#c{})\"", clip.id),
            None => String::new(),
        }
    }

    /// The clip's bounding box in SVG units, or the page.
    fn clip_bounds(&self) -> [f64; 4] {
        let page = [0.0, 0.0, self.geometry.width, self.geometry.height];
        self.clips.iter().fold(page, |acc, clip| {
            [
                acc[0].max(clip.bounds[0]),
                acc[1].max(clip.bounds[1]),
                acc[2].min(clip.bounds[2]),
                acc[3].min(clip.bounds[3]),
            ]
        })
    }

    fn note_blend(&mut self, mode: BlendMode) {
        if mode != BlendMode::Normal {
            self.warn(SvgWarning::BlendModeRefused {
                mode: blend_name(mode),
            });
        }
    }

    /// A filled area: a solid colour, or a pattern.
    fn fill(&mut self, path: &[PathSegment], even_odd: bool, state: &GraphicsState) {
        if self.spent {
            return;
        }
        self.note_blend(state.blend);
        if let Some(name) = &state.fill_pattern {
            let name = name.clone();
            self.pattern_fill(path, even_odd, &name, state);
            return;
        }
        let d = path_data(path, &self.base);
        if d.is_empty() {
            return;
        }
        let mut element = format!("<path d=\"{d}\" fill=\"{}\"", colour(state.fill_color));
        if even_odd {
            element.push_str(" fill-rule=\"evenodd\"");
        }
        push_opacity(&mut element, "fill-opacity", state.fill_alpha);
        element.push_str(&self.clip_attr());
        element.push_str("/>\n");
        self.put(&element);
    }

    /// A stroked outline.
    fn stroke(&mut self, path: &[PathSegment], state: &GraphicsState, text: bool) {
        if self.spent {
            return;
        }
        self.note_blend(state.blend);
        let scale = state.ctm.then(&self.base).expansion();
        if state.stroke_pattern.is_some() {
            let bounds = grow(path_bounds(path, &self.base), state.line_width * scale);
            let path = path.to_vec();
            let state = state.clone();
            self.rasterise(bounds, Rasterised::PatternedStroke, move |renderer| {
                renderer.stroke_path(&path, &state);
            });
            return;
        }
        let d = path_data(path, &self.base);
        if d.is_empty() {
            return;
        }
        let width = if state.line_width * scale > 0.0 {
            state.line_width * scale
        } else {
            THINNEST
        };
        let mut element = format!(
            "<path d=\"{d}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{}\"",
            colour(state.stroke_color),
            num(width)
        );
        // Text is stroked with the renderer's defaults, not the state's
        // (9.3.6 strokes a glyph as a path, and the renderer strokes it with
        // butt caps and miter joins whatever `J` and `j` say).
        if !text {
            match state.line_cap {
                LineCap::Butt => {}
                LineCap::Round => element.push_str(" stroke-linecap=\"round\""),
                LineCap::Square => element.push_str(" stroke-linecap=\"square\""),
            }
            match state.line_join {
                LineJoin::Miter => {}
                LineJoin::Round => element.push_str(" stroke-linejoin=\"round\""),
                LineJoin::Bevel => element.push_str(" stroke-linejoin=\"bevel\""),
            }
        }
        let miter = if text { 10.0 } else { state.miter_limit };
        if miter.is_finite() && miter >= 1.0 {
            let _ = write!(element, " stroke-miterlimit=\"{}\"", num(miter));
        }
        let dashes: Vec<f64> = if text {
            Vec::new()
        } else {
            state.dashes.iter().map(|d| d * scale).collect()
        };
        if dashes.iter().all(|d| d.is_finite() && *d >= 0.0) && dashes.iter().any(|d| *d > 0.0) {
            let list: Vec<String> = dashes.iter().map(|d| num(*d)).collect();
            let _ = write!(element, " stroke-dasharray=\"{}\"", list.join(","));
            let phase = state.dash_phase * scale;
            if phase != 0.0 && phase.is_finite() {
                let _ = write!(element, " stroke-dashoffset=\"{}\"", num(phase));
            }
        }
        push_opacity(&mut element, "stroke-opacity", state.stroke_alpha);
        element.push_str(&self.clip_attr());
        element.push_str("/>\n");
        self.put(&element);
    }

    /// A fill with `/Pattern`: a gradient where one is exact, a raster
    /// otherwise.
    fn pattern_fill(
        &mut self,
        path: &[PathSegment],
        even_odd: bool,
        name: &[u8],
        state: &GraphicsState,
    ) {
        let d = path_data(path, &self.base);
        if d.is_empty() {
            return;
        }
        match self.glyphs.pattern(name) {
            Some(PatternPaint::Shading(shading, matrix)) => {
                let to_page = matrix.then(&self.base);
                if let Some(gradient) = self.gradient(&shading, &to_page) {
                    let mut element = format!("<path d=\"{d}\" fill=\"url(#g{gradient})\"");
                    if even_odd {
                        element.push_str(" fill-rule=\"evenodd\"");
                    }
                    push_opacity(&mut element, "fill-opacity", state.fill_alpha);
                    element.push_str(&self.clip_attr());
                    element.push_str("/>\n");
                    self.put(&element);
                    return;
                }
                self.rasterise_fill(path, even_odd, state, Rasterised::Shading);
            }
            Some(PatternPaint::Tiling(_)) => {
                self.rasterise_fill(path, even_odd, state, Rasterised::TilingPattern);
            }
            Some(PatternPaint::Unsupported) | None => {
                self.warn(SvgWarning::Render(RenderWarning::UnsupportedPattern {
                    name: String::from_utf8_lossy(name).into_owned(),
                }));
            }
        }
    }

    fn rasterise_fill(
        &mut self,
        path: &[PathSegment],
        even_odd: bool,
        state: &GraphicsState,
        what: Rasterised,
    ) {
        let bounds = path_bounds(path, &self.base);
        let path = path.to_vec();
        let state = state.clone();
        self.rasterise(bounds, what, move |renderer| {
            renderer.fill_path(&path, &state, even_odd);
        });
    }

    /// Registers a gradient for `shading` seen through `to_page`, or `None`
    /// where no gradient is exact.
    fn gradient(&mut self, shading: &Shading, to_page: &Matrix) -> Option<u32> {
        if !to_page.is_finite() {
            return None;
        }
        let (element, stops) = match shading {
            Shading::Axial {
                space,
                function,
                coords,
                extend,
            } => {
                if *extend != (true, true) || !coords.iter().all(|v| v.is_finite()) {
                    return None;
                }
                let [x0, y0, x1, y1] = *coords;
                if x0 == x1 && y0 == y1 {
                    return None;
                }
                let stops = gradient_stops(space, function)?;
                (
                    format!(
                        "<linearGradient id=\"g{{id}}\" gradientUnits=\"userSpaceOnUse\" \
                         x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" gradientTransform=\"{}\">",
                        num(x0),
                        num(y0),
                        num(x1),
                        num(y1),
                        matrix_attr(to_page)
                    ),
                    stops,
                )
            }
            Shading::Radial {
                space,
                function,
                coords,
                extend,
            } => {
                let [fx, fy, r0, cx, cy, r1] = *coords;
                if !coords.iter().all(|v| v.is_finite()) || !extend.1 || r0 != 0.0 || r1 <= 0.0 {
                    return None;
                }
                // SVG 1.1 §13.2.3 moves a focal point outside the circle onto
                // it, which is a different picture from 8.7.4.5.4's cone; only
                // a focus strictly inside is the same one.
                if (fx - cx) * (fx - cx) + (fy - cy) * (fy - cy) >= r1 * r1 {
                    return None;
                }
                let stops = gradient_stops(space, function)?;
                (
                    format!(
                        "<radialGradient id=\"g{{id}}\" gradientUnits=\"userSpaceOnUse\" \
                         cx=\"{}\" cy=\"{}\" r=\"{}\" fx=\"{}\" fy=\"{}\" gradientTransform=\"{}\">",
                        num(cx),
                        num(cy),
                        num(r1),
                        num(fx),
                        num(fy),
                        matrix_attr(to_page)
                    ),
                    stops,
                )
            }
            _ => return None,
        };
        let id = self.id();
        let closing = if element.starts_with("<linear") {
            "</linearGradient>\n"
        } else {
            "</radialGradient>\n"
        };
        let mut definition = element.replace("{id}", &id.to_string());
        definition.push('\n');
        for (offset, rgb) in stops {
            let _ = writeln!(
                definition,
                "<stop offset=\"{}\" stop-color=\"{}\"/>",
                num(offset),
                colour_bytes(rgb)
            );
        }
        definition.push_str(closing);
        // A gradient the budget has no room for is no gradient; the caller's
        // fallback is a raster, which the spent budget refuses in turn.
        self.define(&definition).then_some(id)
    }

    /// Draws `paint` through the renderer over `bounds` (SVG units), clipped
    /// as the page is clipped, and embeds the pixels.
    fn rasterise(
        &mut self,
        bounds: [f64; 4],
        what: Rasterised,
        paint: impl FnOnce(&mut Renderer<'_, PageResources>),
    ) {
        if self.spent {
            return;
        }
        let clip = self.clip_bounds();
        let bounds = [
            bounds[0].max(clip[0]),
            bounds[1].max(clip[1]),
            bounds[2].min(clip[2]),
            bounds[3].min(clip[3]),
        ];
        let Geometry {
            crop,
            rotation,
            width,
            height,
        } = self.geometry;
        let scale = page_scale(width, height, self.raster_scale);
        let (full_w, full_h) = page_pixels(width, height, scale);
        let pixel = |v: f64, limit: u32| (v * scale).clamp(0.0, f64::from(limit));
        let (x0, y0) = (
            pixel(bounds[0], full_w).floor(),
            pixel(bounds[1], full_h).floor(),
        );
        let (x1, y1) = (
            pixel(bounds[2], full_w).ceil(),
            pixel(bounds[3], full_h).ceil(),
        );
        if !(x1 > x0 && y1 > y0) {
            return;
        }
        // Both corners are inside the page's pixels, which `page_pixels`
        // bounds far below `u32::MAX`.
        let region = PixelRegion::new(x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32);
        let canvas = region_canvas_clear(region, PixelFormat::Rgba8);
        let base = page_view_transform(crop, rotation, scale);
        let mut renderer =
            Renderer::new(canvas, base, &*self.glyphs).with_page_size(full_w, full_h);
        let unclipped = GraphicsState::default();
        for clip in &self.clips {
            renderer.clip_path(&clip.path, &unclipped, clip.even_odd);
        }
        paint(&mut renderer);
        let (canvas, warnings) = renderer.finish();
        for warning in warnings {
            self.warn(SvgWarning::Render(warning));
        }
        let Some(png) = png(canvas.width, canvas.height, canvas.stride, &canvas.data) else {
            return;
        };
        // The pixels' rectangle in SVG units, as the unit square's transform,
        // so a raster drawn again — the same `sh` four hundred times — is a
        // picture written once.
        let to_page = Matrix {
            a: (x1 - x0) / scale,
            b: 0.0,
            c: 0.0,
            d: (y1 - y0) / scale,
            e: x0 / scale,
            f: y0 / scale,
        };
        let Some(mut element) = self.picture(&png, None, &to_page, "") else {
            return;
        };
        element.push('\n');
        if self.put(&element) {
            self.warn(SvgWarning::Rasterised { what });
        }
    }

    /// A decoded image on the unit square of `state.ctm`.
    fn image(&mut self, image: &DecodedImage, state: &GraphicsState) {
        if self.spent {
            return;
        }
        let (w, h) = (image.width, image.height);
        let pixels = (w as usize).saturating_mul(h as usize);
        if pixels == 0 || image.rgb.len() < pixels.saturating_mul(3) {
            return;
        }
        let mut rgba = Vec::with_capacity(pixels.saturating_mul(4));
        let tint = state.fill_color;
        for index in 0..pixels {
            let alpha = if image.alpha.is_empty() {
                255
            } else {
                image.alpha.get(index).copied().unwrap_or(255)
            };
            if image.stencil {
                rgba.extend_from_slice(&[tint.r, tint.g, tint.b, alpha]);
            } else {
                let at = index * 3;
                rgba.extend_from_slice(image.rgb.get(at..at + 3).unwrap_or(&[0, 0, 0]));
                rgba.push(alpha);
            }
        }
        let Some(png) = png(w, h, w as usize * 4, &rgba) else {
            return;
        };
        // SVG's image runs its first row at the top of the rectangle; PDF's
        // unit square runs it at `y = 1` (8.9.5.2). The flip maps one onto the
        // other before the image's own transform.
        let flip = Matrix {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: -1.0,
            e: 0.0,
            f: 1.0,
        };
        let to_page = flip.then(&state.ctm).then(&self.base);
        if !to_page.is_finite() {
            return;
        }
        let rendering = if image.interpolate {
            "optimizeQuality"
        } else {
            "optimizeSpeed"
        };
        let mut opacity = String::new();
        push_opacity(&mut opacity, "opacity", state.fill_alpha);
        let Some(element) = self.picture(&png, Some(rendering), &to_page, &opacity) else {
            return;
        };
        // Not `clip-path` on the `<image>` (or the `<use>`) itself: SVG 1.1
        // §14.3.5 reads a `userSpaceOnUse` clip in the user space of the
        // element that names it, and an element's own `transform` is part of
        // that space — so the page-space clip would be carried through the
        // unit square's transform with the picture. A `<g>` with no transform
        // names it in page space instead.
        let clip = self.clip_attr();
        if clip.is_empty() {
            self.put(&format!("{element}\n"));
        } else {
            self.put(&format!("<g{clip}>{element}</g>\n"));
        }
    }

    /// The grey rectangle the renderer draws for an image it could not read.
    fn placeholder(&mut self, state: &GraphicsState) {
        let square = [
            PathSegment::MoveTo { x: 0.0, y: 0.0 },
            PathSegment::LineTo { x: 1.0, y: 0.0 },
            PathSegment::LineTo { x: 1.0, y: 1.0 },
            PathSegment::LineTo { x: 0.0, y: 1.0 },
            PathSegment::Close,
        ];
        let to_page = state.ctm.then(&self.base);
        let d = path_data(&square, &to_page);
        if d.is_empty() {
            return;
        }
        let mut element = format!("<path d=\"{d}\" fill=\"#bfbfbf\"");
        push_opacity(&mut element, "fill-opacity", state.fill_alpha);
        element.push_str(&self.clip_attr());
        element.push_str("/>\n");
        self.put(&element);
    }
}

/// 8.4.3.2's thinnest line, at the renderer's own thinnest at scale 1.
const THINNEST: f64 = 0.8;

impl Device for Writer<'_> {
    fn show_glyph(&mut self, glyph: &Glyph, state: &GraphicsState) {
        if self.hidden > 0 {
            return;
        }
        let mode = state.text.render_mode;
        if matches!(mode, TextRenderMode::Invisible) || (!mode.paints() && !mode.clips()) {
            return;
        }
        if mode.clips() {
            self.text_clip_requested = true;
        }
        let Some(outline) = self.glyphs.outline(glyph.font_id, glyph.code) else {
            if !glyph.text.trim().is_empty() {
                self.warn(SvgWarning::Render(RenderWarning::UnreadableFont));
            }
            return;
        };
        if outline.is_empty() || !glyph.transform.is_finite() {
            return;
        }
        let path = glyph_path(&outline, &glyph.transform);
        if mode.fills() {
            self.fill(&path, false, state);
        }
        if mode.strokes() {
            self.stroke(&path, state, true);
        }
        if mode.clips() {
            self.text_clip.extend(path);
        }
    }

    fn begin_text(&mut self) {
        self.text_clip.clear();
        self.text_clip_requested = false;
    }

    fn end_text(&mut self) {
        if !std::mem::take(&mut self.text_clip_requested) {
            return;
        }
        let path = std::mem::take(&mut self.text_clip);
        // 9.3.6: no glyph, so the clip is empty and clips everything away —
        // spec-correct, and named as the renderer names it.
        if path.is_empty() {
            self.warn(SvgWarning::Render(RenderWarning::EmptyTextClip));
        }
        self.add_clip(path, false);
    }

    fn fill_path(&mut self, path: &[PathSegment], state: &GraphicsState, even_odd: bool) {
        if self.hidden == 0 {
            self.fill(path, even_odd, state);
        }
    }

    fn stroke_path(&mut self, path: &[PathSegment], state: &GraphicsState) {
        if self.hidden == 0 {
            self.stroke(path, state, false);
        }
    }

    fn clip_path(&mut self, path: &[PathSegment], _state: &GraphicsState, even_odd: bool) {
        // As the renderer answers: `W n` with no path at all installs no
        // clip. A path of moves alone is a clip of no area, which clips
        // everything, and so is a text object that clips and shows no glyph
        // (`end_text`) — both written as a `<clipPath>` with nothing in it.
        if path.is_empty() {
            return;
        }
        self.add_clip(path.to_vec(), even_odd);
    }

    fn save_state(&mut self) {
        self.saved.push(self.clips.clone());
    }

    fn restore_state(&mut self) {
        if let Some(clips) = self.saved.pop() {
            self.clips = clips;
        }
    }

    fn draw_image(&mut self, image: &ImageRef, state: &GraphicsState) {
        if self.hidden > 0 {
            return;
        }
        self.note_blend(state.blend);
        // The renderer's answer and its words: `Ok(None)` is a name that is
        // not an image, reported under the name, and `Err` the codec.
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
            Ok(decoded) => self.image(&decoded, state),
            Err(codec) => {
                self.warn(SvgWarning::Render(RenderWarning::UnsupportedImage {
                    codec,
                }));
                self.placeholder(state);
            }
        }
    }

    fn draw_shading(&mut self, name: &[u8], state: &GraphicsState) {
        if self.hidden > 0 {
            return;
        }
        self.note_blend(state.blend);
        let shading = match self.glyphs.shading(name) {
            Ok(Some(shading)) => shading,
            Ok(None) => return,
            Err(kind) => {
                self.warn(SvgWarning::Render(RenderWarning::UnsupportedShading {
                    kind,
                }));
                return;
            }
        };
        let to_page = state.ctm.then(&self.base);
        if let Some(gradient) = self.gradient(&shading, &to_page) {
            // 8.7.4.2: `sh` paints the whole clip, and the page where there
            // is none.
            let [x0, y0, x1, y1] = [0.0, 0.0, self.geometry.width, self.geometry.height];
            let mut element = format!(
                "<path d=\"M{} {}H{}V{}H{}Z\" fill=\"url(#g{gradient})\"",
                num(x0),
                num(y0),
                num(x1),
                num(y1),
                num(x0)
            );
            push_opacity(&mut element, "fill-opacity", state.fill_alpha);
            element.push_str(&self.clip_attr());
            element.push_str("/>\n");
            self.put(&element);
            return;
        }
        let page = [0.0, 0.0, self.geometry.width, self.geometry.height];
        let name = name.to_vec();
        let state = state.clone();
        self.rasterise(page, Rasterised::Shading, move |renderer| {
            renderer.draw_shading(&name, &state);
        });
    }

    fn begin_marked_content(
        &mut self,
        _tag: &[u8],
        visible: bool,
        hidden_layer: Option<&str>,
        _props: Option<&MarkedProps>,
    ) {
        if let Some(layer) = hidden_layer {
            self.warn(SvgWarning::Render(RenderWarning::HiddenOptionalContent {
                layer: layer.to_string(),
            }));
        }
        self.marked.push(!visible);
        if !visible {
            self.hidden = self.hidden.saturating_add(1);
        }
    }

    fn end_marked_content(&mut self) {
        if self.marked.pop() == Some(true) {
            self.hidden = self.hidden.saturating_sub(1);
        }
    }

    fn begin_form(&mut self, _id: u64, name: &[u8]) -> bool {
        self.saved.push(self.clips.clone());
        let nested = self.glyphs.form_scope(name);
        self.form_scopes
            .push(nested.map(|scope| std::mem::replace(&mut self.glyphs, Scope::Owned(scope))));
        true
    }

    fn end_form(&mut self, _id: u64) {
        if let Some(Some(outer)) = self.form_scopes.pop() {
            self.glyphs = outer;
        }
        if let Some(clips) = self.saved.pop() {
            self.clips = clips;
        }
    }

    fn begin_group(&mut self, group: Group, state: &GraphicsState) -> bool {
        // As the renderer answers: a group inside hidden content paints
        // nothing, so there is nothing to wrap.
        if self.hidden > 0 {
            return false;
        }
        self.note_blend(state.blend);
        if group.knockout {
            self.warn(SvgWarning::KnockoutRefused);
        }
        let mut element = String::from("<g");
        push_opacity(&mut element, "opacity", state.fill_alpha);
        element.push_str(">\n");
        // Declined when the budget has no room, so no `end_group` arrives for
        // a `<g>` that was never opened. The closing tags are outside the
        // count for that reason: each answers an opening tag that was in it.
        if !self.put(&element) {
            return false;
        }
        self.groups = self.groups.saturating_add(1);
        true
    }

    fn end_group(&mut self) {
        if self.groups > 0 {
            self.groups -= 1;
            self.body.push_str("</g>\n");
        }
    }

    fn begin_soft_mask(
        &mut self,
        _mask: &MaskGroup,
        _bbox: &[PathSegment],
        _state: &GraphicsState,
    ) -> bool {
        self.warn(SvgWarning::SoftMaskRefused);
        false
    }

    /// Yes once the budget is spent: nothing more would be written, so
    /// nothing more is worth replaying or interpreting.
    fn is_cancelled(&self) -> bool {
        self.spent
    }
}

impl Writer<'_> {
    /// Adds a clip — a path, or a text object's glyphs — to what is in force.
    fn add_clip(&mut self, path: Vec<PathSegment>, even_odd: bool) {
        if self.spent {
            return;
        }
        let id = self.id();
        let d = path_data(&path, &self.base);
        let bounds = path_bounds(&path, &self.base);
        let parent = self
            .clips
            .last()
            .map(|clip| format!(" clip-path=\"url(#c{})\"", clip.id))
            .unwrap_or_default();
        let rule = if even_odd {
            " clip-rule=\"evenodd\""
        } else {
            ""
        };
        // Pushed whether or not the budget had room for it: a refusal spends
        // the budget, so nothing is written after it that could name the
        // missing `<clipPath>`, and the stack stays balanced against `Q`.
        self.define(&format!(
            "<clipPath id=\"c{id}\" clipPathUnits=\"userSpaceOnUse\"{parent}><path d=\"{d}\"{rule}/></clipPath>\n"
        ));
        self.clips.push(ClipEntry {
            id,
            path,
            even_odd,
            bounds,
        });
    }
}

/// A number as SVG writes it: rounded to four decimal places, trailing
/// zeros dropped, never `-0`, and `0` for anything not finite.
fn num(value: f64) -> String {
    if !value.is_finite() {
        return "0".to_string();
    }
    // Past 10^11 the fourth place is below an `f64`'s own precision, and the
    // product would overflow to infinity before it got there — which prints
    // as `inf`, a number no SVG reader parses. A whole number is exact there.
    let rounded = if value.abs() < 1e11 {
        (value * 10_000.0).round() / 10_000.0
    } else {
        value.round()
    };
    if rounded == 0.0 {
        return "0".to_string();
    }
    format!("{rounded}")
}

fn matrix_attr(m: &Matrix) -> String {
    format!(
        "matrix({} {} {} {} {} {})",
        num(m.a),
        num(m.b),
        num(m.c),
        num(m.d),
        num(m.e),
        num(m.f)
    )
}

fn colour(rgb: tinker_pdf_content::Rgb) -> String {
    colour_bytes((rgb.r, rgb.g, rgb.b))
}

fn colour_bytes((r, g, b): (u8, u8, u8)) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn push_opacity(element: &mut String, attribute: &str, alpha: f64) {
    let alpha = if alpha.is_finite() {
        alpha.clamp(0.0, 1.0)
    } else {
        1.0
    };
    if alpha < 1.0 {
        let _ = write!(element, " {attribute}=\"{}\"", num(alpha));
    }
}

/// Path data for `path` through `m`, or empty when it draws nothing.
fn path_data(path: &[PathSegment], m: &Matrix) -> String {
    let mut out = String::new();
    let mut drew = false;
    for segment in path {
        match *segment {
            PathSegment::MoveTo { x, y } => {
                let (x, y) = m.apply(x, y);
                let _ = write!(out, "M{} {}", num(x), num(y));
            }
            PathSegment::LineTo { x, y } => {
                let (x, y) = m.apply(x, y);
                let _ = write!(out, "L{} {}", num(x), num(y));
                drew = true;
            }
            PathSegment::CurveTo {
                x1,
                y1,
                x2,
                y2,
                x3,
                y3,
            } => {
                let (x1, y1) = m.apply(x1, y1);
                let (x2, y2) = m.apply(x2, y2);
                let (x3, y3) = m.apply(x3, y3);
                let _ = write!(
                    out,
                    "C{} {} {} {} {} {}",
                    num(x1),
                    num(y1),
                    num(x2),
                    num(y2),
                    num(x3),
                    num(y3)
                );
                drew = true;
            }
            PathSegment::Close => out.push('Z'),
        }
    }
    if drew {
        out
    } else {
        String::new()
    }
}

/// The bounding box of `path` through `m`, as `[x0, y0, x1, y1]`; empty (the
/// corners crossed) for a path with no finite point.
fn path_bounds(path: &[PathSegment], m: &Matrix) -> [f64; 4] {
    let mut bounds = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    let mut take = |x: f64, y: f64| {
        let (x, y) = m.apply(x, y);
        if x.is_finite() && y.is_finite() {
            bounds = [
                bounds[0].min(x),
                bounds[1].min(y),
                bounds[2].max(x),
                bounds[3].max(y),
            ];
        }
    };
    for segment in path {
        match *segment {
            PathSegment::MoveTo { x, y } | PathSegment::LineTo { x, y } => take(x, y),
            PathSegment::CurveTo {
                x1,
                y1,
                x2,
                y2,
                x3,
                y3,
            } => {
                // A Bézier stays inside its control polygon.
                take(x1, y1);
                take(x2, y2);
                take(x3, y3);
            }
            PathSegment::Close => {}
        }
    }
    bounds
}

/// A box widened by half a stroke's width, and a point more for its caps and
/// joins' reach and for rounding.
fn grow(bounds: [f64; 4], width: f64) -> [f64; 4] {
    let by = if width.is_finite() {
        width.abs() * 5.0 + 1.0
    } else {
        1.0
    };
    [
        bounds[0] - by,
        bounds[1] - by,
        bounds[2] + by,
        bounds[3] + by,
    ]
}

/// A glyph's outline as a path in PDF default space, its quadratics raised to
/// cubics exactly.
fn glyph_path(outline: &tinker_pdf_font::Outline, transform: &Matrix) -> Vec<PathSegment> {
    use tinker_pdf_font::Segment;
    let mut out = Vec::with_capacity(outline.segments.len());
    let (mut pen, mut start) = ((0.0, 0.0), (0.0, 0.0));
    for segment in &outline.segments {
        match *segment {
            Segment::MoveTo { x, y } => {
                pen = (x, y);
                start = pen;
                let (x, y) = transform.apply(x, y);
                out.push(PathSegment::MoveTo { x, y });
            }
            Segment::LineTo { x, y } => {
                pen = (x, y);
                let (x, y) = transform.apply(x, y);
                out.push(PathSegment::LineTo { x, y });
            }
            Segment::QuadTo { cx, cy, x, y } => {
                // The one cubic a quadratic is: each control two thirds of the
                // way from an end to the quadratic's control.
                let (c1x, c1y) = (
                    pen.0 + 2.0 / 3.0 * (cx - pen.0),
                    pen.1 + 2.0 / 3.0 * (cy - pen.1),
                );
                let (c2x, c2y) = (x + 2.0 / 3.0 * (cx - x), y + 2.0 / 3.0 * (cy - y));
                pen = (x, y);
                let (x1, y1) = transform.apply(c1x, c1y);
                let (x2, y2) = transform.apply(c2x, c2y);
                let (x3, y3) = transform.apply(x, y);
                out.push(PathSegment::CurveTo {
                    x1,
                    y1,
                    x2,
                    y2,
                    x3,
                    y3,
                });
            }
            Segment::CurveTo {
                c1x,
                c1y,
                c2x,
                c2y,
                x,
                y,
            } => {
                pen = (x, y);
                let (x1, y1) = transform.apply(c1x, c1y);
                let (x2, y2) = transform.apply(c2x, c2y);
                let (x3, y3) = transform.apply(x, y);
                out.push(PathSegment::CurveTo {
                    x1,
                    y1,
                    x2,
                    y2,
                    x3,
                    y3,
                });
            }
            Segment::Close => {
                pen = start;
                out.push(PathSegment::Close);
            }
        }
    }
    out
}

/// One gradient stop: its offset and its colour.
type Stop = (f64, (u8, u8, u8));

/// A gradient's stops, when one states `function` in `space` exactly: see
/// the module documentation for the conditions.
fn gradient_stops(space: &ColorSpace, function: &Function) -> Option<Vec<Stop>> {
    if !matches!(space, ColorSpace::DeviceRgb | ColorSpace::DeviceGray) || !linear(function) {
        return None;
    }
    let mut points = vec![0.0, 1.0];
    breakpoints(function, &mut points);
    points.retain(|t| t.is_finite() && (0.0..=1.0).contains(t));
    points.sort_by(f64::total_cmp);
    points.dedup();
    // Linear between breakpoints, so inside `[0, 1]` everywhere exactly when
    // it is at every breakpoint and on each side of one.
    let values = |t: f64| -> Option<(u8, u8, u8)> {
        let out = function.eval(&[t]);
        out.iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
            .then(|| space.to_rgb(&out))
    };
    const SIDE: f64 = 1e-9;
    let mut stops = Vec::with_capacity(points.len() + 2);
    for &t in &points {
        let at = values(t)?;
        let before = if t > 0.0 { values(t - SIDE)? } else { at };
        let after = if t < 1.0 { values(t + SIDE)? } else { at };
        if before != after {
            stops.push((t, before));
            stops.push((t, after));
        } else {
            stops.push((t, at));
        }
    }
    Some(stops)
}

/// Whether a function is linear between its breakpoints.
fn linear(function: &Function) -> bool {
    match function {
        Function::Exponential { n, .. } => *n == 1.0,
        Function::Stitching { functions, .. } | Function::Array(functions) => {
            !functions.is_empty() && functions.iter().all(linear)
        }
        _ => false,
    }
}

/// Where a piecewise-linear function may bend, in its own input space.
fn breakpoints(function: &Function, out: &mut Vec<f64>) {
    match function {
        Function::Exponential { domain, .. } => {
            out.push(domain.0);
            out.push(domain.1);
        }
        Function::Stitching {
            domain,
            functions,
            bounds,
            encode,
        } => {
            out.push(domain.0);
            out.push(domain.1);
            out.extend(bounds.iter().copied());
            for (index, piece) in functions.iter().enumerate() {
                let lo = if index == 0 {
                    domain.0
                } else {
                    bounds.get(index - 1).copied().unwrap_or(domain.1)
                };
                let hi = bounds.get(index).copied().unwrap_or(domain.1);
                let Some(&(e0, e1)) = encode.get(index) else {
                    continue;
                };
                if e1 == e0 || hi <= lo {
                    continue;
                }
                let mut inner = Vec::new();
                breakpoints(piece, &mut inner);
                for s in inner {
                    let t = lo + (s - e0) * (hi - lo) / (e1 - e0);
                    if t > lo && t < hi {
                        out.push(t);
                    }
                }
            }
        }
        Function::Array(functions) => {
            for piece in functions {
                breakpoints(piece, out);
            }
        }
        _ => {}
    }
}

fn blend_name(mode: BlendMode) -> &'static str {
    match mode {
        BlendMode::Normal => "Normal",
        BlendMode::Multiply => "Multiply",
        BlendMode::Screen => "Screen",
        BlendMode::Overlay => "Overlay",
        BlendMode::Darken => "Darken",
        BlendMode::Lighten => "Lighten",
        BlendMode::ColorDodge => "ColorDodge",
        BlendMode::ColorBurn => "ColorBurn",
        BlendMode::HardLight => "HardLight",
        BlendMode::SoftLight => "SoftLight",
        BlendMode::Difference => "Difference",
        BlendMode::Exclusion => "Exclusion",
        BlendMode::Hue => "Hue",
        BlendMode::Saturation => "Saturation",
        BlendMode::Color => "Color",
        BlendMode::Luminosity => "Luminosity",
    }
}

/// RGBA samples as a PNG.
fn png(width: u32, height: u32, stride: usize, data: &[u8]) -> Option<Vec<u8>> {
    use tinker_pdf_filters::{png_encode, PngColour, PngSource};
    png_encode(&PngSource {
        width,
        height,
        colour: PngColour::Rgba,
        stride,
        data,
    })
    .ok()
}

/// RFC 4648 §4's base64, padded, which is what a `data:` URI carries
/// (RFC 2397).
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk.first().copied().unwrap_or(0),
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let sextet = |shift: u32| char::from(ALPHABET[((n >> shift) & 0x3F) as usize]);
        out.push(sextet(18));
        out.push(sextet(12));
        out.push(if chunk.len() > 1 { sextet(6) } else { '=' });
        out.push(if chunk.len() > 2 { sextet(0) } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 4648 §10's test vectors, every one.
    #[test]
    fn base64_is_rfc_4648_s_own() {
        for (plain, coded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(plain.as_bytes()), coded, "{plain:?}");
        }
    }

    /// The caller's budget lowers the cap and never raises it — the half of
    /// `svg_output.rs`'s budget test that no page short of a quarter of a
    /// gigabyte of markup could show.
    #[test]
    fn a_budget_past_the_cap_is_the_cap() {
        let at = |max_bytes| {
            budget(&SvgOptions {
                max_bytes,
                ..SvgOptions::default()
            })
        };
        assert_eq!(at(usize::MAX), MAX_SVG_BYTES);
        assert_eq!(at(MAX_SVG_BYTES + 1), MAX_SVG_BYTES);
        assert_eq!(at(MAX_SVG_BYTES), MAX_SVG_BYTES);
        assert_eq!(at(4_096), 4_096);
        assert_eq!(at(0), 0);
        assert_eq!(budget(&SvgOptions::default()), MAX_SVG_BYTES);
    }

    #[test]
    fn numbers_are_four_places_and_never_negative_zero() {
        assert_eq!(num(1.0), "1");
        assert_eq!(num(-0.00001), "0");
        assert_eq!(num(12.345_67), "12.3457");
        assert_eq!(num(-3.5), "-3.5");
        assert_eq!(num(f64::NAN), "0");
        assert_eq!(num(612.0), "612");
        // Where four places would overflow the product, a whole number --
        // and never `inf`, which no reader parses.
        assert_eq!(num(1e305).len(), 306);
        assert!(num(-1e305).starts_with("-1000"));
        assert_eq!(num(123_456_789_012.7), "123456789013");
    }

    /// An exponential of `N` 1 is two stops; a stitch of two is three, and a
    /// discontinuity is two stops at one offset; `N` 2, CMYK, or a value
    /// outside `0..=1` is no gradient at all.
    #[test]
    fn a_gradient_is_written_only_where_it_is_exact() {
        let ramp = |c0: Vec<f64>, c1: Vec<f64>, n: f64| Function::Exponential {
            domain: (0.0, 1.0),
            c0,
            c1,
            n,
        };
        let rgb = ColorSpace::DeviceRgb;
        assert_eq!(
            gradient_stops(&rgb, &ramp(vec![1.0, 0.0, 0.0], vec![0.0, 0.0, 1.0], 1.0)),
            Some(vec![(0.0, (255, 0, 0)), (1.0, (0, 0, 255))])
        );
        assert_eq!(
            gradient_stops(&rgb, &ramp(vec![1.0, 0.0, 0.0], vec![0.0, 0.0, 1.0], 2.0)),
            None
        );
        assert_eq!(
            gradient_stops(
                &ColorSpace::DeviceCmyk,
                &ramp(vec![0.0; 4], vec![1.0; 4], 1.0)
            ),
            None
        );
        assert_eq!(
            gradient_stops(&rgb, &ramp(vec![-0.5, 0.0, 0.0], vec![1.0, 0.0, 0.0], 1.0)),
            None
        );
        let stitched = Function::Stitching {
            domain: (0.0, 1.0),
            functions: vec![
                ramp(vec![0.0, 0.0, 0.0], vec![1.0, 1.0, 1.0], 1.0),
                ramp(vec![0.0, 0.0, 1.0], vec![0.0, 1.0, 0.0], 1.0),
            ],
            bounds: vec![0.25],
            encode: vec![(0.0, 1.0), (0.0, 1.0)],
        };
        assert_eq!(
            gradient_stops(&rgb, &stitched),
            Some(vec![
                (0.0, (0, 0, 0)),
                (0.25, (255, 255, 255)),
                (0.25, (0, 0, 255)),
                (1.0, (0, 255, 0)),
            ])
        );
    }
}
