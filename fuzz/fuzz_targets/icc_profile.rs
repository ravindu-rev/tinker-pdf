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
//! makes `pow` return something that is not a number. Applying a lookup-table
//! transform over one, three or four inputs reads it through its grid, which
//! evaluates the table's pipeline at each node the colours reach
//! (`docs/design/icc.md`, P-22), so a hostile table builds its grid here too.
//! The other direction is compiled for every intent as well: a profile's
//! transform out of the connection space inverts its matrix and its curves
//! when it is built, where a singular matrix or a falling curve is a typed
//! refusal (`docs/design/icc.md`, "The output transform").
//!
//! # What this target checks, and what it does not
//!
//! **Only that the code did not panic, hang, or exhaust memory.** What the
//! calls below return is discarded. A profile whose tag table was read at the
//! wrong offsets, or whose curve came back with the wrong points, passes as
//! long as the arithmetic did not overflow. So a run that returned the *wrong* answer
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

use tinker_pdf_color::icc::{Intent, Output, Profile, Transform, Transforms};

fuzz_target!(|data: &[u8]| {
    let Ok(profile) = Profile::parse(data) else {
        return;
    };
    // Every intent's transform, each compiled on its own and through the set
    // a colour space carries: an intent reads a table relative colorimetric's
    // parse did not, so a hostile `A2B0` or `A2B2` is reached only here.
    for intent in Intent::ALL {
        if let Some(transform) = Transform::compile(&profile, intent) {
            apply_all(&transform);
        }
        // And the other direction: a model inverted when it is built, where
        // a hostile matrix or curve is a refusal and never a panic.
        if let Ok(output) = Output::compile(&profile, intent) {
            output_all(&output);
        }
    }
    let Some(transforms) = Transforms::compile(profile) else {
        return;
    };
    for intent in Intent::ALL {
        apply_all(transforms.get(intent));
    }
});

/// A transform out of the connection space must answer for any value a
/// caller hands it: black, the PCS white, a Lab white, values past either end
/// of the axis, and values that are not numbers.
fn output_all(output: &Output) {
    for pcs in [
        [0.0, 0.0, 0.0],
        [0.9642, 1.0, 0.8249],
        [100.0, 0.0, 0.0],
        [0.5, 0.25, 0.75],
        [-1.0, 2.0, 1e300],
        [f64::NAN, f64::INFINITY, f64::NEG_INFINITY],
    ] {
        let _ = output.apply(pcs);
    }
}

/// A compiled transform must answer for any components a content stream
/// could name, including the ones outside 0..1 that `scn` is free to write,
/// and for the wrong number of them, which a malformed `/N` produces; and for
/// a row of every byte value in every channel, which reaches every entry of
/// the row path's tables (`docs/design/icc.md`, P-22, "The row path").
fn apply_all(transform: &Transform) {
    for components in [
        [0.0, 0.0, 0.0],
        [1.0, 1.0, 1.0],
        [0.5, 0.25, 0.75],
        [-1.0, 2.0, f64::NAN],
        [f64::INFINITY, f64::NEG_INFINITY, 0.0],
    ] {
        let _ = transform.apply(&components);
    }
    let _ = transform.apply(&[]);
    let _ = transform.apply(&[0.5]);

    // Each byte in every channel, then against its complement in every
    // other, then a partial pixel.
    let inputs = transform.inputs().max(1);
    let mut samples = Vec::with_capacity(512 * inputs + 1);
    for byte in 0..=u8::MAX {
        samples.extend(std::iter::repeat_n(byte, inputs));
    }
    for byte in 0..=u8::MAX {
        for channel in 0..inputs {
            samples.push(if channel % 2 == 0 { byte } else { !byte });
        }
    }
    samples.push(7);
    let mut rgb = Vec::new();
    transform.convert_row(&samples, &mut rgb);
}
