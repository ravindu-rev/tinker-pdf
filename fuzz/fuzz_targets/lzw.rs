//! LZW, including the early-change convention that differs by one bit and
//! sends a decoder off the end of its table when it is read wrongly.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_filters::{lzw_decode, Limits};

fuzz_target!(|data: &[u8]| {
    let limits = Limits::new(1 << 20);
    let _ = lzw_decode(data, &limits, true, None);
    let _ = lzw_decode(data, &limits, false, None);
});
