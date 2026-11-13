//! Constant-size big unsigned integers, and modular arithmetic over them.
//!
//! Signature verification needs three things this crate did not have: numbers
//! wider than `u128`, multiplication modulo a 4096-bit modulus, and
//! exponentiation built from it. Signing needs the same three on secret
//! operands, without leaking them. That is all this module is. There is no
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
//! # Two paths: the public one, and the constant-time one
//!
//! Verification touches no secret. The modulus, the exponent, the signature
//! and the digest are all published in the document being checked, so the
//! usual reason for a constant-time modular exponentiation — a private
//! exponent leaking through timing — does not exist there, and
//! [`Modulus::pow`] branches on exponent bits and skips leading zeros. That
//! is the **public path**, and it touches no secret. RSAES-PKCS1-v1_5
//! encryption is the one caller with a secret *operand* on it — the content
//! key it seals, under a public exponent — and the crate's own note says what
//! that costs.
//!
//! Signing does have secrets — a private exponent, the primes, a blinding
//! factor (`docs/design/signing.md`, *Secret operands*) — and those take the
//! **constant-time path** ([`Modulus::pow_secret`]), on which four rules hold:
//!
//! 1. No branch on a secret: a choice between two values is a mask, passed
//!    through `core::hint::black_box` so the optimiser has no bit to branch on.
//! 2. No memory access indexed by a secret: a table entry is read by reading
//!    every entry and keeping the wanted one by mask.
//! 3. Fixed lengths: every loop runs over the modulus's own limb count, and an
//!    exponentiation over the modulus's bit length, never the secret's.
//! 4. Branch-free reduction: a Montgomery product's final subtraction is
//!    computed every time, with its borrow, and kept or not by mask.
//!
//! **The path's Montgomery core is its own**
//! (`docs/design/crypto-throughput.md`, *The Montgomery core on SG-01's
//! path*). A product is CIOS over exactly the modulus's limbs — sixteen for a
//! 1 024-bit CRT half, however wide the type that holds it — with each operand
//! cut to that width once, so that its loops walk slices and read or write no
//! limb through a bounds check, and the two limbs above the width stay in
//! registers. A squaring is a core of its own: each cross product `a_i · a_j`
//! with `i < j` formed once and the sum doubled, the squares `a_i²` added, then
//! the same reduction, so its multiplication half forms `n(n+1)/2` limb
//! products where a product's forms `n²`. The exponentiations' squarings —
//! four a window, one a bit of a public exponent — and the blinding pair's are
//! that core. Its loops run over the limb count and its doubling carries by
//! arithmetic, so it branches on no value a product does not.
//!
//! The path's working memory — the window table, the accumulator and the
//! Montgomery scratch — is wiped before an operation returns. Under
//! `cfg(test)` the path counts what it does, and
//! `the_private_path_counts_alike_over_every_exponent` holds those counts —
//! the squarings among the products and the limb products of each half among
//! them — equal over exponents of every shape: a count that moved with the
//! exponent would be a branch or a lookup that depends on it. The counts show
//! the shape of the source; they cannot show what a target's machine code
//! does, which is the independent review the roadmap owes (H-67).
//!
//! # Never panics
//!
//! Every limb access goes through [`Uint::limb`], a `get` or a `get_mut`, or
//! an iterator over a slice cut once to the modulus's width; every arithmetic
//! step is a `wrapping_*` or a `u128` widening, and every loop bound comes from
//! a const parameter or a stored length. Ruling 1: the inputs are
//! attacker-chosen bytes out of a PDF.

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
        if out.read_be_bytes(bytes) {
            Some(out)
        } else {
            None
        }
    }

    /// [`Uint::from_be_bytes`] into an existing value, so that a secret is
    /// read straight into the heap memory that holds it rather than built on
    /// the stack and moved. Returns false, with the value half-written, when
    /// the bytes are too wide; the caller drops it.
    pub(crate) fn read_be_bytes(&mut self, bytes: &[u8]) -> bool {
        for limb in &mut self.limbs {
            *limb = 0;
        }
        let mut limb = 0usize;
        let mut shift = 0u32;
        for &byte in bytes.iter().rev() {
            if limb >= N {
                if byte != 0 {
                    return false;
                }
                continue;
            }
            self.set_limb(limb, self.limb(limb) | (u64::from(byte) << shift));
            shift += 8;
            if shift == 64 {
                shift = 0;
                limb += 1;
            }
        }
        true
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

    /// The limbs the modulus occupies — the width every loop over it runs.
    pub(crate) fn limbs(&self) -> usize {
        self.used
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

// ---- The constant-time path ------------------------------------------------
//
// Everything below this line is for operands that are secret
// (`docs/design/signing.md`, *The constant-time path*). The public path above
// stays as it is: it is what verification runs, and it touches no secret.

/// The constant-time path's Montgomery scratch, in limbs: a squaring's whole
/// square, twice the widest modulus. A product's CIOS uses the low limbs of
/// it, a modulus's width, and holds its two limbs above that in registers.
const SCRATCH_LIMBS: usize = 2 * MAX_LIMBS;

/// The constant-time path's Montgomery scratch.
type Scratchpad = [u64; SCRATCH_LIMBS];

/// The constant-time path's working memory: the sixteen-entry window table,
/// the accumulator, the entry a window selected and the Montgomery scratch.
///
/// It lives in the frame of the operation that made it and is never moved,
/// and its `Drop` wipes every limb, so nothing a secret exponentiation
/// computed is left behind in it when the operation returns — including when
/// it returns early.
struct Workspace<const N: usize> {
    table: [Uint<N>; 16],
    accumulator: Uint<N>,
    entry: Uint<N>,
    scratch: Scratchpad,
}

impl<const N: usize> Workspace<N> {
    fn new() -> Self {
        Self {
            table: [Uint::zero(); 16],
            accumulator: Uint::zero(),
            entry: Uint::zero(),
            scratch: [0; SCRATCH_LIMBS],
        }
    }
}

impl<const N: usize> Drop for Workspace<N> {
    fn drop(&mut self) {
        for value in &mut self.table {
            wipe(&mut value.limbs);
        }
        wipe(&mut self.accumulator.limbs);
        wipe(&mut self.entry.limbs);
        wipe(&mut self.scratch);
    }
}

/// Writes zero over `limbs` so that the stores are not elided as writes to
/// memory about to be freed or reused.
///
/// The crates forbid `unsafe`, so there is no volatile write: the zeros go
/// through `core::hint::black_box`, which the optimiser must assume reads
/// them, and a sequentially consistent compiler fence keeps them ahead of
/// whatever follows. What a target's code generation keeps anyway is the
/// review's to say (`docs/design/signing.md`, *Risks*).
pub(crate) fn wipe(limbs: &mut [u64]) {
    for limb in limbs.iter_mut() {
        *limb = 0;
    }
    let _ = core::hint::black_box(&mut *limbs);
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
}

/// All ones when `bit` is 1 and all zeros when it is 0: the one way the
/// constant-time path chooses (rule 1). The mask goes through `black_box`, so
/// the optimiser cannot see the bit it came from and turn the choice back
/// into a branch.
#[inline]
fn mask(bit: u64) -> u64 {
    core::hint::black_box(0u64.wrapping_sub(bit & 1))
}

/// One when `value` is nonzero and zero when it is zero, without comparing:
/// the top bit of `value | -value` is set exactly when `value` is not zero.
#[inline]
fn nonzero(value: u64) -> u64 {
    (value | value.wrapping_neg()) >> 63
}

/// All ones when `a == b`, all zeros otherwise.
#[inline]
fn equal_mask(a: u64, b: u64) -> u64 {
    mask(nonzero(a ^ b) ^ 1)
}

/// `a` where `choose` is all ones, `b` where it is all zeros.
#[inline]
fn select(choose: u64, a: u64, b: u64) -> u64 {
    (a & choose) | (b & !choose)
}

/// The borrow out of `a - b - borrow`, as 0 or 1, and the difference's low
/// limb. The `u128` difference's top half is all zeros or all ones, so its
/// low bit is the borrow: no comparison is made.
#[inline]
fn subtract_limb(a: u64, b: u64, borrow: u64) -> (u64, u64) {
    let difference = u128::from(a)
        .wrapping_sub(u128::from(b))
        .wrapping_sub(u128::from(borrow));
    (difference as u64, ((difference >> 64) as u64) & 1)
}

/// Adds `$n` to the constant-time path's count `$field`. Outside the crate's
/// own tests it is nothing at all, not even the evaluation of `$n`.
macro_rules! tally {
    ($field:ident, $n:expr) => {
        #[cfg(test)]
        counts::add(|c| c.$field += $n);
    };
}

/// What the constant-time path did, counted under `cfg(test)` only.
///
/// Every count is of work the source performs — loop iterations, limb
/// products, table entries read — rather than a formula for it, so an early
/// exit or a skipped multiplication that depended on a secret would move one.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Counts {
    /// Montgomery products, squarings among them.
    pub(crate) products: u64,
    /// Of those, the squarings, each made by the dedicated squaring.
    pub(crate) squarings: u64,
    /// 64-by-64-bit limb products inside those, both halves of each.
    pub(crate) limb_products: u64,
    /// Of those, the ones in the multiplication halves: `n²` a product and
    /// `n(n+1)/2` a squaring, for a modulus of `n` limbs. The reduction
    /// half's `n²` are the rest.
    pub(crate) multiplication_limb_products: u64,
    /// Window-table entries read.
    pub(crate) table_reads: u64,
    /// Values kept by mask rather than chosen by a branch: a limb of a
    /// product's final subtraction, a table entry's contribution.
    pub(crate) selects: u64,
    /// Iterations of the exponentiation's own loops: each window, each
    /// squaring, each step of a table scan and of the table's construction.
    pub(crate) branches: u64,
}

/// The counts themselves, per thread, so tests running side by side do not
/// add into each other's.
#[cfg(test)]
pub(crate) mod counts {
    use super::Counts;
    use std::cell::Cell;

    thread_local! {
        static COUNTS: Cell<Counts> = Cell::new(Counts::default());
    }

    /// Applies `update` to this thread's counts.
    pub(crate) fn add(update: impl FnOnce(&mut Counts)) {
        COUNTS.with(|cell| {
            let mut counts = cell.get();
            update(&mut counts);
            cell.set(counts);
        });
    }

    /// This thread's counts since the last call, which start again at zero.
    pub(crate) fn take() -> Counts {
        COUNTS.with(|cell| cell.replace(Counts::default()))
    }
}

impl<const N: usize> Modulus<N> {
    /// The modulus's own limbs, `used` of them: the width every loop of the
    /// constant-time core runs.
    fn own_limbs(&self) -> &[u64] {
        // `used` is at most `N` (`Modulus::new`, `SecretModulus::new`), so
        // the cut is always there; an empty slice would compute nothing.
        self.m.limbs.get(..self.used).unwrap_or_default()
    }

    /// CIOS on the constant-time path: `a · b · R^-1`, below `2m`, its low
    /// `used` limbs left in `t[..used]` and its carry limb returned.
    ///
    /// The operands are cut to the modulus's width once, here, and
    /// [`montgomery_product`] walks the slices; the scratch belongs to the
    /// caller, who wipes it, and the final subtraction is
    /// [`Modulus::finish_secret`]'s. `a` may be any value below `R`, `b` any
    /// value below `m`: `(a·b + q·m) / R` is then below `2m`.
    fn core_secret(&self, a: &Uint<N>, b: &Uint<N>, t: &mut Scratchpad) -> u64 {
        let s = self.used;
        match (a.limbs.get(..s), b.limbs.get(..s), t.get_mut(..s)) {
            (Some(a), Some(b), Some(t)) => {
                montgomery_product(a, b, self.own_limbs(), self.n0inv, t)
            }
            // Unreachable: `used` is at most `N`, and `N` at most
            // `MAX_LIMBS`, which `Modulus::new` and `SecretModulus::new`
            // check before any value is computed.
            _ => 0,
        }
    }

    /// The dedicated squaring on the constant-time path: `a² · R^-1`, below
    /// `2m` for `a` below `m`, its low `used` limbs left in
    /// `t[used..2·used]` and its carry limb returned
    /// ([`montgomery_square`]).
    fn square_core_secret(&self, a: &Uint<N>, t: &mut Scratchpad) -> u64 {
        let s = self.used;
        match (a.limbs.get(..s), t.get_mut(..2 * s)) {
            (Some(a), Some(t)) => montgomery_square(a, self.own_limbs(), self.n0inv, t),
            // Unreachable, as in `core_secret`.
            _ => 0,
        }
    }

    /// The product's last step, branch-free (rule 4): the core leaves a value
    /// below `2m` as the modulus's width of limbs, `low`, and a carry limb,
    /// and [`reduce_once`] takes `m` off it or not by mask. Limbs above the
    /// modulus are cleared, which is [`Modulus::narrow`]'s job on the public
    /// path.
    fn finish_secret(&self, low: &[u64], carry: u64, out: &mut Uint<N>) {
        for (limb, &value) in out.limbs.iter_mut().zip(low) {
            *limb = value;
        }
        for limb in out.limbs.iter_mut().skip(self.used) {
            *limb = 0;
        }
        reduce_once(out, &self.m, self.used, carry);
        tally!(products, 1);
    }

    /// `out = a · b · R^-1 mod m`, on the constant-time path.
    fn product_secret(&self, a: &Uint<N>, b: &Uint<N>, out: &mut Uint<N>, t: &mut Scratchpad) {
        let carry = self.core_secret(a, b, t);
        self.finish_secret(t.get(..self.used).unwrap_or_default(), carry, out);
    }

    /// `x = x · b · R^-1 mod m`, on the constant-time path. CIOS has read
    /// every limb of `x` before the first is written back.
    fn product_assign_secret(&self, x: &mut Uint<N>, b: &Uint<N>, t: &mut Scratchpad) {
        let carry = self.core_secret(x, b, t);
        self.finish_secret(t.get(..self.used).unwrap_or_default(), carry, x);
    }

    /// `x = x² · R^-1 mod m` for `x` below `m`, on the constant-time path, by
    /// the dedicated squaring. It has read every limb of `x` before the
    /// first is written back.
    fn square_assign_secret(&self, x: &mut Uint<N>, t: &mut Scratchpad) {
        let s = self.used;
        let carry = self.square_core_secret(x, t);
        self.finish_secret(t.get(s..2 * s).unwrap_or_default(), carry, x);
        tally!(squarings, 1);
    }

    /// The sixteen powers `base^0` to `base^15` in Montgomery form: entry 0
    /// is `R mod m`, Montgomery's one, and entry `k` is entry `k - 1` times
    /// `base · R`, which `work.entry` holds while the table is built.
    fn build_table(&self, base: &Uint<N>, work: &mut Workspace<N>) {
        let Workspace {
            table,
            entry,
            scratch,
            ..
        } = work;
        self.product_secret(base, &self.r2, entry, scratch);
        if let Some(first) = table.first_mut() {
            self.product_secret(&Uint::from_u64(1), &self.r2, first, scratch);
        }
        for k in 1..table.len() {
            let (done, rest) = table.split_at_mut(k);
            if let (Some(previous), Some(next)) = (done.last(), rest.first_mut()) {
                self.product_secret(previous, entry, next, scratch);
            }
            tally!(branches, 1);
        }
    }

    /// `base^exponent mod m`, with the base, the exponent or both secret: the
    /// constant-time path's exponentiation, by fixed windows.
    ///
    /// The exponent is consumed four bits at a time from the top, over
    /// `⌈bits(m) / 4⌉` windows whatever its own length, so its leading zeros
    /// take exactly as long as leading ones would (rule 3). Each window
    /// squares four times and multiplies by the table entry the window names,
    /// read by scanning the whole table (rule 2) — the multiplication made
    /// even for a window of zero, by `base^0`. Every product's final
    /// subtraction is the branch-free one (rule 4).
    ///
    /// `base` must fit in the modulus's limbs — below `2^(64·used)`, which
    /// Montgomery's conversion reduces whatever it is below that — and the
    /// exponent's bits above the last window are not read: every exponent the
    /// callers use is below the modulus it works under. The result is written
    /// into `out`, so that a caller holding it on the heap never has it
    /// returned by value; the workspace is wiped before this returns.
    pub fn pow_secret(&self, base: &Uint<N>, exponent: &Uint<N>, out: &mut Uint<N>) {
        let mut work = Workspace::<N>::new();
        self.build_table(base, &mut work);

        let Workspace {
            table,
            accumulator,
            entry,
            scratch,
        } = &mut work;
        if let Some(one) = table.first() {
            accumulator.limbs = one.limbs;
        }
        let windows = self.bits.div_ceil(4);
        for window in (0..windows).rev() {
            for _ in 0..4 {
                self.square_assign_secret(accumulator, scratch);
                tally!(branches, 1);
            }
            let bit = window * 4;
            let nibble = (exponent.limb(bit / 64) >> (bit % 64)) & 0xf;
            select_entry(table, nibble, self.used, entry);
            self.product_assign_secret(accumulator, entry, scratch);
            tally!(branches, 1);
        }
        self.product_secret(accumulator, &Uint::from_u64(1), out, scratch);
    }
}

/// Copies `table[index]` into `out` by reading every entry and keeping the
/// wanted one by mask (rule 2): the memory a selection touches is the whole
/// table whichever entry the secret names.
fn select_entry<const N: usize>(table: &[Uint<N>; 16], index: u64, used: usize, out: &mut Uint<N>) {
    for limb in &mut out.limbs {
        *limb = 0;
    }
    for (position, entry) in (0u64..).zip(table.iter()) {
        let keep = equal_mask(position, index);
        for (limb, &held) in out.limbs.iter_mut().zip(&entry.limbs).take(used) {
            *limb |= held & keep;
        }
        tally!(table_reads, 1);
        tally!(selects, 1);
        tally!(branches, 1);
    }
}

/// CIOS (Koç, Acar and Kaliski 1996) over slices cut to the modulus's width:
/// `t + carry · 2^(64s) = a · b · R^-1`, below `2m` for `a` below `R` and `b`
/// below `m`, where `s` is `m.len()` and `a`, `b` and `t` are as long. `t` is
/// zeroed first; the carry limb is returned.
///
/// Each of the `s` rounds adds `a · b_i` into `t`, then the multiple of `m`
/// that clears `t`'s low limb, and moves `t` down a limb. The two limbs above
/// the width are held in registers between rounds, and every loop is a walk
/// over the slices, so no limb is read or written through a bounds check;
/// every loop runs `s` times, and nothing branches on a value. A round forms
/// `s` limb products in each half.
fn montgomery_product(a: &[u64], b: &[u64], m: &[u64], n0inv: u64, t: &mut [u64]) -> u64 {
    for limb in t.iter_mut() {
        *limb = 0;
    }
    let Some((&m0, m_rest)) = m.split_first() else {
        return 0;
    };
    #[cfg(test)]
    let mut multiplying = 0u64;
    #[cfg(test)]
    let mut reducing = 0u64;

    // `t[s]` between rounds: a value below `2m` needs no limb above it.
    let mut top = 0u64;
    for &bi in b {
        let bi = u128::from(bi);
        let mut carry = 0u64;
        for (limb, &aj) in t.iter_mut().zip(a) {
            let product = u128::from(*limb) + u128::from(aj) * bi + u128::from(carry);
            *limb = product as u64;
            carry = (product >> 64) as u64;
            #[cfg(test)]
            {
                multiplying += 1;
            }
        }
        // `t[s]` and `t[s + 1]` once `a · b_i` is in.
        let sum = u128::from(top) + u128::from(carry);
        let (above, overflow) = (sum as u64, (sum >> 64) as u64);

        let Some((first, rest)) = t.split_first_mut() else {
            return 0;
        };
        let factor = u128::from(first.wrapping_mul(n0inv));
        let cleared = u128::from(*first) + factor * u128::from(m0);
        let mut carry = (cleared >> 64) as u64;
        #[cfg(test)]
        {
            reducing += 1;
        }
        // Each limb is written one below the limb it was read from: the
        // division by 2^64 that the cleared low limb makes exact.
        let mut below = first;
        for (limb, &mj) in rest.iter_mut().zip(m_rest) {
            let product = u128::from(*limb) + factor * u128::from(mj) + u128::from(carry);
            *below = product as u64;
            carry = (product >> 64) as u64;
            below = limb;
            #[cfg(test)]
            {
                reducing += 1;
            }
        }
        let sum = u128::from(above) + u128::from(carry);
        *below = sum as u64;
        top = overflow.wrapping_add((sum >> 64) as u64);
    }
    tally!(limb_products, multiplying + reducing);
    tally!(multiplication_limb_products, multiplying);
    top
}

/// `a² · R^-1` for `a` below `m`, by a squaring of its own: `t`, of `2s` limbs
/// for `s = m.len()` and zeroed first, receives the whole square `a²`, which
/// is then reduced where it lies. The result's low limbs are left in
/// `t[s..]`, its carry limb is returned, and the value is below `2m`.
///
/// The cross products `a_i · a_j` with `i < j` are each formed once: row `i`
/// adds `a_i · a[i+1..]` at limb `2i + 1`, and its carry lands on limb
/// `i + s`, which no earlier row reached. Their sum, below `2^(128s - 1)`, is
/// doubled by a shift whose carry is arithmetic, and the squares `a_i²` are
/// added at limbs `2i` and `2i + 1`: `s(s - 1)/2 + s = s(s + 1)/2` limb
/// products, where a product's multiplication half forms `s²`. The reduction
/// is the separated form of the same Montgomery reduction: for each low limb
/// in turn, the multiple of `m` that clears it, `s` limb products a limb, the
/// carry out of its top held for the next. Every loop runs over the limb
/// count, and nothing branches on a value.
fn montgomery_square(a: &[u64], m: &[u64], n0inv: u64, t: &mut [u64]) -> u64 {
    let s = m.len();
    for limb in t.iter_mut() {
        *limb = 0;
    }
    #[cfg(test)]
    let mut multiplying = 0u64;
    #[cfg(test)]
    let mut reducing = 0u64;

    // The cross products, each once: row `i` covers limbs `2i + 1` to `i + s`.
    for (i, &ai) in a.iter().enumerate() {
        let (Some(row), Some(upper)) = (t.get_mut(2 * i + 1..=i + s), a.get(i + 1..)) else {
            break;
        };
        let Some((spill, row)) = row.split_last_mut() else {
            break;
        };
        let ai = u128::from(ai);
        let mut carry = 0u64;
        for (limb, &aj) in row.iter_mut().zip(upper) {
            let product = u128::from(*limb) + ai * u128::from(aj) + u128::from(carry);
            *limb = product as u64;
            carry = (product >> 64) as u64;
            #[cfg(test)]
            {
                multiplying += 1;
            }
        }
        *spill = carry;
    }

    // Doubled: the cross products sum to below 2^(128s - 1), so nothing
    // leaves the top limb.
    let mut carry = 0u64;
    for limb in t.iter_mut() {
        let value = *limb;
        *limb = (value << 1) | carry;
        carry = value >> 63;
    }

    // The squares, each at its own two limbs, the carry passed along.
    let mut carry = 0u64;
    for (&ai, pair) in a.iter().zip(t.chunks_exact_mut(2)) {
        let [low, high] = pair else {
            break;
        };
        let square = u128::from(ai) * u128::from(ai);
        let sum = u128::from(*low) + u128::from(square as u64) + u128::from(carry);
        *low = sum as u64;
        let sum = u128::from(*high) + (square >> 64) + (sum >> 64);
        *high = sum as u64;
        carry = (sum >> 64) as u64;
        #[cfg(test)]
        {
            multiplying += 1;
        }
    }

    // The reduction, a low limb at a time: limbs `i` to `i + s - 1` take the
    // multiple of `m` that clears limb `i`, and limb `i + s` its carry and
    // the one the row before passed up.
    let mut extra = 0u64;
    for i in 0..s {
        let Some(window) = t.get_mut(i..=i + s) else {
            break;
        };
        let Some((spill, window)) = window.split_last_mut() else {
            break;
        };
        let Some(&low) = window.first() else {
            break;
        };
        let factor = u128::from(low.wrapping_mul(n0inv));
        let mut carry = 0u64;
        for (limb, &mj) in window.iter_mut().zip(m) {
            let product = u128::from(*limb) + factor * u128::from(mj) + u128::from(carry);
            *limb = product as u64;
            carry = (product >> 64) as u64;
            #[cfg(test)]
            {
                reducing += 1;
            }
        }
        let sum = u128::from(*spill) + u128::from(carry) + u128::from(extra);
        *spill = sum as u64;
        extra = (sum >> 64) as u64;
    }
    tally!(limb_products, multiplying + reducing);
    tally!(multiplication_limb_products, multiplying);
    extra
}

/// Takes `m` off a value below `2m` held as `x`'s low `used` limbs and a
/// `carry` limb, or not, by mask (rule 4).
///
/// It needs reducing when the carry is set, or when the low limbs are at
/// least `m` — which is when `x - m` does not borrow. The borrow is only known
/// once the whole difference is, so the difference is taken twice: once for
/// the borrow, and once to keep its limbs or not. When the carry is set the
/// low limbs are below `m` and the difference wraps to exactly
/// `2^(64·used) + x - m`, the answer.
fn reduce_once<const N: usize>(x: &mut Uint<N>, m: &Uint<N>, used: usize, carry: u64) {
    let mut borrow = 0u64;
    for (&limb, &modulus) in x.limbs.iter().zip(&m.limbs).take(used) {
        borrow = subtract_limb(limb, modulus, borrow).1;
    }
    let take = mask(nonzero(carry) | (borrow ^ 1));
    let mut borrow = 0u64;
    for (limb, &modulus) in x.limbs.iter_mut().zip(&m.limbs).take(used) {
        let (difference, next) = subtract_limb(*limb, modulus, borrow);
        borrow = next;
        *limb = select(take, difference, *limb);
    }
    tally!(selects, used as u64);
}

/// `out = value << bits` for `bits` below 64, every limb written; bits pushed
/// past the top limb are lost, as [`Uint::shl_small`]'s are.
fn shift_left_into<const N: usize>(value: &Uint<N>, bits: u32, out: &mut Uint<N>) {
    if bits == 0 || bits >= 64 {
        out.limbs = value.limbs;
        return;
    }
    let mut carry = 0u64;
    for i in 0..N {
        let limb = value.limb(i);
        out.set_limb(i, (limb << bits) | carry);
        carry = limb >> (64 - bits);
    }
}

/// `out = a - b` over the low `limbs` limbs, the rest of `out` cleared, and
/// the borrow out of the top, as 0 or 1.
fn subtract_into<const N: usize>(a: &Uint<N>, b: &Uint<N>, out: &mut Uint<N>, limbs: usize) -> u64 {
    let mut borrow = 0u64;
    for j in 0..N {
        if j < limbs {
            let (difference, next) = subtract_limb(a.limb(j), b.limb(j), borrow);
            out.set_limb(j, difference);
            borrow = next;
        } else {
            out.set_limb(j, 0);
        }
    }
    borrow
}

/// `R^2 mod m` for a secret `m`, without a branch on it.
///
/// [`radix_mod`]'s steps, each conditional subtraction made every time and
/// kept by mask, and then `64 · used` doublings each reduced by
/// [`reduce_once`]. How many halving steps there are is the number of
/// leading zeros in `m`'s top limb, which its bit length fixes: the length is
/// public on this path, as every exponentiation's window count makes it.
fn r2_secret<const N: usize>(m: &Uint<N>, used: usize, out: &mut Uint<N>) {
    let mut work = Scratch::<N>::new();
    let [shifted, difference, ..] = &mut work.values;
    let z = m.limb(used.saturating_sub(1)).leading_zeros();

    shift_left_into(m, z, shifted);
    let mut borrow = 0u64;
    for j in 0..N {
        if j < used {
            let (limb, next) = subtract_limb(0, shifted.limb(j), borrow);
            out.set_limb(j, limb);
            borrow = next;
        } else {
            out.set_limb(j, 0);
        }
    }
    for i in (0..z).rev() {
        shift_left_into(m, i, shifted);
        let borrow = subtract_into(out, shifted, difference, used);
        let keep = mask(borrow ^ 1);
        for j in 0..used {
            out.set_limb(j, select(keep, difference.limb(j), out.limb(j)));
        }
    }

    for _ in 0..64 * used {
        let mut carry = 0u64;
        for j in 0..used {
            let limb = out.limb(j);
            out.set_limb(j, (limb << 1) | carry);
            carry = limb >> 63;
        }
        reduce_once(out, m, used, carry);
    }
}

/// Working values for the constant-time helpers below, wiped on drop as the
/// exponentiation's workspace is.
struct Scratch<const N: usize> {
    values: [Uint<N>; 4],
    t: Scratchpad,
}

impl<const N: usize> Scratch<N> {
    fn new() -> Self {
        Self {
            values: [Uint::zero(); 4],
            t: [0; SCRATCH_LIMBS],
        }
    }
}

impl<const N: usize> Drop for Scratch<N> {
    fn drop(&mut self) {
        for value in &mut self.values {
            wipe(&mut value.limbs);
        }
        wipe(&mut self.t);
    }
}

impl<const N: usize> Modulus<N> {
    /// `out = a · b · R^-1 mod m`, the Montgomery product, on the
    /// constant-time path: `a` any value below `R`, `b` below `m`.
    pub(crate) fn montgomery_secret(&self, a: &Uint<N>, b: &Uint<N>, out: &mut Uint<N>) {
        let mut work = Scratch::<N>::new();
        self.product_secret(a, b, out, &mut work.t);
    }

    /// `x = x² · R^-1 mod m` in place for `x` below `m`, on the constant-time
    /// path, by the dedicated squaring.
    pub(crate) fn square_secret(&self, x: &mut Uint<N>) {
        let mut work = Scratch::<N>::new();
        self.square_assign_secret(x, &mut work.t);
    }

    /// `out = a · R mod m`: into Montgomery form, for any `a` below `R`.
    pub(crate) fn to_montgomery_secret(&self, a: &Uint<N>, out: &mut Uint<N>) {
        self.montgomery_secret(a, &self.r2, out);
    }

    /// `out = a · b mod m` for ordinary operands — `a` below `R`, `b` below
    /// `m` — as two Montgomery products: `a · R`, then `(a · R) · b · R^-1`.
    pub(crate) fn mul_mod_secret(&self, a: &Uint<N>, b: &Uint<N>, out: &mut Uint<N>) {
        let mut work = Scratch::<N>::new();
        let Scratch { values, t } = &mut work;
        let [a_montgomery, ..] = values;
        self.product_secret(a, &self.r2, a_montgomery, t);
        self.product_secret(a_montgomery, b, out, t);
    }

    /// `out = (a + b) mod m` for `a` and `b` below `m`, on the constant-time
    /// path.
    pub(crate) fn add_secret(&self, a: &Uint<N>, b: &Uint<N>, out: &mut Uint<N>) {
        let s = self.used;
        let mut carry = 0u64;
        for j in 0..N {
            if j < s {
                let sum = u128::from(a.limb(j)) + u128::from(b.limb(j)) + u128::from(carry);
                out.set_limb(j, sum as u64);
                carry = (sum >> 64) as u64;
            } else {
                out.set_limb(j, 0);
            }
        }
        reduce_once(out, &self.m, s, carry);
    }

    /// `out = (a - b) mod m` for `a` and `b` below `m`, on the constant-time
    /// path: the difference, and `m` added back under a mask of its borrow.
    pub(crate) fn sub_secret(&self, a: &Uint<N>, b: &Uint<N>, out: &mut Uint<N>) {
        let s = self.used;
        let borrow = subtract_into(a, b, out, s);
        let add = mask(borrow);
        let mut carry = 0u64;
        for j in 0..s {
            let sum =
                u128::from(out.limb(j)) + u128::from(self.m.limb(j) & add) + u128::from(carry);
            out.set_limb(j, sum as u64);
            carry = (sum >> 64) as u64;
        }
    }

    /// `out = value mod m` for a `value` of `value_limbs` limbs — a count the
    /// caller states and that is public — on the constant-time path.
    ///
    /// Horner's rule over chunks of the modulus's own width, from the top: in
    /// Montgomery form a partial result `A · R` becomes `(A · R + c) · R` by
    /// one product with `R^2` and the chunk `c`'s own product with `R^2`,
    /// added. Each chunk is below `R`, which is all a product asks of it, so
    /// a value any number of times wider than the modulus reduces in as many
    /// steps — an RSA representative under each prime, a half under the
    /// other prime — with no comparison of the value against anything.
    pub(crate) fn reduce_secret(&self, value: &Uint<N>, value_limbs: usize, out: &mut Uint<N>) {
        let s = self.used;
        let mut work = Scratch::<N>::new();
        let Scratch { values, t } = &mut work;
        let [accumulator, shifted, chunk_montgomery, chunk] = values;
        let chunks = value_limbs.div_ceil(s.max(1)).max(1);
        for index in (0..chunks).rev() {
            for j in 0..N {
                chunk.set_limb(j, if j < s { value.limb(index * s + j) } else { 0 });
            }
            self.product_secret(accumulator, &self.r2, shifted, t);
            self.product_secret(chunk, &self.r2, chunk_montgomery, t);
            self.add_secret(shifted, chunk_montgomery, accumulator);
        }
        self.product_secret(accumulator, &Uint::from_u64(1), out, t);
    }

    /// `base^exponent mod m` with a secret base and a **public** exponent —
    /// RSA's `e`, raising a blinding factor or checking a signature — by
    /// square and multiply over the exponent's bits, every product the
    /// constant-time one. The branches are on the exponent, which every
    /// verifier holds.
    pub(crate) fn pow_secret_base(&self, base: &Uint<N>, exponent: &Uint<N>, out: &mut Uint<N>) {
        let mut work = Scratch::<N>::new();
        let Scratch { values, t } = &mut work;
        let [base_montgomery, accumulator, ..] = values;
        self.product_secret(base, &self.r2, base_montgomery, t);
        self.product_secret(&Uint::from_u64(1), &self.r2, accumulator, t);
        for i in (0..exponent.bits()).rev() {
            self.square_assign_secret(accumulator, t);
            if exponent.bit(i) {
                self.product_assign_secret(accumulator, base_montgomery, t);
            }
        }
        self.product_secret(accumulator, &Uint::from_u64(1), out, t);
    }
}

/// Whether `a == b`, every limb read whatever the limbs hold. The answer is
/// the one thing the caller learns, and it is what the caller acts on.
pub(crate) fn equal_secret<const N: usize>(a: &Uint<N>, b: &Uint<N>) -> bool {
    let mut difference = 0u64;
    for j in 0..N {
        difference |= a.limb(j) ^ b.limb(j);
    }
    nonzero(difference) == 0
}

/// Whether `a < b`: the borrow out of `a - b`, every limb read.
pub(crate) fn less_secret<const N: usize>(a: &Uint<N>, b: &Uint<N>) -> bool {
    let mut borrow = 0u64;
    for j in 0..N {
        borrow = subtract_limb(a.limb(j), b.limb(j), borrow).1;
    }
    borrow == 1
}

/// `out = a - small` over every limb; `a` must be at least `small`.
pub(crate) fn subtract_small_secret<const N: usize>(a: &Uint<N>, small: u64, out: &mut Uint<N>) {
    let mut borrow = 0u64;
    for j in 0..N {
        let subtrahend = if j == 0 { small } else { 0 };
        let (difference, next) = subtract_limb(a.limb(j), subtrahend, borrow);
        out.set_limb(j, difference);
        borrow = next;
    }
}

/// Whether `a · b == c` exactly, by a full schoolbook product over every limb
/// of both: what an RSA key's `p · q = n` check asks, without a branch on
/// either prime.
pub(crate) fn product_equals<const N: usize>(a: &Uint<N>, b: &Uint<N>, c: &Uint<N>) -> bool {
    let mut wide = [0u64; 2 * MAX_LIMBS];
    for i in 0..N {
        let ai = u128::from(a.limb(i));
        let mut carry = 0u64;
        for j in 0..N {
            let sum =
                u128::from(read(&wide, i + j)) + ai * u128::from(b.limb(j)) + u128::from(carry);
            write(&mut wide, i + j, sum as u64);
            carry = (sum >> 64) as u64;
        }
        write(&mut wide, i + N, carry);
    }
    let mut difference = 0u64;
    for (k, &limb) in wide.iter().enumerate() {
        difference |= limb ^ c.limb(k);
    }
    wipe(&mut wide);
    nonzero(difference) == 0
}

// ---- Secret values ---------------------------------------------------------

/// A secret number: `N` limbs in one heap allocation, zeroised when it is
/// dropped and never printed (`docs/design/signing.md`, *Key material*).
///
/// Rust moves a stack value by copying its bytes and leaves the old ones in
/// place, so a secret is never a stack value that moves: the limbs sit behind
/// a `Box`, and moving a `Secret` moves the pointer. It is neither `Copy` nor
/// `Clone`; [`Secret::duplicate`] is the explicit copy, and itself a
/// `Secret`. `Debug` writes `[redacted]`.
pub struct Secret<const N: usize> {
    value: Box<Uint<N>>,
}

impl<const N: usize> Secret<N> {
    /// Zero, on the heap: the shape every secret starts in before it is
    /// written in place.
    #[must_use]
    pub fn zero() -> Self {
        Self {
            value: Box::new(Uint::zero()),
        }
    }

    /// Reads big-endian bytes straight into the heap allocation, or `None`
    /// when they are too wide for `N` limbs. The caller's bytes stay the
    /// caller's to wipe.
    #[must_use]
    pub fn from_be_bytes(bytes: &[u8]) -> Option<Self> {
        let mut secret = Self::zero();
        if secret.value.read_be_bytes(bytes) {
            Some(secret)
        } else {
            None
        }
    }

    /// The number.
    #[must_use]
    pub fn value(&self) -> &Uint<N> {
        &self.value
    }

    /// The number, to be written in place.
    pub(crate) fn value_mut(&mut self) -> &mut Uint<N> {
        &mut self.value
    }

    /// A second `Secret` holding the same number, in its own allocation.
    #[must_use]
    pub fn duplicate(&self) -> Self {
        let mut copy = Self::zero();
        copy.value.limbs = self.value.limbs;
        copy
    }
}

impl<const N: usize> Drop for Secret<N> {
    fn drop(&mut self) {
        #[cfg(test)]
        let held = !self.value.is_zero();
        wipe(&mut self.value.limbs);
        #[cfg(test)]
        dropped::record(held, self.value.is_zero());
    }
}

impl<const N: usize> core::fmt::Debug for Secret<N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("[redacted]")
    }
}

/// An odd modulus that is itself secret — an RSA prime — with its Montgomery
/// constants, all on the heap, computed on the constant-time path, wiped on
/// drop and never printed.
pub struct SecretModulus<const N: usize> {
    modulus: Box<Modulus<N>>,
}

impl<const N: usize> SecretModulus<N> {
    /// Precomputes for a secret `m`, straight into the heap, or refuses it as
    /// [`Modulus::new`] does: even, below 3, or wider than [`MAX_LIMBS`].
    ///
    /// The refusal is the one thing that depends on `m`'s value rather than
    /// its length, and it says only that `m` is not a modulus. `R^2 mod m` is
    /// computed without a branch on `m` ([`r2_secret`]).
    #[must_use]
    pub fn new(m: &Secret<N>) -> Option<Self> {
        let value = m.value();
        let bits = value.bits();
        if N > MAX_LIMBS || !value.is_odd() || bits < 2 {
            return None;
        }
        let used = bits.div_ceil(64);
        let mut modulus = Box::new(Modulus {
            m: Uint::zero(),
            used,
            bits,
            n0inv: 0,
            r2: Uint::zero(),
        });
        {
            let Modulus {
                m: held, n0inv, r2, ..
            } = &mut *modulus;
            held.limbs = value.limbs;
            *n0inv = montgomery_n0inv(held.limb(0));
            r2_secret(held, used, r2);
        }
        Some(Self { modulus })
    }

    /// The modulus, for the constant-time path's operations — and only those:
    /// the public path's [`Modulus::pow`] and [`Modulus::mul`] branch on
    /// values, and this modulus is one.
    pub(crate) fn modulus(&self) -> &Modulus<N> {
        &self.modulus
    }
}

impl<const N: usize> Drop for SecretModulus<N> {
    fn drop(&mut self) {
        #[cfg(test)]
        let held = !self.modulus.m.is_zero();
        let modulus = &mut *self.modulus;
        wipe(&mut modulus.m.limbs);
        wipe(&mut modulus.r2.limbs);
        wipe(core::slice::from_mut(&mut modulus.n0inv));
        #[cfg(test)]
        dropped::record(
            held,
            modulus.m.is_zero() && modulus.r2.is_zero() && modulus.n0inv == 0,
        );
    }
}

impl<const N: usize> core::fmt::Debug for SecretModulus<N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("[redacted]")
    }
}

/// Secret octets on the heap — a blinding factor's draw, a salt drawn from
/// the caller's source — wiped when dropped and never printed.
pub(crate) struct SecretBytes {
    bytes: Vec<u8>,
}

impl SecretBytes {
    /// `len` zero octets.
    pub(crate) fn new(len: usize) -> Self {
        Self {
            bytes: vec![0; len],
        }
    }

    pub(crate) fn as_slice(&self) -> &[u8] {
        &self.bytes
    }

    pub(crate) fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.bytes
    }
}

impl Drop for SecretBytes {
    fn drop(&mut self) {
        #[cfg(test)]
        let held = self.bytes.iter().any(|&byte| byte != 0);
        for byte in self.bytes.iter_mut() {
            *byte = 0;
        }
        let _ = core::hint::black_box(&mut self.bytes[..]);
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
        #[cfg(test)]
        dropped::record(held, self.bytes.iter().all(|&byte| byte == 0));
    }
}

impl core::fmt::Debug for SecretBytes {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("[redacted]")
    }
}

/// The test-only hook in every secret's `Drop`: what each dropped secret held
/// before its wipe, and whether it read zero after (`docs/design/signing.md`,
/// milestone 3). The crates forbid `unsafe`, so freed memory cannot be read
/// back; this reads the limbs at the last moment they are still the secret's.
#[cfg(test)]
pub(crate) mod dropped {
    use std::cell::RefCell;

    /// One dropped secret.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) struct Wiped {
        /// It held something other than zero before the wipe.
        pub(crate) held: bool,
        /// Every limb, constant and octet read zero after it.
        pub(crate) zero: bool,
    }

    thread_local! {
        static LOG: RefCell<Vec<Wiped>> = const { RefCell::new(Vec::new()) };
    }

    pub(crate) fn record(held: bool, zero: bool) {
        LOG.with(|log| log.borrow_mut().push(Wiped { held, zero }));
    }

    /// This thread's drops since the last call.
    pub(crate) fn take() -> Vec<Wiped> {
        LOG.with(|log| std::mem::take(&mut *log.borrow_mut()))
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

    // ---- The constant-time path ----------------------------------------

    /// SplitMix64: a fixed stream of test values, so that a failure names a
    /// seed and a draw anybody can repeat.
    struct Draws(u64);

    impl Draws {
        fn draw(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^ (z >> 31)
        }
    }

    /// Every bit below `bits` set to `limb`'s pattern, the rest clear.
    fn pattern<const N: usize>(bits: usize, limb: u64) -> Uint<N> {
        let mut value = Uint::<N>::zero();
        for i in 0..bits.div_ceil(64) {
            value.set_limb(i, limb);
        }
        clear_above(&mut value, bits);
        value
    }

    fn clear_above<const N: usize>(value: &mut Uint<N>, bits: usize) {
        for i in 0..N {
            let low = i * 64;
            if low >= bits {
                value.set_limb(i, 0);
            } else if bits - low < 64 {
                value.set_limb(i, value.limb(i) & ((1u64 << (bits - low)) - 1));
            }
        }
    }

    /// A draw of at most `bits` bits.
    fn drawn<const N: usize>(draws: &mut Draws, bits: usize) -> Uint<N> {
        let mut value = Uint::<N>::zero();
        for i in 0..bits.div_ceil(64) {
            value.set_limb(i, draws.draw());
        }
        clear_above(&mut value, bits);
        value
    }

    /// An odd modulus of exactly `bits` bits.
    fn odd_modulus<const N: usize>(draws: &mut Draws, bits: usize) -> Uint<N> {
        let mut m = drawn::<N>(draws, bits);
        let top = bits - 1;
        m.set_limb(top / 64, m.limb(top / 64) | (1u64 << (top % 64)));
        m.set_limb(0, m.limb(0) | 1);
        m
    }

    fn secret_power<const N: usize>(
        modulus: &Modulus<N>,
        base: &Uint<N>,
        exponent: &Uint<N>,
    ) -> Uint<N> {
        let mut out = Uint::<N>::zero();
        modulus.pow_secret(base, exponent, &mut out);
        out
    }

    /// SG-01.2's exit, extended to the dedicated squaring by P-13.8: the
    /// constant-time exponentiation's operation counts — Montgomery products
    /// and the squarings among them, limb products in all and in the
    /// multiplication halves, table reads, masked selections and loop
    /// iterations — are the same for every exponent and every base under
    /// one modulus, and the answer is the public path's.
    ///
    /// The exponents are every shape a leak would show on: zero, one, every
    /// bit set, the two alternating patterns, the top bit alone, `m - 2` (the
    /// Fermat exponent the inverse takes) and random draws; the bases zero,
    /// one, two, `m - 1` and random. The moduli are a full limb, a partial top
    /// limb, four limbs with the top one short a bit, and 521 bits inside a
    /// nine-limb type — the shape in which every limb above the modulus has
    /// to come back clear — and two random 1 024-bit moduli inside RSA's
    /// sixty-four limbs, sixteen used: a CRT half of an RSA-2048 key as the
    /// key holds it, the width the core is for. Each count is also held to
    /// what the source should do, so a count that agreed with itself by doing
    /// nothing would fail too.
    #[test]
    fn the_private_path_counts_alike_over_every_exponent() {
        let mut draws = Draws(0x5347_2d30_312e_3220);
        for bits in [64usize, 100, 255, 521] {
            counts_alike_under_one_modulus::<9>(&mut draws, bits, 4);
        }
        for _ in 0..2 {
            counts_alike_under_one_modulus::<MAX_LIMBS>(&mut draws, 1024, 2);
        }
    }

    /// One random odd modulus of `bits` bits in an `N`-limb type, and the
    /// exponentiation over the exponents and bases of
    /// `the_private_path_counts_alike_over_every_exponent`, `random` of each
    /// drawn: the answers the public path's, the counts equal over every
    /// pair and what the source should do.
    fn counts_alike_under_one_modulus<const N: usize>(
        draws: &mut Draws,
        bits: usize,
        random: usize,
    ) {
        let m = odd_modulus::<N>(draws, bits);
        let modulus = Modulus::new(m).expect("odd and wide");
        let (m_minus_two, _) = m.sub_borrow(&Uint::from_u64(2));
        let (m_minus_one, _) = m.sub_borrow(&Uint::from_u64(1));
        let mut top_bit = Uint::<N>::zero();
        top_bit.set_limb((bits - 1) / 64, 1u64 << ((bits - 1) % 64));

        let mut exponents = vec![
            Uint::<N>::zero(),
            Uint::from_u64(1),
            pattern(bits, u64::MAX),
            pattern(bits, 0x5555_5555_5555_5555),
            pattern(bits, 0xaaaa_aaaa_aaaa_aaaa),
            top_bit,
            m_minus_two,
        ];
        let mut bases = vec![
            Uint::<N>::zero(),
            Uint::from_u64(1),
            Uint::from_u64(2),
            m_minus_one,
        ];
        for _ in 0..random {
            exponents.push(drawn(draws, bits));
            bases.push(drawn(draws, bits - 1));
        }

        let mut first: Option<Counts> = None;
        for base in &bases {
            for exponent in &exponents {
                let _ = counts::take();
                let got = secret_power(&modulus, base, exponent);
                let counted = counts::take();
                assert_eq!(
                    got,
                    modulus.pow(base, exponent),
                    "{bits}-bit modulus, base {base:?}, exponent {exponent:?}"
                );
                match first {
                    None => first = Some(counted),
                    Some(expected) => assert_eq!(
                        counted, expected,
                        "{bits}-bit modulus: the counts moved with base {base:?}, exponent {exponent:?}"
                    ),
                }
            }
        }

        let counted = first.expect("at least one run");
        let windows = bits.div_ceil(4) as u64;
        let s = bits.div_ceil(64) as u64;
        assert_eq!(
            counted.products,
            5 * windows + 18,
            "two conversions, fifteen table steps, five products a window and one back"
        );
        assert_eq!(
            counted.squarings,
            4 * windows,
            "four squarings a window, each the dedicated one"
        );
        let multiplied = counted.products - counted.squarings;
        assert_eq!(
            counted.multiplication_limb_products,
            multiplied * s * s + counted.squarings * s * (s + 1) / 2,
            "{bits} bits: n² limb products a product multiplies, n(n+1)/2 a squaring"
        );
        assert_eq!(
            counted.limb_products,
            counted.multiplication_limb_products + counted.products * s * s,
            "{bits} bits: and the same reduction after each, n² more"
        );
        assert_eq!(
            counted.table_reads,
            16 * windows,
            "the whole table, every window"
        );
        assert_eq!(counted.selects, 16 * windows + counted.products * s);
        assert_eq!(counted.branches, 21 * windows + 15);
    }

    /// P-13.8's exit: the private path's squaring forms each cross product
    /// `a_i · a_j`, `i < j`, once and doubles their sum, so that for `n = 16`
    /// — a 1 024-bit CRT half — its multiplication half forms `n(n+1)/2` limb
    /// products where a product's forms `n²`, and it then reduces as a
    /// product does, `n²` more, with the same final subtraction by mask.
    ///
    /// And it is the public path's `mul(a, a)`, which
    /// `montgomery_products_agree_with_u128` holds to `u128`, on random
    /// operands and on the operands that carry most: zero, one, `m - 1`,
    /// `m - 2` and every bit below the top set, under random moduli of every
    /// width from two bits to sixteen limbs — a top limb full, short and one
    /// bit long — and under the all-ones moduli of one, two and sixteen
    /// limbs, inside a type wider than the modulus and one it fills.
    #[test]
    fn a_squaring_forms_each_cross_product_once() {
        let mut draws = Draws(0x5031_332e_3820);
        let mut scratch = [0u64; SCRATCH_LIMBS];

        let m = odd_modulus::<MAX_LIMBS>(&mut draws, 1024);
        let modulus = Modulus::new(m).expect("odd and wide");
        assert_eq!(modulus.limbs(), 16);
        for _ in 0..4 {
            let x = drawn::<MAX_LIMBS>(&mut draws, 1023);
            let _ = counts::take();
            let mut squared = x;
            modulus.square_assign_secret(&mut squared, &mut scratch);
            let square = counts::take();
            let mut product = Uint::<MAX_LIMBS>::zero();
            modulus.product_secret(&x, &x, &mut product, &mut scratch);
            let multiplied = counts::take();

            assert_eq!(
                squared, product,
                "the squaring is the value's product with itself"
            );
            assert_eq!(squared, modulus.mul(&x, &x));
            assert_eq!((square.products, square.squarings), (1, 1));
            assert_eq!((multiplied.products, multiplied.squarings), (1, 0));
            assert_eq!(
                square.multiplication_limb_products,
                16 * 17 / 2,
                "a hundred and twenty cross products once, and sixteen squares"
            );
            assert_eq!(multiplied.multiplication_limb_products, 16 * 16);
            assert_eq!(
                square.limb_products - square.multiplication_limb_products,
                16 * 16,
                "the reduction a product makes"
            );
            assert_eq!(
                multiplied.limb_products - multiplied.multiplication_limb_products,
                16 * 16
            );
            assert_eq!(square.selects, multiplied.selects, "one final subtraction");
            assert_eq!(square.selects, 16);
        }

        let mut moduli: Vec<Uint<16>> = vec![
            pattern(64, u64::MAX),
            pattern(128, u64::MAX),
            pattern(1024, u64::MAX),
            Uint::from_u64(3),
        ];
        for bits in [
            2usize, 3, 63, 64, 65, 127, 128, 129, 191, 192, 255, 300, 512, 513, 777, 1000, 1023,
            1024,
        ] {
            moduli.push(odd_modulus(&mut draws, bits));
        }
        for m in moduli {
            let modulus = Modulus::new(m).expect("odd and above 2");
            let bits = modulus.bits();
            let (m_minus_one, _) = m.sub_borrow(&Uint::from_u64(1));
            let (m_minus_two, _) = m.sub_borrow(&Uint::from_u64(2));
            let mut operands = vec![
                Uint::<16>::zero(),
                Uint::from_u64(1),
                m_minus_one,
                m_minus_two,
                pattern(bits - 1, u64::MAX),
            ];
            for _ in 0..8 {
                operands.push(drawn(&mut draws, bits - 1));
            }
            for x in operands {
                let mut squared = x;
                modulus.square_assign_secret(&mut squared, &mut scratch);
                assert_eq!(
                    squared,
                    modulus.mul(&x, &x),
                    "{bits}-bit modulus, {x:?} squared"
                );
                let mut square = x;
                modulus.square_secret(&mut square);
                assert_eq!(square, squared);
            }
        }
    }

    /// The constant-time path against repeated multiplication in `u128`, which
    /// shares no code with either path.
    #[test]
    fn the_private_exponentiation_agrees_with_repeated_multiplication() {
        let m = 0xffff_ffff_ffff_fffbu128;
        let modulus = Modulus::new(from_u128(m)).expect("odd");
        for base in [0u128, 1, 2, 3, 0x1234_5678_9abc_def0, m - 1] {
            let mut expected = 1u128;
            for exponent in 0u64..40 {
                let got = secret_power(&modulus, &from_u128(base), &Uint::<2>::from_u64(exponent));
                assert_eq!(as_u128(&got), expected, "{base:x}^{exponent}");
                expected = expected * base % m;
            }
        }
    }

    /// The branch-free product against the public one, which the tests above
    /// hold to `u128`, on both sides of the final subtraction — and with a
    /// one-limb modulus in a four-limb type, where the public path's borrow
    /// once leaked into the limbs above the modulus, the dedicated squaring
    /// beside the product there.
    #[test]
    fn a_constant_time_product_agrees_with_the_public_one() {
        let moduli: [u128; 4] = [
            0xffff_ffff_ffff_fffb,
            0x1_0000_0000_0000_0001,
            0x0123_4567_89ab_cdef_fedc_ba98_7654_3211,
            0x7fff_ffff_ffff_ffff_ffff_ffff_ffff_ffff,
        ];
        let mut scratch = [0u64; SCRATCH_LIMBS];
        for m in moduli {
            let modulus = Modulus::new(from_u128(m)).expect("odd and above 2");
            for a in SAMPLES {
                for b in SAMPLES {
                    let (a, b) = (from_u128(a % m), from_u128(b % m));
                    let mut out = Uint::<2>::zero();
                    modulus.product_secret(&a, &b, &mut out, &mut scratch);
                    assert_eq!(out, modulus.mul(&a, &b), "{a:?} * {b:?} mod {m:x}");
                }
            }
        }

        let m = 0xffff_ffff_ffff_fffbu128;
        let mut wide_m = Uint::<4>::zero();
        wide_m.set_limb(0, m as u64);
        let wide = Modulus::new(wide_m).expect("odd");
        for offset in 0..32u64 {
            for value in [offset, (m as u64) - 1 - offset, (m as u64) / 2 + offset] {
                let x = Uint::<4>::from_u64(value);
                let mut out = Uint::<4>::from_u64(u64::MAX);
                out.set_limb(3, u64::MAX);
                wide.product_secret(&x, &x, &mut out, &mut scratch);
                assert_eq!(out, wide.mul(&x, &x), "{value:x} squared");
                assert_eq!((out.limb(1), out.limb(2), out.limb(3)), (0, 0, 0));
                let mut squared = x;
                wide.square_assign_secret(&mut squared, &mut scratch);
                assert_eq!(squared, out, "{value:x} by the dedicated squaring");
            }
        }
    }

    /// A selection reads all sixteen entries whichever one it keeps, and
    /// keeps exactly that one. An index past the table matches nothing,
    /// which no window — four bits — can produce.
    #[test]
    fn a_window_is_read_by_scanning_the_whole_table() {
        let mut table = [Uint::<3>::zero(); 16];
        for (k, entry) in (0u64..).zip(table.iter_mut()) {
            for i in 0..3 {
                entry.set_limb(i, (k + 1) * 0x0101_0101_0101_0101 + i as u64);
            }
        }
        for index in 0..16u64 {
            let mut out = Uint::<3>::from_u64(u64::MAX);
            let _ = counts::take();
            select_entry(&table, index, 3, &mut out);
            let counted = counts::take();
            assert_eq!(Some(&out), table.get(index as usize), "entry {index}");
            assert_eq!(counted.table_reads, 16, "entry {index}");
        }
        let mut out = Uint::<3>::from_u64(7);
        select_entry(&table, 16, 3, &mut out);
        assert!(out.is_zero());
    }

    #[test]
    fn the_masks_choose_without_comparing() {
        assert_eq!(mask(0), 0);
        assert_eq!(mask(1), u64::MAX);
        assert_eq!(mask(2), 0, "only the low bit is a choice");
        assert_eq!(nonzero(0), 0);
        for value in [1u64, 2, 0x8000_0000_0000_0000, u64::MAX] {
            assert_eq!(nonzero(value), 1, "{value:x}");
        }
        assert_eq!(equal_mask(5, 5), u64::MAX);
        assert_eq!(equal_mask(5, 6), 0);
        assert_eq!(equal_mask(0, u64::MAX), 0);
        assert_eq!(select(u64::MAX, 3, 4), 3);
        assert_eq!(select(0, 3, 4), 4);
        assert_eq!(subtract_limb(5, 3, 1), (1, 0));
        assert_eq!(subtract_limb(0, 1, 0), (u64::MAX, 1));
        assert_eq!(subtract_limb(0, 0, 1), (u64::MAX, 1));
        assert_eq!(subtract_limb(u64::MAX, u64::MAX, 1), (u64::MAX, 1));
        assert_eq!(subtract_limb(u64::MAX, 0, 1), (u64::MAX - 1, 0));
    }

    #[test]
    fn a_wipe_leaves_zeros() {
        let mut limbs = [u64::MAX, 1, 0x0123_4567_89ab_cdef];
        wipe(&mut limbs);
        assert_eq!(limbs, [0, 0, 0]);
    }

    fn secret_of<const N: usize>(value: &Uint<N>) -> Secret<N> {
        let mut secret = Secret::zero();
        secret.value_mut().limbs = value.limbs;
        secret
    }

    /// A secret modulus's constants, computed without a branch on it, are the
    /// public path's: the same `R^2`, the same `-m^-1`, the same widths. The
    /// moduli cover a top limb with no leading zeros (no halving steps), one
    /// with sixty-three, and random widths inside a wider type.
    #[test]
    fn a_secret_modulus_has_the_public_constants() {
        let mut draws = Draws(0x7365_6372_6574);
        let mut moduli: Vec<Uint<9>> = vec![
            Uint::from_u64(3),
            Uint::from_u64(0xffff_ffff_ffff_fffb),
            pattern(128, u64::MAX),
        ];
        let mut short_top = Uint::<9>::from_u64(1);
        short_top.set_limb(1, 1);
        moduli.push(short_top);
        for bits in [65usize, 127, 300, 512, 575] {
            moduli.push(odd_modulus(&mut draws, bits));
        }
        for m in moduli {
            let public = Modulus::new(m).expect("odd and above 2");
            let secret = SecretModulus::new(&secret_of(&m)).expect("the same modulus");
            let held = secret.modulus();
            assert_eq!(held.m, public.m);
            assert_eq!((held.used, held.bits), (public.used, public.bits), "{m:?}");
            assert_eq!(held.n0inv, public.n0inv, "{m:?}");
            assert_eq!(held.r2, public.r2, "R^2 for {m:?}");
        }
        assert!(SecretModulus::new(&secret_of(&Uint::<9>::from_u64(1024))).is_none());
        assert!(SecretModulus::new(&secret_of(&Uint::<9>::from_u64(1))).is_none());
        assert!(SecretModulus::new(&Secret::<9>::zero()).is_none());
    }

    /// The constant-time reduction against the public one, for values one,
    /// two and many times the modulus's width — the shape a representative
    /// under one prime takes — and for values already below it.
    #[test]
    fn the_secret_reduction_agrees_with_the_public_one() {
        let mut draws = Draws(0x7265_6475_6365);
        for bits in [64usize, 100, 128, 190] {
            let m = odd_modulus::<9>(&mut draws, bits);
            let modulus = Modulus::new(m).expect("odd");
            for width in [1usize, 2, 3, 5, 9] {
                for _ in 0..6 {
                    let value = drawn::<9>(&mut draws, 64 * width);
                    let mut out = Uint::<9>::from_u64(u64::MAX);
                    modulus.reduce_secret(&value, width, &mut out);
                    assert_eq!(
                        out,
                        modulus.reduce(&value),
                        "{bits}-bit modulus, {width} limbs"
                    );
                }
            }
            let below = drawn::<9>(&mut draws, bits - 1);
            let mut out = Uint::<9>::zero();
            modulus.reduce_secret(&below, 9, &mut out);
            assert_eq!(out, below);
            modulus.reduce_secret(&m, 9, &mut out);
            assert!(out.is_zero(), "m reduces to zero");
        }
    }

    #[test]
    fn secret_addition_subtraction_and_products_agree_with_u128() {
        for m in [
            0xffff_ffff_ffff_fffbu128,
            0x0123_4567_89ab_cdef_fedc_ba98_7654_3211,
        ] {
            let modulus = Modulus::new(from_u128(m)).expect("odd");
            for a in SAMPLES {
                for b in SAMPLES {
                    let (a, b) = (a % m, b % m);
                    let (x, y) = (from_u128(a), from_u128(b));
                    let mut out = Uint::<2>::zero();
                    modulus.add_secret(&x, &y, &mut out);
                    assert_eq!(as_u128(&out), (a + b) % m, "{a:x} + {b:x}");
                    modulus.sub_secret(&x, &y, &mut out);
                    assert_eq!(as_u128(&out), (a + m - b) % m, "{a:x} - {b:x}");
                    modulus.mul_mod_secret(&x, &y, &mut out);
                    assert_eq!(as_u128(&out), mulmod(a, b, m), "{a:x} * {b:x}");
                    modulus.montgomery_secret(&x, &y, &mut out);
                    assert_eq!(out, modulus.mul(&x, &y));
                    let mut square = x;
                    modulus.square_secret(&mut square);
                    assert_eq!(square, modulus.mul(&x, &x));
                    modulus.to_montgomery_secret(&x, &mut out);
                    assert_eq!(out, modulus.to_montgomery(&x));
                }
            }
        }
    }

    #[test]
    fn a_secret_base_under_a_public_exponent_agrees_with_pow() {
        let m = 0x0123_4567_89ab_cdef_fedc_ba98_7654_3211u128;
        let modulus = Modulus::new(from_u128(m)).expect("odd");
        for base in SAMPLES {
            for exponent in [0u64, 1, 2, 3, 17, 65537, 0x260445] {
                let base = from_u128(base % m);
                let exponent = Uint::<2>::from_u64(exponent);
                let mut out = Uint::<2>::zero();
                modulus.pow_secret_base(&base, &exponent, &mut out);
                assert_eq!(out, modulus.pow(&base, &exponent));
            }
        }
    }

    #[test]
    fn the_secret_comparisons_agree_with_ord() {
        for a in SAMPLES {
            for b in SAMPLES {
                let (x, y) = (from_u128(a), from_u128(b));
                assert_eq!(equal_secret(&x, &y), a == b, "{a:x} == {b:x}");
                assert_eq!(less_secret(&x, &y), a < b, "{a:x} < {b:x}");
            }
            if a >= 2 {
                let mut out = Uint::<2>::zero();
                subtract_small_secret(&from_u128(a), 2, &mut out);
                assert_eq!(as_u128(&out), a - 2);
            }
        }
    }

    /// `a · b == c` is recognised exactly: products of one- and two-limb
    /// values against `u128`, and every product one off refused.
    #[test]
    fn an_exact_product_is_recognised() {
        for a in [1u128, 3, 0xffff_ffff_ffff_ffff, 0x1234_5678_9abc_def1] {
            for b in [1u128, 5, 0xffff_ffff_ffff_fffb, 0xfedc_ba98_7654_3211] {
                let product = a * b;
                let (x, y) = (from_u128(a), from_u128(b));
                assert!(product_equals(&x, &y, &from_u128(product)), "{a:x} * {b:x}");
                assert!(!product_equals(&x, &y, &from_u128(product + 1)));
                assert!(!product_equals(&x, &y, &from_u128(product - 1)));
            }
        }
        // A product wider than the type is held to the type's own limbs only
        // if its top half is zero: 2^127 · 2 needs a third limb.
        let half = from_u128(1u128 << 127);
        assert!(!product_equals(&half, &Uint::from_u64(2), &Uint::zero()));
    }

    /// Every kind of secret is wiped when it drops: what the hook in each
    /// `Drop` reads is a value before and zeros after. Nothing prints.
    #[test]
    fn every_secret_is_wiped_when_dropped_and_prints_as_redacted() {
        let value = from_u128(0x0123_4567_89ab_cdef_fedc_ba98_7654_3211);
        let _ = dropped::take();
        let secret = secret_of(&value);
        let copy = secret.duplicate();
        assert_eq!(copy.value(), &value);
        let modulus = SecretModulus::new(&secret).expect("odd");
        let mut bytes = SecretBytes::new(4);
        bytes.as_mut_slice().copy_from_slice(&[1, 2, 3, 4]);
        assert_eq!(bytes.as_slice(), &[1, 2, 3, 4]);
        assert_eq!(format!("{secret:?}"), "[redacted]");
        assert_eq!(format!("{modulus:?}"), "[redacted]");
        assert_eq!(format!("{bytes:?}"), "[redacted]");
        drop((secret, copy, modulus, bytes));
        let wiped = dropped::take();
        assert_eq!(wiped.len(), 4, "{wiped:?}");
        assert!(wiped.iter().all(|wiped| *wiped
            == dropped::Wiped {
                held: true,
                zero: true
            }));
    }

    /// A secret is read in place, and refused rather than truncated when it
    /// is too wide, as a public value is.
    #[test]
    fn a_secret_reads_its_bytes_in_place() {
        let secret = Secret::<2>::from_be_bytes(&[0, 0, 1, 2]).expect("fits");
        assert_eq!(secret.value(), &Uint::from_u64(0x0102));
        let mut wide = [0u8; 17];
        wide[0] = 1;
        assert!(Secret::<2>::from_be_bytes(&wide).is_none());
        let mut value = Uint::<2>::from_u64(7);
        assert!(value.read_be_bytes(&[0xff; 16]));
        assert_eq!(as_u128(&value), u128::MAX);
        assert!(value.read_be_bytes(&[]));
        assert!(
            value.is_zero(),
            "an empty string is zero, and clears what was there"
        );
    }
}
