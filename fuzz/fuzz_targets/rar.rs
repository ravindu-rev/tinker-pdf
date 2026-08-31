//! RAR 5 archives: the `vint` chain, the header CRCs, and the file records.
//!
//! A RAR is a **chain**: every header says how long it is and how much data
//! follows it, and the next header is exactly there. So the arithmetic a
//! fuzzer is the right instrument for is the walk's own — a `HeaderSize` that
//! does not advance is a loop, a `DataSize` of `2^60` in a 40-byte file is an
//! offset past everything, and a `vint` is ten bytes of a length the file
//! chose. None of the three is reachable from a fixture an author wrote on
//! purpose.
//!
//! What makes this target different from `tar` and `sevenz` is that RAR checks
//! itself twice: **every header carries a CRC-32 over itself**, so almost
//! every mutation a fuzzer makes is caught one byte later and the walk stops.
//! That is a real property and it is also a hazard for the corpus — a target
//! whose every input dies at the first header is a target exploring nothing —
//! which is why the seeds are written by the same builder the unit tests use,
//! with correct checksums, so a mutation lands somewhere the walk can reach.
//!
//! The control byte picks the bounds rather than the input, in the shape
//! `zip_archive.rs` established.
//!
//! What is asserted beyond "it did not panic":
//!
//! - **A read that succeeds returned exactly the entry's declared size**, and
//!   **a CRC-32 matching the one the archive recorded** when it recorded one.
//!   The second is the format adjudicating the extraction, asserted from
//!   outside so a `read` that stopped checking is caught here too.
//! - **A read that succeeds returned a range of the input.** Every entry this
//!   build reads is stored, so every successful read is a borrow, and that is
//!   checkable by address.
//! - **Every listed entry answers, one way or the other.**
//! - **The entry list does not change under reading.**
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_archive::rar::{Archive, EntryError, Kind, Limits};
use tinker_pdf_filters::crc32;

fuzz_target!(|data: &[u8]| {
    let (control, body) = data.split_at(data.len().min(1));
    let knobs = control.first().copied().unwrap_or(0);

    let limits = Limits {
        max_entries: match knobs & 3 {
            0 => 1,
            1 => 4,
            2 => 64,
            _ => 4096,
        },
        max_header_bytes: match (knobs >> 2) & 3 {
            0 => 8,
            1 => 64,
            2 => 4096,
            _ => 65_536,
        },
        max_name_len: match (knobs >> 4) & 3 {
            0 => 1,
            1 => 16,
            2 => 256,
            _ => 1024,
        },
    };

    let Ok(archive) = Archive::open(body, &limits) else {
        return;
    };

    let listed: Vec<(String, u64, Kind, Option<u32>)> = archive
        .entries()
        .iter()
        .map(|e| (e.name.clone(), e.size, e.kind, e.crc))
        .collect();

    assert!(
        listed.len() <= limits.max_entries,
        "the entry cap was exceeded rather than refused"
    );
    for (name, _, _, _) in &listed {
        assert!(
            name.len() <= limits.max_name_len,
            "a name past the cap was kept at full length"
        );
    }

    for index in (0..listed.len()).rev() {
        match archive.read(index) {
            Ok(bytes) => {
                assert_eq!(
                    bytes.len() as u64, listed[index].1,
                    "a successful read returned a length other than the one the \
                     entry declared"
                );
                assert_eq!(
                    listed[index].2,
                    Kind::File,
                    "an entry that is not a file was read rather than refused"
                );
                if let Some(want) = listed[index].3 {
                    assert_eq!(
                        crc32(&bytes),
                        want,
                        "a read returned bytes whose CRC-32 is not the one the \
                         archive recorded"
                    );
                }
                // Every entry this build reads is stored, so every successful
                // read is a range of the input. A copy would pass every other
                // assertion here.
                if !bytes.is_empty() {
                    let at = bytes.as_ptr() as usize;
                    let start = body.as_ptr() as usize;
                    assert!(
                        at >= start && at + bytes.len() <= start + body.len(),
                        "a read returned bytes that are not inside the archive"
                    );
                }
            }
            Err(EntryError::NoSuchEntry) => {
                panic!("an index taken from the entry list was not an entry")
            }
            Err(_) => {}
        }
    }

    let after: Vec<(String, u64, Kind, Option<u32>)> = archive
        .entries()
        .iter()
        .map(|e| (e.name.clone(), e.size, e.kind, e.crc))
        .collect();
    assert_eq!(listed, after, "the entry list changed under reading");

    if let Ok(shipped) = Archive::open(body, &Limits::DEFAULT) {
        for index in 0..shipped.entries().len().min(64) {
            let _ = shipped.read(index);
        }
    }
});
