//! PNG (ISO/IEC 15948): the chunk walk, both interlace methods, and the
//! invariants a caller trusts without re-checking.
//!
//! The twentieth target, and the second of gap 29's two. What makes a fuzzer
//! the right instrument here is that a PNG carries **two independent length
//! systems over the same bytes** and neither validates the other: the chunk
//! walk's declared lengths, which decide where the IDAT is, and IHDR's width,
//! height, depth and interlace flag, which decide how many bytes the inflated
//! result should be. A hand-built fixture makes them agree by construction. An
//! input that damages one and not the other is where an unfiltering pass runs
//! off the end of a row, and a fixture author does not write those.
//!
//! The control byte picks the **bounds** rather than the input, in the shape
//! `zip_archive` landed: gap 18 milestone 8 found a work cap set above the most
//! its own inputs could ask for, so a target whose limits are all shipped
//! defaults never explores a refusal. `MAX_PNG_SAMPLES` is 2^26 and no fuzz
//! iteration will build a raster near it, so the caller's own ceiling is what
//! moves here — and it moves down to one byte, which is the value that makes
//! `ExceedsOutputLimit` reachable at all.
//!
//! What is asserted beyond "it did not panic":
//!
//! - **A decoded raster is exactly its own declared size.** Gap 29 hands these
//!   bytes to an `/SMask` split and a PDF image XObject whose `/Width` and
//!   `/Height` came from the same header, so a raster that is short by a row is
//!   read past downstream rather than noticed here.
//! - **The scan and the decode agree about the header.** They are two entry
//!   points over one file, the pass-through uses the first and the fallback
//!   uses the second, and milestone 4's whole design is that the two describe
//!   one picture.
//! - **A scan that succeeds produced a usable IDAT and, for colour type 3, a
//!   palette.** Those are the two things `png_scan`'s contract promises a
//!   caller that is *not* going to decode.
//! - **A lower ceiling never turns a refusal into a panic, and never turns a
//!   refusal into a different picture.** Decoding the same bytes twice under
//!   two ceilings must give the same image or a named refusal.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_filters::{png_decode, png_scan, Limits, PngColour, PngError};

fuzz_target!(|data: &[u8]| {
    let (control, body) = data.split_at(data.len().min(1));
    let knobs = control.first().copied().unwrap_or(0);

    // Small enough that a bomb is refused in milliseconds, and varied enough
    // that both sides of the ceiling are reachable from one corpus. One byte is
    // deliberately in the set: it is the only value from which
    // `ExceedsOutputLimit` fires on an otherwise perfectly good file.
    let limits = Limits::new(match knobs & 3 {
        0 => 1,
        1 => 1 << 10,
        2 => 1 << 16,
        _ => 1 << 22,
    });

    // The scan is a public entry point of its own — gap 29's pass-through calls
    // it and never decodes — so it is fuzzed as one rather than only through
    // the decoder.
    if let Ok(scan) = png_scan(body) {
        assert!(
            !scan.idat.is_empty(),
            "a scan that succeeded promised an IDAT"
        );
        assert!(
            scan.header.width > 0 && scan.header.height > 0,
            "a zero dimension is refused, not scanned"
        );
        if scan.header.colour_type == 3 {
            assert!(
                !scan.palette.is_empty() && scan.palette.len() % 3 == 0,
                "colour type 3 without a usable palette is refused"
            );
            assert!(
                scan.palette.len() / 3 <= 1usize << scan.header.bit_depth,
                "the palette outran the bit depth that indexes it"
            );
        }

        // Decoding from the scan and decoding the file are the same operation,
        // and milestone 4 relies on being able to hold one scan and choose.
        let from_scan = scan.decode(&limits);
        let from_bytes = png_decode(body, &limits);
        assert_eq!(
            from_scan.is_ok(),
            from_bytes.is_ok(),
            "the two doors disagreed about whether this file decodes"
        );

        if let Ok(img) = from_scan {
            assert_eq!(img.width, scan.header.width);
            assert_eq!(img.height, scan.header.height);
            assert!(matches!(img.bits_per_component, 8 | 16));
            // The raster is its own declared size, to the byte.
            let want = u64::from(img.width)
                * u64::from(img.height)
                * u64::from(img.colour.components())
                * u64::from(img.bits_per_component / 8);
            assert_eq!(img.data.len() as u64, want, "raster size");
            assert!(want <= limits.max_output as u64, "the ceiling was exceeded");

            // Table 11.1's channel counts survive the expansion: an indexed
            // image never comes back indexed, and an alpha channel is never
            // invented for a colour type that has none and no tRNS.
            let keyed = scan.transparency.is_some();
            let expected = match (scan.header.colour_type, keyed) {
                (0, false) => PngColour::Grey,
                (0, true) | (4, _) => PngColour::GreyAlpha,
                (6, _) => PngColour::Rgba,
                (_, true) => PngColour::Rgba,
                (_, false) => PngColour::Rgb,
            };
            assert_eq!(img.colour, expected, "colour layout");

            // A second, larger ceiling must not change the picture. Only the
            // refusal may differ, and only in the direction that has more room.
            let roomier = Limits::new(limits.max_output.saturating_mul(4).max(1 << 22));
            match png_decode(body, &roomier) {
                Ok(again) => assert_eq!(again.data, img.data, "the ceiling changed the picture"),
                Err(e) => panic!("a roomier ceiling refused what a tighter one decoded: {e}"),
            }
        }
    } else {
        // A file the scan refuses is refused by the decoder for the same
        // reason: the decoder is the scan plus pixels, and nothing may slip
        // between them.
        assert!(
            png_decode(body, &limits).is_err(),
            "the decoder accepted a file the scan refused"
        );
    }

    // The tightest ceiling there is, on every input: `ExceedsOutputLimit` is
    // the one refusal that has nothing to do with the file's contents, and it
    // must still be a refusal rather than a truncated raster.
    match png_decode(body, &Limits::new(1)) {
        Ok(img) => assert!(img.data.len() <= 1),
        Err(PngError::ExceedsOutputLimit { bytes, limit }) => {
            assert_eq!(limit, 1);
            assert!(bytes > 1);
        }
        Err(_) => {}
    }
});
