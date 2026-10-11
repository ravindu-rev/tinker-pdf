//! bzip2: the block header, the symbol map, the selectors and their
//! move-to-front, the delta-coded Huffman lengths, the zero-run coding, the
//! inverse Burrows–Wheeler transform and the run-length pass after it.
//!
//! What makes a fuzzer the right instrument here is that a bzip2 block is
//! **five description systems in sequence, each sized by the one before**: the
//! symbol map sizes the alphabet, the alphabet sizes every Huffman table, the
//! selector count sizes the walk through them, the decoded symbols size the
//! block, and the origin pointer indexes it. A fixture a real encoder wrote
//! makes all five agree. An input that damages one and leaves the rest intact
//! — a selector naming a sixth group of five, a zero run whose weight doubles
//! past two million, an origin one past the block — is where a decoder
//! indexes an array it sized from a different number, and no encoder writes
//! those.
//!
//! The control byte picks the output ceiling rather than the input, in the
//! shape `zip_archive` landed and `brotli` copied: gap 18 milestone 8 found a
//! work cap set above the most its own inputs could ask for, so a target whose
//! limits are all shipped defaults never explores a refusal. One byte is in
//! the set on purpose, because a single run-length count reaches it.
//!
//! What is asserted beyond "it did not panic":
//!
//! - **A decode that succeeds respects its ceiling.** ZIP and 7z size nothing
//!   from the output, but both hold it to a declared length, and a decoder
//!   that returned more than it promised would be read past there rather than
//!   noticed here.
//! - **A roomier ceiling never changes the bytes.** The ceiling is a budget,
//!   not a parameter of the format: the same stream under a larger one must
//!   give the same output, and the only refusal that may differ between two
//!   ceilings is [`Error::TooLarge`].
//!
//! # What this target cannot find, and what covers it instead
//!
//! Every assertion above is the decoder agreeing with itself. None of them
//! asks whether the bytes are what the stream *means* — bzip2's own block and
//! stream CRCs do, inside the decoder, and a stream the fuzzer builds past
//! them is noise that happens to checksum. Correctness is held where the
//! expected answer is known: `crates/tinker-pdf-archive/tests/coders.rs`,
//! whose archives CPython and py7zr wrote over files this repository made,
//! and whose expected output is those files.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_archive::bzip2::{decode, Error, Limits};

fuzz_target!(|data: &[u8]| {
    let (control, body) = data.split_at(data.len().min(1));
    let knobs = control.first().copied().unwrap_or(0);
    let limits = Limits {
        max_unpacked: match knobs & 3 {
            0 => 1,
            1 => 1 << 10,
            2 => 1 << 16,
            _ => 1 << 22,
        },
    };

    match decode(body, &limits) {
        Ok(out) => {
            assert!(
                out.len() <= limits.max_unpacked,
                "a decode returned {} bytes against a ceiling of {}",
                out.len(),
                limits.max_unpacked
            );
            let roomier = Limits {
                max_unpacked: limits.max_unpacked.saturating_mul(4).max(1 << 22),
            };
            match decode(body, &roomier) {
                Ok(again) => assert_eq!(again, out, "the ceiling changed the output"),
                Err(error) => {
                    panic!("a roomier ceiling refused what a tighter one decoded: {error}")
                }
            }
        }
        Err(Error::TooLarge) => {}
        Err(error) => {
            // Anything but the ceiling is a fact about the stream, so a
            // roomier ceiling refuses it the same way.
            let roomier = Limits {
                max_unpacked: limits.max_unpacked.saturating_mul(4).max(1 << 22),
            };
            assert_eq!(
                decode(body, &roomier).err(),
                Some(error),
                "a refusal that was not the ceiling changed with the ceiling"
            );
        }
    }
});
