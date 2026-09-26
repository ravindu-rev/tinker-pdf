//! Coverage accumulated across draws, so abutting images do not conflate.
//!
//! # The artefact this exists to remove
//!
//! Two images that share an edge each cover the pixels along it *partly*.
//! Compositing them one after the other over the page leaves the page showing
//! through in between: a half-and-half boundary keeps a quarter of the
//! backdrop, because source-over of two half-covered draws is not the average
//! of them. That is conflation, and it is what every scanned document
//! assembled from strips is made of — a PCLM page is hundreds of them.
//!
//! The fix is not a better edge. It is to stop compositing between the two:
//! a run of image draws accumulates here first, coverage *adding* rather than
//! compositing, and reaches the canvas once. Two strips each covering half a
//! boundary pixel accumulate to one whole pixel of their average, which is
//! exactly what a render at twice the scale box-filters down to — the
//! metamorphic relation that measures this.
//!
//! # Why premultiplied, and why that fits in a byte
//!
//! Colour is stored already multiplied by its coverage, which is what makes
//! accumulation an addition rather than a weighted mean needing a divide per
//! draw. It cannot overflow a byte: coverage totals are held to 255, so the
//! sum of `colour x coverage / 255` over a pixel's fragments is at most 255 as
//! well. Four bytes a pixel, integers throughout, no float on the path —
//! ruling 4.
//!
//! # What it deliberately does not do
//!
//! Fragments are *added*, so this is only correct for draws that do not
//! overlap. Two images stacked on each other are not fragments of one picture
//! and adding them would show both; [`Fragments::would_overlap`] is what the
//! caller asks before accumulating, and a true overlap ends the run instead.

use crate::blend::BlendMode;
use crate::canvas::{mul255, Canvas, Color};
use crate::fill::Mask;

/// A run of image draws, accumulated before any of it reaches the canvas.
#[derive(Clone, Debug)]
pub struct Fragments {
    /// Left edge in device pixels.
    x0: i32,
    /// Top edge in device pixels.
    y0: i32,
    /// Width in pixels.
    width: u32,
    /// Height in pixels.
    height: u32,
    /// Premultiplied RGBA, four bytes a pixel, row-major.
    data: Vec<u8>,
}

impl Fragments {
    /// An empty run over a device rectangle.
    #[must_use]
    pub fn new(x0: i32, y0: i32, width: u32, height: u32) -> Fragments {
        let len = (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4);
        Fragments {
            x0,
            y0,
            width,
            height,
            data: vec![0; len],
        }
    }

    /// How many pixels the run is holding, which is what a budget counts.
    #[must_use]
    pub fn pixels(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }

    /// The device rectangle, as `(x0, y0, width, height)`.
    #[must_use]
    pub fn region(&self) -> (i32, i32, u32, u32) {
        (self.x0, self.y0, self.width, self.height)
    }

    /// Whether anything has been accumulated at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.data.iter().all(|byte| *byte == 0)
    }

    /// The index of a device pixel, or `None` outside the run.
    fn at(&self, x: u32, y: u32) -> Option<usize> {
        let col = i64::from(x) - i64::from(self.x0);
        let row = i64::from(y) - i64::from(self.y0);
        if col < 0 || row < 0 || col >= i64::from(self.width) || row >= i64::from(self.height) {
            return None;
        }
        usize::try_from(row * i64::from(self.width) + col)
            .ok()?
            .checked_mul(4)
    }

    /// Whether a draw covering `shape` would land on coverage already here.
    ///
    /// Abutment is not overlap, and the difference is a rounding term: two
    /// strips meeting exactly contribute 128 and 128, which is 256 rather than
    /// 255 because each was rounded up independently. [`SLACK`] is what
    /// separates that from an image genuinely drawn over another, which must
    /// end the run — adding it would show both pictures at once.
    #[must_use]
    pub fn would_overlap(&self, shape: &Mask) -> bool {
        /// Coverage levels of overshoot that are rounding rather than overlap.
        const SLACK: u32 = 24;

        let (x0, y0, x1, y1) = shape.overlap(u32::MAX, u32::MAX);
        for y in y0..y1 {
            for x in x0..x1 {
                let covered = u32::from(shape.at(x as i32, y as i32));
                if covered == 0 {
                    continue;
                }
                let Some(index) = self.at(x, y) else {
                    continue;
                };
                let held = u32::from(self.data.get(index + 3).copied().unwrap_or(0));
                if held + covered > 255 + SLACK {
                    return true;
                }
            }
        }
        false
    }

    /// Adds one fragment: `color` covering `coverage` of the pixel.
    pub fn add(&mut self, x: u32, y: u32, color: Color, coverage: u8) {
        let Some(index) = self.at(x, y) else {
            return;
        };
        let Some(slot) = self.data.get_mut(index..index + 4) else {
            return;
        };
        // Held to 255 in total, which is what keeps the premultiplied sums
        // inside a byte as well. An overlap large enough to matter was refused
        // by `would_overlap` before the run got here; what is clamped away is
        // the rounding term of an exact abutment.
        let room = 255 - u32::from(slot[3]);
        let taken = u32::from(coverage).min(room);
        if taken == 0 {
            return;
        }
        for (channel, value) in [color.r, color.g, color.b].into_iter().enumerate() {
            let added = mul255(taken, u32::from(value));
            slot[channel] = (u32::from(slot[channel]) + added).min(255) as u8;
        }
        slot[3] = (u32::from(slot[3]) + taken).min(255) as u8;
    }

    /// Composites the whole run onto the canvas, once.
    ///
    /// `alpha`, `blend` and `clip` are the graphics state the run was opened
    /// with; they apply to the accumulated picture rather than to each draw,
    /// which is the point — a run is one element for compositing purposes.
    pub fn composite(
        &self,
        canvas: &mut Canvas,
        alpha: f64,
        blend: BlendMode,
        clip: Option<&Mask>,
        stop: Option<&dyn Fn() -> bool>,
    ) {
        let whole = (
            self.x0,
            self.y0,
            self.x0.saturating_add(self.width as i32),
            self.y0.saturating_add(self.height as i32),
        );
        self.composite_region(canvas, whole, alpha, blend, clip, stop);
    }

    /// Composites only the part of the run inside `region`.
    ///
    /// A run is allocated across the canvas but rarely covers it, and a page
    /// of small stamps would otherwise pay for the paper at every flush. The
    /// caller passes the rectangle it actually painted, as `(x0, y0, x1, y1)`
    /// with the far edges exclusive.
    pub fn composite_region(
        &self,
        canvas: &mut Canvas,
        region: (i32, i32, i32, i32),
        alpha: f64,
        blend: BlendMode,
        clip: Option<&Mask>,
        stop: Option<&dyn Fn() -> bool>,
    ) {
        let alpha = alpha.clamp(0.0, 1.0);
        let (rx0, ry0, rx1, ry1) = region;
        if rx1 <= rx0 || ry1 <= ry0 {
            return;
        }
        let clamp = |value: i32, limit: u32| value.clamp(0, limit as i32) as u32;
        let x0 = clamp(rx0.max(self.x0), canvas.width);
        let y0 = clamp(ry0.max(self.y0), canvas.height);
        let x1 = clamp(
            rx1.min(self.x0.saturating_add(self.width as i32)),
            canvas.width,
        );
        let y1 = clamp(
            ry1.min(self.y0.saturating_add(self.height as i32)),
            canvas.height,
        );

        for py in y0..y1 {
            if stop.is_some_and(|stop| stop()) {
                return;
            }
            for px in x0..x1 {
                let Some(index) = self.at(px, py) else {
                    continue;
                };
                let Some(slot) = self.data.get(index..index + 4) else {
                    continue;
                };
                let covered = u32::from(slot[3]);
                if covered == 0 {
                    continue;
                }
                let clipped = clip.map_or(255, |mask| mask.at(px as i32, py as i32));
                if clipped == 0 {
                    continue;
                }
                // Back out of premultiplication: the stored channel is the
                // colour already scaled by its own coverage, and the canvas
                // wants the colour and the coverage apart.
                let straight =
                    |channel: u32| ((channel * 255 + covered / 2) / covered).min(255) as u8;
                let color = Color::rgb(
                    straight(u32::from(slot[0])),
                    straight(u32::from(slot[1])),
                    straight(u32::from(slot[2])),
                );
                let effective = alpha * f64::from(covered) / 255.0 * f64::from(clipped) / 255.0;
                canvas.blend_pixel_with(px, py, color, effective, blend);
            }
        }
    }
}
