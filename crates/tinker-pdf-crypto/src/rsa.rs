//! RSASSA-PKCS1-v1_5 signature **verification** (RFC 8017 §8.2.2).
//!
//! Verification and nothing else. There is no key generation, no signing, no
//! private-key operation and no key-file parsing here — a PDF signature
//! arrives as a certificate's public key and a blob of bytes, and this module
//! answers one question about them. `tinker-pdf-pki` will do the DER; this
//! module takes `(n, e)` as big-endian integers and a digest that
//! [`crate::sha1`] or [`crate::sha2`] already computed.
//!
//! # The whole encoding is compared, and that is the point
//!
//! RFC 8017 §8.2.2 step 4 says to build the expected encoded message and
//! compare it with the recovered one. It does **not** say to parse the
//! recovered block, walk past the padding and pull out a digest, and the
//! difference is a real forgery: with a small public exponent an attacker who
//! only has to match a *prefix* can choose the remaining bytes so that the
//! whole block is a perfect cube, and sign without the private key
//! (Bleichenbacher, CRYPTO 2006 rump session; the flaw was found in several
//! shipping implementations). So [`RsaPublicKey::verify_pkcs1_v15`] builds
//!
//! ```text
//! EM = 0x00 || 0x01 || PS || 0x00 || T
//! ```
//!
//! in full — `PS` being `k - tLen - 3` bytes of `0xff`, `T` the DER
//! `DigestInfo` — and compares all `k` bytes. Nothing in this module ever
//! reads a length or an offset out of the recovered block, so there is nothing
//! for a forged block to steer. The CAVP vector set carries 150 published
//! negatives of exactly this shape ("hash moved to left", "00 on end of pad
//! removed"), and the tests below add the cube-root forgery itself.
//!
//! # What it costs
//!
//! A digest algorithm this crate does not implement cannot be verified: the
//! `DigestInfo` prefix is a constant per algorithm and a guessed one would
//! accept the wrong thing. SHA-1, SHA-256, SHA-384 and SHA-512 are the four
//! offered; MD2 and MD5 are not, even though RFC 8017 lists them, because no
//! signature worth checking uses them and a verifier that accepts MD5 is a
//! verifier that accepts a chosen-prefix collision. SHA-224 is absent for the
//! simpler reason that the crate has no SHA-224.
//!
//! A modulus wider than 4096 bits is refused rather than truncated, and so is
//! a signature whose length is not exactly the modulus's.

use crate::bignum::{Modulus, Uint, MAX_LIMBS};
use crate::handler::constant_time_eq;
use crate::{sha1::sha1, sha2};

/// Limbs behind an RSA modulus: 4096 bits, the widest key this verifies.
pub const RSA_LIMBS: usize = MAX_LIMBS;

/// Bytes behind an RSA modulus, and the largest encoded message.
pub const MAX_RSA_BYTES: usize = RSA_LIMBS * 8;

/// A digest algorithm, chosen for its `DigestInfo` prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DigestAlgorithm {
    /// SHA-1. Accepted for legacy signatures and weak: collision resistance is
    /// broken, so a signature over a SHA-1 digest proves the signer signed
    /// *something*, not that they signed this document. Callers are expected
    /// to say so in their verdict.
    Sha1,
    /// SHA-256.
    Sha256,
    /// SHA-384.
    Sha384,
    /// SHA-512.
    Sha512,
}

impl DigestAlgorithm {
    /// The digest's length in bytes.
    #[must_use]
    pub const fn output_len(self) -> usize {
        match self {
            Self::Sha1 => 20,
            Self::Sha256 => 32,
            Self::Sha384 => 48,
            Self::Sha512 => 64,
        }
    }

    /// The DER encoding of `DigestInfo` with an empty digest, which RFC 8017
    /// §9.2 note 1 lists byte for byte for each hash function.
    ///
    /// These are constants rather than a DER encoder because the standard
    /// prints them as constants, and because an encoder able to produce them
    /// is an encoder able to produce variants of them — the `NULL` parameters
    /// omitted, a non-minimal length — which is the ambiguity §9.2 note 1
    /// exists to remove.
    #[must_use]
    pub const fn digest_info_prefix(self) -> &'static [u8] {
        match self {
            Self::Sha1 => &[
                0x30, 0x21, 0x30, 0x09, 0x06, 0x05, 0x2b, 0x0e, 0x03, 0x02, 0x1a, 0x05, 0x00, 0x04,
                0x14,
            ],
            Self::Sha256 => &[
                0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02,
                0x01, 0x05, 0x00, 0x04, 0x20,
            ],
            Self::Sha384 => &[
                0x30, 0x41, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02,
                0x02, 0x05, 0x00, 0x04, 0x30,
            ],
            Self::Sha512 => &[
                0x30, 0x51, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02,
                0x03, 0x05, 0x00, 0x04, 0x40,
            ],
        }
    }

    /// Digests `data` with this algorithm, into a stack buffer.
    #[must_use]
    pub fn digest(self, data: &[u8]) -> DigestValue {
        let mut value = DigestValue {
            bytes: [0u8; 64],
            len: self.output_len(),
        };
        match self {
            Self::Sha1 => value.fill(&sha1(data)),
            Self::Sha256 => value.fill(&sha2::sha256(data)),
            Self::Sha384 => value.fill(&sha2::sha384(data)),
            Self::Sha512 => value.fill(&sha2::sha512(data)),
        }
        value
    }
}

/// A digest, in a buffer wide enough for the longest this crate produces.
///
/// A fixed array rather than a `Vec` for the module's usual reason: nothing on
/// the verification path allocates.
#[derive(Clone, Copy, Debug)]
pub struct DigestValue {
    bytes: [u8; 64],
    len: usize,
}

impl DigestValue {
    fn fill(&mut self, source: &[u8]) {
        if let Some(slot) = self.bytes.get_mut(..source.len()) {
            slot.copy_from_slice(source);
        }
    }

    /// The digest bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.bytes.get(..self.len).unwrap_or(&[])
    }
}

/// Why a key or a signature was refused.
///
/// Typed rather than a bare `false`, because a caller reporting on a signature
/// has to distinguish "this key is not one I can work with" from "this
/// signature does not verify" — the first is a gap in the engine and the
/// second is a fact about the document. Naming the reason leaks nothing: every
/// input here is public, and the module performs no private-key operation for
/// a timing or error oracle to attack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RsaRefusal {
    /// The modulus is wider than 4096 bits.
    ModulusTooWide,
    /// The modulus is even, or below 3. Neither is an RSA modulus.
    ModulusUnusable,
    /// The public exponent is below 3.
    ExponentUnusable,
    /// The signature's length is not the modulus's length. RFC 8017 §8.2.2
    /// step 1.
    SignatureLength {
        /// The modulus's length in bytes, which the signature must match.
        expected: usize,
        /// What arrived.
        found: usize,
    },
    /// The signature, read as an integer, is not below the modulus. RFC 8017
    /// §5.2.2 step 1.
    SignatureOutOfRange,
    /// The digest handed in is not the length its algorithm produces.
    DigestLength {
        /// The algorithm's output length.
        expected: usize,
        /// What arrived.
        found: usize,
    },
    /// The modulus is too short to hold this digest's encoding with the
    /// minimum eight padding bytes. RFC 8017 §9.2 step 3.
    ModulusTooShort,
    /// The recovered block is not the expected encoding. This is the ordinary
    /// "the signature is not valid" answer.
    EncodingMismatch,
}

/// An RSA public key: a modulus and a public exponent, nothing else.
#[derive(Clone, Copy, Debug)]
pub struct RsaPublicKey {
    modulus: Modulus<RSA_LIMBS>,
    exponent: Uint<RSA_LIMBS>,
}

impl RsaPublicKey {
    /// Reads a key from big-endian `n` and `e`, as a certificate's
    /// `RSAPublicKey` stores them.
    ///
    /// # Errors
    ///
    /// Refuses a modulus wider than 4096 bits, an even modulus, and an
    /// exponent below 3. An even exponent is *not* refused: it cannot be a
    /// real RSA exponent, but CAVP's negative vectors reach here by corrupting
    /// `e`, and letting the encoding comparison reject them exercises more of
    /// the code than an early return would.
    pub fn new(modulus: &[u8], exponent: &[u8]) -> Result<Self, RsaRefusal> {
        let n = Uint::<RSA_LIMBS>::from_be_bytes(modulus).ok_or(RsaRefusal::ModulusTooWide)?;
        let e = Uint::<RSA_LIMBS>::from_be_bytes(exponent).ok_or(RsaRefusal::ExponentUnusable)?;
        if e < Uint::from_u64(3) {
            return Err(RsaRefusal::ExponentUnusable);
        }
        let modulus = Modulus::new(n).ok_or(RsaRefusal::ModulusUnusable)?;
        Ok(Self {
            modulus,
            exponent: e,
        })
    }

    /// The modulus's bit length, which a caller reports as the key size.
    #[must_use]
    pub fn modulus_bits(&self) -> usize {
        self.modulus.bits()
    }

    /// The same modulus with a different exponent, skipping the Montgomery
    /// precomputation. CAVP's file holds fifteen moduli and four hundred and
    /// fifty exponents, and rebuilding the modulus per vector is the slowest
    /// thing in this crate's test suite.
    #[cfg(test)]
    fn with_exponent(&self, exponent: &[u8]) -> Result<Self, RsaRefusal> {
        let e = Uint::<RSA_LIMBS>::from_be_bytes(exponent).ok_or(RsaRefusal::ExponentUnusable)?;
        if e < Uint::from_u64(3) {
            return Err(RsaRefusal::ExponentUnusable);
        }
        Ok(Self {
            modulus: self.modulus,
            exponent: e,
        })
    }

    /// Verifies a signature over an already-computed digest.
    ///
    /// # Errors
    ///
    /// [`RsaRefusal::EncodingMismatch`] when the signature is simply not
    /// valid; the other variants name a malformed input.
    pub fn verify_pkcs1_v15(
        &self,
        algorithm: DigestAlgorithm,
        digest: &[u8],
        signature: &[u8],
    ) -> Result<(), RsaRefusal> {
        if digest.len() != algorithm.output_len() {
            return Err(RsaRefusal::DigestLength {
                expected: algorithm.output_len(),
                found: digest.len(),
            });
        }

        // Step 1: the signature is exactly k octets, or it is not a signature
        // under this key. A short one padded with zeros would represent the
        // same integer, and accepting it would make signatures malleable.
        let k = self.modulus.byte_len();
        if signature.len() != k {
            return Err(RsaRefusal::SignatureLength {
                expected: k,
                found: signature.len(),
            });
        }

        // Step 2a (RSAVP1 step 1): 0 <= s < n.
        let s = Uint::<RSA_LIMBS>::from_be_bytes(signature).ok_or(RsaRefusal::ModulusTooWide)?;
        if s >= *self.modulus.value() {
            return Err(RsaRefusal::SignatureOutOfRange);
        }

        // Steps 2b and 2c: m = s^e mod n, written back out to k octets.
        let m = self.modulus.pow(&s, &self.exponent);
        let mut recovered = [0u8; MAX_RSA_BYTES];
        let Some(recovered) = recovered.get_mut(..k) else {
            return Err(RsaRefusal::ModulusTooWide);
        };
        if !m.to_be_bytes(recovered) {
            // Unreachable: m < n and k is n's own length. Refused rather than
            // asserted, because ruling 1 has no room for an assertion here.
            return Err(RsaRefusal::EncodingMismatch);
        }

        // Steps 3 and 4: build the expected encoding and compare all of it.
        let (expected, expected_len) = encode(algorithm, digest, k)?;
        let Some(expected) = expected.get(..expected_len) else {
            return Err(RsaRefusal::ModulusTooShort);
        };
        if constant_time_eq(recovered, expected) {
            Ok(())
        } else {
            Err(RsaRefusal::EncodingMismatch)
        }
    }

    /// Digests `message` and verifies the signature over it.
    ///
    /// # Errors
    ///
    /// As [`RsaPublicKey::verify_pkcs1_v15`].
    pub fn verify_pkcs1_v15_message(
        &self,
        algorithm: DigestAlgorithm,
        message: &[u8],
        signature: &[u8],
    ) -> Result<(), RsaRefusal> {
        let digest = algorithm.digest(message);
        self.verify_pkcs1_v15(algorithm, digest.as_bytes(), signature)
    }
}

/// EMSA-PKCS1-v1_5 encoding (RFC 8017 §9.2): `0x00 || 0x01 || PS || 0x00 || T`.
///
/// Returns the buffer and the used length rather than a slice, so that no
/// allocation and no lifetime is involved. `em_len` below `tLen + 11` is the
/// standard's own refusal — fewer than eight `0xff` bytes leaves the padding
/// too short to be unambiguous.
fn encode(
    algorithm: DigestAlgorithm,
    digest: &[u8],
    em_len: usize,
) -> Result<([u8; MAX_RSA_BYTES], usize), RsaRefusal> {
    if em_len > MAX_RSA_BYTES {
        return Err(RsaRefusal::ModulusTooWide);
    }
    let prefix = algorithm.digest_info_prefix();
    let t_len = prefix.len() + digest.len();
    if em_len < t_len + 11 {
        return Err(RsaRefusal::ModulusTooShort);
    }

    let mut em = [0u8; MAX_RSA_BYTES];
    // em[0] is already 0x00.
    if let Some(slot) = em.get_mut(1) {
        *slot = 0x01;
    }
    // PS runs from index 2 up to the separator, which sits immediately before
    // T. `em_len >= t_len + 11` makes this subtraction safe and leaves at
    // least eight bytes of padding.
    let separator = em_len - t_len - 1;
    if let Some(padding) = em.get_mut(2..separator) {
        for byte in padding {
            *byte = 0xff;
        }
    }
    // em[separator] is already 0x00.
    let t_at = separator + 1;
    if !put(&mut em, t_at, prefix) || !put(&mut em, t_at + prefix.len(), digest) {
        return Err(RsaRefusal::ModulusTooShort);
    }
    Ok((em, em_len))
}

/// Copies `bytes` into `buffer` at `at`, or reports that they do not fit.
fn put(buffer: &mut [u8], at: usize, bytes: &[u8]) -> bool {
    let Some(end) = at.checked_add(bytes.len()) else {
        return false;
    };
    match buffer.get_mut(at..end) {
        Some(slot) => {
            slot.copy_from_slice(bytes);
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unhex(text: &str) -> Vec<u8> {
        let text = text.trim();
        let padded;
        let text = if text.len() % 2 == 1 {
            padded = format!("0{text}");
            padded.as_str()
        } else {
            text
        };
        text.as_bytes()
            .chunks_exact(2)
            .filter_map(|pair| {
                let digits = std::str::from_utf8(pair).ok()?;
                u8::from_str_radix(digits, 16).ok()
            })
            .collect()
    }

    fn algorithm_named(name: &str) -> Option<DigestAlgorithm> {
        match name {
            "SHA1" => Some(DigestAlgorithm::Sha1),
            "SHA256" => Some(DigestAlgorithm::Sha256),
            "SHA384" => Some(DigestAlgorithm::Sha384),
            "SHA512" => Some(DigestAlgorithm::Sha512),
            // SHA-224 groups exist in the file; this crate has no SHA-224 and
            // guessing its DigestInfo prefix would be exactly the kind of
            // constant this module refuses to invent.
            _ => None,
        }
    }

    /// The DigestInfo prefixes, checked against RFC 8017 §9.2 note 1 in the
    /// shape the standard prints them: a `SEQUENCE` whose length covers an
    /// `AlgorithmIdentifier` and an `OCTET STRING` of the digest's length.
    ///
    /// This is a structural check, not a transcription of the same bytes
    /// twice: it recomputes the two DER lengths and the octet-string length
    /// from `output_len` and asserts the constant agrees, which catches a
    /// digit transposed while copying the table.
    #[test]
    fn digest_info_prefixes_are_self_consistent_der() {
        for algorithm in [
            DigestAlgorithm::Sha1,
            DigestAlgorithm::Sha256,
            DigestAlgorithm::Sha384,
            DigestAlgorithm::Sha512,
        ] {
            let prefix = algorithm.digest_info_prefix();
            let len = algorithm.output_len();
            assert_eq!(prefix.first().copied(), Some(0x30), "DigestInfo SEQUENCE");
            assert_eq!(
                prefix.get(1).copied().map(usize::from),
                Some(prefix.len() - 2 + len),
                "the SEQUENCE length covers the rest of T"
            );
            assert_eq!(
                prefix.get(2).copied(),
                Some(0x30),
                "AlgorithmIdentifier SEQUENCE"
            );
            assert_eq!(
                prefix.get(3).copied().map(usize::from),
                Some(prefix.len() - 6),
                "the AlgorithmIdentifier length covers the OID and its NULL"
            );
            let tail = prefix.len() - 2;
            assert_eq!(prefix.get(tail).copied(), Some(0x04), "OCTET STRING");
            assert_eq!(
                prefix.get(tail + 1).copied().map(usize::from),
                Some(len),
                "the OCTET STRING holds exactly the digest"
            );
        }
        // And the one byte that distinguishes the SHA-2 family members, which
        // a structural check cannot catch: the last arc of the OID.
        assert_eq!(
            DigestAlgorithm::Sha256.digest_info_prefix().get(14),
            Some(&0x01)
        );
        assert_eq!(
            DigestAlgorithm::Sha384.digest_info_prefix().get(14),
            Some(&0x02)
        );
        assert_eq!(
            DigestAlgorithm::Sha512.digest_info_prefix().get(14),
            Some(&0x03)
        );
    }

    #[test]
    fn the_encoding_has_the_shape_rfc_8017_section_9_2_describes() {
        let digest = DigestAlgorithm::Sha256.digest(b"abc");
        let (em, len) =
            encode(DigestAlgorithm::Sha256, digest.as_bytes(), 128).expect("k is ample");
        assert_eq!(len, 128);
        let em = em.get(..len).expect("length was just checked");
        assert_eq!(em.first().copied(), Some(0x00));
        assert_eq!(em.get(1).copied(), Some(0x01));
        let t_len = 19 + 32;
        let separator = 128 - t_len - 1;
        assert!(em
            .get(2..separator)
            .expect("padding")
            .iter()
            .all(|&b| b == 0xff));
        assert!(separator - 2 >= 8, "at least eight bytes of padding");
        assert_eq!(em.get(separator).copied(), Some(0x00));
        assert_eq!(
            em.get(separator + 1..separator + 20),
            Some(DigestAlgorithm::Sha256.digest_info_prefix())
        );
        assert_eq!(em.get(separator + 20..), Some(digest.as_bytes()));
    }

    /// A modulus one byte too short for `tLen + 11` is refused rather than
    /// encoded with seven bytes of padding.
    #[test]
    fn a_modulus_too_short_for_the_digest_is_refused() {
        let digest = DigestAlgorithm::Sha512.digest(b"");
        // T is 19 + 64 = 83 bytes, so k must be at least 94.
        assert_eq!(
            encode(DigestAlgorithm::Sha512, digest.as_bytes(), 93).map(|(_, len)| len),
            Err(RsaRefusal::ModulusTooShort)
        );
        assert_eq!(
            encode(DigestAlgorithm::Sha512, digest.as_bytes(), 94).map(|(_, len)| len),
            Ok(94)
        );
    }

    /// Builds a key and a signature that force the verifier to recover exactly
    /// `em`, without anybody holding a private key.
    ///
    /// The trick is to choose the modulus after the block: with `e = 3` and
    /// `s = 2^1024`, `s^3` is exactly `2^3072`, so setting `n = 2^3072 - V`
    /// makes `s^3 mod n = V` for any `V` below `n`. `V` must be odd for `n` to
    /// be odd, which is why every crafted block below ends in an odd byte.
    ///
    /// This is what lets the tests drive real forged encodings through the
    /// real modular exponentiation. Nothing here is a signing operation: no
    /// private exponent exists, and `n` is not a product of two primes — the
    /// verifier neither knows nor cares.
    fn key_recovering(em: &[u8]) -> (RsaPublicKey, Vec<u8>) {
        assert_eq!(em.len(), 384, "the construction fixes k at 3072 bits");
        assert_eq!(
            em.last().copied().unwrap_or(0) & 1,
            1,
            "V must be odd so that n is odd"
        );
        let v = Uint::<RSA_LIMBS>::from_be_bytes(em).expect("384 bytes fit");
        // 2^3072: one byte of 0x01 ahead of 384 zero bytes.
        let mut radix = [0u8; 385];
        if let Some(slot) = radix.first_mut() {
            *slot = 1;
        }
        let radix = Uint::<RSA_LIMBS>::from_be_bytes(&radix).expect("3073 bits fit in 4096");
        let (n, _) = radix.sub_borrow(&v);
        let mut modulus = [0u8; 384];
        assert!(n.to_be_bytes(&mut modulus), "n is 3072 bits");

        // s = 2^1024, big-endian in exactly k bytes.
        let mut signature = vec![0u8; 384];
        if let Some(slot) = signature.get_mut(384 - 129) {
            *slot = 1;
        }

        let key = RsaPublicKey::new(&modulus, &[3]).expect("odd 3072-bit modulus, e = 3");
        (key, signature)
    }

    /// A digest whose last byte is odd, so the crafted blocks satisfy
    /// `key_recovering`'s parity requirement.
    fn odd_tailed_digest(algorithm: DigestAlgorithm) -> DigestValue {
        for n in 0u32..1000 {
            let digest = algorithm.digest(format!("tinker-pdf {n}").as_bytes());
            if digest.as_bytes().last().copied().unwrap_or(0) & 1 == 1 {
                return digest;
            }
        }
        panic!("a thousand digests without an odd last byte is not a thing that happens");
    }

    /// The positive control for the construction above: a *correct* encoding
    /// recovered through the real exponentiation is accepted.
    #[test]
    fn a_correctly_encoded_block_verifies() {
        let digest = odd_tailed_digest(DigestAlgorithm::Sha256);
        let (em, len) =
            encode(DigestAlgorithm::Sha256, digest.as_bytes(), 384).expect("k is ample");
        let em = em.get(..len).expect("just encoded");
        let (key, signature) = key_recovering(em);
        assert_eq!(
            key.verify_pkcs1_v15(DigestAlgorithm::Sha256, digest.as_bytes(), &signature),
            Ok(())
        );
    }

    /// Bleichenbacher's forgery, in the shape that broke real verifiers: three
    /// bytes of padding instead of 380, the `DigestInfo` immediately after the
    /// separator, and garbage filling the rest. A verifier that walks the
    /// padding and reads the digest out accepts this; one that compares the
    /// whole encoding cannot.
    #[test]
    fn a_short_padding_forgery_with_a_correct_digest_is_refused() {
        let digest = odd_tailed_digest(DigestAlgorithm::Sha1);
        let prefix = DigestAlgorithm::Sha1.digest_info_prefix();
        let mut em = vec![0x00, 0x01, 0xff, 0xff, 0xff, 0x00];
        em.extend_from_slice(prefix);
        em.extend_from_slice(digest.as_bytes());
        // Garbage to the end — the attacker's free bytes.
        while em.len() < 384 {
            em.push(0xa5);
        }
        let (key, signature) = key_recovering(&em);
        assert_eq!(
            key.verify_pkcs1_v15(DigestAlgorithm::Sha1, digest.as_bytes(), &signature),
            Err(RsaRefusal::EncodingMismatch),
            "the digest is right and the padding is not; that is still a forgery"
        );
    }

    /// The separator moved: full-length padding, but the `0x00` that ends it
    /// sits one byte early and a spare `0xff` leads the `DigestInfo`.
    #[test]
    fn a_misplaced_separator_is_refused() {
        let digest = odd_tailed_digest(DigestAlgorithm::Sha256);
        let (em, len) =
            encode(DigestAlgorithm::Sha256, digest.as_bytes(), 384).expect("k is ample");
        let mut em = em.get(..len).expect("just encoded").to_vec();
        let separator = 384 - (19 + 32) - 1;
        if let Some(slot) = em.get_mut(separator - 1) {
            *slot = 0x00;
        }
        if let Some(slot) = em.get_mut(separator) {
            *slot = 0xff;
        }
        let (key, signature) = key_recovering(&em);
        assert_eq!(
            key.verify_pkcs1_v15(DigestAlgorithm::Sha256, digest.as_bytes(), &signature),
            Err(RsaRefusal::EncodingMismatch)
        );
    }

    /// Trailing garbage after a complete, correct `DigestInfo`: the block is
    /// shifted left and the tail filled. Everything a lax parser looks at is
    /// right.
    #[test]
    fn trailing_garbage_after_the_digest_info_is_refused() {
        let digest = odd_tailed_digest(DigestAlgorithm::Sha256);
        let (em, len) =
            encode(DigestAlgorithm::Sha256, digest.as_bytes(), 384).expect("k is ample");
        let mut em = em.get(..len).expect("just encoded").to_vec();
        em.remove(2);
        em.push(0x01);
        let (key, signature) = key_recovering(&em);
        assert_eq!(
            key.verify_pkcs1_v15(DigestAlgorithm::Sha256, digest.as_bytes(), &signature),
            Err(RsaRefusal::EncodingMismatch)
        );
    }

    /// A single padding byte changed from `0xff` to `0xfe`. The classic
    /// "everything else is fine" case.
    #[test]
    fn one_wrong_padding_byte_is_refused() {
        let digest = odd_tailed_digest(DigestAlgorithm::Sha384);
        let (em, len) =
            encode(DigestAlgorithm::Sha384, digest.as_bytes(), 384).expect("k is ample");
        let mut em = em.get(..len).expect("just encoded").to_vec();
        if let Some(slot) = em.get_mut(100) {
            *slot = 0xfe;
        }
        let (key, signature) = key_recovering(&em);
        assert_eq!(
            key.verify_pkcs1_v15(DigestAlgorithm::Sha384, digest.as_bytes(), &signature),
            Err(RsaRefusal::EncodingMismatch)
        );
    }

    /// A `DigestInfo` naming SHA-256 over a SHA-256 digest, verified while
    /// claiming SHA-384: the prefix is a real prefix and the digest is a real
    /// digest, and they do not belong together.
    #[test]
    fn a_digest_info_for_the_wrong_algorithm_is_refused() {
        let digest = odd_tailed_digest(DigestAlgorithm::Sha256);
        let (em, len) =
            encode(DigestAlgorithm::Sha256, digest.as_bytes(), 384).expect("k is ample");
        let em = em.get(..len).expect("just encoded");
        let (key, signature) = key_recovering(em);
        let other = DigestAlgorithm::Sha384.digest(b"anything");
        assert_eq!(
            key.verify_pkcs1_v15(DigestAlgorithm::Sha384, other.as_bytes(), &signature),
            Err(RsaRefusal::EncodingMismatch)
        );
    }

    /// The block-type byte changed from 1 to 2 — a PKCS#1 encryption block
    /// offered as a signature block.
    #[test]
    fn the_wrong_block_type_is_refused() {
        let digest = odd_tailed_digest(DigestAlgorithm::Sha256);
        let (em, len) =
            encode(DigestAlgorithm::Sha256, digest.as_bytes(), 384).expect("k is ample");
        let mut em = em.get(..len).expect("just encoded").to_vec();
        if let Some(slot) = em.get_mut(1) {
            *slot = 0x02;
        }
        let (key, signature) = key_recovering(&em);
        assert_eq!(
            key.verify_pkcs1_v15(DigestAlgorithm::Sha256, digest.as_bytes(), &signature),
            Err(RsaRefusal::EncodingMismatch)
        );
    }

    #[test]
    fn malformed_inputs_are_refused_by_name() {
        let digest = odd_tailed_digest(DigestAlgorithm::Sha256);
        let (em, len) =
            encode(DigestAlgorithm::Sha256, digest.as_bytes(), 384).expect("k is ample");
        let em = em.get(..len).expect("just encoded");
        let (key, signature) = key_recovering(em);

        // A signature one byte short is not this key's signature, even though
        // it represents the same integer.
        let mut short = signature.clone();
        short.remove(0);
        assert_eq!(
            key.verify_pkcs1_v15(DigestAlgorithm::Sha256, digest.as_bytes(), &short),
            Err(RsaRefusal::SignatureLength {
                expected: 384,
                found: 383
            })
        );

        // A digest of the wrong length for its algorithm.
        assert_eq!(
            key.verify_pkcs1_v15(DigestAlgorithm::Sha256, &[0u8; 20], &signature),
            Err(RsaRefusal::DigestLength {
                expected: 32,
                found: 20
            })
        );

        // s = n is out of range; RFC 8017 §5.2.2 step 1.
        let mut at_modulus = vec![0u8; 384];
        assert!(key.modulus.value().to_be_bytes(&mut at_modulus));
        assert_eq!(
            key.verify_pkcs1_v15(DigestAlgorithm::Sha256, digest.as_bytes(), &at_modulus),
            Err(RsaRefusal::SignatureOutOfRange)
        );

        // Keys that are not keys.
        assert_eq!(
            RsaPublicKey::new(&[0xff; 600], &[3]).err(),
            Some(RsaRefusal::ModulusTooWide)
        );
        assert_eq!(
            RsaPublicKey::new(&[0xff, 0xf0], &[3]).err(),
            Some(RsaRefusal::ModulusUnusable),
            "an even modulus"
        );
        assert_eq!(
            RsaPublicKey::new(&[0xff, 0xf1], &[1]).err(),
            Some(RsaRefusal::ExponentUnusable)
        );
    }

    /// NIST CAVP `SigVer15_186-3.rsp` from `186-2rsatestvectors.zip`: every
    /// vector for a digest this crate implements, positive and negative alike.
    ///
    /// The counts are asserted, not printed, because "the CAVP vectors pass"
    /// means nothing without them — a parser that silently matched no lines
    /// would report the same success. The negatives are the half that carries
    /// the weight here: 75 have a corrupted message, 75 a corrupted exponent,
    /// 75 a corrupted signature, 75 move the hash left inside the padding and
    /// 75 delete the `0x00` that ends the padding. The last two are the
    /// forgery this module's full-encoding comparison exists to refuse, and
    /// they are published rather than invented.
    #[test]
    fn cavp_sigver15_vectors() {
        const VECTORS: &str = include_str!("../tests/data/cavp/rsa_sigver15.rsp");

        let mut base: Option<RsaPublicKey> = None;
        let mut algorithm: Option<DigestAlgorithm> = None;
        let mut exponent = Vec::new();
        let mut message = Vec::new();
        let mut signature = Vec::new();

        let mut ran = 0usize;
        let mut skipped = 0usize;
        let mut accepted = 0usize;
        let mut rejected = 0usize;

        for line in VECTORS.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
                continue;
            }
            let Some((name, value)) = line.split_once(" = ") else {
                continue;
            };
            match name {
                "n" => {
                    base = Some(
                        RsaPublicKey::new(&unhex(value), &[3]).expect("a CAVP modulus is usable"),
                    );
                }
                "SHAAlg" => algorithm = algorithm_named(value.trim()),
                "e" => exponent = unhex(value),
                "Msg" => message = unhex(value),
                "S" => signature = unhex(value),
                "Result" => {
                    let Some(algorithm) = algorithm else {
                        skipped += 1;
                        continue;
                    };
                    let expected_valid = value.starts_with('P');
                    let base = base.as_ref().expect("a modulus precedes every vector");
                    let outcome = base.with_exponent(&exponent).and_then(|key| {
                        key.verify_pkcs1_v15_message(algorithm, &message, &signature)
                    });
                    assert_eq!(
                        outcome.is_ok(),
                        expected_valid,
                        "vector {ran}: expected {value}, got {outcome:?}"
                    );
                    ran += 1;
                    if expected_valid {
                        accepted += 1;
                    } else {
                        rejected += 1;
                    }
                }
                _ => {}
            }
        }

        assert_eq!(ran, 360, "vectors run");
        assert_eq!(skipped, 90, "SHA-224 vectors, which this crate cannot hash");
        assert_eq!(accepted, 60, "Result = P");
        assert_eq!(rejected, 300, "Result = F");
    }
}
