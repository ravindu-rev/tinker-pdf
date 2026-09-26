//! The text filters and the predictors, which are small enough to share a
//! target and all take the same shape of input.
//!
//! # What this target checks, and what it does not
//!
//! **Only that the code did not panic, hang, or exhaust memory.** Every one
//! of the four decoders below is called with `let _ =`, so neither the bytes
//! they produce nor the leniencies they record are looked at. So a run that
//! returned the *wrong* answer passes this target exactly as a correct one
//! does, and a green `cargo fuzz` here is evidence about ruling 1 and about
//! nothing else.
//!
//! That is worth writing down rather than leaving implied. Correctness for
//! these four lives in `crates/tinker-pdf-filters/tests/vectors.rs`, which
//! holds byte vectors with recorded provenance — including 7.4.4.2's `-----A
//! ---B` example verbatim — and that is the only place any of them is held to
//! an output.
//!
//! Recorded because the same shape has already cost this repository once: the
//! `brotli` target asserts only self-consistency and could not have found the
//! ring-buffer defect that a decoded-bytes comparison found immediately.
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
