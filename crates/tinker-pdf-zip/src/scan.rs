//! The recovery rung: local file headers, found by scanning (APPNOTE 4.3.7).
//!
//! A ZIP is written back-to-front — the central directory is appended last, so
//! it is the only structure that knows what the archive finally held. A writer
//! that stopped early, a download that was truncated, or a file recovered from
//! a damaged medium therefore has local headers and no directory at all.
//!
//! `tinker-pdf-cos` takes exactly this posture for a damaged cross-reference
//! table, and for the same reason: the entries are still there, and refusing
//! the whole archive because its index is gone throws away every page to
//! report a missing table of contents.
//!
//! # What recovery cannot recover
//!
//! A local header written for a **streamed** entry carries zeroes for its
//! sizes and its CRC — the real values follow the data in a descriptor
//! (4.3.9), which is exactly the structure a scan cannot find without first
//! knowing where the data ends. `inflate_raw`'s `end` breaks that circle for a
//! deflated entry, because the decoder knows where the stream stopped. A
//! **stored** streamed entry has no such marker, and this module does not
//! guess: the entry is listed, so the page count stays honest, and
//! [`super::Archive::read`] refuses it by name rather than handing back bytes
//! whose length was inferred.
//!
//! That split is gap 17's rule. A JBIG2 file that found no generic region
//! returned the placeholder rather than a blank page reported as success; an
//! entry whose extent is unknown is refused rather than truncated to a guess.

use crate::le;
use crate::local::{self, LOCAL_SIG};
use crate::{Budget, Entry, Limits, Method, Warnings};

/// Enumerates entries by walking local file headers from the front.
///
/// Never fails: an archive with no recoverable header returns an empty vector
/// and the caller turns that into the refusal, because only the caller can
/// tell "not a ZIP at all" from "a ZIP whose entries are gone".
pub(crate) fn local_headers(
    bytes: &[u8],
    limits: &Limits,
    warnings: &mut Warnings,
    budget: &mut Budget,
) -> Result<Vec<Entry>, crate::ArchiveError> {
    let mut entries = Vec::new();
    let mut at = 0usize;

    while entries.len() < limits.max_entries {
        let Some(found) = le::find(bytes, at, &LOCAL_SIG) else {
            break;
        };
        let Some(header) = local::parse_local(bytes, found) else {
            // A signature that does not parse as a header is data that
            // happened to contain `PK\x03\x04` -- which real image bytes do,
            // often enough that skipping four bytes and continuing is the
            // only workable answer.
            at = found + 4;
            continue;
        };

        let data_at = header.data_offset;
        if data_at > bytes.len() {
            break;
        }

        // Where the entry's data ends decides where the next header can be.
        // Three cases, and only the third is a guess this module refuses to
        // make.
        let streamed = header.flags & local::FLAG_STREAMED != 0;
        let (compressed, uncompressed, crc) = if streamed {
            match extent(bytes, &header, data_at, limits, budget) {
                Some(triple) => triple,
                None => {
                    // A stored streamed entry: no descriptor findable without
                    // knowing the length, and no length without the
                    // descriptor. Listed, refused at read.
                    // No warning: the entry carries `crc: None`, and
                    // `Archive::read` refuses it as `NoChecksum`. Recording it
                    // twice would make one fault look like two.
                    entries.push(Entry {
                        name: crate::decode_name(&header.name, header.flags, limits, warnings),
                        method: Method::from_code(header.method),
                        crc: None,
                        compressed_size: 0,
                        uncompressed_size: 0,
                        encrypted: header.flags & local::FLAG_ENCRYPTED != 0,
                        streamed: true,
                        header_offset: found as u64,
                        index: entries.len(),
                    });
                    // Nothing reliable to skip past, so resume the search
                    // after this header rather than after its data.
                    at = data_at;
                    continue;
                }
            }
        } else {
            (
                header.compressed_size,
                header.uncompressed_size,
                Some(header.crc),
            )
        };

        entries.push(Entry {
            name: crate::decode_name(&header.name, header.flags, limits, warnings),
            method: Method::from_code(header.method),
            crc,
            compressed_size: compressed,
            uncompressed_size: uncompressed,
            encrypted: header.flags & local::FLAG_ENCRYPTED != 0,
            streamed,
            header_offset: found as u64,
            index: entries.len(),
        });

        // Past this entry's data, so the next header is found rather than
        // re-found inside these bytes.
        at = match le::offset(compressed, bytes.len().saturating_sub(data_at)) {
            Some(len) => data_at.saturating_add(len).max(data_at + 1),
            None => data_at + 1,
        };
    }

    if entries.len() >= limits.max_entries {
        return Err(crate::ArchiveError::TooManyEntries);
    }
    Ok(entries)
}

/// A streamed entry's real sizes and checksum, when they can be established
/// without guessing.
///
/// Deflated: `inflate_raw` reports where the stream ended, and the descriptor
/// follows it. **`end` is read only when the decode completed** -- a capped or
/// truncated decode records where the decoder gave up, and trusting it would
/// put the descriptor inside the entry's own bytes.
///
/// Stored: `None`. There is no marker, and inventing one is the failure this
/// module is written to avoid.
///
/// # This is the only inflate that happens at `open`
///
/// Every other decode in this crate is a caller asking for one entry, and it
/// spends [`crate::Archive::read`]'s budget as it goes. This one runs while the
/// archive is still being *enumerated*, before any caller has asked for
/// anything -- so if it did not charge, a damaged archive of a thousand
/// streamed deflated entries could inflate `max_entry_bytes` a thousand times
/// during `open` and never touch the total that exists to stop exactly that.
/// [`crate::limits::MAX_ZIP_INFLATED`] says it is "spent across every entry of
/// one archive and never refunded", and a path that skips it makes that
/// sentence false.
///
/// The charge is the bytes actually produced rather than the ceiling offered,
/// because the ceiling is what this is *allowed* to spend and the total counts
/// what was spent. A refusal here ends the scan rather than the archive: the
/// entries already recovered are real, and the caller is told the budget ran
/// out by the warning the scan pushes.
fn extent(
    bytes: &[u8],
    header: &local::LocalHeader,
    data_at: usize,
    limits: &Limits,
    budget: &mut Budget,
) -> Option<(u64, u64, Option<u32>)> {
    if header.method != 8 {
        return None;
    }
    let tail = bytes.get(data_at..)?;
    // The per-entry ceiling and what the archive has left, whichever is
    // smaller. Offering the per-entry ceiling when the total cannot cover it
    // would inflate bytes this is not permitted to spend and then refuse them.
    //
    // **No test can see this `min`, and that is worth saying rather than
    // leaving for somebody to discover.** Removing it changes no answer: the
    // decode still runs, `spend` still refuses, the entry is still listed
    // unsized and still refused at `read`. The only difference is the work
    // done before the refusal, and ruling 1 says a bound is proved by its own
    // refusal and never by a clock -- so the one symptom this has is the one
    // symptom this repository does not accept as evidence. The injection
    // matrix duly reports it as surviving, correctly.
    //
    // It stays because the alternative is doing the work the cap exists to
    // prevent and then declining to keep it, which is the shape of every
    // decompression bomb: `MAX_ZIP_INFLATED`'s point is that the bytes are
    // never produced, not that they are produced and dropped.
    let room = limits.max_entry_bytes.min(budget.remaining());
    let raw = tinker_pdf_filters::inflate_raw(tail, &tinker_pdf_filters::Limits::new(room));
    if !raw.complete {
        return None;
    }
    // Charged before the descriptor is looked for, because the work is already
    // done by here and the total counts work rather than success.
    budget.spend(raw.data.len()).ok()?;
    let descriptor =
        local::parse_descriptor(bytes, data_at + raw.end, raw.end as u64, header.zip64)
            .or_else(|| local::search_descriptor(bytes, data_at))?;
    Some((
        descriptor.compressed_size,
        descriptor.uncompressed_size,
        Some(descriptor.crc),
    ))
}
