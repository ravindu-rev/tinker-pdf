//! Every bound the RAR reader enforces, in one place, with the numbers that
//! set it.
//!
//! The `tar::limits` shape and the same two scars: a per-item cap is not a
//! total once the item count is chosen by the file (`5adf502`), and **a cap
//! that cannot fire is not a cap** (gap 18a's milestone 8).
//!
//! # There are three, and the middle one is the one tar did not need
//!
//! Like a tar and unlike a 7z, a **stored** RAR entry is a byte range of the
//! input, so the input's own length already bounds it. What RAR has that tar
//! does not is a **header of a size the file chooses**: `HeaderSize` is a
//! `vint`, the extra area inside it is a chain of records, and the name inside
//! that is as long as a field says. So the header needs a cap of its own even
//! though the data does not.

/// The most entries one archive may list.
///
/// | | Entries |
/// | --- | --- |
/// | The most any fixture here spends | 5 (`winrar-rar5.cbr`) |
/// | A 200-page comic (200 pages, a `ComicInfo.xml`, a directory entry) | 202 |
/// | **This cap** | **16 384** |
///
/// The same number as `tinker_pdf_zip::limits::MAX_ZIP_ENTRIES`,
/// [`crate::tar::limits::MAX_TAR_ENTRIES`] and
/// [`crate::sevenz::limits::MAX_7Z_ENTRIES`], deliberately: a comic archive is
/// a comic archive whichever container it arrived in, and four different
/// answers to "how many pages is too many" would mean the same pages opened in
/// one container and not in another.
///
/// Reachable: a file header with an empty name and no data is about 20 bytes,
/// so 16 385 of them is a 330 KB archive — which
/// `an_archive_with_more_entries_than_the_cap_is_refused_by_name` builds.
pub const MAX_RAR_ENTRIES: usize = 16_384;

/// The most bytes of one header, including its extra area.
///
/// Spent per header rather than per archive, and that is the weaker of the two
/// shapes — `5adf502`'s scar is exactly this — but here it is the honest one:
/// a header is a byte range of the input like everything else in a stored RAR,
/// so the file's own length is the total, and this cap exists to stop **one**
/// `HeaderSize` from being believed rather than to bound the sum.
///
/// | | Bytes |
/// | --- | --- |
/// | The most any fixture here spends | 38 (`winrar-rar5.cbr`, a file header with an `htime` record) |
/// | A header with a long path, a Unicode owner record and a hash record | ~1 200 |
/// | **This cap** | **65 536** |
///
/// Reachable: `HeaderSize` is a `vint`, so three bytes declare 2 MB —
/// which `a_header_larger_than_the_cap_ends_the_walk` builds, and which is
/// refused before the CRC is computed over it rather than after.
pub const MAX_RAR_HEADER_BYTES: usize = 65_536;

/// The most bytes of one stored path.
///
/// | | Bytes |
/// | --- | --- |
/// | The most any fixture here spends | 10 (`page10.png`) |
/// | The longest path a real comic archive holds | ~42, measured off the ZIP corpus' own inventory |
/// | **This cap** | **1 024** |
///
/// The same number as the three sibling readers', for [`MAX_RAR_ENTRIES`]'s
/// reason. A name past it is **truncated with a warning** rather than refused:
/// the name is inside a header whose length is already known, so a truncated
/// name costs nothing but the name, and refusing it would cost the page.
///
/// Reachable: `NameLength` is a `vint` bounded only by
/// [`MAX_RAR_HEADER_BYTES`], so a name may be 64 KB long.
pub const MAX_RAR_NAME_LEN: usize = 1_024;
