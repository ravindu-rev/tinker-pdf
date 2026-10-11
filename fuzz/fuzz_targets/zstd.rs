//! Zstandard: frame headers, raw, RLE and compressed blocks, the literals
//! section's Huffman description and one- or four-stream layout, the
//! sequences section's three FSE descriptions and its backward bitstream,
//! repeat offsets, and the content checksum.
//!
//! What makes a fuzzer the right instrument here is that a compressed block
//! is **descriptions sized by descriptions, read in two directions**: a
//! literals header sizes a Huffman description whose weights size a table
//! whose codes are read backwards from the end of a stream whose length a
//! jump table gave; a sequences header names three table modes whose
//! descriptions are read forwards, and then a bitstream read backwards
//! steers three states through those tables, one of which picks how many
//! bits the next offset takes. A real encoder makes every one of those
//! agree. A flipped bit leaves the rest intact and is where a decoder
//! indexes a table it sized from another number, reads before a stream's
//! first byte, or copies from before the frame's.
//!
//! The control byte picks the output ceiling rather than the input, in the
//! shape `zip_archive` landed and `bzip2` copied: a target whose limits are
//! all shipped defaults never explores a refusal. One byte is in the set on
//! purpose, because one RLE block reaches it.
//!
//! What is asserted beyond "it did not panic":
//!
//! - **A decode that succeeds respects its ceiling.** ZIP sizes nothing from
//!   the output but holds it to a declared length, and a decoder that
//!   returned more than it promised would be read past there rather than
//!   noticed here.
//! - **A roomier ceiling never changes the bytes.** The ceiling is a budget,
//!   not a parameter of the format: the same stream under a larger one must
//!   give the same output, and the only refusal that may differ between two
//!   ceilings is [`Error::TooLarge`].
//!
//! # What this target cannot find, and what covers it instead
//!
//! Every assertion above is the decoder agreeing with itself. None of them
//! asks whether the bytes are what the stream *means* — a frame's content
//! checksum does, inside the decoder, when the frame carries one, and the
//! fuzzer can build frames past it simply by clearing the flag. Correctness
//! is held where the expected answer is known:
//! `crates/tinker-pdf-archive/src/zstd/tests.rs`, against RFC 8878's
//! Appendix A and the zstd project's golden files, and
//! `crates/tinker-pdf-archive/tests/coders.rs`, whose frames libzstd wrote
//! over files this repository made, and whose expected output is those
//! files.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_archive::zstd::{decode, Error, Limits};

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
    let roomier = Limits {
        max_unpacked: limits.max_unpacked.saturating_mul(4).max(1 << 22),
    };

    match decode(body, &limits) {
        Ok(out) => {
            assert!(
                out.len() <= limits.max_unpacked,
                "a decode returned {} bytes against a ceiling of {}",
                out.len(),
                limits.max_unpacked
            );
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
            assert_eq!(
                decode(body, &roomier).err(),
                Some(error),
                "a refusal that was not the ceiling changed with the ceiling"
            );
        }
    }
});
