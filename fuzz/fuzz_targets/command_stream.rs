//! Command streams (`docs/design/command-stream.md`): arbitrary bytes read by
//! `tinker_pdf_content::commands` under budgets, and every stream that reads
//! written back and read again.
//!
//! The reader is the one surface of the command stream that takes bytes a
//! host hands it, so it is held to ruling 1 here: every length, count,
//! reference and nesting a stream declares is hostile. The budgets are the
//! design's with a byte budget of 16 MiB, so a stream that would have the
//! reader hold more is refused by name inside one iteration rather than
//! after it.
//!
//! What is asserted beyond "it did not panic":
//!
//! - **What reads, writes back and reads the same.** A stream that decodes is
//!   encoded again, and the encoding decodes under the default budgets and
//!   encodes to the same bytes: the format has one encoding a list of events
//!   can have, and a field the reader took in a way it cannot write back
//!   fails here.
//!
//! # What this target cannot find, and what covers it instead
//!
//! The relation holds the codec to itself, so a field written and read wrong
//! the same way both times passes; `commands.rs`'s
//! `every_event_round_trips_to_the_bit` holds every field of every event to a
//! recording made by hand. Whether a stream draws its page is the facade's
//! replay, which `crates/tinker-pdf/tests/determinism.rs` holds to every
//! fingerprinted page.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_content::commands::{decode_events, encode_events, Budgets};

fuzz_target!(|data: &[u8]| {
    let budgets = Budgets {
        bytes: 1 << 24,
        ..Budgets::DEFAULT
    };
    if let Ok(events) = decode_events(data, &budgets) {
        let again = encode_events(&events);
        let twice = decode_events(&again, &Budgets::DEFAULT).unwrap_or_else(|error| {
            panic!("a stream written from what was read did not read: {error}")
        });
        assert_eq!(
            encode_events(&twice),
            again,
            "a stream written from what was read wrote other bytes again"
        );
    }
});
