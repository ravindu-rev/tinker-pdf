//! TrueType: the table directory, `cmap` in every format read, and `glyf`
//! outlines including composites, whose recursion is the interesting part.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_font::{glyf, Sfnt};

fuzz_target!(|data: &[u8]| {
    let Some(font) = Sfnt::parse(data) else {
        return;
    };

    for tag in [b"glyf", b"cmap", b"loca", b"head"] {
        let _ = font.table(u32::from_be_bytes(*tag));
    }
    for c in ['\0', ' ', 'A', '\u{20AC}', '\u{1F600}'] {
        let _ = font.glyph_for_char(c);
    }
    let _ = font.glyph_for_name("A");

    // Bounded rather than driven by the font's own glyph count, which an
    // input is free to claim is enormous.
    for glyph in 0..64u16 {
        let _ = font.advance(glyph);
        let _ = glyf::outline(&font, glyph);
    }
});
