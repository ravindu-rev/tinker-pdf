//! Constant-size big unsigned integers, and modular arithmetic over them.
//!
//! Signature verification needs three things this crate did not have: numbers
//! wider than `u128`, multiplication modulo a 4096-bit modulus, and
//! exponentiation built from it. That is all this module is. There is no
//! signed type, no general division, no allocation and no parsing — the
//! callers ([`crate::rsa`], [`crate::ecdsa`]) hand it limbs and get limbs
//! back.
//!
//! # Stack arrays, and why the width is a type parameter
//!
//! [`Uint<N>`] is `[u64; N]` and nothing else, so a P-256 field element is 32
//! bytes and an RSA-4096 modulus is 512, and no verification path touches the
//! allocator. The width is a const parameter rather than a runtime length
//! because the alternative — one 4096-bit type used for everything — makes
//! every P-256 field multiplication do four thousand limb products where
//! sixteen would do, and ECDSA does thousands of them per signature.
//!
//! # Montgomery, not Barrett
//!
//! Montgomery multiplication (Montgomery 1985, in the CIOS arrangement of Koç,
//! Acar and Kaliski 1996) has exactly one precondition: an odd modulus. Every
//! modulus this crate meets satisfies it — an RSA modulus is a product of two
//! odd primes, and the NIST field primes and group orders are prime. In
//! exchange the reduction is a shift rather than a quotient estimate, which is
//! both faster and shorter to get right; Barrett would need a precomputed
//! reciprocal and a correction step that is easy to write subtly wrong. An
//! even modulus is refused by [`Modulus::new`] rather than handled.
//!
//! # This is not constant-time, and that is a scope decision
//!
//! Verification touches no secret. The modulus, the exponent, the signature
//! and the digest are all published in the document being checked, so the
//! usual reason for a constant-time modular exponentiation — a private
//! exponent leaking through timing — does not exist here. Accordingly
//! [`Modulus::pow`] branches on exponent bits and skips leading zeros. The
//! crate performs no private-key operation, and if it ever does, this module
//! is not ready for it and this paragraph is the reason.
//!
//! # Never panics
//!
//! Every limb access goes through [`Uint::limb`] or a `get_mut`, every
//! arithmetic step is a `wrapping_*` or a `u128` widening, and every loop
//! bound comes from a const parameter or a stored length. Ruling 1: the inputs
//! are attacker-chosen bytes out of a PDF.

use core::cmp::Ordering;

/// The widest modulus this crate supports: RSA-4096, sixty-four 64-bit limbs.
///
/// The bound exists so the Montgomery scratch array can be a fixed stack array
/// rather than a `Vec`; a wider `Uint` is refused by [`Modulus::new`] instead
/// of silently producing a wrong number.
pub const MAX_LIMBS: usize = 64;

/// An `N`-limb unsigned integer, little-endian by limb.
///
/// `N` counts 64-bit limbs, so `Uint<4>` holds 256 bits and `Uint<64>` holds
/// 4096. There is no normalisation step and no invalid state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Uint<const N: usize> {
    limbs: [u64; N],
}

impl<const N: usize> Uint<N> {
    /// Zero.
    #[must_use]
    pub const fn zero() -> Self {
        Self { limbs: [0; N] }
    }

    /// A small value.
    #[must_use]
    pub fn from_u64(value: u64) -> Self {
        let mut out = Self::zero();
        out.set_limb(0, value);
        out
    }

    /// Limb `i`, or zero past the end. Out of range is a value, not a panic:
    /// callers legitimately ask for limbs above a short modulus.
    #[must_use]
    pub fn limb(&self, i: usize) -> u64 {
        self.limbs.get(i).copied().unwrap_or(0)
    }

    fn set_limb(&mut self, i: usize, value: u64) {
        if let Some(slot) = self.limbs.get_mut(i) {
            *slot = value;
        }
    }

    /// Reads a big-endian byte string, as every wire format in sight stores an
    /// integer (RFC 8017's `OS2IP`, DER `INTEGER` contents, a CAVP `n =`).
    ///
    /// Leading zero bytes are ignored, so DER's sign padding costs nothing. A
    /// value too wide for `N` limbs returns `None` rather than truncating: a
    /// truncated modulus verifies signatures against the wrong number.
    #[must_use]
    pub fn from_be_bytes(bytes: &[u8]) -> Option<Self> {
        let mut out = Self::zero();
        let mut limb = 0usize;
        let mut shift = 0u32;
        for &byte in bytes.iter().rev() {
            if limb >= N {
                if byte != 0 {
                    return None;
                }
                continue;
            }
            out.set_limb(limb, out.limb(limb) | (u64::from(byte) << shift));
            shift += 8;
            if shift == 64 {
                shift = 0;
                limb += 1;
            }
        }
        Some(out)
    }

    /// Writes the value big-endian into `out`, zero-padded on the left to
    /// `out.len()`. Returns false if the value needs more bytes than there
    /// are, in which case `out` holds the low bytes and nothing should use it.
    pub fn to_be_bytes(&self, out: &mut [u8]) -> bool {
        for slot in out.iter_mut() {
            *slot = 0;
        }
        let len = out.len();
        let mut fits = true;
        for i in 0..N {
            let limb = self.limb(i);
            for b in 0..8usize {
                let byte = (limb >> (8 * b)) as u8;
                let position = i * 8 + b;
                if position < len {
                    if let Some(slot) = out.get_mut(len - 1 - position) {
                        *slot = byte;
                    }
                } else if byte != 0 {
                    fits = false;
                }
            }
        }
        fits
    }

    /// True when every limb is zero.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.limbs.iter().all(|&limb| limb == 0)
    }

    /// True when the value is odd — Montgomery's precondition, checked once.
    #[must_use]
    pub fn is_odd(&self) -> bool {
        self.limb(0) & 1 == 1
    }

    /// Bit `i`, counting from the least significant.
    #[must_use]
    pub fn bit(&self, i: usize) -> bool {
        self.limb(i / 64) >> (i % 64) & 1 == 1
    }

    /// The index of the highest set bit, plus one; zero for zero.
    #[must_use]
    pub fn bits(&self) -> usize {
        for i in (0..N).rev() {
            let limb = self.limb(i);
            if limb != 0 {
                return i * 64 + (64 - limb.leading_zeros() as usize);
            }
        }
        0
    }

    /// Sum, and whether it carried out of the top limb.
    #[must_use]
    pub fn add_carry(&self, other: &Self) -> (Self, bool) {
        let mut out = Self::zero();
        let mut carry = 0u64;
        for i in 0..N {
            let sum = u128::from(self.limb(i)) + u128::from(other.limb(i)) + u128::from(carry);
            out.set_limb(i, sum as u64);
            carry = (sum >> 64) as u64;
        }
        (out, carry != 0)
    }

    /// Difference, and whether it borrowed past the top limb. On a borrow the
    /// value returned is the true difference modulo `2^(64N)`, which is what
    /// modular reduction wants.
    #[must_use]
    pub fn sub_borrow(&self, other: &Self) -> (Self, bool) {
        let mut out = Self::zero();
        let mut borrow = 0u64;
        for i in 0..N {
            let difference = u128::from(self.limb(i))
                .wrapping_sub(u128::from(other.limb(i)))
                .wrapping_sub(u128::from(borrow));
            out.set_limb(i, difference as u64);
            borrow = u64::from(difference >> 64 != 0);
        }
        (out, borrow != 0)
    }

    /// Shifts left by fewer than 64 bits. Bits pushed past the top limb are
    /// lost; every caller here has already established they are zero.
    #[must_use]
    pub fn shl_small(&self, bits: u32) -> Self {
        if bits == 0 || bits >= 64 {
            return *self;
        }
        let mut out = Self::zero();
        let mut carry = 0u64;
        for i in 0..N {
            let limb = self.limb(i);
            out.set_limb(i, (limb << bits) | carry);
            carry = limb >> (64 - bits);
        }
        out
    }
}

impl<const N: usize> Default for Uint<N> {
    fn default() -> Self {
        Self::zero()
    }
}

impl<const N: usize> Ord for Uint<N> {
    fn cmp(&self, other: &Self) -> Ordering {
        for i in (0..N).rev() {
            match self.limb(i).cmp(&other.limb(i)) {
                Ordering::Equal => {}
                unequal => return unequal,
            }
        }
        Ordering::Equal
    }
}

impl<const N: usize> PartialOrd for Uint<N> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// An odd modulus with the constants Montgomery arithmetic needs precomputed.
///
/// Construction costs one pass of `64 * used` modular doublings — about what
/// converting a single value into Montgomery form costs — so a caller
/// verifying many signatures under one key should build this once and keep it.
#[derive(Clone, Copy, Debug)]
pub struct Modulus<const N: usize> {
    m: Uint<N>,
    /// Limbs actually occupied by `m`. Montgomery's radix is
    /// `2^(64 * used)`, so a 1024-bit key held in a `Uint<64>` does a
    /// sixteenth of the work a 4096-bit one does rather than paying for the
    /// widest case the type can hold.
    used: usize,
    bits: usize,
    /// `-m^-1 mod 2^64`, the CIOS inner-loop multiplier.
    n0inv: u64,
    /// `R^2 mod m`, which turns a conversion into Montgomery form into a
    /// single multiplication.
    r2: Uint<N>,
}

impl<const N: usize> Modulus<N> {
    /// Precomputes for `m`, or refuses it.
    ///
    /// Refused: an even modulus (Montgomery's radix would share a factor with
    /// it), a modulus below 3, and a width above [`MAX_LIMBS`] — the scratch
    /// array is a stack array of fixed size and a wider one would not fit.
    #[must_use]
    pub fn new(m: Uint<N>) -> Option<Self> {
        if N > MAX_LIMBS || !m.is_odd() {
            return None;
        }
        let bits = m.bits();
        if bits < 2 {
            return None;
        }
        let used = bits.div_ceil(64);

        let n0inv = montgomery_n0inv(m.limb(0));

        // R^2 mod m: take R mod m and double it another 64 * used times. This
        // is the one expensive step of construction, and the reason `Modulus`
        // is worth keeping rather than rebuilding per signature.
        let mut r2 = radix_mod(&m, used);
        for _ in 0..(64 * used) {
            r2 = add_mod(&r2, &r2, &m);
        }

        Some(Self {
            m,
            used,
            bits,
            n0inv,
            r2,
        })
    }

    /// The modulus itself.
    #[must_use]
    pub fn value(&self) -> &Uint<N> {
        &self.m
    }

    /// The modulus's bit length.
    #[must_use]
    pub fn bits(&self) -> usize {
        self.bits
    }

    /// The modulus's byte length — RFC 8017's `k`.
    #[must_use]
    pub fn byte_len(&self) -> usize {
        self.bits.div_ceil(8)
    }

    /// `(a + b) mod m`, for `a` and `b` already below `m`.
    #[must_use]
    pub fn add(&self, a: &Uint<N>, b: &Uint<N>) -> Uint<N> {
        add_mod(a, b, &self.m)
    }

    /// `(a - b) mod m`, for `a` and `b` already below `m`.
    #[must_use]
    pub fn sub(&self, a: &Uint<N>, b: &Uint<N>) -> Uint<N> {
        let (difference, borrowed) = a.sub_borrow(b);
        if borrowed {
            self.narrow(difference.add_carry(&self.m).0)
        } else {
            difference
        }
    }

    /// Clears the limbs above the modulus.
    ///
    /// A borrow out of a subtraction propagates through *every* limb of the
    /// type, but the value it is borrowing against is `2^(64 * used)`, not
    /// `2^(64 * N)` — those are the same number only when the modulus fills
    /// the type. For an RSA-1024 key inside a 4096-bit `Uint` they differ by
    /// forty-eight limbs, and the wrapped difference comes back correct in its
    /// low limbs with the rest set to all-ones. The low limbs are the answer;
    /// this clears the rest.
    ///
    /// Found by a test rather than by reading: [`Modulus::mul`]'s inner loops
    /// only ever read the low `used` limbs, so the garbage propagated
    /// invisibly through a whole modular exponentiation and surfaced only when
    /// the final value was serialised — as a *valid* signature reported
    /// invalid, some of the time, on some key sizes.
    fn narrow(&self, mut value: Uint<N>) -> Uint<N> {
        for i in self.used..N {
            value.set_limb(i, 0);
        }
        value
    }

    /// Reduces an arbitrary value modulo `m`, one bit at a time.
    ///
    /// Linear in the operand's bit length, so it is for the places where a
    /// value genuinely may exceed the modulus — a digest reinterpreted as a
    /// scalar, FIPS 186-4 §6.4 — and not for an inner loop.
    #[must_use]
    pub fn reduce(&self, value: &Uint<N>) -> Uint<N> {
        if *value < self.m {
            return *value;
        }
        let mut remainder = Uint::<N>::zero();
        for i in (0..value.bits()).rev() {
            let (doubled, carried) = remainder.add_carry(&remainder);
            let mut next = doubled;
            if value.bit(i) {
                next = next.add_carry(&Uint::from_u64(1)).0;
            }
            remainder = if carried || next >= self.m {
                next.sub_borrow(&self.m).0
            } else {
                next
            };
        }
        remainder
    }

    /// The Montgomery product `a * b * R^-1 mod m`, by CIOS.
    ///
    /// Both operands must already be below `m`; the result is too.
    #[must_use]
    pub fn mul(&self, a: &Uint<N>, b: &Uint<N>) -> Uint<N> {
        let s = self.used;
        let mut t = [0u64; MAX_LIMBS + 2];

        for i in 0..s {
            let bi = u128::from(b.limb(i));
            let mut carry = 0u64;
            for j in 0..s {
                let product =
                    u128::from(read(&t, j)) + u128::from(a.limb(j)) * bi + u128::from(carry);
                write(&mut t, j, product as u64);
                carry = (product >> 64) as u64;
            }
            let sum = u128::from(read(&t, s)) + u128::from(carry);
            write(&mut t, s, sum as u64);
            write(&mut t, s + 1, (sum >> 64) as u64);

            let factor = read(&t, 0).wrapping_mul(self.n0inv);
            let first = u128::from(read(&t, 0)) + u128::from(factor) * u128::from(self.m.limb(0));
            let mut carry = (first >> 64) as u64;
            for j in 1..s {
                let product = u128::from(read(&t, j))
                    + u128::from(factor) * u128::from(self.m.limb(j))
                    + u128::from(carry);
                write(&mut t, j - 1, product as u64);
                carry = (product >> 64) as u64;
            }
            let sum = u128::from(read(&t, s)) + u128::from(carry);
            write(&mut t, s.saturating_sub(1), sum as u64);
            let top = read(&t, s + 1).wrapping_add((sum >> 64) as u64);
            write(&mut t, s, top);
        }

        let mut out = Uint::<N>::zero();
        for j in 0..s {
            out.set_limb(j, read(&t, j));
        }
        // CIOS leaves a value below 2m spread over s+1 limbs; one conditional
        // subtraction finishes it. When the carry limb is set the subtraction
        // borrows against `2^(64 * s)`, so the borrow has to be confined to
        // the modulus's own width — see `narrow`.
        if read(&t, s) != 0 || out >= self.m {
            self.narrow(out.sub_borrow(&self.m).0)
        } else {
            out
        }
    }

    /// Converts into Montgomery form: `a * R mod m`.
    #[must_use]
    pub fn to_montgomery(&self, a: &Uint<N>) -> Uint<N> {
        self.mul(a, &self.r2)
    }

    /// Converts out of Montgomery form.
    #[must_use]
    pub fn from_montgomery(&self, a: &Uint<N>) -> Uint<N> {
        self.mul(a, &Uint::from_u64(1))
    }

    /// `a * b mod m` for ordinary (non-Montgomery) operands below `m`.
    #[must_use]
    pub fn mul_mod(&self, a: &Uint<N>, b: &Uint<N>) -> Uint<N> {
        self.mul(&self.to_montgomery(a), b)
    }

    /// `base^exponent mod m`, square and multiply, most significant bit first.
    ///
    /// The exponent is public in every use here — an RSA public exponent, or a
    /// prime modulus minus two — so the loop skips its leading zeros and
    /// branches on its bits. See the module note on constant time.
    #[must_use]
    pub fn pow(&self, base: &Uint<N>, exponent: &Uint<N>) -> Uint<N> {
        let reduced = if *base < self.m {
            *base
        } else {
            self.reduce(base)
        };
        let base_montgomery = self.to_montgomery(&reduced);
        let mut accumulator = self.to_montgomery(&Uint::from_u64(1));
        for i in (0..exponent.bits()).rev() {
            accumulator = self.mul(&accumulator, &accumulator);
            if exponent.bit(i) {
                accumulator = self.mul(&accumulator, &base_montgomery);
            }
        }
        self.from_montgomery(&accumulator)
    }

    /// The multiplicative inverse of `a`, **only correct for a prime modulus**.
    ///
    /// Fermat's little theorem: `a^(m-2) = a^-1` when `m` is prime. Every
    /// modulus this is called with is one of the four NIST constants — two
    /// field primes and two group orders — so the extended Euclidean
    /// algorithm, which would work for any modulus and is longer to write
    /// without a division, buys nothing. Zero has no inverse and returns
    /// `None`.
    #[must_use]
    pub fn inverse_prime(&self, a: &Uint<N>) -> Option<Uint<N>> {
        if a.is_zero() {
            return None;
        }
        let (exponent, _) = self.m.sub_borrow(&Uint::from_u64(2));
        Some(self.pow(a, &exponent))
    }
}

/// `(a + b) mod m` for operands already below `m`.
///
/// A carry out of the top limb is not an error: `a + b < 2m`, so the true sum
/// minus `m` fits in `N` limbs and the wrapping subtraction recovers it
/// exactly.
fn add_mod<const N: usize>(a: &Uint<N>, b: &Uint<N>, m: &Uint<N>) -> Uint<N> {
    let (sum, carried) = a.add_carry(b);
    if carried || sum >= *m {
        sum.sub_borrow(m).0
    } else {
        sum
    }
}

/// `2^(64 * used) mod m`, in at most 64 shift-and-subtract steps.
///
/// The naive route — doubling one, `64 * used` times — costs a factor of
/// `used` more. Instead: `m << z` has its top bit set, so the radix is less
/// than twice it and a single subtraction brings the value below `m << z`;
/// `z` halving steps then bring it below `m`.
fn radix_mod<const N: usize>(m: &Uint<N>, used: usize) -> Uint<N> {
    let top = m.limb(used.saturating_sub(1));
    let z = top.leading_zeros();
    let aligned = m.shl_small(z);

    let mut value = Uint::<N>::zero();
    let mut borrow = 0u64;
    for i in 0..used {
        let difference = 0u128
            .wrapping_sub(u128::from(aligned.limb(i)))
            .wrapping_sub(u128::from(borrow));
        value.set_limb(i, difference as u64);
        borrow = u64::from(difference >> 64 != 0);
    }

    for i in (0..z).rev() {
        let shifted = m.shl_small(i);
        if value >= shifted {
            value = value.sub_borrow(&shifted).0;
        }
    }
    value
}

/// `-m0^-1 mod 2^64`, by Newton iteration.
///
/// `x = 1` is already the inverse to one bit because `m0` is odd, and each
/// step doubles the number of correct bits, so six steps reach 64.
fn montgomery_n0inv(m0: u64) -> u64 {
    let mut inverse = 1u64;
    for _ in 0..6 {
        inverse = inverse.wrapping_mul(2u64.wrapping_sub(m0.wrapping_mul(inverse)));
    }
    inverse.wrapping_neg()
}

fn read(scratch: &[u64], i: usize) -> u64 {
    scratch.get(i).copied().unwrap_or(0)
}

fn write(scratch: &mut [u64], i: usize, value: u64) {
    if let Some(slot) = scratch.get_mut(i) {
        *slot = value;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `u128` is the language's own arithmetic, so a two-limb `Uint` can be
    /// checked against it directly rather than against a table of answers
    /// somebody transcribed. Every operation below is compared this way, over
    /// values chosen to cross the limb boundary and the carry.
    fn as_u128<const N: usize>(value: &Uint<N>) -> u128 {
        u128::from(value.limb(0)) | (u128::from(value.limb(1)) << 64)
    }

    fn from_u128(value: u128) -> Uint<2> {
        let mut out = Uint::<2>::zero();
        out.set_limb(0, value as u64);
        out.set_limb(1, (value >> 64) as u64);
        out
    }

    /// `(a * b) mod m` in `u128`, by double-and-add so the product never
    /// needs 256 bits. Correct for any `m` below `2^127`, which every modulus
    /// in these tests is.
    fn mulmod(a: u128, b: u128, m: u128) -> u128 {
        let mut result = 0u128;
        let mut a = a % m;
        let mut b = b % m;
        while b != 0 {
            if b & 1 == 1 {
                result = (result + a) % m;
            }
            a = (a << 1) % m;
            b >>= 1;
        }
        result
    }

    const SAMPLES: [u128; 8] = [
        0,
        1,
        2,
        0xffff_ffff_ffff_ffff,
        0x1_0000_0000_0000_0000,
        0x0123_4567_89ab_cdef_fedc_ba98_7654_3210,
        u128::MAX / 3,
        u128::MAX - 1,
    ];

    #[test]
    fn addition_and_subtraction_agree_with_u128() {
        for a in SAMPLES {
            for b in SAMPLES {
                let (sum, carried) = from_u128(a).add_carry(&from_u128(b));
                assert_eq!(as_u128(&sum), a.wrapping_add(b));
                assert_eq!(carried, a.checked_add(b).is_none());

                let (difference, borrowed) = from_u128(a).sub_borrow(&from_u128(b));
                assert_eq!(as_u128(&difference), a.wrapping_sub(b));
                assert_eq!(borrowed, a < b);
            }
        }
    }

    #[test]
    fn ordering_agrees_with_u128() {
        for a in SAMPLES {
            for b in SAMPLES {
                assert_eq!(from_u128(a).cmp(&from_u128(b)), a.cmp(&b), "{a:x} vs {b:x}");
            }
        }
    }

    #[test]
    fn bit_length_and_bits_agree_with_u128() {
        for a in SAMPLES {
            assert_eq!(from_u128(a).bits(), 128 - a.leading_zeros() as usize);
            for i in 0..128 {
                assert_eq!(from_u128(a).bit(i), a >> i & 1 == 1, "{a:x} bit {i}");
            }
        }
    }

    #[test]
    fn big_endian_bytes_round_trip() {
        for a in SAMPLES {
            let mut bytes = [0u8; 16];
            assert!(from_u128(a).to_be_bytes(&mut bytes));
            assert_eq!(bytes, a.to_be_bytes());
            assert_eq!(Uint::<2>::from_be_bytes(&bytes), Some(from_u128(a)));
        }
    }

    #[test]
    fn a_value_wider_than_the_type_is_refused_not_truncated() {
        // Seventeen bytes into a 128-bit type: the leading byte is not zero,
        // so there is no honest answer and `None` is the answer.
        let mut wide = [0u8; 17];
        if let Some(slot) = wide.first_mut() {
            *slot = 1;
        }
        assert_eq!(Uint::<2>::from_be_bytes(&wide), None);
        // The same width with a zero pad is fine — DER writes that pad.
        assert_eq!(
            Uint::<2>::from_be_bytes(&[0u8; 17]),
            Some(Uint::<2>::zero())
        );
    }

    #[test]
    fn an_even_modulus_is_refused() {
        assert!(Modulus::new(Uint::<2>::from_u64(1024)).is_none());
        assert!(Modulus::new(Uint::<2>::from_u64(1)).is_none());
        assert!(Modulus::new(Uint::<2>::from_u64(1023)).is_some());
    }

    #[test]
    fn montgomery_products_agree_with_u128() {
        // Odd moduli spanning one limb, the limb boundary and two limbs.
        let moduli: [u128; 4] = [
            0xffff_ffff_ffff_fffb,
            0x1_0000_0000_0000_0001,
            0x0123_4567_89ab_cdef_fedc_ba98_7654_3211,
            0x7fff_ffff_ffff_ffff_ffff_ffff_ffff_ffff,
        ];
        for m in moduli {
            let modulus = Modulus::new(from_u128(m)).expect("odd and above 2");
            for a in SAMPLES {
                for b in SAMPLES {
                    let (a, b) = (a % m, b % m);
                    let product = modulus.mul_mod(&from_u128(a), &from_u128(b));
                    let expected = mulmod(a, b, m);
                    assert_eq!(as_u128(&product), expected, "{a:x} * {b:x} mod {m:x}");
                }
            }
        }
    }

    #[test]
    fn reduction_agrees_with_u128() {
        let m = 0x0123_4567_89ab_cdef_fedc_ba98_7654_3211u128;
        let modulus = Modulus::new(from_u128(m)).expect("odd");
        for a in SAMPLES {
            assert_eq!(as_u128(&modulus.reduce(&from_u128(a))), a % m);
        }
    }

    #[test]
    fn addition_and_subtraction_modulo_agree_with_u128() {
        let m = 0xffff_ffff_ffff_fffbu128;
        let modulus = Modulus::new(from_u128(m)).expect("odd");
        for a in SAMPLES {
            for b in SAMPLES {
                let (a, b) = (a % m, b % m);
                assert_eq!(
                    as_u128(&modulus.add(&from_u128(a), &from_u128(b))),
                    (a + b) % m
                );
                assert_eq!(
                    as_u128(&modulus.sub(&from_u128(a), &from_u128(b))),
                    (a + m - b) % m
                );
            }
        }
    }

    #[test]
    fn exponentiation_agrees_with_repeated_multiplication() {
        let m = 0xffff_ffff_ffff_fffbu128;
        let modulus = Modulus::new(from_u128(m)).expect("odd");
        for base in [2u128, 3, 0x1234_5678_9abc_def0, m - 1] {
            let mut expected = 1u128;
            for exponent in 0u64..40 {
                let got = modulus.pow(&from_u128(base), &Uint::<2>::from_u64(exponent));
                assert_eq!(as_u128(&got), expected, "{base:x}^{exponent}");
                expected = expected * base % m;
            }
        }
    }

    /// Fermat's little theorem is only invoked for prime moduli, so the test
    /// uses one: 2^61 - 1, a Mersenne prime.
    #[test]
    fn inversion_returns_a_true_inverse_for_a_prime_modulus() {
        let p = (1u128 << 61) - 1;
        let modulus = Modulus::new(from_u128(p)).expect("odd");
        for a in [1u128, 2, 3, 1_000_003, p - 1] {
            let inverse = modulus.inverse_prime(&from_u128(a)).expect("nonzero");
            let product = modulus.mul_mod(&from_u128(a), &inverse);
            assert_eq!(as_u128(&product), 1, "1/{a} mod {p}");
        }
        assert!(modulus.inverse_prime(&Uint::<2>::zero()).is_none());
    }

    /// The four-limb path is what P-256 uses, and it has to agree with the
    /// two-limb one on values that fit both.
    #[test]
    fn a_wider_type_computes_the_same_answers() {
        let m = 0x0123_4567_89ab_cdef_fedc_ba98_7654_3211u128;
        let narrow = Modulus::new(from_u128(m)).expect("odd");
        let mut wide_m = Uint::<4>::zero();
        wide_m.set_limb(0, m as u64);
        wide_m.set_limb(1, (m >> 64) as u64);
        let wide = Modulus::new(wide_m).expect("odd");

        for a in SAMPLES {
            for b in SAMPLES {
                let (a, b) = (a % m, b % m);
                let mut wide_a = Uint::<4>::zero();
                wide_a.set_limb(0, a as u64);
                wide_a.set_limb(1, (a >> 64) as u64);
                let mut wide_b = Uint::<4>::zero();
                wide_b.set_limb(0, b as u64);
                wide_b.set_limb(1, (b >> 64) as u64);

                let narrow_product = narrow.mul_mod(&from_u128(a), &from_u128(b));
                let wide_product = wide.mul_mod(&wide_a, &wide_b);
                assert_eq!(wide_product.limb(0), narrow_product.limb(0));
                assert_eq!(wide_product.limb(1), narrow_product.limb(1));
                assert_eq!(wide_product.limb(2), 0);
                assert_eq!(wide_product.limb(3), 0);
            }
        }
    }

    /// A one-limb modulus inside a four-limb type — `used < N`, which is what
    /// every RSA key below 4096 bits is.
    ///
    /// **This test failed before `Modulus::narrow` existed**, and it is the
    /// only thing in the suite that did. CIOS's carry limb means the final
    /// conditional subtraction borrows against `2^(64 * used)`, while
    /// `sub_borrow` borrows against `2^(64 * N)`; when those differ the answer
    /// comes back correct in its low limbs with the rest set to all-ones. The
    /// inner loops read only the low limbs, so the garbage rode invisibly
    /// through an entire modular exponentiation and would have surfaced as a
    /// *valid* signature reported invalid — intermittently, and only on key
    /// sizes below the widest. Hence the limb-by-limb assertions rather than a
    /// comparison of the value alone.
    #[test]
    fn a_modulus_narrower_than_its_type_multiplies_correctly() {
        let m = 0xffff_ffff_ffff_fffbu128;
        let mut wide_m = Uint::<4>::zero();
        wide_m.set_limb(0, m as u64);
        let wide = Modulus::new(wide_m).expect("odd");

        let wrap = |value: u128| {
            let mut out = Uint::<4>::zero();
            out.set_limb(0, value as u64);
            out
        };
        // Both ends of the range and the middle: the Montgomery accumulator
        // overflows into the carry limb only for large operands, and the
        // conditional subtraction has to be right either way.
        let mut values = Vec::new();
        for offset in 0..32u128 {
            values.push(offset);
            values.push(m - 1 - offset);
            values.push(m / 2 + offset);
        }

        for &a in &values {
            for &b in &values {
                let product = wide.mul_mod(&wrap(a), &wrap(b));
                assert_eq!(
                    u128::from(product.limb(0)),
                    mulmod(a, b, m),
                    "{a:x} * {b:x}"
                );
                assert_eq!(product.limb(1), 0, "no borrow leaked past the modulus");
                assert_eq!(product.limb(2), 0);
                assert_eq!(product.limb(3), 0);
            }
        }

        // And through an exponentiation, which is where the bug actually bit:
        // the result must serialise into the modulus's own byte width.
        let power = wide.pow(&wrap(m - 3), &wrap(65537));
        let mut bytes = [0u8; 8];
        assert!(
            power.to_be_bytes(&mut bytes),
            "the result fits the modulus's width"
        );
    }

    /// A modulus whose top limb is nearly empty exercises `radix_mod`'s
    /// shift-and-subtract loop at its longest — 63 halving steps.
    #[test]
    fn a_modulus_with_a_short_top_limb_still_reduces_correctly() {
        let m = 0x1_0000_0000_0000_0001u128;
        let modulus = Modulus::new(from_u128(m)).expect("odd");
        for a in SAMPLES {
            for b in SAMPLES {
                let (a, b) = (a % m, b % m);
                assert_eq!(
                    as_u128(&modulus.mul_mod(&from_u128(a), &from_u128(b))),
                    mulmod(a, b, m)
                );
            }
        }
    }
}
