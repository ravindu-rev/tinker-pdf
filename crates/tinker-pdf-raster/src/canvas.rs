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
    /// says happen over ink. It is not offered as a page format: see
    /// `Page::render`.
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
            PixelFormat::GrayA8 | PixelFormat::Rgba8 | PixelFormat::CmykA8 | PixelFormat::LabA8 => {
                true
            }
            PixelFormat::Gray8 | PixelFormat::Rgb8 => false,
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
        let value =
            (u32::from(self.r) * 300 + u32::from(self.g) * 590 + u32::from(self.b) * 110 + 500)
                / 1000;
        value.min(255) as u8
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
    /// The initial backdrop of a *non-isolated* transparency group (11.4.4).
    ///
    /// A non-isolated group starts with its backdrop composited in, so that a
    /// blend mode inside the group can see through to what was underneath.
    /// That leaves the buffer holding two different quantities at once: the
    /// colour channels carry the group **over** its backdrop, while the alpha
    /// channel must carry the group's *own* accumulated alpha, because
    /// 11.4.7.2's removal step is `C = Cn + (Cn - C0)·(a0/agn - a0)` and `agn`
    /// is that own alpha. The two are not recoverable from one another once
    /// the backdrop is opaque — `agn` divides out — so the backdrop is kept.
    ///
    /// `None` on every ordinary canvas and on every isolated group, where the
    /// backdrop's alpha is zero and all of the arithmetic below collapses to
    /// what it was.
    backdrop: Option<Box<Canvas>>,
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
            backdrop: None,
            approximated_blends: 0,
        };
        canvas.clear(background);
        canvas
    }

    /// Repaints every pixel.
    pub fn clear(&mut self, color: Color) {
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
    /// For the formats that exist today the two paths produce the same bytes —
    /// `Gray8`'s round trip survives because `luma` of a replicated grey is
    /// that grey exactly, its weights summing to 1000 — so this could be
    /// deleted tomorrow and no test would notice. It is here for the format
    /// after them. A buffer holding *subtractive* components has no lossless
    /// trip through an RGB `Color`: the relation is exact in one direction and
    /// a projection in the other, so a group nested inside another group of
    /// its own kind would have its ink re-derived at every composite — a
    /// picture, and a different one. Same-format copies must not go through
    /// `Color`, and the cheapest way to guarantee that is for them never to.
    ///
    /// `a_same_format_copy_is_the_bytes_it_started_as` pins the equivalence.
    fn source_from(&self, src: &Canvas, sx: u32, sy: u32) -> Option<([u8; 5], u32)> {
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
        match self.format {
            PixelFormat::Gray8 => [color.luma(), 0, 0, 0, 0],
            PixelFormat::GrayA8 => [color.luma(), color.a, 0, 0, 0],
            PixelFormat::Rgb8 => [color.r, color.g, color.b, 0, 0],
            PixelFormat::Rgba8 => [color.r, color.g, color.b, color.a, 0],
            PixelFormat::CmykA8 => {
                let (c, m, y, k) = rgb_to_cmyk(color.r, color.g, color.b);
                [c, m, y, k, color.a]
            }
            PixelFormat::LabA8 => {
                let (l, a, b) = lab_bytes(color);
                [l, a, b, color.a, 0]
            }
        }
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
        let alpha = if alpha.is_finite() {
            (alpha.clamp(0.0, 1.0) * 255.0).round() as u32
        } else {
            255
        };
        if alpha == 0 {
            return;
        }

        if mode.is_nonseparable() && self.format == PixelFormat::CmykA8 {
            self.approximated_blends = self.approximated_blends.saturating_add(1);
        }
        let components = self.format.components();
        let source = self.encode(color);
        let color_alpha = u32::from(color.a);

        // The mask's own rectangle, not the canvas. Outside it `Mask::at`
        // returns zero and every pixel is skipped, so walking the page was
        // always the same answer at the price of the page: a comma on A4 at
        // 300 dpi visited 8.4 million pixels to composite about two hundred.
        let (x0, y0, x1, y1) = mask.overlap(self.width, self.height);
        for row in y0..y1 {
            for col in x0..x1 {
                let coverage = u32::from(mask.at(col as i32, row as i32));
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
        if x >= self.width || y >= self.height {
            return;
        }
        let alpha = if alpha.is_finite() {
            (alpha.clamp(0.0, 1.0) * 255.0).round() as u32
        } else {
            return;
        };
        let effective = mul255(alpha, u32::from(color.a));
        if effective == 0 {
            return;
        }

        if mode.is_nonseparable() && self.format == PixelFormat::CmykA8 {
            self.approximated_blends = self.approximated_blends.saturating_add(1);
        }
        let components = self.format.components();
        let source = self.encode(color);
        let base = (y as usize) * self.stride + (x as usize) * components;
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

    /// How opaque the initial backdrop is at a pixel, zero without one.
    fn backdrop_alpha(&self, x: u32, y: u32) -> u32 {
        let Some(backdrop) = &self.backdrop else {
            return 0;
        };
        backdrop.alpha_at(x, y)
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
    /// `at` (11.3.6).
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
    /// clip or a soft mask in *this* canvas's coordinates. `src`'s own alpha
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
        let alpha = if alpha.is_finite() {
            (alpha.clamp(0.0, 1.0) * 255.0).round() as u32
        } else {
            255
        };
        if alpha == 0 {
            return;
        }

        // The source rectangle, mapped into this canvas and clipped to it.
        let (x0, y0, x1, y1) = place(at, src.width, src.height, self.width, self.height);
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
                    (i64::from(col) - i64::from(at.0)) as u32,
                    (i64::from(row) - i64::from(at.1)) as u32,
                );
                let Some((source, own)) = self.source_from(src, sx, sy) else {
                    continue;
                };
                let coverage = mask.map_or(255, |mask| u32::from(mask.at(col as i32, row as i32)));
                if coverage == 0 {
                    continue;
                }
                let effective = mul255(mul255(own, alpha), coverage);
                if effective == 0 {
                    continue;
                }

                let base = (row as usize) * self.stride + (col as usize) * components;
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

    /// A copy of a rectangle of this canvas, in `format`.
    ///
    /// Pixels outside this canvas come back transparent, which is the right
    /// answer for a group whose bounding box hangs off the edge of the page:
    /// there is no backdrop out there to blend against.
    #[must_use]
    pub fn extract(&self, at: (i32, i32), width: u32, height: u32, format: PixelFormat) -> Canvas {
        let mut out = Canvas::new(width, height, format, Color::TRANSPARENT);
        for row in 0..height {
            for col in 0..width {
                let (Some(x), Some(y)) = (
                    (i64::from(at.0) + i64::from(col)).try_into().ok(),
                    (i64::from(at.1) + i64::from(row)).try_into().ok(),
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

    /// Starts this buffer from `backdrop`, and remembers it (11.4.4).
    ///
    /// What a **non-isolated** group does: the backdrop is composited in so
    /// that a blend mode inside the group sees through to it. The alpha
    /// channel is deliberately left at zero — it is the group's *own*
    /// accumulation from here on, and 11.4.7.2 needs it separate from the
    /// backdrop's in order to take the backdrop out again.
    pub fn adopt_backdrop(&mut self, backdrop: Canvas) {
        // Format as well as size. Today the two cannot differ — a group buffer
        // takes its format from the canvas it will composite onto — but that
        // is an invariant of the *caller*, not of this method, and it is
        // exactly the invariant a group buffer in its own colour space breaks.
        // Mismatched formats here would walk one stride with the other's
        // channel count and read colour out of alignment: a picture, not a
        // panic.
        if backdrop.width != self.width
            || backdrop.height != self.height
            || backdrop.format != self.format
        {
            return;
        }
        let channels = color_channels(self.format);
        for row in 0..self.height {
            for col in 0..self.width {
                let Some(color) = backdrop.pixel(col, row) else {
                    continue;
                };
                let pixel = self.encode(color);
                let components = self.format.components();
                let base = (row as usize) * self.stride + (col as usize) * components;
                for (i, slot) in self.data.iter_mut().skip(base).take(channels).enumerate() {
                    *slot = pixel.get(i).copied().unwrap_or(0);
                }
            }
        }
        self.backdrop = Some(Box::new(backdrop));
    }

    /// Takes the initial backdrop back out of a non-isolated group's result
    /// (11.4.7.2).
    ///
    /// `C = Cn + (Cn - C0)·(a0/agn - a0)`. Without it the backdrop is counted
    /// twice — once because the group started from it and once because the
    /// group is composited back onto it — and the page comes out *darker*
    /// rather than broken, which is a plausible image and therefore the worst
    /// kind of wrong.
    ///
    /// Nothing happens on an isolated group, which has no stored backdrop and
    /// for which `a0` is zero and the correction vanishes anyway.
    pub fn remove_backdrop(&mut self) {
        let Some(backdrop) = self.backdrop.take() else {
            return;
        };
        if !self.format.has_alpha() {
            return;
        }
        let channels = color_channels(self.format);
        let components = self.format.components();

        for row in 0..self.height {
            for col in 0..self.width {
                let own = self.alpha_at(col, row);
                let initial = backdrop.alpha_at(col, row);
                if own == 0 || initial == 0 {
                    continue;
                }
                // (a0/agn - a0) in 1/255 units. `own` is non-zero here, so the
                // division is safe; it can be large when the group barely
                // painted, which is the known instability of the removal step
                // and is why every channel clamps.
                let factor = (i64::from(initial) * 255 / i64::from(own)) - i64::from(initial);
                let Some(base0) = backdrop.pixel(col, row).map(|c| backdrop.encode(c)) else {
                    continue;
                };
                let base = (row as usize) * self.stride + (col as usize) * components;
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

    /// A copy of this canvas's pixels, without any stored initial backdrop.
    ///
    /// What 11.4.5 keeps: the state a knockout group's buffer starts in, so
    /// that each element can be composited against it rather than against the
    /// elements before it. The backdrop is deliberately not carried over —
    /// this copy is only ever read from.
    #[must_use]
    pub fn snapshot(&self) -> Canvas {
        Canvas {
            width: self.width,
            height: self.height,
            format: self.format,
            stride: self.stride,
            data: self.data.clone(),
            backdrop: None,
            approximated_blends: 0,
        }
    }

    /// Puts `initial` back where `mask` covers (11.4.5).
    ///
    /// A knockout group's elements do not accumulate: each one composites
    /// against the group's *initial* backdrop, so whatever earlier elements
    /// left behind is discarded exactly where the new one paints. Full
    /// coverage is an exact replacement; partial coverage weights between the
    /// two, which is the usual approximation — the spec separates an object's
    /// shape from its alpha and this engine, like the buffers it inherits,
    /// carries only the one number.
    pub fn knock_out(&mut self, initial: &Canvas, mask: &Mask) {
        // See `adopt_backdrop`: the format check is the same insurance against
        // the same coming change. `initial` is a `snapshot` of this buffer
        // today, so it cannot differ — which is precisely why nothing would
        // notice if it started to.
        if initial.width != self.width
            || initial.height != self.height
            || initial.format != self.format
        {
            return;
        }
        let components = self.format.components();
        let (x0, y0, x1, y1) = mask.overlap(self.width, self.height);
        for row in y0..y1 {
            for col in x0..x1 {
                let coverage = u32::from(mask.at(col as i32, row as i32));
                if coverage == 0 {
                    continue;
                }
                let base = (row as usize) * self.stride + (col as usize) * components;
                for i in 0..components {
                    let (Some(from), Some(slot)) = (
                        initial.data.get(base + i).copied(),
                        self.data.get_mut(base + i),
                    ) else {
                        continue;
                    };
                    *slot = (mul255(u32::from(from), coverage)
                        + mul255(u32::from(*slot), 255 - coverage))
                    .min(255) as u8;
                }
            }
        }
    }

    /// Reads this buffer as a coverage mask over device pixels (11.6.5.2).
    ///
    /// `at` is where the buffer's top-left sits in device space, so the mask
    /// comes back addressed the way a clip is. `transfer` is `/TR`,
    /// pre-sampled to 256 entries — a function evaluated per pixel would be
    /// ruinous and would put a `libm` call on a pixel path (ruling 4).
    #[must_use]
    pub fn to_mask(&self, at: (i32, i32), kind: MaskKind, transfer: Option<&[u8; 256]>) -> Mask {
        let mut mask = Mask::empty(at.0, at.1, self.width, self.height);
        for row in 0..self.height {
            for col in 0..self.width {
                let Some(color) = self.pixel(col, row) else {
                    continue;
                };
                let value = match kind {
                    MaskKind::Luminosity => color.luminosity(),
                    MaskKind::Alpha => color.a,
                };
                let value = transfer.map_or(value, |lut| {
                    lut.get(value as usize).copied().unwrap_or(value)
                });
                let index = (row as usize) * (self.width as usize) + (col as usize);
                if let Some(slot) = mask.data.get_mut(index) {
                    *slot = value;
                }
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
    pub fn recolor(&mut self, color: Color) {
        if !self.format.has_alpha() {
            return;
        }
        let components = self.format.components();
        let channels = color_channels(self.format);
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
                let (r, g, b) = lab_colour(*px.first()?, *px.get(1)?, *px.get(2)?);
                Color {
                    r,
                    g,
                    b,
                    a: *px.get(3)?,
                }
            }
        })
    }
}

/// Where a `width` by `height` source placed at `at` lands on a `dst_width` by
/// `dst_height` canvas, as `(x0, y0, x1, y1)` with the far edges exclusive.
///
/// In `i64` throughout: a group's bounding box can be placed at any device
/// coordinate a content stream can name, and `at.0 + width` overflows an `i32`
/// long before anything about it is unreasonable.
fn place(
    at: (i32, i32),
    width: u32,
    height: u32,
    dst_width: u32,
    dst_height: u32,
) -> (u32, u32, u32, u32) {
    let span = |origin: i32, extent: u32, limit: u32| {
        let lo = i64::from(origin).clamp(0, i64::from(limit)) as u32;
        let hi = (i64::from(origin) + i64::from(extent)).clamp(0, i64::from(limit)) as u32;
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
fn lab_bytes(color: Color) -> (u8, u8, u8) {
    let (l, a, b) = tinker_pdf_color::srgb_to_lab(color.r, color.g, color.b);
    let byte = |v: f64| -> u8 { v.clamp(0.0, 255.0).round() as u8 };
    (byte(l / 100.0 * 255.0), byte(a + 128.0), byte(b + 128.0))
}

/// The inverse of [`lab_bytes`].
fn lab_colour(l: u8, a: u8, b: u8) -> (u8, u8, u8) {
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
/// function produces. Nothing in this engine authors CMYK components: a source
/// colour is flattened to sRGB at the resource seam long before a buffer sees
/// it, so every CMYK value in a group buffer originated here and round-trips.
/// The day components are carried through that seam, this comment is the one
/// to revisit.
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

/// `a * b / 255`, rounded, in integers.
pub(crate) fn mul255(a: u32, b: u32) -> u32 {
    let product = a * b + 128;
    (product + (product >> 8)) >> 8
}

/// How many colour channels a format carries, before any alpha.
fn color_channels(format: PixelFormat) -> usize {
    match format {
        PixelFormat::Gray8 | PixelFormat::GrayA8 => 1,
        PixelFormat::Rgb8 | PixelFormat::Rgba8 | PixelFormat::LabA8 => 3,
        PixelFormat::CmykA8 => 4,
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
        let cb = [
            pick(dst.first(), dst.first()),
            pick(dst.get(1), dst.first()),
            pick(dst.get(2), dst.first()),
        ];
        let cs = [
            pick(source.first(), source.first()),
            pick(source.get(1), source.first()),
            pick(source.get(2), source.first()),
        ];
        let mixed = mode.apply_nonseparable(cb, cs);
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

    /// And the copy itself, end to end: `extract` of a whole canvas in its own
    /// format reproduces it byte for byte.
    #[test]
    fn extracting_a_canvas_in_its_own_format_reproduces_it() {
        for format in [
            PixelFormat::Gray8,
            PixelFormat::GrayA8,
            PixelFormat::Rgb8,
            PixelFormat::Rgba8,
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
    use super::*;
    use crate::fill::fill;
    use crate::geom::{FillRule, Path};

    fn square_mask(x: f64, y: f64, w: f64, h: f64, size: u32) -> Mask {
        let mut path = Path::new();
        path.rect(x, y, w, h);
        fill(&path, FillRule::NonZero, 0, 0, size, size, 0.05, None)
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

        group.remove_backdrop();
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
    /// existing arithmetic path is untouched.
    #[test]
    fn an_isolated_group_has_nothing_to_remove() {
        let mut group = Canvas::new(2, 2, PixelFormat::Rgba8, Color::TRANSPARENT);
        group.blend_pixel(0, 0, Color::rgb(10, 200, 30), 0.4);
        let before = group.data.clone();
        group.remove_backdrop();
        assert_eq!(group.data, before, "no backdrop, no correction");
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
}
