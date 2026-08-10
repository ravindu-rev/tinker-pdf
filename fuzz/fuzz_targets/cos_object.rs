//! The object grammar alone, without a file around it.
//!
//! Separated from the document target so a crash here points at 7.3 rather
//! than at the cross-reference machinery, and so the corpus stays small
//! enough to explore the grammar properly.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_cos::{parse_indirect_at, parse_object_at};

fuzz_target!(|data: &[u8]| {
    let _ = parse_object_at(data, 0);
    let _ = parse_indirect_at(data, 0);

    // Also from an offset inside the buffer, which is how the repair scanner
    // reaches objects and a common source of off-by-one reads.
    if data.len() > 4 {
        let _ = parse_object_at(data, data.len() / 2);
    }
});
