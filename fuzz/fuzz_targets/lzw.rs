//! LZW, including the early-change convention that differs by one bit and
//! sends a decoder off the end of its table when it is read wrongly.
//!
//! # What this target checks, and what it does not
//!
//! **Only that the code did not panic, hang, or exhaust memory.** Both calls
//! are discarded. The early-change convention this target's own header calls
//! out — the one that differs by a single bit — produces *different bytes*,
//! not a panic, when it is read wrongly. So a run that returned the *wrong*
//! answer passes this target exactly as a correct one does, and a green
//! `cargo fuzz` here is evidence about ruling 1 and about nothing else.
//!
//! That is worth writing down rather than leaving implied. Correctness lives
//! in `crates/tinker-pdf-filters/tests/vectors.rs`, which holds an LZW strip
//! produced by libtiff and 7.4.4.2's worked example. **That is the check
//! that would catch an early-change defect; this target would not.**
//!
//! Recorded because the same shape has already cost this repository once: the
//! `brotli` target asserts only self-consistency and could not have found the
//! ring-buffer defect that a decoded-bytes comparison found immediately.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_filters::{lzw_decode, Limits};

fuzz_target!(|data: &[u8]| {
    let limits = Limits::new(1 << 20);
    let _ = lzw_decode(data, &limits, true, None);
    let _ = lzw_decode(data, &limits, false, None);
});
