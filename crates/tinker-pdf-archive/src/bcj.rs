//! BCJ, 7z coder `03030103`: the x86 branch converter, run backwards.
//!
//! Not a compressor. An x86 `CALL rel32` (`E8`) or `JMP rel32` (`E9`) carries
//! its target *relative* to the next instruction, so two calls to one function
//! from two places are two different byte strings and an LZ coder sees no
//! repetition. The encoder rewrites each such operand into an **absolute**
//! address, which does repeat, and the decoder here turns it back. `-mf=BCJ`
//! puts it in front of LZMA2, and py7zr's `FILTER_X86` does the same.
//!
//! # Where it was written from, and what adjudicates it
//!
//! There is no specification of this filter beyond its implementations. It is
//! written from the conversion as 7-Zip's `C/Bra86.c` states it (read at
//! `ip7z/7zip` 26.02, a file its header puts in the public domain, "Igor
//! Pavlov : Public domain"): which bytes are candidates, the three-bit mask of
//! recent `E8`/`E9` bytes that decides whether one inside another's operand is
//! an opcode, the "top byte is 00 or FF" test on the operand, and the second
//! conversion when a neighbour's operand byte would otherwise decode wrongly.
//! Nothing of it is copied; the shape below is a plain byte loop where 7-Zip's
//! is an unrolled scan.
//!
//! What says it is right is **the archive's CRC-32 over the original bytes**,
//! checked in `sevenz::Archive::read` after this runs, exactly as for LZMA — a
//! filter wrong by one address fails the format's own check.
//! `tests/coders/py7zr-bcj.7z` is py7zr's BCJ over `x86.bin`, which is shaped
//! to reach every branch here.
//!
//! # Untrusted bytes
//!
//! The filter is total: every input has an output of the same length, and the
//! only thing a hostile stream can do is choose which operands get rewritten.
//! Every index is checked (ruling 1).

/// A byte that is an `E8` or `E9`.
fn is_branch(byte: u8) -> bool {
    byte & 0xFE == 0xE8
}

/// Whether an operand's top byte is `00` or `FF` — a displacement within
/// ±16 MiB, which is what the encoder converted and so what the decoder may.
fn near(byte: u8) -> bool {
    byte.wrapping_add(1) & 0xFE == 0
}

/// Undoes BCJ over a whole stream in place.
///
/// The position of the first byte is zero: 7z runs the filter over each
/// folder's output from its start, so the "instruction pointer" is the offset
/// in that output. The last four bytes are never converted, because an operand
/// there would run past the end — the encoder left them alone for the same
/// reason.
pub(crate) fn x86_decode(data: &mut [u8]) {
    let len = data.len();
    if len < 5 {
        return;
    }
    // Candidates start at most here: an `E8` needs four operand bytes after it.
    let limit = len - 4;
    // Bits 0-2 remember which of the three bytes before the current candidate
    // were themselves `E8`/`E9` bytes skipped as not-an-opcode, bit 2 being
    // the nearest. It is the encoder's state and must be the decoder's too.
    let mut mask: u32 = 0;
    // Where the previous candidate was, so the gap to this one ages the mask.
    let mut prev: Option<usize> = None;
    let mut pos = 0usize;

    while pos < limit {
        let Some(&byte) = data.get(pos) else { break };
        if !is_branch(byte) {
            pos += 1;
            continue;
        }
        // The distance back to the previous candidate. The mask ages by one
        // bit for every byte between the two, not counting the candidate it
        // already describes, hence `gap - 1`.
        let gap = prev.map_or(usize::MAX, |p| pos - p);
        prev = Some(pos);
        if gap > 3 {
            mask = 0;
        } else {
            mask >>= gap - 1;
            // A recent skipped branch byte whose operand this byte could be
            // part of: the encoder did not treat this byte as an opcode.
            let skip = mask != 0
                && (mask > 4
                    || mask == 3
                    || data
                        .get(pos + (mask as usize >> 1) + 1)
                        .copied()
                        .is_some_and(near));
            if skip {
                mask = (mask >> 1) | 4;
                pos += 1;
                continue;
            }
        }

        let top = data.get(pos + 4).copied().unwrap_or(0);
        if !near(top) {
            mask = (mask >> 1) | 4;
            pos += 1;
            continue;
        }
        let Some(operand) = data.get(pos + 1..pos + 5) else {
            break;
        };
        let mut v = u32::from_le_bytes([operand[0], operand[1], operand[2], operand[3]]);
        // The operand is relative to the end of the five-byte instruction.
        let ip = (pos as u32).wrapping_add(5);
        v = v.wrapping_sub(ip);
        if mask != 0 {
            let shift = (mask & 6) << 2;
            if near((v >> shift) as u8) {
                v ^= (0x100u32 << shift).wrapping_sub(1);
                v = v.wrapping_sub(ip);
            }
            mask = 0;
        }
        let out = [
            v as u8,
            (v >> 8) as u8,
            (v >> 16) as u8,
            // Bit 24 of the absolute address decides the sign the relative
            // one is written with: 00 or FF, as it was before encoding.
            0u8.wrapping_sub(((v >> 24) & 1) as u8),
        ];
        if let Some(slot) = data.get_mut(pos + 1..pos + 5) {
            slot.copy_from_slice(&out);
        }
        pos += 5;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The encoder's side, transcribed separately from the same description
    /// and used only to show the two agree on hand-built inputs. It is not
    /// what adjudicates the decoder — `py7zr-bcj.7z`'s CRC-32 is — but it
    /// reaches the mask paths with inputs small enough to read.
    fn x86_encode(data: &mut [u8]) {
        let len = data.len();
        if len < 5 {
            return;
        }
        let limit = len - 4;
        let mut mask: u32 = 0;
        let mut prev: Option<usize> = None;
        let mut pos = 0usize;
        while pos < limit {
            if !is_branch(data[pos]) {
                pos += 1;
                continue;
            }
            let gap = prev.map_or(usize::MAX, |p| pos - p);
            prev = Some(pos);
            if gap > 3 {
                mask = 0;
            } else {
                mask >>= gap - 1;
                if mask != 0
                    && (mask > 4 || mask == 3 || near(data[pos + (mask as usize >> 1) + 1]))
                {
                    mask = (mask >> 1) | 4;
                    pos += 1;
                    continue;
                }
            }
            if !near(data[pos + 4]) {
                mask = (mask >> 1) | 4;
                pos += 1;
                continue;
            }
            let mut v =
                u32::from_le_bytes([data[pos + 1], data[pos + 2], data[pos + 3], data[pos + 4]]);
            let ip = pos as u32 + 5;
            v = v.wrapping_add(ip);
            if mask != 0 {
                let shift = (mask & 6) << 2;
                if near((v >> shift) as u8) {
                    v ^= (0x100u32 << shift) - 1;
                    v = v.wrapping_add(ip);
                }
                mask = 0;
            }
            data[pos + 1] = v as u8;
            data[pos + 2] = (v >> 8) as u8;
            data[pos + 3] = (v >> 16) as u8;
            data[pos + 4] = 0u8.wrapping_sub(((v >> 24) & 1) as u8);
            pos += 5;
        }
    }

    #[test]
    fn a_call_s_relative_target_is_restored_from_the_absolute_one() {
        // At offset 16, `CALL +0x20` targets 16 + 5 + 0x20 = 0x35; the encoder
        // writes 0x35 and the decoder must write +0x20 back.
        let mut data = vec![0x90u8; 32];
        data[16..21].copy_from_slice(&[0xE8, 0x35, 0x00, 0x00, 0x00]);
        x86_decode(&mut data);
        assert_eq!(&data[16..21], &[0xE8, 0x20, 0x00, 0x00, 0x00]);

        // A backwards call: absolute 4 from offset 16 is -17.
        let mut data = vec![0x90u8; 32];
        data[16..21].copy_from_slice(&[0xE9, 0x04, 0x00, 0x00, 0x00]);
        x86_decode(&mut data);
        assert_eq!(&data[16..21], &[0xE9, 0xEF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn an_operand_whose_top_byte_is_not_near_is_left_alone() {
        let mut data = vec![0x90u8; 16];
        data[2..7].copy_from_slice(&[0xE8, 0x11, 0x22, 0x33, 0x44]);
        let before = data.clone();
        x86_decode(&mut data);
        assert_eq!(data, before);
    }

    #[test]
    fn the_last_four_bytes_are_never_converted() {
        let mut data = vec![0x90u8; 12];
        data[8..12].copy_from_slice(&[0xE8, 0x00, 0x00, 0x00]);
        let before = data.clone();
        x86_decode(&mut data);
        assert_eq!(data, before);
        let mut short = vec![0xE8, 0, 0, 0];
        x86_decode(&mut short);
        assert_eq!(short, vec![0xE8, 0, 0, 0]);
    }

    /// Clusters of `E8`/`E9` bytes are what the mask exists for, and every
    /// spacing of up to three between them is reached here.
    #[test]
    fn decode_undoes_encode_over_every_short_cluster_of_branch_bytes() {
        let mut state = 0x1234_5678u32;
        let mut next = || {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            (state >> 16) as u8
        };
        for _ in 0..2000 {
            let mut plain: Vec<u8> = (0..48)
                .map(|_| match next() % 6 {
                    0 => 0xE8,
                    1 => 0xE9,
                    2 => 0x00,
                    3 => 0xFF,
                    _ => next(),
                })
                .collect();
            let original = plain.clone();
            x86_encode(&mut plain);
            x86_decode(&mut plain);
            assert_eq!(plain, original);
        }
    }
}
