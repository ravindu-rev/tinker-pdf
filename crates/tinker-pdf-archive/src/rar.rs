//! RAR 5.0: the signature, the variable-length integer, the header chain and
//! the file records inside it.
//!
//! A `.cbr` is a RAR of page images. RAR 5.0 is the format WinRAR has written
//! since 2013 and is the only one this build reads — [`Error::Rar4`] names the
//! older one, and `docs/design/comic-archives.md` argues that refusal rather
//! than leaving it to be discovered.
//!
//! # What is read, and what is refused, and why the line is where it is
//!
//! RAR 5 stores each file's *compression method* in a six-field packed integer:
//! `0` is **store** and `1` through `5` are the same LZSS-plus-Huffman
//! algorithm at five encoder efforts. This module reads the container in full
//! and **method 0 in full**; methods 1 to 5 are a page-level refusal
//! ([`EntryError::Compressed`]), so an archive that mixes them pages its stored
//! entries and puts a placeholder where the others are (ruling 2).
//!
//! That is not where the line would be if it were free to put anywhere, and
//! the reason it is here is worth stating plainly rather than leaving as an
//! omission. **The committed `winrar-rar5.cbr` stores every one of its five
//! entries.** WinRAR compresses a file only when compressing makes it smaller,
//! and a PNG or a JPEG never is — so the fixture this repository can produce
//! from its own corpus exercises the container and *nothing* of the algorithm.
//! Under ruling 13 a decoder with no first-party fixture cannot be called
//! green, so the algorithm is not claimed. What the fixture does adjudicate,
//! it adjudicates completely: five files, five recorded CRC-32s, and the same
//! five pictures a `.cbz` of the same pages produces.
//!
//! # What adjudicates this reader
//!
//! **Two CRC-32s, both the format's own.** Every header carries one over
//! itself, checked before any field in it is believed; every file record
//! carries one over its unpacked data, checked in [`Archive::read`] before the
//! bytes are handed over. The first is what makes a `vint` chain safe to walk
//! at all — every offset here comes from a field a previous header declared —
//! and the second is what makes the extraction adjudicable without an oracle.
//!
//! # Refused by name
//!
//! **RAR 4** ([`Error::Rar4`]), by its own distinct seven-byte signature.
//! **Multi-volume archives** ([`Error::MultiVolume`]) — the fragment that
//! happens to be here is not the archive. **Encrypted archives**
//! ([`Error::Encrypted`]), a named non-goal shared with `tinker-pdf-zip` and
//! `sevenz`. **Solid entries** ([`EntryError::Solid`]), because a solid RAR
//! entry's dictionary is the entry before it and this build decompresses
//! neither.

use std::borrow::Cow;

use tinker_pdf_filters::crc32;

pub mod limits;

use limits::{MAX_RAR_ENTRIES, MAX_RAR_HEADER_BYTES, MAX_RAR_NAME_LEN};

/// `Rar!\x1A\x07\x01\x00` — RAR 5.0.
const SIGNATURE_5: &[u8] = b"Rar!\x1a\x07\x01\x00";
/// `Rar!\x1A\x07\x00` — RAR 4 and earlier. Recognised only to refuse it by
/// name.
const SIGNATURE_4: &[u8] = b"Rar!\x1a\x07\x00";

// Header types.
const TYPE_MAIN: u64 = 1;
const TYPE_FILE: u64 = 2;
const TYPE_SERVICE: u64 = 3;
const TYPE_ARCHIVE_ENCRYPTION: u64 = 4;
const TYPE_END: u64 = 5;

// Header flags.
const HAS_EXTRA: u64 = 0x0001;
const HAS_DATA: u64 = 0x0002;

// File flags.
const FILE_DIRECTORY: u64 = 0x0001;
const FILE_HAS_MTIME: u64 = 0x0002;
const FILE_HAS_CRC: u64 = 0x0004;
const FILE_UNKNOWN_SIZE: u64 = 0x0008;

// Archive flags.
const ARCHIVE_VOLUME: u64 = 0x0001;
const ARCHIVE_HAS_VOLUME_NUMBER: u64 = 0x0002;

// Extra-area record types, in a file header.
const EXTRA_ENCRYPTION: u64 = 0x01;

/// Why an archive could not be opened at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Error {
    /// No RAR signature at offset zero.
    NotARar,
    /// The RAR 4 signature, which this build recognises and does not read.
    ///
    /// A separate variant rather than [`Error::NotARar`] because the two are
    /// different sentences to a host: "this is not a RAR" and "this is a RAR
    /// of a version I do not read" lead a user to different actions, and the
    /// second one is true. See this module's header, and
    /// `docs/design/comic-archives.md` for why there is no RAR 4 decoder.
    Rar4,
    /// One volume of a multi-volume set.
    MultiVolume,
    /// An encrypted archive: the headers themselves are enciphered, so there
    /// is nothing to walk.
    Encrypted,
    /// The first header does not checksum, so there is no chain to follow.
    ///
    /// Every offset in a RAR is declared by the header before it, so a header
    /// that does not check is the point past which nothing can be located —
    /// which is why this is an [`Error`] for the *first* header and a
    /// [`Warning`] that ends the walk for any later one.
    FirstHeaderCorrupt,
    /// Past [`Limits::max_entries`].
    TooManyEntries,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Error::NotARar => "not a RAR archive",
            Error::Rar4 => "a RAR 4 archive, which this build recognises and does not read",
            Error::MultiVolume => "one volume of a multi-volume RAR",
            Error::Encrypted => "an encrypted RAR, which is a named non-goal",
            Error::FirstHeaderCorrupt => "a first header that does not checksum",
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
    /// A directory, or a service record. Neither holds file data.
    NotAFile,
    /// An entry compressed with RAR 5 methods 1 to 5.
    ///
    /// **Named rather than silently skipped**, and it is a page-level refusal
    /// rather than an archive-level one: an archive that mixes stored and
    /// compressed entries pages the stored ones and puts a placeholder where
    /// the others are. This module's header says why the algorithm is not
    /// implemented and what would have to exist first.
    Compressed { method: u8 },
    /// An entry whose dictionary is the entry before it.
    Solid,
    /// An entry whose data is enciphered.
    Encrypted,
    /// The archive ends before the entry's declared size.
    Truncated,
    /// The recorded CRC-32 does not match the bytes.
    ///
    /// **This is the check that adjudicates the extraction.** Not tolerated:
    /// bytes that fail it are a picture with the wrong pixels in it.
    CrcMismatch,
}

impl core::fmt::Display for EntryError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            EntryError::NoSuchEntry => f.write_str("no entry with that index"),
            EntryError::NotAFile => f.write_str("an entry that holds no file data"),
            EntryError::Compressed { method } => write!(
                f,
                "a RAR entry compressed with method {method}, which this build does not decompress"
            ),
            EntryError::Solid => f.write_str("a solid entry, whose dictionary is the entry before"),
            EntryError::Encrypted => f.write_str("an encrypted entry"),
            EntryError::Truncated => f.write_str("the archive ends before the entry does"),
            EntryError::CrcMismatch => f.write_str("an entry whose recorded CRC-32 does not match"),
        }
    }
}

impl std::error::Error for EntryError {}

/// What reading an archive tolerated (ruling 10).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Warning {
    /// A header that does not checksum. The walk **stops** here and keeps what
    /// came before: every offset in a RAR is declared by the header before it.
    HeaderChecksumFailed { index: usize },
    /// A name that is not UTF-8, decoded with replacement characters.
    ///
    /// RAR 5 declares names to be UTF-8, so this is damage rather than an
    /// encoding this reader does not know — but it is one name and not the
    /// archive.
    NameNotUtf8 { index: usize },
    /// A name past [`Limits::max_name_len`], truncated to it.
    NameTruncated { index: usize },
    /// A header type this build does not act on. Counted once per archive.
    HeaderTypeIgnored,
    /// An entry with no recorded CRC-32, handed over unadjudicated.
    NoCrcRecorded { index: usize },
    /// The archive has no end-of-archive record.
    NoEndOfArchive,
}

/// What an entry is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Kind {
    /// A regular file.
    File,
    /// A directory record.
    Directory,
    /// A service record: a recovery record, a comment, a quick-open index.
    /// Listed so the count stays honest, never read as a page.
    Service,
}

/// One entry, as its header describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The stored path, `\` already turned into `/`.
    pub name: String,
    /// The unpacked size in bytes.
    pub size: u64,
    /// What kind of record it is.
    pub kind: Kind,
    /// The CRC-32 the archive recorded, when it recorded one.
    pub crc: Option<u32>,
    /// The compression method: `0` is store, `1` to `5` are the algorithm.
    pub method: u8,
    /// Whether this entry's dictionary is the entry before it.
    pub solid: bool,
    /// Whether this entry's data is enciphered.
    pub encrypted: bool,
    /// Position in the archive, counting only entries that are listed.
    pub index: usize,
    /// Where the entry's data begins, and how many packed bytes it is.
    at: usize,
    packed: usize,
}

impl Entry {
    /// Whether this entry holds no file data.
    #[must_use]
    pub fn is_directory(&self) -> bool {
        matches!(self.kind, Kind::Directory | Kind::Service)
    }
}

/// Resource ceilings. Mandatory rather than defaulted, for
/// `tinker_pdf_zip::Limits`' reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Entries listed. See [`limits::MAX_RAR_ENTRIES`].
    pub max_entries: usize,
    /// Bytes of one header, including its extra area. See
    /// [`limits::MAX_RAR_HEADER_BYTES`].
    pub max_header_bytes: usize,
    /// Bytes of one stored path. See [`limits::MAX_RAR_NAME_LEN`].
    pub max_name_len: usize,
}

impl Limits {
    /// The constants in [`limits`], which is what [`Default`] hands back.
    pub const DEFAULT: Self = Self {
        max_entries: MAX_RAR_ENTRIES,
        max_header_bytes: MAX_RAR_HEADER_BYTES,
        max_name_len: MAX_RAR_NAME_LEN,
    };
}

impl Default for Limits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// A RAR 5 archive, walked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Archive<'a> {
    bytes: &'a [u8],
    entries: Vec<Entry>,
    warnings: Vec<Warning>,
}

impl<'a> Archive<'a> {
    /// Walks the header chain.
    ///
    /// # Errors
    /// [`Error`], one variant per way a file is not a readable RAR 5.
    pub fn open(bytes: &'a [u8], limits: &Limits) -> Result<Archive<'a>, Error> {
        if bytes.starts_with(SIGNATURE_4) {
            return Err(Error::Rar4);
        }
        if !bytes.starts_with(SIGNATURE_5) {
            return Err(Error::NotARar);
        }

        let mut entries: Vec<Entry> = Vec::new();
        let mut warnings: Vec<Warning> = Vec::new();
        let mut ignored = false;
        let mut ended = false;
        let mut at = SIGNATURE_5.len();
        let mut first = true;

        while at < bytes.len() {
            let Some(header) = Header::read(bytes, at, limits) else {
                if first {
                    return Err(Error::FirstHeaderCorrupt);
                }
                warnings.push(Warning::HeaderChecksumFailed {
                    index: entries.len(),
                });
                break;
            };
            first = false;

            match header.kind {
                TYPE_MAIN => {
                    let mut p = header.body;
                    let flags = vint(bytes, &mut p).unwrap_or(0);
                    if flags & ARCHIVE_VOLUME != 0 || flags & ARCHIVE_HAS_VOLUME_NUMBER != 0 {
                        return Err(Error::MultiVolume);
                    }
                }
                TYPE_ARCHIVE_ENCRYPTION => return Err(Error::Encrypted),
                TYPE_END => {
                    ended = true;
                    break;
                }
                TYPE_FILE | TYPE_SERVICE => {
                    let index = entries.len();
                    if index >= limits.max_entries {
                        return Err(Error::TooManyEntries);
                    }
                    if let Some(entry) = file_entry(bytes, &header, index, limits, &mut warnings) {
                        entries.push(entry);
                    }
                }
                _ => ignored = true,
            }

            let Some(next) = header
                .end
                .checked_add(header.data)
                .filter(|next| *next > at)
            else {
                break;
            };
            at = next;
        }

        if ignored {
            warnings.push(Warning::HeaderTypeIgnored);
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

    /// One entry's bytes, with its recorded CRC-32 checked.
    ///
    /// A `Cow` and not a plain borrow, which is `tinker_pdf_zip::Archive`'s
    /// signature rather than [`crate::tar`]'s: a **stored** RAR entry is a
    /// contiguous range of the input and is handed back borrowed, copied
    /// nowhere, and a compressed one would not be. Today every compressed
    /// entry is refused instead, so the `Owned` arm is unreachable — the
    /// signature is what the format allows rather than what this build does,
    /// and changing it later would be a breaking change for no gain.
    ///
    /// # Errors
    /// [`EntryError`], one variant per reason there are no bytes to hand over.
    pub fn read(&self, index: usize) -> Result<Cow<'a, [u8]>, EntryError> {
        let entry = self.entries.get(index).ok_or(EntryError::NoSuchEntry)?;
        match entry.kind {
            Kind::File => {}
            Kind::Directory | Kind::Service => return Err(EntryError::NotAFile),
        }
        if entry.encrypted {
            return Err(EntryError::Encrypted);
        }
        if entry.solid {
            return Err(EntryError::Solid);
        }
        if entry.method != 0 {
            return Err(EntryError::Compressed {
                method: entry.method,
            });
        }
        let end = entry
            .at
            .checked_add(entry.packed)
            .ok_or(EntryError::Truncated)?;
        let data = self.bytes.get(entry.at..end).ok_or(EntryError::Truncated)?;
        // A stored entry's packed size and unpacked size are the same number,
        // and a header that says otherwise is describing something this build
        // is about to hand back wrongly.
        if entry.size != data.len() as u64 {
            return Err(EntryError::Truncated);
        }
        if let Some(want) = entry.crc {
            if crc32(data) != want {
                return Err(EntryError::CrcMismatch);
            }
        }
        Ok(Cow::Borrowed(data))
    }
}

/// One header's geometry, after its own CRC-32 has been checked.
struct Header {
    kind: u64,
    flags: u64,
    /// Where the type-specific fields begin.
    body: usize,
    /// Where the extra area begins, and how long it is.
    extra: Option<(usize, usize)>,
    /// One past the last byte of the header.
    end: usize,
    /// How many bytes of data area follow it.
    data: usize,
}

impl Header {
    /// Reads a header and **checks its CRC-32 before returning it**.
    ///
    /// `None` for a header that does not check, that runs off the end, or that
    /// is past [`Limits::max_header_bytes`] — three different ways of saying
    /// the same thing to a caller that can only stop.
    fn read(bytes: &[u8], at: usize, limits: &Limits) -> Option<Header> {
        let stored = u32::from_le_bytes(bytes.get(at..at.checked_add(4)?)?.try_into().ok()?);
        let mut p = at.checked_add(4)?;
        let covered = p;
        let size = usize::try_from(vint(bytes, &mut p)?).ok()?;
        if size > limits.max_header_bytes {
            return None;
        }
        let end = p.checked_add(size)?;
        let header = bytes.get(covered..end)?;
        // The CRC covers the size field and everything after it, which is
        // every byte this function is about to believe.
        if crc32(header) != stored {
            return None;
        }

        let kind = vint(bytes, &mut p)?;
        let flags = vint(bytes, &mut p)?;
        let extra_size = if flags & HAS_EXTRA != 0 {
            usize::try_from(vint(bytes, &mut p)?).ok()?
        } else {
            0
        };
        let data = if flags & HAS_DATA != 0 {
            usize::try_from(vint(bytes, &mut p)?).ok()?
        } else {
            0
        };
        let extra = (extra_size > 0).then(|| (end.saturating_sub(extra_size), extra_size));
        Some(Header {
            kind,
            flags,
            body: p,
            extra,
            end,
            data,
        })
    }
}

/// A file or service record, as its header describes it.
fn file_entry(
    bytes: &[u8],
    header: &Header,
    index: usize,
    limits: &Limits,
    warnings: &mut Vec<Warning>,
) -> Option<Entry> {
    // **Bounded by the header's own end**, so a field that runs long reads
    // nothing rather than reading the next header's bytes. The header CRC has
    // already passed at this point, so these bytes are the writer's -- but the
    // writer is untrusted and a `NameLength` past the header is exactly what a
    // hostile one writes.
    let bytes = bytes.get(..header.end)?;
    let mut p = header.body;
    let file_flags = vint(bytes, &mut p)?;
    let size = vint(bytes, &mut p)?;
    let _attributes = vint(bytes, &mut p)?;
    if file_flags & FILE_HAS_MTIME != 0 {
        p = p.checked_add(4)?;
    }
    let crc = if file_flags & FILE_HAS_CRC != 0 {
        let value = u32::from_le_bytes(bytes.get(p..p.checked_add(4)?)?.try_into().ok()?);
        p = p.checked_add(4)?;
        Some(value)
    } else {
        None
    };
    let compression = vint(bytes, &mut p)?;
    let _host_os = vint(bytes, &mut p)?;
    let name_len = usize::try_from(vint(bytes, &mut p)?).ok()?;
    let raw = bytes.get(p..p.checked_add(name_len)?)?;

    // The six fields RAR 5 packs into one integer. Only three of them decide
    // anything here; the dictionary size and the algorithm version decide how
    // to decompress, which this build does not.
    let method = ((compression >> 7) & 0x07) as u8;
    let solid = (compression >> 6) & 1 == 1;

    let mut name = decode_name(raw, index, limits, warnings);
    // RAR records whatever separator the packing machine used, and page order
    // is decided by the name.
    if name.contains('\\') {
        name = name.replace('\\', "/");
    }

    let encrypted = header
        .extra
        .is_some_and(|(at, len)| has_extra_record(bytes, at, len, EXTRA_ENCRYPTION));

    let kind = if header.kind == TYPE_SERVICE {
        Kind::Service
    } else if file_flags & FILE_DIRECTORY != 0 {
        Kind::Directory
    } else {
        Kind::File
    };
    if kind == Kind::File && crc.is_none() {
        warnings.push(Warning::NoCrcRecorded { index });
    }
    // An unknown unpacked size means the writer was streaming. Nothing here
    // can page such an entry, and saying its size is zero is a lie a caller
    // would act on, so it is listed with the packed size instead.
    let size = if file_flags & FILE_UNKNOWN_SIZE != 0 {
        header.data as u64
    } else {
        size
    };

    Some(Entry {
        name,
        size,
        kind,
        crc,
        method,
        solid,
        encrypted,
        index,
        at: header.end,
        packed: header.data,
    })
}

/// Whether the extra area holds a record of `want`.
///
/// The extra area is a chain of `size, type, data` records; a size that does
/// not advance ends the walk, which is the same shape tar's PAX parser needs
/// and for the same reason.
fn has_extra_record(bytes: &[u8], at: usize, len: usize, want: u64) -> bool {
    let Some(area) = bytes.get(at..at.saturating_add(len)) else {
        return false;
    };
    let mut p = 0usize;
    while p < area.len() {
        let start = p;
        let Some(size) = vint(area, &mut p).and_then(|n| usize::try_from(n).ok()) else {
            return false;
        };
        let Some(record) = area.get(p..p.saturating_add(size)) else {
            return false;
        };
        let mut inner = 0usize;
        if vint(record, &mut inner) == Some(want) {
            return true;
        }
        let Some(next) = p.checked_add(size).filter(|next| *next > start) else {
            return false;
        };
        p = next;
    }
    false
}

/// RAR 5's variable-length integer: seven bits a byte, least significant
/// first, high bit set on every byte but the last.
///
/// Capped at ten bytes, which is `ceil(64 / 7)` — a `vint` longer than that
/// cannot be a `u64` and is a file spending the walk's time rather than
/// describing anything.
fn vint(bytes: &[u8], at: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    for step in 0..10u32 {
        let byte = *bytes.get(*at)?;
        *at += 1;
        value |= u64::from(byte & 0x7F).checked_shl(step * 7).unwrap_or(0);
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

/// A name, decoded and bounded.
///
/// RAR 5 declares names to be UTF-8, so bytes that are not are damage — but a
/// comic whose ninth page has a broken character in its name still has nine
/// pages, so the decode is lossy and total (ruling 4) and the warning is what
/// says so.
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
            String::from_utf8_lossy(raw).into_owned()
        }
    }
}

#[cfg(test)]
mod tests;
