//! Type 42: the PostScript wrapper around a TrueType face — the scanner, the
//! structural reading of `/FontType`, `/FontMatrix`, `/Encoding`,
//! `/CharStrings` and the `/sfnts` strings, the face they reassemble, and its
//! outlines.
//!
//! # What this target checks, and what it does not
//!
//! **Only that the code did not panic, hang, or exhaust memory.** Every call
//! is discarded: a wrapper read *wrongly* passes this target exactly as one
//! read rightly does. Correctness lives in `crates/tinker-pdf-font`'s own
//! tests, which read back the face a wrapper was built around byte for byte.
//!
//! The format's signature is its first line, `%!PS-TrueTypeFont`, and the
//! target eats no control prefix, so `cargo xtask fuzz` holds every seed to
//! beginning with it.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_font::{glyf, Type42};

fuzz_target!(|data: &[u8]| {
    let _ = Type42::is_type42(data);
    let Ok(font) = Type42::parse(data) else {
        return;
    };

    // Bounded rather than driven by the program's own count, which an input
    // is free to claim is enormous.
    let names: Vec<(Vec<u8>, u16)> = font
        .glyph_names()
        .take(64)
        .map(|(name, glyph)| (name.to_vec(), glyph))
        .collect();
    for (name, _) in &names {
        let _ = font.glyph_for_name(name);
    }
    for code in [0u8, 32, 65, 127, 128, 255] {
        let _ = font.encoding_name(code);
        let _ = font.glyph_for_code(code);
    }
    let _ = font.font_matrix();
    let _ = font.notdef();
    let _ = font.procedure_count();

    let Some(face) = font.sfnt() else {
        return;
    };
    for (_, glyph) in &names {
        let _ = glyf::outline(&face, *glyph);
        let _ = face.advance(*glyph);
    }
    let _ = glyf::outline(&face, font.notdef());
});
