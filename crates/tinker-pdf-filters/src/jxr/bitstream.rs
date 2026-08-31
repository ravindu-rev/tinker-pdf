//! The codestream bit reader of ITU-T T.832 clause 8.
//!
//! T.832 5.2 defines `u(n)` as an unsigned integer read most-significant bit
//! first, and the format has **no byte stuffing**: unlike T.81 there is no
//! escape for a `0xFF`, so a reader is a plain MSB-first accumulator over the
//! byte slice. That is the whole of it, and the reason this file is small.
//!
//! Every read is checked. A read past the end is [`JxrError::Truncated`]
//! rather than a clamp to zero, because clause 8's parse is a state machine
//! whose next branch depends on the bits just read — a reader that invented
//! zeros would keep parsing a structure the file does not contain, and the
//! refusal is the feature (ruling 1).

#![deny(clippy::float_arithmetic)]

use super::JxrError;

/// An MSB-first bit reader over a codestream slice.
///
/// The position is held in bits rather than in a (byte, bit) pair so that
/// `POS_SEEK` (8.7.1, which seeks to a byte offset from the start of the
/// coded image data) and `IS_BYTE_ALIGNED` (8.4.21) are both one expression.
pub(crate) struct BitReader<'a> {
    data: &'a [u8],
    /// Bits consumed from the start of `data`. Never exceeds `data.len() * 8`
    /// — every advance is checked against that ceiling first.
    pos: u64,
}

impl<'a> BitReader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// Total bits in the slice. `data.len()` is a `usize` from a slice that
    /// exists, so the multiply cannot overflow a `u64` on any target this
    /// engine builds for.
    fn total_bits(&self) -> u64 {
        (self.data.len() as u64) * 8
    }

    pub(crate) fn bits_left(&self) -> u64 {
        self.total_bits().saturating_sub(self.pos)
    }

    /// 8.4.21's `IS_BYTE_ALIGNED( )`.
    pub(crate) fn is_byte_aligned(&self) -> bool {
        self.pos % 8 == 0
    }

    /// Discards bits up to the next byte boundary, checking that each one is
    /// the zero 8.4.21 requires. A non-zero padding bit is *tolerated* — the
    /// clause says the value 1 is reserved rather than that a file carrying
    /// one is unreadable — and reported so the caller can warn (ruling 10).
    pub(crate) fn align_to_byte(&mut self) -> Result<bool, JxrError> {
        let mut clean = true;
        while !self.is_byte_aligned() {
            if self.read(1)? != 0 {
                clean = false;
            }
        }
        Ok(clean)
    }

    /// The slice being read, with the reader's own lifetime rather than the
    /// borrow's.
    ///
    /// Frequency mode needs a second reader over the same codestream — 8.7.9's
    /// FLEXBITS packet is a byte range of its own, stepped alongside the
    /// HIGHPASS packet — and returning `&'a [u8]` rather than `&'_ [u8]` is
    /// what lets the caller hold both without the first borrow outliving the
    /// call.
    pub(crate) fn data(&self) -> &'a [u8] {
        self.data
    }

    /// Byte offset of the next bit. Only meaningful when byte-aligned; used
    /// by the index-table seek, which 8.5.3 defines in bytes.
    pub(crate) fn byte_pos(&self) -> u64 {
        self.pos / 8
    }

    /// 8.7.1's `POS_SEEK( )`: absolute byte offset from the start of the
    /// coded image data.
    pub(crate) fn seek_byte(&mut self, offset: u64) -> Result<(), JxrError> {
        let bit = offset.checked_mul(8).ok_or(JxrError::Truncated)?;
        if bit > self.total_bits() {
            return Err(JxrError::Truncated);
        }
        self.pos = bit;
        Ok(())
    }

    /// `u(n)` for `n` in 0..=64.
    ///
    /// The accumulator is a `u64` and `n` is checked against 64 before the
    /// loop, so the shift below can never be the undefined-in-C, panicking-in-
    /// debug-Rust shift by a width. `n == 0` returns 0 and consumes nothing,
    /// which is what the `RESERVED_L u(15)`-style fields need when a caller
    /// computes a width.
    pub(crate) fn read(&mut self, n: u32) -> Result<u64, JxrError> {
        if n > 64 {
            // Not reachable from any file: every call site passes a literal
            // or a width this module derived. Refused rather than masked so
            // a future caller cannot get a silently truncated field.
            return Err(JxrError::Truncated);
        }
        if u64::from(n) > self.bits_left() {
            return Err(JxrError::Truncated);
        }
        let mut value: u64 = 0;
        for _ in 0..n {
            let byte = self
                .data
                .get((self.pos / 8) as usize)
                .copied()
                .ok_or(JxrError::Truncated)?;
            let shift = 7 - (self.pos % 8);
            let bit = (byte >> shift) & 1;
            value = (value << 1) | u64::from(bit);
            self.pos += 1;
        }
        Ok(value)
    }

    /// `u(n)` narrowed to `u32`, for the many fields clause 8 declares at 32
    /// bits or fewer. The `n <= 32` check keeps the cast lossless.
    pub(crate) fn read_u32(&mut self, n: u32) -> Result<u32, JxrError> {
        if n > 32 {
            return Err(JxrError::Truncated);
        }
        // The width check above makes this cast exact.
        Ok(self.read(n)? as u32)
    }

    /// One bit as a `bool`, which is how clause 8 spells every `_FLAG`.
    pub(crate) fn flag(&mut self) -> Result<bool, JxrError> {
        Ok(self.read(1)? != 0)
    }

    /// Reads `n` bits and interprets them as a two's complement signed value.
    /// 8.4.15's `EXP_BIAS` is the only `i(n)` in clause 8.
    pub(crate) fn read_signed(&mut self, n: u32) -> Result<i64, JxrError> {
        if n == 0 || n > 64 {
            return Err(JxrError::Truncated);
        }
        let raw = self.read(n)?;
        // Sign-extend from bit n-1. `n >= 1` so the shift is in range, and
        // `n <= 64` keeps the mask well-defined.
        if n == 64 {
            return Ok(raw as i64);
        }
        let sign = 1u64 << (n - 1);
        if raw & sign != 0 {
            // Two's complement: subtract 2^n. `n < 64` here.
            Ok((raw as i64) - (1i64 << n))
        } else {
            Ok(raw as i64)
        }
    }

    /// 8.2.4's `VLW_ESC( )`: a variable-length unsigned value whose first
    /// byte selects the width. `0xFD`, `0xFE` and `0xFF` are "escape mode",
    /// which the clause defines as the value 0 rather than as an error.
    pub(crate) fn vlw_esc(&mut self) -> Result<u64, JxrError> {
        let first = self.read(8)?;
        if first < 0xFB {
            let second = self.read(8)?;
            // 8.2.4.1: iValue = FIRST_BYTE * 256 + SECOND_BYTE.
            Ok(first * 256 + second)
        } else if first == 0xFB {
            self.read(32)
        } else if first == 0xFC {
            self.read(64)
        } else {
            // 8.2.4.1: FIRST_BYTE is 0xFD, 0xFE or 0xFF — escape mode.
            Ok(0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_msb_first() {
        let mut r = BitReader::new(&[0b1011_0010, 0b0100_0001]);
        assert_eq!(r.read(1), Ok(1));
        assert_eq!(r.read(3), Ok(0b011));
        assert_eq!(r.read(4), Ok(0b0010));
        assert_eq!(r.read(8), Ok(0b0100_0001));
        assert_eq!(r.read(1), Err(JxrError::Truncated));
    }

    #[test]
    fn a_read_past_the_end_refuses_rather_than_inventing_zeros() {
        let mut r = BitReader::new(&[0xFF]);
        assert_eq!(r.read(9), Err(JxrError::Truncated));
        // And the position did not move, so a caller that recovers sees the
        // same state it had.
        assert_eq!(r.bits_left(), 8);
    }

    #[test]
    fn alignment_reports_a_non_zero_padding_bit() {
        // 8.4.21: the padding bit shall be 0; a 1 is reserved. One bit read,
        // then seven bits of padding, the first of which is set.
        let mut r = BitReader::new(&[0b0100_0000]);
        assert_eq!(r.read(1), Ok(0));
        assert_eq!(r.align_to_byte(), Ok(false));
        assert!(r.is_byte_aligned());
    }

    #[test]
    fn vlw_esc_covers_all_four_of_8_2_4s_branches() {
        // Two-byte form: 0x01 0x02 -> 258.
        assert_eq!(BitReader::new(&[0x01, 0x02]).vlw_esc(), Ok(258));
        // Four-byte form.
        assert_eq!(
            BitReader::new(&[0xFB, 0x00, 0x00, 0x01, 0x00]).vlw_esc(),
            Ok(256)
        );
        // Eight-byte form.
        assert_eq!(
            BitReader::new(&[0xFC, 0, 0, 0, 0, 0, 0, 0, 7]).vlw_esc(),
            Ok(7)
        );
        // Escape mode is the value 0, not a failure.
        assert_eq!(BitReader::new(&[0xFD]).vlw_esc(), Ok(0));
        assert_eq!(BitReader::new(&[0xFE]).vlw_esc(), Ok(0));
        assert_eq!(BitReader::new(&[0xFF]).vlw_esc(), Ok(0));
    }

    #[test]
    fn signed_reads_are_twos_complement() {
        assert_eq!(BitReader::new(&[0xFF]).read_signed(8), Ok(-1));
        assert_eq!(BitReader::new(&[0x80]).read_signed(8), Ok(-128));
        assert_eq!(BitReader::new(&[0x7F]).read_signed(8), Ok(127));
    }

    #[test]
    fn seek_refuses_past_the_end() {
        let mut r = BitReader::new(&[0, 0, 0, 0]);
        assert_eq!(r.seek_byte(4), Ok(()));
        assert_eq!(r.seek_byte(5), Err(JxrError::Truncated));
        assert_eq!(r.seek_byte(u64::MAX), Err(JxrError::Truncated));
    }
}
