//! Bare Type 1: eexec decryption, charstring decryption, `/CharStrings`, and
//! the Type 1 charstring interpreter with its othersubr machinery.
//!
//! Two layers of a stream cipher stand between the input and the interpreter,
//! so every byte of a damaged program decrypts to *something* and the
//! interpreter sees operand streams no writer would ever emit. That is the
//! reason this format earns a target of its own rather than sharing `cff`'s.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_font::Type1;

fuzz_target!(|data: &[u8]| {
    let Some(font) = Type1::parse(data) else {
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
});
