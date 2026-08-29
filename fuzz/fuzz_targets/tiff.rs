//! TIFF 6.0: the directory walk, the strip and tile geometry, and the six
//! codings reached through it.
//!
//! The twenty-fifth target. What makes a fuzzer the right instrument here is
//! that a TIFF carries **three independent length systems over the same
//! bytes** and none of them validates the others: the directory's own offsets
//! and counts, which say where a field's values are; `StripOffsets` and
//! `StripByteCounts`, which say where the coded bytes are; and `ImageWidth`,
//! `ImageLength`, `BitsPerSample`, `RowsPerStrip` and the tile tags, which say
//! how many bytes a strip should decode to. A hand-built fixture makes all
//! three agree by construction, and every one of them is a `u32` an attacker
//! writes. An input that damages one and not the others is where a placement
//! loop runs off the end of a row, and a fixture author does not write those.
//!
//! It is also the first target that reaches five other decoders through one
//! parser: `ccitt.rs`, `lzw.rs`, `jpeg.rs`, `inflate.rs` and `packbits.rs` are
//! all downstream of a two-byte `Compression` field, so the strip that arrives
//! at each of them has been through a geometry calculation the codec's own
//! target never performs.
//!
//! The control byte picks the **bounds** rather than the input, in the shape
//! `png` and `zip_archive` landed: gap 18 milestone 8 found a work cap set
//! above the most its own inputs could ask for, so a target whose limits are
//! all shipped defaults never explores a refusal. `MAX_TIFF_SAMPLES` is 2^26
//! and no fuzz iteration will build a raster near it, so the caller's own
//! ceiling is what moves here — down to one byte, which is the value that
//! makes `ExceedsOutputLimit` reachable at all.
//!
//! What is asserted beyond "it did not panic":
//!
//! - **A decoded raster is exactly its own declared size.** The embed door
//!   hands these bytes to an `/SMask` split and to an image XObject whose
//!   `/Width` and `/Height` came from the same directory, so a raster short by
//!   a row is read past downstream rather than noticed here.
//! - **The scan and the decode agree about the geometry.** They are two entry
//!   points over one file, the pass-through uses the first and the fallback
//!   uses the second, and the embed door's whole design is that the two
//!   describe one picture.
//! - **A scan that succeeds found at least as many segments as the geometry
//!   needs.** That is the one thing `tiff_scan` promises a caller which is not
//!   going to decode, and it is what the pass-through indexes with.
//! - **A lower ceiling never turns a refusal into a panic, and never turns a
//!   refusal into a different picture.**
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_filters::{
    tiff_decode, tiff_scan, Limits, TiffError, TiffLayout, TiffPhotometric, TiffPlanar,
};

fuzz_target!(|data: &[u8]| {
    let (control, body) = data.split_at(data.len().min(1));
    let knobs = control.first().copied().unwrap_or(0);

    let limits = Limits::new(match knobs & 3 {
        0 => 1,
        1 => 1 << 10,
        2 => 1 << 16,
        _ => 1 << 22,
    });

    // The scan is a public entry point of its own — the pass-through calls it
    // and never decodes — so it is fuzzed as one rather than only through the
    // decoder.
    if let Ok(scan) = tiff_scan(body) {
        assert!(
            scan.width > 0 && scan.height > 0,
            "a zero dimension is refused, not scanned"
        );
        assert!(matches!(scan.bits_per_sample, 1 | 2 | 4 | 8 | 16));
        assert!(scan.predictor == 1 || scan.predictor == 2);

        // The geometry's own segment count, recomputed here rather than read
        // back, so the two arithmetics have to agree.
        let planes = match scan.planar {
            TiffPlanar::Chunky => 1u64,
            TiffPlanar::Planar => u64::from(scan.samples_per_pixel).max(1),
        };
        let needed = match scan.layout {
            TiffLayout::Strips { rows_per_strip } => {
                u64::from(scan.height).div_ceil(u64::from(rows_per_strip.max(1))) * planes
            }
            TiffLayout::Tiles { width, height } => {
                u64::from(scan.width).div_ceil(u64::from(width.max(1)))
                    * u64::from(scan.height).div_ceil(u64::from(height.max(1)))
                    * planes
            }
        };
        assert!(
            scan.segments.len() as u64 >= needed,
            "a scan that succeeded promised {needed} segments and found {}",
            scan.segments.len()
        );
        assert_eq!(
            scan.color_map.is_empty(),
            scan.photometric != TiffPhotometric::Palette,
            "a palette image is scanned with a table and nothing else is"
        );

        // Decoding from the scan and decoding the file are the same operation,
        // and the embed door relies on being able to hold one scan and choose.
        let from_scan = scan.decode(&limits);
        let from_bytes = tiff_decode(body, &limits);
        assert_eq!(
            from_scan.is_ok(),
            from_bytes.is_ok(),
            "the two doors disagreed about whether this file decodes"
        );

        if let Ok(img) = from_scan {
            assert_eq!(img.width, scan.width);
            assert_eq!(img.height, scan.height);
            assert!(matches!(img.bits_per_component, 8 | 16));
            let want = u64::from(img.width)
                * u64::from(img.height)
                * u64::from(img.colour.components())
                * u64::from(img.bits_per_component / 8);
            assert_eq!(img.data.len() as u64, want, "raster size");
            assert!(want <= limits.max_output as u64, "the ceiling was exceeded");

            // A palette image never comes back indexed, and an alpha channel is
            // never invented for a file with no extra sample.
            assert_eq!(img.colour, scan.colour(), "the layout the scan promised");
            assert_eq!(
                img.colour.has_alpha(),
                u32::from(scan.samples_per_pixel) > scan.photometric.colour_channels(),
                "an alpha channel exists exactly when ExtraSamples says one does"
            );

            // A second, larger ceiling must not change the picture. Only the
            // refusal may differ, and only in the direction with more room.
            let roomier = Limits::new(limits.max_output.saturating_mul(4).max(1 << 22));
            match tiff_decode(body, &roomier) {
                Ok(again) => assert_eq!(again.data, img.data, "the ceiling changed the picture"),
                Err(e) => panic!("a roomier ceiling refused what a tighter one decoded: {e}"),
            }
        }
    } else {
        // A file the scan refuses is refused by the decoder for the same
        // reason: the decoder is the scan plus pixels, and nothing may slip
        // between them.
        assert!(
            tiff_decode(body, &limits).is_err(),
            "the decoder accepted a file the scan refused"
        );
    }

    // The tightest ceiling there is, on every input: `ExceedsOutputLimit` is
    // the one refusal that has nothing to do with the file's contents, and it
    // must still be a refusal rather than a truncated raster.
    match tiff_decode(body, &Limits::new(1)) {
        Ok(img) => assert!(img.data.len() <= 1),
        Err(TiffError::ExceedsOutputLimit { bytes, limit }) => {
            assert_eq!(limit, 1);
            assert!(bytes > 1);
        }
        Err(_) => {}
    }
});
