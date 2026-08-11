//! Graphics and text state (8.4, 9.3).

/// A 2-D affine transform, in PDF's row-vector convention (8.3.3):
/// `[a b c d e f]` maps `(x, y)` to `(a·x + c·y + e, b·x + d·y + f)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrix {
    /// Row 1, column 1.
    pub a: f64,
    /// Row 1, column 2.
    pub b: f64,
    /// Row 2, column 1.
    pub c: f64,
    /// Row 2, column 2.
    pub d: f64,
    /// Horizontal translation.
    pub e: f64,
    /// Vertical translation.
    pub f: f64,
}

impl Default for Matrix {
    fn default() -> Self {
        Matrix::IDENTITY
    }
}

impl Matrix {
    /// The identity.
    pub const IDENTITY: Matrix = Matrix {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// Uniform scaling.
    #[must_use]
    pub fn scale(sx: f64, sy: f64) -> Matrix {
        Matrix {
            a: sx,
            d: sy,
            ..Matrix::IDENTITY
        }
    }

    /// Translation.
    #[must_use]
    pub fn translate(tx: f64, ty: f64) -> Matrix {
        Matrix {
            e: tx,
            f: ty,
            ..Matrix::IDENTITY
        }
    }

    /// `self` then `other` — the order `cm` composes in (8.4.4).
    #[must_use]
    pub fn then(&self, other: &Matrix) -> Matrix {
        Matrix {
            a: self.a * other.a + self.b * other.c,
            b: self.a * other.b + self.b * other.d,
            c: self.c * other.a + self.d * other.c,
            d: self.c * other.b + self.d * other.d,
            e: self.e * other.a + self.f * other.c + other.e,
            f: self.e * other.b + self.f * other.d + other.f,
        }
    }

    /// Applies the transform to a point.
    #[must_use]
    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    /// The scale this transform applies to lengths, as the geometric mean of
    /// its axes — what a font size must be multiplied by to reach device
    /// space.
    #[must_use]
    pub fn expansion(&self) -> f64 {
        (self.a * self.d - self.b * self.c).abs().sqrt()
    }

    /// Whether every component is finite. A content stream can produce NaN,
    /// and one NaN in the matrix silently voids everything drawn after it.
    #[must_use]
    pub fn is_finite(&self) -> bool {
        [self.a, self.b, self.c, self.d, self.e, self.f]
            .iter()
            .all(|v| v.is_finite())
    }
}

/// How glyphs are painted (9.3.6).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextRenderMode {
    /// 0: fill.
    #[default]
    Fill,
    /// 1: stroke.
    Stroke,
    /// 2: fill then stroke.
    FillStroke,
    /// 3: invisible — the mode scanned pages use for their OCR layer, which
    /// extraction must still read.
    Invisible,
    /// 4: fill and add to the clipping path.
    FillClip,
    /// 5: stroke and clip.
    StrokeClip,
    /// 6: fill, stroke and clip.
    FillStrokeClip,
    /// 7: clip only.
    Clip,
}

impl TextRenderMode {
    /// From the operand of `Tr`.
    #[must_use]
    pub fn from_i64(value: i64) -> TextRenderMode {
        match value {
            1 => TextRenderMode::Stroke,
            2 => TextRenderMode::FillStroke,
            3 => TextRenderMode::Invisible,
            4 => TextRenderMode::FillClip,
            5 => TextRenderMode::StrokeClip,
            6 => TextRenderMode::FillStrokeClip,
            7 => TextRenderMode::Clip,
            _ => TextRenderMode::Fill,
        }
    }

    /// Whether the mode contributes to the clipping path (modes 4 to 7).
    #[must_use]
    pub fn clips(self) -> bool {
        matches!(
            self,
            TextRenderMode::FillClip
                | TextRenderMode::StrokeClip
                | TextRenderMode::FillStrokeClip
                | TextRenderMode::Clip
        )
    }

    /// Whether the mode fills the glyph (9.3.6, Table 106).
    #[must_use]
    pub fn fills(self) -> bool {
        matches!(
            self,
            TextRenderMode::Fill
                | TextRenderMode::FillStroke
                | TextRenderMode::FillClip
                | TextRenderMode::FillStrokeClip
        )
    }

    /// Whether the mode strokes the glyph's outline.
    #[must_use]
    pub fn strokes(self) -> bool {
        matches!(
            self,
            TextRenderMode::Stroke
                | TextRenderMode::FillStroke
                | TextRenderMode::StrokeClip
                | TextRenderMode::FillStrokeClip
        )
    }

    /// Whether the mode paints anything.
    #[must_use]
    pub fn paints(self) -> bool {
        !matches!(self, TextRenderMode::Invisible | TextRenderMode::Clip)
    }
}

/// The text state parameters (9.3), which survive `q`/`Q` inside `BT`/`ET`.
#[derive(Clone, Debug)]
pub struct TextState {
    /// `Tf` size.
    pub size: f64,
    /// `Tc`, character spacing, in unscaled text units.
    pub char_spacing: f64,
    /// `Tw`, word spacing.
    pub word_spacing: f64,
    /// `Tz`, horizontal scaling, as a fraction (100 in the file is 1.0 here).
    pub horizontal_scale: f64,
    /// `TL`, leading.
    pub leading: f64,
    /// `Ts`, rise.
    pub rise: f64,
    /// `Tr`, render mode.
    pub render_mode: TextRenderMode,
    /// The resource name of the current font, from `Tf`.
    pub font: Option<Vec<u8>>,
}

impl Default for TextState {
    fn default() -> Self {
        TextState {
            size: 0.0,
            char_spacing: 0.0,
            word_spacing: 0.0,
            horizontal_scale: 1.0,
            leading: 0.0,
            rise: 0.0,
            render_mode: TextRenderMode::Fill,
            font: None,
        }
    }
}

/// A colour, resolved to RGB by the time it reaches a device.
///
/// The interpreter does not know colour spaces — resolving `/CS` names against
/// a resource dictionary is the caller's job, through
/// [`crate::ColorResolver`] — but it does track which components are current,
/// and hands over the RGB they mean.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
}

impl Rgb {
    /// Opaque black, the initial colour in every device space (8.6.8).
    pub const BLACK: Rgb = Rgb { r: 0, g: 0, b: 0 };
}

impl Default for Rgb {
    fn default() -> Self {
        Rgb::BLACK
    }
}

/// How a stroke's ends are finished (8.4.3.3, Table 54).
///
/// Restated here rather than imported: the rasterizer has the same three
/// cases, but ruling 8 keeps this crate free of it, and a leaf crate must not
/// learn PDF operator numbers to be usable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineCap {
    /// 0: cut off square at the endpoint.
    #[default]
    Butt,
    /// 1: a semicircle beyond the endpoint.
    Round,
    /// 2: a square extending half the width beyond.
    Square,
}

impl LineCap {
    /// Reads the operand of `J`. Anything else is the default (8.4.3.3).
    #[must_use]
    pub fn from_operand(value: f64) -> LineCap {
        match value as i64 {
            1 => LineCap::Round,
            2 => LineCap::Square,
            _ => LineCap::Butt,
        }
    }
}

/// How a stroke's corners are finished (8.4.3.4, Table 55).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineJoin {
    /// 0: extended to a point, unless the miter limit is exceeded.
    #[default]
    Miter,
    /// 1: rounded.
    Round,
    /// 2: cut off square.
    Bevel,
}

impl LineJoin {
    /// Reads the operand of `j`. Anything else is the default (8.4.3.4).
    #[must_use]
    pub fn from_operand(value: f64) -> LineJoin {
        match value as i64 {
            1 => LineJoin::Round,
            2 => LineJoin::Bevel,
            _ => LineJoin::Miter,
        }
    }
}

/// How a source colour combines with what is already there (11.3.5).
///
/// Restated here rather than imported from the rasteriser: ruling 8 keeps
/// this crate free of any dependency on how pixels are made, exactly as
/// `LineCap` and `LineJoin` are. The rendering device maps one onto the
/// other, which is also where the two would be caught disagreeing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BlendMode {
    /// The source replaces the backdrop, and the default.
    #[default]
    Normal,
    /// `/Multiply`.
    Multiply,
    /// `/Screen`.
    Screen,
    /// `/Overlay`.
    Overlay,
    /// `/Darken`.
    Darken,
    /// `/Lighten`.
    Lighten,
    /// `/ColorDodge`.
    ColorDodge,
    /// `/ColorBurn`.
    ColorBurn,
    /// `/HardLight`.
    HardLight,
    /// `/SoftLight`.
    SoftLight,
    /// `/Difference`.
    Difference,
    /// `/Exclusion`.
    Exclusion,
    /// `/Hue`.
    Hue,
    /// `/Saturation`.
    Saturation,
    /// `/Color`.
    Color,
    /// `/Luminosity`.
    Luminosity,
}

impl BlendMode {
    /// The mode a `/BM` name selects.
    ///
    /// 11.3.5: `/Compatible` is a synonym for `/Normal`, and a name this
    /// build does not know is `Normal` rather than an error — a document
    /// using a blend mode from a later specification should still draw.
    #[must_use]
    pub fn from_name(name: &[u8]) -> BlendMode {
        match name {
            b"Multiply" => BlendMode::Multiply,
            b"Screen" => BlendMode::Screen,
            b"Overlay" => BlendMode::Overlay,
            b"Darken" => BlendMode::Darken,
            b"Lighten" => BlendMode::Lighten,
            b"ColorDodge" => BlendMode::ColorDodge,
            b"ColorBurn" => BlendMode::ColorBurn,
            b"HardLight" => BlendMode::HardLight,
            b"SoftLight" => BlendMode::SoftLight,
            b"Difference" => BlendMode::Difference,
            b"Exclusion" => BlendMode::Exclusion,
            b"Hue" => BlendMode::Hue,
            b"Saturation" => BlendMode::Saturation,
            b"Color" => BlendMode::Color,
            b"Luminosity" => BlendMode::Luminosity,
            _ => BlendMode::Normal,
        }
    }

    /// The first name in a `/BM` array this build recognises (11.3.5).
    ///
    /// The array form exists so a producer can offer fallbacks; taking the
    /// first *entry* rather than the first *recognised* entry would pick a
    /// mode nobody implements and render Normal anyway.
    #[must_use]
    pub fn from_names<'a>(names: impl IntoIterator<Item = &'a [u8]>) -> BlendMode {
        for name in names {
            let mode = BlendMode::from_name(name);
            if mode != BlendMode::Normal {
                return mode;
            }
        }
        BlendMode::Normal
    }
}

/// The graphics state (8.4.1), reduced to what interpretation needs.
#[derive(Clone, Debug, Default)]
pub struct GraphicsState {
    /// The current transformation matrix.
    pub ctm: Matrix,
    /// Text parameters.
    pub text: TextState,
    /// `ca`, the non-stroking alpha.
    pub fill_alpha: f64,
    /// `CA`, the stroking alpha.
    pub stroke_alpha: f64,
    /// The non-stroking colour.
    pub fill_color: Rgb,
    /// The stroking colour.
    pub stroke_color: Rgb,
    /// `w`, the line width in user space.
    pub line_width: f64,
    /// `J`, the line cap.
    pub line_cap: LineCap,
    /// `j`, the line join.
    pub line_join: LineJoin,
    /// `M`, the miter limit.
    pub miter_limit: f64,
    /// `d`, the dash array in user space. Empty means a solid line.
    pub dashes: Vec<f64>,
    /// `d`, the distance into the pattern at which to start.
    pub dash_phase: f64,
    /// The resource name of the non-stroking colour space, when one was set
    /// by name rather than by a device operator.
    pub fill_space: Option<Vec<u8>>,
    /// The same for the stroking colour space.
    pub stroke_space: Option<Vec<u8>>,
    /// The pattern name a non-stroking `scn` selected, when it selected one.
    ///
    /// 8.7.3: a pattern carries no colour of its own — the paint comes from
    /// the pattern object. Without this the interpreter has nowhere to put the
    /// name, the device never learns a pattern was chosen, and the black that
    /// `/Pattern` reports as its initial colour is what gets painted.
    pub fill_pattern: Option<Vec<u8>>,
    /// `/BM` from the external graphics state (11.3.5).
    pub blend: BlendMode,
    /// The same for a stroking `SCN`.
    pub stroke_pattern: Option<Vec<u8>>,
}

impl GraphicsState {
    /// The initial state for a page.
    #[must_use]
    pub fn new(ctm: Matrix) -> GraphicsState {
        GraphicsState {
            ctm,
            text: TextState::default(),
            fill_alpha: 1.0,
            stroke_alpha: 1.0,
            fill_color: Rgb::BLACK,
            stroke_color: Rgb::BLACK,
            // 8.4.3.2: the initial line width is 1.0 user-space units.
            line_width: 1.0,
            line_cap: LineCap::Butt,
            line_join: LineJoin::Miter,
            // 8.4.3.5: the initial miter limit is 10.
            miter_limit: 10.0,
            dashes: Vec::new(),
            dash_phase: 0.0,
            fill_space: None,
            stroke_space: None,
            fill_pattern: None,
            blend: BlendMode::Normal,
            stroke_pattern: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composition_follows_pdfs_row_vector_convention() {
        let scale = Matrix::scale(2.0, 3.0);
        let move_it = Matrix::translate(10.0, 20.0);

        // Scaling then translating moves by the untransformed amount.
        let combined = scale.then(&move_it);
        assert_eq!(combined.apply(1.0, 1.0), (12.0, 23.0));

        // The other order scales the translation too.
        let other = move_it.then(&scale);
        assert_eq!(other.apply(1.0, 1.0), (22.0, 63.0));
    }

    #[test]
    fn identity_changes_nothing() {
        let m = Matrix::IDENTITY;
        assert_eq!(m.apply(3.0, 4.0), (3.0, 4.0));
        assert_eq!(m.then(&m), m);
        assert_eq!(m.expansion(), 1.0);
    }

    #[test]
    fn expansion_is_the_area_scale() {
        assert_eq!(Matrix::scale(2.0, 2.0).expansion(), 2.0);
        assert_eq!(Matrix::scale(3.0, 12.0).expansion(), 6.0);
        // A 90-degree rotation preserves length.
        let rot = Matrix {
            a: 0.0,
            b: 1.0,
            c: -1.0,
            d: 0.0,
            e: 0.0,
            f: 0.0,
        };
        assert!((rot.expansion() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn non_finite_matrices_are_detectable() {
        assert!(Matrix::IDENTITY.is_finite());
        let bad = Matrix {
            a: f64::NAN,
            ..Matrix::IDENTITY
        };
        assert!(!bad.is_finite());
        let infinite = Matrix::scale(f64::INFINITY, 1.0);
        assert!(!infinite.is_finite());
    }

    #[test]
    fn render_modes_map_from_their_operands() {
        assert_eq!(TextRenderMode::from_i64(0), TextRenderMode::Fill);
        assert_eq!(TextRenderMode::from_i64(3), TextRenderMode::Invisible);
        assert_eq!(TextRenderMode::from_i64(7), TextRenderMode::Clip);
        assert_eq!(
            TextRenderMode::from_i64(99),
            TextRenderMode::Fill,
            "an out-of-range mode falls back rather than failing"
        );

        // Invisible text is still text: extraction reads it, painting skips it.
        assert!(!TextRenderMode::Invisible.paints());
        assert!(!TextRenderMode::Invisible.clips());
        assert!(TextRenderMode::FillClip.clips() && TextRenderMode::FillClip.paints());
        assert!(TextRenderMode::Clip.clips() && !TextRenderMode::Clip.paints());
    }
}
