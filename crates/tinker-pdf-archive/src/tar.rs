//! tar: POSIX 1003.1 `ustar`, GNU long names, and PAX extended headers.
//!
//! A `.cbt` is a tar of page images. tar is the simplest of the three formats
//! here by a wide margin and it is deliberately first: there is no compression
//! at all, so this module is a walk over 512-byte headers, and every byte an
//! entry holds is a range of the input.
//!
//! # What that buys, and why it is stated here rather than assumed
//!
//! [`Archive::read`] returns `&'a [u8]` — **not** a `Cow`, and never a copy.
//! `tinker-pdf-zip` returns a `Cow` because a deflated entry has to be
//! inflated somewhere and a stored one does not; a tar entry is always the
//! second case, so the borrow is unconditional and the signature says so. The
//! comic path places image bytes into a PDF stream verbatim, so a copy per
//! entry is a copy of the whole archive: the crate header's `Cow` argument is
//! this property, and this is the module that has the strongest form of it.
//!
//! # The format, and the four shapes of it that exist
//!
//! Every header is 512 bytes and every file's data is padded out to a
//! multiple of 512. The differences between the dialects are all about how a
//! name longer than the header's 100-byte field is carried, and there are
//! exactly three answers plus the original:
//!
//! - **v7**, the original: a 100-byte name, no magic field, and nothing else.
//!   Refused here — [`Error::NotATar`] — because the magic is the only thing
//!   that separates a tar from a file that happens to be a multiple of 512
//!   bytes, and a reader with no signature to check is a reader that accepts
//!   anything.
//! - **ustar** (POSIX 1003.1, magic `ustar\0` and version `00`): a 155-byte
//!   `prefix` field at offset 345, joined to the name with a `/`.
//! - **GNU** (magic `ustar  \0` — two spaces and a NUL, which is what 7-Zip's
//!   `-ttar` writes and therefore what the committed `.cbt` fixture is): no
//!   prefix, and a long name is carried in a whole *pseudo-entry* of type `L`
//!   whose data is the name of the entry that follows it.
//! - **PAX** (POSIX 1003.1-2001, type `x` and `g`): a pseudo-entry whose data
//!   is a sequence of `length keyword=value\n` records. `path` overrides the
//!   name and `size` overrides the size, which is how a file larger than 8 GiB
//!   is carried.
//!
//! All four are read except the first, and a header that is none of them ends
//! the walk rather than failing it (ruling 2): a tar is a stream format with
//! no directory, so *what has been read so far* is the recoverable answer and
//! there is no second route to try.
//!
//! # Refused by name
//!
//! **Sparse files** (GNU type `S`, and PAX's `GNU.sparse.*` records) and
//! **multi-volume continuations** (type `M`). Both are listed as entries, so
//! the count stays honest, and both refuse at [`Archive::read`] rather than
//! silently handing back the file with its holes closed up — which is what a
//! reader that ignored the flag would do, and it would be a picture with the
//! wrong bytes in it rather than a picture that failed to decode.

use crate::tar::limits::{MAX_TAR_ENTRIES, MAX_TAR_NAME_LEN};

pub mod limits;

/// One header, and one unit of padding. POSIX 1003.1: everything in a tar is
/// a multiple of this.
const BLOCK: usize = 512;

/// Resource ceilings. Mandatory rather than defaulted, for
/// `tinker_pdf_zip::Limits`' reason: a caller that can forget a budget is a
/// caller that will.
///
/// **There is no `max_entry_bytes` here and its absence is the point.** A tar
/// entry is a byte range of the input and nothing decompresses, so the input's
/// own length is already the bound on every entry and on all of them together.
/// A second cap over the same number would be a constant that no input can
/// reach past, which is decoration with a `MAX_` prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Entries listed. See [`limits::MAX_TAR_ENTRIES`].
    pub max_entries: usize,
    /// Bytes of one stored path, after a long-name or `path` record has
    /// replaced it. See [`limits::MAX_TAR_NAME_LEN`].
    pub max_name_len: usize,
}

impl Limits {
    /// The constants in [`limits`], which is what [`Default`] hands back.
    pub const DEFAULT: Self = Self {
        max_entries: MAX_TAR_ENTRIES,
        max_name_len: MAX_TAR_NAME_LEN,
    };
}

impl Default for Limits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Why an archive could not be opened at all.
///
/// Two variants, and the shortness is a fact about the format: a tar has no
/// directory to be damaged, no end record to be missing and no offsets to be
/// out of range. Everything that goes wrong *inside* one is either a warning
/// or the end of the walk.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Error {
    /// The first header carries no `ustar` magic, or the file is shorter than
    /// one header block.
    ///
    /// The magic is the only signature tar has, so a reader that did not
    /// require it would accept any file whose first 512 bytes happen to
    /// checksum — which for an all-zero block they do.
    NotATar,
    /// Past [`Limits::max_entries`].
    TooManyEntries,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Error::NotATar => "not a tar archive",
            Error::TooManyEntries => "more entries than this build reads",
        })
    }
}

impl std::error::Error for Error {}

/// Why one entry's bytes were not handed over.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EntryError {
    /// No entry with that index. A caller's bug rather than a file's.
    NoSuchEntry,
    /// A sparse file (GNU type `S`, or PAX `GNU.sparse.*`).
    ///
    /// Refused rather than returned with its holes closed up, which is what a
    /// reader that ignored the flag would hand back: bytes in the wrong places
    /// are a picture that decodes to the wrong thing, where a refusal is a
    /// placeholder page that says so.
    Sparse,
    /// A multi-volume continuation (GNU type `M`). The fragment that happens
    /// to be here is not the file.
    MultiVolume,
    /// An entry that holds no file data: a directory, a link, a device node,
    /// or a type flag this build does not know.
    NotAFile,
    /// The archive ends before the entry's declared size.
    Truncated,
}

impl core::fmt::Display for EntryError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            EntryError::NoSuchEntry => "no entry with that index",
            EntryError::Sparse => "a sparse file, which is not reassembled here",
            EntryError::MultiVolume => "a multi-volume continuation",
            EntryError::NotAFile => "an entry that holds no file data",
            EntryError::Truncated => "the archive ends before the entry does",
        })
    }
}

impl std::error::Error for EntryError {}

/// What reading an archive tolerated (ruling 10).
///
/// Every one of these carries the entry index it happened at, because a tar
/// with one bad header among two hundred is the case they exist for and "a
/// header checksum failed" with no index is a sentence a host cannot act on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Warning {
    /// A header whose stored checksum does not match its own bytes, under
    /// either the unsigned or the signed summation.
    ///
    /// The walk **stops** here rather than continuing: a tar has no directory,
    /// so a header that does not checksum is the point past which nothing can
    /// be located, and everything before it is still an answer.
    HeaderChecksumFailed { index: usize },
    /// A type flag outside the set POSIX 1003.1 and GNU define. The entry is
    /// listed and refuses at [`Archive::read`].
    UnknownTypeFlag { index: usize, flag: u8 },
    /// A name that is not UTF-8, decoded byte for byte as ISO-8859-1 instead.
    ///
    /// tar declares no encoding for the 100-byte name field — only PAX's
    /// `path` record is defined as UTF-8 — so a name that is not UTF-8 is
    /// ordinary rather than damaged. The fallback is total and deterministic,
    /// which is what ruling 4 needs from it; it is warned about because the
    /// name is what decides page order.
    NameNotUtf8 { index: usize },
    /// A name past [`Limits::max_name_len`], truncated to it.
    NameTruncated { index: usize },
    /// The archive ends without the two zero blocks POSIX 1003.1 requires.
    ///
    /// Tolerated because a truncated tar still holds the entries it has, and
    /// because a great many writers emit one zero block or none.
    NoEndOfArchive,
    /// A PAX record this build does not act on. Counted once per archive
    /// rather than per record: a header of forty `SCHILY.*` fields has one
    /// thing worth saying about it.
    PaxRecordIgnored,
    /// A PAX or GNU pseudo-entry that describes nothing, because the archive
    /// ends after it. Its name and size are dropped.
    DanglingExtendedHeader { index: usize },
}

/// What an entry is, by its type flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Kind {
    /// A regular file: type flag `0`, `\0` (the original spelling) or `7`.
    File,
    /// A directory: type flag `5`. Also any name ending in `/`, which is how
    /// the original format spelled one before there was a flag.
    Directory,
    /// A hard or symbolic link: type flags `1` and `2`.
    Link,
    /// A sparse file: type flag `S`, or a PAX header carrying `GNU.sparse.*`.
    Sparse,
    /// A multi-volume continuation: type flag `M`.
    MultiVolume,
    /// A type flag this build does not know, kept as itself.
    Other(u8),
}

/// One entry, as its header describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The stored path, with a `prefix`, a GNU long name or a PAX `path`
    /// record already applied.
    pub name: String,
    /// The declared size in bytes.
    pub size: u64,
    /// What the type flag says it is.
    pub kind: Kind,
    /// Where the entry's data begins in the archive's bytes.
    pub offset: usize,
    /// Position in the archive, counting only entries that are listed.
    pub index: usize,
}

impl Entry {
    /// Whether this entry holds no file data.
    #[must_use]
    pub fn is_directory(&self) -> bool {
        self.kind == Kind::Directory
    }
}

/// A tar archive, read.
///
/// Holds the input rather than copying out of it, so an [`Entry`] is a name
/// and a range and [`Archive::read`] is a bounds check.
///
/// `PartialEq` compares the entry list and the warnings, which is what a test
/// asserting `open(..) == Err(..)` needs and the only equality that means
/// anything here — two archives over the same bytes are the same archive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Archive<'a> {
    bytes: &'a [u8],
    entries: Vec<Entry>,
    warnings: Vec<Warning>,
}

impl<'a> Archive<'a> {
    /// Walks an archive's headers.
    ///
    /// # Errors
    /// [`Error::NotATar`] when the first header carries no `ustar` magic, and
    /// [`Error::TooManyEntries`] past [`Limits::max_entries`].
    pub fn open(bytes: &'a [u8], limits: &Limits) -> Result<Archive<'a>, Error> {
        // The signature, at the fixed position POSIX 1003.1 puts it and
        // nowhere else. Both spellings: `ustar\0` is POSIX's and `ustar  `
        // (two spaces, then a NUL in the version field) is GNU's, which is
        // what 7-Zip writes.
        if bytes.get(257..262) != Some(b"ustar".as_slice()) {
            return Err(Error::NotATar);
        }

        let mut entries: Vec<Entry> = Vec::new();
        let mut warnings: Vec<Warning> = Vec::new();
        let mut pax_ignored = false;
        // A GNU `L` or a PAX `x` describes the *next* entry, so what they say
        // is carried forward one header and then spent.
        let mut pending_name: Option<String> = None;
        let mut pending_size: Option<u64> = None;
        let mut pending_sparse = false;
        // A PAX `g` sets defaults for everything after it (POSIX 1003.1-2001
        // "global extended header"), which is a different lifetime from `x`
        // and is why the two are not one variable.
        let mut global_sparse = false;

        let mut at = 0usize;
        let mut ended = false;
        while at.saturating_add(BLOCK) <= bytes.len() {
            let Some(header) = bytes.get(at..at + BLOCK) else {
                break;
            };
            if header.iter().all(|&b| b == 0) {
                // POSIX 1003.1 ends an archive with two of these. One is
                // accepted as the end too: a great many writers emit one, and
                // a reader that insisted would refuse them.
                ended = true;
                break;
            }
            if !checksum_matches(header) {
                warnings.push(Warning::HeaderChecksumFailed {
                    index: entries.len(),
                });
                break;
            }

            let flag = header.get(156).copied().unwrap_or(0);
            let declared = numeric(header.get(124..136).unwrap_or_default());
            let data_at = at.saturating_add(BLOCK);
            let stored = declared.min(bytes.len().saturating_sub(data_at) as u64);
            let data = bytes.get(data_at..data_at.saturating_add(stored as usize));

            match flag {
                // GNU long name: this entry's *data* is the next entry's name.
                // `K` is the same device for a link target, which nothing here
                // reads, so its payload is skipped rather than kept.
                b'L' | b'K' => {
                    if flag == b'L' {
                        let raw = data.unwrap_or_default();
                        // NUL-terminated, and a writer that omitted the
                        // terminator has still said how long it is.
                        let raw = raw.split(|&b| b == 0).next().unwrap_or_default();
                        pending_name = Some(decode_name(raw, entries.len(), limits, &mut warnings));
                    }
                    at = advance(at, declared);
                    continue;
                }
                // PAX extended header, per-file (`x`) and global (`g`).
                b'x' | b'g' => {
                    let records = data.unwrap_or_default();
                    let (path, size, sparse, ignored) = pax(records);
                    if ignored {
                        pax_ignored = true;
                    }
                    if flag == b'g' {
                        global_sparse |= sparse;
                    } else {
                        if let Some(path) = path {
                            pending_name = Some(decode_name(
                                path.as_slice(),
                                entries.len(),
                                limits,
                                &mut warnings,
                            ));
                        }
                        pending_size = size;
                        pending_sparse = sparse;
                    }
                    at = advance(at, declared);
                    continue;
                }
                _ => {}
            }

            let index = entries.len();
            if index >= limits.max_entries {
                return Err(Error::TooManyEntries);
            }

            let name = match pending_name.take() {
                Some(name) => name,
                None => {
                    let short = header.get(0..100).unwrap_or_default();
                    let short = short.split(|&b| b == 0).next().unwrap_or_default();
                    // ustar's 155-byte prefix, joined with a `/`. GNU puts a
                    // sparse map there instead, which is why the join is
                    // conditioned on POSIX's magic rather than on the field
                    // being non-empty.
                    let posix = header.get(257..265) == Some(b"ustar\x0000".as_slice());
                    let prefix = header.get(345..500).unwrap_or_default();
                    let prefix = prefix.split(|&b| b == 0).next().unwrap_or_default();
                    if posix && !prefix.is_empty() {
                        let mut joined = prefix.to_vec();
                        joined.push(b'/');
                        joined.extend_from_slice(short);
                        decode_name(&joined, index, limits, &mut warnings)
                    } else {
                        decode_name(short, index, limits, &mut warnings)
                    }
                }
            };

            let size = pending_size.take().unwrap_or(declared);
            let sparse = pending_sparse || global_sparse;
            pending_sparse = false;
            let kind = match flag {
                _ if sparse => Kind::Sparse,
                b'0' | 0 | b'7' => {
                    // The original format had no directory flag and spelled
                    // one as a trailing slash, which writers still emit.
                    if name.ends_with('/') {
                        Kind::Directory
                    } else {
                        Kind::File
                    }
                }
                b'5' => Kind::Directory,
                b'1' | b'2' => Kind::Link,
                b'S' => Kind::Sparse,
                b'M' => Kind::MultiVolume,
                b'3' | b'4' | b'6' => Kind::Other(flag),
                other => {
                    warnings.push(Warning::UnknownTypeFlag { index, flag: other });
                    Kind::Other(other)
                }
            };

            entries.push(Entry {
                name,
                size,
                kind,
                offset: data_at,
                index,
            });
            at = advance(at, size.max(declared));
        }

        if pending_name.is_some() || pending_size.is_some() {
            warnings.push(Warning::DanglingExtendedHeader {
                index: entries.len(),
            });
        }
        if pax_ignored {
            warnings.push(Warning::PaxRecordIgnored);
        }
        if !ended {
            warnings.push(Warning::NoEndOfArchive);
        }
        Ok(Archive {
            bytes,
            entries,
            warnings,
        })
    }

    /// Every entry, in the order the archive holds them.
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// What was tolerated, in the order it happened (ruling 10).
    #[must_use]
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }

    /// One entry's bytes, **borrowed**.
    ///
    /// No `Cow` and no copy: see this module's header for why that signature
    /// is the point rather than an optimisation.
    ///
    /// # Errors
    /// [`EntryError`], one variant per reason there are no bytes to hand over.
    pub fn read(&self, index: usize) -> Result<&'a [u8], EntryError> {
        let entry = self.entries.get(index).ok_or(EntryError::NoSuchEntry)?;
        match entry.kind {
            Kind::File => {}
            Kind::Sparse => return Err(EntryError::Sparse),
            Kind::MultiVolume => return Err(EntryError::MultiVolume),
            Kind::Directory | Kind::Link | Kind::Other(_) => return Err(EntryError::NotAFile),
        }
        let size = usize::try_from(entry.size).map_err(|_| EntryError::Truncated)?;
        let end = entry
            .offset
            .checked_add(size)
            .ok_or(EntryError::Truncated)?;
        self.bytes
            .get(entry.offset..end)
            .ok_or(EntryError::Truncated)
    }
}

/// Where the next header begins: past this one and past `size` rounded up to a
/// whole number of blocks.
///
/// Saturating throughout, because `size` is a file-derived value and a
/// declared size of `u64::MAX` is a legal thing for a header to say.
fn advance(at: usize, size: u64) -> usize {
    let blocks = size
        .saturating_add(BLOCK as u64 - 1)
        .checked_div(BLOCK as u64)
        .unwrap_or(0);
    let payload = blocks
        .saturating_mul(BLOCK as u64)
        .try_into()
        .unwrap_or(usize::MAX);
    at.saturating_add(BLOCK).saturating_add(payload)
}

/// POSIX 1003.1: the unsigned sum of every header byte, with the checksum
/// field itself read as eight spaces.
///
/// Both summations are computed. The standard says unsigned, and a family of
/// historical writers on platforms with a signed `char` summed it the other
/// way — so a reader that checked only one refuses archives that are correct
/// under the other. Accepting either is the same leniency `tinker-pdf-zip`
/// applies to a local header, and it costs nothing: a block that matches
/// neither is still rejected.
fn checksum_matches(header: &[u8]) -> bool {
    let Some(field) = header.get(148..156) else {
        return false;
    };
    let Some(stored) = octal(field) else {
        return false;
    };
    let mut unsigned = 0u32;
    let mut signed = 0i32;
    for (at, &byte) in header.iter().enumerate() {
        let byte = if (148..156).contains(&at) { b' ' } else { byte };
        unsigned = unsigned.wrapping_add(u32::from(byte));
        signed = signed.wrapping_add(i32::from(byte as i8));
    }
    u64::from(unsigned) == stored || i64::from(signed) == stored as i64
}

/// A numeric header field: octal ASCII, or GNU's base-256 extension.
///
/// The base-256 form is signalled by the high bit of the first byte and exists
/// because an eleven-digit octal field tops out at 8 GiB. Only the
/// non-negative form is read — the negative one encodes a `uid`, never a size,
/// and this crate reads no `uid`.
fn numeric(field: &[u8]) -> u64 {
    match field.first() {
        Some(&first) if first & 0x80 != 0 => {
            let mut value = u64::from(first & 0x7F);
            for &byte in field.iter().skip(1) {
                value = value.saturating_mul(256).saturating_add(u64::from(byte));
            }
            value
        }
        _ => octal(field).unwrap_or(0),
    }
}

/// An octal ASCII field, NUL- and space-padded on either side.
///
/// `None` for a field holding a byte that is neither: a header whose size
/// field is not a number has not said how long its file is, and guessing zero
/// there would run the walk into the middle of the data.
fn octal(field: &[u8]) -> Option<u64> {
    let mut value: u64 = 0;
    let mut digits = 0usize;
    for &byte in field {
        match byte {
            b'0'..=b'7' => {
                value = value.checked_mul(8)?.checked_add(u64::from(byte - b'0'))?;
                digits += 1;
            }
            b' ' | 0 => {
                // Padding. A digit after padding is a malformed field, but it
                // is also what a writer that space-pads on the left produces,
                // so it is accepted.
            }
            _ => return None,
        }
    }
    (digits > 0).then_some(value)
}

/// A name, decoded and bounded.
///
/// UTF-8 where it is UTF-8, and ISO-8859-1 byte for byte where it is not: tar
/// declares no encoding for the name field at all, so a name that is not UTF-8
/// is ordinary rather than damaged, and the fallback has to be total and
/// deterministic (ruling 4) rather than lossy.
fn decode_name(raw: &[u8], index: usize, limits: &Limits, warnings: &mut Vec<Warning>) -> String {
    let raw = if raw.len() > limits.max_name_len {
        warnings.push(Warning::NameTruncated { index });
        raw.get(..limits.max_name_len).unwrap_or_default()
    } else {
        raw
    };
    match core::str::from_utf8(raw) {
        Ok(name) => name.to_owned(),
        Err(_) => {
            warnings.push(Warning::NameNotUtf8 { index });
            raw.iter().map(|&b| char::from(b)).collect()
        }
    }
}

/// PAX extended-header records: `length keyword=value\n`, repeated.
///
/// Returns the `path` and `size` overrides, whether anything said the entry is
/// sparse, and whether a record was seen and not acted on.
///
/// `length` counts **itself**, its space, the keyword, the `=`, the value and
/// the newline — POSIX 1003.1-2001 — which is what makes a value holding a
/// newline readable, and what makes a record with a wrong length unrecoverable
/// rather than merely wrong. A length that does not advance ends the parse.
fn pax(mut records: &[u8]) -> (Option<Vec<u8>>, Option<u64>, bool, bool) {
    let mut path = None;
    let mut size = None;
    let mut sparse = false;
    let mut ignored = false;

    while !records.is_empty() {
        let Some(space) = records.iter().position(|&b| b == b' ') else {
            break;
        };
        let Some(digits) = records.get(..space) else {
            break;
        };
        let mut length = 0usize;
        let mut any = false;
        for &byte in digits {
            let Some(digit) = (byte as char).to_digit(10) else {
                any = false;
                break;
            };
            let Some(next) = length
                .checked_mul(10)
                .and_then(|n| n.checked_add(digit as usize))
            else {
                any = false;
                break;
            };
            length = next;
            any = true;
        }
        // A record shorter than its own header cannot be one, and a record
        // that does not advance would loop forever.
        if !any || length <= space || length > records.len() {
            break;
        }
        let Some(record) = records.get(space + 1..length) else {
            break;
        };
        // The trailing newline the length counted.
        let record = record.strip_suffix(b"\n").unwrap_or(record);
        if let Some(eq) = record.iter().position(|&b| b == b'=') {
            let (keyword, value) = record.split_at(eq);
            let value = value.get(1..).unwrap_or_default();
            match keyword {
                b"path" => path = Some(value.to_vec()),
                b"size" => {
                    size = core::str::from_utf8(value)
                        .ok()
                        .and_then(|s| s.parse().ok());
                }
                // Any `GNU.sparse.*` record means the data is a sparse map
                // rather than the file, whichever of the format's three
                // revisions wrote it.
                _ if keyword.starts_with(b"GNU.sparse.") => sparse = true,
                _ => ignored = true,
            }
        } else {
            ignored = true;
        }
        records = records.get(length..).unwrap_or_default();
    }

    (path, size, sparse, ignored)
}

#[cfg(test)]
mod tests;
