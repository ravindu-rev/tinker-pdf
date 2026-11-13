//! RSA: with a **public key**, signature verification (RFC 8017 §8.2.2 and
//! §8.1.2) and RSAES-PKCS1-v1_5 encryption (§7.2.1); with a **private key**,
//! the private operation those signatures are made with (§5.2.1).
//!
//! A PDF signature arrives as a certificate's public key and a blob of bytes,
//! and the public side answers one question about them; a public-key
//! encrypted document seals its file key to a recipient's certificate, which
//! is the same public key used the other way round. `tinker-pdf-pki` does the
//! DER; this module takes `(n, e)` as big-endian integers, and a digest that
//! [`crate::sha1`] or [`crate::sha2`] already computed or a message and the
//! caller's padding. There is no key generation and no key-file parsing: a
//! private key arrives as its numbers ([`RsaPrivateComponents`]).
//!
//! # The private key
//!
//! [`RsaPrivateKey`] holds RFC 8017 §3.2's second representation and runs
//! the private operation by it (`docs/design/signing.md`, *RSA signing*):
//! `m1 = c^dP mod p` and `m2 = c^dQ mod q`, each the constant-time path's
//! fixed-window exponentiation at its prime's width, joined by Garner's
//! `h = qInv · (m1 - m2) mod p` and `s = m2 + q · h`. Every operation on a
//! secret — the reductions under each prime, the exponentiations, the
//! recombination, the blinding — runs on that path ([`crate::bignum`], *Two
//! paths*), and the key's secrets live in [`crate::bignum::Secret`]s, wiped
//! when it drops.
//!
//! **Blinded by the caller's entropy.** Before the exponentiations `c` is
//! multiplied by `r^e` and after them `s` by `r^-1`, for an `r` drawn from
//! the caller's [`crate::EntropySource`] uniform in `[1, n)`, so the values
//! the private exponents meet are unrelated to anything an observer chose
//! (Kocher, 1996). The pair is drawn on the key's first private operation and
//! on every thirty-second after it, each draw paying for one inverse — by
//! Fermat under each prime, joined by Garner — and squared in between, which
//! keeps it a pair for `r²`. A drawn `r` that is zero, not below `n` or
//! shares a factor with `n` is drawn again; a source that cannot fill is a
//! refusal, and no private operation runs unblinded.
//!
//! **A fault does not leave.** A miscomputed half reveals a prime to anyone
//! holding the faulty signature and a correct one (Boneh, DeMillo and Lipton,
//! 1997), so every result is raised to `e` and compared with `c` before it is
//! returned, and one that fails is [`RsaRefusal::FaultDetected`], carrying
//! nothing of it.
//!
//! **The scheme.** [`RsaPrivateKey::sign_pkcs1_v15`] is RSASSA-PKCS1-V1_5-SIGN
//! (§8.2.1): the same EMSA-PKCS1-v1_5 block the verifier builds and compares
//! whole, from the same per-digest `DigestInfo` prefixes, signed by the
//! private operation. The digest is the caller's choice — any SHA-2 member,
//! never SHA-1 — and the signature is a function of the key and the digest
//! alone, so blinding moves nothing and the same request signs to the same
//! bytes on every target.
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
//! A digest algorithm [`DigestAlgorithm`] has no member for cannot be
//! verified or signed: the `DigestInfo` prefix is a constant per algorithm
//! and a guessed one would accept the wrong thing. SHA-1 and the whole SHA-2
//! family — SHA-224, SHA-256, SHA-384, SHA-512, SHA-512/224 and SHA-512/256,
//! every digest [`crate::sha2`] computes — are offered; MD2 and MD5 are not,
//! even though RFC 8017 lists them, because no signature worth checking uses
//! them and a verifier that accepts MD5 is a verifier that accepts a
//! chosen-prefix collision. SHA-1 is verified and never signed with. The CMS
//! reader in `tinker-pdf-pki`, which resolves a digest OID to this selector,
//! names SHA-1, SHA-256, SHA-384 and SHA-512 only.
//!
//! A modulus wider than 4096 bits is refused rather than truncated, and so is
//! a signature whose length is not exactly the modulus's.
//!
//! # RSASSA-PSS
//!
//! [`RsaPublicKey::verify_pss`] is RFC 8017 §8.1.2 with EMSA-PSS-VERIFY
//! (§9.1.2) and MGF1 (B.2.1). PSS cannot be checked the way PKCS#1 v1.5 is —
//! the salt is random, so there is no expected encoding to build and compare —
//! and the encoding is instead *unmasked and read*. That is the part that has
//! to be exact, so every step that reads a length or an offset reads it from
//! the parameters the caller declared and never from the recovered block: the
//! salt length is the declared one, not whatever a run of zeros happens to
//! imply, and a block whose padding does not end exactly where that length
//! says is refused.
//!
//! Two inputs move the arithmetic in ways a quick reading misses, and both
//! are gated on published data rather than on this module's opinion:
//!
//! * **`emBits` is `modBits - 1`, not `8k`.** For a modulus whose bit length
//!   is one more than a multiple of eight the encoded message is a whole
//!   octet shorter than the signature, and the leading octet of `s^e mod n`
//!   must then be zero. RSA Laboratories' `pss-vect.txt` carries 1 025- to
//!   1 031-bit keys for exactly this reason, and NIST's file has none.
//! * **The leftmost `8·emLen − emBits` bits are cleared after unmasking and
//!   must be zero before it.** Forgetting the first rejects every signature
//!   under such a key; forgetting the second accepts a block with garbage in
//!   bits the encoding does not have.

use std::sync::Mutex;

use crate::bignum::{
    equal_secret, less_secret, product_equals, subtract_small_secret, Modulus, Secret, SecretBytes,
    SecretModulus, Uint, MAX_LIMBS,
};
use crate::handler::constant_time_eq;
use crate::EntropySource;
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
    /// SHA-224.
    Sha224,
    /// SHA-256.
    Sha256,
    /// SHA-384.
    Sha384,
    /// SHA-512.
    Sha512,
    /// SHA-512/224: SHA-512's computation from its own initial value, cut to
    /// 224 bits.
    Sha512_224,
    /// SHA-512/256: SHA-512's computation from its own initial value, cut to
    /// 256 bits.
    Sha512_256,
}

impl DigestAlgorithm {
    /// The digest's length in bytes.
    #[must_use]
    pub const fn output_len(self) -> usize {
        match self {
            Self::Sha1 => 20,
            Self::Sha224 | Self::Sha512_224 => 28,
            Self::Sha256 | Self::Sha512_256 => 32,
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
            Self::Sha224 => &[
                0x30, 0x2d, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02,
                0x04, 0x05, 0x00, 0x04, 0x1c,
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
            Self::Sha512_224 => &[
                0x30, 0x2d, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02,
                0x05, 0x05, 0x00, 0x04, 0x1c,
            ],
            Self::Sha512_256 => &[
                0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02,
                0x06, 0x05, 0x00, 0x04, 0x20,
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
            Self::Sha224 => value.fill(&sha2::sha224(data)),
            Self::Sha256 => value.fill(&sha2::sha256(data)),
            Self::Sha384 => value.fill(&sha2::sha384(data)),
            Self::Sha512 => value.fill(&sha2::sha512(data)),
            Self::Sha512_224 => value.fill(&sha2::sha512_224(data)),
            Self::Sha512_256 => value.fill(&sha2::sha512_256(data)),
        }
        value
    }

    /// Whether a signature may be made over this digest: every member but
    /// SHA-1, whose collisions are found by computation, so that a signature
    /// over its digest binds the signer to a document someone else could
    /// have chosen a twin of (`docs/design/signing.md`, *The schemes*).
    #[must_use]
    pub const fn signs(self) -> bool {
        !matches!(self, Self::Sha1)
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

/// Why a key, a signature or a private operation was refused.
///
/// Typed rather than a bare `false`, because a caller reporting on a signature
/// has to distinguish "this key is not one I can work with" from "this
/// signature does not verify" — the first is a gap in the engine and the
/// second is a fact about the document. Naming the reason leaks nothing. On
/// the public side every input is public. On the private side a refusal names
/// a key whose parts do not make a key, a request the scheme cannot encode, or
/// a caller's source that could not blind, each decided before the private
/// operation runs, or a fault found after it — and none carries a value the
/// operation computed.
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
    /// A message longer than RSAES-PKCS1-v1_5 can carry under this key:
    /// `k - 11` octets (RFC 8017 §7.2.1 step 1).
    MessageTooLong {
        /// The most this key can carry.
        capacity: usize,
    },
    /// Encryption padding that is not exactly `k - 3 - mLen` octets, or that
    /// contains a zero octet, which would end `PS` early and change what the
    /// recipient reads as the message (§7.2.1 step 2b).
    PaddingUnusable,
    /// A private key whose parts do not make a key, naming the check that
    /// found it (RFC 8017 §3.2).
    KeyCheckFailed(KeyCheck),
    /// A message representative not below the modulus: RSASP1's "message
    /// representative out of range" (RFC 8017 §5.2.1 step 1).
    MessageRepresentativeOutOfRange,
    /// The caller's entropy source could not fill a buffer. A private
    /// operation is never run unblinded for want of one.
    EntropyUnavailable,
    /// The caller's entropy source filled every buffer, and draw after draw
    /// was zero, not below the modulus or shared a factor with it: a source
    /// that is not random cannot blind.
    EntropyUnusable,
    /// The lock that holds a key's blinding pair was poisoned by a thread
    /// that panicked holding it. The pair is not trusted, and no operation
    /// runs without one.
    KeyLockPoisoned,
    /// The result failed the check made with the public exponent before it
    /// leaves: the arithmetic went wrong somewhere, and a faulty RSA
    /// signature can reveal a prime. Nothing of it is returned.
    FaultDetected,
    /// A digest that is verified and never signed with: SHA-1
    /// ([`DigestAlgorithm::signs`]).
    DigestNotForSigning,
}

/// The check an RSA private key failed when it was built.
///
/// Each is a property that can be checked without trusting the key, and none
/// says more than that the parts do not make one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyCheck {
    /// A prime is even, below 3, or wider than 4 096 bits.
    PrimeUnusable,
    /// `p · q` is not `n`.
    PrimesDoNotMakeModulus,
    /// `d` is not in `[1, n)`.
    PrivateExponentOutOfRange,
    /// `dP` is not in `[1, p)`, or `dQ` not in `[1, q)`.
    CrtExponentOutOfRange,
    /// `qInv` is not below `p`, or `qInv · q` is not 1 modulo `p`.
    CoefficientMismatch,
}

/// RSASSA-PSS's three choices (RFC 8017 §8.1 and A.2.3): the digest that
/// reduced the message, the digest MGF1 is built on, and the salt length.
///
/// All three are the signer's and come from the signature's own algorithm
/// identifier; `tinker-pdf-pki` reads them out of an `RSASSA-PSS-params`. The
/// verifier takes them as given rather than inferring any of them from the
/// recovered block, because a block cannot be trusted to say how it should be
/// read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PssParameters {
    /// The digest of the message, and of `M'` in EMSA-PSS step 13.
    pub hash: DigestAlgorithm,
    /// The digest MGF1 is computed with. Equal to `hash` in every signature
    /// either published vector set carries, and not required to be.
    pub mask_hash: DigestAlgorithm,
    /// The salt length in octets.
    pub salt_length: usize,
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

    /// Verifies an RSASSA-PSS signature over an already-computed digest
    /// (RFC 8017 §8.1.2).
    ///
    /// # Errors
    ///
    /// [`RsaRefusal::EncodingMismatch`] when the signature is simply not
    /// valid — including a recovered integer too wide for `emLen` octets,
    /// which step 2c calls "invalid signature" rather than an error.
    /// [`RsaRefusal::ModulusTooShort`] when the key cannot hold a digest and
    /// a salt of the declared lengths at all, which is a property of the key
    /// and the parameters rather than of the signature.
    pub fn verify_pss(
        &self,
        parameters: PssParameters,
        digest: &[u8],
        signature: &[u8],
    ) -> Result<(), RsaRefusal> {
        if digest.len() != parameters.hash.output_len() {
            return Err(RsaRefusal::DigestLength {
                expected: parameters.hash.output_len(),
                found: digest.len(),
            });
        }

        // Step 1: exactly k octets, for the same reason as PKCS#1 v1.5.
        let k = self.modulus.byte_len();
        if signature.len() != k {
            return Err(RsaRefusal::SignatureLength {
                expected: k,
                found: signature.len(),
            });
        }

        // Step 2a and 2b: 0 <= s < n, m = s^e mod n.
        let s = Uint::<RSA_LIMBS>::from_be_bytes(signature).ok_or(RsaRefusal::ModulusTooWide)?;
        if s >= *self.modulus.value() {
            return Err(RsaRefusal::SignatureOutOfRange);
        }
        let m = self.modulus.pow(&s, &self.exponent);

        // Step 2c: EM = I2OSP(m, emLen) with emLen = ceil((modBits - 1) / 8).
        // `m < n` fits k octets; when emLen is k - 1 the octet in front must
        // be zero, or I2OSP reports "integer too large" and the signature is
        // invalid.
        let em_bits = self.modulus.bits().saturating_sub(1);
        let em_len = em_bits.div_ceil(8);
        let mut whole = [0u8; MAX_RSA_BYTES];
        let Some(whole) = whole.get_mut(..k) else {
            return Err(RsaRefusal::ModulusTooWide);
        };
        if !m.to_be_bytes(whole) {
            // Unreachable: m < n and k is n's own length.
            return Err(RsaRefusal::EncodingMismatch);
        }
        let Some((lead, em)) = k.checked_sub(em_len).map(|at| whole.split_at(at)) else {
            return Err(RsaRefusal::EncodingMismatch);
        };
        if lead.iter().any(|&byte| byte != 0) {
            return Err(RsaRefusal::EncodingMismatch);
        }

        // Step 3.
        emsa_pss_verify(parameters, digest, em, em_bits)
    }

    /// Digests `message` with `parameters.hash` and verifies the RSASSA-PSS
    /// signature over it.
    ///
    /// # Errors
    ///
    /// As [`RsaPublicKey::verify_pss`].
    pub fn verify_pss_message(
        &self,
        parameters: PssParameters,
        message: &[u8],
        signature: &[u8],
    ) -> Result<(), RsaRefusal> {
        let digest = parameters.hash.digest(message);
        self.verify_pss(parameters, digest.as_bytes(), signature)
    }
}

impl RsaPublicKey {
    /// How many padding octets RSAES-PKCS1-v1_5 needs to encrypt a message of
    /// `message_len` octets under this key, or `None` when the key cannot
    /// carry it (RFC 8017 §7.2.1: `k - 3 - mLen`, at least eight).
    #[must_use]
    pub fn pkcs1_v15_padding_len(&self, message_len: usize) -> Option<usize> {
        let k = self.modulus.byte_len();
        let capacity = k.checked_sub(11)?;
        if message_len > capacity {
            return None;
        }
        Some(k - 3 - message_len)
    }

    /// RSAES-PKCS1-v1_5 encryption (RFC 8017 §7.2.1): `EM = 0x00 || 0x02 ||
    /// PS || 0x00 || M`, raised to the public exponent.
    ///
    /// **The padding is the caller's**, because this crate has no source of
    /// randomness and will not pretend to: `padding` is `PS`, exactly
    /// [`RsaPublicKey::pkcs1_v15_padding_len`] octets, none of them zero, drawn
    /// by the caller from whatever it trusts. Taking it as a parameter is also
    /// what makes the operation testable — RSA Laboratories' vectors publish
    /// the `PS` each ciphertext was made with, so all 300 are exact known
    /// answers rather than round trips through a decryptor this crate does not
    /// have.
    ///
    /// A public-key operation on public values, so the constant-time question
    /// the private side would raise does not arise; `padding` is secret only
    /// in the sense that it must be unpredictable, and nothing here branches
    /// on its value beyond refusing a zero.
    ///
    /// # Errors
    ///
    /// [`RsaRefusal::MessageTooLong`], [`RsaRefusal::PaddingUnusable`], and
    /// [`RsaRefusal::ModulusTooShort`] for a key below the eleven octets any
    /// message needs.
    pub fn encrypt_pkcs1_v15(&self, message: &[u8], padding: &[u8]) -> Result<Vec<u8>, RsaRefusal> {
        let k = self.modulus.byte_len();
        let capacity = k.checked_sub(11).ok_or(RsaRefusal::ModulusTooShort)?;
        if message.len() > capacity {
            return Err(RsaRefusal::MessageTooLong { capacity });
        }
        let ps_len = k - 3 - message.len();
        if padding.len() != ps_len || padding.contains(&0) {
            return Err(RsaRefusal::PaddingUnusable);
        }
        let mut em = vec![0u8; k];
        // Step 2c, laid out by offset: 0x00 at 0, 0x02 at 1, PS from 2, the
        // separating 0x00 after it, then M.
        if let Some(block_type) = em.get_mut(1) {
            *block_type = 0x02;
        }
        if !put(&mut em, 2, padding) || !put(&mut em, 3 + ps_len, message) {
            return Err(RsaRefusal::ModulusTooWide);
        }
        // Step 3a: OS2IP. The leading 0x00 0x02 keeps m below every modulus
        // of k octets, so `m < n` is an invariant; checked rather than assumed.
        let m = Uint::<RSA_LIMBS>::from_be_bytes(&em).ok_or(RsaRefusal::ModulusTooWide)?;
        if m >= *self.modulus.value() {
            return Err(RsaRefusal::ModulusTooShort);
        }
        // Steps 3b and 3c: c = m^e mod n, as exactly k octets.
        let c = self.modulus.pow(&m, &self.exponent);
        let mut out = vec![0u8; k];
        if !c.to_be_bytes(&mut out) {
            return Err(RsaRefusal::ModulusTooWide);
        }
        Ok(out)
    }
}

/// How many private operations one blinding pair serves before a fresh one is
/// drawn: the key's first, and every thirty-second after it.
const BLINDING_REFRESH: u32 = 32;

/// How many draws a blinding factor gets before the source is called
/// unusable. A draw is refused when it is zero, not below `n`, or shares a
/// factor with `n`, and an honest source is refused for the second less than
/// half the time, so 128 refusals in a row is a source that is not random.
const BLINDING_DRAWS: usize = 128;

/// An RSA private key's parts — RFC 8017 §3.2's second representation, as
/// big-endian octet strings, in the order and under the names PKCS#1's
/// `RSAPrivateKey` (A.1.2) gives them.
///
/// Borrowed rather than owned: [`RsaPrivateKey::new`] reads each part
/// straight into memory of its own, and the caller's bytes stay the caller's
/// to wipe. There is deliberately no `Debug`.
#[derive(Clone, Copy)]
pub struct RsaPrivateComponents<'a> {
    /// `n`.
    pub modulus: &'a [u8],
    /// `e`.
    pub public_exponent: &'a [u8],
    /// `d`.
    pub private_exponent: &'a [u8],
    /// `p`.
    pub prime1: &'a [u8],
    /// `q`.
    pub prime2: &'a [u8],
    /// `dP = d mod (p - 1)`.
    pub exponent1: &'a [u8],
    /// `dQ = d mod (q - 1)`.
    pub exponent2: &'a [u8],
    /// `qInv = q^-1 mod p`.
    pub coefficient: &'a [u8],
}

/// `r^e` and `r^-1` modulo `n`, each in Montgomery form, for one blinding
/// factor `r` that is not itself kept.
struct BlindingPair {
    forward: Secret<RSA_LIMBS>,
    inverse: Secret<RSA_LIMBS>,
}

impl BlindingPair {
    fn duplicate(&self) -> Self {
        Self {
            forward: self.forward.duplicate(),
            inverse: self.inverse.duplicate(),
        }
    }
}

/// A key's blinding state, behind its lock: the pair, if one has been drawn,
/// and how many operations have used it.
struct Blinding {
    pair: Option<BlindingPair>,
    uses: u32,
}

/// An RSA private key, on the constant-time path (`docs/design/signing.md`,
/// *The RSA private key*).
///
/// The public `n` and `e`; `d`, `dP` and `dQ` each a [`Secret`], `p` and `q`
/// each a [`SecretModulus`] with its Montgomery constants; `qInv` held in
/// Montgomery form under `p` and `q` in Montgomery form under `n`, which is
/// what the recombination multiplies by; and the blinding pair behind a lock,
/// so threads signing with one key never share a factor. Every secret is
/// wiped when the key drops, and `Debug` prints the key's size and nothing
/// else.
pub struct RsaPrivateKey {
    public: RsaPublicKey,
    d: Secret<RSA_LIMBS>,
    p: SecretModulus<RSA_LIMBS>,
    q: SecretModulus<RSA_LIMBS>,
    dp: Secret<RSA_LIMBS>,
    dq: Secret<RSA_LIMBS>,
    q_inv_montgomery: Secret<RSA_LIMBS>,
    q_montgomery: Secret<RSA_LIMBS>,
    blinding: Mutex<Blinding>,
}

impl RsaPrivateKey {
    /// Builds a key from its parts, or refuses it.
    ///
    /// It checks what can be checked without trusting the key: `n` and `e` as
    /// [`RsaPublicKey::new`] does; both primes odd, at least 3 and within
    /// 4 096 bits; `p · q = n` exactly; `d` in `[1, n)`; `dP` and `dQ` in
    /// `[1, p)` and `[1, q)`; and `qInv` below `p` with `qInv · q ≡ 1 (mod
    /// p)`. Each comparison reads every limb of its operands; a refusal names
    /// the check ([`KeyCheck`]). A `dP` or `dQ` that is in range and wrong
    /// passes these checks and is caught by the check every result takes
    /// before it leaves.
    ///
    /// # Errors
    ///
    /// [`RsaRefusal::ModulusTooWide`], [`RsaRefusal::ModulusUnusable`],
    /// [`RsaRefusal::ExponentUnusable`], or [`RsaRefusal::KeyCheckFailed`]
    /// naming the check.
    pub fn new(components: &RsaPrivateComponents<'_>) -> Result<Self, RsaRefusal> {
        let refused = RsaRefusal::KeyCheckFailed;
        let public = RsaPublicKey::new(components.modulus, components.public_exponent)?;
        let d = Secret::from_be_bytes(components.private_exponent)
            .ok_or(refused(KeyCheck::PrivateExponentOutOfRange))?;
        let p_value =
            Secret::from_be_bytes(components.prime1).ok_or(refused(KeyCheck::PrimeUnusable))?;
        let q_value =
            Secret::from_be_bytes(components.prime2).ok_or(refused(KeyCheck::PrimeUnusable))?;
        let dp = Secret::from_be_bytes(components.exponent1)
            .ok_or(refused(KeyCheck::CrtExponentOutOfRange))?;
        let dq = Secret::from_be_bytes(components.exponent2)
            .ok_or(refused(KeyCheck::CrtExponentOutOfRange))?;
        let q_inv = Secret::from_be_bytes(components.coefficient)
            .ok_or(refused(KeyCheck::CoefficientMismatch))?;

        let p = SecretModulus::new(&p_value).ok_or(refused(KeyCheck::PrimeUnusable))?;
        let q = SecretModulus::new(&q_value).ok_or(refused(KeyCheck::PrimeUnusable))?;
        let n = &public.modulus;
        if !product_equals(p_value.value(), q_value.value(), n.value()) {
            return Err(refused(KeyCheck::PrimesDoNotMakeModulus));
        }
        let zero = Uint::zero();
        if equal_secret(dp.value(), &zero)
            || !less_secret(dp.value(), p_value.value())
            || equal_secret(dq.value(), &zero)
            || !less_secret(dq.value(), q_value.value())
        {
            return Err(refused(KeyCheck::CrtExponentOutOfRange));
        }
        if !less_secret(q_inv.value(), p_value.value()) {
            return Err(refused(KeyCheck::CoefficientMismatch));
        }
        let under_p = p.modulus();
        let mut q_mod_p = Secret::zero();
        under_p.reduce_secret(q_value.value(), q.modulus().limbs(), q_mod_p.value_mut());
        let mut product = Secret::zero();
        under_p.mul_mod_secret(q_inv.value(), q_mod_p.value(), product.value_mut());
        if !equal_secret(product.value(), &Uint::from_u64(1)) {
            return Err(refused(KeyCheck::CoefficientMismatch));
        }

        let mut q_inv_montgomery = Secret::zero();
        under_p.to_montgomery_secret(q_inv.value(), q_inv_montgomery.value_mut());
        let mut q_montgomery = Secret::zero();
        n.to_montgomery_secret(q_value.value(), q_montgomery.value_mut());
        let key = Self {
            public,
            d,
            p,
            q,
            dp,
            dq,
            q_inv_montgomery,
            q_montgomery,
            blinding: Mutex::new(Blinding {
                pair: None,
                uses: 0,
            }),
        };
        if equal_secret(key.d.value(), &zero)
            || !less_secret(key.d.value(), key.public.modulus.value())
        {
            return Err(refused(KeyCheck::PrivateExponentOutOfRange));
        }
        Ok(key)
    }

    /// The public half: what a verifier holds.
    #[must_use]
    pub fn public_key(&self) -> &RsaPublicKey {
        &self.public
    }

    /// The modulus's bit length, the key's size.
    #[must_use]
    pub fn modulus_bits(&self) -> usize {
        self.public.modulus_bits()
    }

    /// RSASSA-PKCS1-V1_5-SIGN (RFC 8017 §8.2.1) over a digest the caller has
    /// already taken with `algorithm`: EMSA-PKCS1-v1_5 (§9.2) builds
    /// `0x00 0x01 PS 0x00 T`, `T` the `DigestInfo` of the digest from the same
    /// per-digest prefixes the verifier compares whole, and the private
    /// operation signs it. The result is `k` octets.
    ///
    /// The digest is the caller's choice and nothing picks one for them.
    /// The signature is a function of the key and the digest alone — blinding
    /// does not move it — so the same request signs to the same bytes on
    /// every target.
    ///
    /// # Errors
    ///
    /// [`RsaRefusal::DigestNotForSigning`] for SHA-1,
    /// [`RsaRefusal::DigestLength`] for a digest of the wrong length,
    /// [`RsaRefusal::ModulusTooShort`] for a key that cannot hold the encoding
    /// with eight octets of padding (§9.2 step 3), and the private
    /// operation's own refusals.
    pub fn sign_pkcs1_v15(
        &self,
        algorithm: DigestAlgorithm,
        digest: &[u8],
        entropy: &mut dyn EntropySource,
    ) -> Result<Vec<u8>, RsaRefusal> {
        if !algorithm.signs() {
            return Err(RsaRefusal::DigestNotForSigning);
        }
        if digest.len() != algorithm.output_len() {
            return Err(RsaRefusal::DigestLength {
                expected: algorithm.output_len(),
                found: digest.len(),
            });
        }
        // §8.2.1 step 1: EM is k octets, which keeps OS2IP(EM) below n.
        let (em, em_len) = encode(algorithm, digest, self.public.modulus.byte_len())?;
        let em = em.get(..em_len).ok_or(RsaRefusal::ModulusTooShort)?;
        let representative =
            Uint::<RSA_LIMBS>::from_be_bytes(em).ok_or(RsaRefusal::ModulusTooWide)?;
        self.private_operation(&representative, entropy)
    }

    /// Digests `message` with `algorithm` and signs the digest by
    /// [`RsaPrivateKey::sign_pkcs1_v15`].
    ///
    /// # Errors
    ///
    /// As [`RsaPrivateKey::sign_pkcs1_v15`]; a digest that is not signed with
    /// is refused before the message is read.
    pub fn sign_pkcs1_v15_message(
        &self,
        algorithm: DigestAlgorithm,
        message: &[u8],
        entropy: &mut dyn EntropySource,
    ) -> Result<Vec<u8>, RsaRefusal> {
        if !algorithm.signs() {
            return Err(RsaRefusal::DigestNotForSigning);
        }
        let digest = algorithm.digest(message);
        self.sign_pkcs1_v15(algorithm, digest.as_bytes(), entropy)
    }

    /// RSASP1 (RFC 8017 §5.2.1) on a message representative below `n`: `k`
    /// octets, blinded by the caller's entropy and checked with the public
    /// exponent before they leave. The signature schemes are its callers.
    /// The representative is public — an encoding of the caller's digest —
    /// so its range check is the one comparison here that may stop early.
    ///
    /// # Errors
    ///
    /// [`RsaRefusal::MessageRepresentativeOutOfRange`],
    /// [`RsaRefusal::EntropyUnavailable`], [`RsaRefusal::EntropyUnusable`],
    /// [`RsaRefusal::KeyLockPoisoned`] or [`RsaRefusal::FaultDetected`].
    pub(crate) fn private_operation(
        &self,
        representative: &Uint<RSA_LIMBS>,
        entropy: &mut dyn EntropySource,
    ) -> Result<Vec<u8>, RsaRefusal> {
        if representative >= self.public.modulus.value() {
            return Err(RsaRefusal::MessageRepresentativeOutOfRange);
        }
        let pair = self.blinding_pair(entropy)?;
        self.private_operation_with(representative, &pair)
    }

    /// The private operation under one blinding pair: §5.2.1 step 2b with the
    /// blinding around it and the fault check after it.
    fn private_operation_with(
        &self,
        representative: &Uint<RSA_LIMBS>,
        pair: &BlindingPair,
    ) -> Result<Vec<u8>, RsaRefusal> {
        let n = &self.public.modulus;
        let p = self.p.modulus();
        let q = self.q.modulus();

        // c · r^e mod n: the pair's forward half is r^e · R, so one product.
        let mut blinded = Secret::<RSA_LIMBS>::zero();
        n.montgomery_secret(representative, pair.forward.value(), blinded.value_mut());

        // Step 2b ii: the two halves, each at its prime's width.
        let mut residue = Secret::zero();
        let mut m1 = Secret::zero();
        p.reduce_secret(blinded.value(), n.limbs(), residue.value_mut());
        p.pow_secret(residue.value(), self.dp.value(), m1.value_mut());
        #[cfg(test)]
        fault::strike(&mut m1);
        let mut m2 = Secret::zero();
        q.reduce_secret(blinded.value(), n.limbs(), residue.value_mut());
        q.pow_secret(residue.value(), self.dq.value(), m2.value_mut());

        // Steps 2b iv and v, then the unblinding by r^-1 · R.
        let mut joined = Secret::zero();
        self.recombine(&m1, &m2, &mut joined);
        let mut signature = Secret::zero();
        n.montgomery_secret(joined.value(), pair.inverse.value(), signature.value_mut());

        // The fault check: s^e mod n is the representative, or nothing leaves.
        let mut check = Secret::zero();
        n.pow_secret_base(signature.value(), &self.public.exponent, check.value_mut());
        if !equal_secret(check.value(), representative) {
            return Err(RsaRefusal::FaultDetected);
        }
        let mut out = vec![0u8; n.byte_len()];
        if !signature.value().to_be_bytes(&mut out) {
            // Unreachable: the result is below n, which fits k octets.
            return Err(RsaRefusal::FaultDetected);
        }
        Ok(out)
    }

    /// Garner's recombination (RFC 8017 §5.2.1 step 2b iv and v):
    /// `h = qInv · (m1 - m2) mod p` and `out = m2 + q · h`, below `n`, on the
    /// constant-time path. It joins the private operation's halves, and the
    /// blinding factor's two Fermat inverses.
    fn recombine(
        &self,
        m1: &Secret<RSA_LIMBS>,
        m2: &Secret<RSA_LIMBS>,
        out: &mut Secret<RSA_LIMBS>,
    ) {
        let n = &self.public.modulus;
        let p = self.p.modulus();
        let mut m2_mod_p = Secret::zero();
        p.reduce_secret(m2.value(), self.q.modulus().limbs(), m2_mod_p.value_mut());
        let mut difference = Secret::zero();
        p.sub_secret(m1.value(), m2_mod_p.value(), difference.value_mut());
        // qInv is held as qInv · R under p: one product is qInv · difference.
        let mut h = Secret::zero();
        p.montgomery_secret(
            self.q_inv_montgomery.value(),
            difference.value(),
            h.value_mut(),
        );
        // q as q · R under n: one product is q · h, below n since h < p.
        let mut qh = Secret::zero();
        n.montgomery_secret(self.q_montgomery.value(), h.value(), qh.value_mut());
        n.add_secret(qh.value(), m2.value(), out.value_mut());
    }

    /// The pair this operation is blinded by: drawn fresh on the key's first
    /// operation and every [`BLINDING_REFRESH`]th after it, squared in
    /// between — `(r^e)² = (r²)^e` and `(r^-1)² = (r²)^-1`, so it stays a
    /// pair — and copied out, so the lock is not held while the key computes.
    fn blinding_pair(&self, entropy: &mut dyn EntropySource) -> Result<BlindingPair, RsaRefusal> {
        let mut guard = self
            .blinding
            .lock()
            .map_err(|_| RsaRefusal::KeyLockPoisoned)?;
        let state = &mut *guard;
        let refresh = state.uses % BLINDING_REFRESH == 0;
        let pair = match &mut state.pair {
            Some(pair) if !refresh => {
                let n = &self.public.modulus;
                n.square_secret(pair.forward.value_mut());
                n.square_secret(pair.inverse.value_mut());
                pair.duplicate()
            }
            slot => {
                let fresh = self.draw_pair(entropy)?;
                let copy = fresh.duplicate();
                *slot = Some(fresh);
                copy
            }
        };
        state.uses = state.uses.wrapping_add(1);
        Ok(pair)
    }

    /// Draws `r` uniform in `[1, n)` from the caller's source — `k` octets,
    /// cut to `n`'s bit length, drawn again when zero or not below `n` — and
    /// returns `(r^e · R, r^-1 · R)`, drawing again when `r` shares a factor
    /// with `n` and so has no inverse.
    fn draw_pair(&self, entropy: &mut dyn EntropySource) -> Result<BlindingPair, RsaRefusal> {
        let n = &self.public.modulus;
        let k = n.byte_len();
        let unused = 8 * k - n.bits();
        let mut bytes = SecretBytes::new(k);
        let mut r = Secret::<RSA_LIMBS>::zero();
        let mut inverse = Secret::zero();
        let mut product = Secret::zero();
        for _ in 0..BLINDING_DRAWS {
            if !entropy.fill(bytes.as_mut_slice()) {
                return Err(RsaRefusal::EntropyUnavailable);
            }
            if let Some(first) = bytes.as_mut_slice().first_mut() {
                *first &= 0xffu8 >> unused;
            }
            if !r.value_mut().read_be_bytes(bytes.as_slice()) {
                continue;
            }
            if equal_secret(r.value(), &Uint::zero()) || !less_secret(r.value(), n.value()) {
                continue;
            }
            self.inverse(&r, &mut inverse);
            n.mul_mod_secret(r.value(), inverse.value(), product.value_mut());
            if !equal_secret(product.value(), &Uint::from_u64(1)) {
                continue;
            }
            let mut power = Secret::zero();
            n.pow_secret_base(r.value(), &self.public.exponent, power.value_mut());
            let mut forward = Secret::zero();
            n.to_montgomery_secret(power.value(), forward.value_mut());
            let mut inverse_montgomery = Secret::zero();
            n.to_montgomery_secret(inverse.value(), inverse_montgomery.value_mut());
            return Ok(BlindingPair {
                forward,
                inverse: inverse_montgomery,
            });
        }
        Err(RsaRefusal::EntropyUnusable)
    }

    /// `out = r^-1 mod n`, by Fermat's little theorem under each prime —
    /// `r^(p-2) mod p` and `r^(q-2) mod q`, secret exponents on the
    /// constant-time path — joined by [`RsaPrivateKey::recombine`]. When `r`
    /// shares a factor with `n` the result is not an inverse, and the caller
    /// checks that it is.
    fn inverse(&self, r: &Secret<RSA_LIMBS>, out: &mut Secret<RSA_LIMBS>) {
        let n_limbs = self.public.modulus.limbs();
        let mut residue = Secret::zero();
        let mut exponent = Secret::zero();
        let mut under_p = Secret::zero();
        let mut under_q = Secret::zero();
        for (prime, result) in [(&self.p, &mut under_p), (&self.q, &mut under_q)] {
            let modulus = prime.modulus();
            modulus.reduce_secret(r.value(), n_limbs, residue.value_mut());
            subtract_small_secret(modulus.value(), 2, exponent.value_mut());
            modulus.pow_secret(residue.value(), exponent.value(), result.value_mut());
        }
        self.recombine(&under_p, &under_q, out);
    }
}

impl core::fmt::Debug for RsaPrivateKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RsaPrivateKey")
            .field("modulus_bits", &self.public.modulus_bits())
            .finish_non_exhaustive()
    }
}

/// A test's way to miscompute one half of the next private operation on its
/// thread, so that the check before release has a fault to find.
#[cfg(test)]
mod fault {
    use crate::bignum::{Secret, Uint};
    use std::cell::Cell;

    thread_local! {
        static ARMED: Cell<bool> = const { Cell::new(false) };
    }

    /// The next private operation on this thread gets its first half wrong.
    pub(super) fn arm() {
        ARMED.with(|armed| armed.set(true));
    }

    /// When armed, the half becomes the half plus one, once.
    pub(super) fn strike<const N: usize>(half: &mut Secret<N>) {
        if ARMED.with(|armed| armed.replace(false)) {
            let wrong = half.value().add_carry(&Uint::from_u64(1)).0;
            *half.value_mut() = wrong;
        }
    }
}

/// The longest seed MGF1 is handed here: a digest (at most 64 octets) and the
/// four-octet counter.
const MGF_SEED_MAX: usize = 64 + 4;

/// MGF1 (RFC 8017 B.2.1), XORed into `out` in place.
///
/// `T = Hash(seed || C(0)) || Hash(seed || C(1)) || …`, cut to `out.len()`
/// octets, where `C(i)` is the counter as four big-endian octets. XORed rather
/// than returned, because the one caller wants `maskedDB ⊕ dbMask` and
/// building the mask separately would be a second 512-octet buffer.
fn mgf1_xor(hash: DigestAlgorithm, seed: &[u8], out: &mut [u8]) {
    let mut input = [0u8; MGF_SEED_MAX];
    let Some(prefix) = input.get_mut(..seed.len()) else {
        return;
    };
    prefix.copy_from_slice(seed);
    let block = hash.output_len();
    let mut counter: u32 = 0;
    for chunk in out.chunks_mut(block) {
        if let Some(slot) = input.get_mut(seed.len()..seed.len() + 4) {
            slot.copy_from_slice(&counter.to_be_bytes());
        }
        let digest = hash.digest(input.get(..seed.len() + 4).unwrap_or(&[]));
        for (byte, mask) in chunk.iter_mut().zip(digest.as_bytes()) {
            *byte ^= mask;
        }
        counter = counter.wrapping_add(1);
    }
}

/// EMSA-PSS-VERIFY (RFC 8017 §9.1.2), over a digest already taken.
///
/// Step numbers are the RFC's. Every offset below is computed from `em.len()`,
/// the digest length and the declared salt length — none is read out of the
/// block — which is what makes a forged block unable to choose where its own
/// salt starts.
fn emsa_pss_verify(
    parameters: PssParameters,
    m_hash: &[u8],
    em: &[u8],
    em_bits: usize,
) -> Result<(), RsaRefusal> {
    let h_len = parameters.hash.output_len();
    let s_len = parameters.salt_length;
    let em_len = em.len();

    // Step 3: emLen >= hLen + sLen + 2.
    let needed = h_len
        .checked_add(s_len)
        .and_then(|sum| sum.checked_add(2))
        .ok_or(RsaRefusal::ModulusTooShort)?;
    if em_len < needed {
        return Err(RsaRefusal::ModulusTooShort);
    }

    // Step 4: the trailer field, 0xbc.
    if em.last() != Some(&0xbc) {
        return Err(RsaRefusal::EncodingMismatch);
    }

    // Step 5: maskedDB is the first emLen - hLen - 1 octets, H the next hLen.
    let db_len = em_len - h_len - 1;
    let (masked_db, rest) = em.split_at(db_len);
    let Some(h) = rest.get(..h_len) else {
        return Err(RsaRefusal::EncodingMismatch);
    };

    // Step 6: the leftmost 8·emLen − emBits bits of maskedDB must be zero.
    // `em_len` is ceil(emBits / 8), so this is between 0 and 7.
    let unused = 8 * em_len - em_bits;
    let keep = 0xffu8.checked_shr(unused as u32).unwrap_or(0);
    if masked_db.first().is_some_and(|&lead| lead & !keep != 0) {
        return Err(RsaRefusal::EncodingMismatch);
    }

    // Steps 7 to 9: DB = maskedDB ⊕ MGF(H, emLen − hLen − 1), with the same
    // leftmost bits cleared.
    let mut db = [0u8; MAX_RSA_BYTES];
    let Some(db) = db.get_mut(..db_len) else {
        return Err(RsaRefusal::ModulusTooWide);
    };
    db.copy_from_slice(masked_db);
    mgf1_xor(parameters.mask_hash, h, db);
    if let Some(lead) = db.first_mut() {
        *lead &= keep;
    }

    // Step 10: emLen − hLen − sLen − 2 zero octets, then 0x01. `needed` above
    // makes this subtraction safe.
    let ps_len = em_len - h_len - s_len - 2;
    let (padding, tail) = db.split_at(ps_len);
    if padding.iter().any(|&byte| byte != 0) || tail.first() != Some(&0x01) {
        return Err(RsaRefusal::EncodingMismatch);
    }

    // Step 11: the salt is the last sLen octets of DB.
    let salt = tail.get(1..).unwrap_or(&[]);
    if salt.len() != s_len {
        // Unreachable by the arithmetic above; refused rather than asserted.
        return Err(RsaRefusal::EncodingMismatch);
    }

    // Steps 12 and 13: H' = Hash(0x00 × 8 || mHash || salt).
    let mut prime = [0u8; 8 + 64 + MAX_RSA_BYTES];
    let prime_len = 8 + h_len + s_len;
    if !put(&mut prime, 8, m_hash) || !put(&mut prime, 8 + h_len, salt) {
        return Err(RsaRefusal::ModulusTooWide);
    }
    let h_prime = parameters
        .hash
        .digest(prime.get(..prime_len).unwrap_or(&[]));

    // Step 14.
    if constant_time_eq(h, h_prime.as_bytes()) {
        Ok(())
    } else {
        Err(RsaRefusal::EncodingMismatch)
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
            "SHA224" => Some(DigestAlgorithm::Sha224),
            "SHA256" => Some(DigestAlgorithm::Sha256),
            "SHA384" => Some(DigestAlgorithm::Sha384),
            "SHA512" => Some(DigestAlgorithm::Sha512),
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
        for algorithm in EVERY_DIGEST {
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
        // a structural check cannot catch: the last arc of the OID, NIST's
        // hashAlgs arc 1 to 6 (RFC 8017 appendix A.2.4).
        for (algorithm, arc) in [
            (DigestAlgorithm::Sha256, 0x01),
            (DigestAlgorithm::Sha384, 0x02),
            (DigestAlgorithm::Sha512, 0x03),
            (DigestAlgorithm::Sha224, 0x04),
            (DigestAlgorithm::Sha512_224, 0x05),
            (DigestAlgorithm::Sha512_256, 0x06),
        ] {
            assert_eq!(
                algorithm.digest_info_prefix().get(14),
                Some(&arc),
                "{algorithm:?}"
            );
        }
        // No two members share a prefix, and each digest is the length its
        // own function returns.
        for (index, algorithm) in EVERY_DIGEST.iter().enumerate() {
            for other in EVERY_DIGEST.iter().skip(index + 1) {
                assert_ne!(algorithm.digest_info_prefix(), other.digest_info_prefix());
            }
            assert_eq!(
                algorithm.digest(b"abc").as_bytes().len(),
                algorithm.output_len()
            );
        }
    }

    const EVERY_DIGEST: [DigestAlgorithm; 7] = [
        DigestAlgorithm::Sha1,
        DigestAlgorithm::Sha224,
        DigestAlgorithm::Sha256,
        DigestAlgorithm::Sha384,
        DigestAlgorithm::Sha512,
        DigestAlgorithm::Sha512_224,
        DigestAlgorithm::Sha512_256,
    ];

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
    /// vector, under all five of its digests, positive and negative alike.
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
                    let algorithm = algorithm.expect("every group names a DigestAlgorithm member");
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

        assert_eq!(
            ran, 450,
            "vectors run, the 90 of the SHA-224 groups among them"
        );
        assert_eq!(accepted, 75, "Result = P");
        assert_eq!(rejected, 375, "Result = F");
    }

    // ---- RSASSA-PSS ------------------------------------------------------

    /// NIST CAVP `SigVerPSS_186-3.rsp`, from the same archive as the PKCS#1
    /// v1.5 file: every vector, under all five of its digests.
    ///
    /// The salt length is read from each vector's `SaltVal`, which is ten
    /// octets throughout, and handed to the verifier as the declared length —
    /// the same thing a CMS `RSASSA-PSS-params` does. The negatives are NIST's
    /// own five kinds, 75 of each across the five digests: the message
    /// changed, `e` changed, the signature changed, the hash moved left in
    /// the encoded message, and the `0x01` that ends the padding removed. The
    /// last two are forgeries against the unmasking, which is the part of PSS
    /// that can be read wrongly.
    #[test]
    fn cavp_sigver_pss_vectors() {
        const VECTORS: &str = include_str!("../tests/data/cavp/rsa_sigver_pss.rsp");

        let mut base: Option<RsaPublicKey> = None;
        let mut algorithm: Option<DigestAlgorithm> = None;
        let mut exponent = Vec::new();
        let mut message = Vec::new();
        let mut signature = Vec::new();
        let mut salt = Vec::new();

        let mut ran = 0usize;
        let mut accepted = 0usize;
        let mut rejected = 0usize;
        let mut by_reason = std::collections::BTreeMap::<String, usize>::new();

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
                "SaltVal" => salt = unhex(value),
                "Result" => {
                    let algorithm = algorithm.expect("every group names a DigestAlgorithm member");
                    let expected_valid = value.starts_with('P');
                    let base = base.as_ref().expect("a modulus precedes every vector");
                    let parameters = PssParameters {
                        hash: algorithm,
                        mask_hash: algorithm,
                        salt_length: salt.len(),
                    };
                    let outcome = base
                        .with_exponent(&exponent)
                        .and_then(|key| key.verify_pss_message(parameters, &message, &signature));
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
                        *by_reason
                            .entry(value.chars().take(5).collect())
                            .or_default() += 1;
                    }
                }
                _ => {}
            }
        }

        assert_eq!(
            ran, 450,
            "vectors run, the 90 of the SHA-224 groups among them"
        );
        assert_eq!(accepted, 75, "Result = P");
        assert_eq!(rejected, 375, "Result = F");
        // Seventy-five of each of NIST's five reasons, so a parser that
        // matched only some of them cannot reach the right total another way.
        assert_eq!(by_reason.len(), 5, "{by_reason:?}");
        assert!(
            by_reason.values().all(|&count| count == 75),
            "{by_reason:?}"
        );
    }

    /// One `pss-vect.txt` example: a key, and the signatures made with it.
    struct PssExample {
        modulus: Vec<u8>,
        exponent: Vec<u8>,
        /// The private key's parts, in the order the file's "Private key"
        /// section prints them: `d`, `p`, `q`, `dP`, `dQ`, `qInv`.
        private: [Vec<u8>; 6],
        /// (message, salt, signature)
        signatures: Vec<(Vec<u8>, Vec<u8>, Vec<u8>)>,
    }

    /// Reads RSA Laboratories' `pss-vect.txt`: labelled blocks of hex octets.
    ///
    /// The public key is read from each example's "Public key" section,
    /// where the exponent is labelled `Exponent`; the private-key section
    /// reuses that label for `d`, which is why only the first `Modulus` and
    /// `Exponent` of each example are taken.
    fn pss_examples() -> Vec<PssExample> {
        const VECTORS: &str = include_str!("../tests/data/pkcs1/pss-vect.txt");
        let mut examples: Vec<PssExample> = Vec::new();
        let mut label = String::new();
        let mut current = Vec::<u8>::new();
        let mut message = Vec::new();
        let mut salt = Vec::new();

        let mut flush = |label: &str, value: Vec<u8>, examples: &mut Vec<PssExample>| {
            let Some(example) = examples.last_mut() else {
                return;
            };
            match label {
                "Modulus" if example.modulus.is_empty() => example.modulus = value,
                "Exponent" if example.exponent.is_empty() => example.exponent = value,
                // The private section's own "Exponent" is d.
                "Exponent" => example.private[0] = value,
                "Prime 1" => example.private[1] = value,
                "Prime 2" => example.private[2] = value,
                "Prime exponent 1" => example.private[3] = value,
                "Prime exponent 2" => example.private[4] = value,
                "Coefficient" => example.private[5] = value,
                "Message to be signed" => message = value,
                "Salt" => salt = value,
                "Signature" => {
                    example
                        .signatures
                        .push((message.clone(), salt.clone(), value));
                }
                _ => {}
            }
        };

        for line in VECTORS.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix('#') {
                let rest = rest.trim();
                if !label.is_empty() {
                    flush(&label, std::mem::take(&mut current), &mut examples);
                }
                label.clear();
                current.clear();
                if rest.starts_with("Example ") && rest.contains("RSA key pair") {
                    examples.push(PssExample {
                        modulus: Vec::new(),
                        exponent: Vec::new(),
                        private: Default::default(),
                        signatures: Vec::new(),
                    });
                } else if let Some(name) = rest.strip_suffix(':') {
                    label = name.trim().to_string();
                }
                continue;
            }
            if label.is_empty() || line.is_empty() {
                continue;
            }
            current.extend(unhex(&line.replace(' ', "")));
        }
        if !label.is_empty() {
            flush(&label, current, &mut examples);
        }
        examples
    }

    /// RSA Laboratories' `pss-vect.txt`: sixty valid signatures under ten
    /// keys, and each one spoiled four ways that must all be refused.
    ///
    /// The file's reason to be here beside NIST's is the key sizes. Seven of
    /// its ten moduli are 1 025 to 1 031 bits, so `emBits = modBits − 1` is
    /// not one less than a multiple of eight: at 1 025 bits the encoded
    /// message is 128 octets under a 129-octet signature, and at 1 026 to
    /// 1 031 the leftmost octet of `maskedDB` has bits that must be zero.
    /// NIST's moduli are all whole octets, so a verifier that took `emLen` to
    /// be `k` passes every CAVP vector and fails here.
    #[test]
    fn rsa_laboratories_pss_vectors() {
        let examples = pss_examples();
        assert_eq!(examples.len(), 10, "ten keys");

        let mut bits = Vec::new();
        let mut verified = 0usize;
        let mut refused = 0usize;
        for (index, example) in examples.iter().enumerate() {
            let key = RsaPublicKey::new(&example.modulus, &example.exponent)
                .expect("an RSA Laboratories key is usable");
            bits.push(key.modulus_bits());
            assert_eq!(example.signatures.len(), 6, "example {}", index + 1);
            for (number, (message, salt, signature)) in example.signatures.iter().enumerate() {
                let parameters = PssParameters {
                    hash: DigestAlgorithm::Sha1,
                    mask_hash: DigestAlgorithm::Sha1,
                    salt_length: salt.len(),
                };
                assert_eq!(salt.len(), 20);
                assert_eq!(
                    key.verify_pss_message(parameters, message, signature),
                    Ok(()),
                    "example {}.{}",
                    index + 1,
                    number + 1
                );
                verified += 1;

                // A different message.
                let mut other = message.clone();
                if let Some(byte) = other.first_mut() {
                    *byte ^= 0x01;
                }
                assert!(key
                    .verify_pss_message(parameters, &other, signature)
                    .is_err());
                // One bit of the signature.
                let mut spoiled = signature.clone();
                if let Some(byte) = spoiled.last_mut() {
                    *byte ^= 0x01;
                }
                assert!(key
                    .verify_pss_message(parameters, message, &spoiled)
                    .is_err());
                // The right signature read with a salt one octet longer or
                // shorter than it was made with: the declared length is what
                // decides where the padding ends.
                for wrong in [salt.len() - 1, salt.len() + 1] {
                    let wrong = PssParameters {
                        salt_length: wrong,
                        ..parameters
                    };
                    assert_eq!(
                        key.verify_pss_message(wrong, message, signature),
                        Err(RsaRefusal::EncodingMismatch),
                        "example {}.{} with a {}-octet salt",
                        index + 1,
                        number + 1,
                        wrong.salt_length
                    );
                }
                refused += 4;
            }
        }
        assert_eq!(verified, 60);
        assert_eq!(refused, 240);
        assert_eq!(
            bits,
            [1024, 1025, 1026, 1027, 1028, 1029, 1030, 1031, 1536, 2048],
            "the key sizes the file is here for"
        );
    }

    /// Four signatures whose MGF1 hash is not their message hash — SHA-256
    /// under MGF1-SHA-1, SHA-1 under MGF1-SHA-256, SHA-512 under MGF1-SHA-256
    /// and SHA-384 under MGF1-SHA-512 — made once by OpenSSL 3.0.13
    /// (`tests/data/openssl/pss-mgf1.txt`, its header and the script beside it
    /// say how).
    ///
    /// They are here because neither published set has one: NIST's
    /// `SigVerPSS` and RSA Laboratories' `pss-vect.txt` both mask with the
    /// message hash, so a verifier that unmasked `maskedDB` with the wrong one
    /// of RFC 8017 §9.1.2 step 8's two hashes passed all 420 of their vectors —
    /// a counted injection measured it at zero. Each signature is read under
    /// its own parameters, and then under the message hash as the mask hash,
    /// which must refuse it: the same octets unmasked with another hash are
    /// not a block.
    #[test]
    fn the_mask_is_generated_with_the_mask_hash_and_not_the_message_hash() {
        const VECTORS: &str = include_str!("../tests/data/openssl/pss-mgf1.txt");

        let mut modulus = Vec::new();
        let mut exponent = Vec::new();
        let mut message = Vec::new();
        let mut hash = None;
        let mut mask_hash = None;
        let mut salt_length = 0usize;
        let mut cases = Vec::new();
        for line in VECTORS.lines() {
            let Some((name, value)) = line.split_once(" = ") else {
                continue;
            };
            match name {
                "n" => modulus = unhex(value),
                "e" => exponent = unhex(value),
                "Msg" => message = unhex(value),
                "Hash" => hash = algorithm_named(value),
                "MGF1" => mask_hash = algorithm_named(value),
                "SaltLength" => salt_length = value.parse().expect("a salt length"),
                "S" => cases.push((
                    PssParameters {
                        hash: hash.expect("a hash precedes every signature"),
                        mask_hash: mask_hash.expect("an MGF1 hash precedes every signature"),
                        salt_length,
                    },
                    unhex(value),
                )),
                _ => {}
            }
        }
        assert_eq!(cases.len(), 4, "signatures read");
        let key = RsaPublicKey::new(&modulus, &exponent).expect("OpenSSL's key is usable");
        assert_eq!(key.modulus_bits(), 2048);

        for (parameters, signature) in &cases {
            assert_ne!(
                parameters.hash, parameters.mask_hash,
                "every case is here for the two hashes differing"
            );
            assert_eq!(
                key.verify_pss_message(*parameters, &message, signature),
                Ok(()),
                "{parameters:?}"
            );
            let one_hash = PssParameters {
                mask_hash: parameters.hash,
                ..*parameters
            };
            assert_eq!(
                key.verify_pss_message(one_hash, &message, signature),
                Err(RsaRefusal::EncodingMismatch),
                "{parameters:?} unmasked with the message hash"
            );
            let mut spoiled = signature.clone();
            if let Some(byte) = spoiled.last_mut() {
                *byte ^= 0x01;
            }
            assert!(key
                .verify_pss_message(*parameters, &message, &spoiled)
                .is_err());
        }
    }

    /// MGF1 against its definition written out a second way: block `i` is
    /// `Hash(seed || i as four big-endian octets)`, and the mask is their
    /// concatenation cut to length.
    #[test]
    fn mgf1_is_the_counter_mode_hash_b_2_1_describes() {
        let seed = b"tinker-pdf";
        let mut mask = [0u8; 50];
        mgf1_xor(DigestAlgorithm::Sha1, seed, &mut mask);
        let mut expected = Vec::new();
        for counter in 0u8..3 {
            let mut block = seed.to_vec();
            block.extend_from_slice(&[0, 0, 0, counter]);
            expected.extend_from_slice(&sha1(&block));
        }
        assert_eq!(&mask[..], &expected[..50]);
    }

    /// Two checks of RFC 8017 §9.1.2 that no published vector reaches: step 6,
    /// the bits of `EM` above `emBits` must be zero *before* unmasking rather
    /// than merely cleared after it; and step 4, the trailer octet.
    ///
    /// A signer following EMSA-PSS-ENCODE cannot produce a block that breaks
    /// either while keeping `H` right, so a verifier that skipped both passed
    /// all 420 vectors — a counted injection measured each at zero. This takes RSA Laboratories'
    /// example 3, a 1 026-bit key whose `emBits` of 1 025 leaves seven bits of
    /// `EM`'s first octet outside the encoding, recovers the real `EM` with
    /// the public key, and sets each of the seven in turn. Step 9 would clear
    /// every one of them again before the padding is read, which is exactly
    /// why the check has to come first.
    #[test]
    fn a_block_a_signer_could_not_have_written_is_refused_even_where_h_matches() {
        let examples = pss_examples();
        let example = examples.get(2).expect("example 3");
        let key = RsaPublicKey::new(&example.modulus, &example.exponent).expect("usable");
        assert_eq!(key.modulus_bits(), 1026);
        let (message, salt, signature) = example.signatures.first().expect("a signature");
        let parameters = PssParameters {
            hash: DigestAlgorithm::Sha1,
            mask_hash: DigestAlgorithm::Sha1,
            salt_length: salt.len(),
        };
        let digest = DigestAlgorithm::Sha1.digest(message);

        let s = Uint::<RSA_LIMBS>::from_be_bytes(signature).expect("fits");
        let m = key.modulus.pow(&s, &key.exponent);
        let mut whole = vec![0u8; key.modulus.byte_len()];
        assert!(m.to_be_bytes(&mut whole));
        let em = &whole[whole.len() - 129..];
        assert_eq!(em[0] & 0xfe, 0, "the encoding leaves the seven bits clear");
        assert_eq!(
            emsa_pss_verify(parameters, digest.as_bytes(), em, 1025),
            Ok(()),
            "the recovered block verifies as it stands"
        );

        for bit in 1..8u32 {
            let mut spoiled = em.to_vec();
            spoiled[0] |= 1 << bit;
            assert_eq!(
                emsa_pss_verify(parameters, digest.as_bytes(), &spoiled, 1025),
                Err(RsaRefusal::EncodingMismatch),
                "bit {bit} of the first octet"
            );
        }

        // Step 4, on the same block: the trailer is not part of `H` or of the
        // salt, so a block whose last octet is anything but `0xbc` and whose
        // other octets are a real encoding still reaches a matching `H'`. No
        // published negative is that shape either — NIST's "signature
        // changed" vectors fail at `H` long before the trailer matters.
        for trailer in [0x00u8, 0xbd, 0xcc, 0xff] {
            let mut spoiled = em.to_vec();
            if let Some(last) = spoiled.last_mut() {
                *last = trailer;
            }
            assert_eq!(
                emsa_pss_verify(parameters, digest.as_bytes(), &spoiled, 1025),
                Err(RsaRefusal::EncodingMismatch),
                "trailer 0x{trailer:02x}"
            );
        }
    }

    // ---- RSAES-PKCS1-v1_5 encryption ---------------------------------------

    /// One `pkcs1v15crypt-vectors.txt` example: a key and its twenty
    /// encryptions, each `(message, padding, ciphertext)`.
    struct EncryptionExample {
        modulus: Vec<u8>,
        exponent: Vec<u8>,
        encryptions: Vec<(Vec<u8>, Vec<u8>, Vec<u8>)>,
    }

    /// Reads RSA Laboratories' encryption vectors: the same labelled hex
    /// blocks as `pss-vect.txt`, with `Seed` the padding `PS` and the private
    /// section's second `Modulus` and `Exponent` ignored for the same reason.
    fn encryption_examples() -> Vec<EncryptionExample> {
        const VECTORS: &str = include_str!("../tests/data/pkcs1/pkcs1v15crypt-vectors.txt");
        let mut examples: Vec<EncryptionExample> = Vec::new();
        let mut label = String::new();
        let mut current = Vec::<u8>::new();
        let mut message = Vec::new();
        let mut seed = Vec::new();

        let mut flush = |label: &str, value: Vec<u8>, examples: &mut Vec<EncryptionExample>| {
            let Some(example) = examples.last_mut() else {
                return;
            };
            match label {
                "Modulus" if example.modulus.is_empty() => example.modulus = value,
                "Exponent" if example.exponent.is_empty() => example.exponent = value,
                "Message" => message = value,
                "Seed" => seed = value,
                "Encryption" => {
                    example
                        .encryptions
                        .push((message.clone(), seed.clone(), value));
                }
                _ => {}
            }
        };

        for line in VECTORS.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix('#') {
                let rest = rest.trim();
                if !label.is_empty() {
                    flush(&label, std::mem::take(&mut current), &mut examples);
                }
                label.clear();
                current.clear();
                if rest.starts_with("Example ") && rest.contains("RSA key pair") {
                    examples.push(EncryptionExample {
                        modulus: Vec::new(),
                        exponent: Vec::new(),
                        encryptions: Vec::new(),
                    });
                } else if let Some(name) = rest.strip_suffix(':') {
                    label = name.trim().to_string();
                }
                continue;
            }
            if label.is_empty() || line.is_empty() {
                continue;
            }
            current.extend(unhex(&line.replace(' ', "")));
        }
        if !label.is_empty() {
            flush(&label, current, &mut examples);
        }
        examples
    }

    /// All 300 of RSA Laboratories' RSAES-PKCS1-v1_5 encryptions, octet for
    /// octet, under fifteen keys from 1 024 to 2 048 bits.
    ///
    /// Exact known answers, because the file publishes the padding each one
    /// was made with: the ciphertext is a function of the key, the message
    /// and `PS`, and this computes it and compares every octet. Seven of the
    /// keys are 1 025 to 1 031 bits, whose leading octet is not full — the
    /// case where an encryptor that sized `EM` from the bit length rather than
    /// the octet length would put `0x02` one octet early.
    #[test]
    fn rsa_laboratories_pkcs1_v15_encryption_vectors() {
        let examples = encryption_examples();
        assert_eq!(examples.len(), 15, "fifteen keys");
        let mut bits = Vec::new();
        let mut matched = 0usize;
        for (index, example) in examples.iter().enumerate() {
            let key = RsaPublicKey::new(&example.modulus, &example.exponent)
                .expect("an RSA Laboratories key is usable");
            bits.push(key.modulus_bits());
            assert_eq!(example.encryptions.len(), 20, "example {}", index + 1);
            for (number, (message, padding, ciphertext)) in example.encryptions.iter().enumerate() {
                assert_eq!(
                    key.pkcs1_v15_padding_len(message.len()),
                    Some(padding.len()),
                    "example {}.{}: k - 3 - mLen",
                    index + 1,
                    number + 1
                );
                assert_eq!(
                    key.encrypt_pkcs1_v15(message, padding).as_deref(),
                    Ok(&ciphertext[..]),
                    "example {}.{}",
                    index + 1,
                    number + 1
                );
                matched += 1;
            }
        }
        assert_eq!(matched, 300);
        assert_eq!(
            bits,
            [
                1024, 1024, 1024, 1024, 1024, 1024, 1025, 1026, 1027, 1028, 1029, 1030, 1031, 1536,
                2048
            ],
            "the key sizes the file carries"
        );
    }

    /// Each way a caller can hand over something RFC 8017 §7.2.1 does not
    /// allow is refused by name, and none reaches the arithmetic.
    #[test]
    fn encryption_refuses_what_the_scheme_does_not_allow() {
        let examples = encryption_examples();
        let example = examples.first().expect("an example");
        let key = RsaPublicKey::new(&example.modulus, &example.exponent).expect("usable");
        let (message, padding, _) = example.encryptions.first().expect("an encryption");

        // One octet too few, one too many.
        assert_eq!(
            key.encrypt_pkcs1_v15(message, &padding[1..]),
            Err(RsaRefusal::PaddingUnusable)
        );
        let mut longer = padding.clone();
        longer.push(0x5A);
        assert_eq!(
            key.encrypt_pkcs1_v15(message, &longer),
            Err(RsaRefusal::PaddingUnusable)
        );
        // A zero octet anywhere in PS would end it there.
        for at in [0, padding.len() / 2, padding.len() - 1] {
            let mut zeroed = padding.clone();
            zeroed[at] = 0;
            assert_eq!(
                key.encrypt_pkcs1_v15(message, &zeroed),
                Err(RsaRefusal::PaddingUnusable),
                "a zero at {at}"
            );
        }
        // A 1024-bit key carries at most 117 octets.
        assert_eq!(key.pkcs1_v15_padding_len(117), Some(8));
        assert_eq!(key.pkcs1_v15_padding_len(118), None);
        assert_eq!(
            key.encrypt_pkcs1_v15(&[0x41; 118], &[0x01; 7]),
            Err(RsaRefusal::MessageTooLong { capacity: 117 })
        );
        // The longest message it can carry, with the minimum eight octets.
        assert!(key.encrypt_pkcs1_v15(&[0x41; 117], &[0x01; 8]).is_ok());
    }

    /// A salt length no modulus of this size could carry is a refusal about
    /// the key and the parameters — not a panic, and not "invalid signature".
    #[test]
    fn an_impossible_salt_length_is_refused_by_name() {
        let examples = pss_examples();
        let example = examples.first().expect("the file has examples");
        let key = RsaPublicKey::new(&example.modulus, &example.exponent).expect("usable");
        let (message, _, signature) = example.signatures.first().expect("a signature");
        // A 1024-bit key's emLen is 128, so a 20-octet digest leaves room for
        // a salt of at most 106.
        for salt_length in [107, 1000, usize::MAX] {
            let parameters = PssParameters {
                hash: DigestAlgorithm::Sha1,
                mask_hash: DigestAlgorithm::Sha1,
                salt_length,
            };
            assert_eq!(
                key.verify_pss_message(parameters, message, signature),
                Err(RsaRefusal::ModulusTooShort),
                "salt {salt_length} under a 1024-bit key"
            );
        }
    }

    // ---- The private key ---------------------------------------------------

    use crate::bignum::dropped;

    /// A deterministic stand-in for a host's source: SHA-256 over a seed and
    /// a counter, as many blocks as a buffer asks for. Any source serves —
    /// blinding moves no result — and a fixed one makes a failure repeatable.
    struct Stream {
        seed: u64,
        counter: u64,
    }

    impl Stream {
        fn new(seed: u64) -> Self {
            Self { seed, counter: 0 }
        }
    }

    impl EntropySource for Stream {
        fn fill(&mut self, out: &mut [u8]) -> bool {
            for chunk in out.chunks_mut(32) {
                let mut input = [0u8; 16];
                input[..8].copy_from_slice(&self.seed.to_be_bytes());
                input[8..].copy_from_slice(&self.counter.to_be_bytes());
                let block = sha2::sha256(&input);
                chunk.copy_from_slice(&block[..chunk.len()]);
                self.counter += 1;
            }
            true
        }
    }

    /// A source with nothing to give.
    struct Exhausted;

    impl EntropySource for Exhausted {
        fn fill(&mut self, _: &mut [u8]) -> bool {
            false
        }
    }

    /// A source that gives the same bytes, right-aligned in zeros, for its
    /// first `times` draws — or for ever, with no `then` — and after them
    /// hands over to `then`.
    struct Fixed {
        bytes: Vec<u8>,
        times: usize,
        then: Option<Stream>,
    }

    impl EntropySource for Fixed {
        fn fill(&mut self, out: &mut [u8]) -> bool {
            if self.times == 0 {
                if let Some(stream) = &mut self.then {
                    return stream.fill(out);
                }
            } else {
                self.times -= 1;
            }
            for byte in out.iter_mut() {
                *byte = 0;
            }
            let start = out.len().saturating_sub(self.bytes.len());
            let tail = self.bytes.len() - (out.len() - start);
            out[start..].copy_from_slice(&self.bytes[tail..]);
            true
        }
    }

    /// `value mod modulus` one bit at a time, for any modulus: the CRT
    /// exponents are reduced modulo `p - 1`, which Montgomery cannot take.
    fn remainder(value: &Uint<RSA_LIMBS>, modulus: &Uint<RSA_LIMBS>) -> Uint<RSA_LIMBS> {
        let mut remainder = Uint::<RSA_LIMBS>::zero();
        for i in (0..value.bits()).rev() {
            let (doubled, carried) = remainder.add_carry(&remainder);
            let mut next = doubled;
            if value.bit(i) {
                next = next.add_carry(&Uint::from_u64(1)).0;
            }
            remainder = if carried || next >= *modulus {
                next.sub_borrow(modulus).0
            } else {
                next
            };
        }
        remainder
    }

    /// A value's big-endian octets, without leading zeros.
    fn be(value: &Uint<RSA_LIMBS>) -> Vec<u8> {
        let mut bytes = vec![0u8; MAX_RSA_BYTES];
        assert!(value.to_be_bytes(&mut bytes));
        let zeros = bytes.iter().take_while(|&&byte| byte == 0).count();
        bytes.split_off(zeros.min(MAX_RSA_BYTES - 1))
    }

    fn uint(bytes: &[u8]) -> Uint<RSA_LIMBS> {
        Uint::from_be_bytes(bytes).expect("fits 4096 bits")
    }

    /// One published key, its eight parts as octets, and the signatures the
    /// file says it made.
    struct VectorKey {
        parts: [Vec<u8>; 8],
        signatures: Vec<Vec<u8>>,
    }

    impl VectorKey {
        fn components(&self) -> RsaPrivateComponents<'_> {
            let [n, e, d, p, q, dp, dq, q_inv] = &self.parts;
            RsaPrivateComponents {
                modulus: n,
                public_exponent: e,
                private_exponent: d,
                prime1: p,
                prime2: q,
                exponent1: dp,
                exponent2: dq,
                coefficient: q_inv,
            }
        }

        fn key(&self) -> RsaPrivateKey {
            RsaPrivateKey::new(&self.components()).expect("a published key is a key")
        }
    }

    /// The fifteen keys of CAVP's `SigVer15` file: `n`, `p` and `q` under
    /// each `[mod = …]`, `e` and `d` per vector — three exponent pairs a
    /// modulus — with the signatures of its `Result = P` vectors. The file
    /// publishes no CRT parts, so they are computed here from `d`, `p` and
    /// `q`: `dP` and `dQ` by long division, `qInv` by Fermat under `p`.
    fn sigver15_keys() -> Vec<VectorKey> {
        const VECTORS: &str = include_str!("../tests/data/cavp/rsa_sigver15.rsp");
        let mut keys: Vec<VectorKey> = Vec::new();
        let (mut n, mut p, mut q) = (Vec::new(), Vec::new(), Vec::new());
        let (mut e, mut d, mut signature) = (Vec::new(), Vec::new(), Vec::new());
        for line in VECTORS.lines() {
            let Some((name, value)) = line.trim().split_once(" = ") else {
                continue;
            };
            match name {
                "n" => n = unhex(value),
                "p" => p = unhex(value),
                "q" => q = unhex(value),
                "e" => e = unhex(value),
                "d" => d = unhex(value),
                "S" => signature = unhex(value),
                "Result" if value.starts_with('P') => {
                    let (p_u, q_u, d_u) = (uint(&p), uint(&q), uint(&d));
                    let one = Uint::from_u64(1);
                    let dp = remainder(&d_u, &p_u.sub_borrow(&one).0);
                    let dq = remainder(&d_u, &q_u.sub_borrow(&one).0);
                    let q_inv = Modulus::new(p_u)
                        .and_then(|modulus| modulus.inverse_prime(&q_u))
                        .expect("q is invertible under the prime p");
                    let parts = [
                        n.clone(),
                        e.clone(),
                        d.clone(),
                        p.clone(),
                        q.clone(),
                        be(&dp),
                        be(&dq),
                        be(&q_inv),
                    ];
                    match keys.iter_mut().find(|key| key.parts == parts) {
                        Some(key) => key.signatures.push(signature.clone()),
                        None => keys.push(VectorKey {
                            parts,
                            signatures: vec![signature.clone()],
                        }),
                    }
                }
                _ => {}
            }
        }
        keys
    }

    /// The ten keys of RSA Laboratories' `pss-vect.txt`, every part
    /// published, with their six signatures each.
    fn pss_vector_keys() -> Vec<VectorKey> {
        pss_examples()
            .into_iter()
            .map(|example| {
                let [d, p, q, dp, dq, q_inv] = example.private;
                VectorKey {
                    parts: [example.modulus, example.exponent, d, p, q, dp, dq, q_inv],
                    signatures: example
                        .signatures
                        .into_iter()
                        .map(|(_, _, signature)| signature)
                        .collect(),
                }
            })
            .collect()
    }

    /// The message representative a published signature was made from:
    /// `s^e mod n`, by the public path.
    fn representative(key: &RsaPrivateKey, signature: &[u8]) -> Uint<RSA_LIMBS> {
        key.public
            .modulus
            .pow(&uint(signature), &key.public.exponent)
    }

    impl RsaPrivateKey {
        /// `c^d mod n` by the first representation: no halves, no blinding,
        /// the exponentiation the constant-time path's at `n`'s width.
        fn private_operation_plain(&self, representative: &Uint<RSA_LIMBS>) -> Vec<u8> {
            let mut s = Secret::<RSA_LIMBS>::zero();
            self.public
                .modulus
                .pow_secret(representative, self.d.value(), s.value_mut());
            let mut out = vec![0u8; self.public.modulus.byte_len()];
            assert!(s.value().to_be_bytes(&mut out));
            out
        }

        /// The pair for `r = 1`, Montgomery's one twice: blinding by it moves
        /// nothing, which is what an unblinded run is.
        fn unblinded_pair(&self) -> BlindingPair {
            let mut forward = Secret::zero();
            self.public
                .modulus
                .to_montgomery_secret(&Uint::from_u64(1), forward.value_mut());
            BlindingPair {
                inverse: forward.duplicate(),
                forward,
            }
        }
    }

    /// SG-01.4's first exit, on every key the committed vectors publish: the
    /// fifteen of CAVP's `SigVer15` and the ten of `pss-vect.txt`, 1 024 to
    /// 4 096 bits, seven of them a few bits past a whole octet.
    ///
    /// For each of the 135 published signatures `s`, the representative it
    /// was made from is `s^e mod n`, and the CRT private operation on it gives
    /// `s` back octet for octet: published data, not the engine agreeing with
    /// itself. Then once a key, the plain `c^d mod n` agrees with the CRT
    /// result, and `0`, `1` and `n - 1` come back as themselves, `d` being
    /// odd.
    #[test]
    fn crt_equals_the_plain_private_operation_on_every_vector_key() {
        let mut entropy = Stream::new(0x5347_2d30_312e_34);
        let sigver = sigver15_keys();
        let pss = pss_vector_keys();
        assert_eq!(sigver.len(), 15, "five moduli, three exponent pairs each");
        assert_eq!(pss.len(), 10);
        let mut published = 0usize;
        let mut bits = Vec::new();
        for vector in sigver.iter().chain(&pss) {
            let key = vector.key();
            bits.push(key.modulus_bits());
            for signature in &vector.signatures {
                let c = representative(&key, signature);
                assert_eq!(
                    key.private_operation(&c, &mut entropy).as_ref(),
                    Ok(signature),
                    "a {}-bit key",
                    key.modulus_bits()
                );
                published += 1;
            }
            let first = vector.signatures.first().expect("a signature a key");
            let c = representative(&key, first);
            assert_eq!(
                key.private_operation(&c, &mut entropy),
                Ok(key.private_operation_plain(&c)),
                "a {}-bit key",
                key.modulus_bits()
            );
            let (n_minus_one, _) = key.public.modulus.value().sub_borrow(&Uint::from_u64(1));
            for c in [Uint::zero(), Uint::from_u64(1), n_minus_one] {
                let mut expected = vec![0u8; key.public.modulus.byte_len()];
                assert!(c.to_be_bytes(&mut expected));
                assert_eq!(key.private_operation(&c, &mut entropy), Ok(expected));
            }
        }
        assert_eq!(published, 75 + 60);
        bits.sort_unstable();
        bits.dedup();
        assert_eq!(
            bits,
            [1024, 1025, 1026, 1027, 1028, 1029, 1030, 1031, 1536, 2048, 3072, 4096]
        );
    }

    /// SG-01.4's second exit: blinding moves nothing. Every published
    /// signature of one key comes out the same blinded by three different
    /// sources as unblinded, and seventy operations in a row on one key from
    /// one source — a draw, thirty-one squarings, a draw, thirty-one more, a
    /// third draw — each give the unblinded result.
    #[test]
    fn a_blinded_run_equals_an_unblinded_one() {
        let vector = pss_vector_keys().remove(1);
        let key = vector.key();
        let unblinded = key.unblinded_pair();
        for signature in &vector.signatures {
            let c = representative(&key, signature);
            let plain = key.private_operation_with(&c, &unblinded);
            assert_eq!(plain.as_ref(), Ok(signature));
            for seed in 1..4 {
                assert_eq!(key.private_operation(&c, &mut Stream::new(seed)), plain);
            }
        }

        let key = vector.key();
        let c = representative(&key, vector.signatures.first().expect("a signature"));
        let expected = key.private_operation_with(&c, &unblinded);
        let mut entropy = Stream::new(99);
        for operation in 0..70 {
            assert_eq!(
                key.private_operation(&c, &mut entropy),
                expected,
                "operation {operation}"
            );
        }
        let uses = key.blinding.lock().map(|state| state.uses).unwrap_or(0);
        assert_eq!(uses, 70);
    }

    /// SG-01.4's fault check: a half miscomputed in the middle of an
    /// operation is refused, and the key goes on signing correctly after it;
    /// and a key whose `dP` is in range and wrong — which no check at
    /// construction can see — is refused on every operation, never once
    /// returning its faulty result.
    #[test]
    fn a_fault_in_a_half_is_refused_before_it_leaves() {
        let vector = pss_vector_keys().remove(0);
        let key = vector.key();
        let signature = vector.signatures.first().expect("a signature");
        let c = representative(&key, signature);
        let mut entropy = Stream::new(5);

        fault::arm();
        assert_eq!(
            key.private_operation(&c, &mut entropy),
            Err(RsaRefusal::FaultDetected)
        );
        assert_eq!(
            key.private_operation(&c, &mut entropy).as_ref(),
            Ok(signature)
        );

        let mut parts = vector.parts.clone();
        if let Some(last) = parts[5].last_mut() {
            *last ^= 0x02;
        }
        let broken = VectorKey {
            parts,
            signatures: Vec::new(),
        };
        let key = broken.key();
        for signature in &vector.signatures {
            let c = representative(&key, signature);
            assert_eq!(
                key.private_operation(&c, &mut entropy),
                Err(RsaRefusal::FaultDetected)
            );
        }
    }

    /// SG-01.4's zeroisation exit: dropping a key that has signed wipes all
    /// nine of its secrets — `d`, `dP`, `dQ`, `qInv` and `q` in their
    /// Montgomery forms, the two primes with their constants, and the
    /// blinding pair's two halves — each read, through the hook in its
    /// `Drop`, holding a value before the wipe and zero after it. Every
    /// secret an operation makes along the way is wiped the same way when it
    /// goes.
    #[test]
    fn a_private_key_is_zeroised_when_dropped() {
        let vector = pss_vector_keys().remove(0);
        let key = vector.key();
        let c = representative(&key, vector.signatures.first().expect("a signature"));

        let _ = dropped::take();
        assert!(key.private_operation(&c, &mut Stream::new(3)).is_ok());
        let temporaries = dropped::take();
        assert!(
            temporaries.len() > 10,
            "the blinding draw and the halves leave secrets to wipe"
        );
        assert!(temporaries.iter().all(|wiped| wiped.zero));

        drop(key);
        let wiped = dropped::take();
        assert_eq!(wiped.len(), 9, "{wiped:?}");
        assert!(wiped.iter().all(|wiped| wiped.held), "{wiped:?}");
        assert!(wiped.iter().all(|wiped| wiped.zero), "{wiped:?}");
    }

    /// Nothing secret prints: the key shows its size, and every secret shows
    /// `[redacted]`.
    #[test]
    fn a_private_key_prints_its_size_and_nothing_else() {
        let vector = pss_vector_keys().remove(0);
        let key = vector.key();
        assert_eq!(
            format!("{key:?}"),
            "RsaPrivateKey { modulus_bits: 1024, .. }"
        );
        assert_eq!(format!("{:?}", key.d), "[redacted]");
        assert_eq!(format!("{:?}", key.p), "[redacted]");
        assert_eq!(format!("{key:#?}").matches("[redacted]").count(), 0);
    }

    /// A source that cannot fill is refused before anything is computed, and
    /// so is one whose draws can never blind: all zeros, all ones (never
    /// below `n`), and a prime of the key itself (a factor of `n`, so no
    /// inverse). A source that gives a factor twice and then good bytes
    /// blinds on the third draw. After each refusal the key still signs.
    #[test]
    fn entropy_that_cannot_blind_is_refused() {
        let vector = pss_vector_keys().remove(0);
        let key = vector.key();
        let signature = vector.signatures.first().expect("a signature");
        let c = representative(&key, signature);
        let k = key.public.modulus.byte_len();
        let p = vector.parts[3].clone();

        assert_eq!(
            key.private_operation(&c, &mut Exhausted),
            Err(RsaRefusal::EntropyUnavailable)
        );
        for (bytes, what) in [
            (Vec::new(), "all zeros"),
            (vec![0xff; k], "all ones"),
            (p.clone(), "the prime p"),
        ] {
            let mut source = Fixed {
                bytes,
                times: 0,
                then: None,
            };
            assert_eq!(
                key.private_operation(&c, &mut source),
                Err(RsaRefusal::EntropyUnusable),
                "{what}"
            );
        }
        let mut source = Fixed {
            bytes: p,
            times: 2,
            then: Some(Stream::new(11)),
        };
        assert_eq!(
            key.private_operation(&c, &mut source).as_ref(),
            Ok(signature)
        );
        assert_eq!(source.times, 0, "both factors were drawn and refused");
        assert_eq!(
            key.private_operation(&c, &mut Stream::new(12)).as_ref(),
            Ok(signature)
        );
    }

    /// A lock poisoned by a thread that panicked holding it is a refusal, not
    /// an unblinded operation.
    #[test]
    fn a_poisoned_blinding_lock_is_refused() {
        let vector = pss_vector_keys().remove(0);
        let key = vector.key();
        let c = representative(&key, vector.signatures.first().expect("a signature"));
        let died = std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let _held = key.blinding.lock();
                    panic!("a thread that dies holding the key's lock");
                })
                .join()
        });
        assert!(died.is_err());
        assert_eq!(
            key.private_operation(&c, &mut Stream::new(1)),
            Err(RsaRefusal::KeyLockPoisoned)
        );
    }

    /// A representative at or past `n` is RSASP1's own refusal, made before
    /// the source is drawn from.
    #[test]
    fn a_representative_not_below_the_modulus_is_refused() {
        let vector = pss_vector_keys().remove(0);
        let key = vector.key();
        let n = *key.public.modulus.value();
        for c in [n, n.add_carry(&Uint::from_u64(1)).0] {
            assert_eq!(
                key.private_operation(&c, &mut Exhausted),
                Err(RsaRefusal::MessageRepresentativeOutOfRange)
            );
        }
    }

    /// Every check at construction refuses by name: the public half's own
    /// refusals, and each [`KeyCheck`].
    #[test]
    fn a_key_whose_parts_disagree_is_refused_by_name() {
        let vector = pss_vector_keys().remove(0);
        let built = |edit: &dyn Fn(&mut [Vec<u8>; 8])| {
            let mut parts = vector.parts.clone();
            edit(&mut parts);
            RsaPrivateKey::new(
                &VectorKey {
                    parts,
                    signatures: Vec::new(),
                }
                .components(),
            )
            .map(|key| key.modulus_bits())
        };
        let refused = RsaRefusal::KeyCheckFailed;
        assert_eq!(built(&|_| {}), Ok(1024));

        let p = uint(&vector.parts[3]);
        let two = Uint::from_u64(2);

        assert_eq!(
            built(&|parts| parts[0] = vec![0xff; 600]),
            Err(RsaRefusal::ModulusTooWide)
        );
        assert_eq!(
            built(&|parts| parts[1] = vec![1]),
            Err(RsaRefusal::ExponentUnusable)
        );
        // An even p, a p of one, a p wider than any modulus.
        assert_eq!(
            built(&|parts| parts[3] = be(&p.add_carry(&Uint::from_u64(1)).0)),
            Err(refused(KeyCheck::PrimeUnusable))
        );
        assert_eq!(
            built(&|parts| parts[3] = vec![1]),
            Err(refused(KeyCheck::PrimeUnusable))
        );
        assert_eq!(
            built(&|parts| parts[4] = vec![0xff; 600]),
            Err(refused(KeyCheck::PrimeUnusable))
        );
        // Odd primes whose product is not n.
        assert_eq!(
            built(&|parts| parts[3] = be(&p.add_carry(&two).0)),
            Err(refused(KeyCheck::PrimesDoNotMakeModulus))
        );
        // dP of zero and of p; dQ of q.
        assert_eq!(
            built(&|parts| parts[5] = vec![0]),
            Err(refused(KeyCheck::CrtExponentOutOfRange))
        );
        assert_eq!(
            built(&|parts| parts[5] = vector.parts[3].clone()),
            Err(refused(KeyCheck::CrtExponentOutOfRange))
        );
        assert_eq!(
            built(&|parts| parts[6] = vector.parts[4].clone()),
            Err(refused(KeyCheck::CrtExponentOutOfRange))
        );
        // qInv plus one, and qInv as large as p.
        let q_inv = uint(&vector.parts[7]);
        assert_eq!(
            built(&|parts| parts[7] = be(&q_inv.add_carry(&Uint::from_u64(1)).0)),
            Err(refused(KeyCheck::CoefficientMismatch))
        );
        assert_eq!(
            built(&|parts| parts[7] = vector.parts[3].clone()),
            Err(refused(KeyCheck::CoefficientMismatch))
        );
        // d of zero, of n, and past n.
        assert_eq!(
            built(&|parts| parts[2] = vec![0]),
            Err(refused(KeyCheck::PrivateExponentOutOfRange))
        );
        assert_eq!(
            built(&|parts| parts[2] = vector.parts[0].clone()),
            Err(refused(KeyCheck::PrivateExponentOutOfRange))
        );
        assert_eq!(
            built(&|parts| parts[2] = vec![0xff; 600]),
            Err(refused(KeyCheck::PrivateExponentOutOfRange))
        );
        // p and q swapped: dP no longer fits under its prime, or qInv is no
        // longer q's inverse — whichever the sizes reach first.
        let swapped = built(&|parts| parts.swap(3, 4));
        assert!(
            matches!(
                swapped,
                Err(RsaRefusal::KeyCheckFailed(
                    KeyCheck::CrtExponentOutOfRange | KeyCheck::CoefficientMismatch
                ))
            ),
            "{swapped:?}"
        );
    }

    // ---- RSASSA-PKCS1-v1_5 signing -------------------------------------------

    /// The key every RSA text works by hand — `p = 61`, `q = 53`, `e = 17`,
    /// `d = 2753`, so `dP = 53`, `dQ = 49` and `qInv = 38` — whose twelve-bit
    /// modulus shares a factor with one draw in thirty or so, which is the
    /// blinding's redraw taken often.
    fn textbook_key() -> RsaPrivateKey {
        RsaPrivateKey::new(&RsaPrivateComponents {
            modulus: &3233u16.to_be_bytes(),
            public_exponent: &[17],
            private_exponent: &2753u16.to_be_bytes(),
            prime1: &[61],
            prime2: &[53],
            exponent1: &[53],
            exponent2: &[49],
            coefficient: &[38],
        })
        .expect("the textbook key is a key")
    }

    /// The private operation on the textbook key, by hand: `2790^2753 mod
    /// 3233` is 65, the message the texts encrypt to 2790 under `e = 17`. A
    /// hundred operations under one source keep giving it, through three
    /// refreshes of a pair whose factor is often a multiple of a prime.
    #[test]
    fn the_textbook_key_signs_as_the_texts_do() {
        let key = textbook_key();
        assert_eq!(key.modulus_bits(), 12);
        let mut entropy = Stream::new(65);
        for _ in 0..100 {
            assert_eq!(
                key.private_operation(&Uint::from_u64(2790), &mut entropy),
                Ok(vec![0x00, 0x41])
            );
        }
        // No digest's encoding fits two octets.
        for algorithm in EVERY_DIGEST {
            let digest = algorithm.digest(b"abc");
            let expected = if algorithm.signs() {
                RsaRefusal::ModulusTooShort
            } else {
                RsaRefusal::DigestNotForSigning
            };
            assert_eq!(
                key.sign_pkcs1_v15(algorithm, digest.as_bytes(), &mut entropy),
                Err(expected),
                "{algorithm:?}"
            );
        }
    }

    /// Every SHA-2 digest signs; SHA-1, the one this module verifies and
    /// never signs with, is refused whichever entry point it comes through
    /// and whatever else is wrong with the request; a digest of the wrong
    /// length is refused by name.
    #[test]
    fn the_signer_takes_the_sha_2_family_and_refuses_sha_1() {
        let vector = pss_vector_keys().remove(9);
        let key = vector.key();
        assert_eq!(key.modulus_bits(), 2048);
        let mut entropy = Stream::new(1);
        for algorithm in EVERY_DIGEST {
            let digest = algorithm.digest(b"tinker-pdf");
            let signed = key.sign_pkcs1_v15(algorithm, digest.as_bytes(), &mut entropy);
            let from_message = key.sign_pkcs1_v15_message(algorithm, b"tinker-pdf", &mut entropy);
            if algorithm.signs() {
                let signature = signed.expect("a SHA-2 digest signs");
                assert_eq!(signature.len(), 256, "k octets");
                assert_eq!(from_message.as_ref(), Ok(&signature));
                assert_eq!(
                    key.public_key()
                        .verify_pkcs1_v15(algorithm, digest.as_bytes(), &signature),
                    Ok(())
                );
            } else {
                assert_eq!(signed, Err(RsaRefusal::DigestNotForSigning));
                assert_eq!(from_message, Err(RsaRefusal::DigestNotForSigning));
                assert_eq!(
                    key.sign_pkcs1_v15(algorithm, &[], &mut Exhausted),
                    Err(RsaRefusal::DigestNotForSigning),
                    "refused before the digest or the source is looked at"
                );
            }
        }
        assert_eq!(
            key.sign_pkcs1_v15(DigestAlgorithm::Sha256, &[0u8; 20], &mut entropy),
            Err(RsaRefusal::DigestLength {
                expected: 32,
                found: 20
            })
        );
        // A key's first operation draws its pair, so a fresh key with a
        // source that cannot fill signs nothing.
        assert_eq!(
            vector
                .key()
                .sign_pkcs1_v15_message(DigestAlgorithm::Sha512, b"x", &mut Exhausted),
            Err(RsaRefusal::EntropyUnavailable)
        );
    }

    /// The same request signs to the same bytes whatever the source, which
    /// is what lets a signed document be identical on every target.
    #[test]
    fn a_pkcs1_v15_signature_does_not_depend_on_the_source() {
        let vector = pss_vector_keys().remove(3);
        let key = vector.key();
        let digest = DigestAlgorithm::Sha384.digest(b"the same request");
        let first = key
            .sign_pkcs1_v15(
                DigestAlgorithm::Sha384,
                digest.as_bytes(),
                &mut Stream::new(1),
            )
            .expect("signs");
        for seed in 2..6 {
            assert_eq!(
                key.sign_pkcs1_v15(
                    DigestAlgorithm::Sha384,
                    digest.as_bytes(),
                    &mut Stream::new(seed)
                )
                .as_ref(),
                Ok(&first)
            );
        }
        // And a 1 027-bit key's signature is its whole k octets, 129 of them,
        // whatever its leading octet holds.
        assert_eq!(key.modulus_bits(), 1027);
        assert_eq!(first.len(), 129);
    }

    /// The SigVer15 file's positive vectors were made by signing: every one,
    /// under its own key, is what the signer makes from its message, SHA-224
    /// among the digests. Published signatures, reproduced octet for octet.
    #[test]
    fn the_sigver15_positives_are_what_the_signer_makes() {
        const VECTORS: &str = include_str!("../tests/data/cavp/rsa_sigver15.rsp");
        let keys = sigver15_keys();
        let built: Vec<RsaPrivateKey> = keys.iter().map(VectorKey::key).collect();
        let mut entropy = Stream::new(15);
        let (mut e, mut d, mut algorithm, mut message, mut signature) =
            (Vec::new(), Vec::new(), None, Vec::new(), Vec::new());
        let mut n = Vec::new();
        let mut made = 0usize;
        for line in VECTORS.lines() {
            let Some((name, value)) = line.trim().split_once(" = ") else {
                continue;
            };
            match name {
                "n" => n = unhex(value),
                "e" => e = unhex(value),
                "d" => d = unhex(value),
                "SHAAlg" => algorithm = algorithm_named(value.trim()),
                "Msg" => message = unhex(value),
                "S" => signature = unhex(value),
                "Result" if value.starts_with('P') => {
                    let index = keys
                        .iter()
                        .position(|key| key.parts[0] == n && key.parts[1] == e && key.parts[2] == d)
                        .expect("the key this vector was made under");
                    let key = built.get(index).expect("one key built a vector key");
                    let algorithm = algorithm.expect("a digest");
                    let made_now = key.sign_pkcs1_v15_message(algorithm, &message, &mut entropy);
                    if algorithm.signs() {
                        assert_eq!(made_now.as_ref(), Ok(&signature), "{algorithm:?}");
                    } else {
                        assert_eq!(made_now, Err(RsaRefusal::DigestNotForSigning));
                    }
                    made += 1;
                }
                _ => {}
            }
        }
        assert_eq!(made, 75, "fifteen keys, five digests each");
    }
}
