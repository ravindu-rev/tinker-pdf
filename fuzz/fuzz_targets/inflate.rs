//! Own inflate. Truncated and corrupt streams are the normal case in real
//! files, so decoding part of one and warning is correct; panicking is not.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_filters::{flate_decode, Limits, PredictorParams};

fuzz_target!(|data: &[u8]| {
    let limits = Limits::new(1 << 20);
    let _ = flate_decode(data, &limits, None);

    // Again through a predictor, which reinterprets the output as rows and is
    // where a length that does not divide evenly goes wrong.
    let predictor = PredictorParams {
        predictor: 12,
        colors: 3,
        bits_per_component: 8,
        columns: 7,
    };
    let _ = flate_decode(data, &limits, Some(&predictor));
});
