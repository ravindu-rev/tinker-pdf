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

pub mod shading;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub use shading::Shading;
use tinker_pdf_content::{
    Device, Glyph, GraphicsState, ImageRef, LineCap as ContentCap, LineJoin as ContentJoin, Matrix,
    PathSegment,
};

use tinker_pdf_font::Outline;
use tinker_pdf_raster::{
    canvas::{Canvas, Color, PixelFormat},
    fill::{fill, Mask},
    geom::{FillRule, Path},
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
    /// The render stopped because it was cancelled.
    Cancelled,
}

/// What a pattern name paints with (8.7.3).
#[derive(Clone, Debug)]
pub enum PatternPaint {
    /// A shading pattern, and the matrix mapping pattern space to the page's
    /// default space.
    Shading(Box<Shading>, Matrix),
    /// A pattern this build does not paint — a tiling pattern, or a shading
    /// pattern over a mesh.
    ///
    /// The area is left alone and reported, rather than filled with the black
    /// that `/Pattern` nominally reports as its colour: an unpainted area
    /// reads as missing, a black one reads as content.
    Unsupported,
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
}

/// A `GlyphSource` that has nothing, for callers that only want geometry.
pub struct NoGlyphs;

impl GlyphSource for NoGlyphs {
    fn outline(&self, _font_id: u64, _code: u32) -> Option<Outline> {
        None
    }
}

/// The rasterizing device.
pub struct Renderer<'g, G: GlyphSource> {
    canvas: Canvas,
    glyphs: &'g G,
    /// The transform from PDF user space to device pixels.
    base: Matrix,
    clip: Option<Mask>,
    clip_stack: Vec<Option<Mask>>,
    warnings: Vec<RenderWarning>,
    cancel: CancelToken,
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
            tolerance: 0.2,
            missing_fonts: 0,
            text_clip: None,
            text_clip_requested: false,
            marked_content: Vec::new(),
            hidden_depth: 0,
        }
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

    /// The canvas and everything the render had to tolerate.
    #[must_use]
    pub fn finish(mut self) -> (Canvas, Vec<RenderWarning>) {
        if self.missing_fonts > 0 {
            self.warnings.push(RenderWarning::UnreadableFont);
        }
        if self.cancel.is_cancelled() {
            self.warnings.push(RenderWarning::Cancelled);
        }
        (self.canvas, self.warnings)
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

    /// Fills a path with a shading pattern, or reports one that cannot be.
    ///
    /// The path becomes the clip and the shading is evaluated per pixel inside
    /// it, which is the same machinery `sh` uses — a shading pattern is `sh`
    /// bounded by a path rather than by the current clip.
    ///
    /// 8.7.3.1: a pattern's matrix maps pattern space to the *default* space
    /// of the page, not to the space in force when it is used. The CTM at fill
    /// time is therefore not part of it, which is why `base` appears here and
    /// `state.ctm` does not. A stroke reaches this through its outline, so
    /// that guarantee covers strokes too — the stroker's transform never
    /// enters the pattern.
    ///
    /// `alpha` is a parameter rather than a field of `state` because 8.4.5
    /// gives painting two of them: a fill uses `ca`, a stroke uses `CA`, and
    /// the callers are the only things that know which they are.
    fn fill_with_pattern(
        &mut self,
        path: &Path,
        rule: FillRule,
        name: &[u8],
        state: &GraphicsState,
        alpha: f64,
    ) {
        let (shading, matrix) = match self.glyphs.pattern(name) {
            Some(PatternPaint::Shading(shading, matrix)) => (*shading, matrix),
            None => return,
            Some(PatternPaint::Unsupported) => {
                let warning = RenderWarning::UnsupportedPattern {
                    name: String::from_utf8_lossy(name).into_owned(),
                };
                if !self.warnings.contains(&warning) {
                    self.warnings.push(warning);
                }
                return;
            }
        };

        let to_device = matrix.then(&self.base);
        let Some(inverse) = invert(&to_device) else {
            return;
        };

        let area = fill(
            path,
            rule,
            0,
            0,
            self.canvas.width,
            self.canvas.height,
            self.tolerance,
        );
        let area = match &self.clip {
            Some(clip) => area.intersect(clip),
            None => area,
        };

        let alpha = alpha.clamp(0.0, 1.0);
        let mode = blend_mode(state.blend);
        for py in 0..self.canvas.height {
            if self.cancel.is_cancelled() {
                return;
            }
            for px in 0..self.canvas.width {
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
    }

    fn paint(&mut self, path: &Path, rule: FillRule, color: Color, alpha: f64, mode: RasterBlend) {
        let mask = fill(
            path,
            rule,
            0,
            0,
            self.canvas.width,
            self.canvas.height,
            self.tolerance,
        );
        let mask = match &self.clip {
            Some(clip) => mask.intersect(clip),
            None => mask,
        };
        self.canvas.fill_mask_with(&mask, color, alpha, mode);
    }

    /// Draws a decoded image into the unit square of the current transform.
    ///
    /// 8.9.5.2: an image occupies the unit square in user space whatever its
    /// pixel dimensions, so the transform carries the placement. Sampling maps
    /// *backwards* — every destination pixel takes exactly one source lookup —
    /// which is what avoids the seams and double-writes a forward map leaves
    /// under rotation or scaling.
    fn blit(&mut self, image: &DecodedImage, state: &GraphicsState) {
        let mode = blend_mode(state.blend);
        if image.width == 0 || image.height == 0 {
            return;
        }

        let unit_to_device = state.ctm.then(&self.base);
        let Some(inverse) = invert(&unit_to_device) else {
            return; // A degenerate transform maps the image to nothing.
        };

        // Only the pixels the unit square could cover need visiting.
        let corners = [
            unit_to_device.apply(0.0, 0.0),
            unit_to_device.apply(1.0, 0.0),
            unit_to_device.apply(0.0, 1.0),
            unit_to_device.apply(1.0, 1.0),
        ];
        if corners
            .iter()
            .any(|(x, y)| !x.is_finite() || !y.is_finite())
        {
            return;
        }
        let xs = corners.iter().map(|(x, _)| *x);
        let ys = corners.iter().map(|(_, y)| *y);
        let x0 = xs.clone().fold(f64::INFINITY, f64::min).floor().max(0.0) as u32;
        let x1 =
            (xs.fold(f64::NEG_INFINITY, f64::max).ceil().max(0.0) as u32).min(self.canvas.width);
        let y0 = ys.clone().fold(f64::INFINITY, f64::min).floor().max(0.0) as u32;
        let y1 =
            (ys.fold(f64::NEG_INFINITY, f64::max).ceil().max(0.0) as u32).min(self.canvas.height);

        let alpha = state.fill_alpha.clamp(0.0, 1.0);
        for py in y0..y1 {
            if self.cancel.is_cancelled() {
                return;
            }
            for px in x0..x1 {
                // Sample at the pixel's centre.
                let (u, v) = inverse.apply(f64::from(px) + 0.5, f64::from(py) + 0.5);
                if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
                    continue;
                }

                // Image rows run top-down; the unit square's v runs upward.
                let sx = ((u * f64::from(image.width)) as u32).min(image.width - 1);
                let sy = (((1.0 - v) * f64::from(image.height)) as u32).min(image.height - 1);
                let index = (sy as usize)
                    .saturating_mul(image.width as usize)
                    .saturating_add(sx as usize);

                let Some(rgb) = image.rgb.get(index * 3..index * 3 + 3) else {
                    continue;
                };
                let coverage = image.alpha.get(index).copied().unwrap_or(255);
                if coverage == 0 {
                    continue;
                }
                let clip = self
                    .clip
                    .as_ref()
                    .map_or(255, |mask| mask.at(px as i32, py as i32));
                if clip == 0 {
                    continue;
                }

                let effective = alpha * f64::from(coverage) / 255.0 * f64::from(clip) / 255.0;
                // 8.9.6.2: a stencil selects where the *current fill colour*
                // is painted; it has no colour of its own.
                let color = if image.stencil {
                    fill_color(state)
                } else {
                    Color::rgb(rgb[0], rgb[1], rgb[2])
                };
                self.canvas.blend_pixel_with(px, py, color, effective, mode);
            }
        }
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
        self.cancel.is_cancelled()
    }

    fn fill_path(&mut self, path: &[PathSegment], state: &GraphicsState, even_odd: bool) {
        if self.cancel.is_cancelled() || self.hidden() {
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
            self.fill_with_pattern(&built, rule, &name, state, state.fill_alpha);
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
        // clipped by another must stay soft on both.
        let mask = fill(
            &built,
            rule,
            0,
            0,
            self.canvas.width,
            self.canvas.height,
            self.tolerance,
        );
        self.clip = Some(match &self.clip {
            Some(existing) => mask.intersect(existing),
            None => mask,
        });
    }

    fn end_text(&mut self) {
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
        let mask = fill(
            &path,
            FillRule::NonZero,
            0,
            0,
            self.canvas.width,
            self.canvas.height,
            self.tolerance,
        );
        self.clip = Some(match &self.clip {
            Some(existing) => mask.intersect(existing),
            None => mask,
        });
    }

    fn save_state(&mut self) {
        self.clip_stack.push(self.clip.clone());
    }

    fn restore_state(&mut self) {
        if let Some(clip) = self.clip_stack.pop() {
            self.clip = clip;
        }
    }

    fn stroke_path(&mut self, path: &[PathSegment], state: &GraphicsState) {
        if self.cancel.is_cancelled() || self.hidden() {
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
        let outline = stroke(&built, &style, self.tolerance);

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
            self.fill_with_pattern(
                &outline,
                FillRule::NonZero,
                &name,
                state,
                state.stroke_alpha,
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
        if self.cancel.is_cancelled() || self.hidden() {
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
                    // A quadratic raised to a cubic: the two control points sit
                    // two thirds of the way from each end toward the control.
                    let (px, py) = last_point(&path).unwrap_or_else(|| combined.apply(x, y));
                    let (cx, cy) = combined.apply(cx, cy);
                    let (x, y) = combined.apply(x, y);
                    path.curve_to(
                        px + 2.0 / 3.0 * (cx - px),
                        py + 2.0 / 3.0 * (cy - py),
                        x + 2.0 / 3.0 * (cx - x),
                        y + 2.0 / 3.0 * (cy - y),
                        x,
                        y,
                    );
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
                self.fill_with_pattern(&path, FillRule::NonZero, &name, state, state.fill_alpha);
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
            let outlined = stroke(&path, &style, self.tolerance);
            if let Some(name) = state.stroke_pattern.clone() {
                self.fill_with_pattern(
                    &outlined,
                    FillRule::NonZero,
                    &name,
                    state,
                    state.stroke_alpha,
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
        if self.cancel.is_cancelled() || self.hidden() {
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
        if self.cancel.is_cancelled() || self.hidden() {
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
        let Some(inverse) = invert(&to_device) else {
            return;
        };

        let alpha = state.fill_alpha.clamp(0.0, 1.0);
        let mode = blend_mode(state.blend);
        for py in 0..self.canvas.height {
            if self.cancel.is_cancelled() {
                return;
            }
            for px in 0..self.canvas.width {
                let clip = self
                    .clip
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
        self.clip_stack.push(self.clip.clone());
        !self.cancel.is_cancelled()
    }

    fn end_form(&mut self, _id: u64) {
        if let Some(clip) = self.clip_stack.pop() {
            self.clip = clip;
        }
    }
}

/// Inverts an affine transform, or `None` when it is degenerate.
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

fn last_point(path: &Path) -> Option<(f64, f64)> {
    use tinker_pdf_raster::geom::Verb;
    match path.verbs().last()? {
        Verb::MoveTo(p) | Verb::LineTo(p) => Some((p.x, p.y)),
        Verb::QuadTo(_, p) | Verb::CurveTo(_, _, p) => Some((p.x, p.y)),
        Verb::Close => None,
    }
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
                width: 2,
                height: 2,
                rgb: vec![r, g, b, r, g, b, r, g, b, r, g, b],
                alpha: self.alpha.clone(),
            }))
        }
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
