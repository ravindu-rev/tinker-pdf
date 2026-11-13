//! Scanline filling with anti-aliasing.
//!
//! Coverage is computed by sampling each pixel row at a fixed number of
//! sub-scanlines and accumulating in **integer** arithmetic. That is a
//! deliberate choice over analytic exact-area coverage: the sample count is
//! fixed, the accumulator is integral, and no platform's floating-point
//! rounding or fused multiply-add can change the result. Ruling 4 makes
//! bit-identical output across linux, windows, macOS and wasm a contract, and
//! this is what pays for it.
//!
//! The cost is that an edge's *vertical* position quantizes to a sixteenth of
//! a pixel, and that is not invisible. Measured 15 September 2026 against the
//! true area of a half-plane, over a sweep of angles and sub-pixel offsets:
//! the worst pixel is **14.7 levels of 255**, at one degree off horizontal,
//! and the error stays between 8 and 10 levels from two degrees all the way to
//! forty-five, falling to 1.1 at eighty-nine. A sixteenth of a pixel of
//! position is about an eighth of that in coverage either side, which is where
//! those numbers come from.
//!
//! **This file used to say that difference was "below one 8-bit level for
//! every angle".** It never was, and the sentence made the trade look free
//! when the trade is real: a fixed grid buys ruling 4's bit-identical output on
//! every target, which a floating-point area integrator cannot, and it is paid
//! for in anti-aliasing quality at shallow angles. Raising `SAMPLES` is the
//! knob; it costs time linearly and would move every fingerprint in the tree,
//! so it is a decision and not a tweak.

use core::ops::Range;

use crate::geom::{flatten_flat, FillRule, Path, Point, MAX_VERBS};

/// Sub-scanlines per pixel row. Sixteen gives 4-bit vertical resolution,
/// combined with exact horizontal spans for 8-bit total coverage.
const SAMPLES: i32 = 16;

/// The sub-scanline of a row that holds its pixels' centres, `row + ½`
/// exactly, and so the lane of a cell [`HardEdge::Centre`] reads.
const CENTRE_LANE: usize = (SAMPLES / 2) as usize;

/// The unit of a pixel, of its 256 across, that holds its centre.
const CENTRE_UNIT: u8 = 128;

/// Rows of the sweep between one cancellation check and the next.
///
/// Asking every row would put a branch in the hottest loop in the engine.
/// Asking every sixteenth amortises it to a sixteenth of that while keeping
/// the promise the render layer's documentation makes, which is scanline-band
/// granularity rather than per-pixel: sixteen rows of a 300 dpi US Letter page
/// is 40 800 pixels, tens of microseconds between one answer and the next.
///
/// A constant, deliberately. **The number cannot change a pixel** — the
/// predicate decides only *whether the sweep continues*, never what a
/// continued row computes — so it is free to be tuned without re-baselining
/// anything, and the seven determinism fingerprints are indifferent to it.
/// The count is of *iterations*, not of row indices, so the first row of every
/// fill is checked whatever part of the region the shape starts in.
const STOP_EVERY: usize = 16;

/// A coverage mask over a rectangular region, and the value it reads outside
/// that region.
#[derive(Clone, Debug)]
pub struct Mask {
    /// Left edge in device pixels.
    pub x0: i32,
    /// Top edge in device pixels.
    pub y0: i32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Coverage, 0 to 255, row-major.
    pub data: Vec<u8>,
    /// What the mask reads at every device pixel outside its rectangle.
    ///
    /// Zero for every clip, every paint's coverage and every mask
    /// [`crate::Canvas::to_mask`] returns: a shape covers nothing past its
    /// own bounds. A soft mask is the one mask that reads something else
    /// there (11.6.5.2): outside the region its group painted, a luminosity
    /// mask is the luminosity of `/BC` alone, which is not zero unless `/BC`
    /// is black. Carried as one value rather than as a field of it the size
    /// of the canvas, so a mask whose backdrop is pale costs its box and not
    /// the page (`docs/design/performance.md`, P-25).
    ///
    /// Every consumer reads it: [`Mask::at`] returns it, [`Mask::overlap_at`]
    /// reaches the whole canvas when it is not zero, the products keep it,
    /// and a reader of `data` by index stays inside the rectangle.
    pub outside: u8,
}

impl Mask {
    /// An all-zero mask, reading zero outside itself too.
    #[must_use]
    pub fn empty(x0: i32, y0: i32, width: u32, height: u32) -> Mask {
        let len = (width as usize).saturating_mul(height as usize);
        Mask {
            x0,
            y0,
            width,
            height,
            data: vec![0; len],
            outside: 0,
        }
    }

    /// A mask holding one value everywhere in its rectangle, and zero outside
    /// it: a rectangle of coverage, such as the whole of a canvas `sh` paints
    /// with no clip in force.
    #[must_use]
    pub fn uniform(x0: i32, y0: i32, width: u32, height: u32, value: u8) -> Mask {
        let len = (width as usize).saturating_mul(height as usize);
        Mask {
            x0,
            y0,
            width,
            height,
            data: vec![value; len],
            outside: 0,
        }
    }

    /// The same mask, reading `outside` beyond its rectangle.
    ///
    /// What a soft mask is (11.6.5.2): the buffer its group drew, read back
    /// over the group's box, and beyond the box the luminosity of `/BC`
    /// through `/TR` — the value [`Mask::outside`] holds.
    #[must_use]
    pub fn with_outside(mut self, outside: u8) -> Mask {
        self.outside = outside;
        self
    }

    /// The canvas pixels this mask can put coverage on, clamped to a canvas of
    /// `width` by `height`, as `(x0, y0, x1, y1)` with the far edges
    /// exclusive: its own rectangle, or the whole canvas when it reads a value
    /// other than zero outside itself.
    ///
    /// A mask reading zero outside itself is zero everywhere this does not
    /// reach, so a consumer that walks the whole canvas computes the same
    /// answer as one that walks this — it just pays the page for a comma.
    /// Every consumer walks this.
    #[must_use]
    pub fn overlap(&self, width: u32, height: u32) -> (u32, u32, u32, u32) {
        self.overlap_at((0, 0), width, height)
    }

    /// [`Mask::overlap`] for a canvas standing at device pixel `origin`: the
    /// canvas pixels this mask can reach, **in the canvas's own indices**, so
    /// device pixel `(origin.0 + x, origin.1 + y)` is canvas pixel `(x, y)`.
    ///
    /// The mask stays in device pixels and the canvas says where it is, which
    /// is what keeps a tile's arithmetic in the page's frame (ruling 5). A
    /// mask whose [`Mask::outside`] is not zero reaches every pixel of every
    /// canvas, wherever its rectangle stands.
    #[must_use]
    pub fn overlap_at(&self, origin: (i32, i32), width: u32, height: u32) -> (u32, u32, u32, u32) {
        if self.outside != 0 {
            return (0, 0, width, height);
        }
        let clamp = |value: i64, limit: u32| value.clamp(0, i64::from(limit)) as u32;
        let (left, top) = (
            i64::from(self.x0) - i64::from(origin.0),
            i64::from(self.y0) - i64::from(origin.1),
        );
        let x0 = clamp(left, width);
        let y0 = clamp(top, height);
        let x1 = clamp(left + i64::from(self.width), width);
        let y1 = clamp(top + i64::from(self.height), height);
        (x0, y0, x1.max(x0), y1.max(y0))
    }

    /// Coverage at a device pixel: [`Mask::outside`] beyond the mask's
    /// rectangle.
    #[must_use]
    pub fn at(&self, x: i32, y: i32) -> u8 {
        let col = i64::from(x) - i64::from(self.x0);
        let row = i64::from(y) - i64::from(self.y0);
        if col < 0 || row < 0 || col >= i64::from(self.width) || row >= i64::from(self.height) {
            return self.outside;
        }
        let index = (row as usize) * (self.width as usize) + (col as usize);
        self.data.get(index).copied().unwrap_or(0)
    }

    /// Every pixel fully covered or fully uncovered: coverage of at least half
    /// becomes 255, anything less becomes 0.
    ///
    /// What turning anti-aliasing off means here. The threshold is on the
    /// coverage `fill` already measured rather than a second sampling rule
    /// beside it, so a hard edge lands where the soft one is half-way — the
    /// shape keeps its area rather than growing by a pixel on every side —
    /// and it is a function of one pixel's coverage alone, so a tile and the
    /// page under it still agree (ruling 5).
    ///
    /// **Two shapes sharing an edge partition the pixels along it.** A value
    /// here is `floor(units / 16)` of the 4 096 units a pixel holds, so it is
    /// at least 128 exactly when the shape holds at least 2 048 units; two
    /// shapes that split a pixel's units between them therefore split the
    /// pixel too, except at an exact half, where both take it. Neither leaves
    /// a gap, which a threshold above one half would.
    ///
    /// The price is the one every such threshold pays: a feature narrower than
    /// half a pixel can vanish where it straddles a pixel edge.
    ///
    /// [`Mask::outside`] is hardened with the rest, so the mask reads one
    /// threshold everywhere it is asked.
    pub fn harden(&mut self) {
        let hard = |value: u8| if value >= 128 { 255 } else { 0 };
        for value in &mut self.data {
            *value = hard(*value);
        }
        self.outside = hard(self.outside);
    }

    /// Every pixel's coverage reduced to `bits` bits
    /// (`docs/design/anti-aliasing.md`): 8 leaves it as it is, 1 to 7 take each
    /// to the nearest of the `2^bits` values spaced evenly from 0 to 255, and 0
    /// is [`Mask::harden`]; a number above 8 reads as 8.
    ///
    /// With `n = 2^bits − 1`, a coverage `c` takes the nearest index
    /// `q = (c·n + 127) / 255`, a half rounding up, and that index's value
    /// `(q·255 + (n − 1) / 2) / n`, in integers: `n` is odd, so `q·255 / n`
    /// is never a whole number and a half and the second division has no tie.
    /// At one bit that is `harden`'s threshold at half, and at eight the
    /// identity. A function of one pixel's coverage, so a tile and the page
    /// under it still agree (ruling 5). [`Mask::outside`] is reduced with the
    /// rest, as `harden` hardens it.
    pub fn quantise(&mut self, bits: u8) {
        match bits {
            0 => self.harden(),
            1..=7 => {
                let table = quantised(bits);
                for value in &mut self.data {
                    *value = table[usize::from(*value)];
                }
                self.outside = table[usize::from(self.outside)];
            }
            _ => {}
        }
    }

    /// Intersects with another mask, multiplying coverages.
    ///
    /// How clipping composes: a clip stack is the product of its masks, and
    /// multiplying is what makes a clipped anti-aliased edge look right rather
    /// than doubly-hard. The product reads the product of the two
    /// [`Mask::outside`] values outside its rectangle, which is:
    ///
    /// - where both read zero outside themselves, the rectangle the two have
    ///   in common, because the product is zero everywhere else by
    ///   construction — one of the two factors is outside itself there and
    ///   reads zero. A larger mask would carry the same answer at a cost
    ///   proportional to the page;
    /// - where one of them reads zero outside itself — a clip times a soft
    ///   mask — that one's own rectangle, outside which the product is zero
    ///   and inside which the other is read, beyond its own rectangle too;
    /// - where neither does, the smallest rectangle holding both, outside
    ///   which each reads its own outside value.
    #[must_use]
    pub fn intersect(&self, other: &Mask) -> Mask {
        let (x0, y0, width, height) = match (self.outside, other.outside) {
            (0, 0) => common_rect(self, other),
            (0, _) => (self.x0, self.y0, self.width, self.height),
            (_, 0) => (other.x0, other.y0, other.width, other.height),
            _ => joint_rect(self, other),
        };
        let mut out =
            Mask::empty(x0, y0, width, height).with_outside(product(self.outside, other.outside));
        for row in 0..height {
            let y = y0.saturating_add(row as i32);
            for col in 0..width {
                let x = x0.saturating_add(col as i32);
                let value = product(self.at(x, y), other.at(x, y));
                let index = (row as usize) * (width as usize) + (col as usize);
                if let Some(slot) = out.data.get_mut(index) {
                    *slot = value;
                }
            }
        }
        out
    }

    /// Multiplies `other`'s coverage into this mask, keeping this one's
    /// rectangle.
    ///
    /// The same arithmetic as [`Mask::intersect`] without the second
    /// allocation, for the caller that already knows its mask is no larger
    /// than the clip it is about to be multiplied by — which is every paint,
    /// since a paint asks for the region its path and the clip have in common
    /// before it rasterizes anything. `other` is read at every pixel of this
    /// mask's rectangle, its [`Mask::outside`] beyond its own, and a mask
    /// reading zero outside itself keeps doing so: the product is zero there.
    ///
    /// A mask that reads a value other than zero outside itself has a product
    /// that may reach past its rectangle, so it becomes [`Mask::intersect`]'s
    /// product instead.
    pub fn intersect_in_place(&mut self, other: &Mask) {
        if self.outside != 0 {
            *self = self.intersect(other);
            return;
        }
        for row in 0..self.height {
            let y = self.y0.saturating_add(row as i32);
            let base = (row as usize) * (self.width as usize);
            for col in 0..self.width {
                let x = self.x0.saturating_add(col as i32);
                let Some(slot) = self.data.get_mut(base + col as usize) else {
                    continue;
                };
                *slot = product(*slot, other.at(x, y));
            }
        }
    }
}

/// The value each of the 256 coverages takes at `bits` bits, 1 to 8
/// ([`Mask::quantise`]); a number outside reads as the nearest of them.
fn quantised(bits: u8) -> [u8; 256] {
    let n = (1u32 << bits.clamp(1, 8)) - 1;
    let mut table = [0u8; 256];
    for (coverage, slot) in (0u32..).zip(table.iter_mut()) {
        let index = (coverage * n + 127) / 255;
        // At most `(n·255 + (n − 1)/2) / n`, which is 255.
        *slot = u8::try_from((index * 255 + (n - 1) / 2) / n).unwrap_or(u8::MAX);
    }
    table
}

/// Two coverages multiplied, rounded rather than truncated so that full
/// coverage stays full.
fn product(a: u8, b: u8) -> u8 {
    ((u32::from(a) * u32::from(b) + 127) / 255) as u8
}

/// The rectangle two masks have in common, as `(x0, y0, width, height)`.
fn common_rect(a: &Mask, b: &Mask) -> (i32, i32, u32, u32) {
    let span = |x0: i32, width: u32| (i64::from(x0), i64::from(x0) + i64::from(width));
    let (ax0, ax1) = span(a.x0, a.width);
    let (bx0, bx1) = span(b.x0, b.width);
    let (ay0, ay1) = span(a.y0, a.height);
    let (by0, by1) = span(b.y0, b.height);

    let x0 = ax0.max(bx0);
    let y0 = ay0.max(by0);
    let width = (ax1.min(bx1) - x0).max(0);
    let height = (ay1.min(by1) - y0).max(0);
    // Both origins came from an `i32`, so the maximum of the two still fits.
    (x0 as i32, y0 as i32, width as u32, height as u32)
}

/// The smallest rectangle holding both masks' rectangles, as `(x0, y0, width,
/// height)`: a rectangle of no pixels holds nothing and is passed over, so two
/// of them give the first one's corner and no pixels.
fn joint_rect(a: &Mask, b: &Mask) -> (i32, i32, u32, u32) {
    let blank = |mask: &Mask| mask.width == 0 || mask.height == 0;
    match (blank(a), blank(b)) {
        (true, true) => (a.x0, a.y0, 0, 0),
        (true, false) => (b.x0, b.y0, b.width, b.height),
        (false, true) => (a.x0, a.y0, a.width, a.height),
        (false, false) => {
            let x0 = i64::from(a.x0).min(i64::from(b.x0));
            let y0 = i64::from(a.y0).min(i64::from(b.y0));
            let x1 =
                (i64::from(a.x0) + i64::from(a.width)).max(i64::from(b.x0) + i64::from(b.width));
            let y1 =
                (i64::from(a.y0) + i64::from(a.height)).max(i64::from(b.y0) + i64::from(b.height));
            // Both origins came from an `i32`, so the minimum of the two fits;
            // the extent is clamped to what a `u32` holds.
            let extent = |lo: i64, hi: i64| (hi - lo).clamp(0, i64::from(u32::MAX)) as u32;
            (x0 as i32, y0 as i32, extent(x0, x1), extent(y0, y1))
        }
    }
}

/// Sub-steps an edge's x carries below the 1/256 the spans are measured in,
/// as a bit count.
///
/// **An edge's slope needs far more precision than its position does**, because
/// the slope is multiplied by the height of the edge and the position is not.
/// Held at 1/256 per sub-scanline — which is what this file did until ruling
/// 5's tile guard went in — a slope is stored as `trunc(slope × 16)`, so:
///
/// - every slope shallower than **one pixel of x per sixteen of y** truncated
///   to zero and the edge came out vertical, and
/// - every steeper edge kept a slope error of up to a whole 1/256 unit per
///   sub-scanline, which over an edge `h` pixels tall walks its lower end
///   **`h/16` pixels** off the line the path states.
///
/// Neither is small. `analytic_coverage.rs`'s shallow parallelogram is two
/// pixels wide, 256 tall and drifts fourteen across — 0.875 of a unit per
/// sub-scanline, truncated to nothing — so it rasterised as a straight
/// vertical bar whose lower end was the whole fourteen pixels wrong.
///
/// Sixteen extra bits leave a slope error under `2^-16` of a 1/256 unit per
/// sub-scanline; over the 12 672 sub-scanlines of a page-tall edge that
/// accumulates to a fifth of one 1/256 unit, which is under a thousandth of a
/// byte level.
const SUBSTEP_BITS: u32 = 16;

/// [`SUBSTEP_BITS`] as a multiplier.
///
/// The reduction back to 1/256 is what makes a tile equal the page under it:
/// **add half a step, then shift right**, which is `floor` of a value moved by
/// a multiple of the step and therefore commutes with translating the whole
/// shape by a whole number of pixels — a tile's frame and the page's quantise
/// a crossing to the same unit. It is also the *nearest* unit rather than the
/// one below, which is the unbiased choice: a pixel's coverage is linear in
/// the crossing, so the nearest unit is the one whose coverage is nearest the
/// coverage the geometry states.
const SUBSTEP: i64 = 1 << SUBSTEP_BITS;

/// One edge, in sub-scanline space.
struct Edge {
    /// Sub-scanline of the upper end, inclusive.
    top: i32,
    /// Sub-scanline of the lower end, exclusive.
    bottom: i32,
    /// x at `top`, in 1/256 pixel scaled by [`SUBSTEP`].
    x: i64,
    /// Change in x per sub-scanline, in the same units as [`Edge::x`].
    dxdy: i64,
    /// +1 or -1, for the non-zero winding rule.
    winding: i32,
    /// Whether no crossing of the edge can saturate ([`unsaturating`]), so
    /// that each sub-scanline's crossing is the one above it plus `dxdy`.
    plain: bool,
}

/// The end of a list: no edge, no block of list heads, no later crossing.
const NONE: u32 = u32::MAX;

/// Rasterizes a path into a coverage mask clipped to the given region.
///
/// The region bounds the work: a path far larger than the page costs only the
/// pixels that could be seen.
///
/// `stop` is asked once every [`STOP_EVERY`] rows, starting with the first,
/// and the sweep ends as soon as it answers `true`. What comes back then is
/// the **partial** mask — the rows already swept, and zero below them — rather
/// than an empty one, because a half-drawn shape is a better progressive frame
/// than a missing one and the caller is discarding the canvas anyway if the
/// stop was real. `None` is the whole of the previous behaviour: no predicate
/// is called and no row is skipped.
///
/// **Nothing is ordered.** A row of the region is a line of [`Cell`]s, one per
/// pixel, each with a lane per sub-scanline; every edge reaching the row
/// deposits its crossings into the lanes, and the row is walked left to right
/// carrying each lane's winding — a prefix sum — and turned into bytes as it
/// goes. No edge is sorted by its top and no sub-scanline's crossings by `x`:
/// a pixel's coverage on a sub-scanline is the count of its 1/256 units the
/// rule calls inside, which the crossings decide whatever order they come in.
/// The bytes are the sorted sweep's, which `fill_reference.rs` keeps and
/// compares with this byte for byte; `docs/design/rasterizer-accumulation.md`
/// is the design.
///
/// The fill works in a [`FillScratch`] of its own, dropped when it returns; a
/// caller filling many shapes keeps one and fills through [`fill_with`].
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn fill(
    path: &Path,
    rule: FillRule,
    x0: i32,
    y0: i32,
    width: u32,
    height: u32,
    tolerance: f64,
    stop: Option<&dyn Fn() -> bool>,
) -> Mask {
    fill_with(
        &mut FillScratch::new(),
        path,
        rule,
        x0,
        y0,
        width,
        height,
        tolerance,
        stop,
    )
}

/// [`fill`], working in `scratch`: the same mask, and from the second fill on
/// no allocation but the mask's while no fill needs more than an earlier one
/// did.
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn fill_with(
    scratch: &mut FillScratch,
    path: &Path,
    rule: FillRule,
    x0: i32,
    y0: i32,
    width: u32,
    height: u32,
    tolerance: f64,
    stop: Option<&dyn Fn() -> bool>,
) -> Mask {
    let mut mask = Mask::empty(x0, y0, width, height);
    if width == 0 || height == 0 {
        return mask;
    }
    #[cfg(test)]
    let held = scratch.capacities();
    scratch.sweep_into(&mut mask, path, rule, tolerance, stop, Measure::Coverage);
    #[cfg(test)]
    count::allocated(scratch.grown_from(&held));
    mask
}

/// The rule a fill with no anti-aliasing paints each pixel whole or leaves it
/// empty by (`docs/design/anti-aliasing.md`).
///
/// [`HardEdge::Half`] reads the 8-bit coverage a fill measures; the other
/// rules read the shape itself, which a coverage does not hold, so they are
/// the filler's own and are asked of it ([`fill_hard_with`],
/// [`fill_outline_hard`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum HardEdge {
    /// Whole where the shape covers at least half the pixel: the coverage
    /// through [`Mask::harden`]. The default, and what no anti-aliasing has
    /// always meant here.
    #[default]
    Half,
    /// Whole where the shape holds the pixel's centre, read on the filler's
    /// own grid: the centre of pixel `(col, row)` is unit `256·col + 128` of
    /// sub-scanline `16·row + 8`, and the pixel is whole when the winding the
    /// fill rule reads there calls it inside. An edge exactly through a
    /// centre takes it on its left and top and gives it up on its right and
    /// bottom, as a span does, so two shapes sharing an edge split its
    /// pixels.
    Centre,
    /// Whole where the shape — the region the fill rule calls inside and its
    /// outline, every crossing across taken to the nearest 1/256 — meets the
    /// pixel's open square, `(col, col + 1) × (row, row + 1)`. On the
    /// filler's grid that is a unit of the pixel inside on one of its sixteen
    /// sub-scanlines, or an edge of the outline whose part inside the open
    /// row, its ends across to the nearest 1/256, reaches into the pixel's
    /// open interval of units, `(256·col, 256·col + 256)` — which is what a
    /// sliver lying wholly between two sub-scanlines is made of. A shape that
    /// only touches a pixel's edge or corner leaves it empty, and a subpath of
    /// one point paints the pixel under it.
    AnyPart,
}

/// What a sweep makes of each pixel of its region.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Measure {
    /// Its 8-bit coverage over the sixteen sub-scanlines.
    Coverage,
    /// 255 where its centre unit is inside, 0 where it is not.
    Centre,
    /// 255 where any unit of it is inside on any of its sub-scanlines, 0
    /// where none is; the outline's edges through it are painted after.
    AnyPart,
}

impl Measure {
    /// What `edge` asks a sweep for: the coverage, which [`HardEdge::Half`]
    /// hardens afterwards, or the rule's own answer.
    fn of(edge: HardEdge) -> Measure {
        match edge {
            HardEdge::Half => Measure::Coverage,
            HardEdge::Centre => Measure::Centre,
            HardEdge::AnyPart => Measure::AnyPart,
        }
    }
}

/// [`fill`] with every pixel whole or empty by `edge`, through a scratch of its
/// own.
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn fill_hard(
    path: &Path,
    rule: FillRule,
    x0: i32,
    y0: i32,
    width: u32,
    height: u32,
    tolerance: f64,
    stop: Option<&dyn Fn() -> bool>,
    edge: HardEdge,
) -> Mask {
    fill_hard_with(
        &mut FillScratch::new(),
        path,
        rule,
        x0,
        y0,
        width,
        height,
        tolerance,
        stop,
        edge,
    )
}

/// [`fill_with`] with every pixel whole or empty by `edge`: under
/// [`HardEdge::Half`] the coverage hardened, under [`HardEdge::Centre`] each
/// pixel whose centre the shape holds on the filler's grid, under
/// [`HardEdge::AnyPart`] each pixel the shape meets.
///
/// The centre rule reads one sub-scanline a row, the row's middle, and one
/// unit a pixel on it, the pixel's middle, in the page's device pixels: a
/// region's left edge is a whole pixel, so a tile reads the units the page
/// reads (ruling 5). The any-part rule reads every unit of every
/// sub-scanline, as the coverage does, and then each edge of the flattened
/// path over each row it reaches, in the page's device pixels too. The rows,
/// the stop and the scratch are [`fill_with`]'s, and a row whose sub-scanline
/// does not close is read as the sorted sweep reads it.
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn fill_hard_with(
    scratch: &mut FillScratch,
    path: &Path,
    rule: FillRule,
    x0: i32,
    y0: i32,
    width: u32,
    height: u32,
    tolerance: f64,
    stop: Option<&dyn Fn() -> bool>,
    edge: HardEdge,
) -> Mask {
    let mut mask = Mask::empty(x0, y0, width, height);
    if width == 0 || height == 0 {
        return mask;
    }
    #[cfg(test)]
    let held = scratch.capacities();
    scratch.sweep_into(&mut mask, path, rule, tolerance, stop, Measure::of(edge));
    #[cfg(test)]
    count::allocated(scratch.grown_from(&held));
    if edge == HardEdge::Half {
        mask.harden();
    }
    mask
}

/// [`fill_outline`] with every pixel whole or empty by `edge`, as
/// [`fill_hard_with`] fills a path: the mask [`fill_hard_with`] gives the
/// outline's path, byte for byte.
///
/// **The any-part rule reads the outline's own pieces** where an edge passes
/// through a pixel, and a scratch holds them only from a stroke written after
/// [`FillScratch::keep_outline_points`] asked it to — every other stroke
/// keeps the edges alone and costs what it did. An outline written without
/// its pieces is read under [`HardEdge::AnyPart`] from its samples alone: a
/// pixel is whole where a unit of it is inside on a sub-scanline, and a piece
/// lying wholly between two sub-scanlines paints nothing.
#[must_use]
pub fn fill_outline_hard(
    scratch: &mut FillScratch,
    x0: i32,
    y0: i32,
    width: u32,
    height: u32,
    stop: Option<&dyn Fn() -> bool>,
    edge: HardEdge,
) -> Mask {
    let mut mask = Mask::empty(x0, y0, width, height);
    if width == 0 || height == 0 {
        return mask;
    }
    #[cfg(test)]
    let held = scratch.capacities();
    let FillScratch { outline, work, .. } = &mut *scratch;
    let measure = Measure::of(edge);
    let cut = work.sweep(
        &mut mask,
        &outline.edges,
        outline.open,
        FillRule::NonZero,
        stop,
        measure,
    );
    if measure == Measure::AnyPart {
        touch_edges(&mut mask, &outline.points, &outline.ends, cut, stop);
    }
    #[cfg(test)]
    count::allocated(scratch.grown_from(&held));
    if edge == HardEdge::Half {
        mask.harden();
    }
    mask
}

/// The outline the last [`crate::stroke_with`] or
/// [`crate::stroke_mapped_with`] wrote into `scratch`, filled under the
/// non-zero rule into the region: the mask [`fill`] gives the [`Path`]
/// [`crate::stroke::stroke`] returns for the same stroke, byte for byte, with
/// no path built and nothing flattened.
///
/// The stroker turned each piece into edges as it made it, so this sweeps
/// them as [`fill`] sweeps a path's — the same rows, cells and stop hook — and
/// the region is the caller's, as it is for [`fill`]: a renderer takes it from
/// [`FillScratch::outline_bounds`], the bounds the outline's path would have.
/// The outline stays in the scratch until the next stroke, whatever is filled
/// through the scratch in between.
#[must_use]
pub fn fill_outline(
    scratch: &mut FillScratch,
    x0: i32,
    y0: i32,
    width: u32,
    height: u32,
    stop: Option<&dyn Fn() -> bool>,
) -> Mask {
    let mut mask = Mask::empty(x0, y0, width, height);
    if width == 0 || height == 0 {
        return mask;
    }
    #[cfg(test)]
    let held = scratch.capacities();
    let FillScratch { outline, work, .. } = &mut *scratch;
    work.sweep(
        &mut mask,
        &outline.edges,
        outline.open,
        FillRule::NonZero,
        stop,
        Measure::Coverage,
    );
    #[cfg(test)]
    count::allocated(scratch.grown_from(&held));
    mask
}

/// The memory a fill works in, which the caller owns and every fill through
/// it reuses: grown when a fill needs more, never shrunk.
///
/// The flattened points, the edges and the lists that hold each until its
/// row, the active set, one row of [`Cell`]s with the crowded lanes' later
/// crossings, and the buffers of a row swept because it does not close. A
/// renderer filling a page's paths, clips, glyphs, image quads and meshes
/// through one allocates for the largest of them, and after that only the
/// masks it is handed back. A stroke's outline is held here too, as the edges
/// the stroker made of it ([`fill_outline`]), apart from a path's.
///
/// **Nothing left in it reaches a pixel.** A fill empties the points, the
/// edges, the lists and the active set before it uses them; every cell is
/// cleared as it is turned into a byte and the crowded-lane table as it is
/// read, and a stop is asked before a row's deposit, so the cells are clear
/// between fills however the last one ended. A stroke empties the outline
/// before it writes one.
#[derive(Default)]
pub struct FillScratch {
    /// The path's points, subpath after subpath.
    points: Vec<Point>,
    /// Where in `points` each subpath ends.
    ends: Vec<usize>,
    /// The edges, in the order the path gives them.
    edges: Vec<Edge>,
    /// The last stroke's outline, as edges.
    outline: Outline,
    /// What the rows are worked in.
    work: Work,
}

impl FillScratch {
    /// An empty scratch, which the first fill through it sizes.
    #[must_use]
    pub fn new() -> FillScratch {
        FillScratch::default()
    }

    /// The bounds of the outline the last stroke wrote into this scratch:
    /// [`Path::bounds`] of the path [`crate::stroke::stroke`] would have
    /// returned for it, from the same points in the same order, and `None`
    /// where that path is empty.
    #[must_use]
    pub fn outline_bounds(&self) -> Option<(f64, f64, f64, f64)> {
        self.outline.bounds
    }

    /// Whether every stroke written into this scratch from now on keeps its
    /// pieces' points beside their edges: what [`fill_outline_hard`] reads
    /// under [`HardEdge::AnyPart`] where an edge passes through a pixel. Off
    /// by default, so that a stroke filled any other way holds and costs
    /// what it did; a caller filling outlines under that rule turns it on
    /// once.
    pub fn keep_outline_points(&mut self, keep: bool) {
        self.outline.keep = keep;
    }

    /// The outline, emptied for a stroke to write.
    pub(crate) fn begin_outline(&mut self) -> &mut Outline {
        self.outline.clear();
        &mut self.outline
    }

    /// Fills `path` into `mask`'s rectangle, which is not empty, measuring
    /// each pixel as `measure` says.
    fn sweep_into(
        &mut self,
        mask: &mut Mask,
        path: &Path,
        rule: FillRule,
        tolerance: f64,
        stop: Option<&dyn Fn() -> bool>,
        measure: Measure,
    ) {
        let FillScratch {
            points,
            ends,
            edges,
            work,
            ..
        } = self;

        flatten_flat(path, tolerance, points, ends);
        edges.clear();
        let mut open = false;
        let mut begin = 0;
        for end in ends.iter() {
            if let Some(poly) = points.get(begin..*end) {
                open |= build_edges(poly, edges);
            }
            begin = *end;
        }
        let cut = work.sweep(mask, edges, open, rule, stop, measure);
        if measure == Measure::AnyPart {
            touch_edges(mask, points, ends, cut, stop);
        }
    }

    /// The capacity of every buffer, for [`FillScratch::grown_from`].
    #[cfg(test)]
    pub(crate) fn capacities(&self) -> [usize; 15] {
        [
            self.points.capacity(),
            self.ends.capacity(),
            self.edges.capacity(),
            self.outline.edges.capacity(),
            self.outline.points.capacity(),
            self.outline.ends.capacity(),
            self.work.starts.capacity(),
            self.work.next.capacity(),
            self.work.active.capacity(),
            self.work.row.cells.capacity(),
            self.work.row.touched.capacity(),
            self.work.row.heads.capacity(),
            self.work.row.later.capacity(),
            self.work.sweep.accumulator.capacity(),
            self.work.sweep.crossings.capacity(),
        ]
    }

    /// How many buffers have grown since `held` was taken: each made at least
    /// one allocation, and a fill makes none anywhere else but its mask.
    #[cfg(test)]
    pub(crate) fn grown_from(&self, held: &[usize; 15]) -> u64 {
        self.capacities()
            .iter()
            .zip(held)
            .filter(|(now, then)| now != then)
            .count() as u64
    }
}

/// A stroke's outline, as the edges of a fill: each piece the stroker makes of
/// it — a segment's quad, a join, a cap — turned into edges as it is made, so
/// the outline is never a [`Path`] and never flattened.
///
/// It fills to what the outline's path fills to because it is made of what
/// filling that path would make of it. A piece is a polygon; its verbs in a
/// path would be a move, a line to each later point and a close, which
/// flattening gives back as the same points closed on the first, and
/// [`build_edges`] makes the same edges of the polygon, in the same order,
/// closing it itself. A path holds at most [`MAX_VERBS`] and drops every verb
/// past them, so the outline counts the verbs its pieces would have been: the
/// piece that reaches the bound keeps the points whose verbs fit, closed on its
/// first as flattening closes a subpath with no close, and nothing after it is
/// kept.
///
/// Asked to ([`FillScratch::keep_outline_points`]), it also keeps each piece's
/// points, as a path's flattened subpaths are kept, for the any-part rule.
#[derive(Default)]
pub(crate) struct Outline {
    /// The edges, piece after piece.
    edges: Vec<Edge>,
    /// [`Path::bounds`] over the points kept.
    bounds: Option<(f64, f64, f64, f64)>,
    /// Whether [`build_edges`] dropped an edge that may cover a sub-scanline.
    open: bool,
    /// The verbs the pieces kept would have been in a path.
    verbs: usize,
    /// Whether each piece's points are kept beside its edges.
    keep: bool,
    /// The pieces' points, piece after piece, when they are kept.
    points: Vec<Point>,
    /// Where in `points` each piece ends.
    ends: Vec<usize>,
}

impl Outline {
    fn clear(&mut self) {
        self.edges.clear();
        self.bounds = None;
        self.open = false;
        self.verbs = 0;
        self.points.clear();
        self.ends.clear();
    }

    /// One piece of the outline: a polygon of finite points, wound as every
    /// other piece is.
    pub(crate) fn piece(&mut self, points: &[Point]) {
        let room = MAX_VERBS.saturating_sub(self.verbs);
        self.verbs = self.verbs.saturating_add((points.len() + 1).min(room));
        let kept = points.get(..points.len().min(room)).unwrap_or(&[]);
        for p in kept {
            self.bounds = Some(match self.bounds {
                None => (p.x, p.y, p.x, p.y),
                Some((x0, y0, x1, y1)) => (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y)),
            });
        }
        self.open |= build_edges(kept, &mut self.edges);
        if self.keep && !kept.is_empty() {
            self.points.extend_from_slice(kept);
            self.ends.push(self.points.len());
        }
    }
}

/// The buffers a fill's rows are worked in, whichever edges they sweep.
#[derive(Default)]
struct Work {
    /// Each active row's list of the edges that begin in it: its first edge.
    starts: Vec<u32>,
    /// Each edge's successor in its row's list.
    next: Vec<u32>,
    /// The edges reaching the current row, in no order.
    active: Vec<u32>,
    /// The current row's cells.
    row: Row,
    /// A row that does not close.
    sweep: Sweep,
}

impl Work {
    /// Sweeps `edges` into `mask`'s rectangle, which is not empty, under
    /// `rule`, measuring each pixel as `measure` says; `open` says whether
    /// [`build_edges`] dropped an edge that may cover a sub-scanline.
    ///
    /// Returns the device row a stop ended the sweep before, which no row at
    /// or below holds anything of, or `None` where the sweep ran to its end.
    fn sweep(
        &mut self,
        mask: &mut Mask,
        edges: &[Edge],
        open: bool,
        rule: FillRule,
        stop: Option<&dyn Fn() -> bool>,
        measure: Measure,
    ) -> Option<i64> {
        let (x0, y0, width, height) = (mask.x0, mask.y0, mask.width, mask.height);
        let Work {
            starts,
            next,
            active,
            row: cells,
            sweep,
        } = self;
        let rows = active_rows(edges, y0, height);
        if rows.is_empty() {
            return None;
        }

        // Each edge waits in the list of the row it begins in, or of the first
        // row when it begins above the region; an edge that ends above the
        // region or begins below it is in no list. Lists, not a sort: the
        // order an edge joins the active set in decides nothing.
        let samples = i64::from(SAMPLES);
        let first_row = i64::from(y0) + i64::from(rows.start);
        starts.clear();
        starts.resize((rows.end - rows.start) as usize, NONE);
        next.clear();
        next.resize(edges.len(), NONE);
        for (index, edge) in edges.iter().enumerate() {
            let begins = i64::from(edge.top).div_euclid(samples);
            let ends = (i64::from(edge.bottom) - 1).div_euclid(samples);
            if ends < first_row {
                continue;
            }
            let slot = (begins.max(first_row) - first_row) as usize;
            if let (Some(head), Some(link)) = (starts.get_mut(slot), next.get_mut(index)) {
                *link = *head;
                *head = index as u32;
            }
        }

        cells.prepare(width as usize);
        active.clear();
        let origin = i64::from(x0) * 256;
        let units = i64::from(width) * 256;

        for (step, row) in rows.enumerate() {
            // Row granularity, not pixel granularity, and before the row's
            // deposit: a stop leaves no row half converted and the cells
            // clear.
            if step % STOP_EVERY == 0 && stop.is_some_and(|stop| stop()) {
                return Some(i64::from(y0) + i64::from(row));
            }
            let mut at = starts.get(step).copied().unwrap_or(NONE);
            while let Some(link) = next.get(at as usize) {
                active.push(at);
                at = *link;
            }

            // The row's first sub-scanline. `active_rows` keeps every row the
            // edges reach between `i32::MIN / 16` and `i32::MAX / 16`, so this
            // is the sweep's `(y0 + row) × 16` with nothing to saturate.
            let top = (i64::from(y0) + i64::from(row)) * samples;
            let base = (row as usize) * (width as usize);
            let Some(out) = mask.data.get_mut(base..base + width as usize) else {
                return None;
            };
            if measure == Measure::Centre {
                // The centre rule reads the row's middle sub-scanline alone,
                // lane `CENTRE_LANE` of the cells, and only it is deposited.
                let middle = top + samples / 2;
                if open && sweep.is_open_at(edges, active, middle, rule) {
                    sweep.centres(edges, active, middle, rule, (x0, width), out);
                } else {
                    for index in active.iter() {
                        let Some(edge) = edges.get(*index as usize) else {
                            continue;
                        };
                        if i64::from(edge.top) <= middle && middle < i64::from(edge.bottom) {
                            // As below: the difference cannot wrap.
                            cells.deposit(
                                CENTRE_LANE,
                                crossing(edge, middle).wrapping_sub(origin),
                                units,
                                edge.winding,
                            );
                        }
                    }
                    cells.convert_centres(rule, out);
                }
            } else if open && sweep.is_open(edges, active, top, rule) {
                let touch = measure == Measure::AnyPart;
                sweep.row(edges, active, top, rule, (x0, width), out, touch);
            } else {
                for index in active.iter() {
                    let Some(edge) = edges.get(*index as usize) else {
                        continue;
                    };
                    let from = i64::from(edge.top).max(top);
                    let to = i64::from(edge.bottom).min(top + samples);
                    // `crossing` is within 2^47 of zero and `origin` within
                    // 2^39, so the difference cannot wrap.
                    each_crossing(edge, from, to, |sub, x| {
                        cells.deposit(
                            (sub - top) as usize,
                            x.wrapping_sub(origin),
                            units,
                            edge.winding,
                        );
                    });
                }
                if measure == Measure::AnyPart {
                    cells.convert::<true>(rule, out);
                } else {
                    cells.convert::<false>(rule, out);
                }
            }

            // The edges that end in this row leave; the hole takes the last.
            let below = top + samples;
            let mut at = 0;
            while let Some(index) = active.get(at) {
                if edges
                    .get(*index as usize)
                    .is_none_or(|edge| i64::from(edge.bottom) <= below)
                {
                    active.swap_remove(at);
                } else {
                    at += 1;
                }
            }
        }
        None
    }
}

/// Where `edge` meets sub-scanline `sub`, in 1/256 pixel.
///
/// Saturating throughout: a path may reach ±1e9, whose sub-scanline index
/// alone exceeds `i32`, and the slope times the height then exceeds `i64`.
/// Clamping puts the crossing far outside the region, which is where it
/// belongs. Down to the 1/256 the coverage is measured in, **rounded** rather
/// than floored: `+ half, >> bits` is a `floor` of a shifted value and
/// therefore commutes with moving the whole shape by a whole pixel — the
/// property a tile rests on, and the reason the shift is spelled from
/// [`SUBSTEP_BITS`] rather than written out beside a constant that could move
/// without it.
fn crossing(edge: &Edge, sub: i64) -> i64 {
    let steps = sub.saturating_sub(i64::from(edge.top));
    edge.dxdy
        .saturating_mul(steps)
        .saturating_add(edge.x)
        .saturating_add(SUBSTEP / 2)
        >> SUBSTEP_BITS
}

/// Hands `put` each sub-scanline of `from..to`, which lie inside the edge's
/// own, with [`crossing`] there.
///
/// **An edge [`Edge::plain`] marks is stepped, not multiplied.** None of its
/// products or sums saturates, so the sum `crossing` reduces is exact, and the
/// next sub-scanline's is this one's plus `dxdy`: one wrapping add a
/// sub-scanline, which cannot wrap, where `crossing` makes a saturating
/// multiply and two saturating adds. Any other edge keeps `crossing` itself,
/// whose saturation is what puts a crossing past `i64` far outside the region.
#[inline(always)]
fn each_crossing(edge: &Edge, from: i64, to: i64, mut put: impl FnMut(i64, i64)) {
    if edge.plain {
        let mut at = edge
            .dxdy
            .wrapping_mul(from - i64::from(edge.top))
            .wrapping_add(edge.x)
            .wrapping_add(SUBSTEP / 2);
        for sub in from..to {
            put(sub, at >> SUBSTEP_BITS);
            at = at.wrapping_add(edge.dxdy);
        }
    } else {
        for sub in from..to {
            put(sub, crossing(edge, sub));
        }
    }
}

/// Whether an edge whose first crossing is `x`, whose slope is `dxdy` and
/// which covers `height` sub-scanlines makes no crossing [`crossing`]
/// saturates: `|dxdy| × height + |x| + SUBSTEP / 2` is at most `i64::MAX`.
///
/// Then for every step from 0 to `height` the product, both sums and the
/// rounding half are exact, each bounded by that one figure, so
/// [`each_crossing`] may step from one sub-scanline to the next by adding
/// `dxdy` and lands on `crossing`'s value at each — the step past the last
/// sub-scanline included, which it takes and does not use. Decided once an
/// edge, as it is built.
fn unsaturating(x: i64, dxdy: i64, height: i64) -> bool {
    dxdy.unsigned_abs()
        .checked_mul(height.unsigned_abs())
        .and_then(|reach| reach.checked_add(x.unsigned_abs()))
        .and_then(|reach| reach.checked_add(SUBSTEP.unsigned_abs() / 2))
        .is_some_and(|reach| reach <= i64::MAX.unsigned_abs())
}

/// The rule as a mask on a winding: the rule calls a winding inside where the
/// winding, so masked, is not zero — every bit for non-zero, the lowest for
/// even-odd, which is oddness in two's complement for a negative winding too.
/// One expression for both rules, so the walk over a cell's lanes holds no
/// branch on the rule.
fn rule_mask(rule: FillRule) -> i32 {
    match rule {
        FillRule::NonZero => -1,
        FillRule::EvenOdd => 1,
    }
}

/// 1 where a winding is inside under the rule `mask` is [`rule_mask`] of, 0
/// where it is not.
#[inline(always)]
fn inside_by(winding: i32, mask: i32) -> i32 {
    i32::from(winding & mask != 0)
}

/// Each lane's bit in a cell's `lanes` and `multi`, lane 0 the lowest.
const LANE_BITS: [u16; 16] = [
    0x0001, 0x0002, 0x0004, 0x0008, 0x0010, 0x0020, 0x0040, 0x0080, 0x0100, 0x0200, 0x0400, 0x0800,
    0x1000, 0x2000, 0x4000, 0x8000,
];

/// The lanes of a cell that hold one crossing each, all sixteen at once: each
/// such lane's winding moved by its crossing's, and what the cell's total and
/// the count of lanes inside gain by it, as `(total, inside)`.
///
/// **One sixteen-wide loop of `i32` with no branch**, written so that the
/// `+simd128` build vectorises it and the plain one runs it as it stands: the
/// lanes that hold one crossing are taken by their bit as a multiplier on the
/// crossing's winding, so a lane that holds none, or several, moves by
/// nothing and gains nothing here, and the rule is [`rule_mask`]'s mask. A
/// lane moved from `W` to `W + w` gains `(256 − o) × (I(W + w) − I(W))`, the
/// units from its crossing at offset `o` to the cell's right edge; the lanes
/// are independent, so taking them together is taking them one by one.
#[inline(always)]
fn single_lanes(winding: &mut [i32; 16], cell: &Cell, mask: i32) -> (i32, i32) {
    let single = cell.lanes & !cell.multi;
    let (mut total, mut inside) = (0i32, 0i32);
    for (((w, &moved), &off), &bit) in winding
        .iter_mut()
        .zip(&cell.w)
        .zip(&cell.off)
        .zip(&LANE_BITS)
    {
        let take = i32::from(single & bit != 0);
        let before = inside_by(*w, mask);
        *w += take * i32::from(moved);
        let change = inside_by(*w, mask) - before;
        total += (256 - i32::from(off)) * change;
        inside += change;
    }
    (total, inside)
}

/// One pixel of the row being filled: a **lane** for each of its sixteen
/// sub-scanlines.
///
/// A lane's first crossing is held in the cell; the crossings after it, which
/// only a crowded pixel has, are lists through the row's [`Row::later`], one
/// head a lane in a block of sixteen the cell takes from [`Row::heads`] when a
/// lane first holds two. Forty bytes.
#[derive(Clone, Copy)]
struct Cell {
    /// The lanes holding at least one crossing.
    lanes: u16,
    /// The lanes holding two or more.
    multi: u16,
    /// A lane's first crossing: its winding.
    w: [i8; 16],
    /// And its offset in the pixel, `x & 255`.
    off: [u8; 16],
    /// The cell's block of sixteen list heads in [`Row::heads`], or [`NONE`].
    more: u32,
}

impl Cell {
    const CLEAR: Cell = Cell {
        lanes: 0,
        multi: 0,
        w: [0; 16],
        off: [0; 16],
        more: NONE,
    };
}

/// A crossing after the first in its lane of a cell.
#[derive(Clone, Copy)]
struct Later {
    /// Its offset in the pixel.
    off: u8,
    /// Its winding.
    w: i8,
    /// The lane's next later crossing, or [`NONE`].
    next: u32,
}

/// The cells of one row, and what turning them into bytes needs.
struct Row {
    /// One a pixel of the region's width, clear between rows.
    cells: Vec<Cell>,
    /// A bit a cell, set where the cell holds a crossing; clear between rows.
    touched: Vec<u64>,
    /// Each lane's winding from the crossings left of the region.
    left: [i32; 16],
    /// Blocks of sixteen list heads, one block a crowded cell.
    heads: Vec<u32>,
    /// The crossings after a lane's first.
    later: Vec<Later>,
    /// A crowded lane's windings summed at each offset, clear between lanes.
    table: [i32; 256],
    /// The offsets of `table` a crowded lane took, clear between lanes.
    map: [u64; 4],
}

impl Default for Row {
    fn default() -> Row {
        Row {
            cells: Vec::new(),
            touched: Vec::new(),
            left: [0; 16],
            heads: Vec::new(),
            later: Vec::new(),
            table: [0; 256],
            map: [0; 4],
        }
    }
}

impl Row {
    /// Readies the row for a fill `width` pixels wide. The cells are clear
    /// already — a fill clears each as it converts it — so only their number
    /// changes.
    fn prepare(&mut self, width: usize) {
        self.cells.resize(width, Cell::CLEAR);
        self.touched.resize(width.div_ceil(64), 0);
        self.left = [0; 16];
        self.heads.clear();
        self.later.clear();
    }

    /// Puts a crossing with winding `w` on `lane`, at `rel` 1/256 units right
    /// of the region's left edge, `units` being the region's width in them.
    ///
    /// Left of the region it is folded into the lane's winding there; at or
    /// right of the region's right edge it can change no pixel of it.
    fn deposit(&mut self, lane: usize, rel: i64, units: i64, w: i32) {
        let lane = lane & 15;
        if rel < 0 {
            self.left[lane] += w;
            return;
        }
        if rel >= units {
            return;
        }
        let index = (rel >> 8) as usize;
        let Some(cell) = self.cells.get_mut(index) else {
            return;
        };
        if let Some(bits) = self.touched.get_mut(index >> 6) {
            *bits |= 1 << (index & 63);
        }
        let off = (rel & 255) as u8;
        let bit = 1u16 << lane;
        if cell.lanes & bit == 0 {
            cell.lanes |= bit;
            cell.w[lane] = w as i8;
            cell.off[lane] = off;
            return;
        }
        cell.multi |= bit;
        if cell.more == NONE {
            cell.more = (self.heads.len() / 16) as u32;
            self.heads.extend_from_slice(&[NONE; 16]);
        }
        if let Some(head) = self.heads.get_mut(cell.more as usize * 16 + lane) {
            self.later.push(Later {
                off,
                w: w as i8,
                next: *head,
            });
            *head = (self.later.len() - 1) as u32;
        }
    }

    /// Walks the row's cells left to right into `out`, one byte a cell, and
    /// leaves every cell clear.
    ///
    /// **Only the cells holding a crossing are visited.** Between two of them
    /// no lane's winding changes, so every pixel there has the same byte; the
    /// walk jumps from one set bit of [`Row::touched`] to the next and writes
    /// the run between as one slice fill (`solid`). A run ends at the first
    /// cell holding a crossing on any lane, so a pixel that reaches 255 only by
    /// the clamp still goes through its cell.
    ///
    /// The walk carries each lane's winding at its position and the count of
    /// lanes the rule calls inside. A cell with no crossing is `16 × inside`:
    /// each lane inside covers its 256 units. A cell with crossings starts
    /// from `256 × inside`, and each of its lanes' crossings at offset `o`
    /// adds `(256 − o) × (I(W + w) − I(W))` as it moves the lane's winding —
    /// the units from the crossing to the cell's right edge change sides
    /// together. The lanes are independent, so the order they are taken in
    /// decides nothing: the lanes holding one crossing are taken all sixteen
    /// at once ([`single_lanes`]), and then each lane holding several, its
    /// crossings taken by offset, bucketed rather than compared, those at one
    /// offset summed first, which is the sweep's zero-length span. The byte is
    /// `min(total / 16, 255)`, as the sweep's.
    ///
    /// With `TOUCH` the byte is instead 255 where the total is above zero —
    /// where any unit of the pixel is inside on any of its sub-scanlines, a
    /// pixel of one to fifteen units among them — and 0 where it is not: the
    /// samples' half of [`HardEdge::AnyPart`]. A constant, so the coverage's
    /// walk is compiled as it was.
    fn convert<const TOUCH: bool>(&mut self, rule: FillRule, out: &mut [u8]) {
        let Row {
            cells,
            touched,
            left,
            heads,
            later,
            table,
            map,
        } = self;
        let mask = rule_mask(rule);
        let mut winding = *left;
        let mut inside: i32 = winding.iter().map(|w| inside_by(*w, mask)).sum();
        // The first pixel not yet written.
        let mut at = 0;
        for (word, bits) in touched.iter_mut().enumerate() {
            let mut held = core::mem::take(bits);
            while held != 0 {
                let index = word * 64 + held.trailing_zeros() as usize;
                held &= held - 1;
                solid::<TOUCH>(out, at, index, inside);
                at = index + 1;
                let (Some(cell), Some(byte)) = (cells.get_mut(index), out.get_mut(index)) else {
                    continue;
                };
                count::converted(1);

                let (moved, changed) = single_lanes(&mut winding, cell, mask);
                let mut total = 256 * inside + moved;
                inside += changed;
                let mut crowded = cell.multi;
                while crowded != 0 {
                    let lane = (crowded.trailing_zeros() & 15) as usize;
                    crowded &= crowded - 1;

                    // A crowded lane: its windings bucketed by offset.
                    let mut put = |off: u8, w: i8| {
                        table[usize::from(off)] += i32::from(w);
                        map[usize::from(off >> 6)] |= 1 << (off & 63);
                    };
                    put(cell.off[lane], cell.w[lane]);
                    let mut crowd = 1u64;
                    let mut next = heads
                        .get(cell.more as usize * 16 + lane)
                        .copied()
                        .unwrap_or(NONE);
                    while let Some(item) = later.get(next as usize) {
                        put(item.off, item.w);
                        crowd += 1;
                        next = item.next;
                    }
                    count::shared(crowd);
                    for (word, bits) in map.iter_mut().enumerate() {
                        let mut taken = core::mem::take(bits);
                        while taken != 0 {
                            let off = word * 64 + taken.trailing_zeros() as usize;
                            taken &= taken - 1;
                            let d = core::mem::take(&mut table[off & 255]);
                            let before = inside_by(winding[lane], mask);
                            winding[lane] += d;
                            let change = inside_by(winding[lane], mask) - before;
                            total += (256 - off as i32) * change;
                            inside += change;
                        }
                    }
                }
                *byte = if TOUCH {
                    if total > 0 {
                        255
                    } else {
                        0
                    }
                } else {
                    (total / 16).min(255) as u8
                };
                *cell = Cell::CLEAR;
            }
        }
        solid::<TOUCH>(out, at, out.len(), inside);
        *left = [0; 16];
        heads.clear();
        later.clear();
    }

    /// Walks the row's cells left to right into `out` as
    /// [`HardEdge::Centre`] reads them, and leaves every cell clear: only
    /// lane [`CENTRE_LANE`] holds crossings, and a pixel is 255 where that
    /// lane's winding at its centre unit is one the rule calls inside, 0
    /// where it is not.
    ///
    /// The winding at a pixel's centre is the lane's winding from the left of
    /// the region, every crossing of the cells before it, and the crossings
    /// of its own cell at an offset up to [`CENTRE_UNIT`]: a crossing at unit
    /// `x` begins or ends a span at `x`, which holds unit `x` and not the one
    /// before it. A sum, so the order the crossings came in decides nothing
    /// and none is compared. Between two cells holding a crossing the
    /// winding does not change, and the run between is written at once.
    fn convert_centres(&mut self, rule: FillRule, out: &mut [u8]) {
        let Row {
            cells,
            touched,
            left,
            heads,
            later,
            ..
        } = self;
        let mask = rule_mask(rule);
        let bit = 1u16 << CENTRE_LANE;
        let mut winding = left[CENTRE_LANE];
        // The first pixel not yet written.
        let mut at = 0;
        for (word, bits) in touched.iter_mut().enumerate() {
            let mut held = core::mem::take(bits);
            while held != 0 {
                let index = word * 64 + held.trailing_zeros() as usize;
                held &= held - 1;
                whole(out, at, index, inside_by(winding, mask) != 0);
                at = index + 1;
                let (Some(cell), Some(byte)) = (cells.get_mut(index), out.get_mut(index)) else {
                    continue;
                };

                // The cell's crossings either side of its centre, summed.
                let (mut before, mut after) = (0i32, 0i32);
                let mut take = |off: u8, w: i8| {
                    if off <= CENTRE_UNIT {
                        before += i32::from(w);
                    } else {
                        after += i32::from(w);
                    }
                };
                if cell.lanes & bit != 0 {
                    take(cell.off[CENTRE_LANE], cell.w[CENTRE_LANE]);
                }
                if cell.multi & bit != 0 {
                    let mut next = heads
                        .get(cell.more as usize * 16 + CENTRE_LANE)
                        .copied()
                        .unwrap_or(NONE);
                    while let Some(item) = later.get(next as usize) {
                        take(item.off, item.w);
                        next = item.next;
                    }
                }
                winding += before;
                if inside_by(winding, mask) != 0 {
                    *byte = 255;
                }
                winding += after;
                *cell = Cell::CLEAR;
            }
        }
        whole(out, at, out.len(), inside_by(winding, mask) != 0);
        *left = [0; 16];
        heads.clear();
        later.clear();
    }
}

/// Writes the run of pixels `from..to` of a row [`HardEdge::Centre`] reads,
/// none of which holds a crossing: whole where the winding across it is
/// inside, and no write at all where it is not, the mask being zero already.
fn whole(out: &mut [u8], from: usize, to: usize, inside: bool) {
    if inside && from < to {
        if let Some(run) = out.get_mut(from..to) {
            run.fill(255);
        }
    }
}

/// Writes the run of pixels `from..to` of a row, none of which holds a
/// crossing, given the count of lanes the rule calls inside across it: one
/// slice fill of `min(16 × inside, 255)` — 255 where every lane is inside — and
/// no write at all where none is, the mask being zero already. With `TOUCH`,
/// 255 where any lane is inside, as [`Row::convert`] reads a cell.
fn solid<const TOUCH: bool>(out: &mut [u8], from: usize, to: usize, inside: i32) {
    if from >= to {
        return;
    }
    count::converted(1);
    let value = if TOUCH {
        if inside > 0 {
            255
        } else {
            0
        }
    } else {
        (16 * inside).min(255) as u8
    };
    if value != 0 {
        if let Some(run) = out.get_mut(from..to) {
            run.fill(value);
        }
    }
}

/// The rows of a fill some of whose sub-scanlines do not close, swept as the
/// reference sweeps them.
///
/// Every subpath is closed before it is filled, so on every sub-scanline the
/// windings of the crossings sum to zero — unless [`build_edges`] dropped an
/// edge that covers it (a point past the finite, a slope that is not, a `dy`
/// under `f64::EPSILON`). Such a sub-scanline ends inside, the sweep never
/// adds the span it last opened, and what it does add depends on the order of
/// crossings at one `x`. So a row holding one is swept as the sweep sweeps it:
/// its crossings sorted by position, then by their edge's top, then by its
/// place in the path — the order the sweep's two stable sorts leave them in —
/// and walked span by span. It is the only comparison a fill makes, and no
/// closed path reaches it.
#[derive(Default)]
struct Sweep {
    /// Coverage of the row, in 1/256 units summed over its sub-scanlines.
    accumulator: Vec<u16>,
    /// A sub-scanline's crossings: position, the edge's top, its index and
    /// its winding.
    crossings: Vec<(i64, i32, u32, i32)>,
}

impl Sweep {
    /// Whether any sub-scanline of the row starting at sub-scanline `top`
    /// ends inside: its windings, over every edge covering it, summing to a
    /// value the rule calls inside.
    fn is_open(&self, edges: &[Edge], active: &[u32], top: i64, rule: FillRule) -> bool {
        let mut total = [0i32; 16];
        for edge in active.iter().filter_map(|index| edges.get(*index as usize)) {
            for sub in i64::from(edge.top).max(top)..i64::from(edge.bottom).min(top + 16) {
                total[((sub - top) & 15) as usize] += edge.winding;
            }
        }
        total.iter().any(|w| inside(*w, rule))
    }

    /// Whether sub-scanline `sub` ends inside, as [`Sweep::is_open`] asks it
    /// of each of a row's sixteen.
    fn is_open_at(&self, edges: &[Edge], active: &[u32], sub: i64, rule: FillRule) -> bool {
        let total: i32 = active
            .iter()
            .filter_map(|index| edges.get(*index as usize))
            .filter(|edge| i64::from(edge.top) <= sub && sub < i64::from(edge.bottom))
            .map(|edge| edge.winding)
            .sum();
        inside(total, rule)
    }

    /// The row's pixels under [`HardEdge::Centre`] when its middle
    /// sub-scanline, `sub`, does not close: that sub-scanline swept as
    /// [`Sweep::row`] sweeps each of its own — crossings sorted by position,
    /// then the edge's top, then its place in the path, and walked span by
    /// span — and every pixel whose centre unit a span holds painted whole.
    /// The span opened last and never closed adds nothing, as it adds no
    /// coverage there.
    fn centres(
        &mut self,
        edges: &[Edge],
        active: &[u32],
        sub: i64,
        rule: FillRule,
        (x0, width): (i32, u32),
        out: &mut [u8],
    ) {
        self.crossings.clear();
        for index in active {
            let Some(edge) = edges.get(*index as usize) else {
                continue;
            };
            if i64::from(edge.top) <= sub && sub < i64::from(edge.bottom) {
                self.crossings
                    .push((crossing(edge, sub), edge.top, *index, edge.winding));
            }
        }
        if self.crossings.len() < 2 {
            return;
        }
        self.crossings.sort_unstable_by(|a, b| {
            count::compared();
            (a.0, a.1, a.2).cmp(&(b.0, b.1, b.2))
        });

        let mut winding = 0i32;
        let mut span_start = 0i64;
        for (x, _, _, w) in &self.crossings {
            let was_inside = inside(winding, rule);
            winding += w;
            let is_inside = inside(winding, rule);
            if !was_inside && is_inside {
                span_start = *x;
            } else if was_inside && !is_inside {
                paint_centres(out, x0, width, span_start, *x);
            }
        }
    }

    /// The row starting at sub-scanline `top`, swept into `out`: each
    /// pixel's coverage, or with `touch` 255 where a span holds any unit of
    /// it and 0 where none does.
    #[allow(clippy::too_many_arguments)]
    fn row(
        &mut self,
        edges: &[Edge],
        active: &[u32],
        top: i64,
        rule: FillRule,
        (x0, width): (i32, u32),
        out: &mut [u8],
        touch: bool,
    ) {
        self.accumulator.clear();
        self.accumulator.resize(width as usize, 0);
        for sub in top..top + i64::from(SAMPLES) {
            self.crossings.clear();
            for index in active {
                let Some(edge) = edges.get(*index as usize) else {
                    continue;
                };
                if i64::from(edge.top) <= sub && sub < i64::from(edge.bottom) {
                    self.crossings
                        .push((crossing(edge, sub), edge.top, *index, edge.winding));
                }
            }
            if self.crossings.len() < 2 {
                continue;
            }
            self.crossings.sort_unstable_by(|a, b| {
                count::compared();
                (a.0, a.1, a.2).cmp(&(b.0, b.1, b.2))
            });

            let mut winding = 0i32;
            let mut span_start = 0i64;
            for (x, _, _, w) in &self.crossings {
                let was_inside = inside(winding, rule);
                winding += w;
                let is_inside = inside(winding, rule);
                if !was_inside && is_inside {
                    span_start = *x;
                } else if was_inside && !is_inside {
                    add_span(&mut self.accumulator, x0, width, span_start, *x);
                }
            }
        }
        for (slot, total) in out.iter_mut().zip(self.accumulator.iter()) {
            *slot = if touch {
                if *total > 0 {
                    255
                } else {
                    0
                }
            } else {
                (*total / SAMPLES as u16).min(255) as u8
            };
        }
    }
}

/// Work a fill does, counted for the guards: compiled only under `cfg(test)`
/// and kept per thread, so tests running side by side cannot add into each
/// other's.
#[cfg(test)]
pub(crate) mod count {
    use std::cell::Cell;

    std::thread_local! {
        static COMPARED: Cell<u64> = const { Cell::new(0) };
        static SHARED: Cell<u64> = const { Cell::new(0) };
        static ALLOCATED: Cell<u64> = const { Cell::new(0) };
        static CONVERTED: Cell<u64> = const { Cell::new(0) };
    }

    /// One comparison made ordering edges or crossings.
    pub(crate) fn compared() {
        COMPARED.with(|count| count.set(count.get() + 1));
    }

    /// `crowd` crossings sharing a lane of a cell.
    pub(crate) fn shared(crowd: u64) {
        SHARED.with(|count| count.set(count.get() + crowd));
    }

    /// One conversion: a cell holding a crossing turned into its byte, or a
    /// run of cells holding none written at once.
    pub(crate) fn converted(conversions: u64) {
        CONVERTED.with(|count| count.set(count.get() + conversions));
    }

    /// `buffers` of a fill's scratch that grew, each by at least one
    /// allocation.
    pub(crate) fn allocated(buffers: u64) {
        ALLOCATED.with(|count| count.set(count.get() + buffers));
    }

    /// What was counted on this thread since the last call, which starts every
    /// count again from zero.
    pub(crate) fn take() -> Counts {
        Counts {
            compared: COMPARED.with(|c| c.replace(0)),
            shared: SHARED.with(|c| c.replace(0)),
            allocated: ALLOCATED.with(|c| c.replace(0)),
            converted: CONVERTED.with(|c| c.replace(0)),
        }
    }

    /// The counts [`take`] reads.
    pub(crate) struct Counts {
        /// Comparisons made ordering edges or crossings.
        pub(crate) compared: u64,
        /// Crossings that shared a lane of a cell.
        pub(crate) shared: u64,
        /// Scratch buffers that grew.
        pub(crate) allocated: u64,
        /// Conversions: cells holding a crossing, and runs of cells holding
        /// none.
        pub(crate) converted: u64,
    }
}

/// The counters outside tests: nothing.
#[cfg(not(test))]
mod count {
    pub(crate) fn compared() {}

    pub(crate) fn shared(_crowd: u64) {}

    pub(crate) fn converted(_conversions: u64) {}
}

/// The rows of the region any edge can reach, as a half-open range of row
/// indices into the mask.
///
/// Rows outside it hold no crossing on any of their sixteen sub-scanlines, so
/// their coverage is zero and the mask already says zero there. A page-tall
/// region and a glyph-tall one therefore do the same work, which is what stops
/// the cost of a fill scaling with the paper rather than with the shape.
fn active_rows(edges: &[Edge], y0: i32, height: u32) -> Range<u32> {
    let (Some(first_sub), Some(last_sub)) = (
        edges.iter().map(|edge| edge.top).min(),
        edges.iter().map(|edge| edge.bottom).max(),
    ) else {
        return 0..0;
    };

    let samples = i64::from(SAMPLES);
    // An edge is exclusive at its lower end, so the last sub-scanline it
    // covers is the one before `bottom`. Rounding that the other way drops the
    // final row of every shape whose lowest edge ends exactly on a row
    // boundary — which for an axis-aligned rectangle is all of them.
    let first = i64::from(first_sub).div_euclid(samples);
    let last = i64::from(last_sub).saturating_sub(1).div_euclid(samples);

    let origin = i64::from(y0);
    let limit = i64::from(height);
    let start = (first - origin).clamp(0, limit) as u32;
    let end = (last - origin + 1).clamp(0, limit) as u32;
    start..end.max(start)
}

fn inside(winding: i32, rule: FillRule) -> bool {
    match rule {
        FillRule::NonZero => winding != 0,
        FillRule::EvenOdd => winding % 2 != 0,
    }
}

/// Paints whole every pixel of a row whose centre unit the span `[from, to)`
/// holds, `from` and `to` in 1/256 pixel, absolute: pixel `i` of a region
/// whose left edge is pixel `x0` has its centre at unit `256·(x0 + i) + 128`.
fn paint_centres(out: &mut [u8], x0: i32, width: u32, from: i64, to: i64) {
    let first_centre = i64::from(x0)
        .saturating_mul(256)
        .saturating_add(i64::from(CENTRE_UNIT));
    // The first pixel whose centre is at or right of `at`, clamped to the row.
    let first_at = |at: i64| {
        at.saturating_sub(first_centre)
            .saturating_add(255)
            .div_euclid(256)
            .clamp(0, i64::from(width)) as usize
    };
    let (start, end) = (first_at(from), first_at(to));
    if start < end {
        if let Some(run) = out.get_mut(start..end) {
            run.fill(255);
        }
    }
}

/// Segments [`touch_edges`] walks between one cancellation check and the
/// next. A segment costs a row of its reach, so a thousand of them are about
/// the work [`STOP_EVERY`] rows of a sweep are.
const TOUCH_STOP_EVERY: usize = 1024;

/// `x` across to the nearest 1/256 of a pixel, a half going up, as a crossing
/// is taken there; `None` for a value that is not a number. A value past
/// `i64` saturates, far outside any region.
fn unit_of(x: f64) -> Option<i64> {
    if x.is_nan() {
        return None;
    }
    Some((x * 256.0 + 0.5).floor() as i64)
}

/// The edges' half of [`HardEdge::AnyPart`]: paints whole every pixel of
/// `mask` an edge of the outline passes through.
///
/// The outline is each subpath of `points` — its points up to each of `ends`
/// — closed on its first point, as [`build_edges`] closes it, with every
/// segment kept, horizontal and short ones too; a subpath of one point is a
/// segment of no length there. A segment's part inside the open row
/// `(r, r + 1)` — from where it crosses the row's top, or begins inside the
/// row, to where it crosses the bottom or ends — has its two ends' `x` taken
/// to the nearest 1/256, and that extent `[lo, hi]` paints each pixel of the
/// row whose open interval of units, `(256·col, 256·col + 256)`, it reaches:
/// columns `⌊lo / 256⌋` to `⌈hi / 256⌉`, the last excluded. So a segment
/// along a row's edge is in neither row, one along a column's edge reaches
/// neither column, and a point on a pixel's edge paints none.
///
/// Every row is a device row and every unit a device unit, so a tile paints
/// what the page paints under it (ruling 5). Rows at or below `cut`, where a
/// stopped sweep stopped, are left as it left them, and `stop` is asked every
/// [`TOUCH_STOP_EVERY`] segments, ending the walk as it ends a sweep.
fn touch_edges(
    mask: &mut Mask,
    points: &[Point],
    ends: &[usize],
    cut: Option<i64>,
    stop: Option<&dyn Fn() -> bool>,
) {
    let (left, top) = (i64::from(mask.x0), i64::from(mask.y0));
    let right = left + i64::from(mask.width);
    let bottom = top + i64::from(mask.height);
    let bottom = cut.map_or(bottom, |cut| cut.min(bottom));
    let width = mask.width as usize;
    let mut walked = 0usize;
    let mut begin = 0;
    for &end in ends {
        let poly = points.get(begin..end).unwrap_or(&[]);
        begin = end;
        let (Some(&first), Some(&last)) = (poly.first(), poly.last()) else {
            continue;
        };
        let closing = (first != last).then_some([last, first]);
        let alone = (poly.len() == 1).then_some([first, first]);
        for pair in poly
            .windows(2)
            .chain(closing.as_ref().map(|pair| &pair[..]))
            .chain(alone.as_ref().map(|pair| &pair[..]))
        {
            let [a, b] = pair else {
                continue;
            };
            if walked % TOUCH_STOP_EVERY == 0 && stop.is_some_and(|stop| stop()) {
                return;
            }
            walked += 1;
            if !a.is_finite() || !b.is_finite() {
                continue;
            }
            let (upper, lower) = if a.y <= b.y { (*a, *b) } else { (*b, *a) };
            // The open rows the segment reaches: `upper.y < r + 1` and
            // `lower.y > r`, kept inside the region and above the cut.
            let from = (upper.y.floor() as i64).max(top);
            let to = (lower.y.ceil() as i64).min(bottom);
            for r in from..to {
                let (lo, hi) = if upper.y == lower.y {
                    (unit_of(a.x.min(b.x)), unit_of(a.x.max(b.x)))
                } else {
                    let at = |y: f64| {
                        if y <= upper.y {
                            upper.x
                        } else if y >= lower.y {
                            lower.x
                        } else {
                            upper.x + (y - upper.y) * (lower.x - upper.x) / (lower.y - upper.y)
                        }
                    };
                    let (u, v) = (
                        unit_of(at(upper.y.max(r as f64))),
                        unit_of(at(lower.y.min((r + 1) as f64))),
                    );
                    match (u, v) {
                        (Some(u), Some(v)) => (Some(u.min(v)), Some(u.max(v))),
                        _ => (None, None),
                    }
                };
                let (Some(lo), Some(hi)) = (lo, hi) else {
                    continue;
                };
                let first_col = lo.div_euclid(256).max(left);
                let last_col = hi.saturating_add(255).div_euclid(256).min(right);
                if first_col >= last_col {
                    continue;
                }
                let base = (r - top) as usize * width;
                let (start, stop_at) = ((first_col - left) as usize, (last_col - left) as usize);
                if let Some(run) = mask.data.get_mut(base + start..base + stop_at) {
                    run.fill(255);
                }
            }
        }
    }
}

/// Adds a horizontal span's contribution, with exact partial coverage at both
/// ends. `from` and `to` are in 1/256 pixel, absolute.
fn add_span(accumulator: &mut [u16], x0: i32, width: u32, from: i64, to: i64) {
    if to <= from {
        return;
    }
    let origin = i64::from(x0).saturating_mul(256);
    let left = from.saturating_sub(origin).max(0);
    let right = to
        .saturating_sub(origin)
        .min(i64::from(width).saturating_mul(256));
    if right <= left {
        return;
    }

    let first = (left / 256) as usize;
    let last = ((right - 1) / 256) as usize;

    if first == last {
        // A span inside one pixel.
        if let Some(slot) = accumulator.get_mut(first) {
            *slot = slot.saturating_add((right - left) as u16);
        }
        return;
    }

    if let Some(slot) = accumulator.get_mut(first) {
        let covered = 256 - (left % 256);
        *slot = slot.saturating_add(covered as u16);
    }
    // The fully covered interior, as one slice rather than one bounds-checked
    // index per pixel. `last` is clamped rather than trusted, because the
    // indexed form silently skipped a column past the end and a slice range
    // past the end would skip the whole run instead -- the same arithmetic,
    // and a different answer on the one case that matters.
    let to = last.min(accumulator.len());
    if let Some(run) = accumulator.get_mut((first + 1).min(to)..to) {
        for slot in run {
            *slot = slot.saturating_add(256);
        }
    }
    if let Some(slot) = accumulator.get_mut(last) {
        let covered = right - (last as i64) * 256;
        *slot = slot.saturating_add(covered as u16);
    }
}

/// Turns a polyline into edges, dropping horizontal ones (they contribute no
/// crossings), and **closes it**.
///
/// ISO 32000-1 8.5.3.1: "before filling, each open subpath is implicitly
/// closed" — and 8.5.4 applies the same to a clipping path, which reaches this
/// function through the same `fill`. So the segment from the last point back
/// to the first is an edge whether or not the path said `h`. Until September
/// 2026 it was not, and `90 10 m 150 30 l 110 60 l f` painted nothing at all:
/// its two edges both run downward, every sub-scanline holds one crossing and
/// never a span. A subpath the stroker or a `re` closed already ends on its
/// first point, so the closing pair is degenerate there and adds no edge.
///
/// Returns whether it **dropped an edge that may cover a sub-scanline** — one
/// with a point past the finite, a `y` whose sixteenfold is not finite, a `dy`
/// under `f64::EPSILON` across a sub-scanline, or a slope or first crossing
/// that is not finite. Only such a drop can leave a sub-scanline whose
/// windings do not sum to zero, which [`Sweep`] is for; an edge whose ends
/// round to one sub-scanline covers none, and dropping it closes everything it
/// would have closed.
fn build_edges(poly: &[Point], edges: &mut Vec<Edge>) -> bool {
    let mut dropped = false;
    let closing = match (poly.first(), poly.last()) {
        (Some(first), Some(last)) if first != last => Some([*last, *first]),
        _ => None,
    };
    for pair in poly
        .windows(2)
        .chain(closing.as_ref().map(|pair| &pair[..]))
    {
        let (Some(a), Some(b)) = (pair.first(), pair.get(1)) else {
            continue;
        };
        if !a.is_finite() || !b.is_finite() {
            dropped = true;
            continue;
        }

        let (top, bottom, winding) = if a.y < b.y { (a, b, 1) } else { (b, a, -1) };

        let y_top = (top.y * f64::from(SAMPLES)).ceil();
        let y_bottom = (bottom.y * f64::from(SAMPLES)).ceil();
        if !y_top.is_finite() || !y_bottom.is_finite() {
            dropped = true;
            continue;
        }
        let (y_top, y_bottom) = (y_top as i32, y_bottom as i32);
        if y_top >= y_bottom {
            continue;
        }

        let dy = bottom.y - top.y;
        if dy.abs() < f64::EPSILON {
            dropped = true;
            continue;
        }
        let slope = (bottom.x - top.x) / dy;

        // x where the edge meets the first sub-scanline it covers.
        let y_first = f64::from(y_top) / f64::from(SAMPLES);
        let x_first = top.x + (y_first - top.y) * slope;
        let dxdy = slope / f64::from(SAMPLES);

        if !x_first.is_finite() || !dxdy.is_finite() {
            dropped = true;
            continue;
        }

        // Into 1/256 of a pixel with [`SUBSTEP`]'s extra bits under it, which
        // is what [`crossing`] reduces from.
        //
        // **No clamp to the segment's own ends here, and none on the crossings
        // either.** One stood in both places while ruling 5's guard was being
        // written, on the theory that a nearly horizontal edge -- `dx / dy`
        // with an ulp or two of `dy` -- could land `x_first` outside the two
        // points that produced it. It cannot: `y_first` is `ceil(y x 16) / 16`
        // and every operation reaching it is exact, `y_first - top.y` and
        // `bottom.y - top.y` are both exact by Sterbenz, so the only error in
        // `x_first` is the one rounding of the division, and the reduction in
        // [`crossing`] is monotone -- a crossing inside `[min x, max x]` reduces to a
        // unit inside their two rounded units. Instrumented over the whole
        // workspace suite, a clamp on the crossings fired **zero** times.
        //
        // Clamping the *start* did fire, on ordinary edges, and was a defect
        // rather than a guard: `x_lo` is `round(min x)`, which sits up to half
        // a unit above the real minimum, so an edge starting below it was
        // pushed up and its whole length went with it. That is 79 pixels of
        // the fingerprint pages moving a level for no reason a geometry can
        // name.
        let sub = |v: f64| (v * 256.0 * SUBSTEP as f64).round() as i64;
        let (x, dxdy) = (sub(x_first), sub(dxdy));

        edges.push(Edge {
            top: y_top,
            bottom: y_bottom,
            // `round`, not `as`, because it is closer and free -- **and it
            // is not what makes a tile equal the page**, which is worth
            // stating here because an earlier draft of this comment claimed it
            // was and the claim is measurably wrong.
            //
            // The load-bearing rounding is the *reduction* in [`crossing`]:
            // that one takes a crossing to the nearest 1/256 unit, which is
            // the unit coverage is measured in and the unit a translation
            // has to commute with. Injecting a truncation there fails seven
            // tests, `tiles_at_other_scales_are_byte_equal_too` among them.
            // This conversion is two-to-the-sixteen finer than that: truncating
            // here moves `x` by less than `2^-16` of a 1/256 unit, which
            // reaches a byte only if the exact value sits inside that of a
            // half-step. Injected on 15 September 2026 it failed **nothing**
            // in a 4 653-test workspace.
            //
            // So it stays on its own merits and no others: a pixel's coverage
            // is linear in the crossing, the nearest representable value is the
            // one whose span is nearest the geometry's, and `round` is
            // correctly rounded by IEEE 754 and named in ruling 4 as safe on a
            // pixel path, so it costs no target stability.
            //
            // What no rounding in this file can fix is named where it shows: a
            // whole-pixel shift is exact as arithmetic, but `fl(u + e)` and
            // `fl(u + e - tx)` are two roundings at two magnitudes, and
            // `crates/tinker-pdf/tests/render_regions.rs` bounds that residual
            // by measurement rather than claiming it away.
            x,
            dxdy,
            winding,
            plain: unsaturating(x, dxdy, i64::from(y_bottom) - i64::from(y_top)),
        });
    }
    dropped
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::flatten;

    fn rect_mask(x: f64, y: f64, w: f64, h: f64) -> Mask {
        let mut path = Path::new();
        path.rect(x, y, w, h);
        fill(&path, FillRule::NonZero, 0, 0, 20, 20, 0.1, None)
    }

    #[test]
    fn a_pixel_aligned_rectangle_is_fully_covered() {
        let mask = rect_mask(2.0, 3.0, 5.0, 4.0);

        for y in 3..7 {
            for x in 2..7 {
                assert_eq!(mask.at(x, y), 255, "inside at ({x}, {y})");
            }
        }
        assert_eq!(mask.at(1, 3), 0, "left of the rectangle");
        assert_eq!(mask.at(7, 3), 0, "right of it");
        assert_eq!(mask.at(2, 2), 0, "above it");
        assert_eq!(mask.at(2, 7), 0, "below it");
    }

    /// A hardened mask holds the two extremes and nothing between, and where
    /// the soft one was already whole or empty it is unchanged.
    #[test]
    fn a_hardened_mask_is_whole_or_empty_everywhere() {
        let mut path = Path::new();
        path.move_to(1.3, 2.7);
        path.curve_to(18.1, 0.4, 19.9, 16.5, 2.2, 17.1);
        path.close();
        let soft = fill(&path, FillRule::NonZero, 0, 0, 20, 20, 0.1, None);
        assert!(
            soft.data.iter().any(|v| *v != 0 && *v != 255),
            "the fixture has partial pixels to harden"
        );
        let mut hard = soft.clone();
        hard.harden();
        for (soft, hard) in soft.data.iter().zip(&hard.data) {
            assert!(*hard == 0 || *hard == 255, "{hard} is neither");
            let expected = if *soft >= 128 { 255 } else { 0 };
            assert_eq!(*hard, expected, "a soft {soft} hardens to {expected}");
        }
    }

    /// **Two shapes sharing an edge leave no gap and no double-painting
    /// between them once hardened**, except at an exact half. The edge is a
    /// diagonal across a square, so every pixel along it is split between the
    /// two triangles at a different ratio.
    #[test]
    fn hardened_shapes_sharing_an_edge_partition_its_pixels() {
        let triangle = |corner: (f64, f64)| {
            let mut path = Path::new();
            path.move_to(0.5, 0.5);
            path.line_to(corner.0, corner.1);
            path.line_to(19.25, 17.75);
            path.close();
            let mut mask = fill(&path, FillRule::NonZero, 0, 0, 20, 20, 0.1, None);
            mask.harden();
            mask
        };
        let (upper, lower) = (triangle((19.25, 0.5)), triangle((0.5, 17.75)));
        let mut square = Path::new();
        square.rect(0.5, 0.5, 18.75, 17.25);
        let whole = fill(&square, FillRule::NonZero, 0, 0, 20, 20, 0.1, None);

        let mut both = 0;
        for y in 0..20 {
            for x in 0..20 {
                if whole.at(x, y) != 255 {
                    continue; // the square's own soft rim is not the edge's business
                }
                let (a, b) = (upper.at(x, y), lower.at(x, y));
                assert!(a == 255 || b == 255, "a gap at ({x}, {y})");
                if a == 255 && b == 255 {
                    both += 1;
                }
            }
        }
        assert!(
            both <= 2,
            "{both} pixels taken by both: only an exact half may be"
        );
    }

    /// **Every coverage quantises to its level** (RC-11.2).
    ///
    /// A mask holding each of the 256 coverages once, quantised at every
    /// level from 0 to 8 and past it. Each value is the quantisation computed
    /// here from its definition, in its own integers: the nearest of the
    /// `2^b` indices, `c·n / 255` rounded half up as `(2·c·n + 255) / 510`,
    /// and that index's value, `q·255 / n` rounded as `(2·q·255 + n) / (2n)`.
    /// Level 0 is `harden`, and so is level 1; level 8 and every number above
    /// it leave the mask alone. At each level the values are non-decreasing,
    /// run from 0 to 255, and are exactly the `2^b` levels, two bits giving 0,
    /// 85, 170 and 255.
    #[test]
    fn every_coverage_quantises_to_its_level() {
        let all: Vec<u8> = (0..=255).collect();
        let at = |bits: u8| {
            let mut mask = Mask {
                x0: -3,
                y0: 7,
                width: 16,
                height: 16,
                data: all.clone(),
                outside: 0,
            };
            mask.quantise(bits);
            assert_eq!(
                (mask.x0, mask.y0, mask.width, mask.height),
                (-3, 7, 16, 16),
                "{bits} bits keeps the rectangle"
            );
            mask.data
        };
        let mut hard = Mask {
            x0: 0,
            y0: 0,
            width: 16,
            height: 16,
            data: all.clone(),
            outside: 0,
        };
        hard.harden();
        assert_eq!(at(0), hard.data, "level 0 is harden");
        assert_eq!(at(1), hard.data, "and so is level 1");
        for bits in [8u8, 9, 16, 255] {
            assert_eq!(at(bits), all, "{bits} bits leaves the coverage");
        }
        for bits in 1..=7u8 {
            let n = (1u64 << bits) - 1;
            let got = at(bits);
            for (&c, &value) in all.iter().zip(&got) {
                let index = (2 * u64::from(c) * n + 255) / 510;
                let want = (2 * index * 255 + n) / (2 * n);
                assert_eq!(u64::from(value), want, "coverage {c} at {bits} bits");
            }
            assert!(
                got.windows(2).all(|pair| pair[0] <= pair[1]),
                "{bits} bits never decreases"
            );
            assert_eq!((got[0], got[255]), (0, 255), "{bits} bits keeps both ends");
            let mut levels = got.clone();
            levels.dedup();
            assert_eq!(
                levels.len() as u64,
                n + 1,
                "{bits} bits holds 2^{bits} levels"
            );
        }
        let mut two = at(2);
        two.dedup();
        assert_eq!(two, [0, 85, 170, 255]);
    }

    #[test]
    fn a_half_pixel_edge_is_half_covered() {
        let mask = rect_mask(0.0, 0.0, 4.5, 4.0);
        let edge = mask.at(4, 1);
        assert!(
            (120..=136).contains(&edge),
            "a half-covered pixel should be near 128, got {edge}"
        );
    }

    #[test]
    fn the_even_odd_rule_leaves_a_hole() {
        // Two nested squares wound the same way: non-zero fills the middle,
        // even-odd punches it out.
        let mut path = Path::new();
        path.rect(0.0, 0.0, 10.0, 10.0);
        path.rect(3.0, 3.0, 4.0, 4.0);

        let nonzero = fill(&path, FillRule::NonZero, 0, 0, 12, 12, 0.1, None);
        let evenodd = fill(&path, FillRule::EvenOdd, 0, 0, 12, 12, 0.1, None);

        assert_eq!(nonzero.at(5, 5), 255, "non-zero fills the inner square");
        assert_eq!(evenodd.at(5, 5), 0, "even-odd leaves it empty");
        assert_eq!(evenodd.at(1, 1), 255, "the outer ring stays filled");
    }

    #[test]
    fn filling_is_bit_identical_across_runs() {
        let mut path = Path::new();
        path.move_to(1.3, 2.7);
        path.curve_to(8.1, 0.4, 9.9, 12.5, 2.2, 11.1);
        path.close();

        let a = fill(&path, FillRule::NonZero, 0, 0, 16, 16, 0.1, None);
        let b = fill(&path, FillRule::NonZero, 0, 0, 16, 16, 0.1, None);
        assert_eq!(a.data, b.data, "ruling 4: the same input, the same bytes");
    }

    /// 8.5.3.1: a subpath is closed before it is filled, whether or not it
    /// said so. The open triangle's two stated edges both run downward, so
    /// without the implicit third edge no sub-scanline holds a span and the
    /// mask is empty; with it, the mask is the closed triangle's exactly.
    #[test]
    fn an_open_subpath_fills_as_if_it_were_closed() {
        let mut open = Path::new();
        open.move_to(9.0, 1.0);
        open.line_to(15.0, 3.0);
        open.line_to(11.0, 6.0);
        let mut closed = open.clone();
        closed.close();

        let a = fill(&open, FillRule::NonZero, 0, 0, 16, 8, 0.1, None);
        let b = fill(&closed, FillRule::NonZero, 0, 0, 16, 8, 0.1, None);
        assert!(b.data.contains(&255), "the fixture has an inside");
        assert_eq!(a.data, b.data, "the open subpath is the closed one");
    }

    #[test]
    fn a_triangle_covers_about_half_its_box() {
        let mut path = Path::new();
        path.move_to(0.0, 0.0);
        path.line_to(20.0, 0.0);
        path.line_to(0.0, 20.0);
        path.close();

        let mask = fill(&path, FillRule::NonZero, 0, 0, 20, 20, 0.1, None);
        let total: u64 = mask.data.iter().map(|&v| u64::from(v)).sum();
        let area = total as f64 / 255.0;
        assert!(
            (180.0..220.0).contains(&area),
            "half of 400 is 200, got {area}"
        );
    }

    #[test]
    fn masks_multiply_when_they_intersect() {
        let a = rect_mask(0.0, 0.0, 10.0, 10.0);
        let b = rect_mask(5.0, 0.0, 10.0, 10.0);
        let clipped = a.intersect(&b);

        assert_eq!(clipped.at(7, 5), 255, "both cover this");
        assert_eq!(clipped.at(2, 5), 0, "only the first covers this");
        assert_eq!(clipped.at(12, 5), 0, "only the second, and outside a");
    }

    /// Bounding the result must not change what it reads: a mask that does
    /// not cover a pixel and one that covers it with zero are the same mask.
    #[test]
    fn an_intersection_costs_the_overlap_and_reads_the_same_outside_it() {
        let mut path = Path::new();
        path.rect(1.0, 1.0, 14.0, 14.0);
        let big = fill(&path, FillRule::NonZero, 0, 0, 16, 16, 0.1, None);
        let small = fill(&path, FillRule::NonZero, 4, 6, 5, 3, 0.1, None);

        let clipped = big.intersect(&small);
        assert_eq!(
            (clipped.x0, clipped.y0, clipped.width, clipped.height),
            (4, 6, 5, 3),
            "the result is the overlap, not the larger of the two"
        );
        assert_eq!(clipped.data.len(), 15, "and it allocated only that");

        // Every pixel of the canvas agrees with the unbounded product.
        for y in -2..18 {
            for x in -2..18 {
                let want = u32::from(big.at(x, y)) * u32::from(small.at(x, y));
                let want = ((want + 127) / 255) as u8;
                assert_eq!(clipped.at(x, y), want, "at ({x}, {y})");
            }
        }

        // And in place, which keeps the destination's rectangle rather than
        // shrinking to the overlap, agrees with it pixel for pixel.
        let mut in_place = big.clone();
        in_place.intersect_in_place(&small);
        for y in -2..18 {
            for x in -2..18 {
                assert_eq!(in_place.at(x, y), clipped.at(x, y), "at ({x}, {y})");
            }
        }
    }

    /// The last row and the last column of an in-place intersection.
    ///
    /// Nothing in the workspace caught an `intersect_in_place` that stopped a
    /// row short — injected and re-run, zero assertions failed. It is invisible
    /// almost everywhere because a paint's rectangle is already the clip's
    /// rectangle, and a rectangular clip is fully covered in the middle of
    /// itself. It is *not* invisible at the rectangle's own edge, which is
    /// where the clip's coverage falls away: a row skipped there is ink
    /// outside the clip.
    ///
    /// The shape here is larger than the region, so its coverage is 255
    /// everywhere and the product must equal the clip exactly — at every
    /// pixel, including the ones a loop bound gets wrong.
    #[test]
    fn intersecting_in_place_reaches_the_last_row_and_column() {
        let mut clip_path = Path::new();
        clip_path.rect(2.0, 2.0, 6.5, 6.5);
        let clip = fill(&clip_path, FillRule::NonZero, 1, 1, 10, 10, 0.1, None);

        let mut path = Path::new();
        path.rect(-5.0, -5.0, 30.0, 30.0);
        let mut mask = fill(&path, FillRule::NonZero, 1, 1, 10, 10, 0.1, None);
        assert_eq!(mask.at(10, 10), 255, "the shape covers the whole region");

        mask.intersect_in_place(&clip);
        for y in 0..12 {
            for x in 0..12 {
                assert_eq!(
                    mask.at(x, y),
                    clip.at(x, y),
                    "an opaque shape clipped is the clip, at ({x}, {y})"
                );
            }
        }
        assert_eq!(mask.at(10, 10), 0, "and the last row is outside the clip");
    }

    /// The rectangle a consumer walks. Every edge is checked independently,
    /// because a mask hanging off one side of the canvas is exactly what a
    /// glyph at the margin is.
    #[test]
    fn a_mask_reports_the_canvas_pixels_it_can_reach() {
        let inside = Mask::empty(3, 4, 5, 6);
        assert_eq!(inside.overlap(100, 100), (3, 4, 8, 10));

        // Hanging off each edge in turn, clamped to the canvas and never past
        // it.
        assert_eq!(Mask::empty(-3, 4, 5, 6).overlap(100, 100), (0, 4, 2, 10));
        assert_eq!(Mask::empty(3, -4, 5, 6).overlap(100, 100), (3, 0, 8, 2));
        assert_eq!(Mask::empty(97, 4, 5, 6).overlap(100, 100), (97, 4, 100, 10));
        assert_eq!(Mask::empty(3, 96, 5, 6).overlap(100, 100), (3, 96, 8, 100));

        // Entirely outside, in both directions: an empty range rather than a
        // reversed one, which would panic a `for` loop or wrap a subtraction.
        assert_eq!(Mask::empty(-50, -50, 5, 6).overlap(100, 100), (0, 0, 0, 0));
        assert_eq!(
            Mask::empty(500, 500, 5, 6).overlap(100, 100),
            (100, 100, 100, 100)
        );
        // And nothing overflows at the extremes. Built by hand rather than
        // through `empty`, which would try to allocate the square of a `u32`.
        let huge = Mask {
            x0: i32::MAX - 1,
            y0: i32::MIN,
            width: u32::MAX,
            height: u32::MAX,
            data: Vec::new(),
            outside: 0,
        };
        // Its columns are past the right edge and its rows cover everything,
        // so the x range is empty and the y range is the canvas.
        assert_eq!(huge.overlap(10, 10), (10, 0, 10, 10));

        // A mask reading a value outside itself reaches the whole canvas,
        // wherever its rectangle is: inside, hanging off, wholly off, empty.
        for mask in [
            Mask::empty(3, 4, 5, 6),
            Mask::empty(-3, 4, 5, 6),
            Mask::empty(500, 500, 5, 6),
            Mask::empty(-50, -50, 0, 0),
        ] {
            let pale = mask.with_outside(51);
            assert_eq!(pale.overlap(100, 100), (0, 0, 100, 100), "{pale:?}");
            assert_eq!(
                pale.overlap_at((-7, 9), 30, 20),
                (0, 0, 30, 20),
                "{pale:?} on a canvas away from the origin"
            );
        }
    }

    /// A canvas-sized field equal to `mask` at every pixel of a `width` by
    /// `height` canvas standing at `origin`, reading zero outside itself: a
    /// pale soft mask spelled without an outside value
    /// (`docs/design/performance.md`, P-25).
    fn field_of(mask: &Mask, origin: (i32, i32), width: u32, height: u32) -> Mask {
        let mut field = Mask::empty(origin.0, origin.1, width, height);
        for row in 0..height {
            for col in 0..width {
                let (x, y) = (origin.0 + col as i32, origin.1 + row as i32);
                field.data[(row * width + col) as usize] = mask.at(x, y);
            }
        }
        field
    }

    /// A mask over `(x0, y0, width, height)` whose pixels differ, reading
    /// `outside` beyond itself.
    fn varied(x0: i32, y0: i32, width: u32, height: u32, outside: u8, seed: u32) -> Mask {
        let mut mask = Mask::empty(x0, y0, width, height).with_outside(outside);
        for (index, value) in mask.data.iter_mut().enumerate() {
            *value = match (index as u32 * 7 + seed) % 5 {
                0 => 0,
                1 => 255,
                _ => ((index as u32 * 53 + seed * 31) % 256) as u8,
            };
        }
        mask
    }

    /// **Every consumer of a mask reads its outside value as a canvas-sized
    /// field holding the same values would read** (P-25.2).
    ///
    /// A soft mask whose `/BC` is pale reads the backdrop's luminosity outside
    /// its group's box, as the box and that value. A field the size of the
    /// canvas, the box written into a sea of that value and reading zero
    /// beyond the canvas, reads the same at every pixel of the canvas, and
    /// each rule `docs/design/performance.md`'s P-25 states is held here
    /// against that field, on canvases standing at the origin and away from
    /// it (a tile of a page) and over boxes inside the canvas, hanging off two
    /// of its edges, wholly off it and of no pixels at all:
    ///
    /// - `at` reads the field's value at every canvas pixel, and `overlap_at`
    ///   reaches the canvas the field covers;
    /// - `intersect` of a clip and the mask — either way round — reads the
    ///   clip times the field at every pixel and keeps the clip's rectangle and
    ///   zero outside it; of two pale masks, the two fields' product, the
    ///   product of the two outsides beyond the smallest rectangle holding
    ///   both;
    /// - `intersect_in_place` of a paint's coverage by the mask is the
    ///   coverage times the field, byte for byte, keeping zero outside; of a
    ///   pale mask by a clip, the product `intersect` gives;
    /// - `harden` hardens the outside value with the rest;
    /// - `Fragments::would_overlap` gives a run inside the canvas the field's
    ///   answer, whatever coverage the run holds;
    /// - `Canvas::fill_mask_inked`, `Canvas::composite` and
    ///   `Canvas::knock_out` write through the mask exactly the bytes they
    ///   write through the field, in every format, under `Normal` and
    ///   `Multiply`, at a whole and a partial alpha — the opaque path, which
    ///   slices the mask's rows, is never taken for a mask that reads outside
    ///   itself;
    /// - and a mask reading zero outside itself, every clip, reads zero there
    ///   still.
    #[test]
    fn every_mask_consumer_reads_the_outside_value() {
        use crate::blend::BlendMode;
        use crate::canvas::{Canvas, Color, PixelFormat};
        use crate::fragments::Fragments;

        let canvases = [((0, 0), 40u32, 30u32), ((-6, 11), 33, 21)];
        let boxes = [
            (5, 14, 17, 9),
            (-9, 8, 20, 30),
            (30, 35, 25, 9),
            (200, -80, 6, 6),
            (12, 16, 0, 0),
        ];
        let formats = [
            PixelFormat::Gray8,
            PixelFormat::GrayA8,
            PixelFormat::Rgb8,
            PixelFormat::Rgba8,
            PixelFormat::CmykA8,
            PixelFormat::LabA8,
        ];
        let mut cases = 0;
        for &(origin, width, height) in &canvases {
            let canvas_pixels = |each: &mut dyn FnMut(i32, i32)| {
                for row in 0..height as i32 {
                    for col in 0..width as i32 {
                        each(origin.0 + col, origin.1 + row);
                    }
                }
            };
            for (index, &(bx, by, bw, bh)) in boxes.iter().enumerate() {
                for outside in [1u8, 51, 128, 254, 255] {
                    let mask = varied(bx, by, bw, bh, outside, index as u32 + 3);
                    let field = field_of(&mask, origin, width, height);
                    let what =
                        format!("box {:?} reading {outside} on {origin:?}", (bx, by, bw, bh));

                    canvas_pixels(&mut |x, y| {
                        assert_eq!(mask.at(x, y), field.at(x, y), "{what}: at ({x}, {y})");
                    });
                    assert_eq!(
                        mask.overlap_at(origin, width, height),
                        field.overlap_at(origin, width, height),
                        "{what}: the pixels it reaches"
                    );

                    // A clip times the mask, and the mask times a clip.
                    let clip = varied(origin.0 + 3, origin.1 + 2, 21, 13, 0, 9);
                    let want = clip.intersect(&field);
                    for got in [clip.intersect(&mask), mask.intersect(&clip)] {
                        assert_eq!(
                            got.outside, 0,
                            "{what}: a clip's product reads zero outside"
                        );
                        assert_eq!(
                            (got.x0, got.y0, got.width, got.height),
                            (clip.x0, clip.y0, clip.width, clip.height),
                            "{what}: a clip's product keeps the clip's rectangle"
                        );
                        canvas_pixels(&mut |x, y| {
                            assert_eq!(
                                got.at(x, y),
                                want.at(x, y),
                                "{what}: clipped at ({x}, {y})"
                            );
                        });
                    }

                    // Two pale masks: the product of the fields, and the
                    // product of the outsides beyond both.
                    let other = varied(origin.0 + 15, origin.1 - 4, 9, 22, 200, 5);
                    let both = mask.intersect(&other);
                    let want = field.intersect(&field_of(&other, origin, width, height));
                    canvas_pixels(&mut |x, y| {
                        assert_eq!(
                            both.at(x, y),
                            want.at(x, y),
                            "{what}: two masks at ({x}, {y})"
                        );
                    });
                    assert_eq!(
                        both.outside,
                        product(outside, 200),
                        "{what}: the outsides multiply"
                    );
                    assert_eq!(both.at(i32::MIN, i32::MAX), product(outside, 200));

                    // A paint's coverage times the mask, in place.
                    let mut paint = varied(origin.0 + 1, origin.1 + 1, width - 2, height - 2, 0, 1);
                    let mut by_field = paint.clone();
                    paint.intersect_in_place(&mask);
                    by_field.intersect_in_place(&field);
                    assert_eq!(paint.data, by_field.data, "{what}: a paint's coverage");
                    assert_eq!(paint.outside, 0, "{what}: a paint reads zero outside still");
                    // And the mask times a clip, in place: `intersect`'s product.
                    let mut pale = mask.clone();
                    pale.intersect_in_place(&clip);
                    let clipped = mask.intersect(&clip);
                    assert_eq!(
                        (
                            pale.x0,
                            pale.y0,
                            pale.width,
                            pale.height,
                            &pale.data,
                            pale.outside
                        ),
                        (
                            clipped.x0,
                            clipped.y0,
                            clipped.width,
                            clipped.height,
                            &clipped.data,
                            clipped.outside
                        ),
                        "{what}: a pale mask clipped in place"
                    );

                    // Hardened, both read one threshold everywhere.
                    let (mut hard, mut hard_field) = (mask.clone(), field.clone());
                    hard.harden();
                    hard_field.harden();
                    canvas_pixels(&mut |x, y| {
                        assert_eq!(
                            hard.at(x, y),
                            hard_field.at(x, y),
                            "{what}: hard at ({x}, {y})"
                        );
                    });

                    // A run of images inside the canvas asks the same question of
                    // either: whether the mask lands on coverage it holds.
                    for held in [30u8, 100, 230] {
                        let mut run = Fragments::new(origin.0 + 2, origin.1 + 3, 19, 14);
                        for row in 0..14 {
                            for col in (row % 3..19).step_by(3) {
                                run.add(origin.0 + 2 + col, origin.1 + 3 + row, Color::WHITE, held);
                            }
                        }
                        assert_eq!(
                            run.would_overlap(&mask),
                            run.would_overlap(&field),
                            "{what}: a run holding {held}"
                        );
                    }

                    // The canvas's own consumers.
                    for format in formats {
                        let colour = Color::rgb(200, 60, 20);
                        let backdrop = Color {
                            r: 30,
                            g: 140,
                            b: 210,
                            a: 160,
                        };
                        let canvas = Canvas::new(width, height, format, backdrop)
                            .at_origin(origin.0, origin.1);
                        for mode in [BlendMode::Normal, BlendMode::Multiply] {
                            for alpha in [1.0, 0.6] {
                                let case = format!("{what}, {format:?}, {mode:?}, alpha {alpha}");
                                let (mut through, mut by_field) = (canvas.clone(), canvas.clone());
                                through.fill_mask_inked(&mask, colour, None, alpha, mode);
                                by_field.fill_mask_inked(&field, colour, None, alpha, mode);
                                assert_eq!(through.data, by_field.data, "{case}: a fill");

                                let mut cell = Canvas::new(13, 9, format, Color::TRANSPARENT);
                                cell.fill_mask(&varied(0, 0, 13, 9, 0, 4), colour, 1.0);
                                let at = (origin.0 + 4, origin.1 + 3);
                                let (mut through, mut by_field) = (canvas.clone(), canvas.clone());
                                through.composite(&cell, at, alpha, mode, Some(&mask), None);
                                by_field.composite(&cell, at, alpha, mode, Some(&field), None);
                                assert_eq!(through.data, by_field.data, "{case}: a composite");
                                cases += 1;
                            }
                        }
                        if format.has_alpha() {
                            let (mut through, mut by_field) = (canvas.clone(), canvas.clone());
                            through.knock_out(&mask);
                            by_field.knock_out(&field);
                            assert_eq!(
                                through.data, by_field.data,
                                "{what}, {format:?}: a knockout"
                            );
                        }
                    }
                }
            }
        }
        assert_eq!(
            cases,
            canvases.len() * boxes.len() * 5 * formats.len() * 4,
            "every case ran"
        );

        // A mask reading zero outside itself reads zero there still.
        let clip = varied(2, 3, 4, 5, 0, 2);
        for (x, y) in [
            (1, 3),
            (6, 3),
            (2, 2),
            (2, 8),
            (i32::MIN, i32::MAX),
            (i32::MAX, i32::MIN),
        ] {
            assert_eq!(clip.at(x, y), 0, "at ({x}, {y})");
        }
    }

    /// The product of two masks that both read a value outside themselves
    /// stands over the smallest rectangle holding both, reads the product of
    /// the two outsides beyond it and the product of the two everywhere
    /// inside; a mask of no pixels holds nothing, so the product takes the
    /// other's rectangle, and two of them hold no pixels at the first one's
    /// corner.
    #[test]
    fn a_product_of_two_masks_reading_outside_themselves_holds_both() {
        let a = varied(2, 3, 4, 5, 100, 6);
        let b = varied(10, -4, 3, 2, 255, 7);
        let both = a.intersect(&b);
        assert_eq!((both.x0, both.y0, both.width, both.height), (2, -4, 11, 12));
        assert_eq!(both.outside, product(100, 255));
        for y in -8..14 {
            for x in -3..18 {
                assert_eq!(
                    both.at(x, y),
                    product(a.at(x, y), b.at(x, y)),
                    "at ({x}, {y})"
                );
            }
        }

        let blank = Mask::empty(40, 40, 0, 0).with_outside(9);
        for (one, two) in [(&a, &blank), (&blank, &a)] {
            let held = one.intersect(two);
            assert_eq!((held.x0, held.y0, held.width, held.height), (2, 3, 4, 5));
            assert_eq!(held.outside, product(100, 9));
            for y in 0..10 {
                for x in 0..8 {
                    assert_eq!(held.at(x, y), product(a.at(x, y), 9), "at ({x}, {y})");
                }
            }
        }
        let none = blank.intersect(&Mask::empty(-7, 1, 3, 0).with_outside(77));
        assert_eq!((none.x0, none.y0, none.width, none.height), (40, 40, 0, 0));
        assert!(none.data.is_empty());
        assert_eq!(none.at(0, 0), product(9, 77));
    }

    /// Ruling 4's claim under the cells, checked rather than assumed: the
    /// order edges arrive in cannot change a pixel.
    ///
    /// Integer accumulation is order-independent by construction, and the
    /// construction is worth naming, because if any of it were floating-point
    /// the claim would be false. An edge's crossings land in the lanes of the
    /// cells by position alone; the walk takes each cell's lanes
    /// independently, and a lane holding several crossings sums those at one
    /// offset before it applies them, so crossings at the *same* x come to the
    /// same winding in either order. Every pixel's total is a count of exact
    /// 1/256 units in an `i32` that cannot pass 4 096. No float, no
    /// saturation, no reordering hazard.
    ///
    /// These two paths hold the same subpaths in the opposite order, so the
    /// edge list is a permutation, and the vertical edges are deliberately
    /// shared so equal-x crossings actually occur.
    #[test]
    fn the_order_edges_arrive_in_does_not_change_a_pixel() {
        let (a, b) = ((2.0, 3.0, 6.0, 9.0), (2.0, 7.0, 6.0, 9.0));
        let mut first = Path::new();
        first.rect(a.0, a.1, a.2, a.3);
        first.rect(b.0, b.1, b.2, b.3);
        let mut second = Path::new();
        second.rect(b.0, b.1, b.2, b.3);
        second.rect(a.0, a.1, a.2, a.3);

        for rule in [FillRule::NonZero, FillRule::EvenOdd] {
            let one = fill(&first, rule, 0, 0, 16, 16, 0.1, None);
            let two = fill(&second, rule, 0, 0, 16, 16, 0.1, None);
            assert_eq!(one.data, two.data, "ruling 4, under {rule:?}");
        }

        // And with the overlap at a fractional x, where the spans really do
        // land inside a pixel rather than on its boundary.
        let mut first = Path::new();
        first.rect(1.25, 2.5, 7.75, 6.5);
        first.rect(5.125, 4.75, 4.5, 8.25);
        let mut second = Path::new();
        second.rect(5.125, 4.75, 4.5, 8.25);
        second.rect(1.25, 2.5, 7.75, 6.5);
        for rule in [FillRule::NonZero, FillRule::EvenOdd] {
            assert_eq!(
                fill(&first, rule, 0, 0, 16, 16, 0.1, None).data,
                fill(&second, rule, 0, 0, 16, 16, 0.1, None).data,
                "ruling 4 at a fractional overlap, under {rule:?}"
            );
        }
    }

    /// The row range the sweep runs over, including the boundary case the
    /// active-edge list is most likely to get wrong: a shape whose last edge
    /// ends exactly on a row boundary.
    #[test]
    fn the_rows_a_shape_reaches_include_the_one_it_ends_on() {
        let rows = |y: f64, h: f64, y0: i32, height: u32| {
            let mut path = Path::new();
            path.rect(2.0, y, 4.0, h);
            let mut edges = Vec::new();
            for poly in &flatten(&path, 0.1) {
                build_edges(poly, &mut edges);
            }
            edges.sort_by_key(|edge| edge.top);
            active_rows(&edges, y0, height)
        };

        // Exactly on both boundaries: rows 3 through 6 inclusive.
        assert_eq!(rows(3.0, 4.0, 0, 20), 3..7);
        // Past them by more than a sub-scanline, so the shape reaches one more
        // row at each end.
        assert_eq!(rows(2.9, 4.15, 0, 20), 2..8);
        // Past them by *less* than a sub-scanline: an edge's top snaps up to
        // the next sample, so a sliver thinner than 1/16 px contributes to no
        // row at all — which is what the row loop already did, and the point
        // of computing the range from the edges rather than from the geometry.
        assert_eq!(rows(2.99, 4.02, 0, 20), 3..8);
        // Fractional, and inside a single row.
        assert_eq!(rows(3.25, 0.5, 0, 20), 3..4);
        // Clamped to the region, from both directions.
        assert_eq!(rows(3.0, 4.0, 5, 20), 0..2);
        assert_eq!(rows(3.0, 4.0, -10, 20), 13..17);
        assert_eq!(rows(3.0, 4.0, 100, 20), 0..0, "entirely above the region");
        assert_eq!(rows(300.0, 4.0, 0, 20), 20..20, "entirely below it");
        assert!(active_rows(&[], 0, 20).is_empty());
    }

    #[test]
    fn degenerate_geometry_produces_an_empty_mask() {
        let empty = fill(&Path::new(), FillRule::NonZero, 0, 0, 4, 4, 0.1, None);
        assert!(empty.data.iter().all(|&v| v == 0));

        // A zero-area region is legal and costs nothing.
        let none = fill(&Path::new(), FillRule::NonZero, 0, 0, 0, 0, 0.1, None);
        assert!(none.data.is_empty());

        // A path entirely outside the region contributes nothing.
        let mut far = Path::new();
        far.rect(1000.0, 1000.0, 10.0, 10.0);
        let outside = fill(&far, FillRule::NonZero, 0, 0, 8, 8, 0.1, None);
        assert!(outside.data.iter().all(|&v| v == 0));
    }

    #[test]
    fn enormous_coordinates_do_not_hang_or_overflow() {
        let mut path = Path::new();
        path.move_to(-1e9, -1e9);
        path.line_to(1e9, -1e9);
        path.line_to(1e9, 1e9);
        path.close();
        let mask = fill(&path, FillRule::NonZero, 0, 0, 8, 8, 0.1, None);
        assert_eq!(mask.data.len(), 64);
    }

    /// **A fill orders nothing** (P-07.3's guard).
    ///
    /// Comparisons made ordering edges or crossings are counted, and on every
    /// closed path the harness generates — overlapping, self-intersecting,
    /// crowded, slivers, far, stroked — and on the 300 triangles of the
    /// `render/300 triangles` benchmark page at 150 dpi there are none, so
    /// there are at most as many as the crossings sharing a pixel on one
    /// sub-scanline. Those are counted beside them, and must be what the
    /// reference sweep's own arithmetic says they are: crowded lanes are met
    /// and bucketed, not compared.
    #[test]
    fn a_fill_sorts_nothing() {
        use crate::fill_reference::{census, generate, region, Class, Stream, CLASSES};

        let mut s = Stream::new(0x736f_7274);
        let (mut fills, mut crowded) = (0u64, 0u64);
        for class in CLASSES {
            if class == Class::Dropped {
                continue;
            }
            for index in 0..24 {
                let path = generate(class, index, &mut s);
                let tolerance = s.range(0.05, 0.5);
                let (x0, y0, width, height) = region(&mut s);
                for rule in [FillRule::NonZero, FillRule::EvenOdd] {
                    count::take();
                    let _ = fill(&path, rule, x0, y0, width, height, tolerance, None);
                    let count::Counts {
                        compared, shared, ..
                    } = count::take();
                    assert_eq!(
                        compared, 0,
                        "{class:?} path {index} under {rule:?}: a closed path compared"
                    );
                    let met = census(&path, rule, x0, y0, width, height, tolerance);
                    assert_eq!(
                        shared, met.shared,
                        "{class:?} path {index} under {rule:?}: the crowded lanes are the                          crossings the sweep finds sharing a pixel"
                    );
                    fills += 1;
                    crowded += shared;
                }
            }
        }
        assert_eq!(fills, 6 * 24 * 2, "every closed class filled");
        assert!(crowded > 10_000, "{crowded} crossings shared a lane");

        let scale = 150.0 / 72.0;
        let mut crowded = 0u64;
        for index in 0..300 {
            let step = f64::from(index);
            let (x, y) = (20.0 + (step * 37.0) % 520.0, 20.0 + (step * 61.0) % 760.0);
            let size = 18.0 + (step * 7.0) % 44.0;
            let corners = [
                (x, y),
                (x + size * 0.93, y + size * 0.37),
                (x + size * 0.41, y + size),
            ];
            let mut path = Path::new();
            for (at, (cx, cy)) in corners.iter().enumerate() {
                let (px, py) = (cx * scale, (842.0 - cy) * scale);
                if at == 0 {
                    path.move_to(px, py);
                } else {
                    path.line_to(px, py);
                }
            }
            path.close();
            let (left, top) = ((x * scale).floor(), ((842.0 - y - size) * scale).floor());
            let side = (size * scale).ceil() as u32 + 2;
            count::take();
            let mask = fill(
                &path,
                FillRule::NonZero,
                left as i32,
                top as i32,
                side,
                side,
                0.2,
                None,
            );
            assert!(mask.data.contains(&255), "triangle {index} drew its inside");
            let count::Counts {
                compared, shared, ..
            } = count::take();
            assert_eq!(compared, 0, "triangle {index} compared");
            crowded += shared;
        }
        assert!(crowded > 0, "the triangles' corners crowd a lane");
    }

    /// **A fill after the first allocates nothing but its mask** (P-07.4's
    /// guard).
    ///
    /// Every allocation a fill makes besides its mask is in its scratch's
    /// buffers, and a buffer that grew made at least one, so the count of
    /// buffers grown is zero exactly when a fill allocated nothing there. The
    /// first fill here is the union of every path after it over the union of
    /// their regions, which needs as much of every buffer as any of them —
    /// points, edges, rows, the active set, the cells and the crowded lanes'
    /// lists. Each fill after it, through the same scratch, under either rule,
    /// grows nothing and draws what a fill through a scratch of its own draws.
    #[test]
    fn a_fill_after_the_first_allocates_no_scratch() {
        use crate::fill_reference::{census, generate, region, Class, Stream, CLASSES};
        use crate::geom::Verb;

        const TOLERANCE: f64 = 0.2;
        let mut s = Stream::new(0x73_6372_6174_6368);
        let mut cases = Vec::new();
        for class in CLASSES {
            if class == Class::Dropped {
                continue;
            }
            for index in 0..12 {
                let path = generate(class, index, &mut s);
                cases.push((path, region(&mut s)));
            }
        }

        let mut union = Path::new();
        let (mut left, mut top, mut right, mut bottom) = (0i32, 0i32, 0i32, 0i32);
        for (path, (x0, y0, width, height)) in &cases {
            for verb in path.verbs() {
                match *verb {
                    Verb::MoveTo(p) => union.move_to(p.x, p.y),
                    Verb::LineTo(p) => union.line_to(p.x, p.y),
                    Verb::QuadTo(c, p) => union.quad_to(c.x, c.y, p.x, p.y),
                    Verb::CurveTo(a, b, p) => union.curve_to(a.x, a.y, b.x, b.y, p.x, p.y),
                    Verb::Close => union.close(),
                }
            }
            left = left.min(*x0);
            top = top.min(*y0);
            right = right.max(x0 + *width as i32);
            bottom = bottom.max(y0 + *height as i32);
        }
        let (width, height) = ((right - left) as u32, (bottom - top) as u32);
        assert_eq!(
            census(
                &union,
                FillRule::NonZero,
                left,
                top,
                width,
                height,
                TOLERANCE
            )
            .unclosed,
            0,
            "the union closes, so no row of it is swept"
        );

        let mut scratch = FillScratch::new();
        count::take();
        let all = fill_with(
            &mut scratch,
            &union,
            FillRule::NonZero,
            left,
            top,
            width,
            height,
            TOLERANCE,
            None,
        );
        assert!(all.data.contains(&255), "the union draws");
        assert!(
            count::take().allocated > 0,
            "the first fill sized the scratch"
        );

        for rule in [FillRule::NonZero, FillRule::EvenOdd] {
            for (index, (path, (x0, y0, width, height))) in cases.iter().enumerate() {
                let alone = fill(path, rule, *x0, *y0, *width, *height, TOLERANCE, None);
                count::take();
                let reused = fill_with(
                    &mut scratch,
                    path,
                    rule,
                    *x0,
                    *y0,
                    *width,
                    *height,
                    TOLERANCE,
                    None,
                );
                assert_eq!(
                    count::take().allocated,
                    0,
                    "fill {index} under {rule:?} grew the scratch"
                );
                assert_eq!(
                    reused.data, alone.data,
                    "fill {index} under {rule:?}: the scratch reached a pixel"
                );
            }
        }
    }

    /// **An interior run is one slice fill** (P-07.5's guard).
    ///
    /// Conversions are counted: a cell holding a crossing turned into its byte
    /// is one, and a run of cells holding none, written at once, is one. So a
    /// row costs the cells holding a crossing plus one for each run before,
    /// between and after them, whatever its width — and that is the count,
    /// exactly, on every closed path the harness generates, against the cells
    /// and the runs the reference sweep's own arithmetic finds. A rectangle
    /// 193 pixels wide costs five a row: its two edge cells and the three runs
    /// around them.
    #[test]
    fn an_interior_run_is_one_slice_fill() {
        use crate::fill_reference::{
            census, fill_reference, generate, region, Class, Stream, CLASSES,
        };

        let mut rect = Path::new();
        rect.rect(3.5, 2.25, 193.0, 58.5);
        count::take();
        let mask = fill(&rect, FillRule::NonZero, 0, 0, 200, 64, 0.1, None);
        assert_eq!(
            count::take().converted,
            5 * 59,
            "59 rows, each two edge cells and three runs"
        );
        assert_eq!(
            mask.data,
            fill_reference(&rect, FillRule::NonZero, 0, 0, 200, 64, 0.1, None).data
        );
        assert_eq!(mask.at(100, 30), 255, "the interior run is whole");

        let mut s = Stream::new(0x7275_6e73);
        let mut runs = 0;
        for class in CLASSES {
            if class == Class::Dropped {
                continue;
            }
            for index in 0..24 {
                let path = generate(class, index, &mut s);
                let tolerance = s.range(0.05, 0.5);
                let (x0, y0, width, height) = region(&mut s);
                for rule in [FillRule::NonZero, FillRule::EvenOdd] {
                    count::take();
                    let _ = fill(&path, rule, x0, y0, width, height, tolerance, None);
                    let converted = count::take().converted;
                    let met = census(&path, rule, x0, y0, width, height, tolerance);
                    assert_eq!(
                        converted,
                        met.cells + met.runs,
                        "{class:?} path {index} under {rule:?}: the cells holding a \
                         crossing and one a run"
                    );
                    runs += met.runs;
                }
            }
        }
        assert!(runs > 1000, "{runs} runs written whole");
    }

    /// The walk over a cell's single lanes one lane at a time: each lane
    /// holding one crossing taken by its set bit in turn, with the rule read
    /// by `inside`. What [`single_lanes`] is held to.
    fn lane_by_lane(winding: &mut [i32; 16], cell: &Cell, rule: FillRule) -> (i32, i32) {
        let (mut total, mut inside_count) = (0i32, 0i32);
        let mut lanes = cell.lanes & !cell.multi;
        while lanes != 0 {
            let lane = lanes.trailing_zeros() as usize;
            lanes &= lanes - 1;
            let before = i32::from(inside(winding[lane], rule));
            winding[lane] += i32::from(cell.w[lane]);
            let change = i32::from(inside(winding[lane], rule)) - before;
            total += (256 - i32::from(cell.off[lane])) * change;
            inside_count += change;
        }
        (total, inside_count)
    }

    /// **The sixteen-wide walk of a cell's lanes is the walk lane by lane**
    /// (P-07.9a).
    ///
    /// Random cells — lanes holding one crossing, several and none, every
    /// offset, windings of either sign and of several crossings summed —
    /// over windings coming in from the left at every sign and size, the
    /// extremes an `i32` can carry among them, under both rules: the total,
    /// the count inside and every lane's winding after it, against the walk
    /// that takes each set lane in turn. A lane holding several crossings, or
    /// none, is left alone by both, whatever its first crossing's fields say.
    #[test]
    fn the_sixteen_lane_walk_is_the_walk_lane_by_lane() {
        use crate::fill_reference::Stream;

        let mut s = Stream::new(0x6c61_6e65);
        let mut walked = 0u64;
        for trial in 0..20_000u32 {
            let lanes = s.below(1 << 16) as u16;
            let multi = match s.below(3) {
                0 => 0,
                1 => lanes,
                _ => lanes & s.below(1 << 16) as u16,
            };
            let mut cell = Cell {
                lanes,
                multi,
                ..Cell::CLEAR
            };
            for (w, off) in cell.w.iter_mut().zip(cell.off.iter_mut()) {
                *w = match s.below(4) {
                    0 => -1,
                    1 => 1,
                    _ => (s.below(256) as u8) as i8,
                };
                *off = s.below(256) as u8;
            }
            let mut incoming = [0i32; 16];
            for value in &mut incoming {
                *value = match s.below(6) {
                    0 => 0,
                    1 => s.below(7) as i32 - 3,
                    2 => i32::MIN + 128 + s.below(64) as i32,
                    3 => i32::MAX - 128 - s.below(64) as i32,
                    _ => s.below(1 << 20) as i32 - (1 << 19),
                };
            }
            for rule in [FillRule::NonZero, FillRule::EvenOdd] {
                let (mut wide, mut narrow) = (incoming, incoming);
                let got = single_lanes(&mut wide, &cell, rule_mask(rule));
                let want = lane_by_lane(&mut narrow, &cell, rule);
                assert_eq!(got, want, "trial {trial} under {rule:?}: total and inside");
                assert_eq!(wide, narrow, "trial {trial} under {rule:?}: the windings");
                walked += u64::from((lanes & !multi).count_ones());
            }
        }
        assert!(walked > 100_000, "{walked} single lanes walked");
    }

    /// The rule's mask reads every winding as the rule does: non-zero and
    /// odd, the negative windings and both ends of an `i32` among them.
    #[test]
    fn the_rule_mask_reads_a_winding_as_the_rule_does() {
        let windings = (-1000..=1000).chain([i32::MIN, i32::MIN + 1, i32::MAX - 1, i32::MAX]);
        for winding in windings {
            for rule in [FillRule::NonZero, FillRule::EvenOdd] {
                assert_eq!(
                    inside_by(winding, rule_mask(rule)),
                    i32::from(inside(winding, rule)),
                    "{winding} under {rule:?}"
                );
            }
        }
    }

    /// A hand-made edge: `x` and `dxdy` as an edge stores them, from
    /// sub-scanline `top` down `height` of them.
    fn edge_of(x: i64, dxdy: i64, top: i32, height: i32) -> Edge {
        Edge {
            top,
            bottom: top.saturating_add(height),
            x,
            dxdy,
            winding: 1,
            plain: unsaturating(x, dxdy, i64::from(height)),
        }
    }

    /// The crossings `each_crossing` hands over for `from..to`, beside
    /// `crossing` at each of the same sub-scanlines.
    fn stepped_and_direct(edge: &Edge, from: i64, to: i64) -> (Vec<(i64, i64)>, Vec<(i64, i64)>) {
        let mut stepped = Vec::new();
        each_crossing(edge, from, to, |sub, x| stepped.push((sub, x)));
        let direct = (from..to).map(|sub| (sub, crossing(edge, sub))).collect();
        (stepped, direct)
    }

    /// **A stepped crossing is the saturating one wherever it is taken**
    /// (P-07.9a).
    ///
    /// An edge none of whose crossings can saturate is stepped, one add a
    /// sub-scanline; every other edge keeps `crossing`. Over random edges at
    /// every scale — slopes and positions near the bound, past it and far
    /// inside it, either sign, tall and short — each window of sixteen
    /// sub-scanlines a row takes, at the edge's top, at its foot and between,
    /// hands over exactly `crossing`'s value at each. The bound itself is
    /// exact: an edge that reaches `i64::MAX` is stepped, and one a unit
    /// further is not, at either sign of `x`.
    #[test]
    fn a_stepped_crossing_is_the_saturating_one_wherever_it_is_taken() {
        use crate::fill_reference::Stream;

        let mut s = Stream::new(0x7374_6570);
        let (mut stepped_edges, mut kept_edges) = (0u32, 0u32);
        for trial in 0..4000u32 {
            let scale = s.below(63) as u32;
            let magnitude = |s: &mut Stream| -> i64 {
                let value = s.below(1u64 << scale.min(62)) as i64;
                if s.coin() {
                    -value
                } else {
                    value
                }
            };
            let x = magnitude(&mut s);
            let dxdy = magnitude(&mut s);
            let top = s.below(1 << 20) as i32 - (1 << 19);
            let span = s.below(31);
            let height = 1 + s.below(1 << span) as i32;
            let edge = edge_of(x, dxdy, top, height);
            if edge.plain {
                stepped_edges += 1;
            } else {
                kept_edges += 1;
            }
            let (first, last) = (i64::from(edge.top), i64::from(edge.bottom));
            let middle = first + (last - first) / 2;
            for from in [first, middle, (last - 16).max(first)] {
                let to = (from + 16).min(last);
                let (stepped, direct) = stepped_and_direct(&edge, from, to);
                assert_eq!(
                    stepped, direct,
                    "trial {trial}: x {x}, dxdy {dxdy}, height {height}, from {from}"
                );
            }
        }
        assert!(stepped_edges > 1000, "{stepped_edges} edges stepped");
        assert!(kept_edges > 100, "{kept_edges} edges kept their saturation");

        // The bound, exactly, with the edge's foot in the window.
        let half = SUBSTEP / 2;
        for sign in [1i64, -1] {
            let (dxdy, height) = (sign * 3_000_000_007, 1_000_003i32);
            let reach = dxdy.abs() * i64::from(height) + half;
            let at = edge_of(sign * (i64::MAX - reach), dxdy, -500, height);
            assert!(at.plain, "an edge reaching i64::MAX steps, sign {sign}");
            let past = edge_of(sign * (i64::MAX - reach + 1), dxdy, -500, height);
            assert!(!past.plain, "one a unit further does not, sign {sign}");
            for edge in [&at, &past] {
                let last = i64::from(edge.bottom);
                let (stepped, direct) = stepped_and_direct(edge, last - 16, last);
                assert_eq!(stepped, direct, "at the bound, sign {sign}");
            }
        }
    }

    /// **An edge that could saturate keeps the saturating crossing, and the
    /// fill keeps the sweep's bytes.** A triangle with a corner a thousand
    /// million million pixels out has edges whose slope and position no
    /// `i64` sum can carry, so `build_edges` marks them; the fill through
    /// them is the reference sweep's, byte for byte, under both rules, and
    /// so is a page-sized one whose every edge steps.
    #[test]
    fn an_edge_that_could_saturate_keeps_the_saturating_crossing() {
        use crate::fill_reference::fill_reference;

        let mut far = Path::new();
        far.move_to(3.5, 2.25);
        far.line_to(1e15, 9.75);
        far.line_to(5.5, 30.5);
        far.close();
        let mut near = Path::new();
        near.move_to(3.5, 2.25);
        near.line_to(611.25, 9.75);
        near.line_to(5.5, 790.5);
        near.close();

        let plainness = |path: &Path| -> Vec<bool> {
            let mut edges = Vec::new();
            for poly in &flatten(path, 0.1) {
                build_edges(poly, &mut edges);
            }
            edges.iter().map(|edge| edge.plain).collect()
        };
        assert!(
            plainness(&far).contains(&false),
            "the far corner's edges are marked"
        );
        assert!(
            plainness(&near).iter().all(|plain| *plain),
            "a page's edges all step"
        );
        for path in [&far, &near] {
            for rule in [FillRule::NonZero, FillRule::EvenOdd] {
                let mask = fill(path, rule, 0, 0, 64, 40, 0.1, None);
                assert!(mask.data.contains(&255), "{rule:?}: the fixture draws");
                assert_eq!(
                    mask.data,
                    fill_reference(path, rule, 0, 0, 64, 40, 0.1, None).data,
                    "{rule:?}"
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // Stopping the sweep (gap 15).
    // -----------------------------------------------------------------------

    /// A predicate that answers `false` until its `at`-th question and `true`
    /// from then on, counting how many times it was asked.
    ///
    /// Deterministic by construction: it fires on the Nth *question*, never on
    /// a clock. A stop test written against elapsed time is flaky by
    /// construction — the sweep either reached the row before the deadline or
    /// it did not — and one in CI fails for reasons nobody can reproduce.
    struct StopAt {
        at: u32,
        calls: std::cell::Cell<u32>,
    }

    impl StopAt {
        fn new(at: u32) -> StopAt {
            StopAt {
                at,
                calls: std::cell::Cell::new(0),
            }
        }

        fn ask(&self) -> bool {
            self.calls.set(self.calls.get().saturating_add(1));
            self.calls.get() >= self.at
        }
    }

    /// A tall bar filling every row of a tall region, so the sweep's step
    /// count and the row index are the same number.
    fn tall_bar() -> Path {
        let mut path = Path::new();
        path.rect(5.0, 0.0, 30.0, 400.0);
        path
    }

    /// Milestone 2: the row loop stops when asked, and what comes back is the
    /// part of the shape it had already drawn.
    ///
    /// The partial mask is the point. Returning an empty one would be simpler
    /// and would pass any test that only asked whether the sweep stopped —
    /// which is why both halves are asserted here: ink above the cut, none at
    /// it or below.
    #[test]
    fn a_stopped_fill_keeps_the_rows_it_had_already_swept() {
        let path = tall_bar();
        let whole = fill(&path, FillRule::NonZero, 0, 0, 40, 400, 0.1, None);
        assert_eq!(whole.at(20, 399), 255, "the shape reaches the last row");

        // The fourth question is asked at row 3 x STOP_EVERY, and stops there.
        let stop = StopAt::new(4);
        let ask = || stop.ask();
        let partial = fill(&path, FillRule::NonZero, 0, 0, 40, 400, 0.1, Some(&ask));

        let cut = 3 * STOP_EVERY as i32;
        assert_eq!(partial.at(20, cut - 1), 255, "the rows before the cut drew");
        assert_eq!(partial.at(20, cut), 0, "the row it stopped on did not");
        assert_eq!(partial.at(20, 399), 0, "nor did anything below it");
        assert_eq!(
            partial.data.iter().filter(|&&v| v != 0).count(),
            cut as usize * 30,
            "exactly the rows above the cut, at the bar's own width"
        );
    }

    /// The hook is not an input to the coverage: a predicate that never
    /// answers yes leaves every byte where `None` left it (ruling 4).
    ///
    /// And the period is a band rather than a row — the cost the risk row in
    /// the gap plan is about. Derived from the constant rather than written
    /// out, so tuning `STOP_EVERY` retunes the assertion with it.
    #[test]
    fn a_predicate_that_never_answers_yes_changes_no_pixel() {
        let path = tall_bar();
        let stop = StopAt::new(u32::MAX);
        let ask = || stop.ask();

        let hooked = fill(&path, FillRule::NonZero, 0, 0, 40, 400, 0.1, Some(&ask));
        let bare = fill(&path, FillRule::NonZero, 0, 0, 40, 400, 0.1, None);

        assert_eq!(hooked.data, bare.data, "the same input, the same bytes");
        assert_eq!(
            stop.calls.get() as usize,
            400usize.div_ceil(STOP_EVERY),
            "one question per band of {STOP_EVERY} rows, not one per row"
        );
    }

    /// A shape that reaches no row is never asked at all, so a page full of
    /// off-canvas geometry costs no questions either.
    #[test]
    fn a_fill_with_nothing_to_sweep_asks_nothing() {
        let mut far = Path::new();
        far.rect(1000.0, 1000.0, 10.0, 10.0);
        let stop = StopAt::new(1);
        let ask = || stop.ask();

        let mask = fill(&far, FillRule::NonZero, 0, 0, 8, 8, 0.1, Some(&ask));
        assert!(mask.data.iter().all(|&v| v == 0));
        assert_eq!(stop.calls.get(), 0, "no rows, no questions");
    }

    // ---- The centre rule (RC-11.3) ----------------------------------------

    /// A circle of four cubics about `(cx, cy)`.
    fn circle(path: &mut Path, (cx, cy): (f64, f64), r: f64) {
        const K: f64 = 0.552_284_75;
        path.move_to(cx + r, cy);
        path.curve_to(cx + r, cy + K * r, cx + K * r, cy + r, cx, cy + r);
        path.curve_to(cx - K * r, cy + r, cx - r, cy + K * r, cx - r, cy);
        path.curve_to(cx - r, cy - K * r, cx - K * r, cy - r, cx, cy - r);
        path.curve_to(cx + K * r, cy - r, cx + r, cy - K * r, cx + r, cy);
        path.close();
    }

    /// A rectangle whose middle sub-scanline in row 0 does not close: its top
    /// edge is the one that rises a single ulp across `y = ½`, which
    /// `build_edges` drops, so on that sub-scanline only the left edge
    /// crosses.
    fn unclosed_middle() -> Path {
        let mut path = Path::new();
        path.move_to(1.0, 0.5);
        path.line_to(6.0, f64::from_bits(0.5f64.to_bits() + 1));
        path.line_to(6.0, 5.0);
        path.line_to(1.0, 5.0);
        path.close();
        path
    }

    /// **The centre rule over a tile is the page under it** (ruling 5,
    /// RC-11.3).
    ///
    /// Rectangles whose edges stand at sixteenths, on and between the tiles'
    /// boundaries and through rows and columns of centres, one reaching past
    /// the page's left and top edges; a turned triangle; a circle of four
    /// cubics; an even-odd ring; a rectangle whose middle sub-scanline does
    /// not close; and a stroke's outline swept from the scratch — each filled
    /// over the whole page and over every tile of every width from one pixel
    /// to four at every column, across three bands of rows, row 0 among
    /// them: every pixel of every tile is the page's. A centre is one unit
    /// of one sub-scanline in the page's device pixels, and a tile's left
    /// edge is a whole pixel, so the crossings left of a tile fold into the
    /// winding it starts with and none moves.
    #[test]
    fn the_centre_rule_on_a_tile_is_the_page_s() {
        const WIDE: u32 = 12;
        const TALL: u32 = 10;
        let mut shapes: Vec<(&str, Path, FillRule)> = Vec::new();
        let mut rects = Path::new();
        rects.rect(2.0 + 5.0 / 16.0, 1.5, 4.0 + 3.0 / 16.0, 6.0);
        rects.rect(8.5, 0.25, 2.0, 9.0);
        shapes.push(("rectangles at sixteenths", rects, FillRule::NonZero));
        let mut past = Path::new();
        past.rect(-3.0, -2.0, 5.5, 4.0);
        shapes.push(("a rectangle past the corner", past, FillRule::NonZero));
        let mut triangle = Path::new();
        triangle.move_to(0.7, 9.3);
        triangle.line_to(11.2, 0.4);
        triangle.line_to(6.1, 8.8);
        triangle.close();
        shapes.push(("a turned triangle", triangle, FillRule::NonZero));
        let mut disc = Path::new();
        circle(&mut disc, (6.2, 5.1), 3.7);
        shapes.push(("a circle", disc, FillRule::NonZero));
        let mut ring = Path::new();
        circle(&mut ring, (5.9, 4.8), 4.4);
        circle(&mut ring, (5.9, 4.8), 2.1);
        shapes.push(("an even-odd ring", ring, FillRule::EvenOdd));
        shapes.push(("an unclosed middle", unclosed_middle(), FillRule::NonZero));

        let bands = [(0, TALL), (3, 4), (TALL as i32 - 1, 1)];
        for (name, path, rule) in &shapes {
            let page = fill_hard(path, *rule, 0, 0, WIDE, TALL, 0.1, None, HardEdge::Centre);
            assert!(page.data.contains(&255), "{name} holds a centre");
            assert!(page.data.iter().all(|&v| v == 0 || v == 255), "{name}");
            for x0 in 0..WIDE {
                for width in 1..=(WIDE - x0).min(4) {
                    for (y0, height) in bands {
                        let tile = fill_hard(
                            path,
                            *rule,
                            x0 as i32,
                            y0,
                            width,
                            height,
                            0.1,
                            None,
                            HardEdge::Centre,
                        );
                        for y in y0..y0 + height as i32 {
                            for x in x0 as i32..(x0 + width) as i32 {
                                assert_eq!(
                                    tile.at(x, y),
                                    page.at(x, y),
                                    "{name}: the tile at ({x0}, {y0}) {width} wide, pixel ({x}, {y})"
                                );
                            }
                        }
                    }
                }
            }
        }

        let mut line = Path::new();
        line.move_to(0.6, 8.7);
        line.line_to(11.3, 1.2);
        let style = crate::StrokeStyle {
            width: 1.3,
            cap: crate::LineCap::Round,
            ..crate::StrokeStyle::default()
        };
        let mut scratch = FillScratch::new();
        crate::stroke_with(&mut scratch, &line, &style, 0.1, None);
        let page = fill_outline_hard(&mut scratch, 0, 0, WIDE, TALL, None, HardEdge::Centre);
        assert!(page.data.contains(&255), "the stroke holds a centre");
        for x0 in 0..WIDE {
            for width in 1..=(WIDE - x0).min(4) {
                for (y0, height) in bands {
                    let tile = fill_outline_hard(
                        &mut scratch,
                        x0 as i32,
                        y0,
                        width,
                        height,
                        None,
                        HardEdge::Centre,
                    );
                    for y in y0..y0 + height as i32 {
                        for x in x0 as i32..(x0 + width) as i32 {
                            assert_eq!(
                                tile.at(x, y),
                                page.at(x, y),
                                "the stroke: the tile at ({x0}, {y0}) {width} wide, pixel ({x}, {y})"
                            );
                        }
                    }
                }
            }
        }
    }

    /// `HardEdge::Half` is the coverage hardened, whichever way it is asked
    /// for: a fill, a fill through a scratch and a stroke's outline, under
    /// both rules, are `harden` of the soft mask byte for byte — the rule
    /// turning anti-aliasing off has always applied.
    #[test]
    fn the_half_rule_is_the_coverage_hardened() {
        let mut path = Path::new();
        path.move_to(1.3, 2.7);
        path.curve_to(18.1, 0.4, 19.9, 16.5, 2.2, 17.1);
        path.close();
        path.rect(6.3, 4.1, 7.7, 8.25);
        let mut scratch = FillScratch::new();
        for rule in [FillRule::NonZero, FillRule::EvenOdd] {
            let mut hardened = fill(&path, rule, 0, 0, 20, 20, 0.1, None);
            assert!(hardened.data.iter().any(|v| *v != 0 && *v != 255));
            hardened.harden();
            let hard = fill_hard(&path, rule, 0, 0, 20, 20, 0.1, None, HardEdge::Half);
            assert_eq!(hard.data, hardened.data, "{rule:?}");
            let through = fill_hard_with(
                &mut scratch,
                &path,
                rule,
                0,
                0,
                20,
                20,
                0.1,
                None,
                HardEdge::Half,
            );
            assert_eq!(through.data, hardened.data, "{rule:?}, through a scratch");
        }

        let style = crate::StrokeStyle {
            width: 2.6,
            ..crate::StrokeStyle::default()
        };
        crate::stroke_with(&mut scratch, &path, &style, 0.1, None);
        let mut hardened = fill_outline(&mut scratch, 0, 0, 20, 20, None);
        hardened.harden();
        let hard = fill_outline_hard(&mut scratch, 0, 0, 20, 20, None, HardEdge::Half);
        assert_eq!(hard.data, hardened.data, "the stroke's outline");
    }

    /// A stroke's outline under the centre rule is the centre fill of the
    /// path the stroker returns, byte for byte, as its coverage is that
    /// path's coverage: the outline's edges are the ones filling that path
    /// makes. Every cap and join, a dash and a curve.
    #[test]
    fn the_centre_rule_outlines_as_it_fills() {
        let mut path = Path::new();
        path.move_to(2.2, 3.1);
        path.line_to(15.7, 4.9);
        path.curve_to(18.0, 12.0, 6.0, 17.5, 3.3, 11.8);
        let mut scratch = FillScratch::new();
        for (index, (cap, join, dashes)) in [
            (crate::LineCap::Butt, crate::LineJoin::Miter, vec![]),
            (crate::LineCap::Round, crate::LineJoin::Round, vec![]),
            (
                crate::LineCap::Square,
                crate::LineJoin::Bevel,
                vec![2.5, 1.25],
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let style = crate::StrokeStyle {
                width: 1.7,
                cap,
                join,
                dashes,
                ..crate::StrokeStyle::default()
            };
            crate::stroke_with(&mut scratch, &path, &style, 0.1, None);
            let swept = fill_outline_hard(&mut scratch, 0, 0, 20, 20, None, HardEdge::Centre);
            let outline = crate::stroke(&path, &style, 0.1, None);
            let filled = fill_hard(
                &outline,
                FillRule::NonZero,
                0,
                0,
                20,
                20,
                0.1,
                None,
                HardEdge::Centre,
            );
            assert_eq!(swept.data, filled.data, "style {index}");
            assert!(swept.data.contains(&255), "style {index} holds a centre");
        }
    }

    /// **A row whose middle sub-scanline does not close is read as the sweep
    /// reads it.** The rectangle of `unclosed_middle` has its top edge
    /// dropped, so on row 0's middle only its left edge crosses and the
    /// winding never returns: the sorted sweep opens a span there and never
    /// adds it, so row 0 holds no centre — where reading the cells' winding
    /// would paint row 0 from column 1 to the region's end. Rows 1 to 4 close
    /// and hold the centres of columns 1 to 5, as the rectangle's corners
    /// say; and the sorted sweep of every row's middle agrees under both
    /// rules.
    #[test]
    fn a_centre_on_a_sub_scanline_that_does_not_close_is_read_as_the_sweep_reads_it() {
        use crate::fill_reference::centres_reference;

        let path = unclosed_middle();
        for rule in [FillRule::NonZero, FillRule::EvenOdd] {
            let mask = fill_hard(&path, rule, 0, 0, 10, 8, 0.1, None, HardEdge::Centre);
            for y in 0..8 {
                for x in 0..10 {
                    let held = (1..=5).contains(&x) && (1..=4).contains(&y);
                    assert_eq!(
                        mask.at(x, y),
                        if held { 255 } else { 0 },
                        "{rule:?}: pixel ({x}, {y})"
                    );
                }
            }
            assert_eq!(
                mask.data,
                centres_reference(&path, rule, 0, 0, 10, 8, 0.1).data,
                "{rule:?}"
            );
        }
    }

    /// **An edge through a row or a column of centres gives them to one
    /// side.** Two rectangles meeting at `x = 4.5`, two meeting at `y = 3.5`
    /// and two triangles meeting along the diagonal `x = y`, which passes
    /// through every pixel's centre on it: every pixel of the pair's union
    /// is painted by exactly one of the two — the right or lower shape, which
    /// the edge begins, takes the centres on it — and nothing outside.
    #[test]
    fn an_edge_through_a_row_of_centres_gives_them_to_one_side() {
        let rect = |x: f64, y: f64, w: f64, h: f64| {
            let mut path = Path::new();
            path.rect(x, y, w, h);
            path
        };
        let triangle = |points: [(f64, f64); 3]| {
            let mut path = Path::new();
            path.move_to(points[0].0, points[0].1);
            path.line_to(points[1].0, points[1].1);
            path.line_to(points[2].0, points[2].1);
            path.close();
            path
        };
        let pairs = [
            (rect(1.0, 1.0, 3.5, 6.0), rect(4.5, 1.0, 3.5, 6.0)),
            (rect(1.0, 1.0, 6.0, 2.5), rect(1.0, 3.5, 6.0, 3.5)),
            (
                triangle([(0.0, 0.0), (8.0, 8.0), (0.0, 8.0)]),
                triangle([(0.0, 0.0), (8.0, 0.0), (8.0, 8.0)]),
            ),
        ];
        for (index, (first, second)) in pairs.iter().enumerate() {
            let centres = |path: &Path| {
                fill_hard(
                    path,
                    FillRule::NonZero,
                    0,
                    0,
                    8,
                    8,
                    0.1,
                    None,
                    HardEdge::Centre,
                )
            };
            let (a, b) = (centres(first), centres(second));
            let mut union = first.clone();
            union.extend(second);
            let whole = centres(&union);
            for y in 0..8 {
                for x in 0..8 {
                    let painted = u32::from(a.at(x, y) == 255) + u32::from(b.at(x, y) == 255);
                    let expected = u32::from(whole.at(x, y) == 255);
                    assert_eq!(painted, expected, "pair {index}, pixel ({x}, {y})");
                }
            }
            assert!(whole.data.contains(&255), "pair {index}");
        }
        // The column of centres at x = 4.5 is the right rectangle's, the row
        // at y = 3.5 the lower one's, and the diagonal's the triangle right
        // of it.
        let right = fill_hard(
            &pairs[0].1,
            FillRule::NonZero,
            0,
            0,
            8,
            8,
            0.1,
            None,
            HardEdge::Centre,
        );
        assert_eq!(right.at(4, 2), 255);
        let lower = fill_hard(
            &pairs[1].1,
            FillRule::NonZero,
            0,
            0,
            8,
            8,
            0.1,
            None,
            HardEdge::Centre,
        );
        assert_eq!(lower.at(2, 3), 255);
        let upper_right = fill_hard(
            &pairs[2].1,
            FillRule::NonZero,
            0,
            0,
            8,
            8,
            0.1,
            None,
            HardEdge::Centre,
        );
        for d in 0..8 {
            assert_eq!(
                upper_right.at(d, d),
                255,
                "the diagonal's centre ({d}, {d})"
            );
        }
    }

    /// A stopped centre fill keeps the rows it had swept, as a stopped
    /// coverage fill does: the fourth question is asked at row
    /// `3 × STOP_EVERY` and stops there, and the rows above it hold the bar's
    /// thirty centres each.
    #[test]
    fn a_stopped_centre_fill_keeps_the_rows_it_had_already_swept() {
        let path = tall_bar();
        let whole = fill_hard(
            &path,
            FillRule::NonZero,
            0,
            0,
            40,
            400,
            0.1,
            None,
            HardEdge::Centre,
        );
        assert_eq!(
            whole.at(20, 399),
            255,
            "the bar's centres reach the last row"
        );
        let stop = StopAt::new(4);
        let ask = || stop.ask();
        let partial = fill_hard(
            &path,
            FillRule::NonZero,
            0,
            0,
            40,
            400,
            0.1,
            Some(&ask),
            HardEdge::Centre,
        );
        let cut = 3 * STOP_EVERY as i32;
        assert_eq!(partial.at(20, cut - 1), 255, "the rows before the cut drew");
        assert_eq!(partial.at(20, cut), 0, "the row it stopped on did not");
        assert_eq!(
            partial.data.iter().filter(|&&v| v != 0).count(),
            cut as usize * 30,
            "exactly the rows above the cut, the bar's thirty columns of centres"
        );
    }

    // ---- The any-part rule (RC-11.4) --------------------------------------

    /// **The any-part rule over a tile is the page under it** (ruling 5,
    /// RC-11.4).
    ///
    /// The centre rule's shapes, and with them slivers that only their edges
    /// reach — one between two sub-scanlines, one across the edge of two
    /// rows, a slanted one — points, one of them on a pixel's edge, and a
    /// stroke's outline thinner than a sub-scanline, its pieces kept: each
    /// filled over the whole page and over every tile of every width from one
    /// pixel to four at every column, across three bands of rows, and every
    /// pixel of every tile is the page's. Both halves of the rule read device
    /// pixels: the samples are the coverage's, and an edge's extent over a
    /// row is a function of the edge and the row alone, which a tile clips
    /// and does not move.
    #[test]
    fn the_any_part_rule_on_a_tile_is_the_page_s() {
        const WIDE: u32 = 12;
        const TALL: u32 = 10;
        let mut shapes: Vec<(&str, Path, FillRule)> = Vec::new();
        let mut rects = Path::new();
        rects.rect(2.0 + 5.0 / 16.0, 1.5, 4.0 + 3.0 / 16.0, 6.0);
        rects.rect(8.5, 0.25, 2.0, 9.0);
        shapes.push(("rectangles at sixteenths", rects, FillRule::NonZero));
        let mut past = Path::new();
        past.rect(-3.0, -2.0, 5.5, 4.0);
        shapes.push(("a rectangle past the corner", past, FillRule::NonZero));
        let mut triangle = Path::new();
        triangle.move_to(0.7, 9.3);
        triangle.line_to(11.2, 0.4);
        triangle.line_to(6.1, 8.8);
        triangle.close();
        shapes.push(("a turned triangle", triangle, FillRule::NonZero));
        let mut disc = Path::new();
        circle(&mut disc, (6.2, 5.1), 3.7);
        shapes.push(("a circle", disc, FillRule::NonZero));
        let mut ring = Path::new();
        circle(&mut ring, (5.9, 4.8), 4.4);
        circle(&mut ring, (5.9, 4.8), 2.1);
        shapes.push(("an even-odd ring", ring, FillRule::EvenOdd));
        shapes.push(("an unclosed middle", unclosed_middle(), FillRule::NonZero));
        let mut slivers = Path::new();
        slivers.rect(1.3, 2.5 + 1.0 / 64.0, 9.4, 1.0 / 32.0);
        slivers.rect(-0.7, 6.0 - 1.0 / 64.0, 5.2, 1.0 / 32.0);
        slivers.move_to(4.4, 7.53);
        slivers.line_to(11.6, 8.51);
        slivers.line_to(11.6, 8.52);
        slivers.line_to(4.4, 7.54);
        slivers.close();
        shapes.push(("slivers", slivers, FillRule::NonZero));
        let mut points = Path::new();
        points.move_to(3.4, 2.7);
        points.move_to(7.0, 4.5);
        points.move_to(9.6, 8.2);
        shapes.push(("points", points, FillRule::NonZero));

        let bands = [(0, TALL), (3, 4), (TALL as i32 - 1, 1)];
        for (name, path, rule) in &shapes {
            let page = fill_hard(path, *rule, 0, 0, WIDE, TALL, 0.1, None, HardEdge::AnyPart);
            assert!(page.data.contains(&255), "{name} meets a pixel");
            assert!(page.data.iter().all(|&v| v == 0 || v == 255), "{name}");
            for x0 in 0..WIDE {
                for width in 1..=(WIDE - x0).min(4) {
                    for (y0, height) in bands {
                        let tile = fill_hard(
                            path,
                            *rule,
                            x0 as i32,
                            y0,
                            width,
                            height,
                            0.1,
                            None,
                            HardEdge::AnyPart,
                        );
                        for y in y0..y0 + height as i32 {
                            for x in x0 as i32..(x0 + width) as i32 {
                                assert_eq!(
                                    tile.at(x, y),
                                    page.at(x, y),
                                    "{name}: the tile at ({x0}, {y0}) {width} wide, pixel ({x}, {y})"
                                );
                            }
                        }
                    }
                }
            }
        }

        let mut line = Path::new();
        line.move_to(0.6, 4.52);
        line.line_to(11.3, 4.52);
        let style = crate::StrokeStyle {
            width: 0.02,
            ..crate::StrokeStyle::default()
        };
        let mut scratch = FillScratch::new();
        scratch.keep_outline_points(true);
        crate::stroke_with(&mut scratch, &line, &style, 0.1, None);
        let page = fill_outline_hard(&mut scratch, 0, 0, WIDE, TALL, None, HardEdge::AnyPart);
        assert!(page.data.contains(&255), "the stroke meets a pixel");
        for x0 in 0..WIDE {
            for width in 1..=(WIDE - x0).min(4) {
                for (y0, height) in bands {
                    let tile = fill_outline_hard(
                        &mut scratch,
                        x0 as i32,
                        y0,
                        width,
                        height,
                        None,
                        HardEdge::AnyPart,
                    );
                    for y in y0..y0 + height as i32 {
                        for x in x0 as i32..(x0 + width) as i32 {
                            assert_eq!(
                                tile.at(x, y),
                                page.at(x, y),
                                "the stroke: the tile at ({x0}, {y0}) {width} wide, pixel ({x}, {y})"
                            );
                        }
                    }
                }
            }
        }
    }

    /// **An outline under the any-part rule reads the pieces its scratch
    /// kept.** With `keep_outline_points` on, a stroke's outline is the
    /// any-part fill of the path the stroker returns, byte for byte, for
    /// every cap and join, a dash and a hairline a fiftieth of a pixel wide
    /// lying between two sub-scanlines, which paints the row it lies in from
    /// end to end. With it off — the default, where a stroke keeps its edges
    /// alone — the outline is read from its samples alone, the sorted sweep's
    /// spans of the same path, and the hairline paints nothing; the coverage
    /// and the other rules read the same edges either way.
    #[test]
    fn an_outline_under_the_any_part_rule_reads_the_pieces_it_kept() {
        use crate::fill_reference::touches_reference;

        let mut curve = Path::new();
        curve.move_to(2.2, 3.1);
        curve.line_to(15.7, 4.9);
        curve.curve_to(18.0, 12.0, 6.0, 17.5, 3.3, 11.8);
        let mut hairline = Path::new();
        hairline.move_to(1.3, 6.52);
        hairline.line_to(16.7, 6.52);
        let styles = [
            (
                &curve,
                crate::StrokeStyle {
                    width: 1.7,
                    cap: crate::LineCap::Butt,
                    join: crate::LineJoin::Miter,
                    ..crate::StrokeStyle::default()
                },
            ),
            (
                &curve,
                crate::StrokeStyle {
                    width: 0.9,
                    cap: crate::LineCap::Round,
                    join: crate::LineJoin::Round,
                    dashes: vec![2.5, 1.25],
                    ..crate::StrokeStyle::default()
                },
            ),
            (
                &hairline,
                crate::StrokeStyle {
                    width: 0.02,
                    cap: crate::LineCap::Butt,
                    ..crate::StrokeStyle::default()
                },
            ),
        ];
        for (index, (path, style)) in styles.iter().enumerate() {
            let outline = crate::stroke(path, style, 0.1, None);
            let mut kept = FillScratch::new();
            kept.keep_outline_points(true);
            crate::stroke_with(&mut kept, path, style, 0.1, None);
            let whole = fill_outline_hard(&mut kept, 0, 0, 20, 20, None, HardEdge::AnyPart);
            let want = fill_hard(
                &outline,
                FillRule::NonZero,
                0,
                0,
                20,
                20,
                0.1,
                None,
                HardEdge::AnyPart,
            );
            assert_eq!(whole.data, want.data, "style {index}, its pieces kept");

            let mut bare = FillScratch::new();
            crate::stroke_with(&mut bare, path, style, 0.1, None);
            let sampled = fill_outline_hard(&mut bare, 0, 0, 20, 20, None, HardEdge::AnyPart);
            let samples = touches_reference(&outline, FillRule::NonZero, 0, 0, 20, 20, 0.1, false);
            assert_eq!(sampled.data, samples.data, "style {index}, its edges alone");
            for edge in [HardEdge::Half, HardEdge::Centre] {
                assert_eq!(
                    fill_outline_hard(&mut bare, 0, 0, 20, 20, None, edge).data,
                    fill_outline_hard(&mut kept, 0, 0, 20, 20, None, edge).data,
                    "style {index} under {edge:?}"
                );
            }
        }

        let mut kept = FillScratch::new();
        kept.keep_outline_points(true);
        crate::stroke_with(&mut kept, &hairline, &styles[2].1, 0.1, None);
        let painted = fill_outline_hard(&mut kept, 0, 0, 20, 20, None, HardEdge::AnyPart);
        for y in 0..20 {
            for x in 0..20 {
                let met = y == 6 && (1..=16).contains(&x);
                assert_eq!(
                    painted.at(x, y),
                    if met { 255 } else { 0 },
                    "the hairline's pixel ({x}, {y})"
                );
            }
        }
        let mut bare = FillScratch::new();
        crate::stroke_with(&mut bare, &hairline, &styles[2].1, 0.1, None);
        let unseen = fill_outline_hard(&mut bare, 0, 0, 20, 20, None, HardEdge::AnyPart);
        assert!(
            unseen.data.iter().all(|&v| v == 0),
            "no sample sees the hairline"
        );
    }

    /// A stopped any-part fill keeps the rows it had swept, both halves of
    /// the rule: a bar and two slivers lying between sub-scanlines, one in
    /// row 10 and one in row 100, stopped by the fourth question, which the
    /// sweep asks at row `3 × STOP_EVERY`. The rows above the cut hold the
    /// bar's thirty columns and the upper sliver's three pixels, which only
    /// its edges reach; the cut row and everything below it, the lower
    /// sliver among them, hold nothing, though a fill that is not stopped
    /// paints it.
    #[test]
    fn a_stopped_any_part_fill_keeps_the_rows_it_had_already_swept() {
        let mut path = tall_bar();
        path.rect(1.25, 10.5 + 1.0 / 64.0, 2.5, 1.0 / 32.0);
        path.rect(1.25, 100.5 + 1.0 / 64.0, 2.5, 1.0 / 32.0);
        let whole = fill_hard(
            &path,
            FillRule::NonZero,
            0,
            0,
            40,
            400,
            0.1,
            None,
            HardEdge::AnyPart,
        );
        assert_eq!(whole.at(2, 100), 255, "the lower sliver, unstopped");
        let asked = std::cell::Cell::new(0u32);
        let ask = || {
            asked.set(asked.get() + 1);
            asked.get() == 4
        };
        let partial = fill_hard(
            &path,
            FillRule::NonZero,
            0,
            0,
            40,
            400,
            0.1,
            Some(&ask),
            HardEdge::AnyPart,
        );
        let cut = 3 * STOP_EVERY as i32;
        assert_eq!(partial.at(20, cut - 1), 255, "the rows before the cut drew");
        assert_eq!(partial.at(20, cut), 0, "the row it stopped on did not");
        assert_eq!(partial.at(2, 10), 255, "the upper sliver, by its edges");
        assert_eq!(partial.at(2, 100), 0, "the lower sliver is below the cut");
        assert_eq!(
            partial.data.iter().filter(|&&v| v != 0).count(),
            cut as usize * 30 + 3,
            "the rows above the cut: the bar's thirty columns and the sliver's three pixels"
        );
    }
}
