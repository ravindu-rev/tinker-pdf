//! Own inflate. Truncated and corrupt streams are the normal case in real
//! files, so decoding part of one and warning is correct; panicking is not.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_filters::{flate_decode, Limits};

fuzz_target!(|data: &[u8]| {
    let _ = flate_decode(data, &Limits::new(1 << 20));
});
