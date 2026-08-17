//! ZIP archives (PKWARE APPNOTE): both routes, and the invariants a caller
//! trusts without re-checking.
//!
//! A ZIP is the one container in this tree with **two independent parsers over
//! the same bytes**, and a fuzzer is the right instrument for exactly that.
//! The central directory is authoritative and the local-header scan is the
//! recovery rung, and which of them runs is decided by the file — so an input
//! that damages an end-of-central-directory record does not fail, it switches
//! parsers. Very little of that seam is reachable from hand-built fixtures,
//! because a fixture author writes the archive they already had in mind.
//!
//! The control byte picks the bounds rather than the input, and it does so in
//! a way that keeps the caps *reachable*: gap 18 milestone 8 found a work cap
//! set above the most its own inputs could ask for, so a fuzz target whose
//! limits are all set to their shipped defaults is a target that never
//! explores a refusal. Every knob takes a distinct pair of bits and each new
//! one would take a higher pair, so a corpus written for an earlier set keeps
//! its meaning.
//!
//! What is asserted beyond "it did not panic":
//!
//! - **A read that succeeds produced exactly the declared length.** Gap 29
//!   copies these bytes into a PDF image stream whose `/Length` and
//!   `/Width`-`/Height` were taken from the entry, so a short buffer is read
//!   past by the rasteriser rather than noticed here.
//! - **A read that succeeds spent no more than the archive's total.** The
//!   total is the only bound between a 22-byte record and an unbounded
//!   allocation, and it is spent and never refunded.
//! - **Every entry is either checksummed or refused.** The recovery rung can
//!   list an entry whose extent it cannot establish; the contract is that such
//!   an entry carries no CRC and cannot be read, never that it is handed back
//!   at a guessed length.
//! - **The entry list does not change under reading.** A caller enumerates
//!   pages once and reads them in any order.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_zip::{Archive, EntryError, Limits, Route};

fuzz_target!(|data: &[u8]| {
    let (control, body) = data.split_at(data.len().min(1));
    let knobs = control.first().copied().unwrap_or(0);

    // Small enough that a bomb is refused in milliseconds rather than
    // gigabytes, and varied enough that both sides of each bound are reachable
    // from the same corpus. The shipped defaults are deliberately *not* here:
    // a 1 GiB total cannot fire inside a fuzz iteration, so a target that used
    // it would leave the crate's only work cap unexplored.
    let limits = Limits {
        max_entries: match knobs & 3 {
            0 => 1,
            1 => 4,
            2 => 64,
            _ => 4096,
        },
        max_entry_bytes: match (knobs >> 2) & 3 {
            0 => 16,
            1 => 1 << 10,
            2 => 1 << 16,
            _ => 1 << 20,
        },
        max_inflated_total: match (knobs >> 4) & 3 {
            0 => 0,
            1 => 1 << 10,
            2 => 1 << 16,
            _ => 1 << 20,
        },
        max_name_len: match (knobs >> 6) & 3 {
            0 => 1,
            1 => 16,
            2 => 256,
            _ => 1024,
        },
    };

    let Ok(mut archive) = Archive::open(body, &limits) else {
        return;
    };

    let route = archive.route();
    let listed: Vec<(String, Option<u32>, u64)> = archive
        .entries()
        .iter()
        .map(|e| (e.name.clone(), e.crc, e.uncompressed_size))
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
    // viewer scrolls, and any state the reader keeps between reads has to
    // survive that.
    for index in (0..listed.len()).rev() {
        match archive.read(index) {
            Ok(bytes) => {
                assert_eq!(
                    bytes.len() as u64,
                    listed[index].2,
                    "a successful read returned a length other than the one \
                     the entry declared"
                );
                assert!(
                    listed[index].1.is_some(),
                    "an entry with no checksum was read rather than refused"
                );
                assert!(
                    archive.inflated() <= limits.max_inflated_total,
                    "the archive total was exceeded rather than refused"
                );
            }
            Err(EntryError::NoSuchEntry) => {
                panic!("an index taken from the entry list was not an entry")
            }
            Err(_) => {}
        }
    }

    // Reading changes the warning list and the spent budget; it must not
    // change what the archive says it holds. A page count that moved because a
    // page was read is a page count no caller can render from.
    let after: Vec<(String, Option<u32>, u64)> = archive
        .entries()
        .iter()
        .map(|e| (e.name.clone(), e.crc, e.uncompressed_size))
        .collect();
    assert_eq!(listed, after, "the entry list changed under reading");

    // The route is decided at open and is what tells a caller whether it is
    // looking at an authoritative list or a recovered one.
    assert_eq!(route, archive.route(), "the route changed under reading");
    if route == Route::LocalHeaderScan {
        assert!(
            !archive.warnings().is_empty(),
            "recovery happened silently"
        );
    }

    // The same bytes under the shipped bounds. Nothing about a wider cap may
    // turn a refusal into a panic, and this is the configuration that actually
    // ships.
    if let Ok(mut shipped) = Archive::open(body, &Limits::DEFAULT) {
        for index in 0..shipped.entries().len().min(64) {
            let _ = shipped.read(index);
        }
    }
});
