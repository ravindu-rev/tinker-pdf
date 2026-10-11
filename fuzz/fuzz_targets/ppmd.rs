//! PPMd variant H with 7z's range coder: the model, its twelve-byte-unit
//! allocator, the binary and masked-context paths, and the restart when the
//! arena runs out.
//!
//! What makes a fuzzer the right instrument here is that PPMd has **no
//! structure in its input at all** past a zero byte: every bit the stream
//! holds is an arithmetic-coded choice among the symbols the model offers,
//! so any bytes decode to *something* for a while, and what they steer is the
//! model — which contexts get created, which frequencies overflow into a
//! rescale, which free blocks get glued, when the arena fills. A fixture
//! steers it where its encoder's text went. A fuzzer steers it everywhere
//! else, including through orders and arena sizes no real archive uses
//! together.
//!
//! The first three bytes are parameters rather than input, the shape
//! `brotli` and `bzip2` use for their ceilings: the model order less two,
//! the arena as a power of two over 2 KiB (2 KiB, the smallest 7-Zip accepts,
//! to 4 MiB), and the length to decode in sixteens. The arena's low end is
//! deliberate: at 2 KiB the model restarts every few dozen symbols, so the
//! allocator's rarest paths are its commonest here.
//!
//! What is asserted beyond "it did not panic":
//!
//! - **A decode that succeeds is exactly the length it was asked for.** 7z
//!   hands the output on as a folder whose substreams are sized from that
//!   number.
//! - **A roomier cap never changes the answer.** The caps are budgets, so the
//!   same stream under larger ones must decode to the same bytes, and a
//!   refusal that was not a cap must be the same refusal.
//!
//! # What this target cannot find, and what covers it instead
//!
//! Both assertions are the decoder agreeing with itself. A model that is
//! wrong in the same way on every run passes them — and PPMd's failure mode
//! is exactly that: one misplaced frequency update decodes plausible bytes
//! from then on. What catches it is `crates/tinker-pdf-archive/tests/coders.rs`,
//! whose archives py7zr wrote over files made here and whose 7z CRC-32s are
//! over the original bytes, and `src/ppmd/tests.rs`, whose streams are 7-Zip's
//! encoder in a 2 KiB arena.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_archive::ppmd::{decode, Error, Limits};

fuzz_target!(|data: &[u8]| {
    let (control, body) = data.split_at(data.len().min(3));
    let byte = |i: usize| control.get(i).copied().unwrap_or(0);
    let order = 2 + byte(0) % 63;
    let arena = 2048u32 << (byte(1) % 12);
    let unpacked = usize::from(byte(2)) * 16;
    let mut props = vec![order];
    props.extend_from_slice(&arena.to_le_bytes());

    let limits = Limits {
        max_unpacked: 4096,
        max_memory: 1 << 22,
    };
    let roomier = Limits {
        max_unpacked: 1 << 20,
        max_memory: 1 << 24,
    };
    match decode(body, &props, unpacked, &limits) {
        Ok(out) => {
            assert_eq!(
                out.len(),
                unpacked,
                "a decode is the length it was asked for"
            );
            assert_eq!(
                decode(body, &props, unpacked, &roomier).as_ref(),
                Ok(&out),
                "a roomier cap changed the answer"
            );
        }
        Err(Error::TooLarge) => {}
        Err(error) => {
            assert_eq!(
                decode(body, &props, unpacked, &roomier).err(),
                Some(error),
                "a refusal that was not a cap changed with the caps"
            );
        }
    }
});
