//! Little-endian reads that are total.
//!
//! Every multi-byte field in a ZIP is little-endian (APPNOTE 4.4), and every
//! offset that reaches one of these comes out of the file. So each returns
//! `Option` rather than indexing: a record that claims a field past the end of
//! the archive is a record this crate declines to believe, which is a
//! different outcome from a panic and the only one ruling 1 permits.

pub(crate) fn u16le(bytes: &[u8], at: usize) -> Option<u16> {
    let s = bytes.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([s[0], s[1]]))
}

pub(crate) fn u32le(bytes: &[u8], at: usize) -> Option<u32> {
    let s = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

pub(crate) fn u64le(bytes: &[u8], at: usize) -> Option<u64> {
    let s = bytes.get(at..at.checked_add(8)?)?;
    Some(u64::from_le_bytes([
        s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7],
    ]))
}

/// A 64-bit file offset as an index into a buffer that may be smaller than
/// 2^32.
///
/// `usize` is 32 bits on both wasm targets this engine builds for, so a Zip64
/// archive's offsets do not all fit on every platform this runs on. Refusing
/// the conversion is what turns that into a named refusal instead of a
/// truncating cast that lands somewhere plausible.
pub(crate) fn offset(value: u64, len: usize) -> Option<usize> {
    let at = usize::try_from(value).ok()?;
    (at <= len).then_some(at)
}

/// Finds the last occurrence of `needle` in `haystack`.
pub(crate) fn rfind(haystack: &[u8], needle: &[u8; 4]) -> Option<usize> {
    if haystack.len() < 4 {
        return None;
    }
    (0..=haystack.len() - 4).rev().find(|&i| {
        // `get` rather than a slice index: the range is derived above and is
        // in bounds, but this loop is on the recovery path for hostile input
        // and a bounds check here costs nothing measurable.
        haystack.get(i..i + 4) == Some(&needle[..])
    })
}

/// Finds the first occurrence of `needle` at or after `from`.
pub(crate) fn find(haystack: &[u8], from: usize, needle: &[u8; 4]) -> Option<usize> {
    if haystack.len() < 4 || from > haystack.len() - 4 {
        return None;
    }
    (from..=haystack.len() - 4).find(|&i| haystack.get(i..i + 4) == Some(&needle[..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_field_past_the_end_is_none_rather_than_a_panic() {
        let bytes = [1u8, 2, 3];
        assert_eq!(u16le(&bytes, 1), Some(0x0302));
        assert_eq!(u16le(&bytes, 2), None);
        assert_eq!(u32le(&bytes, 0), None);
        assert_eq!(u64le(&bytes, 0), None);
        assert_eq!(u16le(&bytes, usize::MAX), None);
    }

    #[test]
    fn an_offset_past_the_buffer_is_refused() {
        assert_eq!(offset(4, 8), Some(4));
        assert_eq!(offset(8, 8), Some(8));
        assert_eq!(offset(9, 8), None);
        assert_eq!(offset(u64::MAX, 8), None);
    }

    #[test]
    fn the_searches_find_the_last_and_the_first() {
        let sig = *b"PK\x03\x04";
        let bytes = b"..PK\x03\x04..PK\x03\x04..";
        assert_eq!(find(bytes, 0, &sig), Some(2));
        assert_eq!(find(bytes, 3, &sig), Some(8));
        assert_eq!(rfind(bytes, &sig), Some(8));
        assert_eq!(find(b"PK\x03", 0, &sig), None);
        assert_eq!(rfind(b"PK\x03", &sig), None);
        assert_eq!(find(bytes, bytes.len(), &sig), None);
    }
}
