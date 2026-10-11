//! GIF: the block walk, a variable-root LZW packed least significant bit
//! first, interlace, and the invariants the embed door trusts.
//!
//! A GIF carries **three independent sizes over one image** and none of them
//! validates the others: the logical screen, which is the canvas; the image
//! descriptor's own position and size, which may lie anywhere on or off it;
//! and the LZW stream, whose length is whatever the sub-blocks hold and whose
//! root set is sized by a byte in front of it. A hand-built fixture makes all
//! three agree. An input where a first image hangs off its screen, or a code
//! stream that defines an entry the width rule has not grown to yet, is where
//! a placement loop runs off a row.
//!
//! The control byte picks the caller's ceiling, `png`'s and `bmp`'s shape.
//!
//! What is asserted beyond "it did not panic":
//!
//! - **A decoded raster is exactly the logical screen**, in whichever layout
//!   it came back — indexed, or RGBA when a local table covered part of it.
//! - **An indexed raster never indexes past its palette**, which `gif.rs`
//!   pads to 256 entries so that this holds.
//! - **A lower ceiling never turns a refusal into a different picture.**
//!
//! # What this target cannot find, and what covers it instead
//!
//! Every assertion is structural; a decode that is well-formed and *wrong*
//! passes. Correctness is `crates/tinker-pdf-filters/tests/image_fixtures.rs`,
//! which holds the decoder to pixels Pillow and omggif were handed.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_filters::{gif_decode, GifError, ImagePixels, Limits};

fuzz_target!(|data: &[u8]| {
    let (control, body) = data.split_at(data.len().min(1));
    let knobs = control.first().copied().unwrap_or(0);

    let limits = Limits::new(match knobs & 3 {
        0 => 1,
        1 => 1 << 10,
        2 => 1 << 16,
        _ => 1 << 22,
    });

    if let Ok(img) = gif_decode(body, &limits) {
        assert!(img.width > 0 && img.height > 0, "a zero dimension is refused");
        let pixels = u64::from(img.width) * u64::from(img.height);
        let want = pixels * img.pixels.bytes_per_pixel() as u64;
        assert_eq!(img.pixels.data().len() as u64, want, "raster size");
        assert!(want <= limits.max_output as u64, "the ceiling was exceeded");
        match &img.pixels {
            ImagePixels::Indexed { palette, indices, .. } => {
                assert_eq!(palette.len(), 256 * 3, "the table is padded to 256");
                assert!(indices.len() as u64 == pixels);
            }
            ImagePixels::Rgba(_) => {}
            ImagePixels::Rgb(_) => panic!("a GIF is never plain RGB"),
        }

        let roomier = Limits::new(limits.max_output.saturating_mul(4).max(1 << 22));
        match gif_decode(body, &roomier) {
            Ok(again) => assert_eq!(again.pixels, img.pixels, "the ceiling changed the picture"),
            Err(e) => panic!("a roomier ceiling refused what a tighter one decoded: {e}"),
        }
    }

    match gif_decode(body, &Limits::new(1)) {
        Ok(img) => assert!(img.pixels.data().len() <= 1),
        Err(GifError::ExceedsOutputLimit { bytes, limit }) => {
            assert_eq!(limit, 1);
            assert!(bytes > 1);
        }
        Err(_) => {}
    }
});
