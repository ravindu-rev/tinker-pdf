//! CRC-32, the checksum ZIP and PNG both carry.
//!
//! One function, and both consumers want the same one. APPNOTE 4.4.7 puts a
//! CRC-32 on every ZIP entry — in the local header, in the central directory
//! and, when general-purpose bit 3 is set, in the data descriptor — and PNG
//! 5.3 puts one on every chunk. Both mean the checksum IEEE 802.3 and ITU-T
//! V.42 define: the polynomial `0x04C1_1DB7`, fed least-significant-bit-first,
//! which is the reflected constant [`POLY`] below; a register that starts at
//! all ones; and a result that is complemented on the way out.
//!
//! **`adler32` is not this.** It appears twice in this crate — in `inflate.rs`
//! and in `deflate.rs` — and it is a different polynomial answering a
//! different question, present only because RFC 1950 puts it in the zlib
//! wrapper. Nothing that owes a CRC may spend an Adler.
//!
//! # Why the checksum is compared rather than stored
//!
//! Gap 29's design turns on passing image bytes through untouched: a JPEG in a
//! comic archive is copied verbatim into a PDF stream, so **no decoder in this
//! engine looks at those bytes again until a page is rendered**, and by then
//! there is nothing left to compare them against. The archive's own CRC is the
//! only integrity evidence that will ever exist for them. Gap 16 found the
//! opposite mistake in this repository — `ccitt_decode`'s warnings discarded at
//! the call site, so every leniency it took was invisible — and a CRC that is
//! read and not compared is that same mistake with a checksum.
//!
//! # Why a table, and why 1 KiB of it
//!
//! Bitwise costs eight tests and eight shifts per byte and needs no table.
//! Table-driven costs one lookup, one xor and one shift, and 1 KiB of
//! `.rodata`. The kilobyte is spent because of where this sits in the design
//! above: with the image bytes passed through rather than decoded, **the CRC is
//! the only pass anything makes over an archive's bytes**, and a plausible
//! comic is a few hundred megabytes of them. A per-byte cost eight times what
//! it needs to be would be most of the cost of opening the document, which is
//! not a trade worth a kilobyte.
//!
//! The table is *derived* from the polynomial in a `const fn`, evaluated at
//! compile time, rather than transcribed from a published listing. That is the
//! same discipline the JPEG 2000 fixed point took with Table F.4 — recompute
//! rather than copy twice — and it costs nothing here: there is no runtime
//! initialisation, no `OnceLock`, no `unsafe` (the crate forbids it), and no
//! 256 lines of hex for a reader to audit. What pins the result is the three
//! published values in the tests and a bitwise reference written the *other*
//! way round, most-significant-bit-first over the unreflected polynomial, so
//! agreement between them is agreement between two formulations rather than an
//! expression agreeing with itself.

/// `0x04C1_1DB7` with its bits reversed, which is what feeding a byte
/// least-significant-bit-first amounts to.
const POLY: u32 = 0xEDB8_8320;

/// One entry per leading byte: eight steps of the bit loop, memoised.
///
/// `static` rather than `const` on purpose — a `const` is substituted at each
/// use site, and a kilobyte is worth naming a single copy for.
static TABLE: [u32; 256] = table();

const fn table() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut n = 0usize;
    while n < 256 {
        let mut c = n as u32;
        let mut bit = 0;
        while bit < 8 {
            c = if c & 1 == 0 { c >> 1 } else { POLY ^ (c >> 1) };
            bit += 1;
        }
        t[n] = c;
        n += 1;
    }
    t
}

/// A CRC-32 in progress.
///
/// Resumable because both consumers need it to be: a ZIP entry is checksummed
/// as it is inflated rather than after, so a bomb caught by the output ceiling
/// has still been charged for what it produced; and a PNG chunk's CRC covers
/// its type *and* its data, which are two slices that are never adjacent in
/// one buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Crc32 {
    /// The register, held un-complemented. [`Crc32::finish`] applies the final
    /// xor, so reading a partial value costs nothing and the state stays
    /// composable.
    state: u32,
}

impl Crc32 {
    /// A fresh register: all ones, per the definition.
    #[must_use]
    pub const fn new() -> Self {
        Self { state: !0 }
    }

    /// Folds `data` in. Any number of calls; only the concatenation matters.
    pub fn update(&mut self, data: &[u8]) {
        let mut c = self.state;
        for &b in data {
            // The mask is what makes this total: the index is 0..=255 and the
            // table has 256 entries, so there is no input that can reach past
            // it (ruling 1).
            c = TABLE[((c ^ u32::from(b)) & 0xff) as usize] ^ (c >> 8);
        }
        self.state = c;
    }

    /// The checksum of everything folded in so far.
    ///
    /// Takes `&self`, so a caller may read a running value and keep going —
    /// which is what a ZIP entry checked against its data descriptor needs.
    #[must_use]
    pub const fn finish(&self) -> u32 {
        !self.state
    }
}

impl Default for Crc32 {
    fn default() -> Self {
        Self::new()
    }
}

/// CRC-32 of one slice.
#[must_use]
pub fn crc32(data: &[u8]) -> u32 {
    let mut c = Crc32::new();
    c.update(data);
    c.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same checksum computed the other way round: bits taken
    /// most-significant-first against the *unreflected* `0x04C1_1DB7`, with
    /// each input byte and the final register bit-reversed to compensate.
    ///
    /// This is not [`crc32`] rearranged. It is the definition's other standard
    /// form — the one a hardware description is written in — and the point of
    /// having it is that a reflection error produces a self-consistent wrong
    /// answer, so no amount of testing `crc32` against itself can see one.
    fn crc32_msb_first(data: &[u8]) -> u32 {
        let mut r: u32 = 0xFFFF_FFFF;
        for &b in data {
            r ^= u32::from(b.reverse_bits()) << 24;
            for _ in 0..8 {
                r = if r & 0x8000_0000 == 0 {
                    r << 1
                } else {
                    (r << 1) ^ 0x04C1_1DB7
                };
            }
        }
        r.reverse_bits() ^ 0xFFFF_FFFF
    }

    /// The three the plan pins, each verifiable outside this repository.
    ///
    /// `""` is the one that catches a missing xor on its own: the register
    /// starts at all ones and is complemented on the way out, so the empty
    /// checksum is zero *because the two cancel*. Drop either one alone and it
    /// becomes `0xFFFF_FFFF`. Drop both and it is zero again — which is why
    /// `123456789`, the check value every CRC catalogue publishes, is here
    /// beside it rather than instead of it.
    ///
    /// `IEND` is the type field of the chunk every PNG in the world ends with,
    /// so the third vector is also the first fixture the milestone-3 decoder
    /// will meet.
    #[test]
    fn crc32_matches_the_three_published_values() {
        assert_eq!(crc32(b""), 0x0000_0000);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
    }

    /// A PNG's trailing chunk, whole: length 0, type `IEND`, then the CRC —
    /// which covers the type and the (empty) data and not the length.
    #[test]
    fn the_iend_vector_is_the_chunk_a_png_actually_ends_with() {
        let mut c = Crc32::new();
        c.update(b"IEND");
        c.update(b"");
        assert_eq!(
            c.finish().to_be_bytes(),
            [0xAE, 0x42, 0x60, 0x82],
            "the four bytes that close every PNG"
        );
    }

    /// Two formulations, one answer, over every single byte and a spread of
    /// lengths. A polynomial reflected the wrong way still produces plausible
    /// checksums; it does not produce *these* ones.
    #[test]
    fn the_table_agrees_with_a_bitwise_reference_written_the_other_way() {
        for n in 0..=255u8 {
            assert_eq!(crc32(&[n]), crc32_msb_first(&[n]), "single byte {n}");
        }
        let long: Vec<u8> = (0..1000u32)
            .map(|i| (i.wrapping_mul(31) % 251) as u8)
            .collect();
        for len in [0usize, 1, 2, 3, 4, 7, 8, 9, 63, 64, 255, 256, 1000] {
            let s = &long[..len];
            assert_eq!(crc32(s), crc32_msb_first(s), "length {len}");
        }
    }

    /// Resumability is the property the ZIP reader leans on, so it is asserted
    /// at *every* split rather than at a convenient one: an update that lost
    /// or double-counted the register across a call boundary would still agree
    /// with the one-shot at the two ends.
    #[test]
    fn a_resumed_crc_equals_the_one_shot() {
        let data = b"PK\x03\x04 a comic archive entry, split every which way";
        let whole = crc32(data);
        for split in 0..=data.len() {
            let mut c = Crc32::new();
            c.update(&data[..split]);
            c.update(&data[split..]);
            assert_eq!(c.finish(), whole, "split at {split}");
        }
        // And in three pieces, since two could be a special case of one.
        let mut c = Crc32::new();
        c.update(&data[..7]);
        c.update(&data[7..19]);
        c.update(&data[19..]);
        assert_eq!(c.finish(), whole);
    }

    #[test]
    fn a_fresh_crc_is_the_empty_checksum() {
        assert_eq!(Crc32::new().finish(), 0);
        assert_eq!(Crc32::default().finish(), crc32(b""));
    }

    /// A zero byte is not a no-op, which is the cheapest way to see that the
    /// register is not simply being carried through.
    #[test]
    fn a_run_of_zero_bytes_still_moves_the_register() {
        assert_eq!(crc32(&[0x00]), 0xD202_EF8D);
        assert_eq!(crc32(&[0x00, 0x00]), 0x41D9_12FF);
        assert_ne!(crc32(&[0x00; 16]), crc32(&[0x00; 15]));
    }
}
