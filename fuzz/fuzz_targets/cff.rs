//! CFF and its Type 2 charstrings: subroutine recursion, `seac`, `flex`, and
//! the hintmask whose length depends on how many stems were declared before
//! it — the operator most often counted wrongly.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_font::Cff;

fuzz_target!(|data: &[u8]| {
    let Some(cff) = Cff::parse(data) else {
        return;
    };
    let count = cff.glyph_count().min(64) as u16;
    for glyph in 0..count {
        let _ = cff.outline(glyph);
        let _ = cff.advance(glyph);
    }
});
