//! Graphics and text state (8.4, 9.3).

use std::sync::Arc;

use tinker_pdf_color::Function;

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
#[derive(Clone, Debug, PartialEq)]
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
/// [`crate::FontSource::resolve_color`] — but it does track which components
/// are current, and hands over the RGB they mean.
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

/// A colour at sixteen bits a channel, beside the bytes an [`Rgb`] holds
/// (`docs/design/sixteen-bit.md`, ruling 23): each channel's top byte is the
/// byte the same colour resolves to, bit for bit, and its low byte refines
/// it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rgb16 {
    /// Red.
    pub r: u16,
    /// Green.
    pub g: u16,
    /// Blue.
    pub b: u16,
}

impl Rgb16 {
    /// Opaque black.
    pub const BLACK: Rgb16 = Rgb16 { r: 0, g: 0, b: 0 };

    /// `color` at sixteen bits with nothing finer to take: each byte `v` as
    /// `257·v`, `v` in its top byte and again in its low one — the same value
    /// over 65 535 as `v` over 255.
    #[must_use]
    pub const fn widen(color: Rgb) -> Rgb16 {
        Rgb16 {
            r: color.r as u16 * 257,
            g: color.g as u16 * 257,
            b: color.b as u16 * 257,
        }
    }

    /// The bytes this colour tops out at: each channel's top byte.
    #[must_use]
    pub const fn top(self) -> Rgb {
        Rgb {
            r: (self.r >> 8) as u8,
            g: (self.g >> 8) as u8,
            b: (self.b >> 8) as u8,
        }
    }
}

impl From<[u16; 3]> for Rgb16 {
    /// Red, green and blue, in that order, as the colour crate answers them.
    fn from([r, g, b]: [u16; 3]) -> Rgb16 {
        Rgb16 { r, g, b }
    }
}

/// What a colour paints by colorant (8.6.6.4, 8.6.6.5), beside the light it
/// is ([`GraphicsState::fill_color`]): how its space names colorants, each
/// colorant it names with its tint, and the process ink it paints where a
/// device has no colorant of a name it gives.
///
/// The generalisation of [`GraphicsState::fill_ink`], which carries a
/// DeviceCMYK colour's four inks and nothing else. Asked for only where the
/// device composites colorants ([`Device::composites_colorants`]); every other
/// device's states carry none, and a colour of a space that names no colorant
/// of its own — DeviceGray, DeviceRGB, a CIE-based or ICC space — carries
/// none either, its light being what such a device turns into ink.
///
/// [`Device::composites_colorants`]: crate::Device::composites_colorants
#[derive(Clone, Debug, PartialEq)]
pub struct ColorantTints {
    /// How the colour's space names its colorants.
    pub kind: ColorantKind,
    /// The colorants the colour names, each with its tint, in the space's
    /// order: DeviceCMYK's four process colorants; a `/Separation`'s one; each
    /// of a `/DeviceN`'s components but `/None`, an `NChannel` space's
    /// components of a CMYK process space under the process colorant's name
    /// and none of another process space's, which are [`ColorantTints::process`].
    pub tints: Vec<ColorantTint>,
    /// The process ink the whole colour paints through its space's alternate
    /// (8.6.6.4, 8.6.6.5): a DeviceCMYK alternate's own components, any other
    /// alternate's light turned into ink. What a `/Separation` or a plain
    /// `/DeviceN` paints where a device has no colorant of a name it gives;
    /// `None` for a DeviceCMYK colour, which is its own ink, and for `/All`
    /// and `/None`, which never revert.
    pub alternate: Option<[u8; 4]>,
    /// An `NChannel` space's components of a process space other than CMYK,
    /// converted together through that space — the components its names
    /// leave out at the space's initial values — to light, and the light to
    /// ink (8.6.6.5, Table 71); `None` where there are none.
    pub process: Option<[u8; 4]>,
}

impl ColorantTints {
    /// A DeviceCMYK colour's own components, as bytes, on the four process
    /// colorants (8.6.4.4): `k`, `K` and four components read with no space
    /// named.
    #[must_use]
    pub fn device_cmyk(ink: [u8; 4]) -> ColorantTints {
        let names: [&[u8]; 4] = [b"Cyan", b"Magenta", b"Yellow", b"Black"];
        ColorantTints {
            kind: ColorantKind::DirectCmyk,
            tints: names
                .iter()
                .zip(ink)
                .map(|(name, tint)| ColorantTint {
                    name: Arc::from(*name),
                    tint,
                    alone: None,
                })
                .collect(),
            alternate: None,
            process: None,
        }
    }
}

/// How a colour's space names its colorants (8.6.6.4, 8.6.6.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorantKind {
    /// DeviceCMYK as the current colour — `k`, `K`, `cs`/`sc` in it, and such
    /// a colour under a `/DefaultCMYK` — its four process colorants at the
    /// colour's own components.
    DirectCmyk,
    /// A `/Separation` or a plain `/DeviceN` painting the colorants it names:
    /// all of them where a device has every one, `/None` aside, and the whole
    /// colour through its alternate where it has not.
    Colorants,
    /// An `NChannel` `/DeviceN`, read component by component: each colorant a
    /// device has paints it, and each it has not paints its own process ink.
    NChannel,
    /// `/Separation /All`: every colorant of the device at the one tint.
    All,
    /// `/Separation /None`, or a `/DeviceN` whose every name is `/None`:
    /// nothing at all.
    None,
}

/// One colorant a colour names, and its tint.
#[derive(Clone, Debug, PartialEq)]
pub struct ColorantTint {
    /// The colorant's name, `#` escapes decoded: `Cyan`, `Magenta`, `Yellow`
    /// or `Black` for a process colorant, any other name for a spot.
    pub name: Arc<[u8]>,
    /// Its tint as a byte, `round(255 × tint)`: 0 no ink, 255 solid.
    pub tint: u8,
    /// An `NChannel` spot's own process ink — its tint through its
    /// `/Colorants` space, the colorant alone — painted where a device has no
    /// colorant of its name; `None` for a process colorant and for every
    /// other kind's colorants, which revert as a whole.
    pub alone: Option<[u8; 4]>,
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

/// The device space family a colour's values are in, as the device-dependent
/// rules ask it: black generation and undercolour removal apply only where
/// DeviceRGB becomes DeviceCMYK (11.7.5.3), and a DeviceGray colour on a CMYK
/// device takes only the grey transfer function (10.5).
///
/// An `/Indexed` colour is its base's family and a `/Separation` or
/// `/DeviceN` one its alternate's, as 11.7.4.3's NOTE 2 reads "the current
/// colour space"; a device space a default colour space replaces (8.6.5.6)
/// is its substitute's. Every CIE-based, ICC and pattern space with no base
/// is `Other`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorFamily {
    /// DeviceGray, the initial space (8.6.8).
    #[default]
    Gray,
    /// DeviceRGB.
    Rgb,
    /// DeviceCMYK.
    Cmyk,
    /// Any other space.
    Other,
}

impl ColorFamily {
    /// The family a device space is named by — `DeviceGray`, `DeviceRGB`,
    /// `DeviceCMYK` and their inline abbreviations `G`, `RGB`, `CMYK` — and
    /// `Other` for every other name.
    #[must_use]
    pub fn of_device_name(name: &[u8]) -> ColorFamily {
        match name {
            b"DeviceGray" | b"G" => ColorFamily::Gray,
            b"DeviceRGB" | b"RGB" => ColorFamily::Rgb,
            b"DeviceCMYK" | b"CMYK" => ColorFamily::Cmyk,
            _ => ColorFamily::Other,
        }
    }

    /// The family components read by their count alone are in: one is grey,
    /// four CMYK, any other count RGB — the reading `sc` takes with no space
    /// named.
    #[must_use]
    pub fn by_count(count: usize) -> ColorFamily {
        match count {
            1 => ColorFamily::Gray,
            4 => ColorFamily::Cmyk,
            _ => ColorFamily::Rgb,
        }
    }
}

/// A rendering intent (8.6.5.8), as `/RI` and `ri` name it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RenderingIntent {
    /// `/AbsoluteColorimetric`.
    AbsoluteColorimetric,
    /// `/RelativeColorimetric`, the initial intent (8.4.1, Table 52).
    #[default]
    RelativeColorimetric,
    /// `/Saturation`.
    Saturation,
    /// `/Perceptual`.
    Perceptual,
    /// Any other name. Kept as unrecognised rather than replaced, so the row
    /// that applies intents reads 8.6.5.8's rule for a name it does not know.
    Unrecognised,
}

impl RenderingIntent {
    /// The intent a name selects.
    #[must_use]
    pub fn from_name(name: &[u8]) -> RenderingIntent {
        match name {
            b"AbsoluteColorimetric" => RenderingIntent::AbsoluteColorimetric,
            b"RelativeColorimetric" => RenderingIntent::RelativeColorimetric,
            b"Saturation" => RenderingIntent::Saturation,
            b"Perceptual" => RenderingIntent::Perceptual,
            _ => RenderingIntent::Unrecognised,
        }
    }
}

/// ISO 32000-2's `/UseBlackPtComp`: whether black-point compensation is used.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BlackPointCompensation {
    /// `/ON`.
    On,
    /// `/OFF`.
    Off,
    /// `/Default`, and the value when the key is absent: the device decides.
    #[default]
    Default,
}

/// A transfer function (10.5), as `/TR` or `/TR2` sets it.
#[derive(Clone, Debug, PartialEq)]
pub enum Transfer {
    /// `/Identity`.
    Identity,
    /// One function for every colorant.
    One(Arc<Function>),
    /// One function for each of the four colorants, in the order 10.5 gives.
    Four([Arc<Function>; 4]),
}

/// A type 1 halftone's spot function (10.6.5.2).
#[derive(Clone, Debug, PartialEq)]
pub enum SpotFunction {
    /// A function of two inputs, read as written.
    Function(Arc<Function>),
    /// One of the predefined spot functions by name, or names in order of
    /// preference, as written: the applying row picks the first it knows.
    Names(Vec<Vec<u8>>),
}

/// A halftone (10.6.5), as `/HT` sets it.
///
/// A threshold array is kept by its stream, `thresholds`, packed
/// `num << 16 | gen` as [`crate::Form::stream`] packs one: it is the one value
/// whose size is the file's, and nothing reads it until a render screens with
/// it, so it is decoded there rather than at every `gs`.
#[derive(Clone, Debug, PartialEq)]
pub enum Halftone {
    /// Type 1: a screen of `frequency` lines per inch at `angle` degrees,
    /// shaped by its spot function.
    Screen {
        /// `/Frequency`.
        frequency: f64,
        /// `/Angle`, in degrees.
        angle: f64,
        /// `/SpotFunction`.
        spot: SpotFunction,
        /// `/AccurateScreens`.
        accurate: bool,
        /// `/TransferFunction`.
        transfer: Option<Transfer>,
    },
    /// Type 5: a halftone for each colorant, in the order written, `/Default`
    /// among them.
    Colorants(Vec<(Vec<u8>, Halftone)>),
    /// Type 6: a threshold array of `width` by `height` bytes.
    Threshold {
        /// `/Width`.
        width: i64,
        /// `/Height`.
        height: i64,
        /// The threshold array's stream.
        thresholds: u64,
        /// `/TransferFunction`.
        transfer: Option<Transfer>,
    },
    /// Type 10: thresholds over two squares.
    Squares {
        /// `/Xsquare`, the first square's side.
        xsquare: i64,
        /// `/Ysquare`, the second's.
        ysquare: i64,
        /// The threshold array's stream.
        thresholds: u64,
        /// `/TransferFunction`.
        transfer: Option<Transfer>,
    },
    /// Type 16: 16-bit thresholds over one rectangle, or two.
    Threshold16 {
        /// `/Width`.
        width: i64,
        /// `/Height`.
        height: i64,
        /// `/Width2` and `/Height2`, when the array has a second rectangle.
        second: Option<(i64, i64)>,
        /// The threshold array's stream.
        thresholds: u64,
        /// `/TransferFunction`.
        transfer: Option<Transfer>,
    },
}

/// The graphics state parameters kept for the rows that apply them: set by
/// `gs`, `ri` and `i`, and read by nothing yet, so none of them moves a pixel.
///
/// A plain field starts at the initial value 8.4.1 (Tables 52 and 53) or 9.3.8
/// gives. An `Option` field's `None` is the device's own: what the renderer
/// does with no key, and what `/Default` restores.
#[derive(Clone, Debug, PartialEq)]
pub struct StoredParameters {
    /// `/RI`, `ri`.
    pub rendering_intent: RenderingIntent,
    /// `/UseBlackPtComp`.
    pub black_point_compensation: BlackPointCompensation,
    /// `/OP`.
    pub stroke_overprint: bool,
    /// `/op`, or `/OP` where `/op` is absent.
    pub fill_overprint: bool,
    /// `/OPM`, the integer written.
    pub overprint_mode: i64,
    /// `/AIS`.
    pub alpha_is_shape: bool,
    /// `/TK`.
    pub text_knockout: bool,
    /// `/FL`, `i`: the number written.
    pub flatness: Option<f64>,
    /// `/SM`: the number written.
    pub smoothness: Option<f64>,
    /// `/SA`.
    pub stroke_adjustment: Option<bool>,
    /// `/BG2` over `/BG`: the function, parsed, never sampled — sampling is
    /// the applying row's, at its stage and depth.
    pub black_generation: Option<Arc<Function>>,
    /// `/UCR2` over `/UCR`.
    pub undercolor_removal: Option<Arc<Function>>,
    /// `/TR2` over `/TR`.
    pub transfer: Option<Transfer>,
    /// `/HT`.
    pub halftone: Option<Arc<Halftone>>,
    /// `/HTO`, ISO 32000-2's halftone origin.
    pub halftone_origin: Option<[f64; 2]>,
}

impl StoredParameters {
    /// Every parameter at its initial value.
    pub const INITIAL: StoredParameters = StoredParameters {
        rendering_intent: RenderingIntent::RelativeColorimetric,
        black_point_compensation: BlackPointCompensation::Default,
        stroke_overprint: false,
        fill_overprint: false,
        overprint_mode: 0,
        alpha_is_shape: false,
        // 9.3.8: text knockout is initially true.
        text_knockout: true,
        flatness: None,
        smoothness: None,
        stroke_adjustment: None,
        black_generation: None,
        undercolor_removal: None,
        transfer: None,
        halftone: None,
        halftone_origin: None,
    };
}

/// What [`GraphicsState::stored_parameters`] answers while every parameter has
/// its initial value.
static INITIAL_STORED: StoredParameters = StoredParameters::INITIAL;

/// The graphics state (8.4.1), reduced to what interpretation needs.
#[derive(Clone, Debug, Default, PartialEq)]
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
    /// The non-stroking colour at sixteen bits a channel
    /// (`docs/design/sixteen-bit.md`): the light
    /// [`GraphicsState::fill_color`] is the bytes of, each channel's top byte
    /// that colour's byte and its low byte a refinement of it. Set with the
    /// colour by every operator that sets it, and by every writer of
    /// `fill_color` beside it; a colour whose resources answer only bytes is
    /// those bytes widened ([`Rgb16::widen`]).
    pub fill_wide: Rgb16,
    /// The same for the stroking colour.
    pub stroke_wide: Rgb16,
    /// The non-stroking colour's own DeviceCMYK components, as bytes, when it
    /// was set in DeviceCMYK — `k`, or `sc`/`scn` in a space that resolves to
    /// it — and `None` for a colour set any other way.
    ///
    /// [`GraphicsState::fill_color`] is that colour as light (8.6.4.4), which
    /// is what every device but one wants. The one is a device compositing in
    /// ink: light turned back into ink loses the separation the document
    /// chose — a rich black `1 1 1 1 k` comes back as the pure `K` of the same
    /// colour — so the components travel beside the light rather than being
    /// recovered from it.
    pub fill_ink: Option<[u8; 4]>,
    /// The same for the stroking colour: `K`, `SC`, `SCN`.
    pub stroke_ink: Option<[u8; 4]>,
    /// The device space family the non-stroking colour's values are in
    /// ([`ColorFamily`]), for the rules that ask it: set with the colour by
    /// `g`, `rg` and `k`, by `cs`, and by `sc`/`scn` read by count.
    pub fill_family: ColorFamily,
    /// The same for the stroking colour.
    pub stroke_family: ColorFamily,
    /// What the non-stroking colour paints by colorant, for a device that
    /// composites colorants ([`ColorantTints`]): set with the colour by `k`,
    /// `cs`, `sc` and `scn`, `None` for a colour of a space that names no
    /// colorant and on every other device. One pointer, so a `q` copies one
    /// word more, and shared, so the copy allocates nothing.
    pub fill_colorants: Option<Arc<ColorantTints>>,
    /// The same for the stroking colour: `K`, `CS`, `SC`, `SCN`.
    pub stroke_colorants: Option<Arc<ColorantTints>>,
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
    /// The parameters kept for the rows that apply them; `None` while every
    /// one has its initial value. One pointer wide, so a `q` copies one word
    /// more, and shared, so a copy allocates nothing.
    pub stored: Option<Arc<StoredParameters>>,
    /// What the non-stroking colour was resolved from through the resource
    /// seam, where it was: the colour a rendering intent changing resolves
    /// again (11.7.5.3). `None` for a colour a device operator converted in
    /// place, which no intent changes. Shared, so a `q` copies one word.
    pub fill_source: Option<Arc<ColorSource>>,
    /// The same for the stroking colour.
    pub stroke_source: Option<Arc<ColorSource>>,
}

/// The components a colour was set with and the space they were read in, kept
/// beside the colour so that a rendering intent changing after the colour was
/// set paints it in the intent in force at the paint (11.7.5.3).
#[derive(Clone, Debug, PartialEq)]
pub struct ColorSource {
    /// The device space the components were read in by its name, where a
    /// device operator or a component count selected it in a scope that
    /// remaps it to a default (8.6.5.6); `None` where the space is the one
    /// the slot names, [`GraphicsState::fill_space`] or
    /// [`GraphicsState::stroke_space`].
    pub family: Option<&'static [u8]>,
    /// The components, as the operator gave them.
    pub components: Vec<f64>,
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
            fill_wide: Rgb16::BLACK,
            stroke_wide: Rgb16::BLACK,
            // 8.6.8: the initial space is DeviceGray, so there is no ink, and
            // it names no colorant.
            fill_ink: None,
            stroke_ink: None,
            fill_family: ColorFamily::Gray,
            stroke_family: ColorFamily::Gray,
            fill_colorants: None,
            stroke_colorants: None,
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
            stored: None,
            fill_source: None,
            stroke_source: None,
        }
    }

    /// The stored parameters in force.
    #[must_use]
    pub fn stored_parameters(&self) -> &StoredParameters {
        self.stored.as_deref().unwrap_or(&INITIAL_STORED)
    }

    /// Changes the stored parameters through `change`. The value in force is
    /// kept when nothing changed, so a `gs` repeated before every text object
    /// allocates nothing after the first, and the initial values are stored as
    /// `None`, so two states holding the same parameters compare equal however
    /// they were reached.
    pub fn set_stored(&mut self, change: impl FnOnce(&mut StoredParameters)) {
        let mut next = self.stored_parameters().clone();
        change(&mut next);
        if next == *self.stored_parameters() {
            return;
        }
        self.stored = (next != StoredParameters::INITIAL).then(|| Arc::new(next));
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

    /// The stored parameters cost a state one word, and a set that changes
    /// nothing keeps the value in force.
    #[test]
    fn stored_parameters_cost_a_state_one_word() {
        assert_eq!(
            core::mem::size_of::<Option<Arc<StoredParameters>>>(),
            core::mem::size_of::<usize>()
        );

        // 8.4.1, Tables 52 and 53, and 9.3.8.
        let initial = StoredParameters::INITIAL;
        assert_eq!(
            initial.rendering_intent,
            RenderingIntent::RelativeColorimetric
        );
        assert_eq!(
            initial.black_point_compensation,
            BlackPointCompensation::Default
        );
        assert!(!initial.stroke_overprint && !initial.fill_overprint);
        assert_eq!(initial.overprint_mode, 0);
        assert!(!initial.alpha_is_shape);
        assert!(initial.text_knockout);
        assert_eq!(
            (
                initial.flatness,
                initial.smoothness,
                initial.stroke_adjustment
            ),
            (None, None, None)
        );
        assert!(initial.black_generation.is_none() && initial.undercolor_removal.is_none());
        assert!(initial.transfer.is_none());
        assert!(initial.halftone.is_none() && initial.halftone_origin.is_none());

        let mut state = GraphicsState::new(Matrix::IDENTITY);
        state.set_stored(|s| s.rendering_intent = RenderingIntent::RelativeColorimetric);
        assert!(
            state.stored.is_none(),
            "the initial value is stored as nothing"
        );

        state.set_stored(|s| s.text_knockout = false);
        let set = state
            .stored
            .clone()
            .expect("a value that is not the initial one");
        let saved = state.clone();
        assert!(
            saved.stored.as_ref().is_some_and(|s| Arc::ptr_eq(s, &set)),
            "a copy of the state shares the value"
        );
        state.set_stored(|s| s.text_knockout = false);
        assert!(
            state.stored.as_ref().is_some_and(|s| Arc::ptr_eq(s, &set)),
            "a set that changes nothing keeps the value in force"
        );
        assert!(!state.stored_parameters().text_knockout);

        state.set_stored(|s| s.text_knockout = true);
        assert!(state.stored.is_none(), "back at the initial values");
        assert_eq!(state, GraphicsState::new(Matrix::IDENTITY));
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
