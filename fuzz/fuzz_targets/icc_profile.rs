//! ICC profiles, whose header is a table of offsets into themselves.
//!
//! Every tag names its own offset and size and the parser has to believe
//! neither: a profile is 128 bytes of header followed by a count that is a
//! 32-bit field, then that many twelve-byte entries each pointing anywhere.
//! The arithmetic on those — `offset + size`, `132 + i * 12`, a curve's
//! `12 + count * 2` — is where ruling 1 lives in this format.
//!
//! Compiling is fuzzed beside parsing because it is where the values become
//! tables: a curve with an absurd count, a matrix of infinities, a gamma that
//! makes `pow` return something that is not a number.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_color::icc::{Profile, Transform};

fuzz_target!(|data: &[u8]| {
    let Ok(profile) = Profile::parse(data) else {
        return;
    };
    let Some(transform) = Transform::compile(&profile) else {
        return;
    };
    // A compiled transform must answer for any components a content stream
    // could name, including the ones outside 0..1 that `scn` is free to write.
    for components in [
        [0.0, 0.0, 0.0],
        [1.0, 1.0, 1.0],
        [0.5, 0.25, 0.75],
        [-1.0, 2.0, f64::NAN],
        [f64::INFINITY, f64::NEG_INFINITY, 0.0],
    ] {
        let _ = transform.apply(&components);
    }
    // And for the wrong number of them, which a malformed `/N` produces.
    let _ = transform.apply(&[]);
    let _ = transform.apply(&[0.5]);
});
