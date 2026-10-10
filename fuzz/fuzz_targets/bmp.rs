//! BMP: five header layouts, two RLE codings, bit fields, and the invariants
//! the embed door trusts without re-checking.
//!
//! A bitmap carries **three length systems over one pixel array** and none of
//! them validates the others: `bfOffBits`, which says where the pixels start;
//! `biSize` and `biClrUsed`, which say where the colour table is and how long;
//! and `biWidth`, `biHeight` and `biBitCount`, which say how long a row is and
//! how many there are. A hand-built fixture makes all three agree by
//! construction. The RLE codings add a fourth — a cursor the stream moves
//! with its own escapes — and a delta that walks the cursor off the image is
//! exactly the input a fixture author does not write.
//!
//! The control byte picks the caller's ceiling, in the shape `png` and `tiff`
//! landed: `MAX_BMP_SAMPLES` is 2^26 and no fuzz iteration builds a raster
//! near it, so the ceiling is what moves — down to one byte, which is the
//! value that makes `ExceedsOutputLimit` reachable at all.
//!
//! What is asserted beyond "it did not panic":
//!
//! - **A decoded raster is exactly its own declared size**, in whichever of
//!   the three layouts it came back. The embed door hands the bytes to an
//!   image XObject whose `/Width` and `/Height` came from the same header.
//! - **An indexed raster never indexes past its palette.** `bmp.rs` pads the
//!   table to the depth's own size so that this holds; a raster that broke it
//!   would be read past by the `/Indexed` lookup downstream.
//! - **A lower ceiling never turns a refusal into a different picture.**
//!
//! # What this target cannot find, and what covers it instead
//!
//! Every assertion above is structural. A decode that is well-formed and
//! *wrong* passes this target exactly as a correct one does; correctness is
//! `crates/tinker-pdf-filters/tests/image_fixtures.rs`, which holds the
//! decoder to pixels Pillow and imagecodecs were handed and to bmpsuite's
//! relations.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_filters::{bmp_decode, BmpError, ImagePixels, Limits};

fuzz_target!(|data: &[u8]| {
    let (control, body) = data.split_at(data.len().min(1));
    let knobs = control.first().copied().unwrap_or(0);

    let limits = Limits::new(match knobs & 3 {
        0 => 1,
        1 => 1 << 10,
        2 => 1 << 16,
        _ => 1 << 22,
    });

    if let Ok(img) = bmp_decode(body, &limits) {
        assert!(img.width > 0 && img.height > 0, "a zero dimension is refused");
        let pixels = u64::from(img.width) * u64::from(img.height);
        let want = pixels * img.pixels.bytes_per_pixel() as u64;
        assert_eq!(img.pixels.data().len() as u64, want, "raster size");
        assert!(want <= limits.max_output as u64, "the ceiling was exceeded");
        if let ImagePixels::Indexed {
            palette,
            indices,
            transparent,
        } = &img.pixels
        {
            assert!(transparent.is_none(), "a BMP has no transparent index");
            assert!(palette.len() % 3 == 0 && palette.len() <= 256 * 3);
            let entries = palette.len() / 3;
            assert!(
                indices.iter().all(|&i| usize::from(i) < entries),
                "an index addresses nothing"
            );
        }

        let roomier = Limits::new(limits.max_output.saturating_mul(4).max(1 << 22));
        match bmp_decode(body, &roomier) {
            Ok(again) => assert_eq!(again.pixels, img.pixels, "the ceiling changed the picture"),
            Err(e) => panic!("a roomier ceiling refused what a tighter one decoded: {e}"),
        }
    }

    match bmp_decode(body, &Limits::new(1)) {
        Ok(img) => assert!(img.pixels.data().len() <= 1),
        Err(BmpError::ExceedsOutputLimit { bytes, limit }) => {
            assert_eq!(limit, 1);
            assert!(bytes > 1);
        }
        Err(_) => {}
    }
});
