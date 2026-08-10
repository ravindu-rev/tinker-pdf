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
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenderWarning {
    /// An image used a codec that is not built in; a placeholder was drawn.
    UnsupportedImage {
        /// Which codec.
        codec: String,
    },
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
        }
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
        self.paint(&path, FillRule::NonZero, PLACEHOLDER, state.fill_alpha);
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
    /// `state.ctm` does not.
    fn fill_with_pattern(
        &mut self,
        path: &Path,
        rule: FillRule,
        name: &[u8],
        state: &GraphicsState,
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

        let alpha = state.fill_alpha.clamp(0.0, 1.0);
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
                    .blend_pixel(px, py, Color { r, g, b, a: 0xFF }, weight);
            }
        }
    }

    fn paint(&mut self, path: &Path, rule: FillRule, color: Color, alpha: f64) {
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
        self.canvas.fill_mask(&mask, color, alpha);
    }

    /// Draws a decoded image into the unit square of the current transform.
    ///
    /// 8.9.5.2: an image occupies the unit square in user space whatever its
    /// pixel dimensions, so the transform carries the placement. Sampling maps
    /// *backwards* — every destination pixel takes exactly one source lookup —
    /// which is what avoids the seams and double-writes a forward map leaves
    /// under rotation or scaling.
    fn blit(&mut self, image: &DecodedImage, state: &GraphicsState) {
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
                self.canvas.blend_pixel(px, py, color, effective);
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
        if self.cancel.is_cancelled() {
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
            self.fill_with_pattern(&built, rule, &name, state);
            return;
        }

        let color = fill_color(state);
        self.paint(&built, rule, color, state.fill_alpha);
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
        let Some(path) = self.text_clip.take() else {
            return;
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
        if self.cancel.is_cancelled() {
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
        let color = stroke_color(state);
        self.paint(&outline, FillRule::NonZero, color, state.stroke_alpha);
    }

    fn show_glyph(&mut self, glyph: &Glyph, state: &GraphicsState) {
        if self.cancel.is_cancelled() {
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
        if mode.fills() {
            self.paint(
                &path,
                FillRule::NonZero,
                fill_color(state),
                state.fill_alpha,
            );
        }
        if mode.strokes() {
            let scale = state.ctm.then(&self.base).expansion();
            let style = StrokeStyle {
                width: (state.line_width * scale).max(0.6),
                ..StrokeStyle::default()
            };
            let outlined = stroke(&path, &style, self.tolerance);
            self.paint(
                &outlined,
                FillRule::NonZero,
                stroke_color(state),
                state.stroke_alpha,
            );
        }
        if mode.clips() {
            // 9.3.6: the glyphs of a text object accumulate into one clip that
            // takes effect at `ET`, not glyph by glyph — clipping immediately
            // would leave each glyph clipped to itself, which is empty.
            self.text_clip.get_or_insert_with(Path::new).extend(&path);
        }
    }

    fn draw_image(&mut self, image: &ImageRef, state: &GraphicsState) {
        if self.cancel.is_cancelled() {
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
        if self.cancel.is_cancelled() {
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
                    .blend_pixel(px, py, Color::rgb(r, g, b), effective);
            }
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
        Verb::CurveTo(_, _, p) => Some((p.x, p.y)),
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
/// keeping its aspect ratio — degraded rather than refused (ruling 2).
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

    #[test]
    fn rendering_is_bit_identical_across_runs() {
        let content = b"1 0 0 1 3 3 cm 0 0 m 12 1 l 6 13 l h f 2 2 m 14 14 l S";
        let (a, _) = render(content, 24);
        let (b, _) = render(content, 24);
        assert_eq!(a.data, b.data, "ruling 4");
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
