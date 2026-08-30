//! Brotli (RFC 7932): the bit reader, both prefix-code representations, the
//! block-switching machinery, the context maps, and the static dictionary.
//!
//! What makes a fuzzer the right instrument here is that a Brotli meta-block
//! is **four independent description systems layered over one bit stream**,
//! and none of them validates another. The header describes prefix codes; the
//! prefix codes describe symbols; the symbols index context maps; the context
//! maps index back into the array of prefix codes. A hand-built fixture makes
//! all four agree by construction. An input that damages one layer and leaves
//! the others intact is where a decoder indexes an array it built from a
//! different number — and a fixture author does not write those.
//!
//! Two loops in this format can be made to consume nothing at all, which is
//! the property this target exists to keep true. A prefix code with one symbol
//! reads **no bits** (§3.4, §3.5), and a static-dictionary transform can
//! produce an **empty** word (`OmitFirst9` of a four-byte word, §8). A command
//! that did both would advance neither the reader nor the output; the decoder
//! names that case rather than trusting the input to make progress, and a hang
//! here is as much a bug as a panic.
//!
//! The control byte picks the **bounds** rather than the input, in the shape
//! `zip_archive` landed and `png` copied: gap 18 milestone 8 found a work cap
//! set above the most its own inputs could ask for, so a target whose limits
//! are all shipped defaults never explores a refusal. One byte is deliberately
//! in the set, because it is the only ceiling from which `ExceedsOutputLimit`
//! fires on an otherwise perfectly good stream.
//!
//! What is asserted beyond "it did not panic":
//!
//! - **A decode that succeeds respects the ceiling it was given.** WOFF2 sizes
//!   a table directory from this length and then reads the result against it,
//!   so a decoder that returned more than it promised is read past downstream
//!   rather than noticed here.
//! - **A roomier ceiling never changes the bytes.** Decoding the same stream
//!   under two ceilings must give the same output or a named refusal — the
//!   ceiling is a budget, not a parameter of the format.
//! - **A refusal at one ceiling is never a panic at another.** The tightest
//!   ceiling there is runs on every input, because `ExceedsOutputLimit` is the
//!   one refusal that has nothing to do with the stream's contents.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_filters::{brotli_decode, BrotliError, Limits};

fuzz_target!(|data: &[u8]| {
    let (control, body) = data.split_at(data.len().min(1));
    let knobs = control.first().copied().unwrap_or(0);

    // Small enough that a decompression bomb is refused in milliseconds, and
    // varied enough that both sides of the ceiling are reachable from one
    // corpus.
    let limits = Limits::new(match knobs & 3 {
        0 => 1,
        1 => 1 << 10,
        2 => 1 << 16,
        _ => 1 << 22,
    });

    match brotli_decode(body, &limits) {
        Ok(out) => {
            assert!(
                out.len() <= limits.max_output,
                "a decode returned {} bytes against a ceiling of {}",
                out.len(),
                limits.max_output
            );

            // A larger budget must not change the answer. Only the refusal may
            // differ, and only in the direction that has more room.
            let roomier = Limits::new(limits.max_output.saturating_mul(4).max(1 << 22));
            match brotli_decode(body, &roomier) {
                Ok(again) => assert_eq!(again, out, "the ceiling changed the output"),
                Err(error) => {
                    panic!("a roomier ceiling refused what a tighter one decoded: {error}")
                }
            }
        }
        Err(BrotliError::ExceedsOutputLimit { limit }) => {
            assert_eq!(limit, limits.max_output, "the refusal named another ceiling");
        }
        Err(_) => {}
    }

    // The tightest ceiling there is, on every input.
    match brotli_decode(body, &Limits::new(1)) {
        Ok(out) => assert!(out.len() <= 1),
        Err(BrotliError::ExceedsOutputLimit { limit }) => assert_eq!(limit, 1),
        Err(_) => {}
    }
});
