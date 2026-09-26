//! A PNG file read back into a [`Bitmap`] — the other direction of
//! [`Bitmap::to_png`].
//!
//! # Why the facade grows this rather than a tool reaching past it
//!
//! `tpdf render` writes PNG and `pdfcmp` compared PNM and PDF, so the
//! comparator could not read the thing the debug CLI writes. The decoder was
//! never what was missing: `tinker_pdf_filters::png_decode` is the one CBZ
//! pages have always gone through, fuzzed as the `png` target and adjudicated
//! by PngSuite in that crate's own tests. What was missing is a door. `xtask`'s
//! `TOOLS` table keeps every tool on the facade, on the rule that a tool
//! exercises what a user gets rather than a leaf the user never sees — and the
//! rule is right, so the door goes here rather than an exception going there.
//!
//! # What the decoder hands back, and what becomes of each shape
//!
//! `png_decode` has already expanded 1-, 2- and 4-bit samples to eight,
//! applied a palette, and applied `tRNS` in all three of its forms, so what
//! arrives is one of four channel layouts at 8 or 16 bits a component. The
//! four layouts are exactly four [`PixelFormat`]s and are taken byte for byte:
//!
//! | Decoded layout | `PixelFormat` |
//! | --- | --- |
//! | grey | `Gray8` |
//! | grey + alpha | `GrayA8` |
//! | truecolour, or a palette with no `tRNS` | `Rgb8` |
//! | truecolour + alpha, or a palette with `tRNS` | `Rgba8` |
//!
//! **Sixteen bits become eight by rounding to nearest**: a sample `v` becomes
//! `(v × 255 + 32 767) / 65 535` in integers, which is `round(v / 257)` — and
//! since 257 is odd there is never a tie to break. It is the exact inverse of
//! the `v × 257` replication that widens eight bits to sixteen, so a 16-bit
//! file written from 8-bit samples comes back to the samples it was written
//! from. It is not the high byte: `0x0081` is 1 here and 0 from the high byte, and
//! `0xFF00` is 254 here and 255 from the high byte, and in both the answer here is
//! the nearer (129 / 257 and 65 280 / 257). A `Bitmap` is a byte a channel,
//! which is the precision [`Bitmap::to_png`]'s own documentation says a round
//! trip is at.
//!
//! # What is refused, by name
//!
//! Every refusal the decoder makes is carried through unchanged as
//! [`PngReadError::Refused`], so "colour type 1 does not exist" stays a
//! different answer from "this is not a PNG". Beside those there is one more:
//! **a raster that came up short** of the height its header declares is
//! [`PngReadError::Incomplete`] rather than a picture with a band of zeroes at
//! the bottom. The decoder degrades there, correctly, for a comic page that
//! would rather show most of itself; a file read back to be *compared* is the
//! opposite case, because a comparator handed half a picture reports the
//! damage as a rendering difference. Damage that costs no pixels — an
//! ancillary chunk with a bad CRC, bytes after `IEND` — is tolerated and named
//! on [`Bitmap::warnings`] as a [`RenderWarning::DamagedImage`] whose `name`
//! is `"PNG"` (ruling 10), since a file read back has no resource name.
//!
//! Ancillary colour chunks — `gAMA`, `cHRM`, `sRGB`, `iCCP` — are not applied.
//! A `Bitmap` says how many components it has and nothing about what they
//! mean, and [`Bitmap::to_png`] writes none of them, so the samples are taken
//! as the bytes they are.
//!
//! # The one budget, and what it refuses that `to_png` wrote
//!
//! The decoder's own `MAX_PNG_SAMPLES` — 2^26 samples, charged at the widest
//! layout the colour type can produce — is the only ceiling here, and the
//! output limit handed to `png_decode` is set to exactly what that cap allows
//! at sixteen bits so that it can never refuse first. The consequence is
//! written down in `png.rs` and repeated here because this is where it bites:
//! **the largest page this engine renders writes a PNG this will not read
//! back.** A reader's budget against a thirteen-byte header asking for 2^63
//! samples is not a writer's, and a page at `MAX_PAGE_PIXELS` is four times it
//! as RGBA.

use tinker_pdf_filters::{png_decode, Limits, PngColour, PngError, MAX_PNG_SAMPLES};

use crate::{Bitmap, PixelFormat, RenderWarning};

/// Why [`Bitmap::from_png`] would not read a file.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PngReadError {
    /// The decoder refused the file, and this is its own reason: a signature
    /// that is not PNG's, a header Table 11.1 does not permit, a critical
    /// chunk with a bad CRC or one this build does not know, no image data,
    /// or a raster past the sample budget.
    Refused(PngError),
    /// The raster ended before the height its header declares, so the rows
    /// that never arrived would be zeroes rather than pixels.
    ///
    /// Carries the decoder's stable identifiers for what it found — the same
    /// strings every other decode in this engine reports — so a caller can
    /// say *why* the file is short rather than only that it is.
    Incomplete {
        /// What the decoder tolerated on the way, as its own identifiers.
        reasons: Vec<&'static str>,
    },
}

impl core::fmt::Display for PngReadError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PngReadError::Refused(why) => write!(f, "not a readable PNG: {why}"),
            PngReadError::Incomplete { reasons } if reasons.is_empty() => {
                f.write_str("the raster is shorter than its header declares")
            }
            PngReadError::Incomplete { reasons } => write!(
                f,
                "the raster is shorter than its header declares ({})",
                reasons.join(", ")
            ),
        }
    }
}

impl std::error::Error for PngReadError {}

impl Bitmap {
    /// A PNG file (ISO/IEC 15948) as a bitmap — the inverse of
    /// [`Bitmap::to_png`] for every format that writes byte for byte.
    ///
    /// `Gray8`, `GrayA8`, `Rgb8` and `Rgba8` bitmaps survive `to_png` and then
    /// this unchanged, dimensions, stride and bytes. The two formats PNG has no
    /// colour type for do not, and cannot: `to_png` writes a `CmykA8` or
    /// `LabA8` bitmap as the light it stands for, under colour type 6, so what
    /// comes back is the `Rgba8` picture that was written.
    ///
    /// Sixteen-bit samples are rounded to eight, grey stays grey, and a palette
    /// arrives applied; the module documentation has the whole table and the
    /// arithmetic.
    ///
    /// # Errors
    ///
    /// [`PngReadError::Refused`] with the decoder's own reason for anything it
    /// will not read, and [`PngReadError::Incomplete`] for a raster that ends
    /// before its declared height. Damage that costs no pixels is not an error:
    /// the bitmap comes back with a [`RenderWarning::DamagedImage`] per thing
    /// the decoder tolerated.
    ///
    /// ```no_run
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let bytes = std::fs::read("page-0001.png")?;
    /// let bitmap = tinker_pdf::Bitmap::from_png(&bytes)?;
    /// println!("{}x{} {:?}", bitmap.width, bitmap.height, bitmap.format);
    /// # Ok(())
    /// # }
    /// ```
    pub fn from_png(bytes: &[u8]) -> Result<Bitmap, PngReadError> {
        // Exactly what `MAX_PNG_SAMPLES` permits at two bytes a sample, so this
        // ceiling never refuses a file the sample budget let through: the
        // decoder's own cap is the only one, which is the claim the module
        // documentation makes. 2^27 fits a 32-bit `usize`.
        let ceiling = usize::try_from(MAX_PNG_SAMPLES.saturating_mul(2)).unwrap_or(usize::MAX);
        let image = png_decode(bytes, &Limits::new(ceiling)).map_err(PngReadError::Refused)?;

        let reasons: Vec<&'static str> = image.warnings.iter().map(|w| w.as_str()).collect();
        if !image.complete {
            return Err(PngReadError::Incomplete { reasons });
        }

        let format = match image.colour {
            PngColour::Grey => PixelFormat::Gray8,
            PngColour::GreyAlpha => PixelFormat::GrayA8,
            PngColour::Rgb => PixelFormat::Rgb8,
            PngColour::Rgba => PixelFormat::Rgba8,
        };
        // Saturating rather than checked: the sample budget keeps both far
        // below `usize::MAX` on every target, and if it ever did not, a
        // saturated product cannot equal the buffer's length and the check
        // below refuses the file rather than a multiply wrapping into one.
        let stride = (image.width as usize).saturating_mul(format.components());
        let samples = stride.saturating_mul(image.height as usize);

        let data = match image.bits_per_component {
            16 => image
                .data
                .chunks_exact(2)
                .map(|pair| {
                    let v = u32::from(u16::from_be_bytes([pair[0], pair[1]]));
                    // `round(v / 257)`; the quotient is at most 255.
                    ((v * 255 + 32_767) / 65_535) as u8
                })
                .collect::<Vec<u8>>(),
            _ => image.data,
        };
        // The decoder promises a raster of exactly its declared size — its fuzz
        // target asserts it — and a promise from another crate is checked
        // rather than trusted where a short buffer would become a `Bitmap`
        // whose rows run past its data (ruling 1).
        if data.len() != samples {
            return Err(PngReadError::Incomplete { reasons });
        }

        let warnings = reasons
            .into_iter()
            .map(|reason| RenderWarning::DamagedImage {
                name: "PNG".to_string(),
                reason: reason.to_string(),
            })
            .collect();

        Ok(Bitmap {
            width: image.width,
            height: image.height,
            format,
            stride,
            data,
            warnings,
        })
    }
}
