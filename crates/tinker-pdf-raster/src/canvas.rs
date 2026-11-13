//! Pixels and compositing.
//!
//! Colour is stored per pixel in the format the caller asked for, and blending
//! is integer arithmetic throughout — a `u32` intermediate with rounded
//! division, never a float — so a composite is bit-identical everywhere
//! (ruling 4).

use crate::blend::BlendMode;

use crate::fill::Mask;

/// How pixels are stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    /// One byte of grey per pixel.
    Gray8,
    /// Grey and alpha.
    GrayA8,
    /// Red, green and blue.
    Rgb8,
    /// Red, green, blue and alpha.
    Rgba8,
    /// Cyan, magenta, yellow, black and alpha.
    ///
    /// The one **subtractive** format: its components are quantities of ink,
    /// so more of one is darker where more of a `Rgb8` channel is lighter.
    /// 11.3.5's blend formulas are written for additive components, so a
    /// subtractive channel enters and leaves them complemented — see `blend`.
    ///
    /// It exists for transparency groups that declare `/DeviceCMYK` as their
    /// `/Group /CS` (11.6.6), which is a group whose blends the specification
    /// says happen over ink. A page comes back in it only when the caller
    /// asks for it by name and opts in (`RenderOptions::allow_cmyk` in the
    /// facade): see `Page::render`.
    CmykA8,
    /// Lightness, `a`, `b` and alpha.
    ///
    /// The one format whose components are not a quantity of anything: `L*`
    /// runs 0..100 and `a`/`b` roughly -128..127, so unlike every other buffer
    /// here they do not live in 0..1 and cannot be stored as though they did.
    /// They are encoded into bytes — `L/100`, `(a + 128)/255`, `(b + 128)/255`
    /// — which is what lets 11.3.5's separable formulas, written for additive
    /// components in 0..1, apply to them at all.
    ///
    /// **That encoding is a choice and is stated here rather than buried.**
    /// The clause gives no encoding for a Lab group's buffer; blending in the
    /// encoded domain is what makes a `/Lab` group composite in Lab rather
    /// than in RGB, which is the whole point of 11.4.7, and it is not the same
    /// as blending the unencoded values.
    ///
    /// It exists for a group that declares `/Lab` as its `/Group /CS`. It is
    /// not offered as a page format: see `Page::render`.
    LabA8,
    /// Blue, green and red: `Rgb8`'s light in the order a host's surfaces
    /// store it.
    ///
    /// An **output order**. A page asked for in it composites in `Rgb8` and is
    /// reordered once at the end (`Canvas::extract`), so no buffer the
    /// renderer opens holds a second channel order. A canvas made in it
    /// composites right all the same: the separable modes are per channel,
    /// and the non-separable ones (11.3.5.3), which reason about red, green
    /// and blue, are handed the channels in that order — see `blend`.
    Bgr8,
    /// Blue, green, red and alpha: `Rgba8` in a host's order, alpha last as
    /// there. An output order, as [`PixelFormat::Bgr8`] is.
    Bgra8,
    /// Cyan, magenta, yellow, black, then `spots` spot channels, then alpha:
    /// `5 + spots` bytes a pixel.
    ///
    /// The plates a press prints, one channel a colorant: every channel is a
    /// **tint**, a quantity of ink — subtractive, as `CmykA8`'s channels are,
    /// so 11.3.5's blend formulas see each one complemented — and the four
    /// process channels with no spot are byte for byte a `CmykA8` pixel. A
    /// paint is a [`PlateInk`], a tint for each channel by its index, and the
    /// channels it names no ink for are knocked out under `Normal`; the spot
    /// channels mean nothing to this crate beyond their index (ruling 8),
    /// and which colorant each is, is the caller's plan. A canvas carries the
    /// spots it is asked for, at most [`MAX_SPOT_PLATES`] ([`Spots`]).
    ///
    /// It exists for separations and overprint simulation, which draw a page
    /// as its inks. It is not a page format: a plate is ink, and a `Bitmap`
    /// says nothing about what its channels mean.
    Plates {
        /// How many spot channels.
        spots: Spots,
    },
}

/// The most spot channels a [`PixelFormat::Plates`] canvas carries beside its
/// four process channels: sixty-eight colour channels and an alpha, sixty-nine
/// bytes a pixel.
pub const MAX_SPOT_PLATES: usize = 64;

/// How many spot channels a [`PixelFormat::Plates`] canvas carries: at most
/// [`MAX_SPOT_PLATES`], which [`Spots::new`] holds, so a canvas past the cap
/// cannot be asked for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Spots(u8);

impl Spots {
    /// No spot channel: the four process channels and an alpha, the bytes of
    /// a [`PixelFormat::CmykA8`] pixel.
    pub const NONE: Spots = Spots(0);

    /// `count` spot channels, or [`TooManySpots`] past [`MAX_SPOT_PLATES`].
    ///
    /// # Errors
    ///
    /// [`TooManySpots`] when `count` is more than [`MAX_SPOT_PLATES`].
    pub const fn new(count: usize) -> Result<Spots, TooManySpots> {
        if count > MAX_SPOT_PLATES {
            return Err(TooManySpots { asked: count });
        }
        // At most the cap, sixty-four, which a byte holds.
        Ok(Spots(count as u8))
    }

    /// How many spot channels.
    #[must_use]
    pub const fn count(self) -> usize {
        self.0 as usize
    }
}

/// A plates canvas asked for with more spot channels than
/// [`MAX_SPOT_PLATES`]: refused by name rather than clamped, because which
/// spots to keep is the caller's plan and not this crate's to choose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TooManySpots {
    /// How many spot channels were asked for.
    pub asked: usize,
}

impl core::fmt::Display for TooManySpots {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "a plates canvas of {} spot channels, past the cap of {MAX_SPOT_PLATES}",
            self.asked
        )
    }
}

impl std::error::Error for TooManySpots {}

/// A paint's ink on a [`PixelFormat::Plates`] canvas: a tint for each colour
/// channel — cyan 0, magenta 1, yellow 2, black 3 and spot `i` at `4 + i` —
/// where 0 is no ink and 255 solid.
///
/// Built from the channels a paint names, every channel it does not name
/// holding no ink: composited under `Normal`, the paint knocks those channels
/// out, which is what 8.6.7 has a paint do with overprint off. A colorant
/// arrives here as its channel's index and never as a name (ruling 8); which
/// colorant a channel is, is the caller's plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlateInk {
    /// One tint per colour channel, process channels first.
    tints: Vec<u8>,
}

impl PlateInk {
    /// No ink in any channel of a canvas with `spots` spot channels.
    #[must_use]
    pub fn none(spots: Spots) -> PlateInk {
        PlateInk {
            tints: vec![0; 4 + spots.count()],
        }
    }

    /// The tint each `(channel, tint)` pair gives its channel, and no ink in
    /// every channel no pair names. A channel past the canvas's last is not
    /// one of its channels and is passed over; a channel named twice keeps
    /// the later tint.
    #[must_use]
    pub fn named(spots: Spots, channels: &[(usize, u8)]) -> PlateInk {
        let mut ink = PlateInk::none(spots);
        for &(channel, tint) in channels {
            if let Some(slot) = ink.tints.get_mut(channel) {
                *slot = tint;
            }
        }
        ink
    }

    /// A DeviceCMYK colour's own components on the four process channels,
    /// and no ink on any spot: what a `CmykA8` canvas composites for the
    /// same colour.
    #[must_use]
    pub fn process(spots: Spots, [c, m, y, k]: [u8; 4]) -> PlateInk {
        PlateInk::named(spots, &[(0, c), (1, m), (2, y), (3, k)])
    }

    /// Light turned into ink on the process channels — 8.6.4.4's relation
    /// inverted with maximum undercolour removal ([`rgb_to_cmyk`]), as a
    /// `CmykA8` canvas turns light into ink — and no ink on any spot.
    #[must_use]
    pub fn from_light(spots: Spots, color: Color) -> PlateInk {
        let (c, m, y, k) = rgb_to_cmyk(color.r, color.g, color.b);
        PlateInk::process(spots, [c, m, y, k])
    }

    /// The tints, one a colour channel, process channels first.
    #[must_use]
    pub fn tints(&self) -> &[u8] {
        &self.tints
    }

    /// The four process tints.
    fn process_tints(&self) -> [u8; 4] {
        let at = |i: usize| self.tints.get(i).copied().unwrap_or(0);
        [at(0), at(1), at(2), at(3)]
    }
}

impl PixelFormat {
    /// Bytes per pixel.
    #[must_use]
    pub fn components(self) -> usize {
        match self {
            PixelFormat::Gray8 => 1,
            PixelFormat::GrayA8 => 2,
            PixelFormat::Rgb8 => 3,
            PixelFormat::Rgba8 => 4,
            PixelFormat::CmykA8 => 5,
            PixelFormat::LabA8 => 4,
            PixelFormat::Bgr8 => 3,
            PixelFormat::Bgra8 => 4,
            PixelFormat::Plates { spots } => 5 + spots.count(),
        }
    }

    /// Whether the format carries alpha.
    #[must_use]
    pub fn has_alpha(self) -> bool {
        // A `match` rather than the `matches!` this was, because `matches!` is
        // not exhaustiveness-checked and this is the worst place in the crate
        // to forget a format. A variant missing here does not fail to compile
        // and does not panic: `blend` stops writing the alpha byte,
        // `alpha_at` answers 255 for every pixel, and `recolor` and
        // `remove_backdrop` become silent no-ops — an uncoloured pattern
        // painting its own colours, and a non-isolated group counting its
        // backdrop twice. Four wrong pictures, no error.
        match self {
            PixelFormat::GrayA8
            | PixelFormat::Rgba8
            | PixelFormat::CmykA8
            | PixelFormat::LabA8
            | PixelFormat::Bgra8
            | PixelFormat::Plates { .. } => true,
            PixelFormat::Gray8 | PixelFormat::Rgb8 | PixelFormat::Bgr8 => false,
        }
    }

    /// Whether the three colour channels are stored blue first: where a
    /// reader of red, green and blue finds each.
    ///
    /// A `match` for `has_alpha`'s reason: a format added later must say
    /// which order it stores, or `blend` hands a non-separable mode its
    /// channels in the wrong one.
    #[must_use]
    pub const fn is_blue_first(self) -> bool {
        match self {
            PixelFormat::Bgr8 | PixelFormat::Bgra8 => true,
            PixelFormat::Gray8
            | PixelFormat::GrayA8
            | PixelFormat::Rgb8
            | PixelFormat::Rgba8
            | PixelFormat::CmykA8
            | PixelFormat::LabA8
            | PixelFormat::Plates { .. } => false,
        }
    }
}

/// A colour in 8-bit sRGB with alpha.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
    /// Alpha, where 255 is opaque.
    pub a: u8,
}

impl Color {
    /// An opaque colour.
    #[must_use]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color { r, g, b, a: 255 }
    }

    /// Opaque black.
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    /// Opaque white.
    pub const WHITE: Color = Color::rgb(255, 255, 255);
    /// Nothing at all: the background a group or tile buffer starts from.
    pub const TRANSPARENT: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };

    /// 11.6.5.2's luminosity, which is 11.3.5.3's `Lum`.
    ///
    /// The specification's own 0.3 / 0.59 / 0.11, not the Rec.601 weights
    /// [`Color::luma`] carries. `blend.rs` already records why the two are kept
    /// apart — they differ in the third digit, "which is invisible on any one
    /// pixel and is the difference between matching a reference renderer and
    /// not" — and a soft mask asks the same question `Lum` does, so it gets the
    /// same answer. That it did not was a defect: `MaskKind`'s own doc cites
    /// 11.6.5.2 by clause number and the code reached for the other weighting.
    ///
    /// `luma` stays where it is. It answers a different question — what a
    /// *grey buffer* stores a colour as — and its weights sum to exactly 1000,
    /// which is what makes a grey round-trip through it unchanged.
    #[must_use]
    pub fn luminosity(self) -> u8 {
        luminosity(self.r, self.g, self.b)
    }

    /// The grey this colour reads as, by the usual luma weights.
    #[must_use]
    pub fn luma(self) -> u8 {
        // Integer weights summing to 1000, rounded: no float, no drift.
        let value =
            (u32::from(self.r) * 299 + u32::from(self.g) * 587 + u32::from(self.b) * 114 + 500)
                / 1000;
        value.min(255) as u8
    }
}

/// Black generation and undercolour removal (ISO 32000-2 10.4.2.4), as two
/// tables over the nominal black `k`, `0..=255`: the black ink, and the
/// amount taken from each of the other three, which may be negative (Table
/// 57: undercolour removal "maps the interval [0.0 1.0] to the interval
/// [−1.0 1.0]").
///
/// What an ink canvas turns light into ink by while a caller holds it in
/// force ([`Canvas::set_ink_tables`]); with none, the canvas takes maximum
/// undercolour removal ([`rgb_to_cmyk`]). Plain numbers: the functions that
/// fill them are the caller's (ruling 8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InkTables {
    /// `BG(k)`, the black ink, at each nominal black.
    pub black: [u8; 256],
    /// `UCR(k)`, what each of cyan, magenta and yellow loses, at each
    /// nominal black.
    pub removal: [i16; 256],
}

impl InkTables {
    /// 10.4.2.4's conversion of one light: `c = 1 − red` and its siblings,
    /// `k = min(c, m, y)`, each ink `min(1, max(0, c − UCR(k)))` and the black
    /// `min(1, max(0, BG(k)))`, in bytes.
    #[must_use]
    pub fn ink(&self, r: u8, g: u8, b: u8) -> (u8, u8, u8, u8) {
        let k = 255 - usize::from(r.max(g).max(b));
        let take = i32::from(self.removal.get(k).copied().unwrap_or(0));
        let ink = |v: u8| (255 - i32::from(v) - take).clamp(0, 255) as u8;
        (
            ink(r),
            ink(g),
            ink(b),
            self.black.get(k).copied().unwrap_or(0),
        )
    }
}

/// [`Color::luminosity`] of three bytes: 0.3, 0.59 and 0.11 as integer
/// weights summing to 1 000, rounded.
fn luminosity(r: u8, g: u8, b: u8) -> u8 {
    let value = (u32::from(r) * 300 + u32::from(g) * 590 + u32::from(b) * 110 + 500) / 1000;
    value.min(255) as u8
}

/// What a rendered buffer is read as when it becomes a mask (11.6.5.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaskKind {
    /// The luminosity of the composited colour.
    Luminosity,
    /// The accumulated alpha, ignoring colour.
    Alpha,
}

/// Rows of a composite between one cancellation check and the next.
///
/// The same reasoning as the filler's `STOP_EVERY`: a branch per row is
/// nothing beside a row's worth of blending, and the predicate decides only
/// whether the walk continues, never what a continued row computes.
const STOP_EVERY: u32 = 16;

/// A rectangular grid of pixels.
#[derive(Clone, Debug)]
pub struct Canvas {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Storage format.
    pub format: PixelFormat,
    /// Bytes per row.
    pub stride: usize,
    /// The pixels.
    pub data: Vec<u8>,
    /// The device pixel this canvas's top-left is, as `(x, y)`.
    ///
    /// **Ruling 5's lattice.** A canvas that is a region of a page — a tile, or
    /// a transparency group's bounding box — used to be drawn in a frame of its
    /// own: the page's transform with a whole number of pixels taken off `e`
    /// and `f`. That is exact as arithmetic and not as floating point,
    /// `fl(u + e)` and `fl(u + e − tx)` being two roundings at two magnitudes,
    /// and the last ulp reached a byte wherever the exact value sat on a grid
    /// the rasterizer quantises to. So a region now keeps the page's frame and
    /// says where in it it is: every mask, every sampled coordinate and every
    /// `at` this crate is handed is in **device** pixels, and only the index
    /// into [`Canvas::data`] subtracts the origin — an integer subtraction,
    /// which cannot round. `(0, 0)` for a canvas that is the whole page, which
    /// is every canvas that existed before this field did.
    origin: (i32, i32),
    /// The canvas a transparency group's buffer will be composited onto, held
    /// by the buffer while the group is open (11.4.4).
    ///
    /// **A move, never a copy.** While a group is open nothing paints into the
    /// canvas beneath it — every paint goes to the group's buffer, and a
    /// nested group's buffer stands in for that one in turn — so the parent's
    /// own pixels are the backdrop the group reads, at the same device pixels,
    /// for as long as it is open. [`Canvas::release_parent`] hands it back.
    ///
    /// `None` on every canvas that is not an open group's buffer.
    parent: Option<Box<Canvas>>,
    /// Whether `parent` is this buffer's initial backdrop: a *non-isolated*
    /// group's (11.4.4), which reads it.
    ///
    /// A non-isolated group starts with its backdrop composited in, so that a
    /// blend mode inside the group can see through to what was underneath.
    /// That leaves the buffer holding two different quantities at once: the
    /// colour channels carry the group **over** its backdrop, while the alpha
    /// channel must carry the group's *own* accumulated alpha, because
    /// 11.4.7.2's removal step is `C = Cn + (Cn - C0)·(a0/agn - a0)` and `agn`
    /// is that own alpha. The two are not recoverable from one another once
    /// the backdrop is opaque — `agn` divides out — so the backdrop is read
    /// again where it lies: `a0` is the parent's alpha at each composited
    /// pixel ([`Canvas::backdrop_alpha`]), and `C0` its colour, in this
    /// buffer's format, at adoption and at removal.
    ///
    /// False on every ordinary canvas and on every isolated group, which
    /// holds its parent and reads nothing of it: its backdrop's alpha is zero
    /// and all of the arithmetic below collapses to what it would be with
    /// none. It stays set once the parent is released, until
    /// [`Canvas::remove_backdrop`] has taken the backdrop out.
    backdrop: bool,
    /// Whether a non-separable blend mode has been applied over a subtractive
    /// buffer, which 11.3.5.3 cannot do without a round trip through light.
    ///
    /// A counter rather than nothing at all, because the round trip is a
    /// *leniency* and ruling 10 says a leniency is named. `rgb_to_cmyk`
    /// produces one particular ink split, so a rich black arrives back as its
    /// pure-K equivalent: the colour is the same and the next blend over it is
    /// not. The comment beside `blend` claimed this was reported and it was
    /// not, which is why the flag exists on the buffer that can see it —
    /// nothing lower down knows what a warning is (ruling 8).
    approximated_blends: u32,
    /// The black generation and undercolour removal a `CmykA8` canvas turns
    /// light into ink by, while a caller holds them in force; `None` is
    /// maximum undercolour removal. Every other format never reads it.
    ink_tables: Option<std::sync::Arc<InkTables>>,
}

impl Canvas {
    /// A canvas filled with `background`.
    ///
    /// A format without alpha must start opaque — a transparent tile is what
    /// a caller sees when a clipped render forgets this, and it is invisible
    /// until someone composites the tile over something dark.
    #[must_use]
    pub fn new(width: u32, height: u32, format: PixelFormat, background: Color) -> Canvas {
        let components = format.components();
        let stride = (width as usize).saturating_mul(components);
        let len = stride.saturating_mul(height as usize);

        let mut canvas = Canvas {
            width,
            height,
            format,
            stride,
            data: vec![0; len],
            origin: (0, 0),
            parent: None,
            backdrop: false,
            approximated_blends: 0,
            ink_tables: None,
        };
        canvas.clear(background);
        canvas
    }

    /// A [`PixelFormat::Plates`] canvas of `spots` spot channels filled with
    /// `background`, as [`Canvas::new`] fills one: its light turned into ink
    /// on the process channels and no ink on any spot. A canvas of `spots` is
    /// allocated for those spots alone, never for the cap.
    ///
    /// # Errors
    ///
    /// [`TooManySpots`] for more spot channels than [`MAX_SPOT_PLATES`].
    pub fn plates(
        width: u32,
        height: u32,
        spots: usize,
        background: Color,
    ) -> Result<Canvas, TooManySpots> {
        let spots = Spots::new(spots)?;
        Ok(Canvas::new(
            width,
            height,
            PixelFormat::Plates { spots },
            background,
        ))
    }

    /// One colour channel's bytes, row by row, `width × height` of them: on a
    /// plates canvas one plate's tints — cyan 0 to black 3, spot `i` at
    /// `4 + i` — 0 no ink and 255 solid, and on any other format that
    /// channel's stored component. `None` for a channel the format does not
    /// carry, its alpha among them.
    ///
    /// A plate is a one-channel image of ink, not of light: read as grey it
    /// is its own negative.
    #[must_use]
    pub fn channel(&self, index: usize) -> Option<Vec<u8>> {
        if index >= color_channels(self.format) {
            return None;
        }
        let components = self.format.components();
        let mut out =
            Vec::with_capacity((self.width as usize).saturating_mul(self.height as usize));
        for row in 0..self.height as usize {
            for col in 0..self.width as usize {
                let at = row * self.stride + col * components + index;
                out.push(self.data.get(at).copied().unwrap_or(0));
            }
        }
        Some(out)
    }

    /// The same canvas, standing at device pixel `(x, y)` rather than at the
    /// origin.
    ///
    /// What a region of a page is: the page's own frame, with this canvas
    /// holding the `width` by `height` pixels whose top-left is `(x, y)`. See
    /// the field's documentation for why a region is not a frame of its own.
    #[must_use]
    pub fn at_origin(mut self, x: i32, y: i32) -> Canvas {
        self.origin = (x, y);
        self
    }

    /// The device pixel this canvas's top-left is.
    #[must_use]
    pub const fn origin(&self) -> (i32, i32) {
        self.origin
    }

    /// The device pixels this canvas holds, as `(x0, y0, width, height)`.
    #[must_use]
    pub const fn device_rect(&self) -> (i32, i32, u32, u32) {
        (self.origin.0, self.origin.1, self.width, self.height)
    }

    /// The canvas pixel holding device pixel `(x, y)`, or `None` off it.
    #[must_use]
    pub fn local(&self, x: i32, y: i32) -> Option<(u32, u32)> {
        let col = i64::from(x) - i64::from(self.origin.0);
        let row = i64::from(y) - i64::from(self.origin.1);
        if col < 0 || row < 0 || col >= i64::from(self.width) || row >= i64::from(self.height) {
            return None;
        }
        Some((col as u32, row as u32))
    }

    /// Repaints every pixel.
    ///
    /// A plates canvas takes `color` as `CmykA8` does — its light turned into
    /// ink on the process channels — with no ink on any spot.
    pub fn clear(&mut self, color: Color) {
        if let PixelFormat::Plates { spots } = self.format {
            let word = plate_word(color, spots, self.ink_tables.as_deref());
            for chunk in self.data.chunks_exact_mut(word.len()) {
                chunk.copy_from_slice(&word);
            }
            return;
        }
        let pixel = self.encode(color);
        for chunk in self.data.chunks_exact_mut(self.format.components()) {
            chunk.copy_from_slice(&pixel[..chunk.len()]);
        }
    }

    /// One pixel of `src`, in *this* canvas's storage, with its own alpha.
    ///
    /// When the two canvases store pixels the same way this reads the bytes
    /// straight across; otherwise it goes through [`Canvas::pixel`] and
    /// [`Canvas::encode`], which is what every cross-format copy has always
    /// done.
    ///
    /// # Why the same-format path is correctness and not speed
    ///
    /// For the additive formats the two paths produce the same bytes —
    /// `Gray8`'s round trip survives because `luma` of a replicated grey is
    /// that grey exactly, its weights summing to 1000. A buffer holding
    /// *subtractive* components has no lossless trip through an RGB `Color`:
    /// the relation is exact in one direction and a projection in the other,
    /// and an ink buffer holds ink the projection never makes — a DeviceCMYK
    /// colour reaches it as the document's own components, so a rich black is
    /// all four inks there and pure K after the trip. A group nested inside
    /// another group of its own kind would have its ink re-derived at every
    /// composite — a picture, and a different one. Same-format copies must not
    /// go through `Color`, and the cheapest way to guarantee that is for them
    /// never to; `adopt_backdrop` and `remove_backdrop` read through here for
    /// that reason.
    ///
    /// `a_same_format_copy_is_the_bytes_it_started_as` pins the equivalence.
    /// A pixel off `src` is `None` either way: the same-format read would
    /// otherwise take a column past the right edge from the next row.
    fn source_from(&self, src: &Canvas, sx: u32, sy: u32) -> Option<([u8; 5], u32)> {
        if sx >= src.width || sy >= src.height {
            return None;
        }
        if src.format == self.format {
            let components = src.format.components();
            let base = (sy as usize) * src.stride + (sx as usize) * components;
            let raw = src.data.get(base..base + components)?;
            let mut word = [0u8; 5];
            word.get_mut(..components)?.copy_from_slice(raw);
            return Some((word, src.alpha_at(sx, sy)));
        }
        let color = src.pixel(sx, sy)?;
        Some((self.encode(color), u32::from(color.a)))
    }

    /// A colour, in the bytes this canvas stores per pixel.
    ///
    /// Five wide rather than four, which is one more than any format uses
    /// today. The width is the widest a format *will* need — four colour
    /// channels and an alpha — and sizing it here rather than at the first
    /// four-channel format keeps that change to the arms of this match. Every
    /// consumer already slices to `components()`, so the spare byte is never
    /// read and never reaches `data`.
    fn encode(&self, color: Color) -> [u8; 5] {
        self.encode_by(color, self.ink_tables.as_deref())
    }

    /// [`Canvas::encode`] with `tables` the black generation and undercolour
    /// removal an ink format turns light into ink by, `None` for maximum
    /// removal.
    fn encode_by(&self, color: Color, tables: Option<&InkTables>) -> [u8; 5] {
        encode_in(self.format, color, tables)
    }

    /// Puts black generation and undercolour removal in force for the light
    /// this canvas turns into ink from now on, or maximum removal again for
    /// `None`, and hands back what was in force.
    ///
    /// Read only by an ink canvas — `CmykA8`, and a plates canvas's process
    /// channels — wherever it encodes light: a colour without ink of its own,
    /// an image's sample, a shading's pixel, a buffer of another format
    /// composited onto it.
    pub fn set_ink_tables(
        &mut self,
        tables: Option<std::sync::Arc<InkTables>>,
    ) -> Option<std::sync::Arc<InkTables>> {
        std::mem::replace(&mut self.ink_tables, tables)
    }

    /// Composites `color` through `mask` onto the canvas.
    ///
    /// `alpha` scales the whole operation, which is what the graphics state's
    /// `ca` and `CA` do (8.6.4.4).
    pub fn fill_mask(&mut self, mask: &Mask, color: Color, alpha: f64) {
        self.fill_mask_with(mask, color, alpha, BlendMode::Normal);
    }

    /// As [`Canvas::fill_mask`], with a blend mode (11.3.5).
    pub fn fill_mask_with(&mut self, mask: &Mask, color: Color, alpha: f64, mode: BlendMode) {
        self.fill_mask_inked(mask, color, None, alpha, mode);
    }

    /// As [`Canvas::fill_mask_with`], with the colour's own ink beside it.
    ///
    /// `ink` is cyan, magenta, yellow and black as bytes, the components the
    /// colour was chosen in, and a [`PixelFormat::CmykA8`] canvas composites
    /// **those** rather than `color` turned back into ink. The two are not the
    /// same: `color` is light, and light has one ink for each colour — maximum
    /// undercolour removal, so a rich black of all four inks comes back as
    /// black ink alone. Every other format composites `color` and never reads
    /// `ink`, and so does this one when `ink` is `None`.
    pub fn fill_mask_inked(
        &mut self,
        mask: &Mask,
        color: Color,
        ink: Option<[u8; 4]>,
        alpha: f64,
        mode: BlendMode,
    ) {
        let alpha = if alpha.is_finite() {
            operation_level(alpha)
        } else {
            255
        };
        if alpha == 0 {
            return;
        }

        // A plates canvas composites the colour's ink on its process
        // channels, as `CmykA8` does — the colour's own components where it
        // has them, its light turned into ink where it has not — and knocks
        // out every spot channel.
        if let PixelFormat::Plates { spots } = self.format {
            let ink = match ink {
                Some(ink) => PlateInk::process(spots, ink),
                None => {
                    let (c, m, y, k) = light_ink(color, self.ink_tables.as_deref());
                    PlateInk::process(spots, [c, m, y, k])
                }
            };
            self.fill_plates(mask, ink.tints(), u32::from(color.a), alpha, mode);
            return;
        }
        if mode.is_nonseparable() && self.format == PixelFormat::CmykA8 {
            self.approximated_blends = self.approximated_blends.saturating_add(1);
        }
        let source = match (ink, self.format) {
            (Some([c, m, y, k]), PixelFormat::CmykA8) => [c, m, y, k, color.a],
            _ => self.encode(color),
        };
        let color_alpha = u32::from(color.a);

        // The mask's own rectangle, not the canvas. Outside it `Mask::at`
        // returns zero and every pixel is skipped, so walking the page was
        // always the same answer at the price of the page: a comma on A4 at
        // 300 dpi visited 8.4 million pixels to composite about two hundred.
        // A mask reading a value outside itself reaches the whole canvas, and
        // only `at` reads that value, so it never takes the opaque path, which
        // slices the mask's own rows.
        let rect = mask.overlap_at(self.origin, self.width, self.height);
        if mode == BlendMode::Normal
            && color_alpha == 255
            && alpha == 255
            && mask.outside == 0
            && !self.backdrop
            && self.format != PixelFormat::CmykA8
        {
            self.fill_opaque(mask, &source, rect);
        } else {
            self.fill_blended(mask, &source, color_alpha, alpha, mode, rect);
        }
    }

    /// Composites `ink` through `mask` onto a [`PixelFormat::Plates`] canvas,
    /// at `alpha` under `mode` (11.3.5, 11.3.6): each colour channel its own
    /// tint, a channel the ink names no tint for taking no ink, so under
    /// `Normal` the paint knocks it out.
    ///
    /// The arithmetic is `CmykA8`'s, channel by channel: a tint enters and
    /// leaves the blend function complemented, the composite is 11.3.6's
    /// over straight alpha, and the coverage and `alpha` multiply as they do
    /// for an opaque colour there. So the four process channels of a plates
    /// canvas come out of the same masks, inks, alphas and modes byte for
    /// byte as a `CmykA8` canvas's do.
    ///
    /// On a canvas of any other format the ink's process tints are painted as
    /// the DeviceCMYK colour they are — that colour's own ink on `CmykA8`,
    /// its light everywhere else — and its spots, which no other format
    /// carries, are not.
    pub fn fill_mask_plates(&mut self, mask: &Mask, ink: &PlateInk, alpha: f64, mode: BlendMode) {
        if !matches!(self.format, PixelFormat::Plates { .. }) {
            let [c, m, y, k] = ink.process_tints();
            let (r, g, b) = cmyk_to_rgb(c, m, y, k);
            self.fill_mask_inked(mask, Color::rgb(r, g, b), Some([c, m, y, k]), alpha, mode);
            return;
        }
        let alpha = if alpha.is_finite() {
            operation_level(alpha)
        } else {
            255
        };
        if alpha == 0 {
            return;
        }
        self.fill_plates(mask, ink.tints(), 255, alpha, mode);
    }

    /// The plates composite of [`Canvas::fill_mask_plates`] and of
    /// [`Canvas::fill_mask_inked`] on a plates canvas: every pixel the mask
    /// covers through [`blend_plates`], at the coverage, the colour's own
    /// alpha and the operation's `alpha` level multiplied as
    /// [`Canvas::fill_mask_inked`]'s general composite multiplies them.
    fn fill_plates(
        &mut self,
        mask: &Mask,
        tints: &[u8],
        color_alpha: u32,
        alpha: u32,
        mode: BlendMode,
    ) {
        if mode.is_nonseparable() {
            self.approximated_blends = self.approximated_blends.saturating_add(1);
        }
        let channels = color_channels(self.format);
        let components = self.format.components();
        // The source as wide as this canvas's colour channels: the ink's own
        // tint in each channel it has, and no ink in each it has not.
        let source: Vec<u8> = (0..channels)
            .map(|channel| tints.get(channel).copied().unwrap_or(0))
            .collect();
        let (x0, y0, x1, y1) = mask.overlap_at(self.origin, self.width, self.height);
        let (ox, oy) = self.origin;
        for row in y0..y1 {
            for col in x0..x1 {
                let coverage = u32::from(
                    mask.at(ox.saturating_add(col as i32), oy.saturating_add(row as i32)),
                );
                if coverage == 0 {
                    continue;
                }
                let effective = mul255(mul255(coverage, color_alpha), alpha);
                if effective == 0 {
                    continue;
                }
                let base = (row as usize) * self.stride + (col as usize) * components;
                let backdrop = self.backdrop_alpha(col, row);
                if let Some(pixel) = self.data.get_mut(base..base + components) {
                    blend_plates(pixel, &source, effective, channels, mode, backdrop);
                }
            }
        }
    }

    /// [`Canvas::fill_mask_inked`]'s general composite: every pixel the mask
    /// covers through `blend`, at the coverage, the colour's alpha and the
    /// state's alpha multiplied, over the canvas pixels `(x0, y0, x1, y1)` —
    /// but for a pixel those multiply to 255 under `Normal`, which takes
    /// [`opaque_normal`], what `blend` writes there whatever the pixel, its
    /// backdrop or the format: a group's buffer, one with a backdrop and an ink
    /// one, the document's ink kept, alike.
    fn fill_blended(
        &mut self,
        mask: &Mask,
        source: &[u8; 5],
        color_alpha: u32,
        alpha: u32,
        mode: BlendMode,
        (x0, y0, x1, y1): (u32, u32, u32, u32),
    ) {
        let components = self.format.components();
        let (ox, oy) = self.origin;
        for row in y0..y1 {
            for col in x0..x1 {
                let coverage = u32::from(
                    mask.at(ox.saturating_add(col as i32), oy.saturating_add(row as i32)),
                );
                if coverage == 0 {
                    continue;
                }
                // Coverage, the colour's own alpha, and the state's alpha all
                // multiply; rounding at each step keeps 255 mapping to 255.
                let effective = mul255(mul255(coverage, color_alpha), alpha);
                if effective == 0 {
                    continue;
                }

                let base = (row as usize) * self.stride + (col as usize) * components;
                if effective == 255 && mode == BlendMode::Normal {
                    if let Some(pixel) = self.data.get_mut(base..base + components) {
                        opaque_normal(pixel, source, self.format);
                    }
                    continue;
                }
                let backdrop = self.backdrop_alpha(col, row);
                blend(
                    self.data.get_mut(base..base + components),
                    source,
                    effective,
                    self.format,
                    mode,
                    backdrop,
                );
            }
        }
    }

    /// [`Canvas::fill_mask_inked`] for an opaque colour, at an opaque state
    /// alpha, under `Normal`, onto a canvas that holds no initial backdrop and
    /// is not ink: the mask's rows read as slices over the canvas pixels
    /// `(x0, y0, x1, y1)`.
    ///
    /// **Over an opaque pixel `blend` collapses**, and this is that collapse
    /// and nothing else. A pixel is opaque in a format without alpha, and in
    /// one with it where its alpha byte is 255. There the effective alpha is
    /// the coverage `c` — `mul255(x, 255)` is `x` — the backdrop's weight is
    /// whole, Normal's mix is the source, and each colour channel comes out
    /// `min(mul255(cs, c) + mul255(cb, 255 − c), 255)`, the alpha staying 255
    /// ([`over_opaque`]). **At full coverage `blend` collapses over any
    /// pixel**: the colour and an alpha of 255, whatever the pixel held
    /// ([`opaque_normal`]). A pixel that is not opaque under any other
    /// coverage goes through `blend`, whose rounding it needs.
    ///
    /// **A row is walked in blocks of [`BLOCK`] pixels** ([`opaque_row`]),
    /// and the loops over a block's bytes are of a fixed width with no call
    /// inside, written so that the `+simd128` build vectorises them: a block
    /// of no coverage is passed over, a block of full coverage is one slice
    /// copy of the encoded colour whatever its pixels held, any other block
    /// over opaque pixels takes the collapse at every byte ([`opaque_block`]),
    /// and a block holding a pixel that is not opaque, like the pixels past
    /// the last whole block, is taken a pixel at a time ([`opaque_pixel`]) —
    /// `blend` for each pixel partly covered that is not opaque, and for no
    /// other.
    fn fill_opaque(
        &mut self,
        mask: &Mask,
        source: &[u8; 5],
        (x0, y0, x1, y1): (u32, u32, u32, u32),
    ) {
        let format = self.format;
        let components = format.components();
        let width = x1.saturating_sub(x0) as usize;
        let stride = self.stride;
        let Some(colour) = source.get(..components) else {
            return;
        };
        // A block's worth of the encoded colour, which a block of full
        // coverage is copied from and the collapse reads its source bytes from.
        let mut solid = [0u8; BLOCK * 5];
        for pixel in solid.chunks_exact_mut(components) {
            pixel.copy_from_slice(colour);
        }
        let Some(solid) = solid.get(..BLOCK * components) else {
            return;
        };

        // The mask pixel under canvas pixel `(x0, y0)`. The rectangle is the
        // mask's own, clamped to the canvas, so neither is negative.
        let column = i64::from(self.origin.0) + i64::from(x0) - i64::from(mask.x0);
        let line = i64::from(self.origin.1) + i64::from(y0) - i64::from(mask.y0);
        let (Ok(column), Ok(line)) = (usize::try_from(column), usize::try_from(line)) else {
            return;
        };
        // One instance of the row walk for each pixel size a format of this
        // path has, so that every loop inside it is of a fixed width.
        let row_of: fn(&mut [u8], &[u8], &[u8; 5], &[u8], PixelFormat) = match format {
            PixelFormat::Gray8 => opaque_row::<1, false>,
            PixelFormat::GrayA8 => opaque_row::<2, true>,
            PixelFormat::Rgb8 | PixelFormat::Bgr8 => opaque_row::<3, false>,
            PixelFormat::Rgba8 | PixelFormat::Bgra8 | PixelFormat::LabA8 => opaque_row::<4, true>,
            PixelFormat::CmykA8 => opaque_row::<5, true>,
            // Plates are not a format of this path: `fill_mask_inked` hands
            // them to `fill_plates` before it is chosen.
            PixelFormat::Plates { .. } => return,
        };
        for (index, row) in (y0..y1).enumerate() {
            let start = (line + index) * (mask.width as usize) + column;
            let Some(coverage) = mask.data.get(start..start + width) else {
                continue;
            };
            let base = (row as usize) * stride + (x0 as usize) * components;
            let Some(pixels) = self.data.get_mut(base..base + width * components) else {
                continue;
            };
            row_of(pixels, coverage, source, solid, format);
        }
    }

    /// Composites one pixel, for callers that sample rather than rasterize.
    ///
    /// An image is drawn by mapping device pixels back into its samples, so
    /// there is no coverage mask to go through — each pixel is decided
    /// individually and blended here.
    /// How many non-separable blends over a subtractive buffer this canvas has
    /// performed, and therefore how many went through 11.3.5.3's round trip.
    ///
    /// Zero on every additive buffer and on every canvas that never used one of
    /// the four modes. A caller turns a non-zero count into the warning ruling
    /// 10 asks for; nothing here knows what a warning is (ruling 8).
    #[must_use]
    pub const fn approximated_blends(&self) -> u32 {
        self.approximated_blends
    }

    pub fn blend_pixel(&mut self, x: u32, y: u32, color: Color, alpha: f64) {
        self.blend_pixel_with(x, y, color, alpha, BlendMode::Normal);
    }

    /// As [`Canvas::blend_pixel`], with a blend mode (11.3.5).
    pub fn blend_pixel_with(&mut self, x: u32, y: u32, color: Color, alpha: f64, mode: BlendMode) {
        self.blend_pixel_inked(x, y, color, None, alpha, mode);
    }

    /// As [`Canvas::blend_pixel_with`], with the colour's own ink beside it,
    /// which a [`PixelFormat::CmykA8`] canvas composites in place of `color`
    /// turned back into ink — what [`Canvas::fill_mask_inked`] does for a
    /// mask, for a caller that samples. Every other format never reads `ink`.
    ///
    /// `alpha` becomes a level as `round(clamp(alpha, 0, 1) × 255)`, by
    /// [`nearest_level`] rather than a call to `round`; a non-finite alpha
    /// composites nothing.
    pub fn blend_pixel_inked(
        &mut self,
        x: u32,
        y: u32,
        color: Color,
        ink: Option<[u8; 4]>,
        alpha: f64,
        mode: BlendMode,
    ) {
        if !alpha.is_finite() {
            return;
        }
        let level = nearest_level(alpha.clamp(0.0, 1.0) * 255.0);
        self.blend_pixel_level(x, y, color, ink, level, mode);
    }

    /// As [`Canvas::blend_pixel_inked`], with the opacity already a level,
    /// 0..=255: how an image pixel reaches the canvas, its level made by
    /// [`Opacity`] from the draw's alpha and the pixel's own factors.
    pub(crate) fn blend_pixel_level(
        &mut self,
        x: u32,
        y: u32,
        color: Color,
        ink: Option<[u8; 4]>,
        level: u32,
        mode: BlendMode,
    ) {
        if x >= self.width || y >= self.height {
            return;
        }
        let effective = mul255(level, u32::from(color.a));
        if effective == 0 {
            return;
        }

        if mode.is_nonseparable() && self.format == PixelFormat::CmykA8 {
            self.approximated_blends = self.approximated_blends.saturating_add(1);
        }
        let components = self.format.components();
        let source = match (ink, self.format) {
            (Some([cyan, magenta, yellow, black]), PixelFormat::CmykA8) => {
                [cyan, magenta, yellow, black, color.a]
            }
            (_, PixelFormat::Plates { spots }) => {
                self.blend_plate_pixel(x, y, color, ink, effective, mode, spots);
                return;
            }
            _ => self.encode(color),
        };
        let base = (y as usize) * self.stride + (x as usize) * components;
        if effective == 255 && mode == BlendMode::Normal {
            if let Some(pixel) = self.data.get_mut(base..base + components) {
                opaque_normal(pixel, &source, self.format);
            }
            return;
        }
        let backdrop = self.backdrop_alpha(x, y);
        blend(
            self.data.get_mut(base..base + components),
            &source,
            effective,
            self.format,
            mode,
            backdrop,
        );
    }

    /// [`Canvas::blend_pixel_level`] on a plates canvas: the colour's ink on
    /// the process channels, as a fill's ([`Canvas::fill_mask_inked`]), no
    /// ink on any spot, at the pixel's `effective` opacity.
    #[allow(clippy::too_many_arguments)]
    fn blend_plate_pixel(
        &mut self,
        x: u32,
        y: u32,
        color: Color,
        ink: Option<[u8; 4]>,
        effective: u32,
        mode: BlendMode,
        spots: Spots,
    ) {
        let [c, m, yellow, k] = match ink {
            Some(ink) => ink,
            None => {
                let (c, m, yellow, k) = light_ink(color, self.ink_tables.as_deref());
                [c, m, yellow, k]
            }
        };
        if mode.is_nonseparable() {
            self.approximated_blends = self.approximated_blends.saturating_add(1);
        }
        // On the stack: an image reaches here once a pixel.
        let mut source = [0u8; 4 + MAX_SPOT_PLATES];
        if let Some(process) = source.get_mut(..4) {
            process.copy_from_slice(&[c, m, yellow, k]);
        }
        let channels = 4 + spots.count();
        let components = self.format.components();
        let base = (y as usize) * self.stride + (x as usize) * components;
        let backdrop = self.backdrop_alpha(x, y);
        if let (Some(pixel), Some(source)) = (
            self.data.get_mut(base..base + components),
            source.get(..channels),
        ) {
            blend_plates(pixel, source, effective, channels, mode, backdrop);
        }
    }

    /// How opaque the initial backdrop is at a pixel, `a0`: the parent's
    /// alpha at the same device pixel, in the parent's own format — an alpha
    /// needs no conversion, so this is one byte, or 255 for a format that
    /// carries none. Zero without a backdrop, and off the parent, where there
    /// is nothing to blend against.
    fn backdrop_alpha(&self, x: u32, y: u32) -> u32 {
        if !self.backdrop {
            return 0;
        }
        self.beneath(x, y)
            .map_or(0, |(parent, px, py)| parent.alpha_at(px, py))
    }

    /// The parent this buffer holds, and its own pixel under this buffer's
    /// pixel `(x, y)`: the same device pixel. `None` without a parent, and
    /// off it.
    fn beneath(&self, x: u32, y: u32) -> Option<(&Canvas, u32, u32)> {
        let parent = self.parent.as_deref()?;
        let (px, py) = under(self.origin, parent, x, y)?;
        Some((parent, px, py))
    }

    /// The stored alpha at a pixel, 255 in a format that carries none.
    fn alpha_at(&self, x: u32, y: u32) -> u32 {
        if !self.format.has_alpha() {
            return 255;
        }
        if x >= self.width || y >= self.height {
            return 0;
        }
        let components = self.format.components();
        let index =
            (y as usize) * self.stride + (x as usize) * components + color_channels(self.format);
        self.data.get(index).map_or(0, |v| u32::from(*v))
    }

    /// Composites another canvas onto this one, `src`'s top-left landing at
    /// device pixel `at` (11.3.6).
    ///
    /// The missing primitive: a `Canvas` could be rendered into but never
    /// blitted onto another, so a transparency group had nowhere to go and a
    /// pattern tile had to be replayed once per lattice position.
    ///
    /// **Bounded by the source.** The walk is `src`'s own rectangle clipped to
    /// this canvas, so blitting a 40x40 tile onto A4 costs 1600 pixels rather
    /// than the page — the same discipline every paint has followed since
    /// bounded painting landed.
    ///
    /// `alpha` scales the whole operation (`ca`/`CA` at the invoking `Do`),
    /// `mode` is the blend mode in force there, and `mask` multiplies in a
    /// clip or a soft mask, in device pixels like `at`. `src`'s own alpha
    /// is honoured, which is what makes a group buffer composite as a unit:
    /// the shape it painted comes from its alpha channel, not from a path.
    pub fn composite(
        &mut self,
        src: &Canvas,
        at: (i32, i32),
        alpha: f64,
        mode: BlendMode,
        mask: Option<&Mask>,
        stop: Option<&dyn Fn() -> bool>,
    ) {
        self.composite_through(src, None, at, alpha, mode, mask, stop);
    }

    /// As [`Canvas::composite`], every pixel of `src` painted in `color`
    /// rather than its own: `src` gives the coverage, its alpha, and the
    /// caller the colour.
    ///
    /// What an uncoloured tiling pattern's cell is (8.7.3.3): a shape whose
    /// colour comes from whoever fills with it, so one cell rasterised once
    /// serves every colour it is filled in. It writes exactly what
    /// [`Canvas::recolor`] of `src` and then `composite` wrote, without
    /// touching `src`: the colour is taken in `src`'s own bytes and read
    /// across into this canvas's as `composite` reads any pixel, and neither
    /// step depends on a pixel's alpha, so one source word serves the whole
    /// cell. A `src` with no alpha channel, which `recolor` leaves as it is,
    /// composites its own colours.
    ///
    /// The colour has no ink of its own, so on a `CmykA8` canvas it becomes
    /// ink by the tables in force here ([`Canvas::set_ink_tables`]), as any
    /// colour painted here does.
    #[allow(clippy::too_many_arguments)]
    pub fn composite_tinted(
        &mut self,
        src: &Canvas,
        color: Color,
        at: (i32, i32),
        alpha: f64,
        mode: BlendMode,
        mask: Option<&Mask>,
        stop: Option<&dyn Fn() -> bool>,
    ) {
        if !src.format.has_alpha() {
            self.composite_through(src, None, at, alpha, mode, mask, stop);
            return;
        }
        if let PixelFormat::Plates { spots } = self.format {
            let tint = plate_word(color, spots, self.ink_tables.as_deref());
            self.composite_plates(src, Some(&tint), at, alpha, mode, mask, stop);
            return;
        }
        let mut swatch = Canvas::new(1, 1, src.format, color);
        if self.format == PixelFormat::CmykA8 && self.ink_tables.is_some() {
            swatch.ink_tables = self.ink_tables.clone();
            swatch.clear(color);
        }
        let Some((tint, _)) = self.source_from(&swatch, 0, 0) else {
            return;
        };
        self.composite_through(src, Some(tint), at, alpha, mode, mask, stop);
    }

    /// The walk [`Canvas::composite`] and [`Canvas::composite_tinted`]
    /// share: each pixel's source is `src`'s own, read across, or `tint`
    /// beside `src`'s alpha.
    #[allow(clippy::too_many_arguments)]
    fn composite_through(
        &mut self,
        src: &Canvas,
        tint: Option<[u8; 5]>,
        at: (i32, i32),
        alpha: f64,
        mode: BlendMode,
        mask: Option<&Mask>,
        stop: Option<&dyn Fn() -> bool>,
    ) {
        if matches!(self.format, PixelFormat::Plates { .. })
            || matches!(src.format, PixelFormat::Plates { .. })
        {
            self.composite_plates(src, None, at, alpha, mode, mask, stop);
            return;
        }
        let alpha = if alpha.is_finite() {
            operation_level(alpha)
        } else {
            255
        };
        if alpha == 0 {
            return;
        }

        // The source rectangle, mapped into this canvas and clipped to it.
        let (ox, oy) = self.origin;
        let at_local = (
            i64::from(at.0) - i64::from(ox),
            i64::from(at.1) - i64::from(oy),
        );
        let (x0, y0, x1, y1) = place(at_local, src.width, src.height, self.width, self.height);
        let components = self.format.components();
        if mode.is_nonseparable() && self.format == PixelFormat::CmykA8 {
            self.approximated_blends = self.approximated_blends.saturating_add(1);
        }
        for row in y0..y1 {
            if row.wrapping_sub(y0) % STOP_EVERY == 0 && stop.is_some_and(|stop| stop()) {
                return;
            }
            for col in x0..x1 {
                // `place` guarantees these subtractions stay in range.
                let (sx, sy) = (
                    (i64::from(col) - at_local.0) as u32,
                    (i64::from(row) - at_local.1) as u32,
                );
                let read = match tint {
                    Some(tint) => Some((tint, src.alpha_at(sx, sy))),
                    None => self.source_from(src, sx, sy),
                };
                let Some((source, own)) = read else {
                    continue;
                };
                let coverage = mask.map_or(255, |mask| {
                    u32::from(mask.at(ox.saturating_add(col as i32), oy.saturating_add(row as i32)))
                });
                if coverage == 0 {
                    continue;
                }
                let effective = mul255(mul255(own, alpha), coverage);
                if effective == 0 {
                    continue;
                }

                let base = (row as usize) * self.stride + (col as usize) * components;
                if effective == 255 && mode == BlendMode::Normal {
                    if let Some(pixel) = self.data.get_mut(base..base + components) {
                        opaque_normal(pixel, &source, self.format);
                    }
                    continue;
                }
                let backdrop = self.backdrop_alpha(col, row);
                blend(
                    self.data.get_mut(base..base + components),
                    &source,
                    effective,
                    self.format,
                    mode,
                    backdrop,
                );
            }
        }
    }

    /// [`Canvas::composite_through`] where either canvas is plates (11.3.6):
    /// each source pixel read across into this canvas's storage by
    /// [`plate_pixel_into`] — the same format's bytes, ink as ink, light
    /// turned into ink — or, for an uncoloured cell, `tint` beside the
    /// source's alpha, and composited by [`blend_plates`] onto plates or by
    /// `blend` onto any other format, at the source's alpha, the operation's
    /// and the mask's multiplied as `composite_through` multiplies them.
    #[allow(clippy::too_many_arguments)]
    fn composite_plates(
        &mut self,
        src: &Canvas,
        tint: Option<&[u8]>,
        at: (i32, i32),
        alpha: f64,
        mode: BlendMode,
        mask: Option<&Mask>,
        stop: Option<&dyn Fn() -> bool>,
    ) {
        let alpha = if alpha.is_finite() {
            operation_level(alpha)
        } else {
            255
        };
        if alpha == 0 {
            return;
        }
        let (ox, oy) = self.origin;
        let at_local = (
            i64::from(at.0) - i64::from(ox),
            i64::from(at.1) - i64::from(oy),
        );
        let (x0, y0, x1, y1) = place(at_local, src.width, src.height, self.width, self.height);
        let format = self.format;
        let components = format.components();
        let plates = matches!(format, PixelFormat::Plates { .. });
        if mode.is_nonseparable() && (plates || format == PixelFormat::CmykA8) {
            self.approximated_blends = self.approximated_blends.saturating_add(1);
        }
        let channels = color_channels(format);
        // Light a source carries becomes ink here by the tables in force.
        let tables = self.ink_tables.clone();
        let mut word = vec![0u8; components];
        for row in y0..y1 {
            if row.wrapping_sub(y0) % STOP_EVERY == 0 && stop.is_some_and(|stop| stop()) {
                return;
            }
            for col in x0..x1 {
                // `place` guarantees these subtractions stay in range.
                let (sx, sy) = (
                    (i64::from(col) - at_local.0) as u32,
                    (i64::from(row) - at_local.1) as u32,
                );
                let read = match tint {
                    Some(tint) => {
                        for (slot, from) in word.iter_mut().zip(tint) {
                            *slot = *from;
                        }
                        sx < src.width && sy < src.height
                    }
                    None => plate_pixel_into(src, sx, sy, format, tables.as_deref(), &mut word),
                };
                if !read {
                    continue;
                }
                let own = src.alpha_at(sx, sy);
                let coverage = mask.map_or(255, |mask| {
                    u32::from(mask.at(ox.saturating_add(col as i32), oy.saturating_add(row as i32)))
                });
                if coverage == 0 {
                    continue;
                }
                let effective = mul255(mul255(own, alpha), coverage);
                if effective == 0 {
                    continue;
                }
                let base = (row as usize) * self.stride + (col as usize) * components;
                let backdrop = self.backdrop_alpha(col, row);
                let Some(pixel) = self.data.get_mut(base..base + components) else {
                    continue;
                };
                if plates {
                    blend_plates(pixel, &word, effective, channels, mode, backdrop);
                } else {
                    let mut five = [0u8; 5];
                    for (slot, from) in five.iter_mut().zip(&word) {
                        *slot = *from;
                    }
                    blend(Some(pixel), &five, effective, format, mode, backdrop);
                }
            }
        }
    }

    /// A copy of a rectangle of this canvas, in `format`.
    ///
    /// `at` is the rectangle's top-left in device pixels, and the copy stands
    /// there: its [`Canvas::origin`] is `at`.
    ///
    /// Pixels outside this canvas come back transparent, which is the right
    /// answer for a group whose bounding box hangs off the edge of the page:
    /// there is no backdrop out there to blend against.
    ///
    /// Plates come out of a plates canvas as their tints, and go into one as
    /// ink: a `CmykA8` pixel's own ink, any other's light turned into ink, on
    /// the process channels, with no ink on any spot. Out of plates into a
    /// format of light, a pixel is its process channels' light, 8.6.4.4 as
    /// [`Canvas::pixel`] reads it — the colour of a spot is its caller's, and
    /// nothing here knows it.
    #[must_use]
    pub fn extract(&self, at: (i32, i32), width: u32, height: u32, format: PixelFormat) -> Canvas {
        let mut out = Canvas::new(width, height, format, Color::TRANSPARENT).at_origin(at.0, at.1);
        if matches!(format, PixelFormat::Plates { .. })
            || matches!(self.format, PixelFormat::Plates { .. })
        {
            self.extract_plates_into(&mut out, at);
            return out;
        }
        for row in 0..height {
            for col in 0..width {
                let (Some(x), Some(y)) = (
                    (i64::from(at.0) + i64::from(col) - i64::from(self.origin.0))
                        .try_into()
                        .ok(),
                    (i64::from(at.1) + i64::from(row) - i64::from(self.origin.1))
                        .try_into()
                        .ok(),
                ) else {
                    continue;
                };
                let Some((word, _)) = out.source_from(self, x, y) else {
                    continue;
                };
                // A source without an alpha channel is opaque; `pixel` already
                // says so, and the copy has to carry that or a non-isolated
                // group over a page would think it had nothing underneath.
                let pixel = word;
                let components = out.format.components();
                let base = (row as usize) * out.stride + (col as usize) * components;
                if let Some(slot) = out.data.get_mut(base..base + components) {
                    slot.copy_from_slice(&pixel[..components]);
                }
            }
        }
        out
    }

    /// [`Canvas::extract`] where either canvas is plates: each pixel of `out`
    /// read across from this canvas by [`plate_pixel_into`].
    fn extract_plates_into(&self, out: &mut Canvas, at: (i32, i32)) {
        let components = out.format.components();
        let mut word = vec![0u8; components];
        for row in 0..out.height {
            for col in 0..out.width {
                let (Some(x), Some(y)) = (
                    (i64::from(at.0) + i64::from(col) - i64::from(self.origin.0))
                        .try_into()
                        .ok(),
                    (i64::from(at.1) + i64::from(row) - i64::from(self.origin.1))
                        .try_into()
                        .ok(),
                ) else {
                    continue;
                };
                if !plate_pixel_into(self, x, y, out.format, out.ink_tables.as_deref(), &mut word) {
                    continue;
                }
                let base = (row as usize) * out.stride + (col as usize) * components;
                if let Some(slot) = out.data.get_mut(base..base + components) {
                    slot.copy_from_slice(&word);
                }
            }
        }
    }

    /// Starts this group's buffer from its parent, the canvas it will be
    /// composited onto, and holds the parent until
    /// [`Canvas::release_parent`] (11.4.4).
    ///
    /// What a **non-isolated** group does: the backdrop is composited in so
    /// that a blend mode inside the group sees through to it. The alpha
    /// channel is deliberately left at zero — it is the group's *own*
    /// accumulation from here on, and 11.4.7.2 needs it separate from the
    /// backdrop's in order to take the backdrop out again.
    ///
    /// **The parent is moved in, never copied**, and read where it lies:
    /// this buffer's pixel `(x, y)` is device pixel `origin + (x, y)`, which
    /// is the parent's own pixel there. `C0` is the parent's pixel in this
    /// buffer's format through `source_from` — the bytes straight across in
    /// the same format, so an ink page's rich black is the backdrop as the
    /// document chose it, and the conversion `pixel` and `encode` make in
    /// another — and `a0` its alpha. A pixel of the buffer off the parent
    /// stays transparent: there is no backdrop out there to blend against.
    pub fn adopt_backdrop(&mut self, parent: Canvas) {
        let channels = color_channels(self.format);
        let components = self.format.components();
        if matches!(self.format, PixelFormat::Plates { .. }) {
            // A plates pixel is wider than `backdrop_word`'s word: each one
            // read across whole, a plates parent's tints straight across.
            let mut word = vec![0u8; components];
            for row in 0..self.height {
                for col in 0..self.width {
                    let Some((px, py)) = under(self.origin, &parent, col, row) else {
                        continue;
                    };
                    if !plate_pixel_into(&parent, px, py, self.format, None, &mut word) {
                        continue;
                    }
                    let base = (row as usize) * self.stride + (col as usize) * components;
                    for (slot, tint) in self.data.iter_mut().skip(base).zip(&word).take(channels) {
                        *slot = *tint;
                    }
                }
            }
            self.parent = Some(Box::new(parent));
            self.backdrop = true;
            return;
        }
        for row in 0..self.height {
            for col in 0..self.width {
                let Some((px, py)) = under(self.origin, &parent, col, row) else {
                    continue;
                };
                let Some(pixel) = self.backdrop_word(&parent, px, py) else {
                    continue;
                };
                let base = (row as usize) * self.stride + (col as usize) * components;
                for (i, slot) in self.data.iter_mut().skip(base).take(channels).enumerate() {
                    *slot = pixel.get(i).copied().unwrap_or(0);
                }
            }
        }
        self.parent = Some(Box::new(parent));
        self.backdrop = true;
    }

    /// Holds an **isolated** group's parent, the canvas it will be
    /// composited onto, until [`Canvas::release_parent`], and reads nothing
    /// of it: 11.4.4's backdrop alpha is zero, and the buffer starts as it
    /// was made.
    pub fn hold_parent(&mut self, parent: Canvas) {
        self.parent = Some(Box::new(parent));
        self.backdrop = false;
    }

    /// Hands back the parent [`Canvas::adopt_backdrop`] or
    /// [`Canvas::hold_parent`] moved in, `None` for a canvas holding none.
    ///
    /// A non-isolated group's buffer still knows it has a backdrop to take
    /// out, which [`Canvas::remove_backdrop`] reads from the parent handed to
    /// it; nothing is painted into the buffer in between.
    pub fn release_parent(&mut self) -> Option<Canvas> {
        self.parent.take().map(|parent| *parent)
    }

    /// Takes the initial backdrop back out of a non-isolated group's result
    /// (11.4.7.2), reading it from `parent`, the canvas the buffer adopted it
    /// from and [`Canvas::release_parent`] handed back.
    ///
    /// `C = Cn + (Cn - C0)·(a0/agn - a0)`. Without it the backdrop is counted
    /// twice — once because the group started from it and once because the
    /// group is composited back onto it — and the page comes out *darker*
    /// rather than broken, which is a plausible image and therefore the worst
    /// kind of wrong. `a0` and `C0` are read as adoption read them, at the
    /// same device pixels of the same parent, which nothing has painted into
    /// since: the same bytes.
    ///
    /// Nothing happens on an isolated group, which has no backdrop and for
    /// which `a0` is zero and the correction vanishes anyway, nor a second
    /// time.
    pub fn remove_backdrop(&mut self, parent: &Canvas) {
        if !core::mem::take(&mut self.backdrop) {
            return;
        }
        if !self.format.has_alpha() {
            return;
        }
        if matches!(self.format, PixelFormat::Plates { .. }) {
            self.remove_plate_backdrop(parent);
            return;
        }
        let channels = color_channels(self.format);
        let components = self.format.components();

        for row in 0..self.height {
            for col in 0..self.width {
                let own = self.alpha_at(col, row);
                let Some((px, py)) = under(self.origin, parent, col, row) else {
                    continue;
                };
                let initial = parent.alpha_at(px, py);
                if own == 0 || initial == 0 {
                    continue;
                }
                // (a0/agn - a0) in 1/255 units. `own` is non-zero here, so the
                // division is safe; it can be large when the group barely
                // painted, which is the known instability of the removal step
                // and is why every channel clamps.
                let factor = (i64::from(initial) * 255 / i64::from(own)) - i64::from(initial);
                let base = (row as usize) * self.stride + (col as usize) * components;
                // `C0` in this buffer's format, as `adopt_backdrop` wrote it:
                // the parent's own bytes when the two share a format, so no
                // trip through light.
                let Some(base0) = self.backdrop_word(parent, px, py) else {
                    continue;
                };
                for i in 0..channels {
                    let Some(slot) = self.data.get_mut(base + i) else {
                        continue;
                    };
                    let cn = i64::from(*slot);
                    let c0 = i64::from(base0.get(i).copied().unwrap_or(0));
                    let value = cn + (cn - c0) * factor / 255;
                    *slot = value.clamp(0, 255) as u8;
                }
            }
        }
    }

    /// [`Canvas::remove_backdrop`] on a plates buffer: the same step, `C0`
    /// the parent's pixel read across whole as adoption read it, a plates
    /// parent's tints straight across, since a plates pixel is wider than
    /// `backdrop_word`'s word.
    fn remove_plate_backdrop(&mut self, parent: &Canvas) {
        let channels = color_channels(self.format);
        let components = self.format.components();
        let mut base0 = vec![0u8; components];
        for row in 0..self.height {
            for col in 0..self.width {
                let own = self.alpha_at(col, row);
                let Some((px, py)) = under(self.origin, parent, col, row) else {
                    continue;
                };
                let initial = parent.alpha_at(px, py);
                if own == 0 || initial == 0 {
                    continue;
                }
                let factor = (i64::from(initial) * 255 / i64::from(own)) - i64::from(initial);
                if !plate_pixel_into(parent, px, py, self.format, None, &mut base0) {
                    continue;
                }
                let base = (row as usize) * self.stride + (col as usize) * components;
                for (i, c0) in base0.iter().enumerate().take(channels) {
                    let Some(slot) = self.data.get_mut(base + i) else {
                        continue;
                    };
                    let cn = i64::from(*slot);
                    let value = cn + (cn - i64::from(*c0)) * factor / 255;
                    *slot = value.clamp(0, 255) as u8;
                }
            }
        }
    }

    /// `C0`, the parent's pixel `(px, py)` in this buffer's format: its bytes
    /// straight across in the same format, and otherwise the conversion
    /// `pixel` and `encode` make, by maximum undercolour removal into ink —
    /// what a copy of the parent in this format holds, whatever black
    /// generation a paint holds on this buffer while it paints.
    fn backdrop_word(&self, parent: &Canvas, px: u32, py: u32) -> Option<[u8; 5]> {
        if parent.format == self.format {
            return self.source_from(parent, px, py).map(|(word, _)| word);
        }
        parent
            .pixel(px, py)
            .map(|color| self.encode_by(color, None))
    }

    /// The state a group's buffer started in at its pixel `(x, y)`, as the
    /// bytes it stores: a non-isolated group's backdrop colour, `C0` at no
    /// alpha, as adoption wrote it, and otherwise the buffer as it was made,
    /// transparent — `clear`, `Color::TRANSPARENT` in this format.
    fn initial_state(&self, x: u32, y: u32, clear: &[u8; 5]) -> [u8; 5] {
        if !self.backdrop {
            return *clear;
        }
        let Some(mut word) = self
            .beneath(x, y)
            .and_then(|(parent, px, py)| self.backdrop_word(parent, px, py))
        else {
            return *clear;
        };
        if self.format.has_alpha() {
            if let Some(alpha) = word.get_mut(color_channels(self.format)) {
                *alpha = 0;
            }
        }
        word
    }

    /// Puts the state this group's buffer started in back where `mask`
    /// covers (11.4.5).
    ///
    /// A knockout group's elements do not accumulate: each one composites
    /// against the group's *initial* backdrop, so whatever earlier elements
    /// left behind is discarded exactly where the new one paints. Full
    /// coverage is an exact replacement; partial coverage weights between the
    /// two, which is the usual approximation — the spec separates an object's
    /// shape from its alpha and this engine, like the buffers it inherits,
    /// carries only the one number.
    ///
    /// **The initial state is a function, not a snapshot**: an isolated
    /// group starts transparent, and a non-isolated one with its backdrop's
    /// colour at no alpha, read from the parent it holds, so a knockout
    /// group keeps no second buffer.
    pub fn knock_out(&mut self, mask: &Mask) {
        if let PixelFormat::Plates { spots } = self.format {
            self.knock_out_plates(mask, spots);
            return;
        }
        let components = self.format.components();
        // The bytes `Canvas::new` cleared the buffer to, with no tables.
        let clear = self.encode_by(Color::TRANSPARENT, None);
        let (x0, y0, x1, y1) = mask.overlap_at(self.origin, self.width, self.height);
        let (ox, oy) = self.origin;
        for row in y0..y1 {
            for col in x0..x1 {
                let coverage = u32::from(
                    mask.at(ox.saturating_add(col as i32), oy.saturating_add(row as i32)),
                );
                if coverage == 0 {
                    continue;
                }
                let initial = self.initial_state(col, row, &clear);
                let base = (row as usize) * self.stride + (col as usize) * components;
                for (i, from) in initial.iter().enumerate().take(components) {
                    let Some(slot) = self.data.get_mut(base + i) else {
                        continue;
                    };
                    *slot = (mul255(u32::from(*from), coverage)
                        + mul255(u32::from(*slot), 255 - coverage))
                    .min(255) as u8;
                }
            }
        }
    }

    /// [`Canvas::knock_out`] on a plates buffer: the initial state read whole
    /// — a non-isolated group's backdrop tints at no alpha, read across from
    /// the parent as adoption read them, and otherwise the transparent pixel
    /// the buffer was made as — since a plates pixel is wider than
    /// `initial_state`'s word.
    fn knock_out_plates(&mut self, mask: &Mask, spots: Spots) {
        let components = self.format.components();
        let clear = plate_word(Color::TRANSPARENT, spots, None);
        let mut initial = vec![0u8; components];
        let (x0, y0, x1, y1) = mask.overlap_at(self.origin, self.width, self.height);
        let (ox, oy) = self.origin;
        for row in y0..y1 {
            for col in x0..x1 {
                let coverage = u32::from(
                    mask.at(ox.saturating_add(col as i32), oy.saturating_add(row as i32)),
                );
                if coverage == 0 {
                    continue;
                }
                let read = self.backdrop
                    && self.beneath(col, row).is_some_and(|(parent, px, py)| {
                        plate_pixel_into(parent, px, py, self.format, None, &mut initial)
                    });
                if read {
                    if let Some(alpha) = initial.last_mut() {
                        *alpha = 0;
                    }
                } else {
                    initial.copy_from_slice(&clear);
                }
                let base = (row as usize) * self.stride + (col as usize) * components;
                for (i, from) in initial.iter().enumerate() {
                    let Some(slot) = self.data.get_mut(base + i) else {
                        continue;
                    };
                    *slot = (mul255(u32::from(*from), coverage)
                        + mul255(u32::from(*slot), 255 - coverage))
                    .min(255) as u8;
                }
            }
        }
    }

    /// Reads this buffer as a coverage mask over device pixels (11.6.5.2).
    ///
    /// `at` is where the buffer's top-left sits in device space, so the mask
    /// comes back addressed the way a clip is, reading zero outside itself.
    /// `transfer` is `/TR`, pre-sampled to 256 entries — a function evaluated
    /// per pixel would be ruinous and would put a `libm` call on a pixel path
    /// (ruling 4).
    ///
    /// **Row by row, in integer** (`mask_value`): each pixel's value is the
    /// one [`Canvas::pixel`] and [`Color::luminosity`] give, read off its
    /// bytes without making a `Color` of them. A grey is its own luminosity,
    /// the weights summing to 1 000; an RGB pixel takes the weights directly;
    /// an ink pixel goes through [`cmyk_to_rgb`], which is integer; an alpha
    /// mask reads the alpha byte, and a format with none reads 255. A
    /// [`PixelFormat::LabA8`] pixel reaches light only through [`lab_to_rgb`],
    /// and keeps that read. `to_mask_reads_every_format_as_pixel_does` holds
    /// each format to the long way round.
    #[must_use]
    pub fn to_mask(&self, at: (i32, i32), kind: MaskKind, transfer: Option<&[u8; 256]>) -> Mask {
        let mut mask = Mask::empty(at.0, at.1, self.width, self.height);
        let components = self.format.components();
        let width = self.width as usize;
        if width == 0 {
            return mask;
        }
        let read = mask_value(kind, self.format);
        for (row, out) in mask.data.chunks_exact_mut(width).enumerate() {
            let start = row * self.stride;
            let Some(pixels) = self.data.get(start..start + width * components) else {
                continue;
            };
            for (slot, pixel) in out.iter_mut().zip(pixels.chunks_exact(components)) {
                let value = read(pixel);
                *slot = transfer.map_or(value, |lut| {
                    lut.get(usize::from(value)).copied().unwrap_or(value)
                });
            }
        }
        mask
    }

    /// Replaces every pixel's colour with `color`, keeping the alpha each one
    /// already had (8.7.3.3).
    ///
    /// What an uncoloured tiling pattern's cell is: a *shape*, drawn by a
    /// content stream whose own colour operators mean nothing, painted in a
    /// colour chosen by whoever used the pattern. Flattening the colour
    /// afterwards is what makes "colour operators inside the cell are ignored"
    /// true by construction rather than by intercepting every paint — there is
    /// no operator, and no path through an interpreter, that can leave a
    /// colour behind here, including ones not written yet.
    ///
    /// A no-op on a format with no alpha channel, where every pixel is opaque
    /// and the result would be one flat rectangle rather than a shape.
    ///
    /// On a plates canvas the colour is light turned into ink on the process
    /// channels, with no ink on any spot.
    pub fn recolor(&mut self, color: Color) {
        if !self.format.has_alpha() {
            return;
        }
        let components = self.format.components();
        let channels = color_channels(self.format);
        if let PixelFormat::Plates { spots } = self.format {
            let word = plate_word(color, spots, self.ink_tables.as_deref());
            for pixel in self.data.chunks_exact_mut(components) {
                for (slot, from) in pixel.iter_mut().zip(&word).take(channels) {
                    *slot = *from;
                }
            }
            return;
        }
        let source = self.encode(color);
        for row in 0..self.height as usize {
            for col in 0..self.width as usize {
                let base = row * self.stride + col * components;
                for i in 0..channels {
                    let (Some(from), Some(slot)) =
                        (source.get(i).copied(), self.data.get_mut(base + i))
                    else {
                        continue;
                    };
                    *slot = from;
                }
            }
        }
    }

    /// The pixel at `(x, y)` as a colour, for tests and readback.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Option<Color> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let components = self.format.components();
        let base = (y as usize) * self.stride + (x as usize) * components;
        let px = self.data.get(base..base + components)?;
        Some(match self.format {
            PixelFormat::Gray8 => {
                let v = *px.first()?;
                Color::rgb(v, v, v)
            }
            PixelFormat::GrayA8 => {
                let v = *px.first()?;
                Color {
                    r: v,
                    g: v,
                    b: v,
                    a: *px.get(1)?,
                }
            }
            PixelFormat::Rgb8 => Color::rgb(*px.first()?, *px.get(1)?, *px.get(2)?),
            PixelFormat::Rgba8 => Color {
                r: *px.first()?,
                g: *px.get(1)?,
                b: *px.get(2)?,
                a: *px.get(3)?,
            },
            PixelFormat::CmykA8 => {
                let (r, g, b) = cmyk_to_rgb(*px.first()?, *px.get(1)?, *px.get(2)?, *px.get(3)?);
                Color {
                    r,
                    g,
                    b,
                    a: *px.get(4)?,
                }
            }
            PixelFormat::LabA8 => {
                let (r, g, b) = lab_to_rgb(*px.first()?, *px.get(1)?, *px.get(2)?);
                Color {
                    r,
                    g,
                    b,
                    a: *px.get(3)?,
                }
            }
            PixelFormat::Bgr8 => Color::rgb(*px.get(2)?, *px.get(1)?, *px.first()?),
            PixelFormat::Bgra8 => Color {
                r: *px.get(2)?,
                g: *px.get(1)?,
                b: *px.first()?,
                a: *px.get(3)?,
            },
            // The process channels' light, as a `CmykA8` pixel's: a spot's
            // colour is its caller's to say, and nothing here knows it.
            PixelFormat::Plates { spots } => {
                let (r, g, b) = cmyk_to_rgb(*px.first()?, *px.get(1)?, *px.get(2)?, *px.get(3)?);
                Color {
                    r,
                    g,
                    b,
                    a: *px.get(4 + spots.count())?,
                }
            }
        })
    }
}

/// The pixel of `parent` under pixel `(x, y)` of a canvas standing at
/// device pixel `origin`: the same device pixel, in the parent's own indices,
/// or `None` off the parent.
fn under(origin: (i32, i32), parent: &Canvas, x: u32, y: u32) -> Option<(u32, u32)> {
    let device = |at: i32, by: u32| i32::try_from(i64::from(at) + i64::from(by)).ok();
    parent.local(device(origin.0, x)?, device(origin.1, y)?)
}

/// How [`Canvas::to_mask`] reads one pixel of `format`'s bytes as `kind`: the
/// value [`Canvas::pixel`] and [`Color::luminosity`] give, chosen once for the
/// whole buffer.
fn mask_value(kind: MaskKind, format: PixelFormat) -> fn(&[u8]) -> u8 {
    fn byte(pixel: &[u8], at: usize) -> u8 {
        pixel.get(at).copied().unwrap_or(0)
    }
    match (kind, format) {
        // `pixel` gives a format without alpha a whole alpha.
        (MaskKind::Alpha, PixelFormat::Gray8 | PixelFormat::Rgb8 | PixelFormat::Bgr8) => |_| 255,
        (MaskKind::Alpha, PixelFormat::GrayA8) => |pixel| byte(pixel, 1),
        (MaskKind::Alpha, PixelFormat::Rgba8 | PixelFormat::Bgra8 | PixelFormat::LabA8) => {
            |pixel| byte(pixel, 3)
        }
        (MaskKind::Alpha, PixelFormat::CmykA8) => |pixel| byte(pixel, 4),
        // A plates pixel's alpha is its last byte, however many spots.
        (MaskKind::Alpha, PixelFormat::Plates { .. }) => |pixel| pixel.last().copied().unwrap_or(0),
        // A grey `v` reads as `(v, v, v)`, whose luminosity is
        // `(1000 v + 500) / 1000`, which is `v`.
        (MaskKind::Luminosity, PixelFormat::Gray8 | PixelFormat::GrayA8) => |pixel| byte(pixel, 0),
        (MaskKind::Luminosity, PixelFormat::Rgb8 | PixelFormat::Rgba8) => {
            |pixel| luminosity(byte(pixel, 0), byte(pixel, 1), byte(pixel, 2))
        }
        // An output order: blue first.
        (MaskKind::Luminosity, PixelFormat::Bgr8 | PixelFormat::Bgra8) => {
            |pixel| luminosity(byte(pixel, 2), byte(pixel, 1), byte(pixel, 0))
        }
        // A plates pixel is its process channels' light, as `pixel` reads it.
        (MaskKind::Luminosity, PixelFormat::CmykA8 | PixelFormat::Plates { .. }) => |pixel| {
            let (r, g, b) = cmyk_to_rgb(
                byte(pixel, 0),
                byte(pixel, 1),
                byte(pixel, 2),
                byte(pixel, 3),
            );
            luminosity(r, g, b)
        },
        (MaskKind::Luminosity, PixelFormat::LabA8) => |pixel| {
            let (r, g, b) = lab_to_rgb(byte(pixel, 0), byte(pixel, 1), byte(pixel, 2));
            luminosity(r, g, b)
        },
    }
}

/// A light colour as a [`PixelFormat::Plates`] pixel: its ink on the process
/// channels, as a `CmykA8` canvas encodes it — by `tables` where they are in
/// force — no ink on any spot, and its alpha.
fn plate_word(color: Color, spots: Spots, tables: Option<&InkTables>) -> Vec<u8> {
    let (c, m, y, k) = light_ink(color, tables);
    let mut word = Vec::with_capacity(5 + spots.count());
    word.extend_from_slice(&[c, m, y, k]);
    word.resize(4 + spots.count(), 0);
    word.push(color.a);
    word
}

/// `src`'s pixel `(sx, sy)` in `format`'s storage, written into `out`, which
/// is `format.components()` bytes, light turned into ink by `tables` where
/// they are in force: `false` off `src` or where `out` is not that wide.
///
/// The conversion every copy across formats makes where one side is plates.
/// The same format reads the bytes across. Into plates, a `CmykA8` or plates
/// pixel's process inks are copied as ink, a pixel of light is turned into
/// ink ([`rgb_to_cmyk`]), and a spot channel takes the source's spot of the
/// same index where the source has one and no ink where it has not. Out of
/// plates into `CmykA8`, the process inks are copied; into any other format
/// the pixel is its process channels' light ([`Canvas::pixel`]).
fn plate_pixel_into(
    src: &Canvas,
    sx: u32,
    sy: u32,
    format: PixelFormat,
    tables: Option<&InkTables>,
    out: &mut [u8],
) -> bool {
    if sx >= src.width || sy >= src.height || out.len() != format.components() {
        return false;
    }
    let src_components = src.format.components();
    let base = (sy as usize) * src.stride + (sx as usize) * src_components;
    let Some(raw) = src.data.get(base..base + src_components) else {
        return false;
    };
    if src.format == format {
        out.copy_from_slice(raw);
        return true;
    }
    let inked = matches!(src.format, PixelFormat::CmykA8 | PixelFormat::Plates { .. });
    let alpha = src.alpha_at(sx, sy).min(255) as u8;
    match format {
        PixelFormat::Plates { spots } => {
            let process = if inked {
                [
                    raw.first().copied().unwrap_or(0),
                    raw.get(1).copied().unwrap_or(0),
                    raw.get(2).copied().unwrap_or(0),
                    raw.get(3).copied().unwrap_or(0),
                ]
            } else {
                let Some(color) = src.pixel(sx, sy) else {
                    return false;
                };
                let (c, m, y, k) = light_ink(color, tables);
                [c, m, y, k]
            };
            let source_spots = match src.format {
                PixelFormat::Plates { spots } => spots.count(),
                _ => 0,
            };
            for (channel, slot) in out.iter_mut().enumerate().take(4 + spots.count()) {
                *slot = match channel {
                    0..=3 => process.get(channel).copied().unwrap_or(0),
                    spot if spot - 4 < source_spots => raw.get(spot).copied().unwrap_or(0),
                    _ => 0,
                };
            }
            if let Some(slot) = out.get_mut(4 + spots.count()) {
                *slot = alpha;
            }
        }
        PixelFormat::CmykA8 => {
            // Out of plates: the process inks, as ink.
            for (slot, from) in out.iter_mut().zip(raw).take(4) {
                *slot = *from;
            }
            if let Some(slot) = out.get_mut(4) {
                *slot = alpha;
            }
        }
        _ => {
            let Some(color) = src.pixel(sx, sy) else {
                return false;
            };
            let word = encode_in(format, color, tables);
            for (slot, from) in out.iter_mut().zip(word.iter()) {
                *slot = *from;
            }
        }
    }
    true
}

/// Where a `width` by `height` source placed at `at` lands on a `dst_width` by
/// `dst_height` canvas, as `(x0, y0, x1, y1)` with the far edges exclusive.
///
/// In `i64` throughout: a group's bounding box can be placed at any device
/// coordinate a content stream can name, and `at.0 + width` overflows an `i32`
/// long before anything about it is unreasonable.
fn place(
    at: (i64, i64),
    width: u32,
    height: u32,
    dst_width: u32,
    dst_height: u32,
) -> (u32, u32, u32, u32) {
    let span = |origin: i64, extent: u32, limit: u32| {
        let lo = origin.clamp(0, i64::from(limit)) as u32;
        let hi = origin
            .saturating_add(i64::from(extent))
            .clamp(0, i64::from(limit)) as u32;
        (lo, hi.max(lo))
    };
    let (x0, x1) = span(at.0, width, dst_width);
    let (y0, y1) = span(at.1, height, dst_height);
    (x0, y0, x1, y1)
}

/// ISO 32000-1 8.6.4.4's device relation, in bytes: ink to light.
///
/// `R = (1 - C)(1 - K)`, and its two siblings. The same relation
/// `tinker_pdf_color::ColorSpace::DeviceCmyk` states over `f64` components;
/// this crate carries its own byte-level copy rather than taking a dependency
/// on that one, for the reason it already carries [`Color::luma`]'s
/// coefficients — a rasterizer knows how to turn stored components into light,
/// and the crate graph says a leaf takes bytes and plain values in.
/// `the_two_crates_agree_on_the_device_relation` holds the copy to the
/// original so it cannot drift.
///
/// **Lab is the exception, and deliberately.** Its conversion needs the two
/// XYZ matrices, and `tinker-pdf-color` has already watched that pair drift
/// when it existed in two places — so [`lab_bytes`] calls that crate rather
/// than copying them. Ruling 8 admits a leaf-to-leaf edge; a duplicated
/// constant is what it does not admit.
///
/// A colour as [`PixelFormat::LabA8`]'s three bytes.
///
/// `L*` is 0..100 and `a`/`b` roughly -128..127, and a byte is 0..255, so the
/// encoding is `L/100`, `(a + 128)/255` and `(b + 128)/255` clamped. That is
/// where a Lab group loses precision relative to the space itself — about a
/// third of a unit of `L*` per step — and it is the price of blending with the
/// same integer arithmetic every other buffer uses.
///
/// `pub` beside [`lab_to_rgb`], its inverse, because the bitmap toolbox
/// encodes a colour into a Lab bitmap exactly as a Lab buffer is filled, and a
/// test that holds it there has to compute the same bytes.
#[must_use]
pub fn lab_bytes(color: Color) -> (u8, u8, u8) {
    let (l, a, b) = tinker_pdf_color::srgb_to_lab(color.r, color.g, color.b);
    let byte = |v: f64| -> u8 { v.clamp(0.0, 255.0).round() as u8 };
    (byte(l / 100.0 * 255.0), byte(a + 128.0), byte(b + 128.0))
}

/// A colour in `format`'s bytes, light turned into ink by `tables` where
/// they are in force and by maximum undercolour removal where they are not:
/// [`Canvas::encode`], for a caller holding a format rather than a canvas.
fn encode_in(format: PixelFormat, color: Color, tables: Option<&InkTables>) -> [u8; 5] {
    match format {
        PixelFormat::Gray8 => [color.luma(), 0, 0, 0, 0],
        PixelFormat::GrayA8 => [color.luma(), color.a, 0, 0, 0],
        PixelFormat::Rgb8 => [color.r, color.g, color.b, 0, 0],
        PixelFormat::Rgba8 => [color.r, color.g, color.b, color.a, 0],
        PixelFormat::CmykA8 => {
            let (c, m, y, k) = light_ink(color, tables);
            [c, m, y, k, color.a]
        }
        PixelFormat::LabA8 => {
            let (l, a, b) = lab_bytes(color);
            [l, a, b, color.a, 0]
        }
        PixelFormat::Bgr8 => [color.b, color.g, color.r, 0, 0],
        PixelFormat::Bgra8 => [color.b, color.g, color.r, color.a, 0],
        // The whole pixel only where there is no spot, which is a `CmykA8`
        // pixel; a plates canvas with spots is wider than this word, and
        // every path that composites on one builds its own
        // (`plate_word`, `PlateInk`) rather than reading this.
        PixelFormat::Plates { .. } => {
            let (c, m, y, k) = light_ink(color, tables);
            [c, m, y, k, color.a]
        }
    }
}

/// Light turned into ink: by black generation and undercolour removal where
/// `tables` hold them (10.4.2.4), and by maximum undercolour removal
/// ([`rgb_to_cmyk`]) where they do not.
fn light_ink(color: Color, tables: Option<&InkTables>) -> (u8, u8, u8, u8) {
    match tables {
        Some(tables) => tables.ink(color.r, color.g, color.b),
        None => rgb_to_cmyk(color.r, color.g, color.b),
    }
}

/// The inverse of [`lab_bytes`]: [`PixelFormat::LabA8`]'s three bytes back to
/// sRGB.
///
/// `pub` beside [`cmyk_to_rgb`] for the same caller and the same reason.
/// `tinker_pdf::Bitmap::to_png` has to turn every one of the six pixel formats
/// into one of PNG's five colour types, and PNG has no Lab; re-deriving the
/// `L/100`, `(a + 128)/255`, `(b + 128)/255` encoding at that call site would
/// be a second copy of a choice this module's own doc says is a choice, which
/// is the drift `tinker-pdf-color` already recorded once.
#[must_use]
pub fn lab_to_rgb(l: u8, a: u8, b: u8) -> (u8, u8, u8) {
    tinker_pdf_color::lab_to_srgb(
        f64::from(l) / 255.0 * 100.0,
        f64::from(a) - 128.0,
        f64::from(b) - 128.0,
    )
}

pub fn cmyk_to_rgb(c: u8, m: u8, y: u8, k: u8) -> (u8, u8, u8) {
    let white = 255 - u32::from(k);
    let ink = |v: u8| mul255(255 - u32::from(v), white).min(255) as u8;
    (ink(c), ink(m), ink(y))
}

/// 8.6.4.4's relation, inverted, with maximum undercolour removal.
///
/// `K` takes as much of the grey as it can — `K = 255 - max(R, G, B)` — and the
/// three inks carry what is left. That choice makes the inverse **exact**:
/// `cmyk_to_rgb(rgb_to_cmyk(x)) == x` for every one of the sixteen million
/// colours, because each channel's intermediate error is bounded below half a
/// level. `the_cmyk_round_trip_is_exact` sweeps every `(v, max)` pair that can
/// occur and finds no exception.
///
/// The other direction is *not* an identity, and the difference matters. A
/// CMYK value that did not come from here — a rich black, say — comes back as
/// the pure-K black with the same colour, because that is the only split this
/// function produces. And ink buffers hold such values: a DeviceCMYK colour
/// reaches a [`PixelFormat::CmykA8`] canvas as the document's own components
/// ([`Canvas::fill_mask_inked`], [`Canvas::blend_pixel_inked`]), so `1 1 1 1 k`
/// is all four inks there, a value this function never returns. So nothing
/// that copies an ink buffer into another of the same format may go through
/// light and back — `source_from` reads the bytes across, and a non-isolated
/// group's backdrop is adopted and removed that way — and the one place that
/// must take the round trip, 11.3.5.3's non-separable blends, counts it as
/// `approximated_blends`.
#[must_use]
pub fn rgb_to_cmyk(r: u8, g: u8, b: u8) -> (u8, u8, u8, u8) {
    let white = u32::from(r.max(g).max(b));
    if white == 0 {
        // Black: all of it is `K`, and asking for the inks would divide by the
        // white that is not there.
        return (0, 0, 0, 255);
    }
    let ink = |v: u8| (255 - (u32::from(v) * 255 + white / 2) / white).min(255) as u8;
    (ink(r), ink(g), ink(b), (255 - white).min(255) as u8)
}

/// Pixels in a block of [`Canvas::fill_opaque`]'s row walk.
const BLOCK: usize = 16;

/// One row of [`Canvas::fill_opaque`]: `coverage` over `pixels`, `N` bytes a
/// pixel, `ALPHA` whether the last of them is alpha, and `solid` a block's
/// worth of the encoded colour `source`.
///
/// Whole blocks of [`BLOCK`] pixels first, each classed by its coverage —
/// one AND and one OR over its bytes — and, in a format with alpha, by an
/// AND over its pixels' alpha bytes: no coverage passes the block over; full
/// coverage copies `solid` whole, whatever its pixels held, `solid` being the
/// colour with an alpha of 255 ([`opaque_normal`]); over pixels all opaque,
/// any other coverage takes [`opaque_block`]; a block holding a pixel that is
/// not opaque, and the pixels past the last whole block, take
/// [`opaque_pixel`] wherever their coverage is not zero. Each class writes
/// the bytes the pixel-by-pixel walk writes, so the order and the grouping
/// decide nothing.
fn opaque_row<const N: usize, const ALPHA: bool>(
    pixels: &mut [u8],
    coverage: &[u8],
    source: &[u8; 5],
    solid: &[u8],
    format: PixelFormat,
) {
    let mut blocks = pixels.chunks_exact_mut(BLOCK * N);
    let mut covers = coverage.chunks_exact(BLOCK);
    for (block, cover) in (&mut blocks).zip(&mut covers) {
        let (all, any) = cover
            .iter()
            .fold((255u8, 0u8), |(all, any), &c| (all & c, any | c));
        if any == 0 {
            continue;
        }
        if all == 255 {
            if let Some(from) = solid.get(..block.len()) {
                block.copy_from_slice(from);
            }
            continue;
        }
        let opaque = !ALPHA
            || block.chunks_exact(N).fold(255u8, |held, pixel| {
                held & pixel.last().copied().unwrap_or(0)
            }) == 255;
        if opaque {
            opaque_block::<N>(block, cover, solid);
        } else {
            each_opaque_pixel::<N>(block, cover, source, format);
        }
    }
    each_opaque_pixel::<N>(blocks.into_remainder(), covers.remainder(), source, format);
}

/// [`opaque_pixel`] for each pixel of `pixels`, `N` bytes each, whose
/// coverage is not zero.
fn each_opaque_pixel<const N: usize>(
    pixels: &mut [u8],
    coverage: &[u8],
    source: &[u8; 5],
    format: PixelFormat,
) {
    for (pixel, &c) in pixels.chunks_exact_mut(N).zip(coverage) {
        if c != 0 {
            opaque_pixel(pixel, source, c, format);
        }
    }
}

/// A block of opaque pixels, `N` bytes each, under coverage `cover`: every
/// byte, colour and alpha alike, `min(mul255(s, c) + mul255(b, 255 − c), 255)`
/// for its byte `s` of `solid` and its pixel's coverage `c`.
///
/// That is [`opaque_pixel`] at every coverage, zero and full included: at 0
/// it is the pixel as it was, at 255 the colour, and between them
/// [`over_opaque`]; an alpha byte of 255 under a source alpha of 255 stays
/// 255, `mul255(255, c) + mul255(255, 255 − c)` being `c + (255 − c)`. So the
/// block is one loop over its bytes, branch-free, in sixteen-bit lanes
/// ([`mul255_narrow`]), each byte's coverage spread from its pixel's first.
fn opaque_block<const N: usize>(block: &mut [u8], cover: &[u8], solid: &[u8]) {
    let mut spread = [0u8; BLOCK * 5];
    for (bytes, &c) in spread.chunks_exact_mut(N).zip(cover) {
        bytes.fill(c);
    }
    for ((byte, &s), &c) in block.iter_mut().zip(solid).zip(&spread) {
        let (c, s, b) = (u16::from(c), u16::from(s), u16::from(*byte));
        *byte = (mul255_narrow(s, c) + mul255_narrow(b, 255 - c)).min(255) as u8;
    }
}

/// [`mul255`] in sixteen bits, for operands of at most 255: `a·b + 128` is at
/// most 65 153 and adding its high byte at most 65 407, so nothing wraps and
/// the answer is `mul255`'s, in lanes half as wide.
#[inline(always)]
fn mul255_narrow(a: u16, b: u16) -> u16 {
    let product = a * b + 128;
    (product + (product >> 8)) >> 8
}

/// One pixel of [`Canvas::fill_opaque`], at a coverage `c` that is not zero:
/// the colour at full coverage over any pixel ([`opaque_normal`]), and at any
/// other a pixel that is not opaque through `blend` and an opaque one through
/// [`over_opaque`].
fn opaque_pixel(pixel: &mut [u8], source: &[u8; 5], c: u8, format: PixelFormat) {
    let channels = color_channels(format);
    if c == 255 {
        opaque_normal(pixel, source, format);
    } else if format.has_alpha() && pixel.get(channels) != Some(&255) {
        blend(
            Some(pixel),
            source,
            u32::from(c),
            format,
            BlendMode::Normal,
            0,
        );
    } else {
        over_opaque(pixel, source, u32::from(c), channels);
    }
}

/// `blend`'s `Normal` composite at a full effective alpha: the source's
/// colour channels, and an alpha of 255 in a format that carries one,
/// whatever the pixel held, whatever its initial backdrop and in every
/// format (`docs/design/performance.md`, P-25).
///
/// **Why that is `blend` exactly.** At an alpha of 255 the union alpha is 255
/// and Normal's mix is the source channel `cs`, so the colour comes out
/// `mul255(255 − ab, cs) + mul255(ab, cs)` renormalised against 255, for the
/// backdrop alpha `ab`. The two terms are `round(x)` and `round(cs − x)` for
/// `x = ab·cs/255`, which sum to `cs` unless `x` is a half-integer, and it
/// never is: `2·ab·cs` is even and an odd multiple of 255 is odd. So every
/// colour channel is `cs` and the alpha `255 + mul255(a, 0)`, 255. A
/// subtractive channel enters Normal complemented and leaves it complemented
/// again, so an ink buffer gets the source's ink, the document's own where
/// [`Canvas::fill_mask_inked`] was handed it.
/// `the_opaque_normal_path_is_blend_in_every_group_format` holds it to `blend`
/// over every channel value against a lattice of alphas, per format.
fn opaque_normal(pixel: &mut [u8], source: &[u8; 5], format: PixelFormat) {
    let channels = color_channels(format);
    for (slot, from) in pixel.iter_mut().zip(source.iter()).take(channels) {
        *slot = *from;
    }
    if format.has_alpha() {
        if let Some(alpha) = pixel.get_mut(channels) {
            *alpha = 255;
        }
    }
}

/// `blend`'s Normal composite of `source` at coverage `c` over a pixel whose
/// alpha is whole, with no initial backdrop: each colour channel
/// `min(mul255(cs, c) + mul255(cb, 255 − c), 255)`, and the alpha, 255 before,
/// 255 after. [`Canvas::fill_opaque`] says why that is `blend` exactly.
fn over_opaque(pixel: &mut [u8], source: &[u8; 5], c: u32, channels: usize) {
    let inverse = 255 - c;
    for (slot, cs) in pixel.iter_mut().zip(source.iter()).take(channels) {
        *slot = (mul255(u32::from(*cs), c) + mul255(u32::from(*slot), inverse)).min(255) as u8;
    }
}

/// `a * b / 255`, rounded, in integers.
pub(crate) fn mul255(a: u32, b: u32) -> u32 {
    let product = a * b + 128;
    (product + (product >> 8)) >> 8
}

/// `x.round() as u32`, without the call: the whole part, and one more where
/// what is left is a half or over.
///
/// `x - whole` is exact. Below one it is `x` itself, and from one up
/// `whole <= x < 2 × whole`, which is Sterbenz's condition for a difference
/// with no rounding in it; so the comparison sees the true fraction and the
/// answer is `round`'s, half away from zero, for every `x` in `[0, 2^32)`.
/// Below zero, past `u32::MAX` and at NaN it saturates as the cast after
/// `round` does, so the two agree on every `f64`.
///
/// `round` is exact as well (ruling 4), and the reason for this is cost
/// rather than determinism: on a target without a rounding instruction
/// `f64::round` is a call into the platform's maths library, and an image
/// made one for every pixel it drew.
#[must_use]
pub(crate) fn nearest_level(x: f64) -> u32 {
    let whole = x as u32;
    whole.saturating_add(u32::from(x - f64::from(whole) >= 0.5))
}

#[cfg(test)]
thread_local! {
    /// `f64::round` calls this thread has made to turn an opacity into a
    /// level: what `an_image_draw_rounds_no_float_per_pixel` counts.
    pub(crate) static LEVEL_ROUNDS: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
}

/// A fill's or a composite's own alpha as a level,
/// `round(clamp(alpha, 0, 1) × 255)`: the one `f64::round` an operation
/// makes, once for the whole of it, and counted under `cfg(test)`.
fn operation_level(alpha: f64) -> u32 {
    #[cfg(test)]
    LEVEL_ROUNDS.with(|rounds| rounds.set(rounds.get() + 1));
    (alpha.clamp(0.0, 1.0) * 255.0).round() as u32
}

/// A draw's alpha, read once, and the level it gives a pixel of integer
/// coverage factors.
///
/// An image pixel's opacity is the draw's alpha times each of its factors
/// over 255 — the quad's coverage, the sample's own alpha and the clip for a
/// pixel drawn directly, the run's coverage and the clip for one composited
/// from a run — evaluated in `f64` in that order and made a level by
/// rounding `clamp(·, 0, 1) × 255`. This answers with that level, exactly,
/// without rounding a float per pixel through `round`:
///
/// - **An alpha of one** gives `(a·b·c + 32512) / 65025` and `(a·b + 127) /
///   255`. The exact value `N / 65025` is never a half-integer, since `2N` is
///   even and 65 025 odd, so it stands at least 1/130 050 from one, and the
///   `f64` expression's six roundings of relative size 2^-53, on a value at
///   most 255, leave it within 2e-13 of the exact value: both round to
///   `floor((N + 32512.5) / 65025)`, which is the integer quotient above
///   because no multiple of 65 025 lies above `N + 32512` and at or below
///   `N + 32512.5`. Two factors stand 1/510 apart, against four roundings.
/// - **Any other alpha** keeps the expression, in its order, rounded by
///   [`nearest_level`]. No integer form can stand in for it: the alpha carries
///   53 bits, and a decimal alpha such as 0.37 puts the exact value of some
///   integer product within 1e-15 of a half-integer, where only the
///   expression's own roundings say which way it goes. The level of a pixel
///   whose factors are all 255 — most of a draw — is worked out once.
///
/// `docs/design/image-edges.md`, P-08, has the argument at more length, and
/// `an_image_pixel_composes_as_the_float_expression_rounded` holds every
/// factor at a sweep of alphas to the expression rounded by `f64::round`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Opacity {
    /// The draw's alpha, clamped into `[0, 1]`; NaN stays NaN and composites
    /// nothing, as it did as a non-finite opacity.
    alpha: f64,
    /// Whether the alpha is exactly one, where the levels are integers.
    unit: bool,
    /// The level of three factors of 255.
    full_three: u32,
    /// The level of two factors of 255.
    full_two: u32,
}

impl Opacity {
    /// The opacity of a draw at `alpha`, which is clamped into `[0, 1]`.
    #[must_use]
    pub(crate) fn new(alpha: f64) -> Opacity {
        let alpha = alpha.clamp(0.0, 1.0);
        let mut opacity = Opacity {
            alpha,
            unit: alpha == 1.0,
            full_three: 0,
            full_two: 0,
        };
        opacity.full_three = opacity.expression_three(255, 255, 255);
        opacity.full_two = opacity.expression_two(255, 255);
        opacity
    }

    /// The level of `alpha × a/255 × b/255 × c/255`: a pixel drawn directly,
    /// `a` its coverage, `b` its sample's alpha and `c` the clip.
    #[must_use]
    pub(crate) fn level_three(&self, a: u8, b: u8, c: u8) -> u32 {
        if self.unit {
            return (u32::from(a) * u32::from(b) * u32::from(c) + 32_512) / 65_025;
        }
        if [a, b, c] == [255; 3] {
            return self.full_three;
        }
        self.expression_three(a, b, c)
    }

    /// The level of `alpha × a/255 × b/255`: a pixel composited from a run,
    /// `a` the run's coverage and `b` the clip.
    #[must_use]
    pub(crate) fn level_two(&self, a: u8, b: u8) -> u32 {
        if self.unit {
            return (u32::from(a) * u32::from(b) + 127) / 255;
        }
        if [a, b] == [255; 2] {
            return self.full_two;
        }
        self.expression_two(a, b)
    }

    /// The three-factor expression in `f64`, in the order a draw writes it.
    fn expression_three(&self, a: u8, b: u8, c: u8) -> u32 {
        level_of(self.alpha * f64::from(a) / 255.0 * f64::from(b) / 255.0 * f64::from(c) / 255.0)
    }

    /// The two-factor expression in `f64`, in the order a run writes it.
    fn expression_two(&self, a: u8, b: u8) -> u32 {
        level_of(self.alpha * f64::from(a) / 255.0 * f64::from(b) / 255.0)
    }
}

/// An opacity in `f64` as the level [`Canvas::blend_pixel_inked`] makes it,
/// with nothing for one that is not finite.
fn level_of(opacity: f64) -> u32 {
    if !opacity.is_finite() {
        return 0;
    }
    nearest_level(opacity.clamp(0.0, 1.0) * 255.0)
}

/// How many colour channels a format carries, before any alpha.
fn color_channels(format: PixelFormat) -> usize {
    match format {
        PixelFormat::Gray8 | PixelFormat::GrayA8 => 1,
        PixelFormat::Rgb8
        | PixelFormat::Rgba8
        | PixelFormat::LabA8
        | PixelFormat::Bgr8
        | PixelFormat::Bgra8 => 3,
        PixelFormat::CmykA8 => 4,
        PixelFormat::Plates { spots } => 4 + spots.count(),
    }
}

/// Composites `source` onto `dst` with the given alpha and blend mode.
///
/// # The convention, which was not written down and was not consistent
///
/// Every channel here is **straight** — not premultiplied. `clear` and
/// `encode` write straight colour and `pixel` reads it back, so that is the
/// only convention the type can be said to have; but the compositing below
/// used to compute `Co = Cs·as + Cb·(1−as)`, which is the *premultiplied*
/// result. The two agree exactly when the backdrop is opaque and disagree
/// everywhere else.
///
/// That was invisible while the only canvas was a page with an opaque
/// background — which is every canvas the engine made until something needed
/// to draw into a transparent buffer. A tile rendered once and blitted, or a
/// transparency group, starts fully transparent, and there the old formula
/// darkens everything toward the uninitialised black underneath.
///
/// So: straight alpha, `Co = (Cs·as + Cb·ab·(1−as)) / ao` with
/// `ao = as + ab·(1−as)` (11.3.6). The opaque case is kept as a fast path
/// because it is still the overwhelmingly common one and it is exactly the
/// old arithmetic, so nothing about page rendering changes.
///
/// # Two alphas, not one
///
/// `initial` is the alpha of a non-isolated group's *initial backdrop*
/// (11.4.4), and is zero everywhere else. Where it is non-zero the pixel
/// carries two different alphas: the colour channels hold the group over its
/// backdrop, so the weight the blend function and the straight-alpha
/// renormalisation both want is the **union** `au = ag + a0·(1 - ag)`, while
/// the alpha channel must keep accumulating the group's *own* `ag` so that
/// 11.4.7.2 can divide the backdrop out again afterwards. With `initial = 0`
/// the two are the same number and every line below is what it was.
fn blend(
    dst: Option<&mut [u8]>,
    source: &[u8; 5],
    alpha: u32,
    format: PixelFormat,
    mode: BlendMode,
    initial: u32,
) {
    let Some(dst) = dst else { return };
    #[cfg(test)]
    blends::counted();
    if alpha == 0 {
        return;
    }
    let inverse = 255 - alpha;
    let channels = color_channels(format);

    // A format without an alpha channel is opaque by construction.
    let own_alpha = if format.has_alpha() {
        dst.get(channels).map_or(255, |a| u32::from(*a))
    } else {
        255
    };
    let backdrop_alpha = own_alpha + mul255(initial, 255 - own_alpha);

    // 11.3.6: the blend function sees the backdrop only to the extent the
    // backdrop is there. Where it is absent the source passes through
    // unblended, which is what keeps `Multiply` from turning a transparent
    // buffer black.
    // Four, for the widest colour-channel count a format will carry. Both
    // loops below bound themselves by `channels`, so the spare entry is never
    // read; it is sized here so that adding a four-channel format is an arm of
    // `encode` rather than a change to this arithmetic.
    let mut blended = [0u32; 4];
    // 11.3.5's separable functions are written for *additive* components: they
    // assume more of a channel is more light. A subtractive channel is a
    // quantity of ink and runs the other way, so it enters and leaves them
    // complemented. Only the blend function is complemented — the weighting
    // around it is 11.3.6's, which averages colour *values* in the group's own
    // space, so it works on the stored components either way.
    let subtractive = matches!(format, PixelFormat::CmykA8);
    for (i, out) in blended.iter_mut().enumerate().take(channels) {
        let (Some(slot), Some(src)) = (dst.get(i), source.get(i)) else {
            continue;
        };
        let cs = u32::from(*src);
        let cb = u32::from(*slot);
        let mixed = if subtractive {
            255 - mode.apply(255 - cb, 255 - cs)
        } else {
            mode.apply(cb, cs)
        };
        *out = mul255(255 - backdrop_alpha, cs) + mul255(backdrop_alpha, mixed);
    }
    if mode.is_nonseparable() && subtractive {
        // 11.3.5.3's four modes are defined over *RGB* values — they reason
        // about hue, saturation and luminosity, which ink quantities do not
        // have. So a subtractive buffer converts its operands to light, blends
        // there, and converts the answer back.
        //
        // That round trip is not free: `rgb_to_cmyk` produces one particular
        // ink split, so a rich black arrives back as its pure-K equivalent.
        // The colour is the same and the *next* blend over it is not, which is
        // why it is reported rather than done quietly — counted on the canvas
        // as `approximated_blends` and turned into
        // `RenderWarning::ApproximatedGroupBlend` when the group closes,
        // because nothing this far down knows what a warning is (ruling 8).
        let (br, bg, bb) = cmyk_to_rgb(
            dst.first().copied().unwrap_or(0),
            dst.get(1).copied().unwrap_or(0),
            dst.get(2).copied().unwrap_or(0),
            dst.get(3).copied().unwrap_or(0),
        );
        let (sr, sg, sb) = cmyk_to_rgb(
            source.first().copied().unwrap_or(0),
            source.get(1).copied().unwrap_or(0),
            source.get(2).copied().unwrap_or(0),
            source.get(3).copied().unwrap_or(0),
        );
        let lit = mode.apply_nonseparable(
            [u32::from(br), u32::from(bg), u32::from(bb)],
            [u32::from(sr), u32::from(sg), u32::from(sb)],
        );
        let byte = |v: u32| v.min(255) as u8;
        let (c, m, y, k) = rgb_to_cmyk(byte(lit[0]), byte(lit[1]), byte(lit[2]));
        for (i, mixed) in [c, m, y, k].into_iter().enumerate() {
            let (Some(out), Some(src)) = (blended.get_mut(i), source.get(i)) else {
                continue;
            };
            *out = mul255(255 - backdrop_alpha, u32::from(*src))
                + mul255(backdrop_alpha, u32::from(mixed));
        }
    } else if mode.is_nonseparable() {
        // 11.3.5.3's four modes are defined over three components, and a
        // one-channel buffer is a grey — so the clause applies to it exactly,
        // by replicating that grey into three and taking the first component
        // back. A grey blended with a grey comes out grey under all four, so
        // nothing is lost in the round trip.
        //
        // This used to be gated on `channels == 3`, which sent a `Gray8`
        // render through `apply`'s `_ => cs` fall-through instead. That paints
        // the *source*, and three of the four modes reduce to the *backdrop*
        // on achromatic operands: `/BM /Hue` over two greys came out 127 levels
        // wrong, and only at one channel, so the same page rendered two ways
        // disagreed with itself.
        let pick = |slot: Option<&u8>, first: Option<&u8>| -> u32 {
            u32::from(*if channels == 1 { first } else { slot }.unwrap_or(&0))
        };
        // Where red, green and blue are stored. The clause's functions read
        // the three in that order — `Lum` weighs them 0.3, 0.59 and 0.11 — so
        // a blue-first pixel is handed over red first and its answer written
        // back blue first. The permutation is its own inverse, so `order[i]`
        // is both the slot holding colour `i` and the colour slot `i` holds.
        let order: [usize; 3] = if format.is_blue_first() {
            [2, 1, 0]
        } else {
            [0, 1, 2]
        };
        let cb = order.map(|at| pick(dst.get(at), dst.first()));
        let cs = order.map(|at| pick(source.get(at), source.first()));
        let mixed = mode.apply_nonseparable(cb, cs);
        let cs = order.map(|at| cs.get(at).copied().unwrap_or(0));
        let mixed = order.map(|at| mixed.get(at).copied().unwrap_or(0));
        // Bounded by `channels`, like the separable loop above it. The bound is
        // redundant today — this branch only runs when `channels` is already 3
        // and `blended` is three wide — and it is here because the *next* thing
        // to touch this file is a fourth colour channel, which would widen
        // `blended` and leave this loop reading `cs[3]` off a three-element
        // array. That is a panic in a crate that forbids unsafe code, on any
        // page carrying `/BM /Luminosity`, reached by nothing in the suite
        // until `blend_modes.rs` grew the two tests that now cover it.
        for (i, out) in blended.iter_mut().enumerate().take(channels) {
            let (Some(cs), Some(mixed)) = (cs.get(i), mixed.get(i)) else {
                continue;
            };
            *out = mul255(255 - backdrop_alpha, *cs) + mul255(backdrop_alpha, *mixed);
        }
    }

    // The colour renormalises against the union alpha; the alpha channel keeps
    // the group's own. They are the same number outside a non-isolated group.
    let union_out = alpha + mul255(backdrop_alpha, inverse);
    let own_out = alpha + mul255(own_alpha, inverse);

    for (slot, mixed) in dst.iter_mut().zip(blended.iter()).take(channels) {
        let cb = u32::from(*slot);
        *slot = if backdrop_alpha == 255 {
            // The fast path, and the old arithmetic exactly.
            (mul255(*mixed, alpha) + mul255(cb, inverse)).min(255) as u8
        } else {
            let weighted = mul255(*mixed, alpha) + mul255(mul255(cb, backdrop_alpha), inverse);
            // A union alpha of zero means nothing is painted at this pixel at
            // all, so there is nothing to renormalise against and the result
            // is zero. Said as the division having no answer rather than as a
            // branch above, which keeps the rounding term beside the quotient
            // it rounds.
            (weighted * 255 + union_out / 2)
                .checked_div(union_out)
                .unwrap_or(0)
                .min(255) as u8
        };
    }

    if format.has_alpha() {
        if let Some(slot) = dst.get_mut(channels) {
            *slot = own_out.min(255) as u8;
        }
    }
}

/// [`blend`] over a [`PixelFormat::Plates`] pixel: `dst` holds `channels`
/// tints and an alpha after them, `source` a tint for each channel, and the
/// source composites at `alpha` under `mode` with `initial` the alpha of a
/// non-isolated group's initial backdrop (11.4.4), zero everywhere else.
///
/// Each channel is `CmykA8`'s arithmetic, line for line: the tint enters and
/// leaves the separable blend function complemented, since 11.3.5's formulas
/// are written for additive values and a tint is a quantity of ink; the
/// result is 11.3.6's straight-alpha composite against the union alpha, and
/// the alpha channel keeps the group's own. So with the same inputs a
/// process channel here is the byte the same channel of a `CmykA8` pixel
/// comes out as, under every mode.
///
/// **The spot channels take a mode only where it preserves white** (11.7.3,
/// 11.7.4.2): a spot colorant blends under a separable, white-preserving mode
/// — every separable mode but `Difference` and `Exclusion` — as a process
/// channel holding its tints would. Under those two the mode applies to the
/// process channels alone, and under the four non-separable modes it applies
/// to the process channels through light, as `CmykA8`'s `blend` takes them —
/// 8.6.4.4 out of ink, 11.3.5.3 over the light, maximum undercolour removal
/// back — while every spot channel takes `Normal`.
fn blend_plates(
    dst: &mut [u8],
    source: &[u8],
    alpha: u32,
    channels: usize,
    mode: BlendMode,
    initial: u32,
) {
    #[cfg(test)]
    blends::counted();
    if alpha == 0 {
        return;
    }
    let inverse = 255 - alpha;
    let own_alpha = dst.get(channels).map_or(255, |a| u32::from(*a));
    let backdrop_alpha = own_alpha + mul255(initial, 255 - own_alpha);
    // The colour renormalises against the union alpha; the alpha channel keeps
    // the group's own. They are the same number outside a non-isolated group.
    let union_out = alpha + mul255(backdrop_alpha, inverse);
    let own_out = alpha + mul255(own_alpha, inverse);
    // 11.3.5.3's modes are over light: the process channels' ink converted,
    // blended and converted back, from the pixel as it is before any channel
    // is written — `blend`'s round trip for `CmykA8`, counted by the caller.
    let lit = mode.is_nonseparable().then(|| {
        let at = |pixel: &[u8], i: usize| pixel.get(i).copied().unwrap_or(0);
        let (br, bg, bb) = cmyk_to_rgb(at(dst, 0), at(dst, 1), at(dst, 2), at(dst, 3));
        let (sr, sg, sb) = cmyk_to_rgb(at(source, 0), at(source, 1), at(source, 2), at(source, 3));
        let lit = mode.apply_nonseparable(
            [u32::from(br), u32::from(bg), u32::from(bb)],
            [u32::from(sr), u32::from(sg), u32::from(sb)],
        );
        let byte = |v: u32| v.min(255) as u8;
        let (c, m, y, k) = rgb_to_cmyk(byte(lit[0]), byte(lit[1]), byte(lit[2]));
        [c, m, y, k]
    });
    let preserves_white =
        !mode.is_nonseparable() && !matches!(mode, BlendMode::Difference | BlendMode::Exclusion);
    let spot_mode = if preserves_white {
        mode
    } else {
        BlendMode::Normal
    };
    for (channel, (slot, src)) in dst.iter_mut().zip(source).take(channels).enumerate() {
        let cs = u32::from(*src);
        let cb = u32::from(*slot);
        let blended = match lit {
            Some(lit) if channel < 4 => u32::from(lit.get(channel).copied().unwrap_or(0)),
            _ if channel < 4 => 255 - mode.apply(255 - cb, 255 - cs),
            _ => 255 - spot_mode.apply(255 - cb, 255 - cs),
        };
        let mixed = mul255(255 - backdrop_alpha, cs) + mul255(backdrop_alpha, blended);
        *slot = if backdrop_alpha == 255 {
            (mul255(mixed, alpha) + mul255(cb, inverse)).min(255) as u8
        } else {
            let weighted = mul255(mixed, alpha) + mul255(mul255(cb, backdrop_alpha), inverse);
            (weighted * 255 + union_out / 2)
                .checked_div(union_out)
                .unwrap_or(0)
                .min(255) as u8
        };
    }
    if let Some(slot) = dst.get_mut(channels) {
        *slot = own_out.min(255) as u8;
    }
}

/// Calls to `blend`, counted for the guards: compiled only under `cfg(test)`
/// and kept per thread, so tests running side by side cannot add into each
/// other's.
#[cfg(test)]
pub(crate) mod blends {
    use std::cell::Cell;

    std::thread_local! {
        static CALLS: Cell<u64> = const { Cell::new(0) };
    }

    /// One call.
    pub(crate) fn counted() {
        CALLS.with(|calls| calls.set(calls.get() + 1));
    }

    /// The calls counted on this thread since the last time, which starts
    /// the count again from zero.
    pub(crate) fn take() -> u64 {
        CALLS.with(|calls| calls.replace(0))
    }
}

#[cfg(test)]
mod tests {

    /// **A soft mask and a grey buffer weigh colour differently, on purpose.**
    ///
    /// 11.6.5.2's luminosity is 11.3.5.3's `Lum`, whose coefficients the
    /// specification gives as 0.3 / 0.59 / 0.11. `luma`'s are Rec.601's
    /// 0.299 / 0.587 / 0.114, and they are what a *grey buffer* stores a colour
    /// as — chosen so the weights sum to exactly 1000 and a grey survives the
    /// round trip.
    ///
    /// They differ in the third digit, which is one level on a saturated
    /// colour and nothing at all on the greys every existing fixture uses — so
    /// `to_mask` reaching for the wrong one moved no fingerprint and would have
    /// gone on not moving one. This is the assertion that noticed.
    #[test]
    fn a_soft_mask_uses_the_clauses_weights_and_a_grey_buffer_uses_luma() {
        let red = Color::rgb(255, 0, 0);
        assert_eq!(red.luminosity(), 77, "0.3 x 255, the clause's weight");
        assert_eq!(red.luma(), 76, "0.299 x 255, Rec.601's");

        // And the property that keeps `luma` where it is: a grey is itself.
        for v in [0u8, 1, 77, 128, 254, 255] {
            assert_eq!(Color::rgb(v, v, v).luma(), v, "grey {v} round-trips");
        }
    }

    /// **The device relation round-trips exactly**, which is the lemma the
    /// whole CMYK path rests on.
    ///
    /// `rgb_to_cmyk` chooses maximum undercolour removal — `K` takes as much of
    /// the grey as it can — and that choice is what makes the inverse exact
    /// rather than approximate. Each channel's intermediate error is bounded
    /// below half a level, so the round trip is the identity for every one of
    /// the sixteen million colours.
    ///
    /// Swept over `(v, max)` rather than over `(r, g, b)`: `K` is fixed by the
    /// maximum, and given `K` each channel is independent of the other two, so
    /// every pair here stands for a whole family of colours and 32 896 of them
    /// cover all 16 777 216.
    #[test]
    fn the_cmyk_round_trip_is_exact() {
        for max in 0..=255u8 {
            for v in 0..=max {
                let (c, _, _, k) = rgb_to_cmyk(v, max, 0);
                let (back, _, _) = cmyk_to_rgb(c, 0, 0, k);
                assert_eq!(
                    back, v,
                    "v {v} of max {max} came back as {back}: the inverse is not \
                     exact, and every CMYK group buffer shifts colour"
                );
            }
        }
    }

    /// Black is all `K`, and white is no ink at all.
    ///
    /// The two ends the general sweep cannot reach: black has no white to
    /// divide by, and 8.6.4.4's initial CMYK colour is `[0 0 0 1]` rather than
    /// all zeros, so an implementation that read "no ink" as "black" would have
    /// every group buffer start inverted.
    #[test]
    fn the_ends_of_the_ink_scale_are_where_they_should_be() {
        assert_eq!(rgb_to_cmyk(0, 0, 0), (0, 0, 0, 255), "black is all K");
        assert_eq!(rgb_to_cmyk(255, 255, 255), (0, 0, 0, 0), "white is no ink");
        assert_eq!(cmyk_to_rgb(0, 0, 0, 255), (0, 0, 0));
        assert_eq!(cmyk_to_rgb(0, 0, 0, 0), (255, 255, 255));
        assert_eq!(cmyk_to_rgb(255, 0, 0, 0), (0, 255, 255), "cyan");
    }

    /// **A same-format copy is the bytes it started as**, which is the identity
    /// `Canvas::source_from`'s fast path rests on.
    ///
    /// Asserted directly rather than by comparing the two paths, because there
    /// is no toggle between them and adding one would be a second code path to
    /// keep in step. What is checked is the property instead: for every format,
    /// the bytes a pixel is stored as are exactly what `encode(pixel(..))`
    /// produces, so reading them straight across cannot differ from the long
    /// way round.
    ///
    /// `Gray8` is the interesting row. Its long way round replicates the grey
    /// into three channels and takes `luma` of them again, and that survives
    /// only because the weights sum to exactly 1000 — 299 + 587 + 114. A
    /// weighting that did not would make the fast path a behaviour change
    /// rather than an optimisation, and this test is where that would show.
    #[test]
    fn a_same_format_copy_is_the_bytes_it_started_as() {
        let colours = [
            Color::rgb(0, 0, 0),
            Color::rgb(255, 255, 255),
            Color::rgb(200, 60, 20),
            Color::rgb(17, 17, 17),
            Color {
                r: 40,
                g: 90,
                b: 200,
                a: 128,
            },
            Color::TRANSPARENT,
        ];
        for format in [
            PixelFormat::Gray8,
            PixelFormat::GrayA8,
            PixelFormat::Rgb8,
            PixelFormat::Rgba8,
            PixelFormat::Bgr8,
            PixelFormat::Bgra8,
        ] {
            for colour in colours {
                let canvas = Canvas::new(1, 1, format, colour);
                let components = format.components();
                let stored = &canvas.data[..components];
                let long_way = canvas.pixel(0, 0).map(|c| canvas.encode(c));
                let long_way = long_way.expect("a pixel");
                assert_eq!(
                    stored,
                    &long_way[..components],
                    "{format:?} at {colour:?}: the stored bytes and the \
                     encode-of-pixel round trip disagree, so a same-format copy \
                     is not a copy"
                );
            }
        }
    }

    /// Every blend mode 11.3.5 names.
    const MODES: [BlendMode; 16] = [
        BlendMode::Normal,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Overlay,
        BlendMode::Darken,
        BlendMode::Lighten,
        BlendMode::ColorDodge,
        BlendMode::ColorBurn,
        BlendMode::HardLight,
        BlendMode::SoftLight,
        BlendMode::Difference,
        BlendMode::Exclusion,
        BlendMode::Hue,
        BlendMode::Saturation,
        BlendMode::Color,
        BlendMode::Luminosity,
    ];

    /// A 5×4 cell in `format`: colours of its own, which a tint must hide,
    /// under alphas that run from none through partial to full.
    fn tinted_cell(format: PixelFormat) -> Canvas {
        let mut cell = Canvas::new(5, 4, format, Color::TRANSPARENT);
        let components = format.components();
        let channels = color_channels(format);
        let alphas = [0u8, 1, 37, 128, 200, 254, 255];
        for pixel in 0..20usize {
            let base = pixel * components;
            for channel in 0..channels {
                cell.data[base + channel] = (pixel * 53 + channel * 91 + 17) as u8;
            }
            if format.has_alpha() {
                cell.data[base + channels] = alphas[pixel % alphas.len()];
            }
        }
        cell
    }

    /// **A tinted composite is a recoloured composite.**
    ///
    /// For a cell in every format that carries alpha, onto a canvas of every
    /// format — an opaque page, a half-transparent buffer, and a non-isolated
    /// group's buffer over its backdrop — under every blend mode, at full,
    /// partial, zero and non-finite alpha, with no mask and with one that
    /// covers in part, and in three colours, compositing the cell through a
    /// colour writes exactly the bytes `recolor` of the cell and then
    /// `composite` wrote, and counts the same approximated blends. A cell with
    /// no alpha channel, which `recolor` leaves alone, composites its own
    /// colours either way.
    #[test]
    fn a_tinted_composite_is_a_recoloured_composite() {
        let all = [
            PixelFormat::Gray8,
            PixelFormat::GrayA8,
            PixelFormat::Rgb8,
            PixelFormat::Rgba8,
            PixelFormat::CmykA8,
            PixelFormat::LabA8,
        ];
        let colours = [
            Color::rgb(200, 60, 20),
            Color::rgb(0, 0, 0),
            Color::rgb(30, 160, 255),
        ];
        let mut partial = Mask::empty(0, 0, 7, 6);
        for (index, value) in partial.data.iter_mut().enumerate() {
            *value = (index * 37 % 256) as u8;
        }
        let mut compared = 0usize;
        for cell_format in all {
            let cell = tinted_cell(cell_format);
            for format in all {
                let page = Canvas::new(7, 6, format, Color::rgb(90, 140, 210));
                let mut targets = vec![("an opaque page", page.clone())];
                if format.has_alpha() {
                    let half = Color {
                        r: 90,
                        g: 140,
                        b: 210,
                        a: 128,
                    };
                    targets.push(("a half-transparent buffer", Canvas::new(7, 6, format, half)));
                    let mut group = Canvas::new(7, 6, format, Color::TRANSPARENT);
                    group.adopt_backdrop(page.clone());
                    targets.push(("a non-isolated group", group));
                }
                for (what, target) in &targets {
                    for mode in MODES {
                        for alpha in [1.0, 0.6, 0.0, f64::NAN] {
                            for mask in [None, Some(&partial)] {
                                for colour in colours {
                                    let mut tinted = target.clone();
                                    tinted.composite_tinted(
                                        &cell,
                                        colour,
                                        (1, 1),
                                        alpha,
                                        mode,
                                        mask,
                                        None,
                                    );
                                    let mut recoloured = cell.clone();
                                    recoloured.recolor(colour);
                                    let mut expected = target.clone();
                                    expected.composite(
                                        &recoloured,
                                        (1, 1),
                                        alpha,
                                        mode,
                                        mask,
                                        None,
                                    );
                                    let case = format!(
                                        "a {cell_format:?} cell onto {what} in {format:?}, \
                                         {mode:?}, alpha {alpha}, mask {}, {colour:?}",
                                        mask.is_some()
                                    );
                                    assert_eq!(tinted.data, expected.data, "{case}");
                                    assert_eq!(
                                        tinted.approximated_blends, expected.approximated_blends,
                                        "{case}"
                                    );
                                    compared += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(compared > 0);

        // A cell carrying no alpha: `recolor` leaves it, so both composite
        // its own colours, and the tint shows nowhere.
        let opaque = tinted_cell(PixelFormat::Rgb8);
        let page = Canvas::new(7, 6, PixelFormat::Rgb8, Color::WHITE);
        let mut tinted = page.clone();
        tinted.composite_tinted(
            &opaque,
            Color::rgb(255, 0, 0),
            (1, 1),
            1.0,
            BlendMode::Normal,
            None,
            None,
        );
        let mut plain = page;
        plain.composite(&opaque, (1, 1), 1.0, BlendMode::Normal, None, None);
        assert_eq!(tinted.data, plain.data);
        assert_eq!(tinted.pixel(1, 1), opaque.pixel(0, 0));
    }

    /// And the copy itself, end to end: `extract` of a whole canvas in its own
    /// format reproduces it byte for byte.
    #[test]
    fn extracting_a_canvas_in_its_own_format_reproduces_it() {
        for format in [
            PixelFormat::Gray8,
            PixelFormat::GrayA8,
            PixelFormat::Rgb8,
            PixelFormat::Rgba8,
            PixelFormat::Bgr8,
            PixelFormat::Bgra8,
        ] {
            let mut canvas = Canvas::new(4, 3, format, Color::rgb(10, 20, 30));
            canvas.blend_pixel(1, 1, Color::rgb(200, 60, 20), 1.0);
            canvas.blend_pixel(
                2,
                2,
                Color {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 128,
                },
                0.5,
            );
            let copy = canvas.extract((0, 0), 4, 3, format);
            assert_eq!(copy.data, canvas.data, "{format:?}");
        }
    }

    /// The two blue-first orders, extracted from a red-first canvas, are its
    /// colours with red and blue exchanged and alpha where it was, and
    /// extracting them back gives the red-first bytes: the reorder a page
    /// asked for in `Bgr8` or `Bgra8` is made by, and nothing else.
    #[test]
    fn a_blue_first_extract_is_the_same_colours_reordered() {
        let mut canvas = Canvas::new(4, 3, PixelFormat::Rgba8, Color::TRANSPARENT);
        canvas.blend_pixel(1, 1, Color::rgb(200, 60, 20), 1.0);
        canvas.blend_pixel(
            2,
            2,
            Color {
                r: 10,
                g: 90,
                b: 240,
                a: 128,
            },
            1.0,
        );
        canvas.blend_pixel(3, 0, Color::rgb(1, 2, 3), 0.5);
        for (blue_first, red_first) in [
            (PixelFormat::Bgra8, PixelFormat::Rgba8),
            (PixelFormat::Bgr8, PixelFormat::Rgb8),
        ] {
            let ordered = canvas.extract((0, 0), 4, 3, blue_first);
            let straight = canvas.extract((0, 0), 4, 3, red_first);
            let n = red_first.components();
            assert_eq!(ordered.format.components(), n, "{blue_first:?}");
            assert_eq!(ordered.format.has_alpha(), red_first.has_alpha());
            for (b, s) in ordered
                .data
                .chunks_exact(n)
                .zip(straight.data.chunks_exact(n))
            {
                assert_eq!([b[2], b[1], b[0]], [s[0], s[1], s[2]], "{blue_first:?}");
                assert_eq!(b[3..], s[3..], "{blue_first:?}: the alpha moved");
            }
            let back = ordered.extract((0, 0), 4, 3, red_first);
            assert_eq!(
                back.data, straight.data,
                "{blue_first:?} back to {red_first:?}"
            );
            for y in 0..3 {
                for x in 0..4 {
                    assert_eq!(ordered.pixel(x, y), straight.pixel(x, y));
                }
            }
        }
    }

    /// **Black generation and undercolour removal are 10.4.2.4's formula**:
    /// with the identity for both, every light is `c − k` of ink and `k` of
    /// black; with no black generation and no removal it is all ink and no
    /// black; a negative removal adds ink. An ink canvas holding tables
    /// encodes light by them, a colour's own ink is untouched, and putting
    /// the device's own back restores maximum removal, byte for byte.
    #[test]
    fn black_generation_and_undercolour_removal_are_the_clauses_formula() {
        let identity = {
            let mut t = InkTables {
                black: [0; 256],
                removal: [0; 256],
            };
            for k in 0..256 {
                t.black[k] = k as u8;
                t.removal[k] = k as i16;
            }
            t
        };
        let none = InkTables {
            black: [0; 256],
            removal: [0; 256],
        };
        let adding = InkTables {
            black: [0; 256],
            removal: [-40; 256],
        };
        // (r, g, b): c = 255 − r and the rest, k = min(c, m, y).
        for (r, g, b) in [
            (255, 255, 255),
            (0, 0, 0),
            (64, 128, 192),
            (250, 10, 90),
            (128, 128, 128),
        ] {
            let (c, m, y) = (255 - r as i32, 255 - g as i32, 255 - b as i32);
            let k = c.min(m).min(y);
            let byte = |v: i32| v.clamp(0, 255) as u8;
            assert_eq!(
                identity.ink(r, g, b),
                (byte(c - k), byte(m - k), byte(y - k), byte(k)),
                "identity at ({r}, {g}, {b})"
            );
            assert_eq!(
                none.ink(r, g, b),
                (byte(c), byte(m), byte(y), 0),
                "no black at ({r}, {g}, {b})"
            );
            assert_eq!(
                adding.ink(r, g, b),
                (byte(c + 40), byte(m + 40), byte(y + 40), 0),
                "a negative removal at ({r}, {g}, {b})"
            );
        }

        let mask = square_mask(1.0, 1.0, 4.0, 4.0, 6);
        let light = Color::rgb(64, 128, 192);
        let mut maximum = Canvas::new(6, 6, PixelFormat::CmykA8, Color::WHITE);
        maximum.fill_mask(&mask, light, 1.0);
        let mut generated = Canvas::new(6, 6, PixelFormat::CmykA8, Color::WHITE);
        let tables = std::sync::Arc::new(identity.clone());
        assert_eq!(generated.set_ink_tables(Some(tables)), None);
        generated.fill_mask(&mask, light, 1.0);
        let (c, m, y, k) = identity.ink(64, 128, 192);
        assert_eq!(&generated.data[(2 * 6 + 2) * 5..][..5], &[c, m, y, k, 255]);
        assert_ne!(generated.data, maximum.data, "the tables changed nothing");
        // An uncoloured cell's tint has no ink of its own either: composited
        // onto the canvas the tables are in force on, it is their ink.
        let mut cell = Canvas::new(6, 6, PixelFormat::CmykA8, Color::TRANSPARENT);
        cell.fill_mask(&mask, Color::BLACK, 1.0);
        let mut tinted = Canvas::new(6, 6, PixelFormat::CmykA8, Color::WHITE);
        tinted.set_ink_tables(Some(std::sync::Arc::new(identity.clone())));
        tinted.composite_tinted(&cell, light, (0, 0), 1.0, BlendMode::Normal, None, None);
        assert_eq!(tinted.data, generated.data, "the tint took no tables");
        let mut untinted = Canvas::new(6, 6, PixelFormat::CmykA8, Color::WHITE);
        untinted.composite_tinted(&cell, light, (0, 0), 1.0, BlendMode::Normal, None, None);
        assert_eq!(untinted.data, maximum.data, "the tint without tables");
        // A colour's own ink is the document's, whatever is in force.
        let mut owned = generated.clone();
        owned.fill_mask_inked(&mask, light, Some([1, 2, 3, 4]), 1.0, BlendMode::Normal);
        assert_eq!(&owned.data[(2 * 6 + 2) * 5..][..5], &[1, 2, 3, 4, 255]);
        // And the device's own back is maximum removal again.
        assert!(generated.set_ink_tables(None).is_some());
        let mut again = Canvas::new(6, 6, PixelFormat::CmykA8, Color::WHITE);
        again.set_ink_tables(None);
        again.fill_mask(&mask, light, 1.0);
        assert_eq!(again.data, maximum.data);
        // A light canvas never reads them.
        let mut rgb = Canvas::new(6, 6, PixelFormat::Rgb8, Color::WHITE);
        let mut plain = rgb.clone();
        rgb.set_ink_tables(Some(std::sync::Arc::new(identity)));
        rgb.fill_mask(&mask, light, 1.0);
        plain.fill_mask(&mask, light, 1.0);
        assert_eq!(rgb.data, plain.data);
    }

    /// **A blue-first canvas composites as the red-first one does,
    /// reordered**, under every blend mode: through a mask, a sampled pixel and
    /// a composite of another canvas, at full and half alpha, over an opaque
    /// backdrop and a transparent one. The separable modes are per channel and
    /// cannot see an order; the non-separable ones (11.3.5.3) can, because
    /// `Lum` weighs red, green and blue unequally, and every colour here has
    /// unequal red and blue, so a pixel handed over in its stored order would
    /// part from the red-first canvas there.
    #[test]
    fn a_bgr_canvas_composites_as_the_rgb_canvas_reordered() {
        let modes = [
            BlendMode::Normal,
            BlendMode::Multiply,
            BlendMode::Screen,
            BlendMode::Overlay,
            BlendMode::Darken,
            BlendMode::Lighten,
            BlendMode::ColorDodge,
            BlendMode::ColorBurn,
            BlendMode::HardLight,
            BlendMode::SoftLight,
            BlendMode::Difference,
            BlendMode::Exclusion,
            BlendMode::Hue,
            BlendMode::Saturation,
            BlendMode::Color,
            BlendMode::Luminosity,
        ];
        let reordered = |data: &[u8], n: usize| -> Vec<u8> {
            data.chunks_exact(n)
                .flat_map(|pixel| {
                    let mut pixel = pixel.to_vec();
                    pixel.swap(0, 2);
                    pixel
                })
                .collect()
        };
        let mask = square_mask(2.3, 1.7, 9.4, 8.2, 12);
        let paint = Color {
            r: 200,
            g: 70,
            b: 20,
            a: 230,
        };
        let sampled = Color {
            r: 15,
            g: 240,
            b: 90,
            a: 200,
        };
        let mut source = Canvas::new(5, 5, PixelFormat::Rgba8, Color::TRANSPARENT);
        source.fill_mask(
            &square_mask(0.4, 0.6, 3.7, 3.1, 5),
            Color::rgb(90, 10, 250),
            1.0,
        );
        let backdrop = Color::rgb(30, 160, 220);
        for (red_first, blue_first, starts) in [
            (PixelFormat::Rgb8, PixelFormat::Bgr8, backdrop),
            (PixelFormat::Rgba8, PixelFormat::Bgra8, backdrop),
            (PixelFormat::Rgba8, PixelFormat::Bgra8, Color::TRANSPARENT),
        ] {
            for mode in modes {
                for alpha in [1.0, 0.5] {
                    let mut rgb = Canvas::new(12, 12, red_first, starts);
                    let mut bgr = Canvas::new(12, 12, blue_first, starts);
                    for canvas in [&mut rgb, &mut bgr] {
                        canvas.fill_mask_with(&mask, paint, alpha, mode);
                        canvas.blend_pixel_with(3, 4, sampled, alpha, mode);
                        canvas.composite(&source, (6, 5), alpha, mode, None, None);
                    }
                    let n = red_first.components();
                    assert_eq!(
                        bgr.data,
                        reordered(&rgb.data, n),
                        "{blue_first:?} over {starts:?}, {mode:?} at {alpha}"
                    );
                    for y in 0..12 {
                        for x in 0..12 {
                            assert_eq!(bgr.pixel(x, y), rgb.pixel(x, y), "({x}, {y})");
                        }
                    }
                }
            }
        }
    }
    use super::*;
    use crate::fill::fill;
    use crate::geom::{FillRule, Path};

    fn square_mask(x: f64, y: f64, w: f64, h: f64, size: u32) -> Mask {
        let mut path = Path::new();
        path.rect(x, y, w, h);
        fill(&path, FillRule::NonZero, 0, 0, size, size, 0.05, None)
    }

    /// The formats an opaque Normal fill can be taken on: all but ink.
    const OPAQUE_FORMATS: [PixelFormat; 7] = [
        PixelFormat::Gray8,
        PixelFormat::GrayA8,
        PixelFormat::Rgb8,
        PixelFormat::Rgba8,
        PixelFormat::LabA8,
        PixelFormat::Bgr8,
        PixelFormat::Bgra8,
    ];

    /// **The opaque Normal path is `blend`, byte for byte, wherever it is
    /// taken** (P-07.6).
    ///
    /// Exhaustive over coverage, source channel and destination channel on
    /// every format its conditions can hold on, one opaque pixel at a time:
    /// the pixel the fill writes at each coverage against `blend` at the same
    /// coverage with no initial backdrop, every colour channel at once. Then
    /// whole fills, the path against the general composite on the same
    /// canvas, mask and colour: canvases standing away from the origin, pixels
    /// of every alpha — opaque, empty and between — runs of full coverage, and
    /// masks hanging off every side.
    #[test]
    fn the_opaque_path_is_the_general_blend_where_it_applies() {
        for format in OPAQUE_FORMATS {
            let components = format.components();
            let channels = color_channels(format);
            for cs in 0..=255u8 {
                let mut source = [cs; 5];
                if format.has_alpha() {
                    source[channels] = 255;
                }
                for cb in 0..=255u8 {
                    let mut backdrop = [cb; 5];
                    if format.has_alpha() {
                        backdrop[channels] = 255;
                    }
                    for c in 0..=255u8 {
                        let (mut want, mut got) = (backdrop, backdrop);
                        blend(
                            want.get_mut(..components),
                            &source,
                            u32::from(c),
                            format,
                            BlendMode::Normal,
                            0,
                        );
                        if c != 0 {
                            opaque_pixel(&mut got[..components], &source, c, format);
                        }
                        assert_eq!(got, want, "{format:?}: {cs} over {cb} at {c}");
                    }
                }
            }
        }

        // Whole fills, through both composites.
        let mut state = 0x9e37_79b9_u32;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state
        };
        for format in OPAQUE_FORMATS {
            let channels = color_channels(format);
            for trial in 0..60 {
                let origin = ((next() % 9) as i32 - 4, (next() % 9) as i32 - 4);
                let mut canvas =
                    Canvas::new(37, 23, format, Color::WHITE).at_origin(origin.0, origin.1);
                for pixel in canvas.data.chunks_exact_mut(format.components()) {
                    for byte in pixel.iter_mut() {
                        *byte = next() as u8;
                    }
                    if format.has_alpha() {
                        pixel[channels] = match next() % 4 {
                            0 => 0,
                            1 => next() as u8,
                            _ => 255,
                        };
                    }
                }
                let (width, height) = (5 + next() % 40, 3 + next() % 30);
                let mut data = Vec::with_capacity((width * height) as usize);
                while data.len() < (width * height) as usize {
                    let value = match next() % 3 {
                        0 => 255,
                        1 => 0,
                        _ => next() as u8,
                    };
                    for _ in 0..1 + next() % 12 {
                        data.push(value);
                    }
                }
                data.truncate((width * height) as usize);
                let mask = Mask {
                    x0: (next() % 50) as i32 - 12,
                    y0: (next() % 36) as i32 - 10,
                    width,
                    height,
                    data,
                    outside: 0,
                };
                let mut source = [0u8; 5];
                for byte in &mut source {
                    *byte = next() as u8;
                }
                if format.has_alpha() {
                    source[channels] = 255;
                }
                let rect = mask.overlap_at(canvas.origin(), canvas.width, canvas.height);
                let mut general = canvas.clone();
                canvas.fill_opaque(&mask, &source, rect);
                general.fill_blended(&mask, &source, 255, 255, BlendMode::Normal, rect);
                assert_eq!(canvas.data, general.data, "{format:?}, trial {trial}");
            }
        }
    }

    /// **An opaque Normal fill onto an opaque canvas calls no general
    /// `blend`** (P-07.6's guard), counted; and every case outside the path's
    /// conditions still does — an alpha below 255 in the state or in the
    /// colour, another mode, and, at the pixels the shape covers in part, a
    /// canvas that is not opaque, the ink canvas and a non-isolated group's
    /// backdrop.
    #[test]
    fn an_opaque_normal_fill_calls_no_general_blend() {
        let mask = square_mask(3.3, 4.6, 50.2, 47.9, 64);
        let colour = Color::rgb(200, 30, 90);
        for format in OPAQUE_FORMATS {
            let mut canvas = Canvas::new(64, 64, format, Color::WHITE);
            blends::take();
            canvas.fill_mask(&mask, colour, 1.0);
            assert_eq!(blends::take(), 0, "{format:?} blended");
            assert_ne!(canvas.pixel(30, 30), Some(Color::WHITE), "{format:?} drew");
        }

        let blended = |canvas: &mut Canvas, colour: Color, alpha: f64, mode: BlendMode| {
            blends::take();
            canvas.fill_mask_with(&mask, colour, alpha, mode);
            blends::take()
        };
        let opaque = || Canvas::new(64, 64, PixelFormat::Rgba8, Color::WHITE);
        let half = Color { a: 128, ..colour };
        assert!(blended(&mut opaque(), colour, 0.5, BlendMode::Normal) > 0);
        assert!(blended(&mut opaque(), half, 1.0, BlendMode::Normal) > 0);
        assert!(blended(&mut opaque(), colour, 1.0, BlendMode::Multiply) > 0);
        let mut clear = Canvas::new(64, 64, PixelFormat::Rgba8, Color::TRANSPARENT);
        assert!(blended(&mut clear, colour, 1.0, BlendMode::Normal) > 0);
        let mut ink = Canvas::new(64, 64, PixelFormat::CmykA8, Color::WHITE);
        assert!(blended(&mut ink, colour, 1.0, BlendMode::Normal) > 0);
        let mut group = Canvas::new(64, 64, PixelFormat::Rgba8, Color::TRANSPARENT);
        group.adopt_backdrop(opaque());
        assert!(blended(&mut group, colour, 1.0, BlendMode::Normal) > 0);
    }

    /// `mul255` in sixteen bits is `mul255`, over every pair of bytes.
    #[test]
    fn mul255_in_sixteen_bits_is_mul255() {
        for a in 0..=255u16 {
            for b in 0..=255u16 {
                assert_eq!(
                    u32::from(mul255_narrow(a, b)),
                    mul255(u32::from(a), u32::from(b)),
                    "{a} x {b}"
                );
            }
        }
    }

    /// One block of opaque pixels through [`opaque_block`] and the same
    /// block a pixel at a time through [`opaque_pixel`], from the same bytes.
    fn block_both_ways<const N: usize>(
        format: PixelFormat,
        block: &[u8],
        cover: &[u8],
        source: &[u8; 5],
    ) -> (Vec<u8>, Vec<u8>) {
        let mut solid = Vec::new();
        for _ in 0..BLOCK {
            solid.extend_from_slice(&source[..N]);
        }
        let mut whole = block.to_vec();
        opaque_block::<N>(&mut whole, cover, &solid);
        let mut by_pixel = block.to_vec();
        each_opaque_pixel::<N>(&mut by_pixel, cover, source, format);
        (whole, by_pixel)
    }

    /// The block test of [`an_opaque_block_is_the_composite_a_pixel_at_a_time`]
    /// for one format of `N` bytes a pixel.
    fn blocks_of<const N: usize>(format: PixelFormat, next: &mut impl FnMut() -> u32) -> u32 {
        let channels = color_channels(format);
        let mut ran = 0;
        // Every coverage at every source byte, sixteen coverages a block,
        // over destination bytes that differ by pixel and by channel.
        for base in 0..16u32 {
            for cs in 0..=255u32 {
                let mut source = [cs as u8; 5];
                source[channels] = 255;
                let cover: Vec<u8> = (0..16).map(|i| (base * 16 + i) as u8).collect();
                let mut block = Vec::with_capacity(BLOCK * N);
                for i in 0..BLOCK as u32 {
                    for k in 0..N as u32 {
                        block.push(((cs * 7 + i * 31 + k * 13 + base) % 256) as u8);
                    }
                    if format.has_alpha() {
                        if let Some(alpha) = block.last_mut() {
                            *alpha = 255;
                        }
                    }
                }
                let (whole, by_pixel) = block_both_ways::<N>(format, &block, &cover, &source);
                assert_eq!(
                    whole,
                    by_pixel,
                    "{format:?}: source {cs}, coverages from {}",
                    base * 16
                );
                ran += 1;
            }
        }
        // And random blocks: random colours over random opaque pixels, the
        // coverage drawn from none, full and between.
        for trial in 0..2000 {
            let mut source = [0u8; 5];
            for byte in &mut source {
                *byte = next() as u8;
            }
            source[channels] = 255;
            let cover: Vec<u8> = (0..BLOCK)
                .map(|_| match next() % 3 {
                    0 => 0,
                    1 => 255,
                    _ => next() as u8,
                })
                .collect();
            let mut block: Vec<u8> = (0..BLOCK * N).map(|_| next() as u8).collect();
            if format.has_alpha() {
                for pixel in block.chunks_exact_mut(N) {
                    pixel[channels] = 255;
                }
            }
            let (whole, by_pixel) = block_both_ways::<N>(format, &block, &cover, &source);
            assert_eq!(whole, by_pixel, "{format:?}, trial {trial}");
            ran += 1;
        }
        ran
    }

    /// **A block of opaque pixels is the composite a pixel at a time**
    /// (P-07.9a): the collapse at every byte, colour and alpha, in sixteen
    /// bits, writes what [`opaque_pixel`] writes pixel by pixel — the colour
    /// at full coverage, the pixel untouched at none, and [`over_opaque`]
    /// between — on every format the path is taken on, at every coverage and
    /// source byte, over destinations of every byte.
    #[test]
    fn an_opaque_block_is_the_composite_a_pixel_at_a_time() {
        let mut state = 0x0b10_c4ed_u32;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state
        };
        let mut ran = 0;
        ran += blocks_of::<1>(PixelFormat::Gray8, &mut next);
        ran += blocks_of::<2>(PixelFormat::GrayA8, &mut next);
        ran += blocks_of::<3>(PixelFormat::Rgb8, &mut next);
        ran += blocks_of::<4>(PixelFormat::Rgba8, &mut next);
        ran += blocks_of::<4>(PixelFormat::LabA8, &mut next);
        assert_eq!(
            ran,
            5 * (16 * 256 + 2000),
            "every block of every format ran"
        );
    }

    /// **The row walk takes every block and every tail as the general
    /// blend does, and blends exactly the pixels it must** (P-07.9a).
    ///
    /// Rows of every width from one pixel to past three blocks, so every
    /// count of whole blocks and every length of tail is met, each block's
    /// coverage none, full, mixed or random and its pixels all opaque, holding
    /// one pixel that is not, or of random alpha: the opaque path against the
    /// general composite on the same canvas, byte for byte, and the general
    /// `blend` called once for each pixel partly covered that is not opaque
    /// and never otherwise — a pixel of full coverage takes the colour
    /// whatever it held (P-25.4).
    #[test]
    fn the_row_walk_takes_every_block_and_tail_as_the_general_blend() {
        let mut state = 0x7a11_5eed_u32;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state
        };
        let mut tails = [false; BLOCK];
        for format in OPAQUE_FORMATS {
            let components = format.components();
            let channels = color_channels(format);
            for width in 1..=(3 * BLOCK as u32 + BLOCK as u32 / 2) {
                tails[width as usize % BLOCK] = true;
                for trial in 0..12 {
                    let mut canvas = Canvas::new(width, 2, format, Color::WHITE).at_origin(-3, 5);
                    let mut data = Vec::with_capacity(width as usize * 2);
                    for row in 0..2u32 {
                        for x in 0..width {
                            let block = (x as usize / BLOCK + row as usize + trial) % 4;
                            data.push(match block {
                                0 => 0,
                                1 => 255,
                                2 => [0, 255, 128, 1, 254][(x as usize + trial) % 5],
                                _ => next() as u8,
                            });
                        }
                    }
                    for (index, pixel) in canvas.data.chunks_exact_mut(components).enumerate() {
                        for byte in pixel.iter_mut() {
                            *byte = next() as u8;
                        }
                        if format.has_alpha() {
                            let block = index / BLOCK;
                            pixel[channels] = match (trial % 3, block % 2) {
                                (0, _) => 255,
                                (1, 0) if index % BLOCK == 7 => next() as u8 % 255,
                                (1, _) => 255,
                                _ => match next() % 3 {
                                    0 => 0,
                                    1 => next() as u8,
                                    _ => 255,
                                },
                            };
                        }
                    }
                    let mask = Mask {
                        x0: -3,
                        y0: 5,
                        width,
                        height: 2,
                        data,
                        outside: 0,
                    };
                    let mut source = [0u8; 5];
                    for byte in &mut source {
                        *byte = next() as u8;
                    }
                    if format.has_alpha() {
                        source[channels] = 255;
                    }
                    let owed = if format.has_alpha() {
                        canvas
                            .data
                            .chunks_exact(components)
                            .zip(&mask.data)
                            .filter(|(pixel, c)| **c != 0 && **c != 255 && pixel[channels] != 255)
                            .count() as u64
                    } else {
                        0
                    };
                    let rect = mask.overlap_at(canvas.origin(), canvas.width, canvas.height);
                    let mut general = canvas.clone();
                    blends::take();
                    canvas.fill_opaque(&mask, &source, rect);
                    assert_eq!(
                        blends::take(),
                        owed,
                        "{format:?}, width {width}, trial {trial}: general blends"
                    );
                    general.fill_blended(&mask, &source, 255, 255, BlendMode::Normal, rect);
                    assert_eq!(
                        canvas.data, general.data,
                        "{format:?}, width {width}, trial {trial}"
                    );
                }
            }
        }
        assert!(tails.iter().all(|met| *met), "every tail length was met");
    }

    #[test]
    fn a_canvas_starts_at_its_background() {
        let canvas = Canvas::new(4, 3, PixelFormat::Rgb8, Color::WHITE);
        assert_eq!(canvas.data.len(), 4 * 3 * 3);
        for y in 0..3 {
            for x in 0..4 {
                assert_eq!(canvas.pixel(x, y), Some(Color::WHITE));
            }
        }
    }

    #[test]
    fn full_coverage_replaces_the_pixel() {
        let mut canvas = Canvas::new(8, 8, PixelFormat::Rgb8, Color::WHITE);
        canvas.fill_mask(&square_mask(2.0, 2.0, 4.0, 4.0, 8), Color::BLACK, 1.0);

        assert_eq!(canvas.pixel(3, 3), Some(Color::BLACK), "inside");
        assert_eq!(canvas.pixel(0, 0), Some(Color::WHITE), "outside");
    }

    /// An ink canvas composites a colour's own ink when it is handed one, and
    /// light turned into ink when it is not; any other canvas never reads the
    /// ink. Black light becomes black ink alone — maximum undercolour removal
    /// — where the ink it was chosen as may be all four.
    #[test]
    fn an_ink_canvas_composites_the_ink_it_is_handed() {
        let mask = square_mask(0.0, 0.0, 4.0, 4.0, 4);
        let rich = Some([255, 255, 255, 255]);
        let at = |canvas: &Canvas| canvas.data[..5].to_vec();

        let mut ink = Canvas::new(4, 4, PixelFormat::CmykA8, Color::TRANSPARENT);
        ink.fill_mask_inked(&mask, Color::BLACK, rich, 1.0, BlendMode::Normal);
        assert_eq!(at(&ink), [255, 255, 255, 255, 255], "the four inks");

        let mut light = Canvas::new(4, 4, PixelFormat::CmykA8, Color::TRANSPARENT);
        light.fill_mask_inked(&mask, Color::BLACK, None, 1.0, BlendMode::Normal);
        assert_eq!(at(&light), [0, 0, 0, 255, 255], "black light is K");

        let mut rgb = Canvas::new(4, 4, PixelFormat::Rgba8, Color::WHITE);
        rgb.fill_mask_inked(&mask, Color::BLACK, rich, 1.0, BlendMode::Normal);
        assert_eq!(&rgb.data[..4], &[0, 0, 0, 255], "light reads no ink");
    }

    /// A non-isolated group over ink starts from the backdrop's own ink and
    /// takes the same ink out again (11.4.4, 11.4.7.2). A rich black is a value
    /// `rgb_to_cmyk` never produces, so a backdrop copied through light came
    /// in as pure K, and the removal step, reading it the same way, recovered
    /// a colour the group never painted.
    #[test]
    fn a_backdrop_of_ink_is_adopted_and_removed_as_that_ink() {
        let mask = square_mask(0.0, 0.0, 4.0, 4.0, 4);
        let mut page = Canvas::new(4, 4, PixelFormat::CmykA8, Color::WHITE);
        page.fill_mask_inked(
            &mask,
            Color::BLACK,
            Some([255, 255, 255, 255]),
            1.0,
            BlendMode::Normal,
        );

        let mut group = Canvas::new(4, 4, PixelFormat::CmykA8, Color::TRANSPARENT);
        group.adopt_backdrop(page);
        assert_eq!(
            group.data[..5],
            [255, 255, 255, 255, 0],
            "the rich black, with none of the group's own alpha"
        );

        // Half of magenta, then the backdrop out: what is left is the group's
        // own colour, magenta, whatever it was painted over.
        let magenta = Color::rgb(255, 0, 255);
        group.fill_mask_inked(&mask, magenta, Some([0, 255, 0, 0]), 0.5, BlendMode::Normal);
        let page = group.release_parent().expect("the group holds its page");
        group.remove_backdrop(&page);
        assert_eq!(
            page.data[..5],
            [255, 255, 255, 255, 255],
            "the page is its own"
        );
        let own = &group.data[..4];
        assert!(
            own.iter()
                .zip([0u8, 255, 0, 0])
                .all(|(got, want)| got.abs_diff(want) <= 3),
            "the group's own ink is magenta, got {own:?}"
        );
    }

    #[test]
    fn half_alpha_lands_halfway() {
        let mut canvas = Canvas::new(4, 4, PixelFormat::Rgb8, Color::WHITE);
        canvas.fill_mask(&square_mask(0.0, 0.0, 4.0, 4.0, 4), Color::BLACK, 0.5);

        let px = canvas.pixel(1, 1).expect("a pixel");
        assert!(
            (126..=130).contains(&px.r),
            "black at half alpha over white is mid grey, got {}",
            px.r
        );
    }

    #[test]
    fn grayscale_stores_one_component() {
        let mut canvas = Canvas::new(4, 4, PixelFormat::Gray8, Color::WHITE);
        assert_eq!(canvas.data.len(), 16);
        canvas.fill_mask(&square_mask(0.0, 0.0, 4.0, 4.0, 4), Color::BLACK, 1.0);
        assert_eq!(canvas.pixel(2, 2).map(|c| c.r), Some(0));
    }

    #[test]
    fn alpha_formats_accumulate_opacity() {
        let mut canvas = Canvas::new(
            4,
            4,
            PixelFormat::Rgba8,
            Color {
                r: 0,
                g: 0,
                b: 0,
                a: 0,
            },
        );
        assert_eq!(canvas.pixel(1, 1).map(|c| c.a), Some(0), "starts clear");

        canvas.fill_mask(&square_mask(0.0, 0.0, 4.0, 4.0, 4), Color::WHITE, 1.0);
        assert_eq!(canvas.pixel(1, 1).map(|c| c.a), Some(255), "becomes opaque");
    }

    #[test]
    fn compositing_is_bit_identical_across_runs() {
        let mask = square_mask(0.5, 0.5, 3.25, 2.75, 8);
        let paint = |c: &mut Canvas| c.fill_mask(&mask, Color::rgb(10, 200, 30), 0.37);

        let mut a = Canvas::new(8, 8, PixelFormat::Rgb8, Color::WHITE);
        let mut b = Canvas::new(8, 8, PixelFormat::Rgb8, Color::WHITE);
        paint(&mut a);
        paint(&mut b);
        assert_eq!(a.data, b.data, "ruling 4");
    }

    /// Compositing onto a *transparent* backdrop.
    ///
    /// This is the case the old arithmetic got wrong, and it was invisible
    /// because every canvas the engine made had an opaque background. The old
    /// formula computed the premultiplied result and stored it as straight
    /// colour: half-covering a transparent buffer with red gave (128, 0, 0)
    /// at alpha 128 — a dark red that is really red-over-black — instead of
    /// (255, 0, 0) at alpha 128, which is red seen through half coverage.
    ///
    /// A tile rendered once and blitted, or a transparency group, starts
    /// fully transparent. Every one of them would have darkened toward the
    /// uninitialised black underneath.
    #[test]
    fn half_covering_a_transparent_buffer_keeps_the_colour() {
        let mut canvas = Canvas::new(1, 1, PixelFormat::Rgba8, Color::TRANSPARENT);
        canvas.blend_pixel(0, 0, Color::rgb(255, 0, 0), 0.5);

        let px = canvas.pixel(0, 0).expect("a pixel");
        assert!(
            px.r > 250,
            "the colour stays red rather than darkening to premultiplied: {px:?}"
        );
        assert!(
            (120..=136).contains(&px.a),
            "and the coverage lands in the alpha: {px:?}"
        );
    }

    /// The opaque path must be untouched by that fix, or every existing
    /// golden moves.
    #[test]
    fn compositing_onto_an_opaque_backdrop_is_unchanged() {
        let mut canvas = Canvas::new(1, 1, PixelFormat::Rgb8, Color::WHITE);
        canvas.blend_pixel(0, 0, Color::rgb(0, 0, 0), 0.5);

        let px = canvas.pixel(0, 0).expect("a pixel");
        assert!(
            (120..=136).contains(&px.r),
            "half black over white is mid grey: {px:?}"
        );
    }

    /// Two half-covering paints must reach the same place as one, whichever
    /// order they arrive in — the property that makes a group buffer usable
    /// at all.
    #[test]
    fn repeated_compositing_converges_rather_than_drifting() {
        let mut canvas = Canvas::new(1, 1, PixelFormat::Rgba8, Color::TRANSPARENT);
        for _ in 0..8 {
            canvas.blend_pixel(0, 0, Color::rgb(255, 0, 0), 0.5);
        }
        let px = canvas.pixel(0, 0).expect("a pixel");
        assert!(px.r > 250, "still red, not drifting dark: {px:?}");
        assert!(px.a > 250, "and now essentially opaque: {px:?}");
    }

    /// A blend mode must see a transparent backdrop as *absent*, not as
    /// black. `Multiply` against black is black, so getting this wrong turns
    /// every multiplied group into a silhouette.
    #[test]
    fn a_blend_mode_ignores_a_backdrop_that_is_not_there() {
        let mut canvas = Canvas::new(1, 1, PixelFormat::Rgba8, Color::TRANSPARENT);
        canvas.blend_pixel_with(0, 0, Color::rgb(255, 0, 0), 1.0, BlendMode::Multiply);

        let px = canvas.pixel(0, 0).expect("a pixel");
        assert!(
            px.r > 250,
            "multiplying against nothing leaves the source: {px:?}"
        );
    }

    /// And where the backdrop *is* there, the mode applies.
    #[test]
    fn a_blend_mode_applies_against_a_real_backdrop() {
        let mut canvas = Canvas::new(1, 1, PixelFormat::Rgb8, Color::rgb(128, 128, 128));
        canvas.blend_pixel_with(0, 0, Color::rgb(128, 128, 128), 1.0, BlendMode::Multiply);

        let px = canvas.pixel(0, 0).expect("a pixel");
        assert!(px.r < 80, "mid grey multiplied by itself is darker: {px:?}");
    }

    // -----------------------------------------------------------------------
    // Compositing one canvas onto another (gap 11, milestone 1).
    // -----------------------------------------------------------------------

    /// The exit criterion, both halves: a small buffer blitted onto a page
    /// matches the same content drawn directly at that position, and the
    /// buffer it was drawn into started fully transparent.
    ///
    /// Written as an equality between two whole canvases rather than as a spot
    /// check, because the interesting failures are at the edges — one row
    /// short, one column offset — and a sample in the middle of the square
    /// cannot see any of them.
    #[test]
    fn a_blitted_buffer_matches_the_same_content_drawn_directly() {
        let mut direct = Canvas::new(16, 16, PixelFormat::Rgb8, Color::WHITE);
        let mut path = Path::new();
        path.rect(5.0, 6.0, 4.5, 3.25);
        let mask = fill(&path, FillRule::NonZero, 0, 0, 16, 16, 0.05, None);
        direct.fill_mask(&mask, Color::rgb(20, 140, 200), 1.0);

        // The same shape into a 6x5 buffer whose top-left is device (5, 6),
        // then blitted back to where it came from.
        let mut tile = Canvas::new(6, 5, PixelFormat::Rgba8, Color::TRANSPARENT);
        let local = fill(&path, FillRule::NonZero, 5, 6, 6, 5, 0.05, None);
        let mut shifted = local.clone();
        shifted.x0 = 0;
        shifted.y0 = 0;
        tile.fill_mask(&shifted, Color::rgb(20, 140, 200), 1.0);

        let mut blitted = Canvas::new(16, 16, PixelFormat::Rgb8, Color::WHITE);
        blitted.composite(&tile, (5, 6), 1.0, BlendMode::Normal, None, None);

        assert_eq!(
            blitted.data, direct.data,
            "a rasterised tile blitted is the same pixels as the shape drawn in place"
        );
    }

    /// The bound: a composite costs the source rectangle, and touches nothing
    /// outside it — including when the source hangs off two edges at once.
    #[test]
    fn a_composite_is_bounded_by_the_source_rectangle() {
        let tile = Canvas::new(4, 4, PixelFormat::Rgba8, Color::rgb(0, 0, 0));

        let mut canvas = Canvas::new(12, 12, PixelFormat::Rgb8, Color::WHITE);
        canvas.composite(&tile, (3, 2), 1.0, BlendMode::Normal, None, None);
        for y in 0..12 {
            for x in 0..12 {
                let inside = (3..7).contains(&x) && (2..6).contains(&y);
                let want = if inside { Color::BLACK } else { Color::WHITE };
                assert_eq!(canvas.pixel(x, y), Some(want), "at ({x}, {y})");
            }
        }

        // Hanging off the top-left and off the bottom-right: the overlap is
        // painted and nothing panics or wraps.
        let mut corner = Canvas::new(6, 6, PixelFormat::Rgb8, Color::WHITE);
        corner.composite(&tile, (-2, -2), 1.0, BlendMode::Normal, None, None);
        corner.composite(&tile, (4, 4), 1.0, BlendMode::Normal, None, None);
        assert_eq!(corner.pixel(0, 0), Some(Color::BLACK), "the visible corner");
        assert_eq!(corner.pixel(2, 2), Some(Color::WHITE), "past it");
        assert_eq!(corner.pixel(5, 5), Some(Color::BLACK), "and the far one");
        // Entirely outside, in both directions.
        corner.composite(&tile, (-40, -40), 1.0, BlendMode::Normal, None, None);
        corner.composite(&tile, (400, 400), 1.0, BlendMode::Normal, None, None);
        corner.composite(
            &tile,
            (i32::MAX, i32::MIN),
            1.0,
            BlendMode::Normal,
            None,
            None,
        );
    }

    /// Compositing onto a transparent buffer keeps straight-alpha colour —
    /// the other half of milestone 1, and the property nested groups stand on.
    #[test]
    fn compositing_onto_a_transparent_buffer_keeps_the_colour() {
        let mut tile = Canvas::new(1, 1, PixelFormat::Rgba8, Color::TRANSPARENT);
        tile.blend_pixel(0, 0, Color::rgb(255, 0, 0), 0.5);

        let mut outer = Canvas::new(1, 1, PixelFormat::Rgba8, Color::TRANSPARENT);
        outer.composite(&tile, (0, 0), 1.0, BlendMode::Normal, None, None);

        let px = outer.pixel(0, 0).expect("a pixel");
        assert!(px.r > 250, "still red rather than premultiplied: {px:?}");
        assert!((120..=136).contains(&px.a), "and half covered: {px:?}");
    }

    /// The source's own alpha, the constant alpha and the mask all multiply,
    /// and the mask is read in the *destination's* coordinates.
    #[test]
    fn a_composite_multiplies_alpha_by_the_mask() {
        let tile = Canvas::new(4, 1, PixelFormat::Rgba8, Color::BLACK);
        let mask = Mask {
            x0: 5,
            y0: 3,
            width: 2,
            height: 1,
            data: vec![255, 0],
            outside: 0,
        };

        let mut canvas = Canvas::new(10, 6, PixelFormat::Rgb8, Color::WHITE);
        canvas.composite(&tile, (4, 3), 1.0, BlendMode::Normal, Some(&mask), None);
        assert_eq!(canvas.pixel(4, 3), Some(Color::WHITE), "left of the mask");
        assert_eq!(canvas.pixel(5, 3), Some(Color::BLACK), "under it");
        assert_eq!(canvas.pixel(6, 3), Some(Color::WHITE), "masked away");
        assert_eq!(canvas.pixel(7, 3), Some(Color::WHITE), "right of the mask");

        // And the constant alpha scales the whole thing.
        let mut half = Canvas::new(10, 6, PixelFormat::Rgb8, Color::WHITE);
        half.composite(&tile, (4, 3), 0.5, BlendMode::Normal, None, None);
        let px = half.pixel(5, 3).expect("a pixel");
        assert!((120..=136).contains(&px.r), "half black over white: {px:?}");
    }

    /// A composite is a paint like any other: it stops when asked, and a
    /// predicate that never answers yes changes no byte (ruling 4).
    #[test]
    fn a_composite_stops_when_asked() {
        let tile = Canvas::new(4, 64, PixelFormat::Rgba8, Color::BLACK);

        let mut whole = Canvas::new(4, 64, PixelFormat::Rgb8, Color::WHITE);
        whole.composite(&tile, (0, 0), 1.0, BlendMode::Normal, None, None);
        assert_eq!(whole.pixel(0, 63), Some(Color::BLACK));

        let never = || false;
        let mut hooked = Canvas::new(4, 64, PixelFormat::Rgb8, Color::WHITE);
        hooked.composite(&tile, (0, 0), 1.0, BlendMode::Normal, None, Some(&never));
        assert_eq!(hooked.data, whole.data, "the hook is not an input");

        // Stops on the second question, which is one band in.
        let calls = std::cell::Cell::new(0u32);
        let after_one = || {
            calls.set(calls.get() + 1);
            calls.get() >= 2
        };
        let mut partial = Canvas::new(4, 64, PixelFormat::Rgb8, Color::WHITE);
        partial.composite(
            &tile,
            (0, 0),
            1.0,
            BlendMode::Normal,
            None,
            Some(&after_one),
        );
        assert_eq!(
            partial.pixel(0, 0),
            Some(Color::BLACK),
            "the first band ran"
        );
        assert_eq!(
            partial.pixel(0, 16),
            Some(Color::WHITE),
            "the second did not"
        );
    }

    /// Reading a rendered buffer as a mask (11.6.5.2), both kinds, and the
    /// transfer function that shifts it.
    #[test]
    fn a_buffer_reads_back_as_a_luminosity_or_alpha_mask() {
        let mut buffer = Canvas::new(2, 1, PixelFormat::Rgba8, Color::TRANSPARENT);
        buffer.blend_pixel(0, 0, Color::WHITE, 1.0);
        buffer.blend_pixel(1, 0, Color::rgb(0, 0, 0), 0.5);

        let luminosity = buffer.to_mask((7, 9), MaskKind::Luminosity, None);
        assert_eq!((luminosity.x0, luminosity.y0), (7, 9), "device addressed");
        assert_eq!(luminosity.at(7, 9), 255, "white is fully unmasked");
        assert_eq!(luminosity.at(8, 9), 0, "black is fully masked");
        assert_eq!(luminosity.at(6, 9), 0, "and outside is outside");

        let alpha = buffer.to_mask((7, 9), MaskKind::Alpha, None);
        assert_eq!(alpha.at(7, 9), 255);
        assert!(
            (120..=136).contains(&alpha.at(8, 9)),
            "half-covered black is half opaque, not black: {}",
            alpha.at(8, 9)
        );

        // /TR, as a 256-entry table: this one inverts.
        let mut invert = [0u8; 256];
        for (value, slot) in invert.iter_mut().enumerate() {
            *slot = 255 - value as u8;
        }
        let shifted = buffer.to_mask((0, 0), MaskKind::Luminosity, Some(&invert));
        assert_eq!(shifted.at(0, 0), 0, "the transfer function is applied");
        assert_eq!(shifted.at(1, 0), 255);
    }

    /// `to_mask` the long way round: a `Color` from `pixel` for every pixel,
    /// read as the kind asks and through the transfer table.
    fn mask_by_pixel(
        canvas: &Canvas,
        at: (i32, i32),
        kind: MaskKind,
        transfer: Option<&[u8; 256]>,
    ) -> Mask {
        let mut mask = Mask::empty(at.0, at.1, canvas.width, canvas.height);
        for row in 0..canvas.height {
            for col in 0..canvas.width {
                let color = canvas.pixel(col, row).expect("a pixel");
                let value = match kind {
                    MaskKind::Luminosity => color.luminosity(),
                    MaskKind::Alpha => color.a,
                };
                let value = transfer.map_or(value, |lut| lut[usize::from(value)]);
                mask.data[(row * canvas.width + col) as usize] = value;
            }
        }
        mask
    }

    /// A `width` by `height` canvas of `format` whose pixel `(x, y)` holds
    /// the bytes `bytes(x, y)` gives, its alpha included.
    fn canvas_of(
        format: PixelFormat,
        width: u32,
        height: u32,
        bytes: impl Fn(u32, u32) -> [u8; 5],
    ) -> Canvas {
        let mut canvas = Canvas::new(width, height, format, Color::TRANSPARENT);
        let components = format.components();
        for row in 0..height {
            for col in 0..width {
                let base = (row as usize) * canvas.stride + (col as usize) * components;
                canvas.data[base..base + components]
                    .copy_from_slice(&bytes(col, row)[..components]);
            }
        }
        canvas
    }

    /// **`to_mask` reads every format as `pixel` does** (P-25.2): the row
    /// reader, in integer, gives each pixel the value `Canvas::pixel` and
    /// `Color::luminosity` give it.
    ///
    /// Exhaustive over every value each reading depends on: every grey and
    /// every grey and alpha pair; every RGB triple, in either order of the
    /// channels, with an alpha that runs through all 256 levels across them;
    /// every pair of two inks and black
    /// at every black, the third ink running through every level, which
    /// meets every ink at every black and every two inks together; and for
    /// Lab, which reads through `lab_to_rgb`'s floats as `pixel` does, every
    /// pair of its three bytes. Each format is read as luminosity everywhere,
    /// and as alpha, through a table that permutes every level and on a buffer
    /// standing away from the origin, over a slice of it that holds every alpha
    /// byte — the alpha reading depends on that byte alone, and the table and
    /// the position on nothing of the format.
    #[test]
    fn to_mask_reads_every_format_as_pixel_does() {
        let mut permute = [0u8; 256];
        for (value, slot) in permute.iter_mut().enumerate() {
            *slot = (value * 167 + 13) as u8;
        }
        let every: [((i32, i32), MaskKind, Option<&[u8; 256]>); 4] = [
            ((0, 0), MaskKind::Luminosity, None),
            ((0, 0), MaskKind::Alpha, None),
            ((-37, 1_000_003), MaskKind::Luminosity, Some(&permute)),
            ((-37, 1_000_003), MaskKind::Alpha, Some(&permute)),
        ];
        let mut checked = 0u64;
        let mut check = |canvas: &Canvas, what: &str, every_way: bool| {
            let ways = if every_way { &every[..] } else { &every[..1] };
            for &(at, kind, transfer) in ways {
                let got = canvas.to_mask(at, kind, transfer);
                let want = mask_by_pixel(canvas, at, kind, transfer);
                assert_eq!(
                    (got.x0, got.y0, got.width, got.height, got.outside),
                    (want.x0, want.y0, want.width, want.height, 0),
                    "{what}: where the mask stands"
                );
                assert!(
                    got.data == want.data,
                    "{what}, {kind:?}, a table: {}: the pixels differ",
                    transfer.is_some()
                );
            }
            checked += u64::from(canvas.width) * u64::from(canvas.height);
        };

        let grey = canvas_of(PixelFormat::Gray8, 256, 1, |x, _| [x as u8, 0, 0, 0, 0]);
        check(&grey, "Gray8", true);
        let grey_alpha = canvas_of(PixelFormat::GrayA8, 256, 256, |x, y| {
            [x as u8, y as u8, 0, 0, 0]
        });
        check(&grey_alpha, "GrayA8", true);
        for red in 0..=255u8 {
            let every_way = red == 0 || red == 200;
            let rgb = canvas_of(PixelFormat::Rgb8, 256, 256, |x, y| {
                [red, x as u8, y as u8, 0, 0]
            });
            check(&rgb, &format!("Rgb8, red {red}"), every_way);
            let rgba = canvas_of(PixelFormat::Rgba8, 256, 256, |x, y| {
                [red, x as u8, y as u8, (x as u8) ^ (y as u8) ^ red, 0]
            });
            check(&rgba, &format!("Rgba8, red {red}"), every_way);
            let bgr = canvas_of(PixelFormat::Bgr8, 256, 256, |x, y| {
                [x as u8, y as u8, red, 0, 0]
            });
            check(&bgr, &format!("Bgr8, red {red}"), every_way);
            let bgra = canvas_of(PixelFormat::Bgra8, 256, 256, |x, y| {
                [x as u8, y as u8, red, (x as u8) ^ (y as u8) ^ red, 0]
            });
            check(&bgra, &format!("Bgra8, red {red}"), every_way);
        }
        for black in 0..=255u8 {
            let ink = canvas_of(PixelFormat::CmykA8, 256, 256, |x, y| {
                let (c, m) = (x as u8, y as u8);
                [c, m, c ^ m ^ black, black, c.wrapping_add(m)]
            });
            check(&ink, &format!("CmykA8, black {black}"), black % 85 == 0);
        }
        let lab = canvas_of(PixelFormat::LabA8, 256, 256, |x, y| {
            let (l, a) = (x as u8, y as u8);
            [l, a, l ^ a, a.wrapping_sub(l), 0]
        });
        check(&lab, "LabA8", true);

        assert_eq!(
            checked,
            256 + 65_536 + 4 * 256 * 65_536 + 256 * 65_536 + 65_536,
            "every pixel of every buffer was read"
        );
    }

    /// 11.4.7.2's removal, checked against the arithmetic it is derived from
    /// rather than against itself.
    ///
    /// A group over an opaque mid-grey backdrop, half-covered by red. The
    /// buffer then holds the *mixture*; taking the backdrop out again must
    /// leave the red the group actually painted, at the alpha it painted it
    /// with — because that is what will be composited back onto the same
    /// backdrop, and anything else counts the grey twice.
    #[test]
    fn removing_the_backdrop_leaves_what_the_group_painted() {
        let backdrop = Canvas::new(1, 1, PixelFormat::Rgba8, Color::rgb(128, 128, 128));
        let mut group = Canvas::new(1, 1, PixelFormat::Rgba8, Color::TRANSPARENT);
        group.adopt_backdrop(backdrop);

        group.blend_pixel(0, 0, Color::rgb(255, 0, 0), 0.5);
        let mixed = group.pixel(0, 0).expect("a pixel");
        assert!(
            (188..=196).contains(&mixed.r) && (60..=68).contains(&mixed.g),
            "the buffer holds the group over its backdrop: {mixed:?}"
        );
        assert!(
            (120..=136).contains(&mixed.a),
            "and its alpha is the group's own, not the union: {mixed:?}"
        );

        let backdrop = group
            .release_parent()
            .expect("the group holds its backdrop");
        group.remove_backdrop(&backdrop);
        let own = group.pixel(0, 0).expect("a pixel");
        assert!(
            own.r > 245 && own.g < 10 && own.b < 10,
            "the backdrop is out again and the red is back: {own:?}"
        );
        assert!((120..=136).contains(&own.a), "at the alpha it painted with");

        // And compositing it back reconstructs the mixture it started from.
        let mut page = Canvas::new(1, 1, PixelFormat::Rgb8, Color::rgb(128, 128, 128));
        page.composite(&group, (0, 0), 1.0, BlendMode::Normal, None, None);
        let back = page.pixel(0, 0).expect("a pixel");
        assert!(
            back.r.abs_diff(mixed.r) <= 2 && back.g.abs_diff(mixed.g) <= 2,
            "round trip: {back:?} against {mixed:?}"
        );
    }

    /// An isolated group has no backdrop, so removal is a no-op and every
    /// existing arithmetic path is untouched: it holds its parent, reads
    /// nothing of it, and hands it back as it was. A canvas that holds no
    /// parent hands back none.
    #[test]
    fn an_isolated_group_has_nothing_to_remove() {
        let parent = Canvas::new(2, 2, PixelFormat::Rgba8, Color::rgb(9, 99, 199));
        let mut group = Canvas::new(2, 2, PixelFormat::Rgba8, Color::TRANSPARENT);
        group.hold_parent(parent.clone());
        assert_eq!(
            group.data,
            Canvas::new(2, 2, PixelFormat::Rgba8, Color::TRANSPARENT).data,
            "nothing adopted"
        );
        group.blend_pixel(0, 0, Color::rgb(10, 200, 30), 0.4);
        let before = group.data.clone();
        let held = group.release_parent().expect("the group holds its parent");
        assert_eq!(held.data, parent.data, "handed back as it was");
        group.remove_backdrop(&held);
        assert_eq!(group.data, before, "no backdrop, no correction");
        assert!(group.release_parent().is_none(), "and it holds nothing now");
        let mut plain = Canvas::new(2, 2, PixelFormat::Rgb8, Color::WHITE);
        assert!(plain.release_parent().is_none(), "a canvas holds no parent");
    }

    /// 11.4.7.2's removal over a backdrop held as `copy` — the parent's
    /// rectangle under `buffer`, in its format, standing where it does: the
    /// arithmetic `remove_backdrop` makes, spelled over the copy's own bytes.
    fn removed_against(buffer: &Canvas, copy: &Canvas) -> Vec<u8> {
        let mut out = buffer.data.clone();
        let channels = color_channels(buffer.format);
        let components = buffer.format.components();
        for row in 0..buffer.height {
            for col in 0..buffer.width {
                let own = buffer.alpha_at(col, row);
                let initial = copy.alpha_at(col, row);
                if own == 0 || initial == 0 {
                    continue;
                }
                let factor = (i64::from(initial) * 255 / i64::from(own)) - i64::from(initial);
                let base = (row as usize) * buffer.stride + (col as usize) * components;
                for i in 0..channels {
                    let cn = i64::from(out[base + i]);
                    let c0 = i64::from(copy.data[base + i]);
                    out[base + i] = (cn + (cn - c0) * factor / 255).clamp(0, 255) as u8;
                }
            }
        }
        out
    }

    /// 11.4.5's restore against `snapshot`, the bytes `buffer` started in: the
    /// arithmetic `knock_out` makes, spelled over a kept copy of them.
    fn knocked_against(buffer: &Canvas, snapshot: &[u8], mask: &Mask) -> Vec<u8> {
        let mut out = buffer.data.clone();
        let components = buffer.format.components();
        let (ox, oy) = buffer.origin();
        for row in 0..buffer.height {
            for col in 0..buffer.width {
                let coverage = u32::from(mask.at(ox + col as i32, oy + row as i32));
                if coverage == 0 {
                    continue;
                }
                let base = (row as usize) * buffer.stride + (col as usize) * components;
                for i in 0..components {
                    let (from, slot) = (u32::from(snapshot[base + i]), u32::from(out[base + i]));
                    out[base + i] =
                        (mul255(from, coverage) + mul255(slot, 255 - coverage)).min(255) as u8;
                }
            }
        }
        out
    }

    /// **A group reads its backdrop from its parent as a copy of it would
    /// hold it** (P-25.3).
    ///
    /// The parent moves into the group's buffer and is read where it lies,
    /// with no copy kept. Held here against a copy the parent would give —
    /// `extract` of the buffer's rectangle in the buffer's format — for every
    /// parent format under every group format, over parents whose pixels and
    /// alphas are all different, and buffers inside the parent, the whole of
    /// it and hanging off its corner: adoption writes the copy's colour at no
    /// alpha; `a0`, read at every composited pixel, is the copy's alpha; a
    /// knockout restore puts back the bytes the buffer started in; removal
    /// takes the copy's backdrop out; and the parent comes back as it went
    /// in. An isolated group holds its parent and reads nothing of it.
    #[test]
    fn a_group_reads_its_backdrop_from_its_parent_as_a_copy_would_hold_it() {
        let mut state = 0x5eed_1e55_u32;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state
        };
        let parents = [
            PixelFormat::Gray8,
            PixelFormat::GrayA8,
            PixelFormat::Rgb8,
            PixelFormat::Rgba8,
            PixelFormat::CmykA8,
            PixelFormat::LabA8,
        ];
        let groups = [
            PixelFormat::GrayA8,
            PixelFormat::Rgba8,
            PixelFormat::CmykA8,
            PixelFormat::LabA8,
        ];
        let places = [((0, 10), 11u32, 9u32), ((-5, 7), 30, 20), ((20, 20), 12, 9)];
        let mut cases = 0;
        for parent_format in parents {
            let mut parent = Canvas::new(30, 20, parent_format, Color::WHITE).at_origin(-5, 7);
            let channels = color_channels(parent_format);
            for pixel in parent.data.chunks_exact_mut(parent_format.components()) {
                for byte in pixel.iter_mut() {
                    *byte = next() as u8;
                }
                if parent_format.has_alpha() {
                    pixel[channels] = match next() % 4 {
                        0 => 0,
                        1 => 255,
                        _ => next() as u8,
                    };
                }
            }
            for format in groups {
                for &(origin, width, height) in &places {
                    let what = format!("a {format:?} group at {origin:?} over {parent_format:?}");
                    let copy = parent.extract(origin, width, height, format);
                    let made = Canvas::new(width, height, format, Color::TRANSPARENT)
                        .at_origin(origin.0, origin.1);
                    let group_channels = color_channels(format);
                    let components = format.components();

                    let mut group = made.clone();
                    group.adopt_backdrop(parent.clone());
                    let mut adopted = made.data.clone();
                    for (pixel, held) in adopted
                        .chunks_exact_mut(components)
                        .zip(copy.data.chunks_exact(components))
                    {
                        pixel[..group_channels].copy_from_slice(&held[..group_channels]);
                    }
                    assert_eq!(group.data, adopted, "{what}: the copy's colour at no alpha");
                    for row in 0..height {
                        for col in 0..width {
                            assert_eq!(
                                group.backdrop_alpha(col, row),
                                copy.alpha_at(col, row),
                                "{what}: a0 at ({col}, {row})"
                            );
                        }
                    }

                    // Paint, knock out where a mask covers, paint again.
                    let snapshot = group.data.clone();
                    let mut cover = Mask::empty(origin.0 + 1, origin.1 + 1, width, height);
                    for value in &mut cover.data {
                        *value = match next() % 3 {
                            0 => 0,
                            1 => 255,
                            _ => next() as u8,
                        };
                    }
                    let colour = Color {
                        r: next() as u8,
                        g: next() as u8,
                        b: next() as u8,
                        a: 255,
                    };
                    group.fill_mask_inked(&cover, colour, None, 0.6, BlendMode::Multiply);
                    let knocked = knocked_against(&group, &snapshot, &cover);
                    group.knock_out(&cover);
                    assert_eq!(group.data, knocked, "{what}: the knockout's initial state");
                    group.fill_mask_inked(&cover, colour, None, 0.35, BlendMode::Screen);

                    let held = group.release_parent().expect("the group holds its parent");
                    assert!(
                        held.data == parent.data,
                        "{what}: the parent comes back as it went in"
                    );
                    assert_eq!(held.origin(), parent.origin(), "{what}");
                    let removed = removed_against(&group, &copy);
                    group.remove_backdrop(&held);
                    assert_eq!(group.data, removed, "{what}: the backdrop taken out");
                    let again = group.data.clone();
                    group.remove_backdrop(&held);
                    assert_eq!(group.data, again, "{what}: and only once");

                    // Isolated: held, read for nothing.
                    let mut isolated = made.clone();
                    isolated.hold_parent(parent.clone());
                    assert_eq!(
                        isolated.data, made.data,
                        "{what}, isolated: nothing adopted"
                    );
                    for row in 0..height {
                        for col in 0..width {
                            assert_eq!(isolated.backdrop_alpha(col, row), 0, "{what}, isolated");
                        }
                    }
                    let snapshot = isolated.data.clone();
                    isolated.fill_mask_inked(&cover, colour, None, 0.6, BlendMode::Multiply);
                    let knocked = knocked_against(&isolated, &snapshot, &cover);
                    isolated.knock_out(&cover);
                    assert_eq!(isolated.data, knocked, "{what}, isolated: back to nothing");
                    let held = isolated.release_parent().expect("held");
                    let painted = isolated.data.clone();
                    isolated.remove_backdrop(&held);
                    assert_eq!(
                        isolated.data, painted,
                        "{what}, isolated: nothing to remove"
                    );
                    cases += 1;
                }
            }
        }
        assert_eq!(
            cases,
            parents.len() * groups.len() * places.len(),
            "every case ran"
        );
    }

    /// A pixel off a canvas is no pixel of it, in its own format as in
    /// another, wherever the column falls: `extract` gives a rectangle
    /// hanging off a canvas's right edge transparent there, not the next
    /// row's pixels.
    #[test]
    fn a_copy_past_a_canvas_s_right_edge_reads_nothing_there() {
        for format in [PixelFormat::Rgba8, PixelFormat::GrayA8, PixelFormat::Rgb8] {
            let mut canvas = Canvas::new(4, 3, format, Color::WHITE);
            for (index, byte) in canvas.data.iter_mut().enumerate() {
                *byte = (index * 29 + 7) as u8;
            }
            for target in [format, PixelFormat::Rgba8] {
                let copy = canvas.extract((2, 0), 4, 3, target);
                let clear = Canvas::new(1, 1, target, Color::TRANSPARENT);
                for row in 0..3 {
                    for col in 2..4 {
                        assert_eq!(
                            copy.pixel(col, row),
                            clear.pixel(0, 0),
                            "{format:?} as {target:?}: column {} of a 4-wide canvas",
                            col + 2
                        );
                    }
                    for col in 0..2 {
                        let mut expected = Canvas::new(1, 1, target, Color::TRANSPARENT);
                        let (word, _) = expected
                            .source_from(&canvas, col + 2, row)
                            .expect("on the canvas");
                        let components = target.components();
                        expected.data.copy_from_slice(&word[..components]);
                        assert_eq!(copy.pixel(col, row), expected.pixel(0, 0), "{format:?}");
                    }
                }
            }
        }
    }

    /// 8.7.3.3: recolouring keeps the shape and replaces the ink. A pixel the
    /// cell never reached stays at nothing, or an uncoloured pattern becomes a
    /// solid rectangle in the operands' colour — which paints, looks
    /// deliberate, and is the whole cell's worth of drawing thrown away.
    #[test]
    fn recolouring_replaces_the_ink_and_keeps_the_shape() {
        let mut cell = Canvas::new(2, 1, PixelFormat::Rgba8, Color::TRANSPARENT);
        cell.blend_pixel(0, 0, Color::rgb(0, 255, 0), 1.0);
        cell.blend_pixel(1, 0, Color::rgb(0, 255, 0), 0.5);

        cell.recolor(Color::rgb(255, 0, 0));

        assert_eq!(
            cell.pixel(0, 0),
            Some(Color {
                r: 255,
                g: 0,
                b: 0,
                a: 255
            }),
            "the ink is the new colour"
        );
        let soft = cell.pixel(1, 0).expect("a pixel");
        assert_eq!((soft.r, soft.g, soft.b), (255, 0, 0));
        assert!(
            (120..=135).contains(&soft.a),
            "and a half-covered pixel keeps its coverage, got {}",
            soft.a
        );
    }

    /// A format with no alpha has no shape to keep, so recolouring it would
    /// paint every pixel — including the ones outside whatever was drawn.
    #[test]
    fn recolouring_a_canvas_without_alpha_does_nothing() {
        let mut canvas = Canvas::new(2, 1, PixelFormat::Rgb8, Color::WHITE);
        let before = canvas.data.clone();
        canvas.recolor(Color::rgb(255, 0, 0));
        assert_eq!(canvas.data, before);
    }

    #[test]
    fn mul255_keeps_its_endpoints() {
        assert_eq!(mul255(255, 255), 255, "opaque over opaque stays opaque");
        assert_eq!(mul255(0, 255), 0);
        assert_eq!(mul255(255, 0), 0);
        assert_eq!(mul255(128, 255), 128);
    }

    /// `nearest_level` is `f64::round` and then the cast, everywhere: across
    /// every quarter-level from below zero to past 255, at every half-integer
    /// and the doubles either side of it, and at the values a cast saturates.
    #[test]
    fn the_nearest_level_is_round_without_the_call() {
        let rounded = |x: f64| x.round() as u32;
        let mut values = vec![
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            -0.0,
            0.0,
            4_294_967_294.5,
            4_294_967_295.0,
            4_294_967_295.4,
            4_294_967_296.0,
            1e300,
        ];
        for quarter in -8..=1100 {
            values.push(f64::from(quarter) / 4.0);
        }
        for whole in -2..=300 {
            let half = f64::from(whole) + 0.5;
            values.extend([
                half,
                f64::from_bits(half.to_bits() - 1),
                f64::from_bits(half.to_bits() + 1),
            ]);
        }
        for x in values {
            assert_eq!(nearest_level(x), rounded(x), "at {x:e}");
        }
    }

    /// **An image pixel's level is the `f64` expression a draw writes, rounded
    /// by `f64::round`, at every coverage, sample alpha and clip.**
    ///
    /// Exhaustive over the three factors and the two, at a sweep of draw
    /// alphas: one, which takes the integer form; a half, two thirds, 0.37 and
    /// 0.999, which take the expression; a thousandth; and the alphas that
    /// clamp or are not numbers at all. The expression is evaluated here as the
    /// draw writes it, left to right, with each prefix kept for the next
    /// factor, which is the same number because `x × y / z × w` is
    /// `((x × y) / z) × w`.
    #[test]
    fn an_image_pixel_composes_as_the_float_expression_rounded() {
        let rounded = |opacity: f64| -> u32 {
            if !opacity.is_finite() {
                return 0;
            }
            (opacity.clamp(0.0, 1.0) * 255.0).round() as u32
        };
        let exhaustive = [1.0, 0.5, 2.0 / 3.0, 0.37, 0.999, 1e-3];
        let degenerate = [0.0, -0.5, 1.5, f64::NAN, f64::INFINITY];
        for alpha in exhaustive.into_iter().chain(degenerate) {
            let opacity = Opacity::new(alpha);
            let clamped = alpha.clamp(0.0, 1.0);
            // The degenerate alphas are a clamp to nothing or to one, or no
            // number at all, so every third factor says what all would.
            let step = if exhaustive.contains(&alpha) { 1 } else { 3 };
            for a in (0..=255u8).step_by(step) {
                let after_a = clamped * f64::from(a) / 255.0;
                for b in 0..=255u8 {
                    let after_b = after_a * f64::from(b) / 255.0;
                    if opacity.level_two(a, b) != rounded(after_b) {
                        panic!(
                            "alpha {alpha}, run coverage {a}, clip {b}: {} against {}",
                            opacity.level_two(a, b),
                            rounded(after_b)
                        );
                    }
                    for c in (0..=255u8).step_by(step) {
                        let after_c = after_b * f64::from(c) / 255.0;
                        if opacity.level_three(a, b, c) != rounded(after_c) {
                            panic!(
                                "alpha {alpha}, coverage {a}, sample alpha {b}, clip {c}: \
                                 {} against {}",
                                opacity.level_three(a, b, c),
                                rounded(after_c)
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn degenerate_canvases_and_alphas_are_harmless() {
        let mut empty = Canvas::new(0, 0, PixelFormat::Rgb8, Color::WHITE);
        empty.fill_mask(&square_mask(0.0, 0.0, 1.0, 1.0, 1), Color::BLACK, 1.0);
        assert!(empty.data.is_empty());
        assert_eq!(empty.pixel(0, 0), None);

        let mut canvas = Canvas::new(2, 2, PixelFormat::Rgb8, Color::WHITE);
        for alpha in [0.0, -1.0, 2.0, f64::NAN] {
            canvas.fill_mask(&square_mask(0.0, 0.0, 2.0, 2.0, 2), Color::BLACK, alpha);
        }
    }
    /// The formats a group's buffer can be: every one that carries alpha.
    const GROUP_FORMATS: [PixelFormat; 4] = [
        PixelFormat::GrayA8,
        PixelFormat::Rgba8,
        PixelFormat::CmykA8,
        PixelFormat::LabA8,
    ];

    /// **The opaque Normal path is `blend` in every group format** (P-25.4):
    /// at a full effective alpha under `Normal`, `opaque_normal` writes what
    /// `blend` writes, over every source and destination channel value,
    /// against a lattice of the destination's own alphas and of its initial
    /// backdrop's, for `GrayA8`, `Rgba8`, `CmykA8` and `LabA8` — and for the
    /// two formats without alpha too, which a page is.
    #[test]
    fn the_opaque_normal_path_is_blend_in_every_group_format() {
        let own_alphas = [0u8, 1, 2, 64, 127, 128, 200, 254, 255];
        let initials = [0u32, 1, 77, 128, 254, 255];
        let mut compared = 0u64;
        for format in GROUP_FORMATS
            .into_iter()
            .chain([PixelFormat::Gray8, PixelFormat::Rgb8])
        {
            let components = format.components();
            let channels = color_channels(format);
            let owns: &[u8] = if format.has_alpha() {
                &own_alphas
            } else {
                &[255]
            };
            for cs in 0..=255u8 {
                let mut source = [cs; 5];
                source[channels] = 255;
                for cb in 0..=255u8 {
                    for &own in owns {
                        let mut pixel = [cb; 5];
                        if format.has_alpha() {
                            pixel[channels] = own;
                        }
                        for initial in initials {
                            let (mut want, mut got) = (pixel, pixel);
                            blend(
                                want.get_mut(..components),
                                &source,
                                255,
                                format,
                                BlendMode::Normal,
                                initial,
                            );
                            opaque_normal(&mut got[..components], &source, format);
                            if got != want {
                                panic!(
                                    "{format:?}: {cs} over {cb} at its own alpha {own} and \
                                     a backdrop alpha {initial}: {got:?} against {want:?}"
                                );
                            }
                            compared += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(
            compared,
            65_536 * initials.len() as u64 * (4 * own_alphas.len() as u64 + 2),
            "every case ran"
        );
    }

    /// What `fill_mask_inked` writes, the long way round: `blend` at every
    /// pixel `mask` covers, at the coverage times the colour's and the state's
    /// alphas, against the canvas's own backdrop.
    fn filled_by_blend(
        canvas: &Canvas,
        mask: &Mask,
        source: &[u8; 5],
        color_alpha: u32,
        alpha: u32,
        mode: BlendMode,
    ) -> Vec<u8> {
        let mut out = canvas.data.clone();
        let components = canvas.format.components();
        let (ox, oy) = canvas.origin();
        for row in 0..canvas.height {
            for col in 0..canvas.width {
                let coverage = u32::from(mask.at(ox + col as i32, oy + row as i32));
                let effective = mul255(mul255(coverage, color_alpha), alpha);
                if effective == 0 {
                    continue;
                }
                let backdrop = canvas.backdrop_alpha(col, row);
                let base = (row as usize) * canvas.stride + (col as usize) * components;
                blend(
                    out.get_mut(base..base + components),
                    source,
                    effective,
                    canvas.format,
                    mode,
                    backdrop,
                );
            }
        }
        out
    }

    /// What `composite` writes, the long way round: `blend` at every pixel of
    /// `src` placed at `at`, at its own alpha times `alpha` times the mask's
    /// coverage, against the canvas's own backdrop.
    fn composited_by_blend(
        canvas: &Canvas,
        src: &Canvas,
        at: (i32, i32),
        alpha: u32,
        mask: Option<&Mask>,
    ) -> Vec<u8> {
        let mut out = canvas.data.clone();
        let components = canvas.format.components();
        let (ox, oy) = canvas.origin();
        for row in 0..canvas.height {
            for col in 0..canvas.width {
                let (px, py) = (ox + col as i32, oy + row as i32);
                let (sx, sy) = (i64::from(px - at.0), i64::from(py - at.1));
                if sx < 0 || sy < 0 || sx >= i64::from(src.width) || sy >= i64::from(src.height) {
                    continue;
                }
                let Some((source, own)) = canvas.source_from(src, sx as u32, sy as u32) else {
                    continue;
                };
                let coverage = mask.map_or(255, |mask| u32::from(mask.at(px, py)));
                let effective = mul255(mul255(own, alpha), coverage);
                if effective == 0 {
                    continue;
                }
                let backdrop = canvas.backdrop_alpha(col, row);
                let base = (row as usize) * canvas.stride + (col as usize) * components;
                blend(
                    out.get_mut(base..base + components),
                    &source,
                    effective,
                    canvas.format,
                    BlendMode::Normal,
                    backdrop,
                );
            }
        }
        out
    }

    /// The group buffers a paint can land in, each `format` standing at
    /// `(3, 2)` over a page of random pixels: isolated, holding the page;
    /// non-isolated, its backdrop adopted from it; and the non-isolated one
    /// after a knockout restore has put its initial state back in part.
    fn groups_over_a_page(
        format: PixelFormat,
        next: &mut impl FnMut() -> u32,
    ) -> Vec<(&'static str, Canvas)> {
        let mut page = Canvas::new(23, 17, PixelFormat::Rgba8, Color::WHITE);
        for (index, byte) in page.data.iter_mut().enumerate() {
            *byte = if index % 4 == 3 {
                [0, 255, next() as u8][index % 3]
            } else {
                next() as u8
            };
        }
        let made = Canvas::new(16, 11, format, Color::TRANSPARENT).at_origin(3, 2);
        let mut isolated = made.clone();
        isolated.hold_parent(page.clone());
        let mut non_isolated = made.clone();
        non_isolated.adopt_backdrop(page);
        let mut cover = Mask::empty(5, 4, 9, 6);
        for value in &mut cover.data {
            *value = match next() % 3 {
                0 => 0,
                1 => 255,
                _ => next() as u8,
            };
        }
        let mut knocked = non_isolated.clone();
        knocked.fill_mask_inked(
            &cover,
            Color::rgb(10, 200, 90),
            None,
            0.7,
            BlendMode::Normal,
        );
        knocked.knock_out(&cover);
        vec![
            ("an isolated group", isolated),
            ("a non-isolated group", non_isolated),
            ("a knockout group", knocked),
        ]
    }

    /// **Every route into a group buffer writes what `blend` writes** (P-25.4):
    /// a fill, an image or a shading's pixel and a nested group's composite,
    /// into an isolated, a non-isolated and a knockout group's buffer in each
    /// group format, an ink buffer handed the document's ink and one handed
    /// none, under masks whose coverage is none, full and between, at a whole
    /// and a partial alpha: byte for byte the canvas `blend` makes at every
    /// pixel, so the opaque path, which takes the pixels of full effective
    /// alpha, writes them as `blend` does.
    #[test]
    fn every_route_into_a_group_writes_what_blend_writes() {
        let mut state = 0x0bad_cafe_u32;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state
        };
        let mut checked = 0;
        for format in GROUP_FORMATS {
            let inks: &[Option<[u8; 4]>] = if format == PixelFormat::CmykA8 {
                &[None, Some([255, 255, 255, 255]), Some([30, 200, 0, 90])]
            } else {
                &[None]
            };
            for (what, group) in groups_over_a_page(format, &mut next) {
                for &ink in inks {
                    for alpha in [1.0, 0.6] {
                        let case = format!("{what}, {format:?}, ink {ink:?}, alpha {alpha}");
                        let colour = Color::rgb(next() as u8, next() as u8, next() as u8);
                        let mut cover = Mask::empty(1, 0, 20, 14);
                        for value in &mut cover.data {
                            *value = match next() % 3 {
                                0 => 0,
                                1 => 255,
                                _ => next() as u8,
                            };
                        }
                        let source = match (ink, format) {
                            (Some([c, m, y, k]), PixelFormat::CmykA8) => [c, m, y, k, colour.a],
                            _ => group.encode(colour),
                        };
                        let level = operation_level(alpha);
                        let want =
                            filled_by_blend(&group, &cover, &source, 255, level, BlendMode::Normal);
                        let mut got = group.clone();
                        got.fill_mask_inked(&cover, colour, ink, alpha, BlendMode::Normal);
                        assert_eq!(got.data, want, "{case}: a fill");

                        let mut sampled = group.clone();
                        let mut by_blend = group.clone();
                        for row in 0..group.height {
                            for col in 0..group.width {
                                let level = [255, 255, 128, 0][(next() % 4) as usize];
                                sampled.blend_pixel_level(
                                    col,
                                    row,
                                    colour,
                                    ink,
                                    level,
                                    BlendMode::Normal,
                                );
                                let base = (row as usize) * group.stride
                                    + (col as usize) * format.components();
                                if level != 0 {
                                    blend(
                                        by_blend.data.get_mut(base..base + format.components()),
                                        &source,
                                        level,
                                        format,
                                        BlendMode::Normal,
                                        group.backdrop_alpha(col, row),
                                    );
                                }
                            }
                        }
                        assert_eq!(sampled.data, by_blend.data, "{case}: sampled pixels");

                        let mut nested = Canvas::new(12, 8, format, colour).at_origin(5, 4);
                        for (index, pixel) in nested
                            .data
                            .chunks_exact_mut(format.components())
                            .enumerate()
                        {
                            if index % 5 == 0 {
                                if let Some(alpha) = pixel.last_mut() {
                                    *alpha = 90;
                                }
                            }
                        }
                        for mask in [None, Some(&cover)] {
                            let want = composited_by_blend(&group, &nested, (5, 4), level, mask);
                            let mut got = group.clone();
                            got.composite(&nested, (5, 4), alpha, BlendMode::Normal, mask, None);
                            assert_eq!(
                                got.data,
                                want,
                                "{case}: a nested group, mask {}",
                                mask.is_some()
                            );
                        }
                        checked += 1;
                    }
                }
            }
        }
        assert_eq!(checked, 3 * 2 * (1 + 1 + 3 + 1), "every case ran");
    }

    /// **An opaque paint inside a group calls no `blend`** (P-25.4's guard),
    /// counted: a fill of full coverage, the document's ink in an ink buffer,
    /// an image's or a shading's pixels at a full level and a nested group's
    /// fully opaque composite, each into an isolated, a non-isolated and a
    /// knockout group's buffer in every group format, make none; and a pixel
    /// covered in part, another mode and an alpha below one still take
    /// `blend`.
    #[test]
    fn an_opaque_paint_inside_a_group_skips_blend() {
        let mut state = 0x1dea_5eed_u32;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state
        };
        let colour = Color::rgb(220, 40, 120);
        for format in GROUP_FORMATS {
            let ink = (format == PixelFormat::CmykA8).then_some([10, 250, 60, 30]);
            for (what, group) in groups_over_a_page(format, &mut next) {
                let case = format!("{what}, {format:?}");
                let whole = Mask::uniform(3, 2, 16, 11, 255);

                let mut painted = group.clone();
                blends::take();
                painted.fill_mask_inked(&whole, colour, ink, 1.0, BlendMode::Normal);
                assert_eq!(blends::take(), 0, "{case}: a fill of full coverage");
                assert_eq!(
                    painted.pixel(5, 5).map(|c| c.a),
                    Some(255),
                    "{case}: opaque"
                );

                let mut sampled = group.clone();
                blends::take();
                for row in 0..group.height {
                    for col in 0..group.width {
                        sampled.blend_pixel_level(col, row, colour, ink, 255, BlendMode::Normal);
                    }
                }
                assert_eq!(blends::take(), 0, "{case}: sampled pixels at a full level");
                assert_eq!(
                    sampled.data, painted.data,
                    "{case}: the same paint either way"
                );

                let nested = Canvas::new(10, 7, format, colour).at_origin(6, 4);
                let mut composited = group.clone();
                blends::take();
                composited.composite(&nested, (6, 4), 1.0, BlendMode::Normal, None, None);
                composited.composite(&nested, (6, 4), 1.0, BlendMode::Normal, Some(&whole), None);
                assert_eq!(blends::take(), 0, "{case}: an opaque nested group");

                let mut knocked = group.clone();
                blends::take();
                knocked.knock_out(&whole);
                knocked.fill_mask_inked(&whole, colour, ink, 1.0, BlendMode::Normal);
                assert_eq!(blends::take(), 0, "{case}: restored and painted");
                assert_eq!(
                    knocked.data, painted.data,
                    "{case}: full coverage replaces all"
                );

                // What still blends.
                let blended = |paint: &dyn Fn(&mut Canvas)| {
                    let mut canvas = group.clone();
                    blends::take();
                    paint(&mut canvas);
                    blends::take()
                };
                let mut edge = whole.clone();
                edge.data[17] = 128;
                assert_eq!(
                    blended(&|c| c.fill_mask_inked(&edge, colour, ink, 1.0, BlendMode::Normal)),
                    1,
                    "{case}: one pixel covered in part"
                );
                assert!(
                    blended(&|c| c.fill_mask_inked(&whole, colour, ink, 1.0, BlendMode::Multiply))
                        > 0,
                    "{case}: another mode"
                );
                assert!(
                    blended(&|c| c.fill_mask_inked(&whole, colour, ink, 0.5, BlendMode::Normal))
                        > 0,
                    "{case}: half alpha"
                );
                assert!(
                    blended(&|c| c.blend_pixel_level(4, 4, colour, ink, 254, BlendMode::Normal))
                        > 0,
                    "{case}: a level below full"
                );
            }
        }
    }

    // ---- The plates format ------------------------------------------------

    /// The twelve separable modes (11.3.5.2), `Normal` first.
    const SEPARABLE: [BlendMode; 12] = [
        BlendMode::Normal,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Overlay,
        BlendMode::Darken,
        BlendMode::Lighten,
        BlendMode::ColorDodge,
        BlendMode::ColorBurn,
        BlendMode::HardLight,
        BlendMode::SoftLight,
        BlendMode::Difference,
        BlendMode::Exclusion,
    ];

    /// The separable modes that preserve white (11.7.4.2): all but
    /// `Difference` and `Exclusion`.
    const WHITE_PRESERVING: [BlendMode; 10] = [
        BlendMode::Normal,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Overlay,
        BlendMode::Darken,
        BlendMode::Lighten,
        BlendMode::ColorDodge,
        BlendMode::ColorBurn,
        BlendMode::HardLight,
        BlendMode::SoftLight,
    ];

    fn spots(count: usize) -> Spots {
        Spots::new(count).expect("under the cap")
    }

    /// A mask over device pixels `x0..x0 + width`, `y0..y0 + height`, every
    /// pixel at `coverage`.
    fn solid(x0: i32, y0: i32, width: u32, height: u32, coverage: u8) -> Mask {
        Mask {
            x0,
            y0,
            width,
            height,
            data: vec![coverage; (width * height) as usize],
            outside: 0,
        }
    }

    /// A mask over the 8 by 8 pixels at the origin whose coverage steps
    /// through the byte: every pixel a different partial cover, zero and
    /// full among them.
    fn ramp() -> Mask {
        Mask {
            x0: 0,
            y0: 0,
            width: 8,
            height: 8,
            data: (0..64u32).map(|i| (i * 255 / 63) as u8).collect(),
            outside: 0,
        }
    }

    /// The four process channels and the alpha of every pixel of a plates
    /// canvas, as a `CmykA8` canvas lays them out.
    fn process_and_alpha(canvas: &Canvas) -> Vec<u8> {
        let components = canvas.format.components();
        canvas
            .data
            .chunks_exact(components)
            .flat_map(|pixel| {
                let mut kept = pixel[..4].to_vec();
                kept.push(pixel[components - 1]);
                kept
            })
            .collect()
    }

    /// **Four process channels composite as `CmykA8` does** (RC-04.3).
    ///
    /// The same masks, inks, alphas and separable modes, over the same
    /// backdrops — opaque white, a colour turned into ink, and nothing at
    /// all — painted on a `CmykA8` canvas through `fill_mask_inked` and on
    /// plates through `fill_mask_plates`, with no spot and with three: the
    /// plates with none are the `CmykA8` bytes, and the plates with three
    /// carry the `CmykA8` bytes on their process channels and alpha. A light
    /// colour on plates through `fill_mask_inked` is its light turned into
    /// ink, as on `CmykA8`.
    #[test]
    fn four_process_channels_composite_as_cmyka8_does() {
        let masks = [
            square_mask(0.0, 0.0, 8.0, 8.0, 8),
            square_mask(1.3, 0.6, 5.2, 4.7, 8),
            ramp(),
            square_mask(2.5, 2.5, 3.0, 3.0, 8),
        ];
        let inks: [[u8; 4]; 4] = [
            [255, 0, 0, 0],
            [12, 200, 77, 31],
            [255, 255, 255, 255],
            [0, 90, 0, 140],
        ];
        for background in [Color::WHITE, Color::rgb(40, 160, 220), Color::TRANSPARENT] {
            for mode in SEPARABLE {
                for alpha in [1.0, 0.5, 0.3] {
                    let mut ink = Canvas::new(8, 8, PixelFormat::CmykA8, background);
                    let mut none = Canvas::plates(8, 8, 0, background).expect("under the cap");
                    let mut three = Canvas::plates(8, 8, 3, background).expect("under the cap");
                    for (index, (mask, cmyk)) in masks.iter().zip(inks).enumerate() {
                        // The first paint is the backdrop, `Normal` and whole.
                        let (mode, alpha) = if index == 0 {
                            (BlendMode::Normal, 1.0)
                        } else {
                            (mode, alpha)
                        };
                        let [c, m, y, k] = cmyk;
                        let (r, g, b) = cmyk_to_rgb(c, m, y, k);
                        ink.fill_mask_inked(mask, Color::rgb(r, g, b), Some(cmyk), alpha, mode);
                        none.fill_mask_plates(
                            mask,
                            &PlateInk::process(spots(0), cmyk),
                            alpha,
                            mode,
                        );
                        three.fill_mask_plates(
                            mask,
                            &PlateInk::process(spots(3), cmyk),
                            alpha,
                            mode,
                        );
                    }
                    let case = format!("{mode:?} at {alpha} over {background:?}");
                    assert_eq!(none.data, ink.data, "no spot: {case}");
                    assert_eq!(process_and_alpha(&three), ink.data, "three spots: {case}");

                    let light = Color::rgb(200, 30, 90);
                    let mut lit_ink = ink.clone();
                    let mut lit_plates = three.clone();
                    lit_ink.fill_mask_inked(&masks[1], light, None, alpha, mode);
                    lit_plates.fill_mask_inked(&masks[1], light, None, alpha, mode);
                    assert_eq!(
                        process_and_alpha(&lit_plates),
                        lit_ink.data,
                        "light: {case}"
                    );
                }
            }
        }
    }

    /// A spot channel is a channel like a process one: under every separable
    /// mode that preserves white, the same tints over the same backdrop give
    /// a spot channel the bytes magenta gets.
    #[test]
    fn a_spot_channel_composites_as_a_process_channel_does() {
        for mode in WHITE_PRESERVING {
            for alpha in [1.0, 0.6] {
                let mut canvas =
                    Canvas::plates(8, 8, 2, Color::TRANSPARENT).expect("under the cap");
                let backdrop = PlateInk::named(spots(2), &[(1, 180), (5, 180)]);
                canvas.fill_mask_plates(
                    &square_mask(0.0, 0.0, 6.0, 6.0, 8),
                    &backdrop,
                    0.8,
                    BlendMode::Normal,
                );
                let paint = PlateInk::named(spots(2), &[(1, 70), (5, 70)]);
                canvas.fill_mask_plates(&ramp(), &paint, alpha, mode);
                assert_eq!(
                    canvas.channel(1),
                    canvas.channel(5),
                    "{mode:?} at {alpha}: the spot is magenta's twin"
                );
            }
        }
    }

    /// **A paint knocks out the channels it does not name** (RC-04.3; 8.6.7
    /// with overprint off): where it covers, every channel it names takes its
    /// tint and every other takes no ink, process and spot alike; where it
    /// covers part of a pixel, or paints at part of an alpha, each knocked-out
    /// channel keeps what 11.3.6's composite of no ink over it leaves.
    #[test]
    fn a_paint_knocks_out_the_channels_it_does_not_name() {
        let mut canvas = Canvas::plates(8, 8, 4, Color::WHITE).expect("under the cap");
        let first = PlateInk::named(spots(4), &[(0, 200), (5, 120), (7, 33)]);
        canvas.fill_mask_plates(&solid(0, 0, 8, 8, 255), &first, 1.0, BlendMode::Normal);
        let second = PlateInk::named(spots(4), &[(2, 90), (6, 60)]);
        canvas.fill_mask_plates(&solid(2, 2, 4, 4, 255), &second, 1.0, BlendMode::Normal);

        let inside = |canvas: &Canvas, channel: usize| {
            canvas.channel(channel).expect("a channel")[3 * 8 + 3]
        };
        let outside =
            |canvas: &Canvas, channel: usize| canvas.channel(channel).expect("a channel")[0];
        for channel in 0..8 {
            let named = match channel {
                2 => 90,
                6 => 60,
                _ => 0,
            };
            assert_eq!(inside(&canvas, channel), named, "inside, channel {channel}");
            let before = match channel {
                0 => 200,
                5 => 120,
                7 => 33,
                _ => 0,
            };
            assert_eq!(
                outside(&canvas, channel),
                before,
                "outside, channel {channel}"
            );
        }

        // At half an alpha, over an opaque backdrop: `cs·a + cb·(1 − a)` in
        // the crate's integer steps, a named channel's `cs` its tint and a
        // knocked-out one's no ink.
        let mut half = Canvas::plates(8, 8, 4, Color::WHITE).expect("under the cap");
        half.fill_mask_plates(&solid(0, 0, 8, 8, 255), &first, 1.0, BlendMode::Normal);
        half.fill_mask_plates(&solid(0, 0, 8, 8, 255), &second, 0.5, BlendMode::Normal);
        let a = 128;
        for channel in 0..8 {
            let cs = match channel {
                2 => 90,
                6 => 60,
                _ => 0,
            };
            let cb = match channel {
                0 => 200,
                5 => 120,
                7 => 33,
                _ => 0,
            };
            let want = (mul255(cs, a) + mul255(cb, 255 - a)).min(255) as u8;
            assert_eq!(
                outside(&half, channel),
                want,
                "half alpha, channel {channel}"
            );
        }
        assert_eq!(half.data[8], 255, "the alpha of an opaque page stays whole");
    }

    /// **Sixty-eight channels each keep their own ink** (RC-04.3): a canvas
    /// at the cap paints each of its channels in a column of its own, and
    /// each plate read out holds its tint in its column and no ink anywhere
    /// else; one paint naming all sixty-eight puts each tint on its own plate.
    #[test]
    fn sixty_eight_channels_each_keep_their_own_ink() {
        let channels = 4 + MAX_SPOT_PLATES;
        let tint = |channel: usize| (channel * 3 + 7) as u8;
        let mut canvas =
            Canvas::plates(channels as u32, 2, MAX_SPOT_PLATES, Color::WHITE).expect("at the cap");
        assert_eq!(canvas.format.components(), 69);
        for channel in 0..channels {
            let ink = PlateInk::named(spots(MAX_SPOT_PLATES), &[(channel, tint(channel))]);
            canvas.fill_mask_plates(
                &solid(channel as i32, 0, 1, 2, 255),
                &ink,
                1.0,
                BlendMode::Normal,
            );
        }
        for channel in 0..channels {
            let plate = canvas.channel(channel).expect("a plate");
            for (index, value) in plate.iter().enumerate() {
                let want = if index % channels == channel {
                    tint(channel)
                } else {
                    0
                };
                assert_eq!(*value, want, "plate {channel} at pixel {index}");
            }
        }
        assert_eq!(canvas.channel(channels), None, "the alpha is no plate");

        let all: Vec<(usize, u8)> = (0..channels)
            .map(|channel| (channel, tint(channel)))
            .collect();
        let mut whole =
            Canvas::plates(3, 3, MAX_SPOT_PLATES, Color::TRANSPARENT).expect("at the cap");
        whole.fill_mask_plates(
            &solid(0, 0, 3, 3, 255),
            &PlateInk::named(spots(MAX_SPOT_PLATES), &all),
            1.0,
            BlendMode::Normal,
        );
        for channel in 0..channels {
            assert_eq!(
                whole.channel(channel),
                Some(vec![tint(channel); 9]),
                "plate {channel}"
            );
        }
        assert!(
            whole.data.chunks_exact(69).all(|pixel| pixel[68] == 255),
            "opaque"
        );
    }

    /// **A canvas past the channel cap is refused by name** (RC-04.3): the
    /// cap is sixty-four spots, a count past it is `TooManySpots` saying how
    /// many were asked for, and no plates canvas is made of it.
    #[test]
    fn a_canvas_past_the_channel_cap_is_refused_by_name() {
        assert_eq!(MAX_SPOT_PLATES, 64);
        assert_eq!(Spots::new(MAX_SPOT_PLATES).map(Spots::count), Ok(64));
        assert_eq!(
            Spots::new(MAX_SPOT_PLATES + 1),
            Err(TooManySpots { asked: 65 })
        );
        assert_eq!(
            Spots::new(usize::MAX),
            Err(TooManySpots { asked: usize::MAX })
        );
        let refused = Canvas::plates(4, 4, 65, Color::WHITE).expect_err("past the cap");
        assert_eq!(refused, TooManySpots { asked: 65 });
        assert_eq!(
            refused.to_string(),
            "a plates canvas of 65 spot channels, past the cap of 64"
        );
        let at_cap = Canvas::plates(4, 4, 64, Color::WHITE).expect("at the cap");
        assert_eq!(at_cap.data.len(), 4 * 4 * 69);
        assert_eq!(Spots::NONE.count(), 0);
        assert_eq!(Spots::default(), Spots::NONE);
    }

    /// A plates canvas starts as `CmykA8` starts — its background's light
    /// turned into ink on the process channels and its alpha — with no ink on
    /// any spot, and a plates canvas is allocated for its own spots.
    #[test]
    fn a_plates_canvas_starts_at_its_background_as_cmyka8_does() {
        for background in [Color::WHITE, Color::TRANSPARENT, Color::rgb(10, 200, 90)] {
            let ink = Canvas::new(2, 2, PixelFormat::CmykA8, background);
            let plates = Canvas::plates(2, 2, 2, background).expect("under the cap");
            assert_eq!(plates.data.len(), 2 * 2 * 7);
            assert_eq!(plates.stride, 2 * 7);
            for (pixel, from) in plates.data.chunks_exact(7).zip(ink.data.chunks_exact(5)) {
                assert_eq!(&pixel[..4], &from[..4], "{background:?}");
                assert_eq!(&pixel[4..6], &[0, 0], "no ink on a spot");
                assert_eq!(pixel[6], from[4], "the alpha");
            }
        }
    }

    /// A plates pixel reads back as its process channels' light, 8.6.4.4 as a
    /// `CmykA8` pixel reads, with its alpha; a spot's colour is not this
    /// crate's to know.
    #[test]
    fn a_plates_canvas_reads_back_as_its_process_light() {
        let mut canvas = Canvas::plates(1, 1, 3, Color::TRANSPARENT).expect("under the cap");
        let ink = PlateInk::named(spots(3), &[(0, 255), (3, 51), (4, 200), (6, 255)]);
        canvas.fill_mask_plates(&solid(0, 0, 1, 1, 255), &ink, 1.0, BlendMode::Normal);
        let (r, g, b) = cmyk_to_rgb(255, 0, 0, 51);
        assert_eq!(canvas.pixel(0, 0), Some(Color { r, g, b, a: 255 }));
        assert_eq!(canvas.pixel(1, 0), None);
        let mask = canvas.to_mask((0, 0), MaskKind::Alpha, None);
        assert_eq!(mask.data, vec![255], "its alpha reads as a mask");
    }

    /// One channel read out is that channel's bytes, row by row, on plates
    /// and on any other format; past the colour channels there is none.
    #[test]
    fn a_plate_is_one_channel_read_out() {
        let mut canvas = Canvas::plates(3, 2, 1, Color::WHITE).expect("under the cap");
        canvas.fill_mask_plates(
            &solid(1, 0, 2, 2, 255),
            &PlateInk::named(spots(1), &[(4, 77), (1, 9)]),
            1.0,
            BlendMode::Normal,
        );
        assert_eq!(canvas.channel(4), Some(vec![0, 77, 77, 0, 77, 77]));
        assert_eq!(canvas.channel(1), Some(vec![0, 9, 9, 0, 9, 9]));
        assert_eq!(canvas.channel(0), Some(vec![0; 6]));
        assert_eq!(canvas.channel(5), None, "the alpha");
        assert_eq!(canvas.channel(500), None);

        let rgb = Canvas::new(2, 1, PixelFormat::Rgb8, Color::rgb(1, 2, 3));
        assert_eq!(rgb.channel(2), Some(vec![3, 3]));
        assert_eq!(rgb.channel(3), None);
    }

    /// A colour handed to plates as a fill or a pixel paints its ink on the
    /// process channels, as on `CmykA8` — its own components where it has
    /// them, its light turned into ink where it has not — and knocks out
    /// every spot.
    #[test]
    fn a_colour_on_plates_paints_its_process_ink() {
        let full = solid(0, 0, 2, 2, 255);
        let mut canvas = Canvas::plates(2, 2, 2, Color::WHITE).expect("under the cap");
        canvas.fill_mask_plates(
            &full,
            &PlateInk::named(spots(2), &[(4, 99), (5, 98)]),
            1.0,
            BlendMode::Normal,
        );
        canvas.fill_mask_inked(
            &full,
            Color::BLACK,
            Some([255, 255, 255, 255]),
            1.0,
            BlendMode::Normal,
        );
        assert_eq!(
            &canvas.data[..7],
            &[255, 255, 255, 255, 0, 0, 255],
            "the rich black"
        );
        canvas.fill_mask_inked(&full, Color::rgb(255, 0, 0), None, 1.0, BlendMode::Normal);
        assert_eq!(
            &canvas.data[..7],
            &[0, 255, 255, 0, 0, 0, 255],
            "red light as ink"
        );

        canvas.blend_pixel_inked(
            1,
            1,
            Color::BLACK,
            Some([10, 20, 30, 40]),
            1.0,
            BlendMode::Normal,
        );
        assert_eq!(
            &canvas.data[21..28],
            &[10, 20, 30, 40, 0, 0, 255],
            "a sampled pixel"
        );
        canvas.blend_pixel_inked(0, 1, Color::rgb(0, 0, 0), None, 1.0, BlendMode::Normal);
        assert_eq!(
            &canvas.data[14..21],
            &[0, 0, 0, 255, 0, 0, 255],
            "black light is K"
        );
        canvas.blend_pixel_inked(9, 9, Color::BLACK, None, 1.0, BlendMode::Normal);
    }

    /// A plate ink on a canvas that is not plates paints its process tints
    /// as the DeviceCMYK colour they are: that colour's ink on `CmykA8` and
    /// its light on every other format, the spots, which no other format
    /// carries, dropped.
    #[test]
    fn a_plate_ink_on_another_format_paints_its_process_colour() {
        let full = solid(0, 0, 2, 2, 255);
        let ink = PlateInk::named(spots(2), &[(0, 30), (2, 60), (4, 255), (5, 255)]);
        let mut cmyk = Canvas::new(2, 2, PixelFormat::CmykA8, Color::WHITE);
        cmyk.fill_mask_plates(&full, &ink, 1.0, BlendMode::Normal);
        assert_eq!(&cmyk.data[..5], &[30, 0, 60, 0, 255]);
        let mut rgb = Canvas::new(2, 2, PixelFormat::Rgb8, Color::WHITE);
        rgb.fill_mask_plates(&full, &ink, 1.0, BlendMode::Normal);
        let (r, g, b) = cmyk_to_rgb(30, 0, 60, 0);
        assert_eq!(&rgb.data[..3], &[r, g, b]);
    }

    /// Ruling 5: a plates canvas standing at a device pixel takes a mask in
    /// device pixels, and its own pixel `(col, row)` is device pixel
    /// `(x0 + col, y0 + row)`.
    #[test]
    fn plates_stand_at_their_origin_in_the_page_frame() {
        let mut canvas = Canvas::plates(4, 4, 1, Color::WHITE)
            .expect("under the cap")
            .at_origin(10, 20);
        let ink = PlateInk::named(spots(1), &[(4, 200)]);
        canvas.fill_mask_plates(&solid(11, 21, 2, 2, 255), &ink, 1.0, BlendMode::Normal);
        let plate = canvas.channel(4).expect("the spot");
        for row in 0..4 {
            for col in 0..4 {
                let want = if (1..3).contains(&col) && (1..3).contains(&row) {
                    200
                } else {
                    0
                };
                assert_eq!(plate[row * 4 + col], want, "({col}, {row})");
            }
        }
    }

    /// Plates copy across as ink: plates to plates keeps every tint, plates
    /// to `CmykA8` its process inks and to light their light, and ink or
    /// light into plates lands on the process channels with no spot.
    #[test]
    fn plates_extract_as_ink_and_light() {
        let mut canvas = Canvas::plates(2, 2, 2, Color::WHITE).expect("under the cap");
        let ink = PlateInk::named(spots(2), &[(0, 40), (3, 80), (5, 120)]);
        canvas.fill_mask_plates(&solid(0, 0, 1, 2, 255), &ink, 1.0, BlendMode::Normal);
        let format = canvas.format;

        let same = canvas.extract((0, 0), 2, 2, format);
        assert_eq!(same.data, canvas.data);
        let cmyk = canvas.extract((0, 0), 2, 2, PixelFormat::CmykA8);
        assert_eq!(&cmyk.data[..5], &[40, 0, 0, 80, 255]);
        let light = canvas.extract((0, 0), 2, 2, PixelFormat::Rgba8);
        let (r, g, b) = cmyk_to_rgb(40, 0, 0, 80);
        assert_eq!(&light.data[..4], &[r, g, b, 255]);
        let off = canvas.extract((1, 0), 2, 2, format);
        assert_eq!(
            &off.data[7..14],
            &[0, 0, 0, 255, 0, 0, 0],
            "off the source: the transparent pixel a new canvas starts as"
        );

        let mut rich = Canvas::new(1, 1, PixelFormat::CmykA8, Color::TRANSPARENT);
        rich.fill_mask_inked(
            &solid(0, 0, 1, 1, 255),
            Color::BLACK,
            Some([255; 4]),
            1.0,
            BlendMode::Normal,
        );
        let inked = rich.extract((0, 0), 1, 1, format);
        assert_eq!(
            inked.data,
            vec![255, 255, 255, 255, 0, 0, 255],
            "ink stays ink"
        );
        let lit = Canvas::new(1, 1, PixelFormat::Rgb8, Color::rgb(0, 255, 255)).extract(
            (0, 0),
            1,
            1,
            format,
        );
        assert_eq!(
            lit.data,
            vec![255, 0, 0, 0, 0, 0, 255],
            "light turned into ink"
        );
    }

    /// Recolouring plates paints the colour as process ink on every colour
    /// channel, spots knocked out, and keeps each pixel's alpha.
    #[test]
    fn recolouring_plates_turns_the_colour_into_process_ink() {
        let mut canvas = Canvas::plates(2, 1, 1, Color::TRANSPARENT).expect("under the cap");
        canvas.fill_mask_plates(
            &solid(0, 0, 1, 1, 128),
            &PlateInk::named(spots(1), &[(4, 255)]),
            1.0,
            BlendMode::Normal,
        );
        canvas.recolor(Color::rgb(0, 0, 255));
        let (c, m, y, k) = rgb_to_cmyk(0, 0, 255);
        assert_eq!(&canvas.data[..6], &[c, m, y, k, 0, 128]);
        assert_eq!(&canvas.data[6..12], &[c, m, y, k, 0, 0]);
    }

    /// The ink's own constructors: a channel named twice keeps the later
    /// tint, a channel past the format's is passed over, and the process and
    /// light forms carry no spot.
    #[test]
    fn a_plate_ink_names_its_channels() {
        let ink = PlateInk::named(spots(1), &[(4, 1), (4, 2), (9, 99), (0, 5)]);
        assert_eq!(ink.tints(), &[5, 0, 0, 0, 2]);
        assert_eq!(PlateInk::none(spots(2)).tints(), &[0; 6]);
        assert_eq!(
            PlateInk::process(spots(1), [1, 2, 3, 4]).tints(),
            &[1, 2, 3, 4, 0]
        );
        let (c, m, y, k) = rgb_to_cmyk(10, 20, 30);
        assert_eq!(
            PlateInk::from_light(spots(1), Color::rgb(10, 20, 30)).tints(),
            &[c, m, y, k, 0]
        );
    }

    /// Degenerate plates are harmless (ruling 1): an empty canvas, alphas
    /// that are no alpha, and an ink built for another number of spots —
    /// the channels it lacks take no ink and the ones the canvas lacks are
    /// passed over.
    #[test]
    fn degenerate_plates_are_harmless() {
        let mut empty = Canvas::plates(0, 0, 3, Color::WHITE).expect("under the cap");
        empty.fill_mask_plates(
            &solid(0, 0, 1, 1, 255),
            &PlateInk::none(spots(3)),
            1.0,
            BlendMode::Normal,
        );
        assert!(empty.data.is_empty());
        assert_eq!(empty.channel(0), Some(Vec::new()));

        let mut canvas = Canvas::plates(2, 2, 2, Color::WHITE).expect("under the cap");
        for alpha in [0.0, -1.0, 2.0, f64::NAN, f64::INFINITY] {
            canvas.fill_mask_plates(
                &solid(0, 0, 2, 2, 255),
                &PlateInk::named(spots(2), &[(4, 9)]),
                alpha,
                BlendMode::Multiply,
            );
        }
        let narrow = PlateInk::named(spots(0), &[(1, 50)]);
        canvas.fill_mask_plates(&solid(0, 0, 2, 2, 255), &narrow, 1.0, BlendMode::Normal);
        assert_eq!(
            &canvas.data[..7],
            &[0, 50, 0, 0, 0, 0, 255],
            "the missing spots take no ink"
        );
        let wide = PlateInk::named(spots(9), &[(1, 60), (12, 255)]);
        canvas.fill_mask_plates(&solid(0, 0, 2, 2, 255), &wide, 1.0, BlendMode::Normal);
        assert_eq!(
            &canvas.data[..7],
            &[0, 60, 0, 0, 0, 0, 255],
            "the extra ones are passed over"
        );
    }

    // ---- Transparency on plates ---------------------------------------------

    /// Every blend mode: the twelve separable and the four non-separable.
    const EVERY_MODE: [BlendMode; 16] = [
        BlendMode::Normal,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Overlay,
        BlendMode::Darken,
        BlendMode::Lighten,
        BlendMode::ColorDodge,
        BlendMode::ColorBurn,
        BlendMode::HardLight,
        BlendMode::SoftLight,
        BlendMode::Difference,
        BlendMode::Exclusion,
        BlendMode::Hue,
        BlendMode::Saturation,
        BlendMode::Color,
        BlendMode::Luminosity,
    ];

    /// Paints `cmyk` through `mask` as the document's own ink: through
    /// `fill_mask_plates` on plates, through `fill_mask_inked` on `CmykA8`.
    fn ink(canvas: &mut Canvas, mask: &Mask, cmyk: [u8; 4], alpha: f64, mode: BlendMode) {
        match canvas.format {
            PixelFormat::Plates { spots } => {
                canvas.fill_mask_plates(mask, &PlateInk::process(spots, cmyk), alpha, mode);
            }
            _ => {
                let [c, m, y, k] = cmyk;
                let (r, g, b) = cmyk_to_rgb(c, m, y, k);
                canvas.fill_mask_inked(mask, Color::rgb(r, g, b), Some(cmyk), alpha, mode);
            }
        }
    }

    /// A page of process ink with an isolated, a non-isolated and a knockout
    /// group on it, each painting process ink under `mode` and composited
    /// back at `alpha` under `mode` — every step a renderer takes for a group
    /// (11.4.4, 11.4.5, 11.4.7.2) — in `format`.
    fn grouped_page(format: PixelFormat, mode: BlendMode, alpha: f64) -> Canvas {
        let mut page = Canvas::new(12, 12, format, Color::WHITE);
        let whole = square_mask(0.0, 0.0, 12.0, 12.0, 12);
        ink(&mut page, &whole, [40, 0, 120, 10], 1.0, BlendMode::Normal);
        ink(
            &mut page,
            &square_mask(2.4, 1.3, 7.1, 8.6, 12),
            [0, 210, 30, 0],
            0.7,
            BlendMode::Multiply,
        );

        // Isolated: from nothing, holding the page while it is open.
        let mut isolated = Canvas::new(6, 6, format, Color::TRANSPARENT).at_origin(1, 1);
        isolated.hold_parent(page);
        ink(
            &mut isolated,
            &square_mask(1.5, 1.5, 4.0, 3.0, 12),
            [255, 0, 0, 40],
            1.0,
            BlendMode::Normal,
        );
        ink(
            &mut isolated,
            &square_mask(2.2, 2.7, 4.5, 4.4, 12),
            [0, 90, 200, 0],
            0.6,
            mode,
        );
        let mut page = isolated.release_parent().expect("the group holds its page");
        page.composite(&isolated, (1, 1), alpha, mode, None, None);

        // Non-isolated: the page's own ink its backdrop, taken out at the close.
        let mut adopted = Canvas::new(7, 7, format, Color::TRANSPARENT).at_origin(4, 3);
        adopted.adopt_backdrop(page);
        ink(
            &mut adopted,
            &square_mask(4.6, 3.2, 5.0, 4.1, 12),
            [255, 255, 255, 255],
            0.8,
            mode,
        );
        ink(
            &mut adopted,
            &square_mask(6.3, 5.5, 3.9, 4.2, 12),
            [10, 20, 30, 200],
            0.5,
            mode,
        );
        let mut page = adopted.release_parent().expect("the group holds its page");
        adopted.remove_backdrop(&page);
        page.composite(&adopted, (4, 3), alpha, mode, None, None);

        // Knockout: each element over the group's initial state, the page's
        // ink at no alpha.
        let mut knockout = Canvas::new(8, 8, format, Color::TRANSPARENT).at_origin(3, 4);
        knockout.adopt_backdrop(page);
        for (mask, cmyk) in [
            (square_mask(3.5, 4.5, 6.0, 5.0, 12), [200, 0, 0, 0]),
            (square_mask(5.2, 6.1, 5.0, 5.3, 12), [0, 0, 180, 60]),
        ] {
            knockout.knock_out(&mask);
            ink(&mut knockout, &mask, cmyk, 0.9, mode);
        }
        let mut page = knockout.release_parent().expect("the group holds its page");
        knockout.remove_backdrop(&page);
        page.composite(&knockout, (3, 4), alpha, mode, None, None);

        // An isolated knockout group: each element over nothing.
        let mut isolated = Canvas::new(5, 5, format, Color::TRANSPARENT).at_origin(6, 6);
        isolated.hold_parent(page);
        for (mask, cmyk) in [
            (square_mask(6.5, 6.5, 4.0, 4.0, 12), [0, 255, 0, 0]),
            (square_mask(7.5, 7.0, 3.5, 4.5, 12), [30, 0, 0, 90]),
        ] {
            isolated.knock_out(&mask);
            ink(&mut isolated, &mask, cmyk, 0.7, mode);
        }
        let mut page = isolated.release_parent().expect("the group holds its page");
        page.composite(&isolated, (6, 6), alpha, mode, None, None);
        page
    }

    /// **Spot groups composite as CMYK groups do** (RC-04.4): an isolated, a
    /// non-isolated and a knockout group of process ink, under every mode and
    /// at two alphas, give the plates with no spot a `CmykA8` page's bytes,
    /// backdrop removal included, and the plates with three spots those bytes
    /// on their process channels and alpha, no ink reaching a spot; the
    /// non-separable blends taken through light are counted alike.
    #[test]
    fn spot_groups_composite_as_cmyk_groups_do() {
        for mode in EVERY_MODE {
            for alpha in [1.0, 0.6] {
                let ink = grouped_page(PixelFormat::CmykA8, mode, alpha);
                let none = grouped_page(PixelFormat::Plates { spots: spots(0) }, mode, alpha);
                let three = grouped_page(PixelFormat::Plates { spots: spots(3) }, mode, alpha);
                let case = format!("{mode:?} at {alpha}");
                assert_eq!(none.data, ink.data, "no spot: {case}");
                assert_eq!(process_and_alpha(&three), ink.data, "three spots: {case}");
                for spot in 4..7 {
                    assert!(
                        three
                            .channel(spot)
                            .expect("a spot")
                            .iter()
                            .all(|tint| *tint == 0),
                        "no ink on spot {spot}: {case}"
                    );
                }
                assert_eq!(
                    three.approximated_blends(),
                    ink.approximated_blends(),
                    "counted alike: {case}"
                );
            }
        }
    }

    /// The twin of `a_backdrop_of_ink_is_adopted_and_removed_as_that_ink` on
    /// plates: a non-isolated group adopts the page's own tints, spots among
    /// them, with none of the group's own alpha (11.4.4); half of a magenta
    /// and spot paint, the backdrop taken out (11.4.7.2), leaves the group's
    /// own ink; and compositing it back reconstructs the mixture.
    #[test]
    fn a_plates_backdrop_is_adopted_and_removed_as_its_tints() {
        let whole = solid(0, 0, 4, 4, 255);
        let mut page = Canvas::plates(4, 4, 2, Color::WHITE).expect("under the cap");
        let rich = PlateInk::named(
            spots(2),
            &[(0, 255), (1, 255), (2, 255), (3, 255), (5, 180)],
        );
        page.fill_mask_plates(&whole, &rich, 1.0, BlendMode::Normal);

        let mut group = Canvas::plates(4, 4, 2, Color::TRANSPARENT).expect("under the cap");
        group.adopt_backdrop(page);
        assert_eq!(
            group.data[..7],
            [255, 255, 255, 255, 0, 180, 0],
            "the page's tints, with none of the group's own alpha"
        );

        let paint = PlateInk::named(spots(2), &[(1, 255), (4, 255)]);
        group.fill_mask_plates(&whole, &paint, 0.5, BlendMode::Normal);
        let mixed = group.data[..7].to_vec();
        let mut page = group.release_parent().expect("the group holds its page");
        assert_eq!(
            page.data[..7],
            [255, 255, 255, 255, 0, 180, 255],
            "the page is its own"
        );
        group.remove_backdrop(&page);
        let own = &group.data[..6];
        assert!(
            own.iter()
                .zip([0u8, 255, 0, 0, 255, 0])
                .all(|(got, want)| got.abs_diff(want) <= 3),
            "the group's own ink is magenta and the first spot, got {own:?}"
        );
        assert_eq!(group.data[6], mixed[6], "at the alpha it painted with");

        page.composite(&group, (0, 0), 1.0, BlendMode::Normal, None, None);
        let back = &page.data[..6];
        assert!(
            back.iter()
                .zip(&mixed[..6])
                .all(|(got, want)| got.abs_diff(*want) <= 2),
            "round trip: {back:?} against {mixed:?}"
        );

        let mut isolated = Canvas::plates(2, 2, 1, Color::TRANSPARENT).expect("under the cap");
        isolated.fill_mask_plates(
            &solid(0, 0, 1, 1, 200),
            &PlateInk::named(spots(1), &[(4, 99)]),
            0.4,
            BlendMode::Normal,
        );
        let before = isolated.data.clone();
        let parent = Canvas::plates(2, 2, 1, Color::WHITE).expect("under the cap");
        isolated.hold_parent(parent);
        let parent = isolated
            .release_parent()
            .expect("the group holds its parent");
        isolated.remove_backdrop(&parent);
        assert_eq!(isolated.data, before, "no backdrop, no correction");
    }

    /// **A spot under a non-isolated group comes back as it went in**
    /// (RC-04.4): the group adopts the page's spot as its backdrop, paints
    /// process ink over half of it under `Multiply` — no ink in the spot,
    /// which a white-preserving mode leaves as it is — and nothing over the
    /// rest; with the backdrop taken out and the group composited back, the
    /// spot is the page's own tint everywhere, painted over or not, at a whole
    /// alpha and, where the group painted nothing, at half of one.
    #[test]
    fn a_spot_under_a_non_isolated_group_comes_back_as_it_went_in() {
        for alpha in [1.0, 0.5] {
            let mut page = Canvas::plates(8, 8, 2, Color::WHITE).expect("under the cap");
            page.fill_mask_plates(
                &solid(0, 0, 8, 8, 255),
                &PlateInk::named(spots(2), &[(0, 100)]),
                1.0,
                BlendMode::Normal,
            );
            page.fill_mask_plates(
                &solid(0, 0, 4, 8, 255),
                &PlateInk::named(spots(2), &[(0, 100), (4, 150)]),
                1.0,
                BlendMode::Normal,
            );
            let spot_before = page.channel(4).expect("the first spot");
            let cyan_before = page.channel(0).expect("cyan");

            let mut group = Canvas::plates(8, 8, 2, Color::TRANSPARENT).expect("under the cap");
            group.adopt_backdrop(page);
            group.fill_mask_plates(
                &solid(0, 0, 8, 4, 255),
                &PlateInk::named(spots(2), &[(1, 200)]),
                1.0,
                BlendMode::Multiply,
            );
            let mut page = group.release_parent().expect("the group holds its page");
            group.remove_backdrop(&page);
            page.composite(&group, (0, 0), alpha, BlendMode::Normal, None, None);

            let spot_after = page.channel(4).expect("the first spot");
            let cyan_after = page.channel(0).expect("cyan");
            let magenta = page.channel(1).expect("magenta");
            for index in 0..64 {
                let painted = index / 8 < 4;
                if painted && alpha < 1.0 {
                    continue;
                }
                assert_eq!(
                    spot_after[index], spot_before[index],
                    "the spot at {index}, alpha {alpha}"
                );
                assert_eq!(
                    cyan_after[index], cyan_before[index],
                    "cyan at {index}, alpha {alpha}"
                );
                let want = if painted { 200 } else { 0 };
                assert_eq!(magenta[index], want, "magenta at {index}, alpha {alpha}");
            }
        }
    }

    /// **The non-separable modes reach spot channels as transcribed**
    /// (RC-04.4; 11.7.3, 11.7.4.2): under `Difference`, `Exclusion` and the
    /// four non-separable modes the process channels take the mode — the
    /// non-separable ones through light, byte for byte as on `CmykA8`, and
    /// counted — while every spot channel takes `Normal`, the bytes the same
    /// paint under `Normal` leaves there; under a white-preserving mode a
    /// spot takes the mode.
    #[test]
    fn non_separable_modes_reach_spot_channels_as_transcribed() {
        let whole = solid(0, 0, 8, 8, 255);
        let backdrop = [(0, 30), (1, 160), (2, 0), (3, 90), (4, 200), (5, 60)];
        let paint = [(0, 220), (1, 10), (2, 140), (3, 0), (4, 90), (5, 0)];
        for mode in [
            BlendMode::Difference,
            BlendMode::Exclusion,
            BlendMode::Hue,
            BlendMode::Saturation,
            BlendMode::Color,
            BlendMode::Luminosity,
        ] {
            for background in [Color::WHITE, Color::TRANSPARENT] {
                for alpha in [1.0, 0.5] {
                    let case = format!("{mode:?} at {alpha} over {background:?}");
                    let mut plates = Canvas::plates(8, 8, 2, background).expect("under the cap");
                    plates.fill_mask_plates(
                        &whole,
                        &PlateInk::named(spots(2), &backdrop),
                        1.0,
                        BlendMode::Normal,
                    );
                    plates.fill_mask_plates(
                        &ramp(),
                        &PlateInk::named(spots(2), &paint),
                        alpha,
                        mode,
                    );

                    let mut cmyk = Canvas::new(8, 8, PixelFormat::CmykA8, background);
                    ink(&mut cmyk, &whole, [30, 160, 0, 90], 1.0, BlendMode::Normal);
                    ink(&mut cmyk, &ramp(), [220, 10, 140, 0], alpha, mode);
                    assert_eq!(process_and_alpha(&plates), cmyk.data, "process: {case}");

                    let mut normal = Canvas::plates(8, 8, 2, background).expect("under the cap");
                    normal.fill_mask_plates(
                        &whole,
                        &PlateInk::named(spots(2), &backdrop),
                        1.0,
                        BlendMode::Normal,
                    );
                    normal.fill_mask_plates(
                        &ramp(),
                        &PlateInk::named(spots(2), &paint),
                        alpha,
                        BlendMode::Normal,
                    );
                    for spot in [4, 5] {
                        assert_eq!(
                            plates.channel(spot),
                            normal.channel(spot),
                            "spot {spot}: {case}"
                        );
                    }
                    assert_eq!(
                        plates.approximated_blends(),
                        u32::from(mode.is_nonseparable()),
                        "a blend through light is counted: {case}"
                    );
                }
            }
        }

        // A white-preserving mode reaches a spot: under `Multiply` a spot of
        // 200 under a paint of 90 is their product's complement, not the 90
        // `Normal` leaves.
        let mut multiplied = Canvas::plates(1, 1, 1, Color::WHITE).expect("under the cap");
        multiplied.fill_mask_plates(
            &solid(0, 0, 1, 1, 255),
            &PlateInk::named(spots(1), &[(4, 200)]),
            1.0,
            BlendMode::Normal,
        );
        multiplied.fill_mask_plates(
            &solid(0, 0, 1, 1, 255),
            &PlateInk::named(spots(1), &[(4, 90)]),
            1.0,
            BlendMode::Multiply,
        );
        let want = 255 - mul255(255 - 200, 255 - 90);
        assert_eq!(multiplied.channel(4), Some(vec![want as u8]));
        assert_eq!(multiplied.approximated_blends(), 0, "nothing through light");
    }

    /// Plates composite across formats as ink and light: a plates group onto
    /// a `CmykA8` page lands its process ink, onto a light page their light,
    /// its spots, which neither carries, dropped; a light group onto plates is
    /// its light turned into ink, and an ink group its own ink, on the process
    /// channels, every spot under its shape knocked out (Table 146's last row:
    /// a group paints every component).
    #[test]
    fn plates_and_other_formats_composite_across_as_ink_and_light() {
        let whole = solid(0, 0, 2, 2, 255);
        let mut group = Canvas::plates(2, 2, 1, Color::TRANSPARENT).expect("under the cap");
        group.fill_mask_plates(
            &whole,
            &PlateInk::named(spots(1), &[(0, 90), (3, 30), (4, 200)]),
            1.0,
            BlendMode::Normal,
        );
        let mut cmyk = Canvas::new(2, 2, PixelFormat::CmykA8, Color::WHITE);
        cmyk.composite(&group, (0, 0), 1.0, BlendMode::Normal, None, None);
        assert_eq!(&cmyk.data[..5], &[90, 0, 0, 30, 255], "process ink, as ink");
        let mut light = Canvas::new(2, 2, PixelFormat::Rgba8, Color::WHITE);
        light.composite(&group, (0, 0), 1.0, BlendMode::Normal, None, None);
        let (r, g, b) = cmyk_to_rgb(90, 0, 0, 30);
        assert_eq!(&light.data[..4], &[r, g, b, 255], "and as light");

        let mut page = Canvas::plates(2, 2, 1, Color::WHITE).expect("under the cap");
        page.fill_mask_plates(
            &whole,
            &PlateInk::named(spots(1), &[(4, 200)]),
            1.0,
            BlendMode::Normal,
        );
        let mut red = Canvas::new(1, 1, PixelFormat::Rgba8, Color::TRANSPARENT);
        red.fill_mask(&solid(0, 0, 1, 1, 255), Color::rgb(255, 0, 0), 1.0);
        page.composite(&red, (0, 0), 1.0, BlendMode::Normal, None, None);
        assert_eq!(
            &page.data[..6],
            &[0, 255, 255, 0, 0, 255],
            "light as ink, the spot out"
        );
        let mut rich = Canvas::new(1, 1, PixelFormat::CmykA8, Color::TRANSPARENT);
        rich.fill_mask_inked(
            &solid(0, 0, 1, 1, 255),
            Color::BLACK,
            Some([255; 4]),
            1.0,
            BlendMode::Normal,
        );
        page.composite(&rich, (1, 0), 1.0, BlendMode::Normal, None, None);
        assert_eq!(
            &page.data[6..12],
            &[255, 255, 255, 255, 0, 255],
            "ink as ink"
        );
        assert_eq!(
            &page.data[12..18],
            &[0, 0, 0, 0, 200, 255],
            "outside the shapes, as it was"
        );
    }

    /// An uncoloured cell on plates paints its colour's light turned into
    /// ink, no ink on a spot, wherever the cell painted: what recolouring the
    /// cell and compositing it writes, byte for byte, as on every format.
    #[test]
    fn an_uncoloured_cell_tints_plates_as_its_colour_in_ink() {
        let mut cell = Canvas::plates(4, 4, 1, Color::TRANSPARENT).expect("under the cap");
        cell.fill_mask_plates(
            &square_mask(0.5, 0.5, 2.6, 2.2, 4),
            &PlateInk::named(spots(1), &[(4, 255), (0, 9)]),
            1.0,
            BlendMode::Normal,
        );
        let blue = Color::rgb(0, 0, 255);
        let mut tinted = Canvas::plates(4, 4, 1, Color::WHITE).expect("under the cap");
        tinted.composite_tinted(&cell, blue, (0, 0), 0.8, BlendMode::Normal, None, None);
        let mut recoloured = cell.clone();
        recoloured.recolor(blue);
        let mut composited = Canvas::plates(4, 4, 1, Color::WHITE).expect("under the cap");
        composited.composite(&recoloured, (0, 0), 0.8, BlendMode::Normal, None, None);
        assert_eq!(tinted.data, composited.data);
        assert!(tinted
            .channel(4)
            .expect("the spot")
            .iter()
            .all(|tint| *tint == 0));
        let (c, m, ..) = rgb_to_cmyk(0, 0, 255);
        assert!(tinted
            .channel(0)
            .expect("cyan")
            .iter()
            .any(|tint| *tint > 0 && *tint <= c));
        assert!(tinted
            .channel(1)
            .expect("magenta")
            .iter()
            .any(|tint| *tint > 0 && *tint <= m));
    }

    /// A plates composite stops when asked, between rows, and leaves what it
    /// had not reached as it was.
    #[test]
    fn a_plates_composite_stops_when_asked() {
        let mut group = Canvas::plates(4, 4, 1, Color::TRANSPARENT).expect("under the cap");
        group.fill_mask_plates(
            &solid(0, 0, 4, 4, 255),
            &PlateInk::named(spots(1), &[(4, 255)]),
            1.0,
            BlendMode::Normal,
        );
        let mut page = Canvas::plates(4, 4, 1, Color::WHITE).expect("under the cap");
        let before = page.data.clone();
        let stop = || true;
        page.composite(&group, (0, 0), 1.0, BlendMode::Normal, None, Some(&stop));
        assert_eq!(page.data, before, "stopped before the first row");
        page.composite(&group, (0, 0), 1.0, BlendMode::Normal, None, None);
        assert_eq!(
            page.channel(4),
            Some(vec![255; 16]),
            "and composited when not"
        );
    }
}
