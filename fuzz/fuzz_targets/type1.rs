//! Bare Type 1: eexec decryption, charstring decryption, `/CharStrings`, and
//! the Type 1 charstring interpreter with its othersubr machinery, a
//! multiple-master program's blend othersubrs among them, at three instances.
//!
//! Two layers of a stream cipher stand between the input and the interpreter,
//! so every byte of a damaged program decrypts to *something* and the
//! interpreter sees operand streams no writer would ever emit. That is the
//! reason this format earns a target of its own rather than sharing `cff`'s.
//!
//! # What this target checks, and what it does not
//!
//! **Only that the code did not panic, hang, or exhaust memory.** Six calls,
//! every one discarded. Both layers of the stream cipher make this worse than
//! usual, as this target's own header says: every byte of a damaged program
//! decrypts to *something*, so a wrong key yields a plausible charstring
//! rather than a failure. So a run that returned the *wrong* answer passes
//! this target exactly as a correct one does, and a green `cargo fuzz` here
//! is evidence about ruling 1 and about nothing else.
//!
//! That is worth writing down rather than leaving implied. Correctness lives
//! in `crates/tinker-pdf-font`'s own tests, which check the glyphs a Type 1
//! program produces.
//!
//! Recorded because the same shape has already cost this repository once: the
//! `brotli` target asserts only self-consistency and could not have found the
//! ring-buffer defect that a decoded-bytes comparison found immediately.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_font::Type1;

fuzz_target!(|data: &[u8]| {
    let Some(mut font) = Type1::parse(data) else {
        return;
    };

    // Bounded rather than driven by the program's own count, which an input
    // is free to claim is enormous.
    let count = font.glyph_count().min(64);

    for index in 0..count {
        // Name-keyed lookup is how a Type 1 glyph is actually addressed: the
        // index is not a glyph id, so the round trip is the thing to check.
        if let Some(name) = font.glyph_name(index) {
            let _ = font.glyph_for_name(name);
        }
    }
    let _ = font.glyph_for_name(b".notdef");
    let _ = font.glyph_for_name(b"");

    // The built-in encoding, which a PDF font dictionary may or may not
    // override.
    for code in [0u8, 32, 65, 127, 128, 255] {
        let _ = font.glyph_for_code(code);
    }

    for glyph in 0..count as u16 {
        // `seac`, `flex` and hint replacement all live behind `outline`;
        // `advance` is `hsbw`/`sbw` and the width-only path.
        let _ = font.outline(glyph);
        let _ = font.advance(glyph);
    }

    // A multiple-master program instanced at three design vectors — every
    // axis at its map's first design coordinate, at its last, and midway —
    // and at one of the wrong length, each outlined through the blend
    // othersubrs; then its default instance again.
    let _ = font.is_multiple_master();
    let _ = font.weight_vector();
    let Some(blend) = font.blend().cloned() else {
        return;
    };
    let _ = blend.is_standard();
    let low: Vec<f64> = blend
        .maps
        .iter()
        .map(|map| map.first().map_or(0.0, |point| point.0))
        .collect();
    let high: Vec<f64> = blend
        .maps
        .iter()
        .map(|map| map.last().map_or(0.0, |point| point.0))
        .collect();
    let middle: Vec<f64> = low.iter().zip(&high).map(|(a, b)| (a + b) / 2.0).collect();
    for design in [low, high, middle, Vec::new()] {
        let _ = font.instance(&design);
        let _ = font.weights();
        for glyph in 0..count as u16 {
            let _ = font.outline(glyph);
            let _ = font.advance(glyph);
        }
    }
    font.default_instance();
    let _ = font.outline(0);
});
