//! 7z archives: the header grammar, the folder graph, and the LZMA engine
//! behind them.
//!
//! The richest of this crate's three container targets, and the reason is the
//! shape of the format. A tar is one linear walk. A 7z header is a **grammar**
//! — nested property-id sections, counts that must agree across three of them,
//! and a `NUMBER` encoding whose first byte decides how many more bytes it
//! eats — and then the header is usually *compressed*, so a fuzzer that gets
//! past the signature is fuzzing an LZMA decoder with a `kCodersUnpackSize`
//! it chose itself.
//!
//! Three things in that are not reachable from a fixture an author wrote:
//!
//! - **A count that disagrees with another count.** `kNumUnpackStream` says
//!   how many substreams a folder has, `kSize` lists all but the last, and
//!   `kCRC` lists only the ones the folder's own CRC does not already cover.
//!   Getting two of the three to agree and the third not is the whole class.
//! - **A folder graph that is not a chain.** Bind pairs are indices into a
//!   stream list the same header defines, so a cycle, a self-reference and an
//!   index past the end are all one byte away from a valid archive.
//! - **A `kCodersUnpackSize` of 2^62 in a 40-byte file.** Every allocation
//!   here is downstream of that number.
//!
//! The control byte picks the bounds rather than the input, in the shape
//! `zip_archive.rs` established and for its stated reason: gap 18a milestone 8
//! found a work cap set above the most its own inputs could ask for, so a
//! target whose limits are all shipped defaults never explores a refusal. Each
//! knob takes a distinct pair of bits, so a corpus written for an earlier set
//! keeps its meaning.
//!
//! What is asserted beyond "it did not panic":
//!
//! - **A read that succeeds returned exactly the entry's declared size.** The
//!   comic path copies these bytes into a PDF image stream whose `/Length` was
//!   taken from the entry.
//! - **A read that succeeds has a CRC-32 matching the one the archive
//!   recorded**, when it recorded one. This is the format adjudicating the
//!   decompressor, asserted from outside so that a `read` which stopped
//!   checking would be caught here rather than only where it is tested.
//! - **Every listed entry answers, one way or the other.**
//! - **The entry list does not change under reading**, which a folder cache
//!   makes a real question rather than a formality.
//! - **Reading twice gives the same bytes.** Determinism (ruling 4), and the
//!   cache is exactly the thing that could break it.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_archive::sevenz::{Archive, EntryError, Kind, Limits};
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
        max_folders: match (knobs >> 2) & 3 {
            0 => 1,
            1 => 2,
            2 => 16,
            _ => 256,
        },
        max_coders: match (knobs >> 4) & 3 {
            0 => 1,
            1 => 2,
            2 => 4,
            _ => 32,
        },
        max_unpacked: match (knobs >> 6) & 3 {
            0 => 1 << 8,
            1 => 1 << 12,
            2 => 1 << 16,
            _ => 1 << 22,
        },
        max_name_len: 1024,
    };

    let Ok(mut archive) = Archive::open(body, &limits) else {
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

    // Reverse order on purpose: a caller reads pages in whatever order the
    // viewer scrolls, and the folder cache is what that order can break.
    let mut first_pass: Vec<Option<Vec<u8>>> = vec![None; listed.len()];
    for index in (0..listed.len()).rev() {
        match archive.read(index) {
            Ok(bytes) => {
                assert_eq!(
                    bytes.len() as u64, listed[index].1,
                    "a successful read returned a length other than the one the \
                     entry declared"
                );
                assert!(
                    matches!(listed[index].2, Kind::File | Kind::EmptyFile),
                    "an entry that is not a file was read rather than refused"
                );
                // **The format adjudicating the decompressor**, checked from
                // outside. A `read` that stopped verifying would pass every
                // other assertion here.
                if let Some(want) = listed[index].3 {
                    assert_eq!(
                        crc32(&bytes),
                        want,
                        "a read returned bytes whose CRC-32 is not the one the \
                         archive recorded"
                    );
                }
                assert!(
                    bytes.len() <= limits.max_unpacked,
                    "an entry past the unpacked cap was returned rather than refused"
                );
                first_pass[index] = Some(bytes);
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

    // Forwards this time. The folder cache holds one decompressed block, so
    // the order a caller reads in decides how often it is refilled — and a
    // cache that returned a stale block would show up here and nowhere else.
    for index in 0..listed.len() {
        let again = archive.read(index).ok();
        assert_eq!(
            again, first_pass[index],
            "reading an entry twice gave two different answers"
        );
    }

    // The same bytes under the shipped bounds, which is the configuration that
    // ships and the one no widened cap may turn into a panic.
    if let Ok(mut shipped) = Archive::open(body, &Limits::DEFAULT) {
        for index in 0..shipped.entries().len().min(64) {
            let _ = shipped.read(index);
        }
    }
});
