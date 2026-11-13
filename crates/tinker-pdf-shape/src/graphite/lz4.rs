//! One LZ4 block, decoded: the Graphite Table Format's compression scheme 1.
//!
//! The GTF names the scheme "an LZ4 block decompressor. This is not the LZ4
//! framing format just the inner block level format without any checksum",
//! and the LZ4 Block Format Description is what this reads (THIRDPARTY.md
//! names its revision). A block is a run of *sequences*: a token whose high
//! four bits are a literal count and whose low four a match length less
//! four, each continued by bytes while they read 255; the literals; and,
//! except in the last sequence, a two-byte little-endian offset back into
//! what has been decoded, from which the match is copied a byte at a time,
//! so a match may overlap the bytes it is still writing.
//!
//! # What is refused
//!
//! The decoded size is the table's own word for it, so it is the bound:
//! nothing is written past it, and a block that decodes to less is as wrong
//! as one that would decode to more. An offset of zero, which the format
//! calls "an invalid (corrupted) block", and one reaching before the first
//! decoded byte are refused; so is a block that ends inside a sequence —
//! inside its length bytes, its literals or its offset. Each refusal is
//! `None`, and the caller names the table (ruling 10). The format's
//! end-of-block conditions — the last five bytes literals, the last match
//! twelve bytes from the end — are a decoder's to reject *at its choice*,
//! and this one accepts a block that breaks them and decodes to its stated
//! size: they are rules an encoder keeps for older decoders' speed, and the
//! bytes are the same either way.

/// The bytes `block` decodes to, which must be exactly `size` of them.
///
/// `None` for a block the format calls corrupted, one that ends inside a
/// sequence, and one that decodes to more or fewer bytes than `size`.
pub(crate) fn decode(block: &[u8], size: usize) -> Option<Vec<u8>> {
    let mut out: Vec<u8> = Vec::with_capacity(size);
    let mut at = 0usize;
    while at < block.len() {
        let token = *block.get(at)?;
        at += 1;
        let literals = length(block, &mut at, usize::from(token >> 4))?;
        let end = at.checked_add(literals)?;
        let run = block.get(at..end)?;
        if out.len().checked_add(literals)? > size {
            return None;
        }
        out.extend_from_slice(run);
        at = end;
        if at == block.len() {
            // The last sequence holds only literals: "the block ends right
            // after the literals (no offset field)".
            break;
        }
        let offset = usize::from(u16::from_le_bytes([*block.get(at)?, *block.get(at + 1)?]));
        at += 2;
        // "0 is an invalid offset value", and an offset reaching before the
        // first byte names nothing.
        if offset == 0 || offset > out.len() {
            return None;
        }
        // "minmatch" is 4: the field counts from it.
        let matched = length(block, &mut at, usize::from(token & 0x0F))?.checked_add(4)?;
        if out.len().checked_add(matched)? > size {
            return None;
        }
        let start = out.len() - offset;
        if offset >= matched {
            out.extend_from_within(start..start + matched);
        } else {
            // "Match bytes are copied one at a time, in increasing order, so
            // matchlength may exceed offset: bytes just copied are copied
            // again."
            for index in start..start + matched {
                let byte = *out.get(index)?;
                out.push(byte);
            }
        }
    }
    (out.len() == size).then_some(out)
}

/// A token field's length: the field itself, and where it is 15 every byte
/// after it added on, while they read 255.
fn length(block: &[u8], at: &mut usize, field: usize) -> Option<usize> {
    let mut total = field;
    if field == 15 {
        loop {
            let byte = *block.get(*at)?;
            *at += 1;
            total = total.checked_add(usize::from(byte))?;
            if byte != 255 {
                break;
            }
        }
    }
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::decode;

    /// A block of literals alone decodes to them.
    #[test]
    fn a_block_of_literals_alone_is_its_literals() {
        assert_eq!(decode(&[0x30, b'a', b'b', b'c'], 3), Some(b"abc".to_vec()));
        // "Even empty input can be represented, using a zero byte".
        assert_eq!(decode(&[0x00], 0), Some(Vec::new()));
        assert_eq!(decode(&[], 0), Some(Vec::new()));
    }

    /// A match copies from the bytes already decoded, overlapping itself
    /// where its length passes its offset.
    #[test]
    fn a_match_copies_what_was_decoded() {
        // "ab", then four bytes from two back: "abab"; then "c" alone.
        let block = [0x20, b'a', b'b', 0x02, 0x00, 0x10, b'c'];
        assert_eq!(decode(&block, 7), Some(b"abababc".to_vec()));
        // An offset of 1 repeats the last byte matchlength times.
        let block = [0x13, b'x', 0x01, 0x00, 0x10, b'y'];
        assert_eq!(decode(&block, 9), Some(b"xxxxxxxxy".to_vec()));
    }

    /// The stated size is the bound both ways.
    #[test]
    fn a_block_decoding_to_another_size_is_refused() {
        assert_eq!(decode(&[0x30, b'a', b'b', b'c'], 2), None);
        assert_eq!(decode(&[0x30, b'a', b'b', b'c'], 4), None);
        let block = [0x13, b'x', 0x01, 0x00, 0x10, b'y'];
        assert_eq!(decode(&block, 8), None);
    }

    /// The corrupted blocks the format names, and blocks that end inside a
    /// sequence.
    #[test]
    fn a_corrupted_or_truncated_block_is_refused() {
        // An offset of zero.
        assert_eq!(decode(&[0x10, b'a', 0x00, 0x00, 0x00], 5), None);
        // An offset before the first byte.
        assert_eq!(decode(&[0x10, b'a', 0x02, 0x00, 0x00], 5), None);
        // Fewer literals than the token counts.
        assert_eq!(decode(&[0x30, b'a', b'b'], 3), None);
        // Half an offset.
        assert_eq!(decode(&[0x10, b'a', 0x01], 5), None);
        // A length continued past the block's end.
        assert_eq!(decode(&[0xF0, 255], 300), None);
        assert_eq!(decode(&[0x1F, b'a', 0x01, 0x00, 255], 300), None);
    }
}
