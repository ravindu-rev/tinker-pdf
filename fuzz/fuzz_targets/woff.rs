//! WOFF 1.0 and WOFF 2.0, which are two compressors wrapped in a table
//! directory that describes itself.
//!
//! Ruling 1's case for this target is short. A WOFF is
//! **document-controlled arithmetic all the way down**: WOFF 1.0's directory
//! states an offset, a compressed length and an original length per table and
//! every one of them is a `u32` a hostile file picks; WOFF 2.0 states its
//! lengths in `UIntBase128`, which reaches 2^32 - 1 in five bytes, so a
//! forty-byte file can ask a decoder for four gigabytes.
//!
//! And the `glyf` transform is worse than either, because it is not one buffer
//! but seven, each with its own cursor, and a glyph's point count, contour
//! count and instruction length are read from three different streams and then
//! multiplied together. `nPoints` summing past what `nContours` can address,
//! a composite naming itself, a bounding box the bitmap says is present and
//! the stream ended before — none of those is reachable from a well-formed
//! file, which is exactly why they belong here rather than in a fixture.
//!
//! The seeds are `fuzz/corpus/woff/`, and
//! `crates/tinker-pdf-font/tests/woff_seeds.rs` replays them on stable so the
//! corpus does not quietly stop describing the parser.
//! # What this target cannot find, and what covers it instead
//!
//! The one assertion here is that the sniffer and the decoder agree about
//! whether a container is WOFF at all. Nothing checks that the sfnt which
//! comes out is the sfnt that went in — a table decompressed to the wrong
//! bytes, or a WOFF2 transform reversed wrongly, yields a face that parses
//! and draws the wrong glyphs.
//!
//! Correctness lives in `crates/tinker-pdf-font/tests/woff_seeds.rs` and the
//! font crate's own tests, which unpack the seven committed files from three
//! encoders and compare the sfnt inside.
//!
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_font::glyf::outline;
use tinker_pdf_font::woff::{decode, packaging};
use tinker_pdf_font::Sfnt;

/// A megabyte, which every seed unpacks far inside. Chosen so the target
/// spends its time in the decoder rather than in an allocator, and so a stated
/// length of four gigabytes is refused by the ceiling rather than honoured.
const LIMIT: usize = 1 << 20;

fuzz_target!(|data: &[u8]| {
    // The sniffer runs on everything, including the inputs that are not
    // containers: it is what the EPUB facade calls first, on bytes it has no
    // other opinion about.
    let announced = packaging(data);

    let Ok(font) = decode(data, LIMIT) else {
        return;
    };

    // Anything that decoded announced itself, or the sniffer and the decoder
    // disagree about what this file is.
    assert!(
        announced.is_some(),
        "decoded a container the sniffer did not recognise"
    );
    assert!(font.len() <= LIMIT, "the ceiling held");

    // A decoder that returns bytes has said they are a font, so they have to
    // survive being read as one. This is where a reconstructed directory with
    // an offset past the end turns from "an odd number" into a panic.
    let Some(sfnt) = Sfnt::parse(&font) else {
        return;
    };
    for glyph in [0u16, 1, 2, 65, 255, 256, u16::MAX] {
        let _ = outline(&sfnt, glyph);
        let _ = sfnt.advance(glyph);
    }
    for c in ['\0', 'A', '\u{FFFD}', '\u{10FFFF}'] {
        let _ = sfnt.glyph_for_char(c);
    }

    // Decoding is a function of the bytes and the ceiling and of nothing else
    // (ruling 4). Cheap to check and it catches the class of bug where a
    // reused buffer leaks between calls.
    assert_eq!(decode(data, LIMIT), Ok(font), "two decodes, one answer");
});
