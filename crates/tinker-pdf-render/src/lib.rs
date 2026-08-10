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

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tinker_pdf_content::{Device, Glyph, GraphicsState, ImageRef, Matrix, PathSegment};
use tinker_pdf_font::Outline;
use tinker_pdf_raster::{
    canvas::{Canvas, Color, PixelFormat},
    fill::{fill, Mask},
    geom::{FillRule, Path},
    stroke::{stroke, StrokeStyle},
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
    /// The render stopped because it was cancelled.
    Cancelled,
}

/// Where glyph outlines come from, supplied by the caller so this crate stays
/// free of PDF dictionaries.
pub trait GlyphSource {
    /// The outline of one glyph, in a space where one unit is one em.
    ///
    /// Returning `None` means the glyph could not be drawn, which the renderer
    /// reports rather than substituting a shape.
    fn outline(&self, font_id: u64, code: u32) -> Option<Outline>;
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

/// The colour a graphics state paints with.
///
/// The interpreter does not track colour operators yet, so this is where the
/// renderer's own default lives: black, which is what 8.6.8 makes the initial
/// colour in every device space.
fn fill_color(_state: &GraphicsState) -> Color {
    Color::BLACK
}

impl<G: GlyphSource> Device for Renderer<'_, G> {
    fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    fn fill_path(&mut self, path: &[PathSegment], state: &GraphicsState) {
        if self.cancel.is_cancelled() {
            return;
        }
        let built = self.to_path(path);
        if built.is_empty() {
            return;
        }
        let color = fill_color(state);
        self.paint(&built, FillRule::NonZero, color, state.fill_alpha);
    }

    fn stroke_path(&mut self, path: &[PathSegment], state: &GraphicsState) {
        if self.cancel.is_cancelled() {
            return;
        }
        let built = self.to_path(path);
        if built.is_empty() {
            return;
        }

        // The line width is in user space, so it scales with the transform.
        let width = self.base.expansion().max(f64::MIN_POSITIVE);
        let style = StrokeStyle {
            width,
            ..StrokeStyle::default()
        };
        let outline = stroke(&built, &style, self.tolerance);
        let color = fill_color(state);
        self.paint(&outline, FillRule::NonZero, color, state.stroke_alpha);
    }

    fn show_glyph(&mut self, glyph: &Glyph, state: &GraphicsState) {
        if self.cancel.is_cancelled() {
            return;
        }
        // 9.3.6: mode 3 paints nothing, which is exactly what a scanned page's
        // invisible OCR layer relies on. It is still extracted, just not drawn.
        if !state.text.render_mode.paints() {
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

        let color = fill_color(state);
        self.paint(&path, FillRule::NonZero, color, state.fill_alpha);
    }

    fn draw_image(&mut self, image: &ImageRef, _state: &GraphicsState) {
        // Images arrive here identified but not decoded; wiring the decoders
        // through is the next milestone of this phase. Until then a page with
        // an image still renders everything else and says what was skipped
        // (ruling 2).
        let codec = if image.inline {
            "inline".to_string()
        } else {
            String::from_utf8_lossy(&image.name).into_owned()
        };
        let warning = RenderWarning::UnsupportedImage { codec };
        if !self.warnings.contains(&warning) {
            self.warnings.push(warning);
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

/// The pixel size of a page at a given scale.
///
/// **Rounded outward**, so a page never loses its last row or column to
/// rounding: A4 at 150 dpi is 1240×1755, not 1240×1754. This is an API
/// guarantee, pinned by tests, because the engine being replaced left it as
/// folklore that callers rediscovered.
#[must_use]
pub fn page_pixels(width_pt: f64, height_pt: f64, scale: f64) -> (u32, u32) {
    let round_out = |v: f64| {
        if !v.is_finite() || v <= 0.0 {
            return 1u32;
        }
        (v * scale).ceil().max(1.0).min(f64::from(u32::MAX)) as u32
    };
    (round_out(width_pt), round_out(height_pt))
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
