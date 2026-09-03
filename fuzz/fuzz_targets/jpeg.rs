//! Baseline JPEG: Huffman tables, restart markers, and the component
//! sampling factors that decide how much memory a frame claims.
//!
//! # What this target checks, and what it does not
//!
//! **Only that the code did not panic, hang, or exhaust memory.** The single
//! call below is `let _ = jpeg_decode(data, 1 << 22);`. Nothing looks at the
//! raster, its dimensions, or its component count. So a run that returned the
//! *wrong* answer passes this target exactly as a correct one does, and a
//! green `cargo fuzz` here is evidence about ruling 1 and about nothing else.
//!
//! That is worth writing down rather than leaving implied. Correctness lives
//! in `crates/tinker-pdf-filters`'s own decoder tests and in the rendered
//! page comparisons. A wrong JPEG decode is a plausible photograph, which is
//! precisely the failure a non-panic target cannot see.
//!
//! Recorded because the same shape has already cost this repository once: the
//! `brotli` target asserts only self-consistency and could not have found the
//! ring-buffer defect that a decoded-bytes comparison found immediately.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_filters::jpeg_decode;

fuzz_target!(|data: &[u8]| {
    let _ = jpeg_decode(data, 1 << 22);
});
