//! The end-of-central-directory record and the directory it points at
//! (APPNOTE 4.3.16, 4.3.12), plus Zip64's versions of both (4.3.14, 4.3.15).
//!
//! This is the route. A ZIP is written back-to-front — the directory is the
//! last thing appended, so it is the only structure that knows what the
//! archive finally contained — which is why a writer that crashed halfway
//! leaves local headers with no directory, and why [`super::scan`] exists.
//!
//! # Why the EOCD is searched backwards, and bounded
//!
//! The record ends with a comment whose length is a `u16`, so the signature
//! can sit anywhere in the last 65 557 bytes and nowhere else. Searching from
//! the end and stopping there is both correct and a bound: an archive whose
//! last 64 KiB hold no `PK\x05\x06` has no directory to find, however many
//! megabytes precede it.
//!
//! # What is checked and what is trusted
//!
//! Every offset is checked against the buffer before it is used, and the
//! directory's own entry count is a hint rather than an instruction: the walk
//! stops at whichever comes first of the count, the caller's cap, and the
//! bytes running out. A count of four billion in a two-kilobyte file is a
//! declaration, not an allocation.

use crate::le;
use crate::local::{self, CENTRAL_SIG, EOCD_SIG, ZIP64_EOCD_SIG, ZIP64_LOCATOR_SIG};
use crate::{Entry, Limits, Method, Warning, Warnings};

/// The furthest back a `PK\x05\x06` can be: the fixed record is 22 bytes and
/// the comment after it is bounded by its own `u16` length.
const EOCD_SEARCH: usize = 22 + 0xFFFF;

/// One central-directory record's fixed part (4.3.12).
const CENTRAL_FIXED: usize = 46;

/// Reads the central directory, or `None` if there is not one to read.
///
/// `None` is not a failure — it is the signal to fall back to a local-header
/// scan, and the caller records that as [`Warning::RecoveredFromLocalHeaders`]
/// rather than as an error. An archive genuinely refuses only when neither
/// route finds an entry.
pub(crate) fn central_directory(
    bytes: &[u8],
    limits: &Limits,
    warnings: &mut Warnings,
) -> Result<Option<Vec<Entry>>, crate::ArchiveError> {
    let Some(eocd) = find_eocd(bytes) else {
        // Distinct from `CentralDirectoryUnreadable`, and the difference is
        // worth a variant: no record at all is a writer that stopped or a
        // download that was cut, while a record that does not lead anywhere is
        // a file that was damaged after it was finished. Both fall back to the
        // scan, and a caller reporting to a human wants to say which.
        warnings.push(Warning::NoEndOfCentralDirectory);
        return Ok(None);
    };

    // 4.3.16's fields, at their offsets from the signature.
    let disk = le::u16le(bytes, eocd + 4).unwrap_or(0);
    let directory_disk = le::u16le(bytes, eocd + 6).unwrap_or(0);
    let mut count = u64::from(le::u16le(bytes, eocd + 10).unwrap_or(0));
    let mut start = u64::from(le::u32le(bytes, eocd + 16).unwrap_or(0));

    // Zip64, when the 32-bit record says its fields overflowed (4.3.14).
    // The locator sits immediately before the EOCD; the record it names holds
    // the real count and offset.
    if count == 0xFFFF || start == 0xFFFF_FFFF {
        if let Some((zip64_count, zip64_start)) = zip64(bytes, eocd) {
            count = zip64_count;
            start = zip64_start;
        } else {
            // The sentinel says "look in the Zip64 record" and there is not
            // one. Refusing by name beats walking from a truncated offset.
            return Err(crate::ArchiveError::Zip64OutOfBounds);
        }
    }

    // A multi-disk archive is not damaged, it is incomplete, and no amount of
    // reading this file will produce the rest of it.
    if disk != 0 || directory_disk != 0 {
        return Err(crate::ArchiveError::MultiDisk);
    }

    let Some(mut at) = le::offset(start, bytes.len()) else {
        // The directory is not where the record says. Recovery may still
        // find the entries, so this is a fallback rather than a refusal.
        warnings.push(Warning::CentralDirectoryUnreadable);
        return Ok(None);
    };

    let mut entries = Vec::new();
    // The declared count bounds the walk, but so do the caller's cap and the
    // buffer: whichever runs out first wins. `count` is the file's claim.
    while entries.len() < limits.max_entries {
        if u64::try_from(entries.len()).unwrap_or(u64::MAX) >= count {
            break;
        }
        if bytes.get(at..at + 4) != Some(&CENTRAL_SIG[..]) {
            break;
        }
        match parse_central(bytes, at, entries.len(), limits, warnings) {
            Some((entry, next)) => {
                entries.push(entry);
                at = next;
            }
            None => {
                // A record that does not parse ends the walk rather than the
                // archive: what came before it is still real.
                warnings.push(Warning::CentralDirectoryUnreadable);
                break;
            }
        }
    }

    // The cap refuses rather than warns. A directory naming more entries than
    // the caller will hold is not a document with some pages missing -- it is
    // an archive this build declines, and gap 29's rule is that a page count
    // is either right or refused, never quietly short.
    if entries.len() >= limits.max_entries {
        return Err(crate::ArchiveError::TooManyEntries);
    }
    // Fewer records than the count promised is a truncated directory, and the
    // difference is worth saying: a reader that sees 40 of 200 pages should
    // be told the archive claimed 200.
    if u64::try_from(entries.len()).unwrap_or(u64::MAX) < count && !entries.is_empty() {
        warnings.push(Warning::EntryCountDisagrees);
    }

    if entries.is_empty() {
        return Ok(None);
    }
    Ok(Some(entries))
}

/// The last `PK\x05\x06` in the final 64 KiB and change.
fn find_eocd(bytes: &[u8]) -> Option<usize> {
    let from = bytes.len().saturating_sub(EOCD_SEARCH);
    le::rfind(&bytes[from..], &EOCD_SIG).map(|at| from + at)
}

/// Zip64's locator and record (4.3.15, 4.3.14): `(entry count, directory
/// offset)`.
fn zip64(bytes: &[u8], eocd: usize) -> Option<(u64, u64)> {
    // The locator is fixed at 20 bytes and sits immediately before the EOCD.
    let locator = eocd.checked_sub(20)?;
    if bytes.get(locator..locator + 4) != Some(&ZIP64_LOCATOR_SIG[..]) {
        return None;
    }
    let record = le::offset(le::u64le(bytes, locator + 8)?, bytes.len())?;
    if bytes.get(record..record + 4) != Some(&ZIP64_EOCD_SIG[..]) {
        return None;
    }
    let count = le::u64le(bytes, record + 32)?;
    let start = le::u64le(bytes, record + 48)?;
    Some((count, start))
}

/// One central-directory record, and where the next one starts.
fn parse_central(
    bytes: &[u8],
    at: usize,
    index: usize,
    limits: &Limits,
    warnings: &mut Warnings,
) -> Option<(Entry, usize)> {
    let flags = le::u16le(bytes, at + 8)?;
    let method = le::u16le(bytes, at + 10)?;
    let crc = le::u32le(bytes, at + 16)?;
    let mut compressed = u64::from(le::u32le(bytes, at + 20)?);
    let mut uncompressed = u64::from(le::u32le(bytes, at + 24)?);
    let name_len = usize::from(le::u16le(bytes, at + 28)?);
    let extra_len = usize::from(le::u16le(bytes, at + 30)?);
    let comment_len = usize::from(le::u16le(bytes, at + 32)?);
    let mut header_offset = u64::from(le::u32le(bytes, at + 42)?);

    let name_at = at + CENTRAL_FIXED;
    let extra_at = name_at.checked_add(name_len)?;
    let comment_at = extra_at.checked_add(extra_len)?;
    let next = comment_at.checked_add(comment_len)?;
    if next > bytes.len() {
        return None;
    }

    // Zip64's extra field replaces whichever of the three overflowed, in a
    // fixed order and only for the ones carrying the sentinel (4.5.3). Reading
    // them positionally without checking which are present is the classic way
    // to get an offset from a size.
    if uncompressed == 0xFFFF_FFFF || compressed == 0xFFFF_FFFF || header_offset == 0xFFFF_FFFF {
        let extra = bytes.get(extra_at..comment_at)?;
        let zip64 = local::zip64_extra(extra)?;
        let mut cursor = 0usize;
        if uncompressed == 0xFFFF_FFFF {
            uncompressed = le::u64le(zip64, cursor)?;
            cursor += 8;
        }
        if compressed == 0xFFFF_FFFF {
            compressed = le::u64le(zip64, cursor)?;
            cursor += 8;
        }
        if header_offset == 0xFFFF_FFFF {
            header_offset = le::u64le(zip64, cursor)?;
        }
    }

    let raw_name = bytes.get(name_at..extra_at)?;
    let name = crate::decode_name(raw_name, flags, limits, warnings);

    Some((
        Entry {
            name,
            method: Method::from_code(method),
            crc: Some(crc),
            compressed_size: compressed,
            uncompressed_size: uncompressed,
            encrypted: flags & local::FLAG_ENCRYPTED != 0,
            streamed: flags & local::FLAG_STREAMED != 0,
            header_offset,
            index,
        },
        next,
    ))
}
