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
            | PixelFormat::Bgra8 => true,
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
            | PixelFormat::LabA8 => false,
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
            origin: (0, 0),
            backdrop: None,
            approximated_blends: 0,
        };
        canvas.clear(background);
        canvas
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
            PixelFormat::Bgr8 => [color.b, color.g, color.r, 0, 0],
            PixelFormat::Bgra8 => [color.b, color.g, color.r, color.a, 0],
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
        let rect = mask.overlap_at(self.origin, self.width, self.height);
        if mode == BlendMode::Normal
            && color_alpha == 255
            && alpha == 255
            && self.backdrop.is_none()
            && self.format != PixelFormat::CmykA8
        {
            self.fill_opaque(mask, &source, rect);
        } else {
            self.fill_blended(mask, &source, color_alpha, alpha, mode, rect);
        }
    }

    /// [`Canvas::fill_mask_inked`]'s general composite: every pixel the mask
    /// covers through `blend`, at the coverage, the colour's alpha and the
    /// state's alpha multiplied, over the canvas pixels `(x0, y0, x1, y1)`.
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
    /// ([`over_opaque`]). A pixel that is not opaque goes through `blend`,
    /// whose rounding a pixel that is not opaque needs.
    ///
    /// **A row is walked in blocks of [`BLOCK`] pixels** ([`opaque_row`]),
    /// and the loops over a block's bytes are of a fixed width with no call
    /// inside, written so that the `+simd128` build vectorises them: a block
    /// of no coverage is passed over, a block of full coverage over opaque
    /// pixels is one slice copy of the encoded colour, any other block over
    /// opaque pixels takes the collapse at every byte ([`opaque_block`]), and
    /// a block holding a pixel that is not opaque, like the pixels past the
    /// last whole block, is taken a pixel at a time ([`opaque_pixel`]) —
    /// `blend` for each covered pixel that is not opaque, exactly as many as
    /// before.
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
            _ => self.encode(color),
        };
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
        let swatch = Canvas::new(1, 1, src.format, color);
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
    /// `at` is the rectangle's top-left in device pixels, and the copy stands
    /// there: its [`Canvas::origin`] is `at`.
    ///
    /// Pixels outside this canvas come back transparent, which is the right
    /// answer for a group whose bounding box hangs off the edge of the page:
    /// there is no backdrop out there to blend against.
    #[must_use]
    pub fn extract(&self, at: (i32, i32), width: u32, height: u32, format: PixelFormat) -> Canvas {
        let mut out = Canvas::new(width, height, format, Color::TRANSPARENT).at_origin(at.0, at.1);
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
            || backdrop.origin != self.origin
        {
            return;
        }
        // The backdrop's own bytes, through `source_from`'s same-format path —
        // the formats are equal, checked above. Going through `pixel` and
        // `encode` instead re-derived an ink buffer's ink from its light, and
        // an ink page holds ink that light has no spelling for: a rich black
        // the document chose came back as pure K, and a blend inside the group
        // saw a backdrop the page never had.
        let channels = color_channels(self.format);
        for row in 0..self.height {
            for col in 0..self.width {
                let Some((pixel, _)) = self.source_from(&backdrop, col, row) else {
                    continue;
                };
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
                // `C0` is the backdrop's own bytes, as `adopt_backdrop` copied
                // them in: the backdrop is in this buffer's format, so this is
                // `source_from`'s same-format read and no trip through light.
                let Some((base0, _)) = self.source_from(&backdrop, col, row) else {
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
            origin: self.origin,
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
/// AND over its pixels' alpha bytes: no coverage passes the block over; over
/// pixels all opaque, full coverage copies `solid` whole and any other takes
/// [`opaque_block`]; a block holding a pixel that is not opaque, and the
/// pixels past the last whole block, take [`opaque_pixel`] wherever their
/// coverage is not zero. Each class writes the bytes the pixel-by-pixel walk
/// writes, so the order and the grouping decide nothing.
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
        let opaque = !ALPHA
            || block.chunks_exact(N).fold(255u8, |held, pixel| {
                held & pixel.last().copied().unwrap_or(0)
            }) == 255;
        if !opaque {
            each_opaque_pixel::<N>(block, cover, source, format);
        } else if all == 255 {
            if let Some(from) = solid.get(..block.len()) {
                block.copy_from_slice(from);
            }
        } else {
            opaque_block::<N>(block, cover, solid);
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
/// a pixel that is not opaque through `blend`, and an opaque one taking the
/// colour at full coverage and [`over_opaque`] at any other.
fn opaque_pixel(pixel: &mut [u8], source: &[u8; 5], c: u8, format: PixelFormat) {
    let channels = color_channels(format);
    if format.has_alpha() && pixel.get(channels) != Some(&255) {
        blend(
            Some(pixel),
            source,
            u32::from(c),
            format,
            BlendMode::Normal,
            0,
        );
    } else if c == 255 {
        if let Some(colour) = source.get(..pixel.len()) {
            pixel.copy_from_slice(colour);
        }
    } else {
        over_opaque(pixel, source, u32::from(c), channels);
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
    /// colour, another mode, a canvas that is not opaque, the ink canvas, and
    /// a non-isolated group's backdrop.
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
    /// `blend` called once for each covered pixel that is not opaque and never
    /// otherwise — the count the walk a pixel at a time made.
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
                            .filter(|(pixel, c)| **c != 0 && pixel[channels] != 255)
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
        group.adopt_backdrop(page.extract((0, 0), 4, 4, PixelFormat::CmykA8));
        assert_eq!(
            group.data[..5],
            [255, 255, 255, 255, 0],
            "the rich black, with none of the group's own alpha"
        );

        // Half of magenta, then the backdrop out: what is left is the group's
        // own colour, magenta, whatever it was painted over.
        let magenta = Color::rgb(255, 0, 255);
        group.fill_mask_inked(&mask, magenta, Some([0, 255, 0, 0]), 0.5, BlendMode::Normal);
        group.remove_backdrop();
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
}
