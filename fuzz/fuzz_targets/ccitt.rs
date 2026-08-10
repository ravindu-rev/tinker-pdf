//! CCITT G3 and G4. The first two bytes of the input choose the parameters,
//! so one corpus explores both dimensions and every combination of the flags
//! rather than needing a target each.
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
        rows: 0,
        black_is_1: knobs & 4 != 0,
        byte_align: knobs & 8 != 0,
    };
    let _ = ccitt_decode(body, &params, 1 << 20);
});
