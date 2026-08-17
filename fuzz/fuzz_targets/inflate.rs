//! Own inflate. Truncated and corrupt streams are the normal case in real
//! files, so decoding part of one and warning is correct; panicking is not.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_filters::{flate_decode, inflate_raw, Limits, PredictorParams};

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

    // And through the other door, because there are two now. `inflate_raw`
    // skips the zlib sniff, so it reaches the state machine on inputs
    // `flate_decode` hands to the wrapper first, and it is the entry point a
    // ZIP entry's attacker-chosen bytes will arrive at from gap 29's milestone
    // 2 onwards.
    let raw = inflate_raw(data, &limits);
    assert!(raw.data.len() <= limits.max_output);
    // `end` indexes the caller's own slice, and milestone 2 slices with it to
    // find a data descriptor. Out of range is a panic in the consumer rather
    // than here, which is exactly the kind of defect a fuzzer should own.
    assert!(raw.end <= data.len());
    assert!(!(raw.capped && raw.complete));
});
