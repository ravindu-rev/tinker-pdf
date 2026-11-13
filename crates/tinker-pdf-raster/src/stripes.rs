//! Coverage a stripe at a time, for a liquid-crystal panel's red, green and
//! blue stripes (`docs/design/lcd-text.md`).
//!
//! A stripe is a third of a pixel wide: stripe `k` of a row is the device
//! interval `[k/3, (k+1)/3)` across, in the page's frame, so pixel `p` holds
//! stripes `3p`, `3p + 1` and `3p + 2`. A shape's stripes are measured by
//! [`fill`](crate::fill::fill) itself — the path mapped by `(x, y) → (3x, y)`
//! and filled over stripe columns as though each were a pixel — so a stripe's
//! coverage is the grid's own: sixteen sub-scanlines a row, exact spans in
//! 1/256 of a stripe. Five integer taps then filter the stripes into three
//! coverages a pixel, in the order the panel's stripes stand.
//!
//! Integer throughout, and every coordinate in the page's frame, so a stripe
//! is the same byte on every target (ruling 4) and in a tile as on the page
//! under it (ruling 5): the region is widened by the filter's reach as
//! geometry, never by what a canvas holds.

use crate::fill::{fill_with, FillScratch, Mask};
use crate::geom::{FillRule, Path};
use crate::image::Transform;

/// The order a panel's three stripes stand in, left to right.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SubpixelOrder {
    /// Red, green, blue.
    Rgb,
    /// Blue, green, red.
    Bgr,
}

/// The filter's taps: a pixel's average, `[1, 1, 1]`, taken twice. Its
/// response has a double zero at one cycle a pixel, the stripes' own period,
/// where colour fringes sit; its taps sum to [`TAP_SUM`], so a run of equal
/// stripes filters to itself; and it is symmetric, so it moves no edge.
pub const TAPS: [u32; 5] = [1, 2, 3, 2, 1];

/// What [`TAPS`] sum to, and so what a filtered stripe is divided by.
pub const TAP_SUM: u32 = 9;

/// Stripes the filter reads on each side of the one it writes.
const REACH: usize = 2;

/// Stripes a pixel holds.
const STRIPES: i64 = 3;

/// The map a shape's stripes are filled under: three times across, about the
/// page frame's `x = 0`, and nothing else. `3x` is rounded once, the same way
/// wherever it is computed, and `y` is untouched.
const THREE_ACROSS: Transform = Transform {
    a: 3.0,
    b: 0.0,
    c: 0.0,
    d: 1.0,
    e: 0.0,
    f: 0.0,
};

/// Three coverages a pixel — red, green and blue — over a rectangular
/// region: [`Mask`]'s counterpart for a shape measured a stripe at a time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StripeMask {
    /// Left edge in device pixels.
    pub x0: i32,
    /// Top edge in device pixels.
    pub y0: i32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Red, green and blue coverage, each 0 to 255, a pixel at a time and
    /// row-major: the canvas's channel order, whatever the panel's.
    pub data: Vec<[u8; 3]>,
}

impl StripeMask {
    /// A mask holding no coverage.
    #[must_use]
    pub fn empty(x0: i32, y0: i32, width: u32, height: u32) -> StripeMask {
        let len = (width as usize).saturating_mul(height as usize);
        StripeMask {
            x0,
            y0,
            width,
            height,
            data: vec![[0; 3]; len],
        }
    }

    /// The three coverages at a device pixel, none outside the mask.
    #[must_use]
    pub fn at(&self, x: i32, y: i32) -> [u8; 3] {
        let (Some(col), Some(row)) = (x.checked_sub(self.x0), y.checked_sub(self.y0)) else {
            return [0; 3];
        };
        if col < 0 || row < 0 || col as u32 >= self.width || row as u32 >= self.height {
            return [0; 3];
        }
        let index = (row as usize) * (self.width as usize) + (col as usize);
        self.data.get(index).copied().unwrap_or([0; 3])
    }
}

/// A stripe's filtered coverage from the five stripes around it, the one it
/// writes in the middle: [`TAPS`] summed and divided by [`TAP_SUM`], rounded
/// half up. The largest sum is `9 × 255 + 4`, which divides to 255, so the
/// answer is a byte.
fn filtered(window: &[u8]) -> u8 {
    let sum: u32 = window
        .iter()
        .zip(TAPS)
        .map(|(&c, tap)| u32::from(c) * tap)
        .sum();
    u8::try_from((sum + TAP_SUM / 2) / TAP_SUM).unwrap_or(u8::MAX)
}

/// The stripe columns a mask over pixels `x0` to `x0 + width` reads: the
/// region widened by one whole pixel each side, which holds the two stripes
/// the filter reads past either end, three stripes a pixel, as `(first,
/// count)`. `None` where they do not fit the fill's coordinates.
fn stripe_columns(x0: i32, width: u32) -> Option<(i32, u32)> {
    let first = (i64::from(x0) - 1).checked_mul(STRIPES)?;
    let count = (i64::from(width) + 2).checked_mul(STRIPES)?;
    Some((i32::try_from(first).ok()?, u32::try_from(count).ok()?))
}

/// A shape's coverage a stripe at a time over a region of pixels, filtered
/// into three coverages a pixel in `order` ([`fill_with`]'s arguments
/// otherwise, and its scratch of its own).
///
/// The stripes are `fill` of the path mapped three times across, under the
/// same rule and tolerance, over the region widened by one pixel each side;
/// each pixel's three coverages are its stripes filtered by [`TAPS`]. A region
/// whose stripe columns would not fit the fill's coordinates — a pixel more
/// than about 715 million from the origin — holds no coverage. `stop` is the
/// fill's, and a stopped fill's stripes filter as they stand.
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn fill_stripes(
    path: &Path,
    rule: FillRule,
    x0: i32,
    y0: i32,
    width: u32,
    height: u32,
    tolerance: f64,
    order: SubpixelOrder,
    stop: Option<&dyn Fn() -> bool>,
) -> StripeMask {
    fill_stripes_with(
        &mut FillScratch::new(),
        path,
        rule,
        x0,
        y0,
        width,
        height,
        tolerance,
        order,
        stop,
    )
}

/// [`fill_stripes`], filling through `scratch` as [`fill_with`] does.
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn fill_stripes_with(
    scratch: &mut FillScratch,
    path: &Path,
    rule: FillRule,
    x0: i32,
    y0: i32,
    width: u32,
    height: u32,
    tolerance: f64,
    order: SubpixelOrder,
    stop: Option<&dyn Fn() -> bool>,
) -> StripeMask {
    let mut mask = StripeMask::empty(x0, y0, width, height);
    if width == 0 || height == 0 {
        return mask;
    }
    let Some((first, count)) = stripe_columns(x0, width) else {
        return mask;
    };
    let stripes = fill_with(
        scratch,
        &path.mapped(&THREE_ACROSS),
        rule,
        first,
        y0,
        count,
        height,
        tolerance,
        stop,
    );
    filter_into(&stripes, order, &mut mask);
    mask
}

/// `stripes`, a row of stripe columns one pixel wider than `mask` on each
/// side, filtered into `mask`'s three coverages a pixel.
///
/// The region's stripe `j` is column `j + 3` of a row of `stripes`, and its
/// window of five starts at column `j + 1`: the windows of a row taken from
/// the second, three a pixel.
fn filter_into(stripes: &Mask, order: SubpixelOrder, mask: &mut StripeMask) {
    let span = stripes.width as usize;
    let width = mask.width as usize;
    if span == 0 || width == 0 {
        return;
    }
    for (line, target) in stripes
        .data
        .chunks_exact(span)
        .zip(mask.data.chunks_exact_mut(width))
    {
        let mut filtered_stripes = line.windows(2 * REACH + 1).skip(1).map(filtered);
        for pixel in target.iter_mut() {
            let (Some(left), Some(middle), Some(right)) = (
                filtered_stripes.next(),
                filtered_stripes.next(),
                filtered_stripes.next(),
            ) else {
                break;
            };
            *pixel = match order {
                SubpixelOrder::Rgb => [left, middle, right],
                SubpixelOrder::Bgr => [right, middle, left],
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fill::fill;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Path {
        let mut path = Path::new();
        path.rect(x, y, w, h);
        path
    }

    /// The taps are `[1, 1, 1]` convolved with itself, and a run of five
    /// equal stripes filters to that stripe at every coverage: unit gain.
    #[test]
    fn the_taps_are_a_pixels_average_taken_twice() {
        let mut twice = [0u32; 5];
        for i in 0..3 {
            for j in 0..3 {
                twice[i + j] += 1;
            }
        }
        assert_eq!(TAPS, twice);
        assert_eq!(TAPS.iter().sum::<u32>(), TAP_SUM);
        for c in 0..=255u8 {
            assert_eq!(filtered(&[c; 5]), c, "five stripes of {c}");
        }
        assert_eq!(
            filtered(&[1, 0, 0, 0, 0]),
            0,
            "a ninth of a level rounds to nothing"
        );
        assert_eq!(filtered(&[255, 0, 0, 0, 0]), 28, "a ninth of 255");
        assert_eq!(filtered(&[255, 255, 0, 0, 0]), 85, "three ninths of 255");
        assert_eq!(
            filtered(&[0, 0, 255, 0, 0]),
            85,
            "the middle stripe's third"
        );
        assert_eq!(filtered(&[0, 0, 255, 255, 255]), 170, "six ninths, rounded");
    }

    /// A shape covering the region and a pixel past it on every side fills
    /// every channel of every pixel, in both orders.
    #[test]
    fn a_whole_region_of_stripes_is_whole_in_every_channel() {
        for order in [SubpixelOrder::Rgb, SubpixelOrder::Bgr] {
            let mask = fill_stripes(
                &rect(0.0, 0.0, 10.0, 6.0),
                FillRule::NonZero,
                2,
                1,
                5,
                4,
                0.1,
                order,
                None,
            );
            assert_eq!((mask.x0, mask.y0, mask.width, mask.height), (2, 1, 5, 4));
            assert!(
                mask.data.iter().all(|&pixel| pixel == [255; 3]),
                "{order:?}"
            );
        }
    }

    /// **A horizontal edge is grey in all three channels.** Away from the
    /// shape's ends every stripe of a row holds the same partial coverage,
    /// and equal stripes filter to themselves, so each channel is the grey
    /// fill's coverage at that pixel.
    #[test]
    fn a_horizontal_edge_is_its_grey_coverage_in_every_channel() {
        let path = rect(-4.0, 1.3, 20.0, 2.45);
        let grey = fill(&path, FillRule::NonZero, 0, 0, 8, 5, 0.1, None);
        let stripes = fill_stripes(
            &path,
            FillRule::NonZero,
            0,
            0,
            8,
            5,
            0.1,
            SubpixelOrder::Rgb,
            None,
        );
        for y in 0..5 {
            for x in 0..8 {
                let c = grey.at(x, y);
                assert_eq!(stripes.at(x, y), [c; 3], "pixel ({x}, {y})");
            }
        }
        assert!(
            grey.data.iter().any(|&c| c > 0 && c < 255),
            "the edges are partial"
        );
    }

    /// **A vertical edge is three coverages.** A rectangle from a third of
    /// the way into pixel 2 to a third of the way into pixel 8, rows whole,
    /// covers stripes 7 to 24 and no other: pixel 2 holds stripes 6, 7 and 8,
    /// the last two the shape's. Filtered, each stripe is the ninths of the
    /// shape its five taps read — 255 × n / 9, rounded — so the coverages
    /// climb across the left edge and fall across the right, the filter
    /// reaching a stripe past each into the pixel beside; and the panel's
    /// other order reads every pixel's three right to left.
    #[test]
    fn a_vertical_edge_at_a_stripe_is_three_coverages_and_bgr_reverses_them() {
        let path = rect(2.0 + 1.0 / 3.0, 0.0, 6.0, 4.0);
        let fill_in = |order| fill_stripes(&path, FillRule::NonZero, 0, 0, 10, 4, 0.1, order, None);
        let rgb = fill_in(SubpixelOrder::Rgb);
        let bgr = fill_in(SubpixelOrder::Bgr);
        let ninths = |n: u32| ((255 * n + 4) / 9) as u8;
        assert_eq!(rgb.at(0, 1), [0, 0, 0]);
        assert_eq!(
            rgb.at(1, 1),
            [0, 0, ninths(1)],
            "stripe 5 reads stripe 7 at its outermost tap"
        );
        assert_eq!(
            rgb.at(2, 1),
            [ninths(3), ninths(6), ninths(8)],
            "pixel 2's stripes read three, six and eight ninths of the shape"
        );
        assert_eq!(
            rgb.at(3, 1),
            [255; 3],
            "stripes 9 to 11 read only the shape"
        );
        assert_eq!(rgb.at(7, 1), [255, 255, ninths(8)]);
        assert_eq!(
            rgb.at(8, 1),
            [ninths(6), ninths(3), ninths(1)],
            "the right edge mirrors the left"
        );
        assert_eq!(rgb.at(9, 1), [0, 0, 0]);
        for y in 0..4 {
            for x in 0..10 {
                let [r, g, b] = rgb.at(x, y);
                assert_eq!(bgr.at(x, y), [b, g, r], "pixel ({x}, {y})");
            }
        }
    }

    /// An empty region fills nothing; a region whose stripe columns would not
    /// fit the fill's coordinates holds no coverage rather than panicking;
    /// a fill stopped before its first row holds none either.
    #[test]
    fn a_region_with_no_stripes_to_fill_holds_no_coverage() {
        let path = rect(0.0, 0.0, 4.0, 4.0);
        for (x0, width, height) in [(0, 0, 4), (0, 4, 0), (i32::MAX - 1, 4, 4), (i32::MIN, 4, 4)] {
            let mask = fill_stripes(
                &path,
                FillRule::NonZero,
                x0,
                0,
                width,
                height,
                0.1,
                SubpixelOrder::Rgb,
                None,
            );
            assert_eq!(mask.data.len(), (width * height) as usize);
            assert!(
                mask.data.iter().all(|&pixel| pixel == [0; 3]),
                "{x0} {width} {height}"
            );
        }
        let stop = || true;
        let stopped = fill_stripes(
            &path,
            FillRule::NonZero,
            0,
            0,
            4,
            4,
            0.1,
            SubpixelOrder::Rgb,
            Some(&stop),
        );
        assert!(stopped.data.iter().all(|&pixel| pixel == [0; 3]));
    }

    /// Outside itself a mask reports nothing, and inside it reads its own
    /// pixels.
    #[test]
    fn a_stripe_mask_reads_zero_outside_itself() {
        let mut mask = StripeMask::empty(3, 4, 2, 2);
        mask.data[3] = [1, 2, 3];
        assert_eq!(mask.at(4, 5), [1, 2, 3]);
        for (x, y) in [(2, 4), (5, 4), (3, 3), (3, 6), (i32::MIN, 4), (3, i32::MIN)] {
            assert_eq!(mask.at(x, y), [0; 3], "({x}, {y})");
        }
    }
}
