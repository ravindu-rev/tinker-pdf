//! Image sampling.
//!
//! An image arriving here is bytes, two dimensions and a transform. There is
//! no colour space, no filter chain and no dictionary — ruling 8, and the
//! reason the sampling policy can be tested against numbers instead of against
//! documents.
//!
//! Sampling maps *backwards*: every destination pixel is walked through the
//! inverse transform into the image, rather than every source sample being
//! pushed forward. A forward map leaves seams where it lands short and
//! double-writes where it overlaps, and both depend on the transform, so
//! neither is fixable by rounding differently.
//!
//! The sampling policy is recorded in `docs/features/rasterizer.md`.

use crate::blend::BlendMode;
use crate::canvas::{mul255, Canvas, Color, Opacity, PlateInk};
use crate::fill::{fill_hard_with, fill_with, FillScratch, HardEdge, Mask};
use crate::fragments::Fragments;
use crate::geom::{FillRule, Path};

/// A decoded image: its samples, and how many of them there are.
///
/// Three bytes of colour per sample and an optional byte of coverage — or, for
/// an image of two colours, one byte a sample choosing between them
/// ([`TwoColour`]). What produced them — a codec, a colour conversion, a mask —
/// is the caller's business and is deliberately not expressible here.
#[derive(Clone, Copy, Debug)]
pub struct ImageSource<'a> {
    /// Samples across.
    pub width: u32,
    /// Samples down.
    pub height: u32,
    /// Three bytes per sample, row-major from the **top** row. Not read where
    /// [`Self::two_colour`] holds the samples.
    pub rgb: &'a [u8],
    /// One byte of coverage per sample; empty means every sample is opaque.
    /// Not read where [`Self::two_colour`] holds the samples.
    pub alpha: &'a [u8],
    /// The one-channel form: an image whose every sample is one of two
    /// colours, held at one byte a sample in place of `rgb` and `alpha`.
    /// `None` for an image of three bytes a sample.
    pub two_colour: Option<TwoColour<'a>>,
    /// The rectangle of the image the buffers hold, where they hold less
    /// than all of it; `None` where they hold every sample.
    ///
    /// The image is still [`Self::width`] by [`Self::height`]: the sampling
    /// policy, the plans and every tap are worked out over the whole of it,
    /// in its own sample coordinates, exactly as for the image held whole, and
    /// only the read of a sample subtracts the window's corner — an integer
    /// subtraction, which cannot round (ruling 5's reasoning, applied to the
    /// samples). A sample outside the window reads the nearest one inside
    /// it: a stand-in only a pixel the caller cannot show reads, since the
    /// caller widens the window by the sampler's reach around every pixel it
    /// can (`docs/design/jpx-speed.md`, "The margin a region needs for
    /// ruling 5"), and one that keeps every pixel of the image's square
    /// covered, so a run of images decides what it overlaps as it would over
    /// the whole image. Rows of the buffers are the window's, `width` samples
    /// each.
    pub window: Option<SourceWindow>,
}

/// A rectangle of an image's samples: `width` by `height` of them from column
/// `x` and row `y`, counted from its top-left corner ([`ImageSource::window`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SourceWindow {
    /// The first column held.
    pub x: u32,
    /// The first row held.
    pub y: u32,
    /// Columns held.
    pub width: u32,
    /// Rows held.
    pub height: u32,
}

/// An image of two colours, held as one byte a sample: a bilevel scan, a
/// one-bit image of one component, a stencil.
///
/// A sample of zero takes the first of [`Self::colours`] and any other value
/// the second. Both filters are linear in their weights, so a pixel is sampled
/// by summing one channel — the weight on samples of the second colour —
/// beside the weight, and each of its four channels comes from the two sums
/// exactly as the average of the image expanded to RGB rounds it. A stencil is
/// the pair with its paint opaque and the rest transparent, its colour the
/// draw's [`ImageDraw::tint`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TwoColour<'a> {
    /// One byte per sample, row-major from the **top** row.
    pub samples: &'a [u8],
    /// Red, green, blue and alpha of a sample of zero, then of any other.
    pub colours: [[u8; 4]; 2],
}

impl ImageSource<'_> {
    /// Where sample `(x, y)` sits in a buffer of one entry a sample, or `None`
    /// off the image.
    ///
    /// Every index is checked rather than saturated. `usize` is 32 bits on
    /// wasm32 and 64 on the other three targets, and a sample count that fits
    /// one and not the other is exactly the divergence ruling 4 exists to
    /// catch: a wrapping multiply would read a different sample on one target
    /// and the same document would render differently there.
    ///
    /// A windowed source answers for every sample of the image: one inside
    /// the window from its own buffers, one outside from the nearest inside
    /// ([`Self::window`]); an empty window holds nothing.
    fn index(&self, x: u32, y: u32) -> Option<usize> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let Some(window) = self.window else {
            return (y as usize)
                .checked_mul(self.width as usize)?
                .checked_add(x as usize);
        };
        if window.width == 0 || window.height == 0 {
            return None;
        }
        let clamp = |at: u32, start: u32, size: u32| {
            let last = u64::from(start) + u64::from(size) - 1;
            // In `u64`, since a corner plus a size may pass `u32`; the result
            // is at most `at`, which fits.
            u64::from(at).clamp(u64::from(start), last.max(u64::from(start))) - u64::from(start)
        };
        let column = usize::try_from(clamp(x, window.x, window.width)).ok()?;
        let row = usize::try_from(clamp(y, window.y, window.height)).ok()?;
        row.checked_mul(window.width as usize)?.checked_add(column)
    }

    /// The sample at `(x, y)`, or `None` where the buffer stops short: for a
    /// two-colour image, the colour its byte chooses.
    fn texel(&self, x: u32, y: u32) -> Option<(u8, u8, u8, u8)> {
        let index = self.index(x, y)?;
        if let Some(two) = &self.two_colour {
            let [r, g, b, a] = two.colours[usize::from(*two.samples.get(index)? != 0)];
            return Some((r, g, b, a));
        }
        let start = index.checked_mul(3)?;
        let rgb = self.rgb.get(start..start.checked_add(3)?)?;
        let alpha = self.alpha.get(index).copied().unwrap_or(255);
        Some((rgb[0], rgb[1], rgb[2], alpha))
    }

    /// Whether the sample at `(x, y)` of a two-colour image takes the second
    /// colour, or `None` where the buffer stops short: one byte read.
    fn chosen(&self, two: &TwoColour<'_>, x: u32, y: u32) -> Option<bool> {
        Some(*two.samples.get(self.index(x, y)?)? != 0)
    }
}

/// A 2x3 affine map, in the `[a b c d e f]` order every graphics API writes
/// one.
///
/// The rasterizer's fills and strokes take paths that are already in device
/// space, so this is the only transform the crate holds. An image cannot be
/// pre-transformed the same way — the mapping is what decides which sample a
/// destination pixel reads — so it arrives as a matrix.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    /// x scale.
    pub a: f64,
    /// y shear.
    pub b: f64,
    /// x shear.
    pub c: f64,
    /// y scale.
    pub d: f64,
    /// x translation.
    pub e: f64,
    /// y translation.
    pub f: f64,
}

impl Transform {
    /// The identity.
    pub const IDENTITY: Transform = Transform {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// Maps a point.
    #[must_use]
    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    /// The inverse, or `None` when the map collapses.
    ///
    /// A determinant this small maps the image to a sliver of nothing, and
    /// inverting it would produce coordinates whose magnitude has no useful
    /// bits left.
    #[must_use]
    pub fn invert(&self) -> Option<Transform> {
        let determinant = self.a * self.d - self.b * self.c;
        if !determinant.is_finite() || determinant.abs() < 1e-12 {
            return None;
        }
        let inverse = 1.0 / determinant;
        Some(Transform {
            a: self.d * inverse,
            b: -self.b * inverse,
            c: -self.c * inverse,
            d: self.a * inverse,
            e: (self.c * self.f - self.d * self.e) * inverse,
            f: (self.b * self.e - self.a * self.f) * inverse,
        })
    }
}

/// How a destination pixel takes its colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    /// One truncating tap: the sample the pixel's centre lands in.
    Nearest,
    /// Four taps around the pixel's centre, weighted by how near each is.
    Bilinear,
    /// Every sample the destination pixel covers, weighted by how much of it
    /// the pixel covers.
    ///
    /// The only filter that is *correct* on the way down, and the difference
    /// is not subtle. Bilinear takes four taps whatever the ratio, so a
    /// downscale of 1.5:1 weights four samples by distance when it should be
    /// averaging 2.25 of them by area — it keeps whichever samples the grid
    /// landed near and throws the rest away. Measured against an exact box
    /// average of a 512-square source, on noise, mean absolute error per
    /// channel out of 255:
    ///
    /// | ratio | bilinear | area |
    /// | ---: | ---: | ---: |
    /// | 1.5 | 43.26 | 0.06 |
    /// | 2.0 | 0.23 | 0.06 |
    /// | 3.0 | 26.33 | 0.10 |
    /// | 4.0 | 0.50 | 0.07 |
    /// | 6.0 | 9.58 | 0.13 |
    ///
    /// Powers of two were already right, because the pyramid reduces to
    /// exactly 1:1 and the interpolation has nothing left to do. Everything
    /// between them was wrong by a tenth of full scale, which is most images
    /// on most pages: a page scale is rarely a power of two.
    Area,
}

/// What the policy decided for one draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sampling {
    /// How each destination pixel takes its colour.
    pub filter: Filter,
    /// How many times each axis is halved before it does.
    pub halvings: (u32, u32),
    /// For [`Filter::Area`], what one destination pixel covers of the reduced
    /// image, per axis, in 16.16 texels. Meaningless for the other two.
    pub footprint: (u64, u64),
}

/// One in 16.16 fixed point.
const ONE: u64 = 1 << 16;

/// The most times an axis may be halved.
///
/// Sixteen takes any image a decoder will produce down to a single sample, so
/// the cap is a guard against a nonsensical transform rather than a policy.
const MAX_HALVINGS: u32 = 16;

/// The downscale beyond which a box-filter pyramid takes over from exact area
/// integration, in whole texels per device pixel.
///
/// # Why there is a threshold at all, and why it is here
///
/// Area integration over the destination pixel's true source rectangle is
/// **scale-invariant**: halve the device scale and each pixel covers exactly
/// the union of the four it replaced, so a render at twice the resolution,
/// box-filtered down, is the same picture. A pyramid is not. It averages
/// source-aligned blocks of two, so the support it integrates is quantised to
/// powers of two, and *which* blocks a destination pixel lands on moves with
/// the device scale. That is the whole of the `dpi` relation's residual on
/// strip-built scans: with the pyramid `pclm-in.pdf` disagreed on 15.9 % of
/// its pixels, and with exact integration it agrees.
///
/// So why keep a pyramid. Exact integration costs one visit per source pixel
/// per draw, which is the same as building the pyramid and is why removing it
/// costs 3.5 % over the whole corpus and no file's time gate. What it does not
/// bound is the *same* image drawn many times at extreme minification — a
/// tiling pattern — where the pyramid is built once and every repeat then
/// costs the destination rather than the source.
///
/// **128 is measured, not chosen.** Over the pdfjs corpus the worst downscale
/// any draw asks for is 72:1, with the 99.9th percentile at 26 and the median
/// at 2. A threshold of 128 therefore leaves every real file on the exact
/// path with most of an octave to spare, and leaves the bound in place for the
/// synthetic case that would abuse it.
const PYRAMID_ABOVE: u64 = 128;

/// What the pyramid reduces to once it does engage.
///
/// Unchanged from when the pyramid was the only path: bringing the residual
/// ratio within four keeps the per-pixel cost at sixteen taps for the extreme
/// downscales that reach here at all.
const PYRAMID_TARGET: u64 = 4;

/// Source samples per device pixel along each axis, in 16.16 fixed point.
///
/// The transform's columns are the device vectors of the image's own axes, so
/// their lengths are how far the whole image reaches; divided by the samples
/// along that axis they are the size of one sample, and the reciprocal is how
/// many samples a device pixel spans. One or less is magnification.
///
/// **Fixed point, because a policy that branches on a float branches
/// differently.** `sqrt` and division are correctly rounded, so the value
/// arriving here is the same on every target — but it is turned into an
/// integer before anything compares it, so the comparison cannot be the thing
/// that diverges. Gap 13 rejected the plan's recursive flattener for the
/// mirror image of this: a float termination test that changes the *number* of
/// segments rather than their positions.
fn sample_ratios(image: &ImageSource<'_>, t: &Transform) -> (u64, u64) {
    let across = (t.a * t.a + t.b * t.b).sqrt();
    let down = (t.c * t.c + t.d * t.d).sqrt();
    (
        fixed_ratio(f64::from(image.width), across),
        fixed_ratio(f64::from(image.height), down),
    )
}

/// `samples / device_length` in 16.16, saturating.
///
/// A zero-length axis means the image is squashed to a line, which is an
/// unbounded downscale; `u64::MAX` is that, and the saturating `as` cast says
/// the same thing about an overflow.
fn fixed_ratio(samples: f64, device: f64) -> u64 {
    // `<= 0.0` rather than a negated `>`: NaN answers false to both, and the
    // check below catches it.
    if device <= 0.0 || !device.is_finite() || !samples.is_finite() {
        return u64::MAX;
    }
    let ratio = samples / device;
    if !ratio.is_finite() {
        return u64::MAX;
    }
    // Multiplying by a power of two is exact until it overflows, and the cast
    // saturates rather than wrapping.
    (ratio * 65536.0) as u64
}

/// Plan 07's sampling policy.
///
/// | Condition | Filter |
/// | --- | --- |
/// | Scale >= 1 per axis, `interpolate` true | Bilinear |
/// | Scale >= 1 per axis, `interpolate` false | Nearest |
/// | Downscale up to 2:1 | Area |
/// | Downscale beyond 2:1 | Box-filter pyramid to within 2:1, then area |
///
/// The debatable row is nearest at or above 1:1 without the flag, and it is
/// deliberate. `/Interpolate` is defined as opt-*in* smoothing for magnified
/// images: false means the author wanted hard pixels — a barcode, a
/// screenshot, an upscaled one-bit mask — and blurring them is a change the
/// file asked not to have. It also keeps a 1:1 blit byte-preserving, which is
/// a property the tile contract leans on.
///
/// **The flag has no say on the way down.** It is defined for magnification,
/// and a shrunk image that takes one tap per destination pixel does not keep
/// hard edges — it keeps whichever of the source pixels the sampling grid
/// happened to land on and throws the rest away. That is not the sharpness the
/// author asked for, it is aliasing, and it is a difference against every
/// other renderer rather than a matter of taste.
#[must_use]
pub fn sampling_for(image: &ImageSource<'_>, t: &Transform, interpolate: bool) -> Sampling {
    let (across, down) = sample_ratios(image, t);
    // At or above 1:1 on both axes.
    if across <= ONE && down <= ONE {
        return Sampling {
            filter: if interpolate {
                Filter::Bilinear
            } else {
                Filter::Area
            },
            halvings: (0, 0),
            footprint: (across.max(1), down.max(1)),
        };
    }
    let halvings = (halvings(across, image.width), halvings(down, image.height));
    Sampling {
        filter: Filter::Area,
        halvings,
        // What one destination pixel covers of the *reduced* image, per axis,
        // in 16.16 texels. Below [`PYRAMID_ABOVE`] there is no reduction and
        // this is the true ratio, which is what makes the filter agree with
        // itself at another scale; above it the pyramid brings the residual
        // within [`PYRAMID_TARGET`], so the cost per pixel stays bounded
        // however extreme the downscale.
        //
        // A shift rather than a division, for `halvings`' own reason: the
        // ratio is already 16.16 and shifting is exact everywhere. It
        // truncates by less than a sixty-five-thousandth of a texel, which
        // cannot move a weight by a whole 1/256th.
        //
        // Floored at one texel. An axis that is *magnifying* while the other
        // shrinks has a footprint smaller than a sample, and averaging a
        // fraction of one sample is that sample — which is the nearest-neighbour
        // answer the policy gives a magnified axis anyway.
        footprint: (
            (across >> halvings.0).max(ONE),
            (down >> halvings.1).max(ONE),
        ),
    }
}

/// How many times an axis must be halved, and none at all below
/// [`PYRAMID_ABOVE`].
///
/// # Why this is a shift rather than a logarithm
///
/// The obvious spelling is `ceil(log2(ratio / 2))`, and `log2` is a
/// transcendental: `cargo xtask libm` refuses it on a pixel path precisely
/// because no two platforms round it the same way, and a *count* that differs
/// by one is not a rounding difference in the output — it is a different image,
/// half the resolution of the other. Gap 13 found the same class of defect in
/// the flattener, where a float termination test changed the number of
/// segments rather than their positions.
///
/// So the ratio arrives as a 16.16 integer and the loop shifts it. Integer
/// shifts are exact and identical everywhere, the comparison is between two
/// integers, and the only float that ever touched the decision was
/// correctly-rounded `sqrt` and division upstream of the fixed-point cast. The
/// loop also stops when the axis reaches a single sample, because halving that
/// again would build fifteen more copies of one pixel.
fn halvings(ratio: u64, extent: u32) -> u32 {
    // Below the threshold the destination pixel's true source rectangle is
    // integrated directly, which is what makes the result independent of the
    // device scale. See [`PYRAMID_ABOVE`].
    if ratio <= PYRAMID_ABOVE * ONE {
        return 0;
    }
    let mut count = 0;
    // The threshold is doubled rather than the ratio halved: shifting the
    // ratio down would truncate, and a ratio a hair over 4 would then look
    // like exactly 2 after one halving and stop one short. Doubling is exact,
    // and the largest threshold reached is 2^33.
    while count < MAX_HALVINGS && (extent >> count) > 1 && ratio > (PYRAMID_TARGET * ONE) << count {
        count += 1;
    }
    count
}

/// One reduced copy of an image.
#[derive(Clone, Debug)]
struct Level {
    /// How many halvings of each axis produced it.
    halvings: (u32, u32),
    width: u32,
    height: u32,
    rgb: Vec<u8>,
    /// Always present, even where the original had none: a level is built by
    /// averaging, and a sample the original did not supply has to average as
    /// absent rather than as black.
    alpha: Vec<u8>,
    /// The rectangle of the level its samples hold, a windowed image's
    /// window halved ([`halve`]); `None` for a level of a whole image.
    window: Option<SourceWindow>,
}

impl Level {
    fn source(&self) -> ImageSource<'_> {
        ImageSource {
            width: self.width,
            height: self.height,
            rgb: &self.rgb,
            alpha: &self.alpha,
            two_colour: None,
            window: self.window,
        }
    }
}

/// Halved copies of an image, held by whoever asked for the draw.
///
/// Four taps cover a 2 x 2 footprint. Beyond 2:1 the footprint of a
/// destination pixel is larger than that, so bilinear alone steps *over*
/// source samples and the ones it skips come back as moiré — a fine
/// checkerboard shrunk sixteen times becomes a coarse one rather than grey.
/// Exact 2 x 2 area averaging, repeated, is what covers the footprint; the
/// residual is never worse than 2:1 and bilinear finishes it.
///
/// The levels are the **caller's**. `draw_image` fills in the ones it needs
/// and leaves them here; nothing is retained between calls, so how long a
/// pyramid lives — and whether a second draw of the same image reuses one — is
/// a decision phase 08 makes, where an image's lifetime is known. A pyramid
/// handed an image of different dimensions, or of another window, rebuilds
/// rather than reusing.
#[derive(Clone, Debug, Default)]
pub struct Pyramid {
    source: (u32, u32),
    window: Option<SourceWindow>,
    levels: Vec<Level>,
}

impl Pyramid {
    /// An empty pyramid.
    #[must_use]
    pub fn new() -> Pyramid {
        Pyramid::default()
    }

    /// How many reduced copies are held.
    #[must_use]
    pub fn levels(&self) -> usize {
        self.levels.len()
    }

    /// The dimensions of one held level, the smallest reduction first.
    #[must_use]
    pub fn level_size(&self, index: usize) -> Option<(u32, u32)> {
        self.levels.get(index).map(|l| (l.width, l.height))
    }

    /// Builds whatever is missing to reach `halvings`, and answers with the
    /// image to sample.
    fn reduce<'a>(&'a mut self, source: &ImageSource<'a>, halvings: (u32, u32)) -> ImageSource<'a> {
        if self.source != (source.width, source.height) || self.window != source.window {
            self.levels.clear();
            self.source = (source.width, source.height);
            self.window = source.window;
        }

        // The canonical path: every halving of x first, then every halving of
        // y. Averaging is per-axis and rounds, so x-then-y and y-then-x can
        // differ in the last bit; fixing the order makes the answer a function
        // of the counts alone, which is what lets a cached level be reused.
        let wanted: Vec<(u32, u32)> = (1..=halvings.0)
            .map(|x| (x, 0))
            .chain((1..=halvings.1).map(|y| (halvings.0, y)))
            .collect();

        let keep = self
            .levels
            .iter()
            .zip(wanted.iter())
            .take_while(|(held, want)| held.halvings == **want)
            .count();
        self.levels.truncate(keep);

        for (index, step) in wanted.iter().enumerate().skip(keep) {
            let level = {
                let previous = if index == 0 {
                    *source
                } else {
                    self.levels[index - 1].source()
                };
                halve(&previous, *step)
            };
            self.levels.push(level);
        }

        self.levels.last().map_or(*source, Level::source)
    }
}

/// Halves one axis by exact 2 x 2 — or here 2 x 1 — area averaging.
///
/// Integer, round half up. An odd extent leaves a last sample with no partner,
/// which averages with itself and so survives unchanged rather than being
/// dropped or paired across the edge.
///
/// A two-colour source is read through the colour each sample chooses, so its
/// first level is the level the image expanded to RGB halves to, and every
/// level is RGB. That is what a draw past [`PYRAMID_ABOVE`] needs: rounding
/// each channel's mean half up is not linear in coverage — a halved pair of
/// two colours holds each channel's rounded midpoint, not a third weight — so
/// a two-colour image that reduces is sampled as its RGB expansion would be.
///
/// A windowed source halves its window alone, into the level's window: the
/// pairs from the one its first sample is in to the one its last is in, each
/// the whole image's pair — samples `2x` and `2x + 1` — so a level of a window
/// whose edges lie on a multiple of two is the same samples as the whole
/// image's level over it, and one whose edge cuts a pair takes the pair's far
/// sample from the nearest held, as every read of a window does.
fn halve(source: &ImageSource<'_>, halvings: (u32, u32)) -> Level {
    let horizontal = halvings.1 == 0;
    let width = if horizontal {
        source.width.div_ceil(2).max(1)
    } else {
        source.width
    };
    let height = if horizontal {
        source.height
    } else {
        source.height.div_ceil(2).max(1)
    };
    let window = source.window.map(|held| {
        if held.width == 0 || held.height == 0 {
            return SourceWindow::default();
        }
        // The pairs the held samples reach, in `u64` since a corner plus a
        // size may pass `u32`, and never past the level's own edge.
        let halved = |start: u32, size: u32, extent: u32| {
            let first = start / 2;
            let end = (u64::from(start) + u64::from(size))
                .div_ceil(2)
                .min(u64::from(extent));
            (first, (end as u32).saturating_sub(first))
        };
        if horizontal {
            let (x, width) = halved(held.x, held.width, width);
            SourceWindow { x, width, ..held }
        } else {
            let (y, height) = halved(held.y, held.height, height);
            SourceWindow { y, height, ..held }
        }
    });
    let (x0, y0, columns, rows) = window.map_or((0, 0, width, height), |held| {
        (held.x, held.y, held.width, held.height)
    });

    let samples = (columns as usize).saturating_mul(rows as usize);
    let mut rgb = Vec::with_capacity(samples.saturating_mul(3));
    let mut alpha = Vec::with_capacity(samples);

    for y in (0..rows).map(|row| y0 + row) {
        for x in (0..columns).map(|column| x0 + column) {
            let (first, second) = if horizontal {
                ((x * 2, y), (x * 2 + 1, y))
            } else {
                ((x, y * 2), (x, y * 2 + 1))
            };
            let pair = (
                source.texel(first.0, first.1),
                source.texel(second.0, second.1),
            );
            // A sample the buffer does not supply averages as its partner, so
            // a truncated image reduces to what it does have rather than to
            // black. Neither present is transparent, which paints nothing.
            let (a, b) = match pair {
                (Some(a), Some(b)) => (a, b),
                (Some(a), None) => (a, a),
                (None, Some(b)) => (b, b),
                (None, None) => ((0, 0, 0, 0), (0, 0, 0, 0)),
            };
            // Round half up, which `div_ceil` on the plain sum is exactly.
            let mean = |p: u8, q: u8| (u16::from(p) + u16::from(q)).div_ceil(2) as u8;
            rgb.extend_from_slice(&[mean(a.0, b.0), mean(a.1, b.1), mean(a.2, b.2)]);
            alpha.push(mean(a.3, b.3));
        }
    }

    Level {
        halvings,
        width,
        height,
        rgb,
        alpha,
        window,
    }
}

/// One image draw: the pixels, where they go, and how they compose.
pub struct ImageDraw<'a> {
    /// The samples.
    pub image: ImageSource<'a>,
    /// Maps the image's unit square — `(0, 0)` at its bottom-left corner and
    /// `(1, 1)` at its top-right — to device space.
    pub unit_to_device: Transform,
    /// The square the draw is cut at, where it is smaller than the one its
    /// samples span: a reduced decode's samples reach past the image they
    /// stand for wherever its last column and row stand for less than a whole
    /// sample, so they are mapped by [`Self::unit_to_device`] onto the square
    /// they span and cut here, at the image's own unit square. The coverage of
    /// the image's edge is this square's; `None` cuts at
    /// [`Self::unit_to_device`]'s own.
    pub cut: Option<Transform>,
    /// Whether the image asked to be smoothed when it is magnified.
    ///
    /// The one bit of the policy the caller supplies; everything else is read
    /// off the transform.
    pub interpolate: bool,
    /// Scales the whole draw; clamped to 0..=1.
    pub alpha: f64,
    /// How the samples compose with what is already there.
    pub blend: BlendMode,
    /// Coverage the draw is confined to, if any.
    pub clip: Option<&'a Mask>,
    /// Paint this colour rather than the image's own, taking only coverage
    /// from the image.
    ///
    /// What a one-bit stencil needs, expressed without naming one: the image
    /// says *where*, the caller says *what*.
    pub tint: Option<Color>,
    /// The tint's own ink — cyan, magenta, yellow and black as bytes, the
    /// components it was chosen in — which a [`crate::PixelFormat::CmykA8`]
    /// canvas composites in place of the tint turned back into ink, as
    /// [`Canvas::fill_mask_inked`] does for a fill. Read only with a `tint`,
    /// and only by an ink canvas.
    pub ink: Option<[u8; 4]>,
    /// The tint's ink by plate — a tint per colour channel of a
    /// [`crate::PixelFormat::Plates`] canvas, spots among them — which a
    /// plates canvas composites in place of [`Self::ink`], every channel it
    /// names no tint for knocked out, as [`Canvas::fill_mask_plates`] does for
    /// a mask; any other canvas paints its process tints as the DeviceCMYK
    /// colour they are. Read only with a `tint`, and only by [`draw_image`]:
    /// a run ([`accumulate_image`]) holds a colour's light and ink, never its
    /// plates, so a draw carrying them is composited on its own.
    pub plates: Option<&'a PlateInk>,
    /// Asked once per destination row; drawing stops as soon as it answers
    /// `true`.
    pub stop: Option<&'a dyn Fn() -> bool>,
    /// The bits of coverage the image's own edge keeps
    /// (`docs/design/anti-aliasing.md`): 8, the default, anti-aliased; 1 to 7
    /// quantised; 0 whole or empty at half a pixel's coverage. The unit
    /// square's coverage goes through [`Mask::quantise`] at this level, the
    /// same function a path's coverage goes through at its own, so an image
    /// edge and a filled edge beside it agree about every pixel. Only the
    /// *edge*: the samples inside are resampled exactly as before, because
    /// what a pixel's colour is and how much of it is covered are different
    /// questions.
    pub edge_bits: u8,
    /// The rule the edge is drawn by at [`ImageDraw::edge_bits`] 0, where
    /// each pixel is whole or empty: [`HardEdge::Half`], the default, reads
    /// the square's coverage; [`HardEdge::Centre`] paints the pixels whose
    /// centres the square holds and [`HardEdge::AnyPart`] every pixel it
    /// meets, the filler's own rules ([`crate::fill_hard_with`]). Read at 0
    /// bits only.
    pub edge_rule: HardEdge,
}

impl<'a> ImageDraw<'a> {
    /// An opaque, unclipped, normally-blended draw.
    #[must_use]
    pub fn new(image: ImageSource<'a>, unit_to_device: Transform) -> ImageDraw<'a> {
        ImageDraw {
            image,
            unit_to_device,
            cut: None,
            interpolate: false,
            alpha: 1.0,
            blend: BlendMode::Normal,
            clip: None,
            tint: None,
            ink: None,
            plates: None,
            stop: None,
            edge_bits: 8,
            edge_rule: HardEdge::Half,
        }
    }
}

/// Draws an image into the unit square of its transform.
///
/// Nothing outside the image's own placement is visited, so a stamp in a
/// corner costs the corner rather than the page.
///
/// `pyramid` is scratch space the caller owns: any reduced copies this draw
/// needs are built into it and left there. Pass a fresh [`Pyramid::new`] to
/// throw them away, or keep one beside the image to reuse them. `scratch` is
/// the memory the image's unit square is filled in, which a caller drawing
/// many things keeps and passes to every fill ([`fill_with`]).
pub fn draw_image(
    canvas: &mut Canvas,
    draw: &ImageDraw<'_>,
    pyramid: &mut Pyramid,
    scratch: &mut FillScratch,
) {
    // The draw's alpha, read once: each pixel's level is it times the pixel's
    // coverage, its sample's alpha and the clip, made a level without a call
    // to `round` (`Opacity`).
    let opacity = Opacity::new(draw.alpha);
    // The canvas's own device pixels, which is all a draw can reach. The walk
    // is in device pixels — the frame every placement is stated in — and only
    // the write below turns one into a canvas index (ruling 5).
    let rect = canvas.device_rect();
    let ink = draw.tint.and(draw.ink);
    let plates = draw.tint.and(draw.plates);
    walk(
        draw,
        pyramid,
        scratch,
        rect,
        |px, py, color, covered, own| {
            let clip = draw.clip.map_or(255, |mask| mask.at(px, py));
            if clip == 0 {
                return;
            }
            let Some((x, y)) = canvas.local(px, py) else {
                return;
            };
            let level = opacity.level_three(covered, own, clip);
            match plates {
                Some(plates) => {
                    canvas.blend_plates_level(x, y, plates, color.a, level, draw.blend);
                }
                None => canvas.blend_pixel_level(x, y, color, ink, level, draw.blend),
            }
        },
    );
}

/// Adds an image draw to a run instead of compositing it.
///
/// The run's own graphics state — alpha, blend mode and clip — is applied when
/// it reaches the canvas, so this deliberately ignores all three: a run is one
/// element for compositing purposes, which is the whole reason abutting strips
/// stop conflating. See [`crate::fragments`].
///
/// `canvas` is the device rectangle the draw is clipped to, as `(x0, y0,
/// width, height)` — the canvas's [`Canvas::device_rect`], which is the same
/// extent [`draw_image`] would have used. `pyramid` and `scratch` are as
/// [`draw_image`]'s.
pub fn accumulate_image(
    fragments: &mut Fragments,
    draw: &ImageDraw<'_>,
    pyramid: &mut Pyramid,
    canvas: (i32, i32, u32, u32),
    scratch: &mut FillScratch,
) {
    walk(
        draw,
        pyramid,
        scratch,
        canvas,
        |px, py, color, covered, own| {
            fragments.add(
                px,
                py,
                color,
                mul255(u32::from(covered), u32::from(own)) as u8,
            );
        },
    );
}

/// The image resampled onto a grid of `width` by `height` pixels that it
/// covers exactly, each pixel handed to `emit` as its column, its row and the
/// area filter's colour and alpha for it — the bitmap toolbox's scale
/// (`docs/design/bitmap-toolbox.md`).
///
/// **The sampler's area filter, its spans planned in integers.** A grid the
/// image covers exactly gives each pixel a rectangle of the image that is a
/// rational number of samples wide: column `x` of `width` covers
/// `[x · W / width, (x + 1) · W / width)` of an image `W` samples across, and
/// row `y` likewise of `H` down, counted from the top row as the image's own
/// rows are. Each column's and each row's taps are measured exactly, in
/// `1 / width` and `1 / height` of a sample — the overlap of
/// `[x · W, (x + 1) · W)` with sample `i`'s `[i · width, (i + 1) · width)` —
/// and summed by [`area_taps`], the filter [`draw_image`] sums its own taps
/// with: every sample under the rectangle weighted by how much of it the
/// rectangle covers, each channel `(Σ w · s + W · H / 2) / (W · H)`, rounded
/// half up. That holds whichever way an axis goes: a pixel inside one sample
/// is that sample, and one across a boundary the samples weighted by their
/// share, as a draw with no `/Interpolate` magnifies.
///
/// A draw placed through a matrix finds the same rectangle from a centre the
/// inverse matrix maps in floating point and quantises to 1/65536 of a sample,
/// which breaks a tie in the average one way or the other wherever the centre
/// lands an ulp off; a grid the image covers exactly has no matrix, so its
/// spans are the rationals themselves. No pyramid is built at any ratio: the
/// pyramid bounds the *same* image drawn many times, and a resample visits
/// each sample once a line it reaches.
///
/// `false`, with nothing emitted, for an empty image or grid, and for an
/// image of so many samples that a channel's sum, at most `255 · W · H`,
/// would not fit 64 bits — past 7 · 10^16 samples, which no buffer holds.
pub fn resample(
    image: &ImageSource<'_>,
    width: u32,
    height: u32,
    mut emit: impl FnMut(u32, u32, (u8, u8, u8, u8)),
) -> bool {
    if image.width == 0 || image.height == 0 || width == 0 || height == 0 {
        return false;
    }
    // The weights a pixel sums to are `W` across and `H` down, so a channel's
    // sum is at most `255 · W · H`, which `area_taps` holds in a `u64`.
    let fits = u64::from(image.width)
        .checked_mul(u64::from(image.height))
        .and_then(|samples| samples.checked_mul(255))
        .is_some();
    if !fits {
        return false;
    }
    let columns = Lines::exact(width, image.width);
    let rows = Lines::exact(height, image.height);
    for y in 0..height {
        let down = rows.line(y as usize);
        for x in 0..width {
            if let Some(sample) = area_taps(image, columns.line(x as usize), down) {
                emit(x, y, sample);
            }
        }
    }
    true
}

/// The device pixels an image draw can reach on a canvas at the origin, as
/// `(x0, y0, width, height)`.
///
/// What a caller needs to size a run before accumulating into one, and the
/// same rectangle the draw itself will visit.
#[must_use]
pub fn image_bounds(t: &Transform, width: u32, height: u32) -> Option<(i32, i32, u32, u32)> {
    image_bounds_in(t, (0, 0, width, height))
}

/// [`image_bounds`] against a canvas's device rectangle, `(x0, y0, width,
/// height)`, wherever it stands.
#[must_use]
pub fn image_bounds_in(t: &Transform, rect: (i32, i32, u32, u32)) -> Option<(i32, i32, u32, u32)> {
    let (x0, x1, y0, y1) = device_bounds(t, rect)?;
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some((x0, y0, x1.abs_diff(x0), y1.abs_diff(y0)))
}

/// The coverage an image draw puts on each device pixel of a region.
///
/// Exposed because a caller accumulating a run has to know whether a draw
/// lands on coverage it is already holding before it adds any, and that
/// question is about the geometry rather than the samples. The region is the
/// caller's so the question can be asked of the band where two draws meet
/// rather than of either draw whole. The square is filled in `scratch`
/// ([`fill_with`]).
#[must_use]
pub fn image_coverage(
    t: &Transform,
    x0: i32,
    y0: i32,
    width: u32,
    height: u32,
    stop: Option<&dyn Fn() -> bool>,
    scratch: &mut FillScratch,
) -> Mask {
    unit_quad(scratch, t, (x0, y0, width, height), stop, None)
}

/// The square the draw is cut at — [`ImageDraw::cut`], or the one its samples
/// span — over `region`, `(x0, y0, width, height)`, as the draw's edge has
/// it: the coverage quantised at [`ImageDraw::edge_bits`], and at 0 bits
/// whole or empty by [`ImageDraw::edge_rule`] — the coverage hardened under
/// [`HardEdge::Half`], the filler's own rule under any other.
fn edge_shape(
    draw: &ImageDraw<'_>,
    scratch: &mut FillScratch,
    region: (i32, i32, u32, u32),
) -> Mask {
    let cut = draw.cut.as_ref().unwrap_or(&draw.unit_to_device);
    if draw.edge_bits == 0 {
        return unit_quad(scratch, cut, region, draw.stop, Some(draw.edge_rule));
    }
    let mut shape = unit_quad(scratch, cut, region, draw.stop, None);
    shape.quantise(draw.edge_bits);
    shape
}

/// Walks the destination pixels of one image draw, handing each to `emit` as
/// its colour, the share of the pixel the image's own rectangle covers, and
/// the sample's own alpha.
///
/// The two consumers differ only in what they do with those: [`draw_image`]
/// composites immediately, [`accumulate_image`] adds to a run. Splitting them
/// any earlier than this would give the engine two samplers, which is exactly
/// how the two would come to disagree about a boundary.
fn walk(
    draw: &ImageDraw<'_>,
    pyramid: &mut Pyramid,
    scratch: &mut FillScratch,
    rect: (i32, i32, u32, u32),
    mut emit: impl FnMut(i32, i32, Color, u8, u8),
) {
    let image = &draw.image;
    if image.width == 0 || image.height == 0 {
        return;
    }
    let Some(inverse) = draw.unit_to_device.invert() else {
        return; // A degenerate transform maps the image to nothing.
    };
    // The square whose pixels the draw visits and whose coverage is its edge:
    // the image's own, which a reduced decode's samples reach past.
    let cut = draw.cut.as_ref().unwrap_or(&draw.unit_to_device);

    let Some((x0, x1, y0, y1)) = device_bounds(cut, rect) else {
        return;
    };

    // `device_bounds` clamps each edge to the canvas independently, so an
    // image entirely off one side comes back with its far edge *below* its
    // near one — 5000..100 for a stamp a page-width to the right. The draw
    // loop's ranges are empty either way and always were; the coverage mask
    // below would take its size from the difference, and an underflow there is
    // a four-billion-pixel allocation rather than a wrong pixel (ruling 1).
    if x1 <= x0 || y1 <= y0 {
        return;
    }

    // 8.9.5.2: the image occupies the unit square of its transform, and the
    // edge of that square is where a destination pixel is only *partly*
    // covered. Rasterised through the crate's own path filler rather than
    // measured a second time here, so an image edge anti-aliases exactly as a
    // filled path does — same sub-scanline grid, same fixed point, same
    // cancellation. `docs/design/image-edges.md` records what this trades.
    let shape = edge_shape(draw, scratch, (x0, y0, x1.abs_diff(x0), y1.abs_diff(y0)));

    let sampling = sampling_for(image, &draw.unit_to_device, draw.interpolate);
    // Borrowed for the rest of the draw: the level, or the image itself when
    // the policy asked for no reduction. A two-colour image is sampled in its
    // one channel only here, where nothing was reduced; its levels are RGB
    // (`halve`).
    let image = &pyramid.reduce(image, sampling.halvings);
    let plan = Plan::new(image, &inverse, sampling, (x0, x1), (y0, y1));
    // The per-pixel area filter's two lines of taps, reused by every pixel of
    // a draw that is not planned rather than allocated by each.
    let mut scratch = (Vec::new(), Vec::new());

    for py in y0..y1 {
        if draw.stop.is_some_and(|stop| stop()) {
            return;
        }
        // Inside `y0..y1` and `x0..x1`, so neither difference can wrap.
        let row = py.abs_diff(y0) as usize;
        for px in x0..x1 {
            let covered = shape.at(px, py);
            if covered == 0 {
                continue;
            }
            let column = px.abs_diff(x0) as usize;

            let sample = match &plan {
                Plan::Area { columns, rows } => {
                    area_taps(image, columns.line(column), rows.line(row))
                }
                Plan::Bilinear { columns, rows } => match (columns.get(column), rows.get(row)) {
                    (Some(across), Some(down)) => bilinear_taps(image, *across, *down),
                    _ => None,
                },
                Plan::PerPixel => {
                    let (u, v) = centre(&inverse, px, py);
                    match sampling.filter {
                        Filter::Nearest => nearest(image, u, v),
                        Filter::Bilinear => bilinear(image, u, v),
                        Filter::Area => area(image, u, v, sampling.footprint, &mut scratch),
                    }
                }
            };
            let Some((r, g, b, own)) = sample else {
                continue;
            };
            if own == 0 {
                continue;
            }
            emit(
                px,
                py,
                draw.tint.unwrap_or(Color::rgb(r, g, b)),
                covered,
                own,
            );
        }
    }
}

/// Where a pixel samples: its centre mapped into the unit square and clamped
/// there, as `(u, v)`.
///
/// The quad decided how *much* of the pixel the image reaches; a centre just
/// outside it still needs a colour, and an image has none outside itself, so
/// the nearest edge is the answer — which is what `bilinear` already does
/// between samples. A centre inside is untouched.
fn centre(inverse: &Transform, px: i32, py: i32) -> (f64, f64) {
    let (u, v) = inverse.apply(f64::from(px) + 0.5, f64::from(py) + 0.5);
    (u.clamp(0.0, 1.0), v.clamp(0.0, 1.0))
}

#[cfg(test)]
thread_local! {
    /// Columns and rows planned on this thread, one for each line of every
    /// planned draw: what `a_draw_plans_its_weights_once_per_column_and_row`
    /// counts.
    static PLANNED_LINES: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
}

/// Counts one planned column or row, under `cfg(test)` only.
#[inline]
fn planned_line() {
    #[cfg(test)]
    PLANNED_LINES.with(|lines| lines.set(lines.get() + 1));
}

#[cfg(test)]
thread_local! {
    /// Taps the two filters have summed on this thread, and the channel sums
    /// those taps went into: what `a_coverage_image_sums_one_channel` counts.
    static SUMMED: core::cell::Cell<(usize, usize)> = const { core::cell::Cell::new((0, 0)) };
}

/// Counts `taps` taps each summed into `channels` channels, under
/// `cfg(test)` only.
#[inline]
fn summed(taps: usize, channels: usize) {
    #[cfg(test)]
    SUMMED.with(|summed| {
        let (held, sums) = summed.get();
        summed.set((held + taps, sums + taps * channels));
    });
    #[cfg(not(test))]
    let _ = (taps, channels);
}

/// What a draw's pixels read, planned before the first of them is visited.
///
/// # Which draws are planned
///
/// [`Transform::invert`] maps a pixel centre `(x, y)` back to the unit square
/// as `u = a·x + c·y + e` and `v = b·x + d·y + f`. Where the inverse's `b` and
/// `c` are both zero — no shear and no rotation, though either axis may be
/// flipped — `c·y` and `b·x` are signed zeros at every finite centre, so `u`
/// is the same number on every row and `v` the same number in every column,
/// up to the sign of a zero, which nothing downstream reads: the area filter
/// truncates `u·w·65536` and bilinear subtracts a half. So each column's taps
/// and each row's are worked out once, from the page-frame centres the
/// per-pixel map would use (ruling 5), and a pixel only sums them. Every other
/// draw maps each pixel on its own, and so does [`Filter::Nearest`], which
/// [`sampling_for`] never chooses. `docs/design/image-edges.md`, P-08, has the
/// argument in full.
enum Plan {
    /// Each pixel is mapped through the inverse on its own.
    PerPixel,
    /// The area filter's taps, for every column and every row.
    Area {
        /// One line of taps a column, left to right.
        columns: Lines,
        /// One line of taps a row, top to bottom.
        rows: Lines,
    },
    /// Bilinear's two taps and weight, for every column and every row.
    Bilinear {
        /// One a column, left to right.
        columns: Vec<Pair>,
        /// One a row, top to bottom.
        rows: Vec<Pair>,
    },
}

impl Plan {
    /// The plan for a draw over columns `x0..x1` and rows `y0..y1` of the
    /// page frame, sampling `image`: the level, where the pyramid reduced.
    fn new(
        image: &ImageSource<'_>,
        inverse: &Transform,
        sampling: Sampling,
        (x0, x1): (i32, i32),
        (y0, y1): (i32, i32),
    ) -> Plan {
        if inverse.b != 0.0 || inverse.c != 0.0 {
            return Plan::PerPixel;
        }
        // Any row's centre gives a column its `u`, and any column's a row its
        // `v`; the first of each is as good as every other.
        let across = |px: i32| centre(inverse, px, y0).0;
        let down = |py: i32| centre(inverse, x0, py).1;
        let (width, height) = (image.width, image.height);
        match sampling.filter {
            Filter::Area => {
                let (hx, hy) = half_footprint(sampling.footprint);
                Plan::Area {
                    columns: Lines::planned(x0..x1, width, |px| {
                        let cx = area_centre(across(px), width);
                        (cx.saturating_sub(hx), cx.saturating_add(hx))
                    }),
                    rows: Lines::planned(y0..y1, height, |py| {
                        let cy = area_centre(1.0 - down(py), height);
                        (cy.saturating_sub(hy), cy.saturating_add(hy))
                    }),
                }
            }
            Filter::Bilinear => Plan::Bilinear {
                columns: (x0..x1)
                    .map(|px| {
                        planned_line();
                        pair(across(px) * f64::from(width) - 0.5, width)
                    })
                    .collect(),
                rows: (y0..y1)
                    .map(|py| {
                        planned_line();
                        pair((1.0 - down(py)) * f64::from(height) - 0.5, height)
                    })
                    .collect(),
            },
            Filter::Nearest => Plan::PerPixel,
        }
    }
}

/// One of the area filter's taps along an axis: a texel, and how much of it a
/// pixel's span covers in 1/65536ths, never zero.
type Tap = (u32, u32);

/// Every line's area taps along one axis, end to end, and where each line's
/// begin.
struct Lines {
    /// Every line's taps, one line after another.
    taps: Vec<Tap>,
    /// Where line `i`'s taps begin; line `i` ends where line `i + 1` begins,
    /// and the last entry is the end of them all.
    starts: Vec<usize>,
}

impl Lines {
    /// One line for each of `lines`, whose 16.16 span `ends` gives.
    fn planned(
        lines: core::ops::Range<i32>,
        extent: u32,
        ends: impl Fn(i32) -> (i64, i64),
    ) -> Lines {
        let mut taps = Vec::new();
        let mut starts = Vec::with_capacity(lines.len() + 1);
        for line in lines {
            starts.push(taps.len());
            planned_line();
            let (lo, hi) = ends(line);
            area_line(lo, hi, extent, &mut taps);
        }
        starts.push(taps.len());
        Lines { taps, starts }
    }

    /// One line for each of `lines` pixels over an axis of `extent` samples
    /// the pixels cover exactly ([`resample`]): line `n` covers
    /// `[n · extent, (n + 1) · extent)` and sample `i` covers
    /// `[i · lines, (i + 1) · lines)`, both in `1 / lines` of a sample, and
    /// each tap is the overlap of the two — an integer, at most a whole
    /// sample, `lines`. The ends are below `lines · extent`, under 2^64, and
    /// an overlap under 2^32.
    fn exact(lines: u32, extent: u32) -> Lines {
        let (count, extent) = (u64::from(lines), u64::from(extent));
        let mut taps = Vec::new();
        let mut starts = Vec::new();
        for line in 0..count {
            starts.push(taps.len());
            planned_line();
            let lo = line * extent;
            let hi = lo + extent;
            // `extent` is at least one, so `hi - 1` cannot wrap.
            for index in lo / count..=(hi - 1) / count {
                let start = index * count;
                let covered = hi.min(start + count) - lo.max(start);
                if covered != 0 {
                    // `index` is below `extent` and `covered` at most `lines`,
                    // both `u32`s.
                    taps.push((index as u32, covered as u32));
                }
            }
        }
        starts.push(taps.len());
        Lines { taps, starts }
    }

    /// Line `index`'s taps; none for a line the plan does not hold.
    fn line(&self, index: usize) -> &[Tap] {
        match (self.starts.get(index), self.starts.get(index + 1)) {
            (Some(&from), Some(&to)) => self.taps.get(from..to).unwrap_or(&[]),
            _ => &[],
        }
    }
}

/// The image's unit square in device space, as coverage over the pixels the
/// draw visits.
///
/// 11.4.5 calls this the element's *shape*, and the render layer already
/// builds the same quad for a knockout group's restore. Here it is the
/// anti-aliasing: a pixel the square half covers takes half the paint.
///
/// The tolerance cannot matter — four corners and four straight edges have
/// nothing to flatten — but `fill` takes one, so it is named rather than left
/// as a bare number somebody would later try to tune. The region is `(x0, y0,
/// width, height)`, and the square is filled in `scratch`: its coverage, or
/// with `hard` whole or empty by that rule ([`fill_hard_with`]).
fn unit_quad(
    scratch: &mut FillScratch,
    t: &Transform,
    (x0, y0, width, height): (i32, i32, u32, u32),
    stop: Option<&dyn Fn() -> bool>,
    hard: Option<HardEdge>,
) -> Mask {
    /// Unused: the quad has no curves.
    const TOLERANCE: f64 = 0.2;

    // Snapped to the fill's own 1/256 grid. An image rectangle placed by a
    // translation lands on a 1/256 boundary far more often than not — every
    // strip of every scan does. The translation is exact but the arithmetic
    // that applies it need not be: `a + (e - t)` and `(a + e) - t` differ by
    // an ulp, and an ulp either side of a grid line falls on the wrong side
    // of it. `fill` takes a crossing to the nearest 1/256 unit and an edge's
    // rows from `ceil(y × 16)`, and neither rule can reconcile two corners
    // that already differ, so the corners are made one value here instead
    // (`docs/design/image-edges.md`). `round` is one of the operations
    // IEEE 754 pins exactly, so this costs no determinism (ruling 4). The
    // `crop` relation requires a cropped page to be the sub-rectangle of
    // the whole one *exactly*, and `pclm-in.pdf` is where it showed.
    let snap = |value: f64| (value * 256.0).round() / 256.0;

    let mut quad = Path::new();
    for (index, (x, y)) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]
        .iter()
        .enumerate()
    {
        let (x, y) = t.apply(*x, *y);
        let (x, y) = (snap(x), snap(y));
        if index == 0 {
            quad.move_to(x, y);
        } else {
            quad.line_to(x, y);
        }
    }
    quad.close();

    match hard {
        None => fill_with(
            scratch,
            &quad,
            FillRule::NonZero,
            x0,
            y0,
            width,
            height,
            TOLERANCE,
            stop,
        ),
        Some(edge) => fill_hard_with(
            scratch,
            &quad,
            FillRule::NonZero,
            x0,
            y0,
            width,
            height,
            TOLERANCE,
            stop,
            edge,
        ),
    }
}

/// The destination pixels the unit square can reach, clipped to the canvas's
/// device rectangle, as `(x0, x1, y0, y1)` with the far edges exclusive.
///
/// Each edge is clamped into the rectangle independently, so an image wholly
/// off one side comes back with its far edge at or below its near one — which
/// every caller reads as "nothing to draw".
fn device_bounds(t: &Transform, rect: (i32, i32, u32, u32)) -> Option<(i32, i32, i32, i32)> {
    let corners = [
        t.apply(0.0, 0.0),
        t.apply(1.0, 0.0),
        t.apply(0.0, 1.0),
        t.apply(1.0, 1.0),
    ];
    if corners
        .iter()
        .any(|(x, y)| !x.is_finite() || !y.is_finite())
    {
        return None;
    }
    let xs = corners.iter().map(|(x, _)| *x);
    let ys = corners.iter().map(|(_, y)| *y);
    let (left, top) = (f64::from(rect.0), f64::from(rect.1));
    let right = left + f64::from(rect.2);
    let bottom = top + f64::from(rect.3);
    // In `f64` until the clamp is done: a corner at 1e300 must land on the
    // canvas edge rather than saturate somewhere past it. Both ends of the
    // clamp are integers well inside `i32`, so the conversion is exact.
    let x0 = xs.clone().fold(f64::INFINITY, f64::min).floor().max(left);
    let x1 = xs.fold(f64::NEG_INFINITY, f64::max).ceil().min(right);
    let y0 = ys.clone().fold(f64::INFINITY, f64::min).floor().max(top);
    let y1 = ys.fold(f64::NEG_INFINITY, f64::max).ceil().min(bottom);
    let x0 = x0.min(right) as i32;
    let y0 = y0.min(bottom) as i32;
    let x1 = x1.max(left) as i32;
    let y1 = y1.max(top) as i32;
    Some((x0, x1, y0, y1))
}

/// One truncating tap: the sample the pixel's centre lands in.
///
/// Image rows run top-down and the unit square's `v` runs upward, which is the
/// flip in the second line.
fn nearest(image: &ImageSource<'_>, u: f64, v: f64) -> Option<(u8, u8, u8, u8)> {
    let sx = ((u * f64::from(image.width)) as u32).min(image.width - 1);
    let sy = (((1.0 - v) * f64::from(image.height)) as u32).min(image.height - 1);
    image.texel(sx, sy)
}

/// Four taps around the pixel's centre, weighted by how near each is.
///
/// Sample centres sit at half-integers, which is the `- 0.5` below: without
/// it the whole image shifts by half a sample and a 1:1 draw stops being
/// itself. Samples off the edge clamp to the edge rather than fading out,
/// because an image has no defined colour outside itself and fading to one
/// would put a dark rim on every magnified picture.
fn bilinear(image: &ImageSource<'_>, u: f64, v: f64) -> Option<(u8, u8, u8, u8)> {
    bilinear_taps(
        image,
        pair(u * f64::from(image.width) - 0.5, image.width),
        pair((1.0 - v) * f64::from(image.height) - 0.5, image.height),
    )
}

/// Bilinear's share of one axis: the tap below the point and the tap above
/// it, both clamped into the image, and how far past the first the point lies
/// in 1/256ths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pair {
    /// The tap at or below the point.
    near: u32,
    /// The tap above it.
    far: u32,
    /// How far past `near` the point lies, 0..=255.
    weight: u32,
}

/// The pair a coordinate in texels, sample centres at half-integers, reads
/// along an axis of `extent` samples.
fn pair(position: f64, extent: u32) -> Pair {
    let (index, weight) = split(position);
    // `extent` is at least one: an empty image is refused before any plan.
    let last = i64::from(extent) - 1;
    Pair {
        near: index.clamp(0, last) as u32,
        far: index.saturating_add(1).clamp(0, last) as u32,
        weight,
    }
}

/// Four taps, weighted.
///
/// # Why the weights are integers
///
/// Ruling 4. The two fractions are the only float arithmetic here, and they
/// became 0..=255 in [`split`]; everything after that is a `u32` multiply-add
/// with round-half-up, so the result cannot depend on how a target rounds. The
/// four weights sum to exactly 65 536, which is what keeps four identical
/// samples returning that sample rather than one level below it.
fn bilinear_taps(image: &ImageSource<'_>, x: Pair, y: Pair) -> Option<(u8, u8, u8, u8)> {
    if let Some(two) = &image.two_colour {
        return two_colour_bilinear_taps(image, two, x, y);
    }
    // The nearer of the two rows and columns decides whether the pixel is on
    // the image at all; the other three fall back to it where the buffer is
    // shorter than the dimensions claim, so a truncated image blurs into its
    // own last sample rather than into black.
    let anchor = image.texel(x.near, y.near)?;
    let t10 = image.texel(x.far, y.near).unwrap_or(anchor);
    let t01 = image.texel(x.near, y.far).unwrap_or(anchor);
    let t11 = image.texel(x.far, y.far).unwrap_or(anchor);
    summed(4, 4);

    let (w00, w10, w01, w11) = bilinear_weights(x, y);
    let mix = |a: u8, b: u8, c: u8, d: u8| {
        let total =
            u32::from(a) * w00 + u32::from(b) * w10 + u32::from(c) * w01 + u32::from(d) * w11;
        ((total + 32768) >> 16) as u8
    };

    Some((
        mix(anchor.0, t10.0, t01.0, t11.0),
        mix(anchor.1, t10.1, t01.1, t11.1),
        mix(anchor.2, t10.2, t01.2, t11.2),
        mix(anchor.3, t10.3, t01.3, t11.3),
    ))
}

/// Bilinear's four weights, for the taps `(near, near)`, `(far, near)`,
/// `(near, far)` and `(far, far)`: each the product of the two axes' shares,
/// and the four summing to exactly 65 536.
fn bilinear_weights(x: Pair, y: Pair) -> (u32, u32, u32, u32) {
    let (wx, wy) = (x.weight, y.weight);
    (
        (256 - wx) * (256 - wy),
        wx * (256 - wy),
        (256 - wx) * wy,
        wx * wy,
    )
}

/// Four taps of a two-colour image, weighted: [`bilinear_taps`] summing one
/// channel.
///
/// Each channel of the four-tap mix is `X0 × (weight on taps of the first
/// colour) + X1 × (weight on taps of the second)`, and the four weights sum to
/// 65 536, so `B1`, the weight on taps of the second colour, is all a pixel
/// needs: its channels are `(X0 × (65536 − B1) + X1 × B1 + 32768) >> 16`, the
/// same integers the image expanded to RGB mixes. A tap past a short buffer
/// takes the anchor's choice, as the RGB taps take the anchor's colour.
fn two_colour_bilinear_taps(
    image: &ImageSource<'_>,
    two: &TwoColour<'_>,
    x: Pair,
    y: Pair,
) -> Option<(u8, u8, u8, u8)> {
    let anchor = image.chosen(two, x.near, y.near)?;
    let t10 = image.chosen(two, x.far, y.near).unwrap_or(anchor);
    let t01 = image.chosen(two, x.near, y.far).unwrap_or(anchor);
    let t11 = image.chosen(two, x.far, y.far).unwrap_or(anchor);
    summed(4, 1);

    let (w00, w10, w01, w11) = bilinear_weights(x, y);
    let ones = u32::from(anchor) * w00
        + u32::from(t10) * w10
        + u32::from(t01) * w01
        + u32::from(t11) * w11;
    Some(two_colour_mix(two.colours, u64::from(ones), 1 << 16))
}

/// A pixel's colour from a two-colour image's two sums: `ones`, the weight on
/// samples of the second colour, out of `weight` in all.
///
/// Each channel is `(X0 × (weight − ones) + X1 × ones + weight / 2) / weight`,
/// which is what both filters divide the RGB expansion's channel sum by: the
/// area filter's quotient over any weight, and bilinear's shift where the
/// weight is 65 536. A pixel wholly on one colour is that colour, because
/// `(X × W + W / 2) / W` is `X` for every `W`, which is what makes the
/// interior of a scan cost no division. `weight` is never zero, and every
/// channel sum is at most 255 × `weight`: the bound the RGB sums keep.
fn two_colour_mix(colours: [[u8; 4]; 2], ones: u64, weight: u64) -> (u8, u8, u8, u8) {
    let [zero, one] = colours;
    if ones == 0 {
        return (zero[0], zero[1], zero[2], zero[3]);
    }
    if ones == weight {
        return (one[0], one[1], one[2], one[3]);
    }
    let zeros = weight - ones;
    let half = weight / 2;
    let mix =
        |c: usize| ((u64::from(zero[c]) * zeros + u64::from(one[c]) * ones + half) / weight) as u8;
    (mix(0), mix(1), mix(2), mix(3))
}

/// Every sample the destination pixel covers, weighted by how much of it the
/// pixel covers: the area filter for a pixel mapped on its own.
///
/// `scratch` holds the pixel's two lines of taps, and is the caller's so that
/// a draw allocates them once rather than once a pixel.
///
/// ## Why samples outside the image are dropped rather than clamped
///
/// [`bilinear`] clamps, because it is interpolating *between* samples and an
/// edge has nothing on its far side. This is averaging the samples a pixel
/// covers, so a pixel half off the image covers half as many samples and the
/// answer is the average of the half that exist. Clamping instead would weight
/// the edge sample twice and pull every border pixel towards it — visible as a
/// dark or light rim on every image drawn at a fractional offset.
fn area(
    image: &ImageSource<'_>,
    u: f64,
    v: f64,
    footprint: (u64, u64),
    scratch: &mut (Vec<Tap>, Vec<Tap>),
) -> Option<(u8, u8, u8, u8)> {
    // The pixel's centre in texels, 16.16. `v` counts up the image and texel
    // rows count down it, which is the flip every sampler here performs.
    let cx = area_centre(u, image.width);
    let cy = area_centre(1.0 - v, image.height);
    let (hx, hy) = half_footprint(footprint);
    let (columns, rows) = scratch;
    columns.clear();
    rows.clear();
    area_line(
        cx.saturating_sub(hx),
        cx.saturating_add(hx),
        image.width,
        columns,
    );
    area_line(
        cy.saturating_sub(hy),
        cy.saturating_add(hy),
        image.height,
        rows,
    );
    area_taps(image, columns, rows)
}

/// A position `t` of the way along an axis of `extent` samples, in 16.16
/// texels.
fn area_centre(t: f64, extent: u32) -> i64 {
    (t * f64::from(extent) * 65536.0) as i64
}

/// Half a 16.16 footprint, per axis: how far a pixel's source span reaches
/// either side of its centre.
fn half_footprint(footprint: (u64, u64)) -> (i64, i64) {
    (
        (footprint.0 / 2).min(i64::MAX as u64) as i64,
        (footprint.1 / 2).min(i64::MAX as u64) as i64,
    )
}

/// Appends to `out` the texels the 16.16 span `[lo, hi)` touches along an axis
/// of `extent` samples, each with how much of it the span covers, leaving out
/// any it covers none of.
///
/// A span's ends saturate at `i64`'s range rather than wrapping, which only an
/// axis of one sample squashed past 10^14 samples a pixel reaches.
fn area_line(lo: i64, hi: i64, extent: u32, out: &mut Vec<Tap>) {
    for index in span(lo, hi, extent) {
        let covered = overlap(lo, hi, index);
        if covered != 0 {
            // At most a whole texel, 65 536.
            out.push((index, covered as u32));
        }
    }
}

/// The area filter over two lines of taps: every sample present, weighted by
/// the product of its two overlaps.
///
/// ## Why this is integers, and why `u64` holds it
///
/// The weights are areas, and an area is a product of two overlaps. In floats
/// the accumulation order would decide the last bit, and ruling 4 says a last
/// bit is a different image. So every overlap is an integer and so is every
/// sum, gathered a row at a time: `oy × (Σ ox × t)` over the rows is
/// `Σ (ox × oy) × t` exactly, and the weight beside it sums `ox × oy` over the
/// samples the buffer holds, so a sample a short buffer stops before drops out
/// of both.
///
/// An overlap is at most 2^16, and a pixel's overlaps along one axis sum to at
/// most its footprint: 128 texels to 128:1, where nothing is reduced, and past
/// it 4 texels, a level of two samples, or after sixteen halvings a level of
/// at most 2^16 samples. A channel's sum is at most `255 × Σox × Σoy`: under
/// 2^54 to 128:1 and under 2^63 with one axis halved sixteen times. A sum past
/// 2^64 needs both axes halved sixteen times and still thousands of samples
/// wide — an image of 2^56 samples, whose first pyramid level no address space
/// holds. `docs/design/image-edges.md`, P-08, says the same at more length.
fn area_taps(image: &ImageSource<'_>, columns: &[Tap], rows: &[Tap]) -> Option<(u8, u8, u8, u8)> {
    if let Some(two) = &image.two_colour {
        return two_colour_area_taps(image, two, columns, rows);
    }
    let mut sum = [0u64; 4];
    let mut weight = 0u64;
    for &(ty, oy) in rows {
        let mut line = [0u64; 4];
        let mut line_weight = 0u64;
        for &(tx, ox) in columns {
            let Some(texel) = image.texel(tx, ty) else {
                continue;
            };
            summed(1, 4);
            let ox = u64::from(ox);
            line_weight += ox;
            line[0] += ox * u64::from(texel.0);
            line[1] += ox * u64::from(texel.1);
            line[2] += ox * u64::from(texel.2);
            line[3] += ox * u64::from(texel.3);
        }
        let oy = u64::from(oy);
        weight += oy * line_weight;
        for (total, part) in sum.iter_mut().zip(line) {
            *total += oy * part;
        }
    }
    if weight == 0 {
        return None;
    }
    let half = weight / 2;
    let mix = |channel: u64| ((channel + half) / weight) as u8;
    Some((mix(sum[0]), mix(sum[1]), mix(sum[2]), mix(sum[3])))
}

/// The area filter over a two-colour image: [`area_taps`] summing one
/// channel.
///
/// A channel's sum over the image expanded to RGB is
/// `Σ (ox × oy) × X` over the samples present, which is `X0 × W0 + X1 × W1`
/// with `W1` the weight on samples of the second colour and `W0` the rest of
/// the weight `W` — exactly, in integers. So a pixel sums `W1` beside `W`, a
/// row at a time as the RGB taps do, and [`two_colour_mix`] makes each channel
/// the quotient the RGB sums give. The sampler reaches a two-colour image only
/// where the pyramid reduced nothing, so `W` is at most 2^46 and every channel
/// sum stays under 2^54.
fn two_colour_area_taps(
    image: &ImageSource<'_>,
    two: &TwoColour<'_>,
    columns: &[Tap],
    rows: &[Tap],
) -> Option<(u8, u8, u8, u8)> {
    let mut ones = 0u64;
    let mut weight = 0u64;
    for &(ty, oy) in rows {
        let mut line_ones = 0u64;
        let mut line_weight = 0u64;
        for &(tx, ox) in columns {
            let Some(one) = image.chosen(two, tx, ty) else {
                continue;
            };
            summed(1, 1);
            let ox = u64::from(ox);
            line_weight += ox;
            line_ones += ox * u64::from(one);
        }
        let oy = u64::from(oy);
        weight += oy * line_weight;
        ones += oy * line_ones;
    }
    if weight == 0 {
        return None;
    }
    Some(two_colour_mix(two.colours, ones, weight))
}

/// The texel indices a 16.16 span touches, clipped to the image.
fn span(lo: i64, hi: i64, extent: u32) -> core::ops::Range<u32> {
    if extent == 0 || hi <= 0 {
        return 0..0;
    }
    let first = (lo.max(0) >> 16).min(i64::from(u32::MAX)) as u32;
    // `hi - 1` so a span ending exactly on a boundary does not reach for the
    // texel past it, which would contribute a zero-area tap and, at the right
    // edge, read one sample too far.
    let last = ((hi - 1).max(0) >> 16).min(i64::from(u32::MAX)) as u32;
    let first = first.min(extent - 1);
    let last = last.min(extent - 1);
    first..(last + 1)
}

/// How much of texel `index` the 16.16 span `[lo, hi)` covers, in 1/65536ths.
fn overlap(lo: i64, hi: i64, index: u32) -> u64 {
    let start = i64::from(index) << 16;
    let end = start + 65536;
    (hi.min(end) - lo.max(start)).max(0) as u64
}

/// A coordinate as the sample below it and how far past it the point lies, in
/// 1/256ths.
///
/// The floor without calling `floor`: a truncating cast, one less where the
/// cast rounded up, which it does for a negative value that is not whole. That
/// is `value.floor()` for every `|value| < 2^52` and for NaN, and the value is
/// a clamped `u` or `v` times the image's extent less a half, so it lies in
/// `[-0.5, 2^32]`. The fraction is then the same subtraction of the same two
/// numbers `floor` would have made.
fn split(value: f64) -> (i64, u32) {
    let truncated = (value as i64) as f64;
    let floor = if truncated > value {
        truncated - 1.0
    } else {
        truncated
    };
    // Saturating: a placement far outside the canvas still has to produce an
    // index, and the clamp in `pair` turns any of them into an edge sample.
    let index = floor as i64;
    let fraction = ((value - floor) * 256.0) as u32;
    (index, fraction.min(255))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::PixelFormat;

    /// A `size` x `size` image whose samples are all distinct.
    fn ramp(size: u32) -> Vec<u8> {
        let mut rgb = Vec::new();
        for y in 0..size {
            for x in 0..size {
                rgb.extend_from_slice(&[(x * 8) as u8, (y * 8) as u8, 0]);
            }
        }
        rgb
    }

    /// The transform that puts an image over the whole of a square canvas.
    ///
    /// Device y runs down and the unit square's v runs up, so the placement
    /// flips: `d` is negative and `f` is the far edge.
    fn over(size: f64) -> Transform {
        Transform {
            a: size,
            b: 0.0,
            c: 0.0,
            d: -size,
            e: 0.0,
            f: size,
        }
    }

    fn draw(canvas: &mut Canvas, image: &ImageSource<'_>, t: Transform) {
        draw_image(
            canvas,
            &ImageDraw::new(*image, t),
            &mut Pyramid::new(),
            &mut FillScratch::new(),
        );
    }

    /// An image wholly off the canvas draws nothing and **allocates nothing**.
    ///
    /// `device_bounds` clamps the near edge up to zero and the far edge down
    /// to the canvas independently, so a placement past one side comes back
    /// far-below-near — `5000..64` for the case below. The draw loop's ranges
    /// were empty either way, which is why this was invisible until the
    /// coverage mask started taking its *size* from the difference: the
    /// subtraction underflowed and asked for four billion pixels. Found by the
    /// qpdf corpus, on `pclm-out.pdf`, as a 1.2 TB allocation abort (ruling 1).
    #[test]
    fn an_image_entirely_off_the_canvas_costs_nothing() {
        let rgb = ramp(8);
        let image = ImageSource {
            width: 8,
            height: 8,
            rgb: &rgb,
            alpha: &[],
            two_colour: None,
            window: None,
        };
        let mut canvas = Canvas::new(64, 64, PixelFormat::Rgb8, Color::WHITE);
        for (e, f) in [(5000.0, 0.0), (-5000.0, 0.0), (0.0, 5000.0), (0.0, -5000.0)] {
            let placement = Transform {
                a: 32.0,
                b: 0.0,
                c: 0.0,
                d: -32.0,
                e,
                f: f + 32.0,
            };
            draw(&mut canvas, &image, placement);
        }
        assert!(
            canvas.data.iter().all(|byte| *byte == 255),
            "an image off every side of the canvas left ink on it"
        );
    }

    /// **A draw cut at a smaller square samples the square its samples span
    /// and covers the cut** (P-09.8): three by two samples standing for a
    /// five by three image, each two of its own, span 1.2 by 4/3 of the
    /// image's square from its top-left corner. Drawn over that square and
    /// cut at the image's own, they are the same samples drawn over the larger
    /// square under a clip of the smaller one, pixel for pixel — the cut's
    /// edges are whole pixels, so neither coverage is partial — and nothing
    /// outside the cut is painted, where the same draw uncut paints the rest
    /// of the larger square. The same holds for a run's accumulation.
    #[test]
    fn a_cut_draw_samples_its_square_and_covers_the_cut() {
        let rgb: Vec<u8> = (0..3u32 * 2 * 3).map(|i| (i * 37 % 256) as u8).collect();
        let image = ImageSource {
            width: 3,
            height: 2,
            rgb: &rgb,
            alpha: &[],
            two_colour: None,
            window: None,
        };
        // The image's own square: 50 by 30 pixels at (4, 6), its first row at
        // the top.
        let unit = Transform {
            a: 50.0,
            b: 0.0,
            c: 0.0,
            d: -30.0,
            e: 4.0,
            f: 36.0,
        };
        // Its samples' square: 60 by 40 pixels from the same top-left corner.
        let samples = Transform {
            a: 60.0,
            b: 0.0,
            c: 0.0,
            d: -40.0,
            e: 4.0,
            f: 46.0,
        };
        let paint = |draw: &ImageDraw<'_>| {
            let mut canvas = Canvas::new(80, 60, PixelFormat::Rgb8, Color::WHITE);
            draw_image(
                &mut canvas,
                draw,
                &mut Pyramid::new(),
                &mut FillScratch::new(),
            );
            canvas
        };
        let cut = paint(&ImageDraw {
            cut: Some(unit),
            ..ImageDraw::new(image, samples)
        });
        let clip = Mask::uniform(4, 6, 50, 30, 255);
        let clipped = paint(&ImageDraw {
            clip: Some(&clip),
            ..ImageDraw::new(image, samples)
        });
        assert!(
            cut.data == clipped.data,
            "the cut draw is the larger square's samples clipped to the cut"
        );
        let uncut = paint(&ImageDraw::new(image, samples));
        for y in 0..60usize {
            for x in 0..80usize {
                let at = (y * 80 + x) * 3;
                let inside = (4..54).contains(&x) && (6..36).contains(&y);
                if inside {
                    assert_eq!(cut.data[at..at + 3], uncut.data[at..at + 3], "({x}, {y})");
                } else {
                    assert_eq!(
                        cut.data[at..at + 3],
                        [255, 255, 255],
                        "({x}, {y}) is past the cut"
                    );
                }
            }
        }
        assert!(
            uncut.data != cut.data,
            "uncut, the samples' square paints past the image"
        );

        // A run accumulates the cut square's fragments and no others.
        let extent = (0, 0, 80, 60);
        let mut fragments = Fragments::new(0, 0, 80, 60);
        accumulate_image(
            &mut fragments,
            &ImageDraw {
                cut: Some(unit),
                ..ImageDraw::new(image, samples)
            },
            &mut Pyramid::new(),
            extent,
            &mut FillScratch::new(),
        );
        let outside = Mask::uniform(0, 36, 80, 24, 255);
        assert!(
            !fragments.would_overlap(&outside),
            "nothing below the cut was accumulated"
        );
        let inside = Mask::uniform(10, 10, 4, 4, 255);
        assert!(fragments.would_overlap(&inside), "the cut square was");
    }

    /// With anti-aliasing off, a rotated image's edge is whole or absent at
    /// every pixel — and the same draw with it on is not, so the fixture has
    /// an edge worth hardening. One colour of samples, so every covered pixel
    /// must be exactly that colour and every other one exactly the page.
    #[test]
    fn a_hard_edged_image_covers_each_pixel_wholly_or_not_at_all() {
        let rgb = [0u8, 160, 40].repeat(4);
        let image = ImageSource {
            width: 2,
            height: 2,
            rgb: &rgb,
            alpha: &[],
            two_colour: None,
            window: None,
        };
        // A quarter of a turn and a bit, with a translation off the grid.
        let placement = Transform {
            a: 20.0,
            b: 11.0,
            c: -11.0,
            d: 20.0,
            e: 17.3,
            f: 4.6,
        };
        let render = |edge_bits: u8| {
            let mut canvas = Canvas::new(48, 48, PixelFormat::Rgb8, Color::WHITE);
            let draw = ImageDraw {
                edge_bits,
                ..ImageDraw::new(image, placement)
            };
            draw_image(
                &mut canvas,
                &draw,
                &mut Pyramid::new(),
                &mut FillScratch::new(),
            );
            canvas
        };
        let partial = |canvas: &Canvas| {
            canvas
                .data
                .chunks_exact(3)
                .filter(|p| *p != [255, 255, 255] && *p != [0, 160, 40])
                .count()
        };
        let (soft, hard) = (render(8), render(0));
        assert!(partial(&soft) > 20, "the soft edge has partial pixels");
        assert_eq!(partial(&hard), 0, "the hard edge has none");
        let inked = hard
            .data
            .chunks_exact(3)
            .filter(|p| *p == [0, 160, 40])
            .count();
        // The quad's area is 20² + 11² = 521 pixels.
        assert!(
            inked.abs_diff(521) < 30,
            "a hard edge keeps the image's area, {inked} against 521"
        );
    }

    /// **An image's edge quantises as a path's edge** (RC-11.2).
    ///
    /// A black image turned a little past a quarter and placed off the grid,
    /// drawn with its edge at every level from 0 to 8, is byte for byte the
    /// canvas its unit square's coverage — `image_coverage` over the same
    /// pixels — quantised at that level by `Mask::quantise` and filled in
    /// black gives: the image's edge goes through the function a path's
    /// coverage goes through. Over white, a pixel the edge covers `c` is
    /// `255 − c`, so at 1 to 7 bits every pixel is one of the level's values
    /// taken from white; at 8 the edge has partial pixels, and at 0 none.
    #[test]
    fn an_image_edge_quantises_as_a_path_edge() {
        let rgb = [0u8, 0, 0].repeat(4);
        let image = ImageSource {
            width: 2,
            height: 2,
            rgb: &rgb,
            alpha: &[],
            two_colour: None,
            window: None,
        };
        let placement = Transform {
            a: 20.0,
            b: 11.0,
            c: -11.0,
            d: 20.0,
            e: 17.3,
            f: 4.6,
        };
        let (x0, y0, width, height) =
            image_bounds(&placement, 48, 48).expect("the image is on the canvas");
        let partial = |canvas: &Canvas| {
            canvas
                .data
                .iter()
                .filter(|v| **v != 0 && **v != 255)
                .count()
        };
        for edge_bits in 0..=8u8 {
            let mut drawn = Canvas::new(48, 48, PixelFormat::Rgb8, Color::WHITE);
            draw_image(
                &mut drawn,
                &ImageDraw {
                    edge_bits,
                    ..ImageDraw::new(image, placement)
                },
                &mut Pyramid::new(),
                &mut FillScratch::new(),
            );
            let mut shape = image_coverage(
                &placement,
                x0,
                y0,
                width,
                height,
                None,
                &mut FillScratch::new(),
            );
            shape.quantise(edge_bits);
            let mut filled = Canvas::new(48, 48, PixelFormat::Rgb8, Color::WHITE);
            filled.fill_mask(&shape, Color::BLACK, 1.0);
            assert_eq!(drawn.data, filled.data, "the edge at {edge_bits} bits");

            match edge_bits {
                0 => assert_eq!(partial(&drawn), 0, "a hard edge has no partial pixel"),
                8 => assert!(partial(&drawn) > 20, "the soft edge has partial pixels"),
                bits => {
                    let n = (1u32 << bits) - 1;
                    let levels: Vec<u8> = (0..=n)
                        .map(|k| (255 - (2 * k * 255 + n) / (2 * n)) as u8)
                        .collect();
                    assert!(
                        drawn.data.iter().all(|v| levels.contains(v)),
                        "at {bits} bits every pixel is one of the level's values"
                    );
                }
            }
        }
    }

    /// **An image's edge under the centre rule is the pixels whose centres its
    /// square holds** (RC-11.3).
    ///
    /// A black image upright on the canvas, its corners off the grid at
    /// `(2.3, 3.35)` and `(7.8, 7.6)`: at 0 bits under `HardEdge::Centre` it
    /// paints exactly the pixels whose centres the rectangle holds, columns 2
    /// to 7 and rows 3 to 7, computed here from the corners — a centre
    /// `c + ½` at or right of the left edge and left of the right one, as a
    /// span holds its units — and nothing else, with no partial pixel. Under
    /// `HardEdge::Half` the same draw is its square's coverage hardened: an
    /// edge alone parts its pixels as the centres do, so column 2, 0.7 of a
    /// pixel across, is kept, but the two left corners, 0.7 across times
    /// 0.625 down, are under half and left out where their centres are in.
    /// The rule is read at 0 bits only: at 8 the edge is soft whatever it
    /// says. And turned, the drawn edge is the unit square filled by the rule
    /// over the draw's own pixels.
    #[test]
    fn an_image_edge_under_the_centre_rule_paints_the_pixels_whose_centres_it_holds() {
        let rgb = [0u8, 0, 0].repeat(4);
        let image = ImageSource {
            width: 2,
            height: 2,
            rgb: &rgb,
            alpha: &[],
            two_colour: None,
            window: None,
        };
        let upright = Transform {
            a: 5.5,
            b: 0.0,
            c: 0.0,
            d: -4.25,
            e: 2.3,
            f: 7.6,
        };
        let render = |placement: Transform, edge_bits: u8, edge_rule: HardEdge| {
            let mut canvas = Canvas::new(12, 12, PixelFormat::Rgb8, Color::WHITE);
            draw_image(
                &mut canvas,
                &ImageDraw {
                    edge_bits,
                    edge_rule,
                    ..ImageDraw::new(image, placement)
                },
                &mut Pyramid::new(),
                &mut FillScratch::new(),
            );
            canvas
        };
        let pixel = |canvas: &Canvas, x: usize, y: usize| {
            let at = (y * 12 + x) * 3;
            [canvas.data[at], canvas.data[at + 1], canvas.data[at + 2]]
        };

        let centred = render(upright, 0, HardEdge::Centre);
        for y in 0..12 {
            for x in 0..12 {
                let (cx, cy) = (x as f64 + 0.5, y as f64 + 0.5);
                let held = (2.3..7.8).contains(&cx) && (3.35..7.6).contains(&cy);
                let want = if held { [0, 0, 0] } else { [255, 255, 255] };
                assert_eq!(pixel(&centred, x, y), want, "pixel ({x}, {y})");
            }
        }

        let halved = render(upright, 0, HardEdge::Half);
        assert_eq!(pixel(&halved, 2, 4), [0, 0, 0], "0.7 of the pixel is held");
        assert_eq!(pixel(&halved, 4, 3), [0, 0, 0], "and 0.625 down");
        for corner in [(2, 3), (2, 7)] {
            assert_eq!(
                pixel(&halved, corner.0, corner.1),
                [255, 255, 255],
                "the corner {corner:?} is under half a pixel"
            );
            assert_eq!(
                pixel(&centred, corner.0, corner.1),
                [0, 0, 0],
                "and its centre is in"
            );
        }
        assert_eq!(
            render(upright, 8, HardEdge::Centre).data,
            render(upright, 8, HardEdge::Half).data,
            "at 8 bits the rule is not read"
        );

        let turned = Transform {
            a: 6.0,
            b: 3.3,
            c: -3.3,
            d: 6.0,
            e: 5.37,
            f: 1.18,
        };
        let drawn = render(turned, 0, HardEdge::Centre);
        let (x0, y0, width, height) =
            image_bounds(&turned, 12, 12).expect("the image is on the canvas");
        let shape = unit_quad(
            &mut FillScratch::new(),
            &turned,
            (x0, y0, width, height),
            None,
            Some(HardEdge::Centre),
        );
        let mut filled = Canvas::new(12, 12, PixelFormat::Rgb8, Color::WHITE);
        filled.fill_mask(&shape, Color::BLACK, 1.0);
        assert_eq!(drawn.data, filled.data, "the turned square, by its centres");
        assert!(
            drawn.data.iter().all(|v| *v == 0 || *v == 255),
            "no partial pixel"
        );
    }

    /// **An image's edge under the any-part rule paints every pixel its
    /// square meets** (RC-11.4).
    ///
    /// A black image upright on the canvas, its corners at `(2.6, 3.35)` and
    /// `(7.4, 7.6)`: at 0 bits under `HardEdge::AnyPart` it paints exactly the
    /// pixels whose open squares the rectangle meets, columns 2 to 7 and rows
    /// 3 to 7, computed here from the corners, where the centre rule keeps
    /// columns 3 to 6 — the outer columns' shares, 0.4 of a pixel, hold no
    /// centre. Turned, the drawn edge is the unit square filled by the rule.
    #[test]
    fn an_image_edge_under_the_any_part_rule_paints_every_pixel_its_square_meets() {
        let rgb = [0u8, 0, 0].repeat(4);
        let image = ImageSource {
            width: 2,
            height: 2,
            rgb: &rgb,
            alpha: &[],
            two_colour: None,
            window: None,
        };
        let render = |placement: Transform, edge_rule: HardEdge| {
            let mut canvas = Canvas::new(12, 12, PixelFormat::Rgb8, Color::WHITE);
            draw_image(
                &mut canvas,
                &ImageDraw {
                    edge_bits: 0,
                    edge_rule,
                    ..ImageDraw::new(image, placement)
                },
                &mut Pyramid::new(),
                &mut FillScratch::new(),
            );
            canvas
        };
        let black = |canvas: &Canvas, x: usize, y: usize| canvas.data[(y * 12 + x) * 3] == 0;
        let upright = Transform {
            a: 4.8,
            b: 0.0,
            c: 0.0,
            d: -4.25,
            e: 2.6,
            f: 7.6,
        };
        let touched = render(upright, HardEdge::AnyPart);
        let centred = render(upright, HardEdge::Centre);
        for y in 0..12 {
            for x in 0..12 {
                let (left, top) = (x as f64, y as f64);
                let met = left < 7.4 && left + 1.0 > 2.6 && top < 7.6 && top + 1.0 > 3.35;
                assert_eq!(black(&touched, x, y), met, "pixel ({x}, {y})");
            }
        }
        assert!(black(&touched, 2, 5) && !black(&centred, 2, 5), "column 2");
        assert!(black(&touched, 7, 5) && !black(&centred, 7, 5), "column 7");
        assert!(black(&centred, 3, 5) && black(&centred, 6, 5));
        assert!(
            touched.data.iter().all(|v| *v == 0 || *v == 255),
            "no partial pixel"
        );

        let turned = Transform {
            a: 6.0,
            b: 3.3,
            c: -3.3,
            d: 6.0,
            e: 5.37,
            f: 1.18,
        };
        let drawn = render(turned, HardEdge::AnyPart);
        let (x0, y0, width, height) =
            image_bounds(&turned, 12, 12).expect("the image is on the canvas");
        let shape = unit_quad(
            &mut FillScratch::new(),
            &turned,
            (x0, y0, width, height),
            None,
            Some(HardEdge::AnyPart),
        );
        let mut filled = Canvas::new(12, 12, PixelFormat::Rgb8, Color::WHITE);
        filled.fill_mask(&shape, Color::BLACK, 1.0);
        assert_eq!(
            drawn.data, filled.data,
            "the turned square, every pixel it meets"
        );
    }

    #[test]
    fn a_one_to_one_blit_preserves_every_byte() {
        let rgb = ramp(8);
        let image = ImageSource {
            width: 8,
            height: 8,
            rgb: &rgb,
            alpha: &[],
            two_colour: None,
            window: None,
        };
        let mut canvas = Canvas::new(8, 8, PixelFormat::Rgb8, Color::WHITE);
        draw(&mut canvas, &image, over(8.0));

        // The image's top row is device row 0.
        for y in 0..8u32 {
            for x in 0..8u32 {
                let want = Color::rgb((x * 8) as u8, (y * 8) as u8, 0);
                assert_eq!(canvas.pixel(x, y), Some(want), "at {x},{y}");
            }
        }
    }

    #[test]
    fn a_degenerate_transform_draws_nothing() {
        let rgb = ramp(4);
        let image = ImageSource {
            width: 4,
            height: 4,
            rgb: &rgb,
            alpha: &[],
            two_colour: None,
            window: None,
        };
        let mut canvas = Canvas::new(4, 4, PixelFormat::Rgb8, Color::WHITE);
        draw(
            &mut canvas,
            &image,
            Transform {
                a: 0.0,
                b: 0.0,
                c: 0.0,
                d: 0.0,
                e: 2.0,
                f: 2.0,
            },
        );
        assert!(canvas.data.iter().all(|b| *b == 255), "nothing was painted");
    }

    #[test]
    fn a_transparent_sample_leaves_the_page_alone() {
        let rgb = vec![0u8; 4 * 3];
        let image = ImageSource {
            width: 2,
            height: 2,
            rgb: &rgb,
            alpha: &[0, 255, 0, 255],
            two_colour: None,
            window: None,
        };
        let mut canvas = Canvas::new(4, 4, PixelFormat::Rgb8, Color::WHITE);
        draw(&mut canvas, &image, over(4.0));

        assert_eq!(canvas.pixel(0, 0), Some(Color::WHITE), "transparent");
        assert_eq!(canvas.pixel(3, 0), Some(Color::BLACK), "opaque");
    }

    #[test]
    fn a_tint_replaces_the_image_colour_and_keeps_its_coverage() {
        let rgb = vec![0u8; 4 * 3];
        let image = ImageSource {
            width: 2,
            height: 2,
            rgb: &rgb,
            alpha: &[0, 255, 0, 255],
            two_colour: None,
            window: None,
        };
        let mut canvas = Canvas::new(4, 4, PixelFormat::Rgb8, Color::WHITE);
        let mut draw = ImageDraw::new(image, over(4.0));
        draw.tint = Some(Color::rgb(255, 0, 0));
        draw_image(
            &mut canvas,
            &draw,
            &mut Pyramid::new(),
            &mut FillScratch::new(),
        );

        assert_eq!(canvas.pixel(0, 0), Some(Color::WHITE), "still transparent");
        assert_eq!(canvas.pixel(3, 0), Some(Color::rgb(255, 0, 0)), "tinted");
    }

    /// A tint's own ink reaches an ink canvas, drawn directly and through a
    /// run alike; without a tint, or on a light canvas, it is never read.
    /// The tint is black light, which maximum undercolour removal would make
    /// black ink alone, and the ink it was chosen as is all four.
    #[test]
    fn an_ink_canvas_composites_a_tint_s_own_ink() {
        let rgb = vec![0u8; 3];
        let image = ImageSource {
            width: 1,
            height: 1,
            rgb: &rgb,
            alpha: &[255],
            two_colour: None,
            window: None,
        };
        let rich = Some([255, 255, 255, 255]);
        let at = |canvas: &Canvas| canvas.data[..5].to_vec();

        let mut draw = ImageDraw::new(image, over(4.0));
        draw.tint = Some(Color::BLACK);
        draw.ink = rich;
        let mut direct = Canvas::new(4, 4, PixelFormat::CmykA8, Color::TRANSPARENT);
        draw_image(
            &mut direct,
            &draw,
            &mut Pyramid::new(),
            &mut FillScratch::new(),
        );
        assert_eq!(at(&direct), [255, 255, 255, 255, 255], "drawn directly");

        let mut run = Fragments::new(0, 0, 4, 4);
        accumulate_image(
            &mut run,
            &draw,
            &mut Pyramid::new(),
            (0, 0, 4, 4),
            &mut FillScratch::new(),
        );
        let mut held = Canvas::new(4, 4, PixelFormat::CmykA8, Color::TRANSPARENT);
        run.composite_region_inked(
            &mut held,
            (0, 0, 4, 4),
            1.0,
            BlendMode::Normal,
            None,
            rich,
            None,
        );
        assert_eq!(at(&held), [255, 255, 255, 255, 255], "through a run");

        let mut light = Canvas::new(4, 4, PixelFormat::Rgb8, Color::WHITE);
        draw_image(
            &mut light,
            &draw,
            &mut Pyramid::new(),
            &mut FillScratch::new(),
        );
        assert_eq!(light.pixel(0, 0), Some(Color::BLACK), "light reads no ink");

        draw.tint = None;
        let mut untinted = Canvas::new(4, 4, PixelFormat::CmykA8, Color::TRANSPARENT);
        draw_image(
            &mut untinted,
            &draw,
            &mut Pyramid::new(),
            &mut FillScratch::new(),
        );
        assert_eq!(at(&untinted), [0, 0, 0, 255, 255], "no tint, no ink");
    }

    /// **A stencil's plate ink lands on its plates** (RC-04.8): drawn onto a
    /// plates canvas, each channel the ink names takes its tint where the
    /// stencil paints and every other channel no ink, process and spot
    /// alike, and nothing changes where it does not paint; an ink naming no
    /// spot draws, edge for edge, the bytes the tint's own ink draws; and on
    /// a canvas of another format the ink's process tints are the DeviceCMYK
    /// colour they are — that colour's own ink on `CmykA8`, its light on
    /// `Rgb8`. Without a tint the plate ink is never read.
    #[test]
    fn a_stencil_s_plate_ink_lands_on_its_plates() {
        use crate::canvas::Spots;
        use crate::fill::fill;

        let spots = Spots::new(2).expect("under the cap");
        let rgb = vec![0u8; 4 * 3];
        let stencil = ImageSource {
            width: 2,
            height: 2,
            rgb: &rgb,
            alpha: &[255, 0, 255, 255],
            two_colour: None,
            window: None,
        };
        let backdrop = PlateInk::named(spots, &[(0, 50), (4, 70)]);
        let page = || {
            let mut canvas = Canvas::plates(4, 4, 2, Color::WHITE).expect("under the cap");
            let mut whole = Path::new();
            whole.rect(0.0, 0.0, 4.0, 4.0);
            let mask = fill(&whole, FillRule::NonZero, 0, 0, 4, 4, 0.05, None);
            canvas.fill_mask_plates(&mask, &backdrop, 1.0, BlendMode::Normal);
            canvas
        };
        let ink = PlateInk::named(spots, &[(1, 90), (5, 220)]);
        let mut draw = ImageDraw::new(stencil, over(4.0));
        draw.tint = Some(Color::BLACK);
        // Beside plates on a plates canvas, never read.
        draw.ink = Some([255; 4]);
        draw.plates = Some(&ink);
        let mut canvas = page();
        draw_image(
            &mut canvas,
            &draw,
            &mut Pyramid::new(),
            &mut FillScratch::new(),
        );
        let plate = |canvas: &Canvas, channel: usize, x: usize, y: usize| {
            canvas.channel(channel).expect("a channel")[y * 4 + x]
        };
        for (channel, painted, untouched) in [
            (0, 0, 50),
            (1, 90, 0),
            (2, 0, 0),
            (3, 0, 0),
            (4, 0, 70),
            (5, 220, 0),
        ] {
            assert_eq!(plate(&canvas, channel, 0, 0), painted, "painted, {channel}");
            assert_eq!(plate(&canvas, channel, 3, 3), painted, "painted, {channel}");
            assert_eq!(
                plate(&canvas, channel, 3, 0),
                untouched,
                "where the stencil does not paint, {channel}"
            );
        }

        // Partly covered edges and part of an alpha: an ink naming no spot is
        // the tint's own ink, byte for byte.
        let process = [30, 0, 140, 200];
        let only = PlateInk::process(spots, process);
        let placed = Transform {
            a: 3.3,
            b: 0.0,
            c: 0.0,
            d: -2.7,
            e: 0.35,
            f: 3.6,
        };
        let mut by_plates = ImageDraw::new(stencil, placed);
        by_plates.tint = Some(Color::BLACK);
        by_plates.alpha = 0.7;
        by_plates.plates = Some(&only);
        let mut by_ink = ImageDraw::new(stencil, placed);
        by_ink.tint = Some(Color::BLACK);
        by_ink.alpha = 0.7;
        by_ink.ink = Some(process);
        let (mut plates_page, mut ink_page) = (page(), page());
        draw_image(
            &mut plates_page,
            &by_plates,
            &mut Pyramid::new(),
            &mut FillScratch::new(),
        );
        draw_image(
            &mut ink_page,
            &by_ink,
            &mut Pyramid::new(),
            &mut FillScratch::new(),
        );
        assert_eq!(plates_page.data, ink_page.data, "the process ink alone");

        // On another format, the DeviceCMYK colour the process tints are.
        let (r, g, b) = crate::canvas::cmyk_to_rgb(30, 0, 140, 200);
        let mut as_cmyk = ImageDraw::new(stencil, placed);
        as_cmyk.tint = Some(Color::rgb(r, g, b));
        as_cmyk.alpha = 0.7;
        as_cmyk.ink = Some(process);
        for format in [PixelFormat::CmykA8, PixelFormat::Rgb8] {
            let mut through = Canvas::new(4, 4, format, Color::WHITE);
            let mut direct = Canvas::new(4, 4, format, Color::WHITE);
            draw_image(
                &mut through,
                &by_plates,
                &mut Pyramid::new(),
                &mut FillScratch::new(),
            );
            draw_image(
                &mut direct,
                &as_cmyk,
                &mut Pyramid::new(),
                &mut FillScratch::new(),
            );
            assert_eq!(through.data, direct.data, "{format:?}");
        }

        // No tint, no plates: the image's own samples.
        let mut untinted = ImageDraw::new(stencil, over(4.0));
        untinted.plates = Some(&ink);
        let mut own = page();
        draw_image(
            &mut own,
            &untinted,
            &mut Pyramid::new(),
            &mut FillScratch::new(),
        );
        let mut plain = page();
        draw_image(
            &mut plain,
            &ImageDraw::new(stencil, over(4.0)),
            &mut Pyramid::new(),
            &mut FillScratch::new(),
        );
        assert_eq!(own.data, plain.data, "untinted");
    }

    #[test]
    fn a_short_buffer_skips_pixels_rather_than_panicking() {
        // Ruling 1: the dimensions claim sixteen samples and there are four.
        let rgb = vec![7u8; 4 * 3];
        let image = ImageSource {
            width: 4,
            height: 4,
            rgb: &rgb,
            alpha: &[],
            two_colour: None,
            window: None,
        };
        let mut canvas = Canvas::new(8, 8, PixelFormat::Rgb8, Color::WHITE);
        draw(&mut canvas, &image, over(8.0));
        assert_eq!(
            canvas.pixel(0, 0),
            Some(Color::rgb(7, 7, 7)),
            "what is there"
        );
        assert_eq!(canvas.pixel(7, 7), Some(Color::WHITE), "what is not");
    }

    #[test]
    fn drawing_stops_when_asked() {
        let rgb = ramp(8);
        let image = ImageSource {
            width: 8,
            height: 8,
            rgb: &rgb,
            alpha: &[],
            two_colour: None,
            window: None,
        };
        let mut canvas = Canvas::new(8, 8, PixelFormat::Rgb8, Color::WHITE);
        let mut draw = ImageDraw::new(image, over(8.0));
        let stop = || true;
        draw.stop = Some(&stop);
        draw_image(
            &mut canvas,
            &draw,
            &mut Pyramid::new(),
            &mut FillScratch::new(),
        );
        assert!(canvas.data.iter().all(|b| *b == 255), "no row was drawn");
    }

    /// Plan 07's first two rows: the flag is the only thing that decides
    /// whether a magnified image is **smoothed**.
    ///
    /// The claim is about smoothing, and it still holds — but it is no longer
    /// the same as "the filter is `Nearest`". Magnifying without the flag now
    /// averages the destination pixel's *own* footprint, which above 1:1 is
    /// smaller than one sample: a pixel inside a sample reads that sample and
    /// nothing else, and only a pixel straddling the boundary between two
    /// mixes them. That is the sample's edge anti-aliased, not the image
    /// blurred, and it is what `/Interpolate false` asks for — hard pixels.
    ///
    /// Nearest was quantising every internal sample boundary to whole device
    /// pixels, which is the same defect as the image's outer edge one level
    /// down, and it is what the `dpi` corpus relation was breaking on: a
    /// render at twice the scale resolves those boundaries twice as finely, so
    /// box-filtering it back down could not reproduce the coarser render.
    /// Measured on a 512-square source, share of pixels disagreeing between a
    /// render and the box-filtered render at twice the scale: **49.9 % before,
    /// 0.00 % after**, at every ratio tried.
    #[test]
    fn magnification_smooths_only_when_the_file_asks() {
        let rgb = ramp(2);
        let image = ImageSource {
            width: 2,
            height: 2,
            rgb: &rgb,
            alpha: &[],
            two_colour: None,
            window: None,
        };
        // Eight device pixels per axis for two samples: 4x. The footprint is a
        // quarter of a sample, so the average has one sample to take except on
        // a boundary.
        let hard = sampling_for(&image, &over(8.0), false);
        assert_eq!(hard.filter, Filter::Area);
        assert!(
            hard.footprint.0 < ONE && hard.footprint.1 < ONE,
            "a magnified pixel covers less than a sample: {:?}",
            hard.footprint
        );
        assert_eq!(
            sampling_for(&image, &over(8.0), true).filter,
            Filter::Bilinear
        );
        // And at exactly 1:1 the footprint is exactly one sample, which is what
        // keeps a 1:1 blit byte-preserving.
        let one_to_one = sampling_for(&image, &over(2.0), false);
        assert_eq!(one_to_one.footprint, (ONE, ONE));
    }

    #[test]
    fn a_magnified_image_gains_values_between_its_samples() {
        // Black and white on one diagonal, mid grey on the other.
        let rgb = vec![0, 0, 0, 255, 255, 255, 128, 128, 128, 128, 128, 128];
        let image = ImageSource {
            width: 2,
            height: 2,
            rgb: &rgb,
            alpha: &[],
            two_colour: None,
            window: None,
        };

        let distinct = |interpolate: bool| {
            let mut canvas = Canvas::new(16, 16, PixelFormat::Rgb8, Color::WHITE);
            let mut draw = ImageDraw::new(image, over(16.0));
            draw.interpolate = interpolate;
            draw_image(
                &mut canvas,
                &draw,
                &mut Pyramid::new(),
                &mut FillScratch::new(),
            );
            let mut values: Vec<u8> = canvas.data.iter().step_by(3).copied().collect();
            values.sort_unstable();
            values.dedup();
            values.len()
        };

        assert_eq!(distinct(false), 3, "nearest keeps the three source values");
        assert!(
            distinct(true) > 20,
            "bilinear fills in between them: {} values",
            distinct(true)
        );
    }

    /// A 1:1 draw is byte-preserving under *either* filter: the sample centres
    /// line up, so all the weight lands on one tap.
    #[test]
    fn bilinear_at_one_to_one_is_still_byte_preserving() {
        let rgb = ramp(8);
        let image = ImageSource {
            width: 8,
            height: 8,
            rgb: &rgb,
            alpha: &[],
            two_colour: None,
            window: None,
        };
        let mut canvas = Canvas::new(8, 8, PixelFormat::Rgb8, Color::WHITE);
        let mut draw = ImageDraw::new(image, over(8.0));
        draw.interpolate = true;
        draw_image(
            &mut canvas,
            &draw,
            &mut Pyramid::new(),
            &mut FillScratch::new(),
        );

        for y in 0..8u32 {
            for x in 0..8u32 {
                let want = Color::rgb((x * 8) as u8, (y * 8) as u8, 0);
                assert_eq!(canvas.pixel(x, y), Some(want), "at {x},{y}");
            }
        }
    }

    /// Four identical samples must weigh out to that sample, not to one level
    /// below it — which is what the `+ 32768` rounding is for.
    #[test]
    fn bilinear_weights_sum_to_one() {
        let rgb = vec![255u8; 4 * 3];
        let image = ImageSource {
            width: 2,
            height: 2,
            rgb: &rgb,
            alpha: &[],
            two_colour: None,
            window: None,
        };
        for step in 0..16 {
            let u = 0.5 / 16.0 + f64::from(step) / 16.0;
            let sample = bilinear(&image, u, u).expect("on the image");
            assert_eq!(sample, (255, 255, 255, 255), "at u = {u}");
        }
    }

    /// A horizontal ramp: `steps` samples one row tall, rising by `rise` a
    /// sample.
    fn gradient(steps: u32, rise: u8) -> Vec<u8> {
        let mut rgb = Vec::new();
        for x in 0..steps {
            let value = (x as u8).wrapping_mul(rise);
            rgb.extend_from_slice(&[value, value, value]);
        }
        rgb
    }

    /// The largest jump between neighbouring values.
    fn max_step(values: &[u8]) -> u8 {
        values
            .windows(2)
            .map(|pair| pair[1].abs_diff(pair[0]))
            .max()
            .unwrap_or(0)
    }

    /// Milestone 3: an upscaled gradient has no stair-steps.
    ///
    /// "No stair-steps" is a measurement, not an impression: the largest jump
    /// between neighbouring output pixels. Sixteen device pixels per source
    /// sample, so a filter that interpolates cannot step by more than one
    /// sixteenth of the source's own step, and one that does not must step by
    /// the whole of it.
    #[test]
    fn magnifying_a_gradient_leaves_no_stair_steps() {
        let rgb = gradient(8, 32);
        let image = ImageSource {
            width: 8,
            height: 1,
            rgb: &rgb,
            alpha: &[],
            two_colour: None,
            window: None,
        };

        let row = |interpolate: bool| {
            let mut canvas = Canvas::new(128, 1, PixelFormat::Rgb8, Color::WHITE);
            let mut draw = ImageDraw::new(
                image,
                Transform {
                    a: 128.0,
                    b: 0.0,
                    c: 0.0,
                    d: -1.0,
                    e: 0.0,
                    f: 1.0,
                },
            );
            draw.interpolate = interpolate;
            draw_image(
                &mut canvas,
                &draw,
                &mut Pyramid::new(),
                &mut FillScratch::new(),
            );
            canvas.data.iter().step_by(3).copied().collect::<Vec<u8>>()
        };

        assert_eq!(
            max_step(&row(false)),
            32,
            "nearest steps by the source's whole step: that is the stair"
        );
        assert_eq!(
            max_step(&row(true)),
            2,
            "bilinear steps by the source step over the magnification, 32/16"
        );
    }

    /// Milestone 3: a 2:1 downscale keeps its midtones.
    ///
    /// A one-sample checkerboard halved is the case that makes the point: its
    /// mean is 127.5 and every pixel of it is 0 or 255, so a sampler that
    /// takes one tap returns a page of whichever phase its grid landed on and
    /// reports the mean of *that*.
    #[test]
    fn a_two_to_one_downscale_keeps_its_midtones() {
        let mut rgb = Vec::new();
        for y in 0..16u32 {
            for x in 0..16u32 {
                let value = if (x + y) % 2 == 0 { 0 } else { 255 };
                rgb.extend_from_slice(&[value, value, value]);
            }
        }
        let image = ImageSource {
            width: 16,
            height: 16,
            rgb: &rgb,
            alpha: &[],
            two_colour: None,
            window: None,
        };

        // 16 samples into 8 device pixels: exactly 2:1, which the pyramid
        // leaves alone and the area filter averages two samples per pixel.
        let sampling = sampling_for(&image, &over(8.0), false);
        assert_eq!(sampling.filter, Filter::Area);
        assert_eq!(sampling.halvings, (0, 0), "2:1 needs no reduction");
        assert_eq!(
            sampling.footprint,
            (2 * ONE, 2 * ONE),
            "and each destination pixel covers exactly two samples"
        );

        let mut canvas = Canvas::new(8, 8, PixelFormat::Rgb8, Color::WHITE);
        draw_image(
            &mut canvas,
            &ImageDraw::new(image, over(8.0)),
            &mut Pyramid::new(),
            &mut FillScratch::new(),
        );
        let values: Vec<u8> = canvas.data.iter().step_by(3).copied().collect();

        let mean = values.iter().map(|v| u32::from(*v)).sum::<u32>() / values.len() as u32;
        assert_eq!(mean, 128, "the midtone survives: mean {mean}");
        let (low, high) = (
            *values.iter().min().expect("pixels"),
            *values.iter().max().expect("pixels"),
        );
        assert_eq!(
            (low, high),
            (128, 128),
            "and every pixel is that midtone rather than one phase of the \
             checkerboard: {low}..{high}"
        );
    }

    /// The flag is about magnification. On the way down there is no sharpness
    /// to preserve, only samples to drop, so the policy does not consult it.
    #[test]
    fn a_downscale_ignores_the_interpolate_flag() {
        let rgb = gradient(16, 16);
        let image = ImageSource {
            width: 16,
            height: 1,
            rgb: &rgb,
            alpha: &[],
            two_colour: None,
            window: None,
        };
        let squash = Transform {
            a: 8.0,
            b: 0.0,
            c: 0.0,
            d: -1.0,
            e: 0.0,
            f: 1.0,
        };
        assert_eq!(sampling_for(&image, &squash, false).filter, Filter::Area);
        assert_eq!(sampling_for(&image, &squash, true).filter, Filter::Area);
    }

    /// A one-sample checkerboard: the pattern that aliases worst, because its
    /// period is exactly two samples.
    fn checkerboard(size: u32) -> Vec<u8> {
        let mut rgb = Vec::new();
        for y in 0..size {
            for x in 0..size {
                let value = if (x + y) % 2 == 0 { 0 } else { 255 };
                rgb.extend_from_slice(&[value, value, value]);
            }
        }
        rgb
    }

    /// Milestone 4: a 16:1 downscale of a fine checkerboard is flat grey
    /// rather than moiré.
    ///
    /// Four taps cover two samples. At 16:1 the four land on samples eight
    /// apart, and eight is a whole number of periods of this board, so every
    /// tap of every pixel reads the *same phase* — bilinear alone returns a
    /// flat black or a flat white page, not an average of anything. The test
    /// asserts that directly as well as asserting the area filter's answer, so
    /// it cannot pass by the two behaving alike.
    ///
    /// 16:1 is below [`PYRAMID_ABOVE`], so this integrates the destination
    /// pixel's true sixteen-by-sixteen source rectangle rather than reducing
    /// first. That is the path every real file takes.
    #[test]
    fn a_sixteen_to_one_downscale_is_flat_grey_rather_than_moire() {
        let rgb = checkerboard(256);
        let image = ImageSource {
            width: 256,
            height: 256,
            rgb: &rgb,
            alpha: &[],
            two_colour: None,
            window: None,
        };

        let sampling = sampling_for(&image, &over(16.0), false);
        assert_eq!(sampling.filter, Filter::Area);
        assert_eq!(
            sampling.halvings,
            (0, 0),
            "16:1 is well inside the exact path and reduces nothing"
        );
        assert_eq!(sampling.footprint, (16 * ONE, 16 * ONE));

        // What four taps on the unreduced image would have produced.
        let unreduced: Vec<u8> = (0..16)
            .map(|px| {
                let u = (f64::from(px) + 0.5) / 16.0;
                bilinear(&image, u, u).expect("on the image").0
            })
            .collect();
        assert!(
            unreduced.iter().all(|v| *v == unreduced[0]),
            "bilinear alone reads one phase of the board: {unreduced:?}"
        );

        let mut pyramid = Pyramid::new();
        let mut canvas = Canvas::new(16, 16, PixelFormat::Rgb8, Color::WHITE);
        draw_image(
            &mut canvas,
            &ImageDraw::new(image, over(16.0)),
            &mut pyramid,
            &mut FillScratch::new(),
        );
        let values: Vec<u8> = canvas.data.iter().step_by(3).copied().collect();

        let mean = values.iter().map(|v| u32::from(*v)).sum::<u32>() / values.len() as u32;
        assert_eq!(mean, 128, "the mean is the board's own: {mean}");
        let variance = values
            .iter()
            .map(|v| i64::from(*v) - i64::from(mean as u8))
            .map(|d| d * d)
            .sum::<i64>()
            / values.len() as i64;
        assert_eq!(variance, 0, "and it is flat: variance {variance}");
    }

    /// The levels are the caller's: built into the pyramid handed in, left
    /// there, and reused by the next draw rather than rebuilt.
    #[test]
    fn pyramid_levels_are_handed_back_and_reused() {
        let rgb = checkerboard(512);
        let image = ImageSource {
            width: 512,
            height: 512,
            rgb: &rgb,
            alpha: &[],
            two_colour: None,
            window: None,
        };

        let mut pyramid = Pyramid::new();
        let mut first = Canvas::new(2, 2, PixelFormat::Rgb8, Color::WHITE);
        draw_image(
            &mut first,
            &ImageDraw::new(image, over(2.0)),
            &mut pyramid,
            &mut FillScratch::new(),
        );

        // 512 samples into 2 pixels is 256:1, which is past
        // `PYRAMID_ABOVE` and so the one shape of draw that still reduces:
        // six halvings per axis bring it to `PYRAMID_TARGET`, and the axes are
        // halved one at a time.
        assert_eq!(pyramid.levels(), 12, "six halvings of x, then six of y");
        assert_eq!(pyramid.level_size(0), Some((256, 512)));
        assert_eq!(pyramid.level_size(11), Some((8, 8)));

        let mut second = Canvas::new(2, 2, PixelFormat::Rgb8, Color::WHITE);
        draw_image(
            &mut second,
            &ImageDraw::new(image, over(2.0)),
            &mut pyramid,
            &mut FillScratch::new(),
        );
        assert_eq!(first.data, second.data, "the reused levels are the same");
        assert_eq!(pyramid.levels(), 12, "and nothing was rebuilt");

        // A pyramid offered a different image rebuilds rather than sampling
        // another picture's levels.
        let other = checkerboard(256);
        let smaller = ImageSource {
            width: 256,
            height: 256,
            rgb: &other,
            alpha: &[],
            two_colour: None,
            window: None,
        };
        // Also past `PYRAMID_ABOVE`, and deliberately so: below it a draw
        // reduces nothing, and a pyramid with no levels would prove nothing
        // about rebuilding.
        let mut third = Canvas::new(1, 1, PixelFormat::Rgb8, Color::WHITE);
        draw_image(
            &mut third,
            &ImageDraw::new(smaller, over(1.0)),
            &mut pyramid,
            &mut FillScratch::new(),
        );
        assert_eq!(
            pyramid.level_size(0),
            Some((128, 256)),
            "rebuilt for 256 x 256"
        );
    }

    /// The level count is decided by integer shifts, so it cannot come out
    /// one different on a target whose `log2` rounds the other way.
    #[test]
    fn the_level_count_is_decided_on_integers() {
        // Nothing below `PYRAMID_ABOVE` reduces at all: the destination
        // pixel's true source rectangle is integrated, which is what makes the
        // answer independent of the device scale and is why the `dpi` relation
        // holds on strip-built scans.
        assert_eq!(halvings(PYRAMID_ABOVE * ONE, 4096), 0, "exactly 128:1");
        assert_eq!(halvings(16 * ONE, 4096), 0, "and everything under it");
        // Past it, the old policy: halve until the residual is within
        // `PYRAMID_TARGET`. Reducing to within four rather than two is worth
        // the extra level and reducing further is not — measured against an
        // exact box average of a 512-square noise source, mean error per
        // channel out of 255 at a 3:1 downscale was 9.88 reducing to within 2,
        // 0.25 within 4, and 0.25 within 8.
        assert_eq!(
            halvings(PYRAMID_ABOVE * ONE + 1, 4096),
            6,
            "a hair past the threshold reduces all the way to within four"
        );
        assert_eq!(halvings(256 * ONE, 4096), 6, "and so does twice it");
        assert_eq!(halvings(512 * ONE, 4096), 7);
        // An axis of one sample cannot be halved, whatever the ratio claims.
        assert_eq!(halvings(u64::MAX, 1), 0);
        // And a nonsense transform is capped rather than looping.
        assert_eq!(halvings(u64::MAX, 65536), MAX_HALVINGS);
    }

    /// An odd extent has a last sample with no partner. It must survive, not
    /// be dropped and not be paired with the sample on the other edge.
    #[test]
    fn halving_an_odd_extent_keeps_the_last_sample() {
        let rgb = vec![10, 10, 10, 20, 20, 20, 30, 30, 30];
        let image = ImageSource {
            width: 3,
            height: 1,
            rgb: &rgb,
            alpha: &[],
            two_colour: None,
            window: None,
        };
        let level = halve(&image, (1, 0));
        assert_eq!((level.width, level.height), (2, 1));
        // (10 + 20 + 1) / 2 = 15, and the odd one averages with itself.
        assert_eq!(&level.rgb[..], &[15, 15, 15, 30, 30, 30]);
    }

    #[test]
    fn a_transform_and_its_inverse_compose_to_the_identity() {
        let t = Transform {
            a: 2.0,
            b: 0.5,
            c: -1.0,
            d: 3.0,
            e: 7.0,
            f: -2.0,
        };
        let inverse = t.invert().expect("invertible");
        let (x, y) = t.apply(1.25, -0.5);
        let (back_x, back_y) = inverse.apply(x, y);
        assert!((back_x - 1.25).abs() < 1e-12, "{back_x}");
        assert!((back_y + 0.5).abs() < 1e-12, "{back_y}");
        assert!(Transform::IDENTITY.invert().is_some());
    }

    /// The per-pixel sampler: every pixel mapped through the inverse on its
    /// own, the area filter's products accumulated in `u128` and `split`
    /// through `floor`. What the planned walk is held to.
    mod per_pixel {
        use super::super::*;

        pub(super) fn walk(
            draw: &ImageDraw<'_>,
            pyramid: &mut Pyramid,
            scratch: &mut FillScratch,
            rect: (i32, i32, u32, u32),
            mut emit: impl FnMut(i32, i32, Color, u8, u8),
        ) {
            let image = &draw.image;
            if image.width == 0 || image.height == 0 {
                return;
            }
            let Some(inverse) = draw.unit_to_device.invert() else {
                return;
            };
            let Some((x0, x1, y0, y1)) = device_bounds(&draw.unit_to_device, rect) else {
                return;
            };
            if x1 <= x0 || y1 <= y0 {
                return;
            }
            let shape = edge_shape(draw, scratch, (x0, y0, x1.abs_diff(x0), y1.abs_diff(y0)));
            let sampling = sampling_for(image, &draw.unit_to_device, draw.interpolate);
            let filter = sampling.filter;
            let image = &pyramid.reduce(image, sampling.halvings);
            for py in y0..y1 {
                if draw.stop.is_some_and(|stop| stop()) {
                    return;
                }
                for px in x0..x1 {
                    let covered = shape.at(px, py);
                    if covered == 0 {
                        continue;
                    }
                    let (u, v) = inverse.apply(f64::from(px) + 0.5, f64::from(py) + 0.5);
                    let (u, v) = (u.clamp(0.0, 1.0), v.clamp(0.0, 1.0));
                    let sample = match filter {
                        Filter::Nearest => nearest(image, u, v),
                        Filter::Bilinear => bilinear(image, u, v),
                        Filter::Area => area(image, u, v, sampling.footprint),
                    };
                    let Some((r, g, b, own)) = sample else {
                        continue;
                    };
                    if own == 0 {
                        continue;
                    }
                    emit(
                        px,
                        py,
                        draw.tint.unwrap_or(Color::rgb(r, g, b)),
                        covered,
                        own,
                    );
                }
            }
        }

        fn bilinear(image: &ImageSource<'_>, u: f64, v: f64) -> Option<(u8, u8, u8, u8)> {
            let x = u * f64::from(image.width) - 0.5;
            let y = (1.0 - v) * f64::from(image.height) - 0.5;

            let (x0, wx) = split(x);
            let (y0, wy) = split(y);
            let last_x = i64::from(image.width) - 1;
            let last_y = i64::from(image.height) - 1;
            let cx0 = x0.clamp(0, last_x) as u32;
            let cx1 = (x0 + 1).clamp(0, last_x) as u32;
            let cy0 = y0.clamp(0, last_y) as u32;
            let cy1 = (y0 + 1).clamp(0, last_y) as u32;

            let anchor = image.texel(cx0, cy0)?;
            let t10 = image.texel(cx1, cy0).unwrap_or(anchor);
            let t01 = image.texel(cx0, cy1).unwrap_or(anchor);
            let t11 = image.texel(cx1, cy1).unwrap_or(anchor);

            let w00 = (256 - wx) * (256 - wy);
            let w10 = wx * (256 - wy);
            let w01 = (256 - wx) * wy;
            let w11 = wx * wy;
            let mix = |a: u8, b: u8, c: u8, d: u8| {
                let total = u32::from(a) * w00
                    + u32::from(b) * w10
                    + u32::from(c) * w01
                    + u32::from(d) * w11;
                ((total + 32768) >> 16) as u8
            };

            Some((
                mix(anchor.0, t10.0, t01.0, t11.0),
                mix(anchor.1, t10.1, t01.1, t11.1),
                mix(anchor.2, t10.2, t01.2, t11.2),
                mix(anchor.3, t10.3, t01.3, t11.3),
            ))
        }

        fn area(
            image: &ImageSource<'_>,
            u: f64,
            v: f64,
            footprint: (u64, u64),
        ) -> Option<(u8, u8, u8, u8)> {
            let cx = (u * f64::from(image.width) * 65536.0) as i64;
            let cy = ((1.0 - v) * f64::from(image.height) * 65536.0) as i64;
            let (hx, hy) = (
                (footprint.0 / 2).min(i64::MAX as u64) as i64,
                (footprint.1 / 2).min(i64::MAX as u64) as i64,
            );
            let (x0, x1) = (cx - hx, cx + hx);
            let (y0, y1) = (cy - hy, cy + hy);

            let mut sum = [0u128; 4];
            let mut weight = 0u128;
            let mut any = false;
            for ty in span(y0, y1, image.height) {
                let oy = overlap(y0, y1, ty);
                if oy == 0 {
                    continue;
                }
                for tx in span(x0, x1, image.width) {
                    let ox = overlap(x0, x1, tx);
                    if ox == 0 {
                        continue;
                    }
                    let Some(texel) = image.texel(tx, ty) else {
                        continue;
                    };
                    any = true;
                    let w = u128::from(ox) * u128::from(oy);
                    weight += w;
                    sum[0] += w * u128::from(texel.0);
                    sum[1] += w * u128::from(texel.1);
                    sum[2] += w * u128::from(texel.2);
                    sum[3] += w * u128::from(texel.3);
                }
            }
            if !any || weight == 0 {
                return None;
            }
            let half = weight / 2;
            let mix = |channel: u128| ((channel + half) / weight) as u8;
            Some((mix(sum[0]), mix(sum[1]), mix(sum[2]), mix(sum[3])))
        }

        fn split(value: f64) -> (i64, u32) {
            let floor = value.floor();
            let index = floor as i64;
            let fraction = ((value - floor) * 256.0) as u32;
            (index, fraction.min(255))
        }
    }

    /// One pixel a walk hands its consumer: where, its colour, the quad's
    /// coverage and the sample's own alpha.
    type Emitted = (i32, i32, Color, u8, u8);

    /// Everything the planned walk hands its consumer, in order.
    fn planned_walk(draw: &ImageDraw<'_>, rect: (i32, i32, u32, u32)) -> Vec<Emitted> {
        let mut out = Vec::new();
        walk(
            draw,
            &mut Pyramid::new(),
            &mut FillScratch::new(),
            rect,
            |px, py, color, covered, own| {
                out.push((px, py, color, covered, own));
            },
        );
        out
    }

    /// Everything the per-pixel walk hands its consumer, in order.
    fn per_pixel_walk(draw: &ImageDraw<'_>, rect: (i32, i32, u32, u32)) -> Vec<Emitted> {
        let mut out = Vec::new();
        per_pixel::walk(
            draw,
            &mut Pyramid::new(),
            &mut FillScratch::new(),
            rect,
            |px, py, color, covered, own| {
                out.push((px, py, color, covered, own));
            },
        );
        out
    }

    /// Bytes from a linear congruential generator: no structure a filter could
    /// get right by accident, and the same bytes on every target.
    fn noise(count: usize, seed: u32) -> Vec<u8> {
        let mut state = seed.wrapping_mul(2_654_435_761).wrapping_add(17);
        (0..count)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 24) as u8
            })
            .collect()
    }

    /// Holds one draw's two walks to each other, naming the first pixel they
    /// part at.
    fn assert_same_walk(draw: &ImageDraw<'_>, rect: (i32, i32, u32, u32), what: &str) -> usize {
        let planned = planned_walk(draw, rect);
        let per_pixel = per_pixel_walk(draw, rect);
        if planned != per_pixel {
            let at = planned
                .iter()
                .zip(&per_pixel)
                .position(|(a, b)| a != b)
                .unwrap_or(planned.len().min(per_pixel.len()));
            panic!(
                "{what}: the planned walk parts from the per-pixel one at pixel {at} of \
                 {} against {}: {:?} against {:?}",
                planned.len(),
                per_pixel.len(),
                planned.get(at),
                per_pixel.get(at),
            );
        }
        planned.len()
    }

    /// **The planned walk is the per-pixel walk, pixel for pixel.**
    ///
    /// Axis-aligned draws from 8:1 magnified to past 128:1 minified, where the
    /// pyramid reduces first and, on an axis of three samples, stops at a
    /// level of two; every flip of the two axes; offsets off the pixel grid;
    /// a buffer shorter than its dimensions, sample alpha and alpha cut short;
    /// both values of `/Interpolate`, a hard edge and a tint; and canvases
    /// standing at three places in the page frame, so a plan made over a
    /// region is the page's plan there (ruling 5). Rotated, sheared and
    /// quarter-turned draws take the per-pixel map, and it is held to the walk
    /// it replaced as well: its `u64` sums and its floor-free split.
    #[test]
    fn the_column_plan_is_the_per_pixel_sampler() {
        let rects = [(0, 0, 64, 64), (5, -3, 23, 17), (-20, 11, 50, 30)];
        let images: [(u32, u32); 7] = [
            (1, 1),
            (2, 3),
            (7, 5),
            (16, 9),
            (33, 21),
            (600, 3),
            (3, 600),
        ];
        // Device pixels a sample, along x and along y.
        let scales = [
            (8.0, 8.0),
            (3.7, 0.73),
            (1.0, 1.0),
            (0.5, 1.0 / 3.0),
            (1.0 / 7.3, 2.25),
            (1.0 / 150.0, 1.0 / 150.0),
        ];
        PLANNED_LINES.with(|lines| lines.set(0));
        let (mut cases, mut pixels) = (0usize, 0usize);
        for (index, &(width, height)) in images.iter().enumerate() {
            let samples = (width * height) as usize;
            let rgb = noise(samples * 3, index as u32);
            let alpha = noise(samples, 100 + index as u32);
            let sources = [
                ImageSource {
                    width,
                    height,
                    rgb: &rgb,
                    alpha: &[],
                    two_colour: None,
                    window: None,
                },
                // Two thirds of the samples the dimensions claim.
                ImageSource {
                    width,
                    height,
                    rgb: &rgb[..rgb.len() * 2 / 3],
                    alpha: &[],
                    two_colour: None,
                    window: None,
                },
                // Alpha for the first half of them, opaque after.
                ImageSource {
                    width,
                    height,
                    rgb: &rgb,
                    alpha: &alpha[..alpha.len().div_ceil(2)],
                    two_colour: None,
                    window: None,
                },
            ];
            for source in sources {
                for &(sx, sy) in &scales {
                    let (w, h) = (f64::from(width) * sx, f64::from(height) * sy);
                    for (flip_x, flip_y) in
                        [(false, false), (true, false), (false, true), (true, true)]
                    {
                        let (left, bottom) = (3.37, 2.61);
                        let placement = Transform {
                            a: if flip_x { -w } else { w },
                            b: 0.0,
                            c: 0.0,
                            d: if flip_y { h } else { -h },
                            e: if flip_x { left + w } else { left },
                            f: if flip_y { bottom } else { bottom + h },
                        };
                        for interpolate in [false, true] {
                            let mut draw = ImageDraw::new(source, placement);
                            draw.interpolate = interpolate;
                            draw.edge_bits = if cases % 5 != 0 { 8 } else { 0 };
                            if cases % 10 == 5 {
                                draw.edge_rule = HardEdge::Centre;
                            }
                            if cases % 7 == 0 {
                                draw.tint = Some(Color::rgb(200, 30, 90));
                            }
                            let rect = rects[cases % rects.len()];
                            pixels += assert_same_walk(
                                &draw,
                                rect,
                                &format!(
                                    "{width}x{height} ({} rgb bytes, {} alpha) at {sx}x{sy}, \
                                     flipped {flip_x}/{flip_y}, interpolate {interpolate}, \
                                     over {rect:?}",
                                    source.rgb.len(),
                                    source.alpha.len()
                                ),
                            );
                            cases += 1;
                        }
                    }
                }
            }
        }
        let planned = PLANNED_LINES.with(core::cell::Cell::get);

        // The draws no plan serves: a rotation, a shear and a quarter turn.
        let turned = [
            Transform {
                a: 20.0,
                b: 11.0,
                c: -11.0,
                d: 20.0,
                e: 17.3,
                f: 4.6,
            },
            Transform {
                a: 30.0,
                b: 0.0,
                c: 7.5,
                d: -20.0,
                e: 5.2,
                f: 31.7,
            },
            Transform {
                a: 0.0,
                b: 23.5,
                c: 31.25,
                d: 0.0,
                e: 2.5,
                f: 3.75,
            },
        ];
        for (index, &(width, height)) in images.iter().enumerate() {
            let samples = (width * height) as usize;
            let rgb = noise(samples * 3, 200 + index as u32);
            let alpha = noise(samples, 300 + index as u32);
            let source = ImageSource {
                width,
                height,
                rgb: &rgb,
                alpha: &alpha,
                two_colour: None,
                window: None,
            };
            for placement in turned {
                for interpolate in [false, true] {
                    let mut draw = ImageDraw::new(source, placement);
                    draw.interpolate = interpolate;
                    pixels += assert_same_walk(
                        &draw,
                        (0, 0, 64, 64),
                        &format!("{width}x{height} under {placement:?}, interpolate {interpolate}"),
                    );
                    cases += 1;
                }
            }
        }
        assert_eq!(
            PLANNED_LINES.with(core::cell::Cell::get),
            planned,
            "a turned draw plans nothing"
        );
        assert!(
            planned > 10_000,
            "the axis-aligned draws planned: {planned} lines"
        );
        assert!(
            pixels > 200_000,
            "{cases} draws compared {pixels} pixels, which is too few to say anything"
        );
    }

    /// A draw works out its weights once a column and once a row, rather than
    /// once a pixel: the plan is what each pixel sums.
    ///
    /// Two draws, one per filter the plan serves — the area filter on a 3:1
    /// minification, bilinear on a magnification that asked for it — each
    /// planning exactly as many lines as it has columns and rows, which its
    /// pixels outnumber many times over.
    #[test]
    fn a_draw_plans_its_weights_once_per_column_and_row() {
        let rgb = noise(40 * 30 * 3, 9);
        let image = ImageSource {
            width: 40,
            height: 30,
            rgb: &rgb,
            alpha: &[],
            two_colour: None,
            window: None,
        };
        let rect = (0, 0, 128, 128);
        for (w, h, interpolate) in [(13.3, 10.0, false), (97.0, 71.3, true)] {
            let placement = Transform {
                a: w,
                b: 0.0,
                c: 0.0,
                d: -h,
                e: 2.6,
                f: 3.1 + h,
            };
            let mut draw = ImageDraw::new(image, placement);
            draw.interpolate = interpolate;
            let filter = sampling_for(&image, &placement, interpolate).filter;
            let (x0, x1, y0, y1) = device_bounds(&placement, rect).expect("on the canvas");
            let lines = x1.abs_diff(x0) as usize + y1.abs_diff(y0) as usize;

            PLANNED_LINES.with(|planned| planned.set(0));
            let mut pixels = 0usize;
            walk(
                &draw,
                &mut Pyramid::new(),
                &mut FillScratch::new(),
                rect,
                |_, _, _, _, _| pixels += 1,
            );
            let planned = PLANNED_LINES.with(core::cell::Cell::get);

            assert_eq!(
                planned, lines,
                "{filter:?}: one plan a column and one a row"
            );
            assert!(
                pixels > 4 * planned,
                "{filter:?}: the pixels, {pixels}, outnumber the plans, {planned}"
            );
        }
    }

    /// **An image draw rounds no float per pixel through `f64::round`.**
    ///
    /// Each draw here, direct and through a run, at an alpha of one and of
    /// 0.6, with and without a clip, composites hundreds of pixels; their
    /// levels come from `Opacity`, and the counter of `f64::round` calls on
    /// the composition path may see at most one a draw. It sees none. A fill
    /// on the same canvas rounds its own alpha, once, which is what shows the
    /// counter counting.
    #[test]
    fn an_image_draw_rounds_no_float_per_pixel() {
        use crate::canvas::LEVEL_ROUNDS;
        let rounds = || LEVEL_ROUNDS.with(core::cell::Cell::get);

        let rgb = noise(12 * 9 * 3, 4);
        let alpha = noise(12 * 9, 5);
        let image = ImageSource {
            width: 12,
            height: 9,
            rgb: &rgb,
            alpha: &alpha,
            two_colour: None,
            window: None,
        };
        let placement = Transform {
            a: 37.3,
            b: 0.0,
            c: 0.0,
            d: -25.1,
            e: 3.4,
            f: 30.2,
        };
        let mut square = Path::new();
        square.move_to(10.5, 4.25);
        square.line_to(30.75, 4.25);
        square.line_to(30.75, 22.5);
        square.line_to(10.5, 22.5);
        square.close();
        let mask = crate::fill::fill(&square, FillRule::NonZero, 0, 0, 64, 64, 0.2, None);

        let mut canvas = Canvas::new(64, 64, crate::PixelFormat::Rgb8, Color::WHITE);
        let start = rounds();
        let mut draws = 0;
        for state_alpha in [1.0, 0.6] {
            for clip in [None, Some(&mask)] {
                let mut draw = ImageDraw::new(image, placement);
                draw.alpha = state_alpha;
                draw.clip = clip;
                draw_image(
                    &mut canvas,
                    &draw,
                    &mut Pyramid::new(),
                    &mut FillScratch::new(),
                );
                let mut run = Fragments::new(0, 0, 64, 64);
                accumulate_image(
                    &mut run,
                    &draw,
                    &mut Pyramid::new(),
                    (0, 0, 64, 64),
                    &mut FillScratch::new(),
                );
                run.composite_region_inked(
                    &mut canvas,
                    (0, 0, 64, 64),
                    state_alpha,
                    BlendMode::Normal,
                    clip,
                    None,
                    None,
                );
                draws += 2;
            }
        }
        let made = rounds() - start;
        assert!(
            made <= draws,
            "{draws} image draws made {made} f64::round calls on the composition path"
        );
        assert_eq!(made, 0, "and none of them makes one at all");
        let painted = canvas
            .data
            .chunks_exact(3)
            .filter(|pixel| *pixel != [255, 255, 255])
            .count();
        assert!(painted > 800, "the draws painted {painted} pixels");

        canvas.fill_mask(&mask, Color::BLACK, 0.5);
        assert_eq!(rounds() - start, 1, "a fill rounds its own alpha, once");
    }

    /// A two-colour image's RGB expansion: each sample's chosen colour as
    /// three bytes and its alpha as one.
    fn expanded(two: &TwoColour<'_>) -> (Vec<u8>, Vec<u8>) {
        let mut rgb = Vec::with_capacity(two.samples.len() * 3);
        let mut alpha = Vec::with_capacity(two.samples.len());
        for &sample in two.samples {
            let [r, g, b, a] = two.colours[usize::from(sample != 0)];
            rgb.extend_from_slice(&[r, g, b]);
            alpha.push(a);
        }
        (rgb, alpha)
    }

    /// A two-colour image of `width` by `height` over `samples`.
    fn two_colour_source<'a>(
        width: u32,
        height: u32,
        samples: &'a [u8],
        colours: [[u8; 4]; 2],
    ) -> ImageSource<'a> {
        ImageSource {
            width,
            height,
            rgb: &[],
            alpha: &[],
            two_colour: Some(TwoColour { samples, colours }),
            window: None,
        }
    }

    /// The pixels a draw paints, drawn directly onto a canvas standing at
    /// `rect` in the page frame and through a run composited onto another.
    fn painted(draw: &ImageDraw<'_>, rect: (i32, i32, u32, u32)) -> (Vec<u8>, Vec<u8>) {
        let (x0, y0, width, height) = rect;
        let mut direct = Canvas::new(width, height, PixelFormat::Rgb8, Color::rgb(30, 160, 220))
            .at_origin(x0, y0);
        draw_image(
            &mut direct,
            draw,
            &mut Pyramid::new(),
            &mut FillScratch::new(),
        );
        let mut run = Fragments::new(x0, y0, width, height);
        accumulate_image(
            &mut run,
            draw,
            &mut Pyramid::new(),
            rect,
            &mut FillScratch::new(),
        );
        let mut held = Canvas::new(width, height, PixelFormat::Rgb8, Color::rgb(30, 160, 220))
            .at_origin(x0, y0);
        run.composite_region_inked(
            &mut held,
            (x0, y0, x0 + width as i32, y0 + height as i32),
            draw.alpha,
            draw.blend,
            None,
            None,
            None,
        );
        (direct.data, held.data)
    }

    /// **A two-colour image draws as its RGB expansion, pixel for pixel.**
    ///
    /// Images from one sample to 600 by 3, their samples noise, long runs,
    /// bytes other than 0 and 1, all of the first colour and all of the
    /// second, some cut short of their dimensions; colour pairs opaque and
    /// translucent, black and white both ways round, a stencil's opaque paint
    /// and transparent rest drawn under a tint, a colour key folded into one
    /// colour's alpha, and one colour twice. Each is drawn from 8:1 magnified
    /// to past 128:1 minified — where the pyramid reduces and the image is
    /// sampled through RGB levels — on every flip of its axes, at offsets off
    /// the pixel grid, with both values of `/Interpolate`, hard and soft
    /// edges, a translucent draw alpha and canvases at three places in the
    /// page frame; and rotated, sheared and quarter-turned on the per-pixel
    /// map. Every pixel the walk hands its consumer is the expansion's, and so
    /// is every byte drawn directly and through a run. The three samplers,
    /// asked at a lattice of points and footprints, answer alike too:
    /// nearest, which the policy never picks, bilinear and the area filter.
    #[test]
    fn a_two_colour_image_draws_as_its_rgb_expansion() {
        let rects = [(0, 0, 64, 64), (5, -3, 23, 17), (-20, 11, 50, 30)];
        let images: [(u32, u32); 7] = [
            (1, 1),
            (2, 3),
            (7, 5),
            (16, 9),
            (33, 21),
            (600, 3),
            (3, 600),
        ];
        let scales = [
            (8.0, 8.0),
            (3.7, 0.73),
            (1.0, 1.0),
            (0.5, 1.0 / 3.0),
            (1.0 / 7.3, 2.25),
            (1.0 / 150.0, 1.0 / 150.0),
        ];
        let pairs: [[[u8; 4]; 2]; 7] = [
            [[0, 0, 0, 255], [255, 255, 255, 255]],
            [[255, 255, 255, 255], [0, 0, 0, 255]],
            [[31, 58, 147, 255], [242, 201, 76, 255]],
            // A stencil: its paint opaque, the rest transparent.
            [[0, 0, 0, 255], [0, 0, 0, 0]],
            // A colour key that selected the samples of zero.
            [[17, 200, 3, 0], [250, 9, 128, 255]],
            [[10, 20, 30, 77], [200, 150, 100, 191]],
            [[90, 91, 92, 255], [90, 91, 92, 255]],
        ];
        // The samples of each image, five ways.
        let patterns = |count: usize, seed: u32| -> [Vec<u8>; 5] {
            let raw = noise(count, seed);
            [
                raw.iter().map(|byte| byte & 1).collect(),
                (0..count).map(|i| u8::from((i / 5) % 3 == 0)).collect(),
                raw,
                vec![0; count],
                vec![1; count],
            ]
        };

        let (mut cases, mut pixels) = (0usize, 0usize);
        let mut compare = |draw_two: &ImageDraw<'_>,
                           draw_rgb: &ImageDraw<'_>,
                           rect: (i32, i32, u32, u32),
                           what: &str| {
            let (two, rgb) = (planned_walk(draw_two, rect), planned_walk(draw_rgb, rect));
            if two != rgb {
                let at = two
                    .iter()
                    .zip(&rgb)
                    .position(|(a, b)| a != b)
                    .unwrap_or(two.len().min(rgb.len()));
                panic!(
                    "{what}: the two-colour walk parts from its expansion at pixel {at} of {} \
                     against {}: {:?} against {:?}",
                    two.len(),
                    rgb.len(),
                    two.get(at),
                    rgb.get(at),
                );
            }
            if cases % 11 == 0 {
                assert!(
                    painted(draw_two, rect) == painted(draw_rgb, rect),
                    "{what}: the canvases part"
                );
            }
            pixels += two.len();
            cases += 1;
        };

        for (index, &(width, height)) in images.iter().enumerate() {
            let count = (width * height) as usize;
            let patterns = patterns(count, 40 + index as u32);
            for (scale, &(sx, sy)) in scales.iter().enumerate() {
                let (w, h) = (f64::from(width) * sx, f64::from(height) * sy);
                for (flip_x, flip_y) in [(false, false), (true, false), (false, true), (true, true)]
                {
                    let (left, bottom) = (3.37, 2.61);
                    let placement = Transform {
                        a: if flip_x { -w } else { w },
                        b: 0.0,
                        c: 0.0,
                        d: if flip_y { h } else { -h },
                        e: if flip_x { left + w } else { left },
                        f: if flip_y { bottom } else { bottom + h },
                    };
                    for interpolate in [false, true] {
                        for (pair, colours) in pairs.iter().enumerate() {
                            let base = &patterns[(scale + pair + index) % patterns.len()];
                            // Every fourth draw holds two thirds of its samples.
                            let samples = if (pair + scale) % 4 == 0 {
                                &base[..base.len() * 2 / 3]
                            } else {
                                &base[..]
                            };
                            let two = TwoColour {
                                samples,
                                colours: *colours,
                            };
                            let (rgb, alpha) = expanded(&two);
                            let mut draw_two = ImageDraw::new(
                                two_colour_source(width, height, samples, *colours),
                                placement,
                            );
                            draw_two.interpolate = interpolate;
                            draw_two.edge_bits = if (pair + scale) % 5 != 0 { 8 } else { 0 };
                            if pair == 3 || (pair + scale) % 3 == 0 {
                                draw_two.tint = Some(Color::rgb(200, 30, 90));
                            }
                            if (pair + index) % 6 == 0 {
                                draw_two.alpha = 0.37;
                            }
                            let draw_rgb = ImageDraw {
                                image: ImageSource {
                                    width,
                                    height,
                                    rgb: &rgb,
                                    alpha: &alpha,
                                    two_colour: None,
                                    window: None,
                                },
                                ..draw_two
                            };
                            let rect = rects[(pair + scale + index) % rects.len()];
                            compare(
                                &draw_two,
                                &draw_rgb,
                                rect,
                                &format!(
                                    "{width}x{height}, {} samples, colours {colours:?} at \
                                     {sx}x{sy}, flipped {flip_x}/{flip_y}, interpolate \
                                     {interpolate}, over {rect:?}",
                                    samples.len()
                                ),
                            );
                        }
                    }
                }
            }
        }

        // The draws no plan serves: a rotation, a shear and a quarter turn.
        let turned = [
            Transform {
                a: 20.0,
                b: 11.0,
                c: -11.0,
                d: 20.0,
                e: 17.3,
                f: 4.6,
            },
            Transform {
                a: 30.0,
                b: 0.0,
                c: 7.5,
                d: -20.0,
                e: 5.2,
                f: 31.7,
            },
            Transform {
                a: 0.0,
                b: 23.5,
                c: 31.25,
                d: 0.0,
                e: 2.5,
                f: 3.75,
            },
        ];
        for (index, &(width, height)) in images.iter().enumerate() {
            let count = (width * height) as usize;
            let patterns = patterns(count, 80 + index as u32);
            for placement in turned {
                for interpolate in [false, true] {
                    for (pair, colours) in pairs.iter().enumerate() {
                        let samples = &patterns[(pair + index) % patterns.len()];
                        let two = TwoColour {
                            samples,
                            colours: *colours,
                        };
                        let (rgb, alpha) = expanded(&two);
                        let mut draw_two = ImageDraw::new(
                            two_colour_source(width, height, samples, *colours),
                            placement,
                        );
                        draw_two.interpolate = interpolate;
                        if pair == 3 {
                            draw_two.tint = Some(Color::rgb(12, 140, 60));
                        }
                        let draw_rgb = ImageDraw {
                            image: ImageSource {
                                width,
                                height,
                                rgb: &rgb,
                                alpha: &alpha,
                                two_colour: None,
                                window: None,
                            },
                            ..draw_two
                        };
                        compare(
                            &draw_two,
                            &draw_rgb,
                            (0, 0, 64, 64),
                            &format!(
                                "{width}x{height}, colours {colours:?} under {placement:?}, \
                                 interpolate {interpolate}"
                            ),
                        );
                    }
                }
            }
        }
        assert!(
            pixels > 250_000,
            "{cases} draws compared {pixels} pixels, which is too few to say anything"
        );

        // The samplers themselves, at a lattice of points.
        let footprints = [
            (ONE / 4, ONE / 4),
            (ONE, ONE),
            (3 * ONE / 2, 5 * ONE / 2),
            (40 * ONE, 7 * ONE),
            (128 * ONE, 128 * ONE),
        ];
        let mut scratch = (Vec::new(), Vec::new());
        let mut asked = 0usize;
        for (index, &(width, height)) in images.iter().enumerate() {
            let count = (width * height) as usize;
            for samples in &patterns(count, 120 + index as u32) {
                for colours in pairs {
                    let two = TwoColour { samples, colours };
                    let (rgb, alpha) = expanded(&two);
                    let source = two_colour_source(width, height, samples, colours);
                    let expansion = ImageSource {
                        width,
                        height,
                        rgb: &rgb,
                        alpha: &alpha,
                        two_colour: None,
                        window: None,
                    };
                    for step_u in 0..=16 {
                        for step_v in 0..=16 {
                            let (u, v) = (f64::from(step_u) / 16.0, f64::from(step_v) / 16.0);
                            assert_eq!(
                                nearest(&source, u, v),
                                nearest(&expansion, u, v),
                                "nearest, {width}x{height} at {u},{v}"
                            );
                            assert_eq!(
                                bilinear(&source, u, v),
                                bilinear(&expansion, u, v),
                                "bilinear, {width}x{height} at {u},{v}"
                            );
                            for footprint in footprints {
                                assert_eq!(
                                    area(&source, u, v, footprint, &mut scratch),
                                    area(&expansion, u, v, footprint, &mut scratch),
                                    "area over {footprint:?}, {width}x{height} at {u},{v}"
                                );
                            }
                            asked += 1;
                        }
                    }
                }
            }
        }
        assert!(asked > 10_000, "the samplers were asked at {asked} points");
    }

    /// **A two-colour image sums one channel a tap.**
    ///
    /// Three draws of a 40 by 30 two-colour image — the area filter's plan on
    /// a 3:1 minification, bilinear's on a magnification that asked for it,
    /// and the area filter mapped pixel by pixel under a rotation — each sum
    /// thousands of taps, and the channel sums those taps went into number
    /// exactly as many: one channel a tap. Its RGB expansion, drawn the same
    /// three ways, reads the same taps into four channels each. A draw past
    /// 128:1 reduces through RGB levels, and sums four channels a tap there:
    /// the one channel is summed only where nothing was reduced.
    #[test]
    fn a_coverage_image_sums_one_channel() {
        let samples: Vec<u8> = noise(40 * 30, 3).iter().map(|byte| byte & 1).collect();
        let colours = [[0, 0, 0, 255], [255, 255, 255, 255]];
        let two = TwoColour {
            samples: &samples,
            colours,
        };
        let (rgb, alpha) = expanded(&two);
        let coverage = two_colour_source(40, 30, &samples, colours);
        let expansion = ImageSource {
            width: 40,
            height: 30,
            rgb: &rgb,
            alpha: &alpha,
            two_colour: None,
            window: None,
        };
        let count = |source: ImageSource<'_>, placement: Transform, interpolate: bool| {
            SUMMED.with(|summed| summed.set((0, 0)));
            let mut draw = ImageDraw::new(source, placement);
            draw.interpolate = interpolate;
            let mut pixels = 0usize;
            walk(
                &draw,
                &mut Pyramid::new(),
                &mut FillScratch::new(),
                (0, 0, 128, 128),
                |_, _, _, _, _| pixels += 1,
            );
            let (taps, sums) = SUMMED.with(core::cell::Cell::get);
            (taps, sums, pixels)
        };

        let draws = [
            (
                "the area filter's plan",
                Transform {
                    a: 13.3,
                    b: 0.0,
                    c: 0.0,
                    d: -10.0,
                    e: 2.6,
                    f: 13.1,
                },
                false,
            ),
            (
                "bilinear's plan",
                Transform {
                    a: 97.0,
                    b: 0.0,
                    c: 0.0,
                    d: -71.3,
                    e: 2.6,
                    f: 74.4,
                },
                true,
            ),
            (
                "the area filter, pixel by pixel",
                Transform {
                    a: 30.0,
                    b: 12.0,
                    c: -14.0,
                    d: 26.0,
                    e: 40.3,
                    f: 9.6,
                },
                false,
            ),
        ];
        for (what, placement, interpolate) in draws {
            let (taps, sums, pixels) = count(coverage, placement, interpolate);
            assert!(
                taps > 1000 && pixels > 100,
                "{what}: {taps} taps over {pixels} pixels"
            );
            assert_eq!(sums, taps, "{what}: one channel a tap");
            let (expanded_taps, expanded_sums, expanded_pixels) =
                count(expansion, placement, interpolate);
            assert_eq!(
                (expanded_taps, expanded_pixels),
                (taps, pixels),
                "{what}: the expansion reads the same taps"
            );
            assert_eq!(expanded_sums, 4 * taps, "{what}: four channels a tap");
        }

        // 40 samples into a quarter of a pixel and 30 into a fifth: past
        // 128:1 on both axes, so the pyramid reduces.
        let squashed = Transform {
            a: 0.25,
            b: 0.0,
            c: 0.0,
            d: -0.2,
            e: 1.3,
            f: 1.7,
        };
        assert_ne!(sampling_for(&coverage, &squashed, false).halvings, (0, 0));
        let (taps, sums, _) = count(coverage, squashed, false);
        assert!(taps > 0, "the reduced draw read its level");
        assert_eq!(sums, 4 * taps, "a reduced draw sums its RGB level");
    }

    /// **A resample's lines are the rationals themselves**: each line's taps
    /// sum to the extent, every sample's taps over all the lines sum to a
    /// whole sample, and no tap is empty — down, up and one to one, at ratios
    /// that are whole and that are not.
    #[test]
    fn a_resample_plans_each_line_exactly() {
        for (lines, extent) in [
            (1u32, 1u32),
            (1, 9),
            (9, 1),
            (3, 7),
            (7, 3),
            (5, 5),
            (2, 9),
            (9, 2),
            (64, 48),
            (32, 48),
            (72, 48),
        ] {
            let plan = Lines::exact(lines, extent);
            let mut per_sample = vec![0u64; extent as usize];
            for line in 0..lines as usize {
                let taps = plan.line(line);
                assert!(!taps.is_empty(), "{lines} over {extent}: line {line}");
                let sum: u64 = taps.iter().map(|&(_, w)| u64::from(w)).sum();
                assert_eq!(sum, u64::from(extent), "{lines} over {extent}: line {line}");
                for &(index, weight) in taps {
                    assert!(weight > 0 && weight <= lines);
                    per_sample[index as usize] += u64::from(weight);
                }
            }
            assert!(
                per_sample.iter().all(|&w| w == u64::from(lines)),
                "{lines} over {extent}: every sample covered once in all"
            );
        }
    }

    /// The area average, written from its definition with rationals in
    /// `u128`: the overlap of `[x W, (x + 1) W)` with `[i w, (i + 1) w)` on
    /// each axis, and every channel's weighted sum rounded half up.
    fn rational_average(
        source: &[u8],
        alpha: &[u8],
        (sw, sh): (u32, u32),
        (dw, dh): (u32, u32),
        (x, y): (u32, u32),
    ) -> (u8, u8, u8, u8) {
        let overlaps = |d: u32, dn: u32, sn: u32| -> Vec<(u32, u128)> {
            let (lo, hi) = (
                u128::from(d) * u128::from(sn),
                u128::from(d + 1) * u128::from(sn),
            );
            (0..sn)
                .filter_map(|i| {
                    let (start, end) = (
                        u128::from(i) * u128::from(dn),
                        u128::from(i + 1) * u128::from(dn),
                    );
                    let covered = hi.min(end).saturating_sub(lo.max(start));
                    (covered > 0).then_some((i, covered))
                })
                .collect()
        };
        let (columns, rows) = (overlaps(x, dw, sw), overlaps(y, dh, sh));
        let mut sum = [0u128; 4];
        let mut weight = 0u128;
        for &(ty, oy) in &rows {
            for &(tx, ox) in &columns {
                let at = (ty * sw + tx) as usize;
                let w = ox * oy;
                weight += w;
                for (c, total) in sum.iter_mut().enumerate().take(3) {
                    *total += w * u128::from(source[at * 3 + c]);
                }
                sum[3] += w * u128::from(alpha.get(at).copied().unwrap_or(255));
            }
        }
        let mix = |channel: u128| ((channel * 2 + weight) / (weight * 2)) as u8;
        (mix(sum[0]), mix(sum[1]), mix(sum[2]), mix(sum[3]))
    }

    /// **A resample is the area average of what each pixel covers**, every
    /// pixel of every channel, alpha included, at whole and fractional ratios
    /// down and up and with the two axes going opposite ways; and one to one
    /// it is the image.
    #[test]
    fn a_resample_is_the_area_average_of_what_each_pixel_covers() {
        let (sw, sh) = (12u32, 9u32);
        let rgb = noise((sw * sh * 3) as usize, 5);
        let alpha = noise((sw * sh) as usize, 11);
        for with_alpha in [false, true] {
            let image = ImageSource {
                width: sw,
                height: sh,
                rgb: &rgb,
                alpha: if with_alpha { &alpha } else { &[] },
                two_colour: None,
                window: None,
            };
            for (dw, dh) in [
                (6, 3),
                (4, 9),
                (8, 6),
                (24, 18),
                (18, 27),
                (5, 7),
                (7, 5),
                (3, 20),
                (12, 9),
            ] {
                let mut seen = 0u32;
                let drawn = resample(&image, dw, dh, |x, y, got| {
                    seen += 1;
                    let want = rational_average(&rgb, image.alpha, (sw, sh), (dw, dh), (x, y));
                    assert_eq!(got, want, "{sw}x{sh} to {dw}x{dh} at ({x}, {y})");
                });
                assert!(drawn);
                assert_eq!(seen, dw * dh, "every pixel of {dw}x{dh} once");
            }
            let mut index = 0u32;
            resample(&image, sw, sh, |x, y, got| {
                assert_eq!(index, y * sw + x, "row by row");
                assert_eq!(Some(got), image.texel(x, y), "one to one is the image");
                index += 1;
            });
        }
    }

    /// A resample with no pixels, of an image with none, or of one whose sums
    /// a `u64` cannot hold, emits nothing and says so — the last before a
    /// sample is read, so a buffer that does not hold such an image is never
    /// reached.
    #[test]
    fn a_resample_refuses_what_it_cannot_sum() {
        let rgb = [1u8, 2, 3];
        let one = ImageSource {
            width: 1,
            height: 1,
            rgb: &rgb,
            alpha: &[],
            two_colour: None,
            window: None,
        };
        let mut emitted = 0;
        assert!(!resample(&one, 0, 4, |_, _, _| emitted += 1));
        assert!(!resample(&one, 4, 0, |_, _, _| emitted += 1));
        let empty = ImageSource { width: 0, ..one };
        assert!(!resample(&empty, 4, 4, |_, _, _| emitted += 1));
        let vast = ImageSource {
            width: u32::MAX,
            height: u32::MAX,
            rgb: &[],
            alpha: &[],
            two_colour: None,
            window: None,
        };
        assert!(!resample(&vast, 1, 1, |_, _, _| emitted += 1));
        assert_eq!(emitted, 0);
        // At the edge: a sum of 255 W H that fits is resampled.
        assert!(resample(&one, 3, 2, |_, _, got| {
            emitted += 1;
            assert_eq!(got, (1, 2, 3, 255));
        }));
        assert_eq!(emitted, 6);
    }

    // ---- windows (`ImageSource::window`) ------------------------------------

    /// `image`'s rectangle `window`, three bytes a sample and one of alpha
    /// where it has alpha, row by row: the buffers a windowed source holds.
    fn cropped(image: &ImageSource<'_>, window: SourceWindow) -> (Vec<u8>, Vec<u8>) {
        let (mut rgb, mut alpha) = (Vec::new(), Vec::new());
        for y in window.y..window.y + window.height {
            for x in window.x..window.x + window.width {
                let at = (y * image.width + x) as usize;
                rgb.extend_from_slice(&image.rgb[at * 3..at * 3 + 3]);
                if !image.alpha.is_empty() {
                    alpha.push(image.alpha[at]);
                }
            }
        }
        (rgb, alpha)
    }

    /// **A sample outside a window reads the nearest inside it**, and one
    /// inside reads its own place in the window's rows; a read off the image
    /// is still none, and an empty window holds nothing.
    #[test]
    fn a_window_reads_its_own_samples_and_the_nearest_outside_itself() {
        let (width, height) = (9u32, 7u32);
        let rgb: Vec<u8> = (0..width * height * 3).map(|i| (i % 251) as u8).collect();
        let whole = ImageSource {
            width,
            height,
            rgb: &rgb,
            alpha: &[],
            two_colour: None,
            window: None,
        };
        let window = SourceWindow {
            x: 2,
            y: 3,
            width: 4,
            height: 2,
        };
        let (held, _) = cropped(&whole, window);
        let windowed = ImageSource {
            rgb: &held,
            window: Some(window),
            ..whole
        };
        for y in 0..height {
            for x in 0..width {
                let nearest = (x.clamp(2, 5), y.clamp(3, 4));
                assert_eq!(
                    windowed.texel(x, y),
                    whole.texel(nearest.0, nearest.1),
                    "({x}, {y}) reads ({}, {})",
                    nearest.0,
                    nearest.1
                );
            }
        }
        assert_eq!(windowed.texel(width, 0), None, "off the image");
        assert_eq!(windowed.texel(0, height), None, "off the image");
        let empty = ImageSource {
            rgb: &[],
            window: Some(SourceWindow {
                x: 3,
                y: 3,
                width: 0,
                height: 2,
            }),
            ..whole
        };
        assert_eq!(empty.texel(3, 3), None, "an empty window holds nothing");
        // A window whose corner is past the image is read from the samples
        // it was given, never past them.
        let past = ImageSource {
            rgb: &held,
            window: Some(SourceWindow {
                x: u32::MAX - 1,
                y: 0,
                width: 4,
                height: 2,
            }),
            ..whole
        };
        assert_eq!(past.texel(0, 0), Some((held[0], held[1], held[2], 255)));
    }

    /// **A window halves to the whole image's level over it**: halving a
    /// window whose edges lie on even samples, or on the image's own far edge,
    /// gives the very samples halving the whole image gives over the window's
    /// halves, alpha included; and the level's window is those halves.
    #[test]
    fn a_window_halves_to_the_whole_images_level_over_it() {
        let (width, height) = (23u32, 17u32);
        let rgb = noise((width * height * 3) as usize, 7);
        let alpha = noise((width * height) as usize, 8);
        let whole = ImageSource {
            width,
            height,
            rgb: &rgb,
            alpha: &alpha,
            two_colour: None,
            window: None,
        };
        for window in [
            SourceWindow {
                x: 4,
                y: 2,
                width: 10,
                height: 8,
            },
            // To the image's odd far corner, whose last sample has no partner.
            SourceWindow {
                x: 14,
                y: 8,
                width: 9,
                height: 9,
            },
            SourceWindow {
                x: 0,
                y: 0,
                width: 2,
                height: 2,
            },
        ] {
            let (held_rgb, held_alpha) = cropped(&whole, window);
            let windowed = ImageSource {
                rgb: &held_rgb,
                alpha: &held_alpha,
                window: Some(window),
                ..whole
            };
            for halvings in [(1, 0), (0, 1)] {
                let all = halve(&whole, halvings);
                let part = halve(&windowed, halvings);
                assert_eq!(
                    (part.width, part.height),
                    (all.width, all.height),
                    "{window:?} {halvings:?}: the level's own size"
                );
                let held = part.window.expect("a window's level is a window");
                let want = if halvings.0 == 1 {
                    SourceWindow {
                        x: window.x / 2,
                        width: (window.x + window.width).div_ceil(2) - window.x / 2,
                        ..window
                    }
                } else {
                    SourceWindow {
                        y: window.y / 2,
                        height: (window.y + window.height).div_ceil(2) - window.y / 2,
                        ..window
                    }
                };
                assert_eq!(held, want, "{window:?} {halvings:?}: the window halved");
                for y in held.y..held.y + held.height {
                    for x in held.x..held.x + held.width {
                        assert_eq!(
                            part.source().texel(x, y),
                            all.source().texel(x, y),
                            "{window:?} {halvings:?}: ({x}, {y})"
                        );
                    }
                }
            }
        }
    }

    /// Where a draw's pixel reads samples: its centre mapped into the image,
    /// in the image's own sample coordinates, columns from the left and rows
    /// from the top.
    fn read_at(t: &Transform, image: &ImageSource<'_>, px: i32, py: i32) -> (f64, f64) {
        let (u, v) = centre(&t.invert().expect("invertible"), px, py);
        (
            u * f64::from(image.width),
            (1.0 - v) * f64::from(image.height),
        )
    }

    /// **A window draws what the whole image draws wherever it holds what a
    /// pixel reads** (`ImageSource::window`): every pixel the whole image's
    /// walk hands on, a windowed walk hands on with the same coverage and the
    /// same alpha — the window reads its nearest sample where it holds none,
    /// so a run accumulating it holds what it would of the whole image — and
    /// with the same colour wherever the pixel's reach, its filter's
    /// footprint and two samples beside it at the level it samples, lies in
    /// the window or runs off the image's own edge with it. Magnified, at one
    /// sample a pixel, minified within the exact area filter and past 128:1,
    /// where the pyramid reduces a window whose edges lie on its levels'
    /// samples; flipped and at fractional offsets; with and without
    /// `/Interpolate`.
    #[test]
    fn a_window_draws_the_whole_image_wherever_it_holds_the_samples_a_pixel_reads() {
        let mut compared = 0usize;
        let small: &[SourceWindow] = &[
            SourceWindow {
                x: 13,
                y: 9,
                width: 30,
                height: 22,
            },
            SourceWindow {
                x: 0,
                y: 0,
                width: 20,
                height: 47,
            },
            SourceWindow {
                x: 40,
                y: 20,
                width: 21,
                height: 27,
            },
            SourceWindow {
                x: 7,
                y: 30,
                width: 54,
                height: 11,
            },
        ];
        // On the level six halvings make of the 2 560 columns: edges on
        // multiples of 64.
        let large: &[SourceWindow] = &[
            SourceWindow {
                x: 640,
                y: 0,
                width: 1280,
                height: 20,
            },
            SourceWindow {
                x: 0,
                y: 4,
                width: 1536,
                height: 12,
            },
        ];
        let cases: [((u32, u32), &[(f64, f64)], &[SourceWindow]); 2] = [
            (
                (61, 47),
                &[(3.0, 2.5), (1.0, 1.0), (0.59, 0.71), (0.37, 0.43)],
                small,
            ),
            // Past 128:1 across, 2 560 samples into sixteen pixels, and 2:1
            // down.
            ((2560, 20), &[(16.0 / 2560.0, 0.5)], large),
        ];
        for (index, ((width, height), scales, windows)) in cases.into_iter().enumerate() {
            let rgb = noise((width * height * 3) as usize, 40 + index as u32);
            let whole = ImageSource {
                width,
                height,
                rgb: &rgb,
                alpha: &[],
                two_colour: None,
                window: None,
            };
            for &(sx, sy) in scales {
                let (w, h) = (f64::from(width) * sx, f64::from(height) * sy);
                for (flip, interpolate) in [(false, false), (true, false), (false, true)] {
                    let t = Transform {
                        a: if flip { -w } else { w },
                        b: 0.0,
                        c: 0.0,
                        d: -h,
                        e: if flip { 2.37 + w } else { 2.37 },
                        f: 1.61 + h,
                    };
                    let sampling = sampling_for(&whole, &t, interpolate);
                    if index == 1 {
                        assert_eq!(sampling.halvings, (6, 0), "the pyramid reduces");
                    }
                    let reach = |footprint: u64, halvings: u32| {
                        (footprint.div_ceil(1 << 16) as f64 + 2.0) * f64::from(1u32 << halvings)
                    };
                    let (rx, ry) = (
                        reach(sampling.footprint.0, sampling.halvings.0),
                        reach(sampling.footprint.1, sampling.halvings.1),
                    );
                    let rect = (0, 0, (w.abs() + 6.0) as u32, (h.abs() + 6.0) as u32);
                    let mut draw = ImageDraw::new(whole, t);
                    draw.interpolate = interpolate;
                    let all = planned_walk(&draw, rect);
                    for &window in windows {
                        let (held, _) = cropped(&whole, window);
                        let windowed = ImageSource {
                            rgb: &held,
                            window: Some(window),
                            ..whole
                        };
                        let what = format!(
                            "{width}x{height} at {sx}x{sy}, flipped {flip}, interpolate \
                             {interpolate}, {window:?}"
                        );
                        let part = planned_walk(
                            &ImageDraw {
                                image: windowed,
                                ..draw
                            },
                            rect,
                        );
                        assert_eq!(all.len(), part.len(), "{what}: the pixels handed on");
                        let inside = |at: f64, r: f64, start: u32, size: u32, extent: u32| {
                            (start == 0 || at - r >= f64::from(start))
                                && (start + size == extent || at + r <= f64::from(start + size))
                        };
                        for (a, b) in all.iter().zip(&part) {
                            assert_eq!(
                                (a.0, a.1, a.3, a.4),
                                (b.0, b.1, b.3, b.4),
                                "{what}: where, the coverage and the alpha"
                            );
                            let (x, y) = read_at(&t, &whole, a.0, a.1);
                            if inside(x, rx, window.x, window.width, width)
                                && inside(y, ry, window.y, window.height, height)
                            {
                                assert_eq!(a.2, b.2, "{what}: the colour at ({}, {})", a.0, a.1);
                                compared += 1;
                            }
                        }
                    }
                }
            }
        }
        assert!(compared > 10_000, "{compared} pixels compared");
    }
}
