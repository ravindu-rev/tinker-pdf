//! Every bound the 7z reader enforces, in one place, with the numbers that set
//! it.
//!
//! The `tar::limits` shape, and the same two scars: a per-item cap is not a
//! total once the item count is chosen by the file (`5adf502`), and **a cap
//! that cannot fire is not a cap** (gap 18a's milestone 8). So every constant
//! carries three numbers — the most any fixture in this repository spends, the
//! most a plausible real archive spends, and the constant.
//!
//! # There are five, where tar has two, and the difference is compression
//!
//! Nothing in a tar expands, so the input's own length bounds every allocation
//! and two caps were honest. **Everything in a 7z expands**, and the ratio is
//! not bounded by anything in the format: 258 bytes of packed header
//! decompress to whatever the stream says, and a folder's declared
//! `kCodersUnpackSize` is a `NUMBER` that may be `2^63`. So the output needs a
//! real cap, and so do the three counts the header lets a file choose freely —
//! entries, folders and coders-per-folder.

/// The most entries one archive may list.
///
/// | | Entries |
/// | --- | --- |
/// | The most any fixture here spends | 5 (`7z-lzma2.cb7`) |
/// | A 200-page comic (200 pages, a `ComicInfo.xml`, a directory entry) | 202 |
/// | **This cap** | **16 384** |
///
/// The same number as `tinker_pdf_zip::limits::MAX_ZIP_ENTRIES` and
/// [`crate::tar::limits::MAX_TAR_ENTRIES`], deliberately: a comic archive is a
/// comic archive whichever container it arrived in, and two different answers
/// to "how many pages is too many" would mean a `.cb7` and a `.cbz` of the
/// same pages disagreed about whether they open.
///
/// Reachable: `kFilesInfo`'s count is a `NUMBER`, so an eight-byte field
/// declares 16 385 in nine bytes — which
/// `an_archive_declaring_more_entries_than_the_cap_is_refused_by_name` builds.
pub const MAX_7Z_ENTRIES: usize = 16_384;

/// The most folders — decompression units — one archive may hold.
///
/// A folder is a solid block. One is the normal case and is what `-m0=LZMA2`
/// writes; a writer using `-ms=off` writes one per file, which is where the
/// second column comes from.
///
/// | | Folders |
/// | --- | --- |
/// | The most any fixture here spends | 1 (`7z-lzma2.cb7`) |
/// | A 200-page comic written non-solid | 200 |
/// | **This cap** | **4 096** |
///
/// Lower than [`MAX_7Z_ENTRIES`] on purpose, and the gap is the point: an
/// entry costs a name, and a folder costs a `Folder` with three `Vec`s and a
/// coder list, so the two are not the same allocation and a single number for
/// both would be sized for the cheaper one.
///
/// Reachable: `kFolder`'s count is a `NUMBER` and the folders themselves are
/// two bytes each, so 4 097 of them is a 12 KB header.
pub const MAX_7Z_FOLDERS: usize = 4_096;

/// The most coders in one folder, **and** the most streams one coder may
/// declare on either side.
///
/// A chain is what this build decodes and the longest one a real writer emits
/// is three — a filter, a compressor and, historically, a second filter.
///
/// | | Coders |
/// | --- | --- |
/// | The most any fixture here spends | 1 (`7z-lzma2.cb7`) |
/// | `-mf=BCJ -m0=LZMA2` with a delta filter | 3 |
/// | **This cap** | **32** |
///
/// It bounds the stream counts too, because `numInStreams` is the number the
/// bind-pair loop below it runs on: a coder declaring `2^40` input streams is
/// a `Vec` of that many pairs before anything has been decompressed, and it is
/// four bytes to say so.
///
/// Reachable: `NumCoders` is a `NUMBER`, so 33 of them is one byte plus 33
/// two-byte coders.
pub const MAX_7Z_CODERS: usize = 32;

/// The most bytes one folder may decompress to, and the most a compressed
/// header may.
///
/// This is the cap that matters: it is the only one standing between a
/// `kCodersUnpackSize` of `2^62` and an allocation of it.
///
/// | | Bytes |
/// | --- | --- |
/// | The most any fixture here spends | 18 483 (`7z-lzma2.cb7`, all five pages in one solid block) |
/// | A 200-page scanned comic, solid | ~600 MB |
/// | **This cap** | **1 GiB** |
///
/// A folder is a *solid block*, so this is not a per-page number and must not
/// be set as though it were: every page of a solid comic is in one folder and
/// the whole folder decompresses to read any of them. That is the fact about
/// 7z that this constant exists to price, and it is why the number is larger
/// than any per-entry cap elsewhere in this workspace.
///
/// Reachable: `kCodersUnpackSize` is a `NUMBER`, so a nine-byte field declares
/// a terabyte — which
/// `a_folder_declaring_more_than_the_cap_is_refused_before_it_allocates`
/// builds, and which is refused *before* the `Vec` rather than after.
pub const MAX_7Z_UNPACKED: usize = 1 << 30;

/// The most bytes of one stored path.
///
/// | | Bytes |
/// | --- | --- |
/// | The most any fixture here spends | 10 (`page10.png`) |
/// | The longest path a real comic archive holds | ~42, measured off the ZIP corpus' own inventory |
/// | **This cap** | **1 024** |
///
/// The same number as `tinker_pdf_zip::limits::MAX_ZIP_NAME_LEN` and
/// [`crate::tar::limits::MAX_TAR_NAME_LEN`], for [`MAX_7Z_ENTRIES`]'s reason.
/// A name past it is **truncated with a warning** rather than refused: 7z
/// stores every name in one blob, so a refused name would cost the walk its
/// place in that blob and every name after it.
///
/// Reachable: the name blob's own size is a `NUMBER` and the names inside it
/// are only NUL-terminated, so one name may be as long as the header.
pub const MAX_7Z_NAME_LEN: usize = 1_024;

/// The most bytes one LZMA or LZMA2 stream may decode to.
///
/// Not a separate number: [`crate::lzma`]'s cap **is** [`MAX_7Z_UNPACKED`],
/// passed down by `sevenz::Limits::lzma`. Named here so that a reader looking
/// for the LZMA bound finds it and finds out that it is the folder bound,
/// rather than finding a second constant that could drift from it.
pub const MAX_LZMA_UNPACKED: usize = MAX_7Z_UNPACKED;
