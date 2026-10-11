//! BCJ2, 7z coder `0303011B`: the x86 branch converter with four streams.
//!
//! Where BCJ ([`crate::bcj`]) rewrites a call's operand in place, BCJ2 takes
//! it out of the byte stream altogether. Its encoder writes four streams: the
//! code with every converted operand removed (**main**), the absolute targets
//! of converted `E8` calls (**call**, big-endian), those of converted `E9`
//! jumps and `0F 8x` conditional jumps (**jump**), and a range-coded stream of
//! one decision per branch opcode — converted or not (**rc**). Each of the
//! first three can then be compressed with a coder suited to it, which is the
//! point, and is why a BCJ2 folder is not a chain: four coders' outputs meet
//! in one.
//!
//! # Where it was written from, and what adjudicates it
//!
//! 7-Zip's `C/Bcj2.c` and `C/Bcj2.h`, read at `ip7z/7zip` 26.02, whose headers
//! put them in the public domain ("Igor Pavlov : Public domain"): which bytes
//! are branch opcodes (`E8`, `E9`, and `8x` after `0F`), which of the 258
//! probabilities a decision is coded with (`E8` by the byte before it, `E9`
//! one, a conditional jump one), the 11-bit probabilities and five-bit
//! adaptation LZMA also uses, the first range-coder byte that must be zero,
//! and that a target is relative to the position after its four bytes. The
//! loop below is a plain transcription of that behaviour, not of 7-Zip's
//! unrolled, resumable one.
//!
//! What says it is right is the 7z archive's CRC-32 over the original bytes,
//! checked in `sevenz::Archive::read`. The fixture is 7-Zip 26.02's own BCJ2
//! (`tests/coders/7zz-bcj2.7z`) over `x86.bin`, whose `E8`, `E9` and `0F 8x`
//! branches target addresses inside the file, as a converter wants.
//!
//! # Untrusted bytes
//!
//! Every stream is read through a checked index; one that runs out before the
//! declared output is [`Error::Truncated`]. The output is sized by the
//! folder's declared length, which the caller has already bounded.

/// Why the four streams did not decode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Error {
    /// The range-coded stream's first byte is not zero, or its initial code is
    /// `FFFFFFFF` — the two checks 7-Zip makes on the five bytes that start it.
    BadRangeStart,
    /// A stream ran out before the declared output did.
    Truncated,
}

const TOP: u32 = 1 << 24;
const PROB_BITS: u32 = 11;
const MOVE_BITS: u32 = 5;

/// Whether `b1`, after `b0`, is an opcode BCJ2 made a decision about: a call
/// (`E8`), a jump (`E9`), or the second byte of a conditional jump (`0F 8x`).
fn is_branch(b0: u8, b1: u8) -> bool {
    b1 & 0xFE == 0xE8 || (b0 == 0x0F && b1 & 0xF0 == 0x80)
}

struct Range<'a> {
    input: &'a [u8],
    at: usize,
    range: u32,
    code: u32,
}

impl<'a> Range<'a> {
    fn new(input: &'a [u8]) -> Result<Self, Error> {
        let head = input.get(..5).ok_or(Error::Truncated)?;
        if head.first() != Some(&0) {
            return Err(Error::BadRangeStart);
        }
        // `head` is exactly five bytes: `get(..5)` above returned it.
        let code = u32::from_be_bytes([head[1], head[2], head[3], head[4]]);
        if code == u32::MAX {
            return Err(Error::BadRangeStart);
        }
        Ok(Range {
            input,
            at: 5,
            range: u32::MAX,
            code,
        })
    }

    /// One decision. Normalised before rather than after, as 7-Zip's decoder
    /// does, so the last decision never asks for a byte the encoder's flush
    /// did not write — and normalised once, so a decision reads at most one
    /// byte, which is what lets `sevenz`'s `feeders_fit` bound a decision
    /// stream another coder decodes by the output it serves.
    fn bit(&mut self, prob: &mut u16) -> Result<bool, Error> {
        if self.range < TOP {
            let byte = *self.input.get(self.at).ok_or(Error::Truncated)?;
            self.at += 1;
            self.range <<= 8;
            self.code = (self.code << 8) | u32::from(byte);
        }
        let bound = (self.range >> PROB_BITS).wrapping_mul(u32::from(*prob));
        if self.code < bound {
            self.range = bound;
            *prob += ((1 << PROB_BITS) - *prob) >> MOVE_BITS;
            Ok(false)
        } else {
            self.range -= bound;
            self.code -= bound;
            *prob -= *prob >> MOVE_BITS;
            Ok(true)
        }
    }
}

/// Rebuilds `out_size` bytes of x86 code from BCJ2's four streams.
pub(crate) fn decode(
    main: &[u8],
    call: &[u8],
    jump: &[u8],
    rc: &[u8],
    out_size: usize,
) -> Result<Vec<u8>, Error> {
    let mut rc = Range::new(rc)?;
    // `E8` decisions by the byte before, then one for `E9` and one for a
    // conditional jump.
    let mut probs = [1u16 << (PROB_BITS - 1); 258];
    let mut out: Vec<u8> = Vec::with_capacity(out_size.min(1 << 20));
    let (mut m, mut c, mut j) = (0usize, 0usize, 0usize);
    let mut prev = 0u8;

    while out.len() < out_size {
        let b = *main.get(m).ok_or(Error::Truncated)?;
        m += 1;
        out.push(b);
        if !is_branch(prev, b) {
            prev = b;
            continue;
        }
        if out.len() == out_size {
            // A branch opcode in the last byte has no operand to restore.
            break;
        }
        let slot = match b {
            0xE8 => usize::from(prev),
            0xE9 => 256,
            _ => 257,
        };
        if !rc.bit(&mut probs[slot])? {
            prev = b;
            continue;
        }
        let (stream, at) = if b == 0xE8 {
            (call, &mut c)
        } else {
            (jump, &mut j)
        };
        let bytes = stream.get(*at..*at + 4).ok_or(Error::Truncated)?;
        *at += 4;
        // `bytes` is exactly four: `get(at..at + 4)` above returned it.
        let target = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        // Relative to the end of the four operand bytes.
        let relative = target.wrapping_sub((out.len() as u32).wrapping_add(4));
        let operand = relative.to_le_bytes();
        let room = (out_size - out.len()).min(4);
        out.extend_from_slice(&operand[..room]);
        prev = operand[3];
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The range coder's five opening bytes for "every decision is 0": a
    /// zero, then a code of zero, which is below every bound.
    const NO_CONVERSIONS: [u8; 9] = [0, 0, 0, 0, 0, 0, 0, 0, 0];

    #[test]
    fn with_no_conversions_the_main_stream_is_the_output() {
        let code = [0x55, 0xE8, 0x10, 0x20, 0x30, 0x40, 0x0F, 0x85, 0xC3];
        assert_eq!(
            decode(&code, &[], &[], &NO_CONVERSIONS, code.len()),
            Ok(code.to_vec())
        );
    }

    #[test]
    fn the_range_coder_s_first_byte_is_checked() {
        assert_eq!(
            decode(b"\x90", &[], &[], &[1, 0, 0, 0, 0], 1),
            Err(Error::BadRangeStart)
        );
        assert_eq!(
            decode(b"\x90", &[], &[], &[0, 0xFF, 0xFF, 0xFF, 0xFF], 1),
            Err(Error::BadRangeStart)
        );
        assert_eq!(decode(b"\x90", &[], &[], &[0, 0], 1), Err(Error::Truncated));
    }

    #[test]
    fn a_short_stream_is_truncated_rather_than_padded() {
        assert_eq!(
            decode(b"\x90\x90", &[], &[], &NO_CONVERSIONS, 3),
            Err(Error::Truncated)
        );
        // A decision of 1 (a code at the top of the range) wants a call
        // target the call stream does not have.
        let rc = [0, 0xFF, 0xFF, 0xFF, 0xFE];
        assert_eq!(decode(b"\x90\xE8", &[], &[], &rc, 6), Err(Error::Truncated));
    }

    #[test]
    fn hostile_streams_answer_rather_than_panic() {
        let mut state = 0x2545_F491u32;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state
        };
        for _ in 0..2000 {
            let stream = |next: &mut dyn FnMut() -> u32, n: u32| -> Vec<u8> {
                (0..next() % n).map(|_| next() as u8).collect()
            };
            let main: Vec<u8> = (0..next() % 64)
                .map(|_| [0xE8, 0xE9, 0x0F, 0x85, next() as u8][(next() % 5) as usize])
                .collect();
            let call = stream(&mut next, 24);
            let jump = stream(&mut next, 24);
            let mut rc = stream(&mut next, 16);
            if let Some(first) = rc.first_mut() {
                *first = 0;
            }
            let size = (next() % 80) as usize;
            if let Ok(out) = decode(&main, &call, &jump, &rc, size) {
                assert_eq!(out.len(), size);
            }
        }
    }
}
