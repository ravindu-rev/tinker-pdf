//! The two bit orders Zstandard reads in.
//!
//! Every entropy-coded stream in a frame — Huffman literals, the sequences,
//! the FSE-coded Huffman weights — is written forwards and read **backwards**
//! (RFC 8878 §4.1): the encoder ends it with a single `1` bit and zero to
//! seven `0`s of padding, so the decoder starts at the last byte, skips the
//! padding and the mark, and reads towards the first byte. The one thing read
//! forwards is an FSE table description (§4.1.1), little-endian, lowest bit
//! first.

/// A backward bitstream. Bits are counted from the least significant bit of
/// the first byte; `left` is how many have not been read, and each read takes
/// the highest of those — the stream as one little-endian number, consumed
/// from the top.
pub(super) struct Backward<'a> {
    data: &'a [u8],
    /// Bits not yet read. Negative once a read has gone past the first byte,
    /// which is how "more bits than the stream holds" is detected: the reads
    /// themselves return zeros there, as §4.2.2.2 says to assume, and the
    /// caller asks [`Backward::overflowed`].
    left: i64,
}

impl<'a> Backward<'a> {
    /// `None` for an empty stream or one whose last byte is zero, which has
    /// no end mark and so is not a stream (§4.1, §4.2.2).
    pub(super) fn new(data: &'a [u8]) -> Option<Self> {
        let (&last, _) = data.split_last()?;
        if last == 0 {
            return None;
        }
        // The mark is the highest set bit of the last byte; the bits below
        // it are the first ones read.
        let mark = 7 - i64::from(last.leading_zeros());
        let whole = i64::try_from(data.len() - 1).ok()?.checked_mul(8)?;
        Some(Backward {
            data,
            left: whole + mark,
        })
    }

    /// The next `n` bits without reading them, `n` at most 56. Bits past the
    /// start of the stream read as zero.
    pub(super) fn peek(&self, n: u32) -> u64 {
        debug_assert!(n <= 56);
        if n == 0 || self.left <= 0 {
            return 0;
        }
        let start = self.left - i64::from(n);
        if start >= 0 {
            // `start / 8` is a byte inside `data`: `left` never exceeds the
            // stream's length in bits.
            let byte = (start / 8) as usize;
            let shift = (start % 8) as u32;
            (load(self.data, byte) >> shift) & mask(n)
        } else {
            // Fewer than `n` bits remain: they are the top of the value, and
            // the zeros past the start fill its bottom.
            let have = self.left as u32;
            (load(self.data, 0) & mask(have)) << (n - have)
        }
    }

    /// Reads `n` bits, `n` at most 56.
    pub(super) fn read(&mut self, n: u32) -> u64 {
        let value = self.peek(n);
        self.left -= i64::from(n);
        value
    }

    /// Discards `n` bits already looked at with [`Backward::peek`].
    pub(super) fn skip(&mut self, n: u32) {
        self.left -= i64::from(n);
    }

    /// More bits have been read than the stream holds.
    pub(super) fn overflowed(&self) -> bool {
        self.left < 0
    }

    /// Every bit has been read and none past the start: what a stream that
    /// decoded correctly ends at (§4.2.2, §3.1.1.3.2.1).
    pub(super) fn finished(&self) -> bool {
        self.left == 0
    }
}

/// A forward little-endian bitstream, for the FSE table description. Reads
/// past the end return zeros; [`Forward::bytes_used`] says how far it went,
/// and the caller refuses a description that went further than it had.
pub(super) struct Forward<'a> {
    data: &'a [u8],
    at: u64,
}

impl<'a> Forward<'a> {
    pub(super) fn new(data: &'a [u8]) -> Self {
        Forward { data, at: 0 }
    }

    /// The next `n` bits, lowest first, without reading them; `n` at most 32.
    pub(super) fn peek(&self, n: u32) -> u32 {
        debug_assert!(n <= 32);
        let byte = usize::try_from(self.at / 8).unwrap_or(usize::MAX);
        let shift = (self.at % 8) as u32;
        ((load(self.data, byte) >> shift) & mask(n)) as u32
    }

    pub(super) fn skip(&mut self, n: u32) {
        self.at += u64::from(n);
    }

    pub(super) fn read(&mut self, n: u32) -> u32 {
        let value = self.peek(n);
        self.skip(n);
        value
    }

    /// Whole bytes the reads so far have touched.
    pub(super) fn bytes_used(&self) -> u64 {
        self.at.div_ceil(8)
    }
}

/// Eight bytes little-endian from `at`, zeros past the end.
fn load(data: &[u8], at: usize) -> u64 {
    let mut buf = [0u8; 8];
    for (slot, &b) in buf.iter_mut().zip(data.get(at..).unwrap_or(&[])) {
        *slot = b;
    }
    u64::from_le_bytes(buf)
}

fn mask(n: u32) -> u64 {
    if n >= 64 {
        u64::MAX
    } else {
        (1u64 << n) - 1
    }
}
