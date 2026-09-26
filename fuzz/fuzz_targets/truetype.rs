//! TrueType: the table directory, `cmap` in every format read, and `glyf`
//! outlines including composites, whose recursion is the interesting part.
//!
//! # What this target checks, and what it does not
//!
//! **Only that the code did not panic, hang, or exhaust memory.** Five calls,
//! every one discarded. A composite glyph assembled with the wrong component
//! transform, or a `cmap` subtable read in the wrong format, produces wrong
//! outlines rather than a crash. So a run that returned the *wrong* answer
//! passes this target exactly as a correct one does, and a green `cargo fuzz`
//! here is evidence about ruling 1 and about nothing else.
//!
//! That is worth writing down rather than leaving implied. Correctness lives
//! in `crates/tinker-pdf-font`'s own tests and in the rendered glyph
//! comparisons.
//!
//! Recorded because the same shape has already cost this repository once: the
//! `brotli` target asserts only self-consistency and could not have found the
//! ring-buffer defect that a decoded-bytes comparison found immediately.
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
