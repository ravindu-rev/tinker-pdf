//! The one output shape the BMP, GIF and WebP decoders share.
//!
//! `png.rs` and `tiff.rs` each hand back their own four-layout raster at 8 or
//! 16 bits, because both formats carry 16-bit samples and both have a
//! pass-through route that never builds one. The three decoders that joined
//! them for the archive row have neither: every sample any of them carries is
//! eight bits or fewer, and none of them is a `/Filter` a PDF stream could
//! hold. What they *do* have that PNG's decoded route does not keep is a
//! palette worth keeping — a BMP or a GIF is overwhelmingly indexed, and
//! expanding an index to three samples triples the one allocation `cbz.rs`'s
//! module note argues about. So the shape is **indexed or direct**, and a
//! consumer learns it once for all three formats.
//!
//! Ruling 8: samples and values only. A palette is RGB triples because that is
//! what every one of these formats stores, and whether it becomes an
//! `/Indexed` space is the embedder's decision, not this crate's.

/// Eight-bit pixels, top row first, no row padding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImagePixels {
    /// One byte an index into `palette`.
    Indexed {
        /// RGB triples, index zero first. Never more than 256 entries, and
        /// never fewer than the largest index in `indices` needs: a decoder
        /// that met an index past the table it was given pads the table with
        /// black rather than handing back an index that addresses nothing.
        palette: Vec<u8>,
        /// `width x height` indices.
        indices: Vec<u8>,
        /// An index every pixel carrying it is fully transparent at, which is
        /// exactly 8.9.6.4's colour-key range test with one value — and so,
        /// unlike partial alpha, needs no second image.
        transparent: Option<u8>,
    },
    /// `width x height x 3` samples, red first.
    Rgb(Vec<u8>),
    /// `width x height x 4` samples, red first, alpha last and **not**
    /// premultiplied.
    Rgba(Vec<u8>),
}

impl ImagePixels {
    /// Bytes a pixel occupies.
    #[must_use]
    pub const fn bytes_per_pixel(&self) -> usize {
        match self {
            Self::Indexed { .. } => 1,
            Self::Rgb(_) => 3,
            Self::Rgba(_) => 4,
        }
    }

    /// The sample buffer, whichever layout it is in.
    #[must_use]
    pub fn data(&self) -> &[u8] {
        match self {
            Self::Indexed { indices, .. } => indices,
            Self::Rgb(data) | Self::Rgba(data) => data,
        }
    }

    /// One pixel as `[r, g, b, a]`, or `None` past the end.
    ///
    /// Not a fast path — the embedders never call it — but the one place the
    /// three layouts are reconciled, so a test comparing a decode against an
    /// authored picture compares colours rather than layouts.
    #[must_use]
    pub fn rgba_at(&self, index: usize) -> Option<[u8; 4]> {
        match self {
            Self::Indexed {
                palette,
                indices,
                transparent,
            } => {
                let i = *indices.get(index)?;
                let at = usize::from(i) * 3;
                let rgb = palette.get(at..at + 3)?;
                let alpha = if *transparent == Some(i) { 0 } else { 255 };
                Some([*rgb.first()?, *rgb.get(1)?, *rgb.get(2)?, alpha])
            }
            Self::Rgb(data) => {
                let px = data.get(index.checked_mul(3)?..index.checked_mul(3)? + 3)?;
                Some([*px.first()?, *px.get(1)?, *px.get(2)?, 255])
            }
            Self::Rgba(data) => {
                let px = data.get(index.checked_mul(4)?..index.checked_mul(4)? + 4)?;
                Some([*px.first()?, *px.get(1)?, *px.get(2)?, *px.get(3)?])
            }
        }
    }
}

/// Scales an `n`-bit value to eight bits, rounding to nearest.
///
/// The exact division rather than bit replication, because the two differ for
/// some widths and this one is the definition: `max` maps to 255 and zero to
/// zero, and every value between lands on the nearest of 256 steps. Integer
/// arithmetic, so identical on every target (ruling 4).
pub(crate) const fn scale_to_8(value: u32, max: u32) -> u8 {
    if max == 0 {
        return 0;
    }
    let v = if value > max { max } else { value };
    ((v as u64 * 255 + max as u64 / 2) / max as u64) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scaling_is_exact_at_both_ends_and_rounds_between() {
        assert_eq!(scale_to_8(0, 31), 0);
        assert_eq!(scale_to_8(31, 31), 255);
        // 16 x 255 / 31 = 131.6, which rounds up.
        assert_eq!(scale_to_8(16, 31), 132);
        assert_eq!(scale_to_8(1, 1), 255);
        assert_eq!(scale_to_8(1023, 1023), 255);
        assert_eq!(scale_to_8(512, 1023), 128);
        assert_eq!(scale_to_8(7, 0), 0);
    }

    #[test]
    fn the_three_layouts_answer_one_colour_question() {
        let indexed = ImagePixels::Indexed {
            palette: vec![1, 2, 3, 4, 5, 6],
            indices: vec![1, 0],
            transparent: Some(0),
        };
        assert_eq!(indexed.rgba_at(0), Some([4, 5, 6, 255]));
        assert_eq!(indexed.rgba_at(1), Some([1, 2, 3, 0]));
        assert_eq!(indexed.rgba_at(2), None);
        assert_eq!(
            ImagePixels::Rgb(vec![9, 8, 7]).rgba_at(0),
            Some([9, 8, 7, 255])
        );
        assert_eq!(
            ImagePixels::Rgba(vec![9, 8, 7, 6]).rgba_at(0),
            Some([9, 8, 7, 6])
        );
    }
}
