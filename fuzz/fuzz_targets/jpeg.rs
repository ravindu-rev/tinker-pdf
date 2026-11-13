//! Baseline JPEG: Huffman tables, restart markers, and the component
//! sampling factors that decide how much memory a frame claims — at every
//! scale a strip decode takes.
//!
//! # What this target checks, and what it does not
//!
//! **Only that the code did not panic, hang, or exhaust memory.** The input's
//! length picks the scale (`docs/design/jpeg-speed.md`, "Geometry"): modulo
//! four, 0 is full size, decoded as `jpeg_decode(data, 1 << 22)`, and 1, 2
//! and 3 are 1/2, 1/4 and 1/8, decoded as `jpeg_decode_strips(data, 1 << 22,
//! scale, &mut Dropped)` into a sink that drops every strip. So every seed
//! reaches a scale, and a mutation that adds or drops a byte reaches another.
//! Nothing looks at the raster, its dimensions, or its component count. So a
//! run that returned the *wrong* answer passes this target exactly as a
//! correct one does, and a green `cargo fuzz` here is evidence about ruling 1
//! and about nothing else.
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

use tinker_pdf_filters::{jpeg_decode, jpeg_decode_strips, JpegLayout, JpegScale, JpegSink};

/// A sink that drops every strip it is handed: what a reduced decode paints
/// is the decoder's tests' to judge, and this target's is only that it ends.
struct Dropped;

impl JpegSink for Dropped {
    fn begin(&mut self, _: JpegLayout) {}

    fn strip(&mut self, _: u32, _: u32, _: &[u8]) {}
}

fuzz_target!(|data: &[u8]| {
    match JpegScale::ALL[data.len() % 4] {
        JpegScale::Full => {
            let _ = jpeg_decode(data, 1 << 22);
        }
        scale => {
            let _ = jpeg_decode_strips(data, 1 << 22, scale, &mut Dropped);
        }
    }
});
