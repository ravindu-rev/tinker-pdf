//! Baseline JPEG: Huffman tables, restart markers, and the component
//! sampling factors that decide how much memory a frame claims.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_filters::jpeg_decode;

fuzz_target!(|data: &[u8]| {
    let _ = jpeg_decode(data);
});
