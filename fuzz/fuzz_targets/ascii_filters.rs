//! The text filters and the predictors, which are small enough to share a
//! target and all take the same shape of input.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_filters::{
    ascii85_decode, ascii_hex_decode, predictor_decode, run_length_decode, Limits, PredictorParams,
};

fuzz_target!(|data: &[u8]| {
    let limits = Limits::new(1 << 20);
    let _ = ascii_hex_decode(data, &limits);
    let _ = ascii85_decode(data, &limits);
    let _ = run_length_decode(data, &limits);

    let params = PredictorParams {
        predictor: 12,
        colors: 3,
        bits_per_component: 8,
        columns: 8,
    };
    let _ = predictor_decode(data, &params, &limits);
});
