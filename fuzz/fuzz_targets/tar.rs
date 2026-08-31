//! tar archives (POSIX 1003.1, GNU and PAX): the header walk, and the
//! invariants a caller trusts without re-checking.
//!
//! A tar has **no directory and no second route** — it is one linear walk in
//! which every header says where the next one is — so the thing a fuzzer is
//! the right instrument for here is not a choice between parsers, it is the
//! walk's own arithmetic. A size field is eleven octal digits or GNU's
//! base-256 escape, either of which can declare 8 GiB inside a 512-byte file;
//! a GNU long-name pseudo-entry's data is the *next* entry's name and is as
//! long as it says; a PAX record's length counts itself, so a length that does
//! not advance is a loop. None of those three is reachable from a fixture an
//! author wrote on purpose.
//!
//! The control byte picks the bounds rather than the input, in the shape
//! `zip_archive.rs` established and for its stated reason: gap 18a milestone 8
//! found a work cap set above the most its own inputs could ask for, so a
//! target whose limits are all shipped defaults never explores a refusal.
//! Each knob takes a distinct pair of bits, so a corpus written for an earlier
//! set keeps its meaning.
//!
//! What is asserted beyond "it did not panic":
//!
//! - **A read that succeeds returned exactly the entry's declared size.** The
//!   comic path copies these bytes into a PDF image stream whose `/Length` was
//!   taken from the entry, so a short buffer is read past by the rasteriser
//!   rather than noticed here.
//! - **A read that succeeds returned a range of the input.** This is the
//!   property the whole crate is organised around — `tar::Archive::read`
//!   borrows and never copies — and it is checkable from outside by address.
//! - **Every listed entry answers, one way or the other.** An index taken from
//!   the entry list is never `NoSuchEntry`.
//! - **The entry list does not change under reading.** A caller enumerates
//!   pages once and reads them in any order.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_archive::tar::{Archive, EntryError, Kind, Limits};

fuzz_target!(|data: &[u8]| {
    let (control, body) = data.split_at(data.len().min(1));
    let knobs = control.first().copied().unwrap_or(0);

    // Small enough that a refusal is reachable inside one iteration, and
    // varied enough that both sides of each bound are reachable from the same
    // corpus.
    let limits = Limits {
        max_entries: match knobs & 3 {
            0 => 1,
            1 => 4,
            2 => 64,
            _ => 4096,
        },
        max_name_len: match (knobs >> 2) & 3 {
            0 => 1,
            1 => 16,
            2 => 256,
            _ => 1024,
        },
    };

    let Ok(archive) = Archive::open(body, &limits) else {
        return;
    };

    let listed: Vec<(String, u64, Kind)> = archive
        .entries()
        .iter()
        .map(|e| (e.name.clone(), e.size, e.kind))
        .collect();

    assert!(
        listed.len() <= limits.max_entries,
        "the entry cap was exceeded rather than refused"
    );
    for (name, _, _) in &listed {
        assert!(
            name.len() <= limits.max_name_len,
            "a name past the cap was kept at full length"
        );
    }

    // Reverse order on purpose: a caller reads pages in whatever order the
    // viewer scrolls.
    for index in (0..listed.len()).rev() {
        match archive.read(index) {
            Ok(bytes) => {
                assert_eq!(
                    bytes.len() as u64,
                    listed[index].1,
                    "a successful read returned a length other than the one \
                     the entry declared"
                );
                assert_eq!(
                    listed[index].2,
                    Kind::File,
                    "an entry that is not a file was read rather than refused"
                );
                // **The borrow, checked from outside.** A `read` that returned
                // an allocation rather than a range of the input would pass
                // every other assertion here and would be the one regression
                // this crate exists to prevent.
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

    let after: Vec<(String, u64, Kind)> = archive
        .entries()
        .iter()
        .map(|e| (e.name.clone(), e.size, e.kind))
        .collect();
    assert_eq!(listed, after, "the entry list changed under reading");

    // The same bytes under the shipped bounds. Nothing about a wider cap may
    // turn a refusal into a panic, and this is the configuration that ships.
    if let Ok(shipped) = Archive::open(body, &Limits::DEFAULT) {
        for index in 0..shipped.entries().len().min(64) {
            let _ = shipped.read(index);
        }
    }
});
