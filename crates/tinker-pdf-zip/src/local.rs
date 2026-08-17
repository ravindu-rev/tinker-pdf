//! Local file headers (APPNOTE 4.3.7) and data descriptors (4.3.9).
//!
//! The local header is read for exactly one thing this crate believes: **where
//! the entry's data starts.** Its name and extra-field lengths are its own —
//! APPNOTE allows the local extra field to differ from the central one, and
//! routinely it does — so the data offset can only be computed from the header
//! that sits in front of the data.
//!
//! Everything else in it is compared against the central directory and never
//! followed; [`crate::Archive`] documents why the central directory wins.

use crate::le::{u16le, u32le, u64le};

pub(crate) const LOCAL_SIG: [u8; 4] = *b"PK\x03\x04";
pub(crate) const CENTRAL_SIG: [u8; 4] = *b"PK\x01\x02";
pub(crate) const EOCD_SIG: [u8; 4] = *b"PK\x05\x06";
pub(crate) const ZIP64_EOCD_SIG: [u8; 4] = *b"PK\x06\x06";
pub(crate) const ZIP64_LOCATOR_SIG: [u8; 4] = *b"PK\x06\x07";
pub(crate) const DESCRIPTOR_SIG: [u8; 4] = *b"PK\x07\x08";

/// The fixed part of a local file header is 30 bytes (4.3.7).
pub(crate) const LOCAL_FIXED: usize = 30;

/// General-purpose bit 0: the entry is encrypted (4.4.4).
pub(crate) const FLAG_ENCRYPTED: u16 = 1 << 0;
/// General-purpose bit 3: the sizes and CRC follow the data (4.3.9).
pub(crate) const FLAG_STREAMED: u16 = 1 << 3;
/// General-purpose bit 11: the name is UTF-8 rather than CP437 (D.2).
pub(crate) const FLAG_UTF8: u16 = 1 << 11;

/// The header in front of one entry's data.
pub(crate) struct LocalHeader {
    pub flags: u16,
    pub method: u16,
    pub crc: u32,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
    pub name: Vec<u8>,
    /// A Zip64 extended information extra field was present, which is also
    /// what 4.3.9.2 says decides whether a data descriptor here carries
    /// 32-bit or 64-bit sizes.
    ///
    /// This is not decoration, and validation cannot replace it. A descriptor
    /// holding a 64-bit compressed size of 7 begins with the same four bytes
    /// as one holding a 32-bit 7, because little-endian puts the significant
    /// half first -- so for every size below 4 GB, reading a 64-bit descriptor
    /// as 32-bit *succeeds*, agrees with the length the caller brought, and
    /// silently takes the high half of the compressed size as the uncompressed
    /// one. Trying both widths and keeping whichever matches therefore always
    /// keeps 32-bit, and is wrong exactly when it matters.
    ///
    /// So the flag chooses and validation confirms. See [`parse_descriptor`].
    pub zip64: bool,
    /// The first byte of the entry's data.
    pub data_offset: usize,
}

/// Reads the header at `at`, or `None` if it is not one or does not fit.
pub(crate) fn parse_local(bytes: &[u8], at: usize) -> Option<LocalHeader> {
    if bytes.get(at..at.checked_add(4)?)? != LOCAL_SIG {
        return None;
    }
    let flags = u16le(bytes, at + 6)?;
    let method = u16le(bytes, at + 8)?;
    let crc = u32le(bytes, at + 14)?;
    let compressed = u32le(bytes, at + 18)?;
    let uncompressed = u32le(bytes, at + 22)?;
    let name_len = usize::from(u16le(bytes, at + 26)?);
    let extra_len = usize::from(u16le(bytes, at + 28)?);

    let name_at = at.checked_add(LOCAL_FIXED)?;
    let name = bytes.get(name_at..name_at.checked_add(name_len)?)?.to_vec();
    let extra_at = name_at + name_len;
    let extra = bytes.get(extra_at..extra_at.checked_add(extra_len)?)?;
    let data_offset = extra_at + extra_len;

    let zip64 = zip64_extra(extra).is_some();
    let (compressed_size, uncompressed_size) = match zip64_extra(extra) {
        // 4.5.3's fields appear in a fixed order and only for the 32-bit
        // fields that were set to the sentinel, so which one a value belongs
        // to depends on which sentinels are present — the order is not a
        // per-field tag and cannot be read as one.
        Some(z) => {
            let mut p = 0;
            let mut un = u64::from(uncompressed);
            let mut co = u64::from(compressed);
            if uncompressed == u32::MAX {
                un = u64le(z, p)?;
                p += 8;
            }
            if compressed == u32::MAX {
                co = u64le(z, p)?;
            }
            (co, un)
        }
        None => (u64::from(compressed), u64::from(uncompressed)),
    };

    Some(LocalHeader {
        flags,
        method,
        crc,
        compressed_size,
        uncompressed_size,
        name,
        zip64,
        data_offset,
    })
}

/// The payload of the Zip64 extended information extra field (4.5.3), if the
/// extra area holds one.
///
/// The extra area is a sequence of `(id, len, data)` records and a malformed
/// length must end the walk rather than wrap it: the whole area is
/// attacker-chosen.
pub(crate) fn zip64_extra(extra: &[u8]) -> Option<&[u8]> {
    let mut p = 0usize;
    while p + 4 <= extra.len() {
        let id = u16le(extra, p)?;
        let len = usize::from(u16le(extra, p + 2)?);
        let start = p + 4;
        let end = start.checked_add(len)?;
        if end > extra.len() {
            return None;
        }
        if id == 0x0001 {
            return extra.get(start..end);
        }
        p = end;
    }
    None
}

/// What a data descriptor said, and where it ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Descriptor {
    pub crc: u32,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
    /// The first byte after the descriptor, which is where the next local
    /// header begins.
    pub end: usize,
}

/// Reads the descriptor that begins at `at`, given the compressed length the
/// entry is already known to have.
///
/// Four shapes are legal and none of them is tagged: the signature is optional
/// (4.3.9.3 says it *should* be there and it is a "SHOULD"), and the two sizes
/// are 4 bytes each or 8 bytes each depending on whether the entry is Zip64
/// (4.3.9.2), and two different things pick between them.
///
/// **`zip64` picks the width**, because nothing else can. 4.3.9.2 says a Zip64
/// extended information extra field in the local header is what makes the
/// descriptor's sizes 64-bit, and that is not recoverable from the bytes: a
/// 64-bit little-endian 7 starts with the same four bytes as a 32-bit 7, so a
/// 64-bit descriptor read as 32-bit matches any expected length under 4 GB and
/// hands back the high half of the compressed size as the uncompressed one. An
/// earlier draft of this function tried both widths and kept whichever agreed;
/// it was wrong for every archive small enough to be ordinary, which is to say
/// all of them.
///
/// **`expected_compressed` picks the signature**, and confirms the width. It is
/// the length the caller already knows -- from the consumed length of the
/// deflate stream, or from the distance back to the start of the data -- and a
/// candidate whose compressed size disagrees with it is not this entry's
/// descriptor. The declared width is tried first and the other after it,
/// because a writer that sets the flag wrongly is a real archive and the
/// fallback costs one comparison.
pub(crate) fn parse_descriptor(
    bytes: &[u8],
    at: usize,
    expected_compressed: u64,
    zip64: bool,
) -> Option<Descriptor> {
    let signed = bytes.get(at..at.checked_add(4)?) == Some(&DESCRIPTOR_SIG[..]);
    // With the signature skipped first, because that is the common shape; an
    // unsigned descriptor whose CRC happens to be `PK\x07\x08` is then still
    // reachable by the second candidate rather than lost to the first.
    let starts = if signed { [Some(at + 4), Some(at)] } else { [Some(at), None] };

    // The declared width first, the other only as a fallback for a writer that
    // flagged it wrongly. Never the other order: see above.
    let widths = if zip64 { [true, false] } else { [false, true] };

    for start in starts.into_iter().flatten() {
        for wide in widths {
            let read = if wide {
                (
                    u32le(bytes, start),
                    u64le(bytes, start + 4),
                    u64le(bytes, start + 12),
                    start + 20,
                )
            } else {
                (
                    u32le(bytes, start),
                    u32le(bytes, start + 4).map(u64::from),
                    u32le(bytes, start + 8).map(u64::from),
                    start + 12,
                )
            };
            if let (Some(crc), Some(c), Some(u), end) = read {
                if c == expected_compressed {
                    return Some(Descriptor {
                        crc,
                        compressed_size: c,
                        uncompressed_size: u,
                        end,
                    });
                }
            }
        }
    }
    None
}

/// Finds a signed data descriptor for an entry whose data starts at
/// `data_offset` and whose length is unknown.
///
/// This is the recovery route's answer for a **stored** entry with general-
/// purpose bit 3 set: there is no deflate stream to consume, so there is no
/// consumed length to locate the descriptor from, and the only handle left is
/// the signature. Each candidate is validated by the one number that ties it
/// to this entry — its compressed size must be the distance back to the start
/// of the data — so a `PK\x07\x08` that happens to occur inside the data is
/// rejected rather than believed.
///
/// An unsigned descriptor is not recoverable this way and is not searched for.
/// The entry is listed and refused at [`crate::Archive::read`], which is the
/// honest outcome: nothing here can produce a checksum for it, and gap 29's
/// pass-through design is what makes handing back unchecked bytes the one
/// answer this crate may not give.
pub(crate) fn search_descriptor(bytes: &[u8], data_offset: usize) -> Option<Descriptor> {
    let mut from = data_offset;
    while let Some(at) = crate::le::find(bytes, from, &DESCRIPTOR_SIG) {
        let expected = (at - data_offset) as u64;
        if let Some(d) = parse_descriptor(bytes, at, expected, false) {
            return Some(d);
        }
        from = at + 4;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The four shapes, each disambiguated by the length the caller brings.
    #[test]
    fn a_descriptor_is_read_in_whichever_of_its_four_shapes_it_has() {
        let mut signed_32 = Vec::new();
        signed_32.extend_from_slice(&DESCRIPTOR_SIG);
        signed_32.extend_from_slice(&0xdead_beefu32.to_le_bytes());
        signed_32.extend_from_slice(&7u32.to_le_bytes());
        signed_32.extend_from_slice(&9u32.to_le_bytes());
        let d = parse_descriptor(&signed_32, 0, 7, false).expect("signed, 32-bit");
        assert_eq!((d.crc, d.uncompressed_size, d.end), (0xdead_beef, 9, 16));

        let unsigned_32 = &signed_32[4..];
        let d = parse_descriptor(unsigned_32, 0, 7, false).expect("unsigned, 32-bit");
        assert_eq!((d.crc, d.uncompressed_size, d.end), (0xdead_beef, 9, 12));

        let mut signed_64 = Vec::new();
        signed_64.extend_from_slice(&DESCRIPTOR_SIG);
        signed_64.extend_from_slice(&1u32.to_le_bytes());
        signed_64.extend_from_slice(&7u64.to_le_bytes());
        signed_64.extend_from_slice(&9u64.to_le_bytes());
        let d = parse_descriptor(&signed_64, 0, 7, true).expect("signed, 64-bit");
        assert_eq!((d.crc, d.uncompressed_size, d.end), (1, 9, 24));

        let unsigned_64 = &signed_64[4..];
        let d = parse_descriptor(unsigned_64, 0, 7, true).expect("unsigned, 64-bit");
        assert_eq!((d.crc, d.uncompressed_size, d.end), (1, 9, 20));
    }

    /// A descriptor whose compressed size is not the length the caller already
    /// knows is not this entry's descriptor, whatever it looks like.
    #[test]
    fn a_descriptor_that_does_not_agree_with_the_known_length_is_refused() {
        let mut d = Vec::new();
        d.extend_from_slice(&DESCRIPTOR_SIG);
        d.extend_from_slice(&0u32.to_le_bytes());
        d.extend_from_slice(&7u32.to_le_bytes());
        d.extend_from_slice(&9u32.to_le_bytes());
        assert_eq!(parse_descriptor(&d, 0, 8, false), None);
        assert_eq!(parse_descriptor(&d, 0, 7, false).map(|d| d.end), Some(16));
    }

    /// The search skips a signature inside the data, because the candidate's
    /// own compressed size does not point back at the start of the entry.
    #[test]
    fn the_search_steps_over_a_signature_that_occurs_inside_the_data() {
        let mut bytes = vec![0u8; 4];
        // A decoy: the signature, followed by numbers that make no sense as
        // this entry's descriptor.
        bytes.extend_from_slice(&DESCRIPTOR_SIG);
        bytes.extend_from_slice(&[0xff; 12]);
        let data_len = bytes.len() - 4;
        bytes.extend_from_slice(&DESCRIPTOR_SIG);
        bytes.extend_from_slice(&5u32.to_le_bytes());
        bytes.extend_from_slice(&(data_len as u32).to_le_bytes());
        bytes.extend_from_slice(&3u32.to_le_bytes());
        let d = search_descriptor(&bytes, 4).expect("the real one, past the decoy");
        assert_eq!((d.crc, d.compressed_size, d.uncompressed_size), (5, 16, 3));
    }

    /// An extra area whose record length runs past its own end stops the walk
    /// rather than wrapping it.
    #[test]
    fn a_zip64_extra_field_with_a_length_past_the_area_is_refused() {
        let good = [0x01, 0x00, 0x08, 0x00, 1, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(zip64_extra(&good).map(<[u8]>::len), Some(8));
        let bad = [0x01, 0x00, 0xff, 0xff, 1, 0, 0, 0];
        assert_eq!(zip64_extra(&bad), None);
        let other_first = [
            0x99, 0x99, 0x02, 0x00, 0, 0, 0x01, 0x00, 0x08, 0x00, 2, 0, 0, 0, 0, 0, 0, 0,
        ];
        assert_eq!(zip64_extra(&other_first).and_then(|z| u64le(z, 0)), Some(2));
    }
}
