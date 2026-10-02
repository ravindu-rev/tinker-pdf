//! WebP: the RIFF chunk walk, the `VP8X` canvas and its first `ANMF` frame,
//! VP8L's transforms, prefix codes, back-references and colour cache, and
//! VP8's key frame with the `ALPH` chunk beside it.
//!
//! A lossless WebP is **a chain of images, each sized by the one before it**:
//! the header's dimensions, a colour-indexing transform that narrows the width
//! the rest are read at, predictor and colour-transform images whose sizes are
//! the width divided by a block size the stream chooses, and a meta prefix
//! image whose pixels name prefix-code groups by number. A hand-built fixture
//! keeps every one of those consistent; an input where a group number outruns
//! the groups read, a back-reference reaches before the first pixel, or a
//! packed width rounds differently from the unpacked one is where an index
//! runs off a buffer. A lossy WebP is the other shape of the same risk: a
//! frame whose size is not a whole number of macroblocks, partitions whose
//! stated lengths overrun the chunk, segment and mode trees steering the
//! dequantizer and the subblock predictors, and an `ALPH` plane decoded at
//! the frame's size by a second bitstream entirely.
//!
//! The control byte picks the caller's ceiling, `png`'s and `gif`'s shape.
//!
//! What is asserted beyond "it did not panic":
//!
//! - **A decoded raster is exactly `width x height`**, RGB or RGBA, and a WebP
//!   is never indexed.
//! - **RGBA only when some pixel is less than opaque** — `webp.rs`'s decision
//!   that an opaque picture needs no soft mask, held on every input.
//! - **The ceiling is charged at four samples a pixel**, the RGBA every WebP
//!   decodes to before an opaque one is narrowed.
//! - **A lower ceiling never turns a refusal into a different picture.**
//!
//! # What this target cannot find, and what covers it instead
//!
//! Every assertion is structural; a decode that is well-formed and *wrong*
//! passes. Correctness is `crates/tinker-pdf-filters/tests/image_fixtures.rs`,
//! which holds the lossless decoder to pixels libwebp was handed through
//! Pillow and imagecodecs and the lossy one to the pictures it was handed at a
//! stated distance, and `src/webp/vp8/tests.rs`, which holds VP8 to the WebM
//! project's published test vectors (fetched, pinned and run by CI's
//! `vp8-vectors` job) and its colour conversion to BT.601.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_filters::{webp_decode, ImagePixels, Limits, WebpError};

fuzz_target!(|data: &[u8]| {
    let (control, body) = data.split_at(data.len().min(1));
    let knobs = control.first().copied().unwrap_or(0);

    let limits = Limits::new(match knobs & 3 {
        0 => 1,
        1 => 1 << 10,
        2 => 1 << 16,
        _ => 1 << 22,
    });

    if let Ok(img) = webp_decode(body, &limits) {
        assert!(img.width > 0 && img.height > 0, "a zero dimension is refused");
        let pixels = u64::from(img.width) * u64::from(img.height);
        let want = pixels * img.pixels.bytes_per_pixel() as u64;
        assert_eq!(img.pixels.data().len() as u64, want, "raster size");
        assert!(pixels * 4 <= limits.max_output as u64, "the ceiling was exceeded");
        match &img.pixels {
            ImagePixels::Rgba(v) => {
                assert!(
                    v.chunks_exact(4).any(|p| p[3] != 255),
                    "an opaque picture came back RGBA"
                );
            }
            ImagePixels::Rgb(_) => {}
            ImagePixels::Indexed { .. } => panic!("a WebP is never indexed"),
        }

        let roomier = Limits::new(limits.max_output.saturating_mul(4).max(1 << 22));
        match webp_decode(body, &roomier) {
            Ok(again) => assert_eq!(again.pixels, img.pixels, "the ceiling changed the picture"),
            Err(e) => panic!("a roomier ceiling refused what a tighter one decoded: {e}"),
        }
    }

    match webp_decode(body, &Limits::new(1)) {
        Ok(_) => panic!("no WebP fits one byte: every picture is at least one pixel of four"),
        Err(WebpError::ExceedsOutputLimit { bytes, limit }) => {
            assert_eq!(limit, 1);
            assert!(bytes > 1);
        }
        Err(_) => {}
    }
});
