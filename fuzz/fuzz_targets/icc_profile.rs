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
//!
//! # What this target checks, and what it does not
//!
//! **Only that the code did not panic, hang, or exhaust memory.** The three
//! calls below are discarded. A profile whose tag table was read at the wrong
//! offsets, or whose curve came back with the wrong points, passes as long as
//! the arithmetic did not overflow. So a run that returned the *wrong* answer
//! passes this target exactly as a correct one does, and a green `cargo fuzz`
//! here is evidence about ruling 1 and about nothing else.
//!
//! That is worth writing down rather than leaving implied. Correctness lives
//! in `crates/tinker-pdf-color`'s own tests, which check transformed colour
//! values rather than that a parse returned.
//!
//! Recorded because the same shape has already cost this repository once: the
//! `brotli` target asserts only self-consistency and could not have found the
//! ring-buffer defect that a decoded-bytes comparison found immediately.
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
