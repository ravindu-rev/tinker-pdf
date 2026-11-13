//! Command streams (`docs/design/command-stream.md`): arbitrary bytes read by
//! `tinker_pdf_content::commands` and by the facade's `CommandStream` under
//! budgets, every stream that reads written back and read again, and every
//! page stream the facade reads drawn.
//!
//! The reader is the one surface of the command stream that takes bytes a
//! host hands it, so it is held to ruling 1 here: every length, count,
//! reference and nesting a stream declares is hostile, and so is every table
//! a replay answers the renderer from — an outline, a decode, a shading, a
//! pattern, a cell's stream, a scope. The budgets are the design's with a
//! byte budget of 16 MiB, so a stream that would have the reader hold more
//! is refused by name inside one iteration rather than after it.
//!
//! What is asserted beyond "it did not panic":
//!
//! - **What reads, writes back and reads the same.** A stream whose events
//!   decode is encoded again, and the encoding decodes under the default
//!   budgets and encodes to the same bytes: the format has one encoding a
//!   list of events can have, and a field the reader took in a way it cannot
//!   write back fails here. A page stream the facade reads is written back at
//!   its own version, and those bytes read to the same records and write the
//!   same bytes again.
//! - **What reads, draws.** A page stream the facade reads is rendered whole,
//!   at the scale that fits its page in a square of `SQUARE` pixels and
//!   inside that square, so a replay asks for every outline, image, shading,
//!   pattern and cell the stream's calls reach; the bitmap is the corner of
//!   the page `pixel_size` gives, cut to the square.
//!
//! # What this target cannot find, and what covers it instead
//!
//! The relations hold the codec and the reader to themselves, so a field
//! written and read wrong the same way both times passes; `commands.rs`'s
//! `every_event_round_trips_to_the_bit` holds every field of every event to a
//! recording made by hand. Whether a stream draws its page is
//! `crates/tinker-pdf/tests/determinism.rs`'s replay of every fingerprinted
//! page's stream, and `crates/tinker-pdf/tests/command_stream.rs`'s under
//! every option a render reads.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf::{CommandBudgets, CommandStream, PixelRegion, RenderOptions};
use tinker_pdf_content::commands::{decode_events, encode_events};

/// The side of the square a page stream is drawn in, in pixels.
const SQUARE: u32 = 64;

fuzz_target!(|data: &[u8]| {
    let budgets = CommandBudgets {
        bytes: 1 << 24,
        ..CommandBudgets::DEFAULT
    };
    codec(data, &budgets);
    facade(data, &budgets);
});

/// The codec's relation: events that decode encode to a stream that decodes
/// and encodes to the same bytes.
fn codec(data: &[u8], budgets: &CommandBudgets) {
    if let Ok(events) = decode_events(data, budgets) {
        let again = encode_events(&events);
        let twice = decode_events(&again, &CommandBudgets::DEFAULT).unwrap_or_else(|error| {
            panic!("a stream written from what was read did not read: {error}")
        });
        assert_eq!(
            encode_events(&twice),
            again,
            "a stream written from what was read wrote other bytes again"
        );
    }
}

/// The facade's: a page stream that reads writes back to bytes that read to
/// the same records and write the same bytes, and draws.
fn facade(data: &[u8], budgets: &CommandBudgets) {
    let Ok(stream) = CommandStream::parse_with(data, budgets) else {
        return;
    };
    {
        let again = stream.to_bytes();
        let twice =
            CommandStream::parse_with(&again, &CommandBudgets::DEFAULT).unwrap_or_else(|error| {
                panic!("a page stream written from what was read did not read: {error}")
            });
        assert!(
            twice.records() == stream.records(),
            "a page stream written from what was read read to other records"
        );
        assert!(
            twice.to_bytes() == again,
            "a page stream written from what was read wrote other bytes again"
        );
    }

    let (width, height) = stream.size();
    let extent = width.abs().max(height.abs());
    let side = f64::from(SQUARE);
    let scale = if extent.is_finite() && extent > side {
        side / extent
    } else {
        1.0
    };
    let options = RenderOptions {
        scale,
        region: Some(PixelRegion::new(0, 0, SQUARE, SQUARE)),
        ..RenderOptions::default()
    };
    let bitmap = stream.render(&options);
    let (full_width, full_height) = stream.pixel_size(&options);
    assert_eq!(
        (bitmap.width, bitmap.height),
        (full_width.min(SQUARE), full_height.min(SQUARE)),
        "the drawing is not the corner of the page its size names"
    );
}
