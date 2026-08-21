//! SHA-1 (FIPS 180-4 §6.1), for OCF's font obfuscation key (gap 31,
//! milestone 9).
//!
//! # This is not here as a security primitive
//!
//! SHA-1's collision resistance is broken and nothing in this repository
//! should treat a SHA-1 digest as a commitment. It is here for exactly one
//! reason: EPUB's OCF 3.3 §4.4.3 defines the font obfuscation key as *"the
//! SHA-1 digest of the UTF-8 encoding of the publication's unique
//! identifier"*, and a reading system that wants to de-obfuscate a font has to
//! compute the number the specification names rather than a better one. The
//! obfuscation itself is a XOR over the first kilobyte and protects nothing —
//! it exists so a font's licence terms are not violated by dragging the file
//! out of the archive — so the hash is a **key derivation nobody is relying
//! on**, and saying that plainly here is better than a caller discovering it
//! from the algorithm's name.
//!
//! [`crate::sha2`] is what the standard security handler uses, and no caller
//! should reach for this one instead.
//!
//! # Two implementations, and why
//!
//! `tinker-pdf-filters`' CRC-32 records the rule this file follows: **a hash
//! written wrong is self-consistently wrong.** A reflected polynomial, a
//! swapped word order or a rotation by the wrong amount produces plausible
//! digests for every input, and no amount of testing one implementation
//! against itself finds it. So the tests below carry a second SHA-1 written
//! from FIPS 180-4's **other** method — §6.1.3's sixteen-word circular buffer
//! rather than §6.1.2's eighty-word expanded schedule, with the round
//! functions in their alternative boolean forms and the padding built as a
//! whole message rather than streamed — and the two are asserted equal over
//! **every** length from nothing to two full blocks. Published vectors pin
//! where both of them sit; the second implementation is what says they are not
//! both wrong in the same direction.

/// The four round constants, FIPS 180-4 §4.2.1.
const K: [u32; 4] = [0x5A82_7999, 0x6ED9_EBA1, 0x8F1B_BCDC, 0xCA62_C1D6];

/// SHA-1 streaming state.
#[derive(Clone, Debug)]
pub struct Sha1 {
    state: [u32; 5],
    buffer: [u8; 64],
    buffered: usize,
    length_bits: u64,
}

impl Default for Sha1 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha1 {
    /// The initial state, FIPS 180-4 §5.3.1.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: [
                0x6745_2301,
                0xEFCD_AB89,
                0x98BA_DCFE,
                0x1032_5476,
                0xC3D2_E1F0,
            ],
            buffer: [0u8; 64],
            buffered: 0,
            length_bits: 0,
        }
    }

    /// Absorbs more of the message.
    pub fn update(&mut self, mut data: &[u8]) {
        self.length_bits = self.length_bits.wrapping_add((data.len() as u64) << 3);

        if self.buffered > 0 {
            let take = (64 - self.buffered).min(data.len());
            if let (Some(dst), Some(src)) = (
                self.buffer.get_mut(self.buffered..self.buffered + take),
                data.get(..take),
            ) {
                dst.copy_from_slice(src);
            }
            self.buffered += take;
            data = data.get(take..).unwrap_or(&[]);
            if self.buffered == 64 {
                let block = self.buffer;
                self.compress(&block);
                self.buffered = 0;
            } else {
                // `data` is exhausted; see the note in `md5::Md5::update`.
                return;
            }
        }

        let mut chunks = data.chunks_exact(64);
        for chunk in &mut chunks {
            let mut block = [0u8; 64];
            block.copy_from_slice(chunk);
            self.compress(&block);
        }

        let rest = chunks.remainder();
        if let Some(dst) = self.buffer.get_mut(..rest.len()) {
            dst.copy_from_slice(rest);
        }
        self.buffered = rest.len();
    }

    /// §5.1.1's padding, then the digest, big-endian.
    #[must_use]
    pub fn finish(mut self) -> [u8; 20] {
        let bits = self.length_bits;
        self.pad(&[0x80]);
        while self.buffered != 56 {
            self.pad(&[0x00]);
        }
        self.pad(&bits.to_be_bytes());

        let mut out = [0u8; 20];
        for (chunk, word) in out.chunks_exact_mut(4).zip(self.state.iter()) {
            chunk.copy_from_slice(&word.to_be_bytes());
        }
        out
    }

    /// Feeds padding without letting it count towards the length.
    fn pad(&mut self, data: &[u8]) {
        let saved = self.length_bits;
        self.update(data);
        self.length_bits = saved;
    }

    /// §6.1.2: the eighty-word message schedule, expanded up front.
    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 80];
        for (i, chunk) in block.chunks_exact(4).enumerate() {
            let mut bytes = [0u8; 4];
            bytes.copy_from_slice(chunk);
            if let Some(slot) = w.get_mut(i) {
                *slot = u32::from_be_bytes(bytes);
            }
        }
        for i in 16..80 {
            // The rotate is SHA-1's whole difference from SHA-0 and is the one
            // line a wrong implementation most often omits — which is exactly
            // the kind of self-consistent error the second implementation in
            // the tests below exists to catch.
            let x = w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16];
            w[i] = x.rotate_left(1);
        }

        let [mut a, mut b, mut c, mut d, mut e] = self.state;
        for (i, word) in w.iter().enumerate() {
            let (f, k) = match i / 20 {
                0 => ((b & c) | (!b & d), K[0]),
                1 => (b ^ c ^ d, K[1]),
                2 => ((b & c) | (b & d) | (c & d), K[2]),
                _ => (b ^ c ^ d, K[3]),
            };
            let t = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }

        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
    }
}

/// SHA-1 of one slice.
#[must_use]
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut hash = Sha1::new();
    hash.update(data);
    hash.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same digest computed the other way round.
    ///
    /// This is not [`sha1`] rearranged. Three things are done differently and
    /// each of them is a place the first implementation could be
    /// self-consistently wrong:
    ///
    /// * **The schedule is §6.1.3's sixteen-word circular buffer** rather than
    ///   §6.1.2's eighty-word array, so the indices `w[i-3] ^ w[i-8] ^ …`
    ///   are computed modulo sixteen from a different expression.
    /// * **The round functions are their alternative boolean forms** —
    ///   `d ^ (b & (c ^ d))` for `Ch` and `(b & c) | (d & (b | c))` for `Maj` —
    ///   which are equal to the first implementation's only if both are right.
    /// * **The message is padded whole** into a `Vec` before any compression,
    ///   rather than streamed through a buffer, so a length counted or a
    ///   boundary handled wrongly in [`Sha1::update`] does not reach here.
    ///
    /// A digest that agrees with this one over every length up to two blocks
    /// is a digest whose schedule, rounds, padding and length encoding all
    /// agree with a second reading of the standard.
    fn sha1_circular(data: &[u8]) -> [u8; 20] {
        let bits = (data.len() as u64) << 3;
        let mut message = data.to_vec();
        message.push(0x80);
        while message.len() % 64 != 56 {
            message.push(0x00);
        }
        message.extend_from_slice(&bits.to_be_bytes());

        let mut h: [u32; 5] = [
            0x6745_2301,
            0xEFCD_AB89,
            0x98BA_DCFE,
            0x1032_5476,
            0xC3D2_E1F0,
        ];
        for block in message.chunks_exact(64) {
            let mut w = [0u32; 16];
            for (i, chunk) in block.chunks_exact(4).enumerate() {
                let mut bytes = [0u8; 4];
                bytes.copy_from_slice(chunk);
                w[i] = u32::from_be_bytes(bytes);
            }

            let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
            for t in 0..80usize {
                let s = t & 0x0F;
                if t >= 16 {
                    let mixed = w[(s + 13) & 0x0F] ^ w[(s + 8) & 0x0F] ^ w[(s + 2) & 0x0F] ^ w[s];
                    w[s] = mixed.rotate_left(1);
                }
                let (f, k) = match t {
                    0..=19 => (d ^ (b & (c ^ d)), 0x5A82_7999u32),
                    20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                    40..=59 => ((b & c) | (d & (b | c)), 0x8F1B_BCDC),
                    _ => (b ^ c ^ d, 0xCA62_C1D6),
                };
                let temp = a
                    .rotate_left(5)
                    .wrapping_add(f)
                    .wrapping_add(e)
                    .wrapping_add(k)
                    .wrapping_add(w[s]);
                e = d;
                d = c;
                c = b.rotate_left(30);
                b = a;
                a = temp;
            }

            h[0] = h[0].wrapping_add(a);
            h[1] = h[1].wrapping_add(b);
            h[2] = h[2].wrapping_add(c);
            h[3] = h[3].wrapping_add(d);
            h[4] = h[4].wrapping_add(e);
        }

        let mut out = [0u8; 20];
        for (chunk, word) in out.chunks_exact_mut(4).zip(h.iter()) {
            chunk.copy_from_slice(&word.to_be_bytes());
        }
        out
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// FIPS 180-4's own examples and the two vectors every SHA-1 catalogue
    /// publishes beside them.
    ///
    /// The empty string is the one that catches a missing padding block on its
    /// own: with no message at all there is still a whole block to compress,
    /// made entirely of the `0x80`, the zeros and the length. A build that
    /// compressed only what the caller supplied would return the initial state
    /// instead, which is a plausible-looking twenty bytes.
    ///
    /// The 448-bit message is the one that straddles the padding boundary:
    /// fifty-six bytes leave exactly no room for the length, so it takes a
    /// second block. The 896-bit one crosses two.
    #[test]
    fn sha1_matches_the_published_vectors() {
        assert_eq!(hex(&sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(
            hex(&sha1(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            hex(&sha1(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
        assert_eq!(
            hex(&sha1(b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu")),
            "a49b2446a02c645bf419f995b67091253a04a259"
        );
    }

    /// The million-`a` vector, which is the only published one that exercises
    /// the streaming path over more blocks than a single `update` would ever
    /// carry in this repository's use of it.
    #[test]
    fn sha1_matches_the_million_a_vector() {
        let mut hash = Sha1::new();
        for _ in 0..1000 {
            hash.update(&[b'a'; 1000]);
        }
        assert_eq!(
            hex(&hash.finish()),
            "34aa973cd4c4daa4f61eeb2bdbad27316534016f"
        );
    }

    /// Two formulations, one answer, at **every** length from nothing to two
    /// full blocks.
    ///
    /// Every length rather than a spread, because the interesting ones are the
    /// ones nobody would choose: 55 and 56 bytes are either side of the point
    /// where the length no longer fits in the first block, 63 and 64 either
    /// side of the block boundary itself, and 119 and 120 the same two facts
    /// one block later. A rotation dropped from the schedule — SHA-0 rather
    /// than SHA-1 — is caught at every length above sixteen words; a padding
    /// error is caught at exactly those six.
    #[test]
    fn the_two_implementations_agree_over_every_length_to_two_blocks() {
        let long: Vec<u8> = (0..128u32)
            .map(|i| (i.wrapping_mul(97).wrapping_add(13) % 251) as u8)
            .collect();
        for len in 0..=128usize {
            let message = &long[..len];
            assert_eq!(
                sha1(message),
                sha1_circular(message),
                "the two implementations disagree at length {len}"
            );
        }
    }

    /// The second implementation is pinned too, so that "they agree" cannot be
    /// two copies of the same mistake.
    ///
    /// Without this the agreement test would still pass if both were written
    /// from the same wrong memory of the standard; with it, the reference is
    /// anchored to a value published outside this repository and the agreement
    /// test carries the rest.
    #[test]
    fn the_reference_implementation_is_pinned_to_a_published_vector() {
        assert_eq!(
            hex(&sha1_circular(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            hex(&sha1_circular(b"")),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
    }

    /// A digest resumed across a call boundary equals the one-shot, at every
    /// split rather than a convenient one.
    ///
    /// The buffer in [`Sha1::update`] is the only mutable state a caller can
    /// reach, and the split that breaks a wrong one is the split nobody picks:
    /// one byte before the block boundary, and one after.
    #[test]
    fn a_resumed_digest_equals_the_one_shot() {
        let message: Vec<u8> = (0..200u32).map(|i| (i % 253) as u8).collect();
        let whole = sha1(&message);
        for split in 0..=message.len() {
            let mut hash = Sha1::new();
            hash.update(&message[..split]);
            hash.update(&message[split..]);
            assert_eq!(hash.finish(), whole, "split at {split}");
        }
    }
}
