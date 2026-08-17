//! Every bound this crate enforces, in one place, with the numbers that set
//! it.
//!
//! Two scars in this repository decide how these are written down, and both
//! are cited in gap 29's bounds table.
//!
//! `5adf502` found an 1 851-byte page that took 19.3 seconds to render, with
//! `MAX_GROUP_DEPTH` in place the entire time: *depth is not work once the
//! recursion branches*. The ZIP form of that sentence is that **a per-entry
//! output cap is not a total once the entry count is chosen by the file** —
//! which is what `42.zip` is, and it needs no nesting at all. So
//! [`MAX_ZIP_INFLATED`] is a total, spent across every entry of one archive
//! and never refunded, and [`MAX_ZIP_ENTRY_BYTES`] sits beside it saying in as
//! many words that it is not that total.
//!
//! Gap 18a's milestone 8 found the opposite failure in a constant written
//! specifically to avoid the first: `MAX_JPX_WORK` was set *above* the most
//! its own inputs could ask for, so it could never fire. **A cap that cannot
//! fire is not a cap.** Every constant here therefore carries three numbers —
//! the most any fixture in this repository spends, the most a plausible real
//! archive spends (gap 29's yardstick: a 200-page comic at 2000 x 3000), and
//! the constant — and each is proved to fire in a test **by its own refusal or
//! warning, never by a clock**.
//!
//! The first of the three numbers is measured rather than asserted:
//! [`crate::Archive::inflated`] reports what an archive actually spent, and
//! `the_fixtures_in_this_crate_spend_what_the_ledger_says` reads it back.
//!
//! # There is deliberately no cap on path depth
//!
//! Gap 29's design lists "the deepest stored path" among the per-item caps a
//! ZIP reader might carry, and this one does not have it. Nothing here ever
//! touches a filesystem: a name is decoded, held as a `String`, and handed
//! back. Depth costs one byte per separator and bounds no allocation, no
//! recursion and no work, so a constant for it would be a number that can
//! never fire — the failure directly above, arrived at from the other
//! direction. What a name *does* cost is its length, and [`MAX_ZIP_NAME_LEN`]
//! bounds that.

/// The most entries one archive may hold, from either route.
///
/// The end-of-central-directory record's count is a **claim**: a 22-byte
/// record can declare four billion entries, and nothing in this crate ever
/// allocates on it. The cap is spent against records actually walked, so an
/// archive that declares four billion and holds three opens with three.
///
/// | | Entries |
/// | --- | --- |
/// | The most any fixture here spends | 6 |
/// | A 200-page comic (200 pages, a `ComicInfo.xml`, a directory entry) | 202 |
/// | **This cap** | **16 384** |
///
/// Reachable: a central directory record is 46 bytes plus a name, so 16 385
/// of them is a 754 KB archive — which
/// `an_archive_with_more_entries_than_the_cap_is_refused_by_name` builds.
pub const MAX_ZIP_ENTRIES: usize = 16_384;

/// The most bytes one entry may inflate to.
///
/// **This is not the work cap.** It bounds one entry; the archive chooses how
/// many entries there are, so the only thing that bounds an archive is
/// [`MAX_ZIP_INFLATED`], and reading this as the total is the mistake
/// `MAX_SCRIPT_STEPS` and `MAX_TILE_WORK` each carry the same warning about.
///
/// | | Bytes |
/// | --- | --- |
/// | The most any fixture here spends | 1 024 |
/// | The largest plausible page: a 2000 x 3000 16-bit RGBA PNG that barely compresses | ~48 MB |
/// | **This cap** | **128 MiB** |
///
/// The same number as `tinker_pdf_cos::limits::MAX_DECODED_STREAM`, and for
/// the same reason: it is the point past which a single decoded object is not
/// a document's content but somebody's idea of a joke.
pub const MAX_ZIP_ENTRY_BYTES: usize = 128 << 20;

/// **The work cap.** Bytes one archive may inflate, across every entry, spent
/// and never refunded.
///
/// A stored entry does not spend it: stored data is a subslice of the input,
/// it allocates nothing, it cannot expand, and it is already bounded by the
/// archive's own length. Inflation is the only place bounded input becomes
/// unbounded output, so inflation is what the total counts.
///
/// | | Bytes |
/// | --- | --- |
/// | The most any fixture here spends | 1 024 |
/// | A 200-page comic, stored — which gap 29 says is the common case, because JPEG and PNG do not compress again | **0** |
/// | The same comic with every page deflated anyway, at 1.5 MB a page | ~300 MB |
/// | **This cap** | **1 GiB** |
///
/// A GiB rather than more, on two counts. `usize` is 32 bits on
/// `wasm32-unknown-unknown` and `wasm32-wasip1`, both of which this engine
/// builds for, so a 2 GiB total would be half the entire address space and
/// could never fire there before the allocator did. And the archive past which
/// this refuses — 200 pages of deflated 5 MB PNG — is one that contradicts its
/// own format's premise, so refusing it *by name* is the honest answer rather
/// than a lost capability.
pub const MAX_ZIP_INFLATED: usize = 1 << 30;

/// The longest entry name kept, in decoded bytes.
///
/// A longer name is **truncated and warned about, not dropped**, because
/// dropping the entry would drop a page and renumber every page after it —
/// gap 29's first "where a half-implementation is worse than none". This is
/// `tinker_pdf_cos::limits::MAX_NAME_LEN`'s rule, for the same reason.
///
/// | | Bytes |
/// | --- | --- |
/// | The most any fixture here spends | 24 |
/// | `Some Comic (2019)/Chapter 03/page 001.jpg` | ~42 |
/// | **This cap** | **1 024** |
///
/// Reachable: the name-length field is a `u16`, so an entry may legally carry
/// 65 535 bytes of name, and
/// `a_name_past_the_cap_is_truncated_and_says_so` builds one.
pub const MAX_ZIP_NAME_LEN: usize = 1024;
