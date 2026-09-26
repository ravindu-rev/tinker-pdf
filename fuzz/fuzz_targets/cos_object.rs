//! The object grammar alone, without a file around it.
//!
//! Separated from the document target so a crash here points at 7.3 rather
//! than at the cross-reference machinery, and so the corpus stays small
//! enough to explore the grammar properly.
//!
//! # What this target checks, and what it does not
//!
//! **Only that the code did not panic, hang, or exhaust memory.** Five calls,
//! every one discarded. An object parsed with the wrong type, a dictionary
//! key silently dropped, or a number read to the wrong value all pass. So a
//! run that returned the *wrong* answer passes this target exactly as a
//! correct one does, and a green `cargo fuzz` here is evidence about ruling 1
//! and about nothing else.
//!
//! That is worth writing down rather than leaving implied. Correctness lives
//! in `crates/tinker-pdf-cos`'s own grammar tests, which compare parsed
//! objects against expected ones.
//!
//! Recorded because the same shape has already cost this repository once: the
//! `brotli` target asserts only self-consistency and could not have found the
//! ring-buffer defect that a decoded-bytes comparison found immediately.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_cos::{parse_indirect_at, parse_object_at, NameTable, WarningSink};

fuzz_target!(|data: &[u8]| {
    let names = NameTable::new();
    let mut sink = WarningSink::new();

    let _ = parse_object_at(data, 0, &names, &mut sink);
    let _ = parse_indirect_at(data, 0, &names, &mut sink);

    // Also from an offset inside the buffer, which is how the repair scanner
    // reaches objects and a common source of off-by-one reads.
    if data.len() > 4 {
        let middle = (data.len() / 2) as u64;
        let _ = parse_object_at(data, middle, &names, &mut sink);
        let _ = parse_indirect_at(data, middle, &names, &mut sink);
    }

    // Past the end: an xref entry is free to point anywhere at all.
    let _ = parse_object_at(data, u64::MAX, &names, &mut sink);
});
