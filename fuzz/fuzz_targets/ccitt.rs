//! CCITT G3 and G4. The first two bytes of the input choose the parameters,
//! so one corpus explores both dimensions and every combination of the flags
//! rather than needing a target each.
//!
//! Every knob lives in the *first* byte and every new one is added at a higher
//! bit, so a seed written for an earlier set of parameters keeps both its body
//! and its meaning: the five original seeds have zeros in bits 4 to 7, which
//! is `/EndOfLine false`, `/EndOfBlock true` and `/Rows 0` — exactly what they
//! were decoded with before those parameters existed.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_filters::{ccitt_decode, CcittParams};

fuzz_target!(|data: &[u8]| {
    let (control, body) = data.split_at(data.len().min(2));
    let knobs = control.first().copied().unwrap_or(0);
    let params = CcittParams {
        k: match knobs & 3 {
            0 => 0,
            1 => -1,
            _ => 4,
        },
        // Kept small so a fuzzer's time goes into the decoder rather than
        // into allocating rows.
        columns: u32::from(control.get(1).copied().unwrap_or(8)).max(1),
        // `/EndOfBlock false` means nothing without a row count — it is the
        // parameter that says where the image stops when no EOFB will ever
        // arrive — so a non-zero `/Rows` has to be reachable. Zero stays
        // reachable as well: that is the "until the data runs out" shape.
        rows: match (knobs >> 6) & 3 {
            0 => 0,
            1 => 1,
            2 => 4,
            _ => 64,
        },
        black_is_1: knobs & 4 != 0,
        byte_align: knobs & 8 != 0,
        end_of_line: knobs & 16 != 0,
        end_of_block: knobs & 32 == 0,
    };
    let _ = ccitt_decode(body, &params, 1 << 20);
});
