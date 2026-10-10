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
/// | The most any fixture here spends | 5 (each of the three `.cb7`s) |
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
/// writes; a writer using `-ms=off` writes one per file, and the first column
/// stopped being 1 when `7z-nonsolid.cb7` was added for exactly that reason —
/// with one folder in every fixture, the walk in `decode_folder` was entered
/// once and never continued.
///
/// | | Folders |
/// | --- | --- |
/// | The most any fixture here spends | 5 (`7z-nonsolid.cb7`, one per page) |
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
/// The largest folder a real writer emits is BCJ2's: four coders, BCJ2 and
/// an LZMA for each of its three compressible streams.
///
/// | | Coders |
/// | --- | --- |
/// | The most any fixture here spends | 4 (`7zz-bcj2.cb7` and `tests/coders/7zz-bcj2.7z`: BCJ2 fed by three LZMA coders), and 4 streams into one coder |
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
/// | The most any fixture here spends | 18 483 (`7z-lzma2.cb7` and `7z-dictreset.cb7`, all five pages in one solid block; `7z-nonsolid.cb7`'s largest folder is 5 184, because splitting the block is what lowers this number) |
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
///
/// **It is checked on every coder's output, not only the folder's**, and
/// before that coder's inputs are decoded — and that alone is not a folder
/// bound, because a folder of several coders decodes each of them. A coder's
/// inputs that other coders decode are therefore held to what it can read for
/// its own declared output, wherever there is such a number: a Copy's or a
/// BCJ's to exactly that output, and BCJ2's main, call and jump streams to
/// that output and three bytes between them, its decisions to that output and
/// five. Before that rule a BCJ2 folder declaring a one-byte file decoded
/// whatever its three feeders declared, up to this cap apiece: a 9 676-byte
/// archive decoded 64 MiB of call targets nothing read, in 1.2 s, and three
/// caps was the ceiling. `bcj2_s_feeders_are_held_to_what_it_can_read_before_they_are_decoded`
/// holds both bounds at their edges. A compressor's input has no such number,
/// so a coder feeding one is held to this cap alone; no writer emits that
/// shape.
///
/// **It bounds a PPMd folder's model arena too**, and that is one cap on two
/// allocations by design rather than a second constant: the arena is the only
/// other allocation a folder's header sizes (four property bytes, up to
/// `2^32 - 37`), it is a cost of decoding the folder exactly as the output is,
/// and a separate number would have to be argued against this one anyway. The
/// most a fixture here asks for is 16 MiB (`py7zr-ppmd.7z`); the most 7-Zip's
/// own presets ask for is 256 MiB, `1 << (level + 19)` at level 9, and less
/// for a file small enough that 7-Zip shrinks it to fit. `crate::ppmd`'s
/// `the_arena_and_the_output_are_bounded_before_anything_is_allocated` is
/// where the refusal fires, before the arena exists.
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
