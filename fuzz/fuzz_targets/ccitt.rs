//! CCITT G3 and G4. The first two bytes of the input choose the parameters,
//! so one corpus explores both dimensions and every combination of the flags
//! rather than needing a target each.
//!
//! Every knob lives in the *first* byte and every new one is added at a higher
//! bit, so a seed written for an earlier set of parameters keeps both its body
//! and its meaning: the five original seeds have zeros in bits 4 to 7, which
//! is `/EndOfLine false`, `/EndOfBlock true` and `/Rows 0` — exactly what they
//! were decoded with before those parameters existed. The byte is full now,
//! so the one knob added since — where in the bitstream `T6Rows` starts —
//! reads bits already spoken for rather than a byte no seed carries.
//!
//! The same body then goes through both entry points, because they are two
//! ways in rather than two decoders: `decode` owns `/K`, `/Rows`, EOL and
//! EOFB and starts at bit zero, and `T6Rows` is the T.6 rows alone from
//! wherever a caller says.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_filters::{ccitt_decode, CcittParams, T6Rows};

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
    let (packed, _) = ccitt_decode(body, &params, 1 << 20);
    // The shape the caller re-lays rows from: whole rows of `/Columns` bits,
    // padded to a byte. A result that is not a whole number of rows puts every
    // row after the ragged one at the wrong offset, which is an image that
    // looks decoded and is not.
    let stride = (params.columns as usize).div_ceil(8);
    assert_eq!(packed.len() % stride, 0, "a partial row came back");

    // `T6Rows` is the same T.6 coding with none of 7.4.6's wrapper around it:
    // the caller says where in the bitstream a region starts and pulls rows one
    // at a time. Nothing above reaches it — `decode` starts at bit zero and
    // owns its own loop — so a stream that is fine whole-file says nothing
    // about the seam an MMR region enters through. The offset is the low three
    // bits of the knob byte, already spent on `/K` and `/BlackIs1`, so no seed
    // changes meaning and a row boundary still lands off a byte boundary.
    let mut rows = T6Rows::new(body, usize::from(knobs & 7), params.columns);
    // A byte of slack past the row with a sentinel in it: `next_row` takes any
    // buffer at least a row long, and a region decoder hands it a slice of
    // something bigger.
    let mut row = vec![0x5Au8; rows.row_bytes() + 1];
    let guard = row.len() - 1;
    for _ in 0..1024 {
        let before = rows.bit_position();
        if !rows.next_row(&mut row) {
            break;
        }
        assert!(
            rows.bit_position() > before,
            "a row decoded out of no bits, so a caller pulling rows until one \
             fails never stops"
        );
        assert_eq!(row[guard], 0x5A, "a row was written past its own width");
    }
});
