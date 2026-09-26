//! Every bound the tar reader enforces, in one place, with the numbers that
//! set it.
//!
//! Written in `tinker-pdf-zip`'s `limits` shape, and it carries the same two
//! scars: a per-item cap is not a total once the item count is chosen by the
//! file (`5adf502`), and **a cap that cannot fire is not a cap** (gap 18a's
//! milestone 8). So every constant here carries three numbers — the most any
//! fixture in this repository spends, the most a plausible real archive
//! spends, and the constant — and each is proved to fire by its own refusal or
//! warning.
//!
//! # There are two of them, and the shortness is the interesting part
//!
//! `tinker-pdf-zip` needs four because a deflated entry expands: what comes
//! *out* of an archive is not bounded by what went in, so it needs a
//! per-entry cap, a total, and the two are famously not each other.
//!
//! **Nothing in a tar expands.** Every entry is a byte range of the input, so
//! the input's own length already bounds one entry and all of them together,
//! and a `MAX_TAR_ENTRY_BYTES` would be a constant no input could reach past.
//! Writing one anyway is exactly the failure `MAX_JPX_WORK` was: a number with
//! a `MAX_` prefix that reads as a defence and is not one. What is left is the
//! two places a tar *does* allocate on a file-derived number — the entry list,
//! and a GNU long name.

/// The most entries one archive may list.
///
/// Spent against headers actually walked, so an archive that declares nothing
/// and holds three opens with three — a tar has no count field to lie in.
/// What this bounds is the `Vec<Entry>`, which is the only allocation that
/// grows with the archive rather than with one entry.
///
/// | | Entries |
/// | --- | --- |
/// | The most any fixture here spends | 5 (`7z-tar.cbt`) |
/// | A 200-page comic (200 pages, a `ComicInfo.xml`, a directory entry) | 202 |
/// | **This cap** | **16 384** |
///
/// The same number as `tinker_pdf_zip::limits::MAX_ZIP_ENTRIES`, deliberately:
/// a comic archive is a comic archive whichever container it arrived in, and
/// two different answers to "how many pages is too many" would mean a `.cbt`
/// and a `.cbz` of the same pages disagreed about whether they open.
///
/// Reachable: a header is 512 bytes, so 16 385 of them is an 8.4 MB archive —
/// which `an_archive_with_more_entries_than_the_cap_is_refused_by_name`
/// builds.
pub const MAX_TAR_ENTRIES: usize = 16_384;

/// The most bytes of one stored path.
///
/// The 100-byte header field cannot reach this and is not what it is for: a
/// **GNU long name** is a whole pseudo-entry whose data *is* the name, and a
/// PAX `path` record is the same shape, so both are a length the file chooses
/// and both allocate a `String` of it.
///
/// | | Bytes |
/// | --- | --- |
/// | The most any fixture here spends | 100 (the header field, which is its own cap) |
/// | The longest path a real comic archive holds | ~42, measured off the ZIP corpus' own inventory |
/// | **This cap** | **1 024** |
///
/// The same number as `tinker_pdf_zip::limits::MAX_ZIP_NAME_LEN`, for
/// [`MAX_TAR_ENTRIES`]'s reason. A name past it is **truncated with a
/// warning** rather than refused, which is the opposite of that crate's
/// choice for a reason: a ZIP has a central directory, so a refused name
/// costs one entry, and a tar is a stream where the name and the file are in
/// different blocks — refusing a name would leave the walk with a file it
/// could not name and no way to skip only that.
///
/// Reachable: a long-name pseudo-entry's size field is eleven octal digits,
/// so a name may be declared up to 8 GiB long.
pub const MAX_TAR_NAME_LEN: usize = 1_024;
