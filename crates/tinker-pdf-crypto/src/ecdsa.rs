//! ECDSA **verification** over P-256 and P-384 (FIPS 186-4 §6.4.2).
//!
//! Verification only, for [`crate::rsa`]'s reasons and one more: signing needs
//! a per-signature random `k` whose bias leaks the private key, and this crate
//! deliberately owns no entropy source (see [`crate::EntropySource`]). A
//! verifier needs no randomness at all.
//!
//! Two curves, because those are the two a PDF signature realistically
//! carries. P-521, the Koblitz and binary curves, Ed25519 and the Brainpool
//! set are absent: each would need its own constants and its own vectors, and
//! a curve nobody has produced a signature on is a liability rather than a
//! feature. An unrecognised curve is the caller's refusal to report, not a
//! guess this module makes.
//!
//! # What is checked, and in which order
//!
//! FIPS 186-4 §6.4.2 has four preconditions, and the order matters:
//!
//! 1. `r` and `s` are integers in `[1, n-1]`. This is checked **before any
//!    curve arithmetic**, because `s = 0` has no inverse and `r = 0` makes the
//!    final comparison meaningless — a verifier that reaches the point
//!    multiplication first is a verifier doing arithmetic on values it has not
//!    yet agreed to.
//! 2. The public key's coordinates are below `p`.
//! 3. The public key satisfies `y^2 = x^3 - 3x + b`. A point off the curve
//!    lands in a different group, where the discrete logarithm may be easy;
//!    accepting one is the invalid-curve attack. Refused at construction, so
//!    an [`EcPublicKey`] that exists has been checked.
//! 4. The public key is not the point at infinity. Both supported curves have
//!    prime order and cofactor 1, so any other on-curve point has order `n`
//!    and no separate subgroup check is needed — that is a property of these
//!    two curves, not a general one.
//!
//! Then `u1 G + u2 Q` is computed, and the point at infinity is refused there
//! too (§6.4.2 step 4).
//!
//! # Jacobian coordinates, and `a = -3`
//!
//! Points are held in Jacobian projective form `(X, Y, Z)` standing for the
//! affine `(X/Z^2, Y/Z^3)`, so the whole scalar multiplication costs one field
//! inversion at the end instead of one per addition. The doubling formula is
//! the `a = -3` special case (Bernstein–Lange `dbl-2001-b`), which is why the
//! curve parameter `a` never appears as a constant below: both NIST prime
//! curves fix `a = p - 3` and the formula folds it in. Addition is
//! `add-2007-bl`, with its two degenerate cases — equal points, and points
//! that sum to infinity — handled explicitly rather than left to produce a
//! wrong answer.
//!
//! Field elements live in the Montgomery domain throughout, which is why
//! additions and subtractions can stay ordinary modular ones: the domain is
//! linear.
//!
//! # Not constant-time
//!
//! As [`crate::bignum`] says: everything here is public. The scalars `u1` and
//! `u2` are derived from the signature and the digest, both of which the
//! attacker already has.

use crate::bignum::{Modulus, Uint};
use crate::rsa::DigestAlgorithm;

/// Limbs behind a P-256 field element or scalar.
pub const P256_LIMBS: usize = 4;

/// Limbs behind a P-384 field element or scalar.
pub const P384_LIMBS: usize = 6;

/// The longest coordinate either supported curve has.
const MAX_FIELD_BYTES: usize = 48;

/// A supported curve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Curve {
    /// NIST P-256, also called secp256r1 and prime256v1.
    P256,
    /// NIST P-384, also called secp384r1.
    P384,
}

impl Curve {
    /// The byte length of one affine coordinate — and of `r`, `s` and the
    /// scalar field, both curves having a field and an order of the same size.
    #[must_use]
    pub const fn field_bytes(self) -> usize {
        match self {
            Self::P256 => 32,
            Self::P384 => 48,
        }
    }
}

/// Why a key or a signature was refused.
///
/// Typed for [`crate::rsa::RsaRefusal`]'s reasons: a caller reporting on a
/// signature has to tell "this key is not usable" from "this signature does
/// not verify", and every input here is public so naming the reason leaks
/// nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EcdsaRefusal {
    /// A coordinate was not exactly the curve's field length. Uncompressed
    /// points are fixed width (SEC 1 §2.3.3); a short one means the caller
    /// sliced the encoding wrongly, and left-padding it would invent a key.
    CoordinateLength {
        /// The curve's coordinate length in bytes.
        expected: usize,
        /// What arrived.
        found: usize,
    },
    /// A coordinate is not below the field prime.
    CoordinateOutOfRange,
    /// The public key is the point at infinity, which signs nothing.
    PointAtInfinity,
    /// The public key does not satisfy the curve equation.
    PointNotOnCurve,
    /// `r` or `s` is zero, or not below the group order. FIPS 186-4 §6.4.2
    /// step 1, checked before any curve arithmetic.
    ScalarOutOfRange,
    /// The digest was empty, so there is nothing to have signed.
    DigestEmpty,
    /// `u1 G + u2 Q` is the point at infinity. FIPS 186-4 §6.4.2 step 4.
    ResultAtInfinity,
    /// The curve arithmetic did not complete — an inversion of zero, which
    /// the preceding checks are meant to make unreachable. Reported rather
    /// than asserted, ruling 1 leaving no room for an assertion here.
    ArithmeticFailed,
    /// `v != r`. This is the ordinary "the signature is not valid" answer.
    SignatureMismatch,
}

/// An ECDSA public key: a curve and an affine point on it.
///
/// Constructing one runs the curve-membership check, so a value of this type
/// is a point on its curve and not the point at infinity.
#[derive(Clone, Copy, Debug)]
pub struct EcPublicKey {
    curve: Curve,
    x: [u8; MAX_FIELD_BYTES],
    y: [u8; MAX_FIELD_BYTES],
}

impl EcPublicKey {
    /// Reads an affine point, big-endian, exactly as SEC 1 §2.3.3's
    /// uncompressed form stores the two coordinates after its `0x04` tag.
    ///
    /// # Errors
    ///
    /// Refuses a coordinate of the wrong length, a coordinate not below `p`,
    /// the point at infinity, and any point off the curve.
    pub fn new(curve: Curve, x: &[u8], y: &[u8]) -> Result<Self, EcdsaRefusal> {
        let width = curve.field_bytes();
        if x.len() != width {
            return Err(EcdsaRefusal::CoordinateLength {
                expected: width,
                found: x.len(),
            });
        }
        if y.len() != width {
            return Err(EcdsaRefusal::CoordinateLength {
                expected: width,
                found: y.len(),
            });
        }

        let mut key = Self {
            curve,
            x: [0u8; MAX_FIELD_BYTES],
            y: [0u8; MAX_FIELD_BYTES],
        };
        if let Some(slot) = key.x.get_mut(..width) {
            slot.copy_from_slice(x);
        }
        if let Some(slot) = key.y.get_mut(..width) {
            slot.copy_from_slice(y);
        }

        match curve {
            Curve::P256 => check_membership(&p256()?, x, y),
            Curve::P384 => check_membership(&p384()?, x, y),
        }?;
        Ok(key)
    }

    /// The curve this key is on.
    #[must_use]
    pub fn curve(&self) -> Curve {
        self.curve
    }

    /// Verifies `(r, s)` over an already-computed digest.
    ///
    /// `r` and `s` are big-endian integers of any length that fits the curve —
    /// DER strips leading zeros, so they are often shorter than a coordinate.
    ///
    /// # Errors
    ///
    /// [`EcdsaRefusal::SignatureMismatch`] when the signature is simply not
    /// valid; the other variants name a malformed input.
    pub fn verify(&self, digest: &[u8], r: &[u8], s: &[u8]) -> Result<(), EcdsaRefusal> {
        let width = self.curve.field_bytes();
        let (Some(x), Some(y)) = (self.x.get(..width), self.y.get(..width)) else {
            return Err(EcdsaRefusal::ArithmeticFailed);
        };
        match self.curve {
            Curve::P256 => verify_on(&p256()?, x, y, digest, r, s),
            Curve::P384 => verify_on(&p384()?, x, y, digest, r, s),
        }
    }

    /// Digests `message` and verifies the signature over it.
    ///
    /// # Errors
    ///
    /// As [`EcPublicKey::verify`].
    pub fn verify_message(
        &self,
        algorithm: DigestAlgorithm,
        message: &[u8],
        r: &[u8],
        s: &[u8],
    ) -> Result<(), EcdsaRefusal> {
        let digest = algorithm.digest(message);
        self.verify(digest.as_bytes(), r, s)
    }
}

/// A curve's constants, with the two moduli already precomputed.
struct Parameters<const N: usize> {
    /// The field prime `p`.
    field: Modulus<N>,
    /// The group order `n`.
    order: Modulus<N>,
    /// The curve coefficient `b`, in the Montgomery domain of `field`.
    b: Uint<N>,
    /// The base point, in the Montgomery domain of `field`.
    generator: Point<N>,
}

/// P-256's constants (FIPS 186-4 D.1.2.3; `a` is `p - 3` and folded into the
/// doubling formula, so it is not stored).
///
/// `p = 2^256 - 2^224 + 2^192 + 2^96 - 1`, and the test below recomputes that
/// identity rather than trusting the digits.
fn p256() -> Result<Parameters<P256_LIMBS>, EcdsaRefusal> {
    parameters(
        "ffffffff00000001000000000000000000000000ffffffffffffffffffffffff",
        "ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551",
        "5ac635d8aa3a93e7b3ebbd55769886bc651d06b0cc53b0f63bce3c3e27d2604b",
        "6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296",
        "4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5",
    )
}

/// P-384's constants (FIPS 186-4 D.1.2.4).
///
/// `p = 2^384 - 2^128 - 2^96 + 2^32 - 1`.
fn p384() -> Result<Parameters<P384_LIMBS>, EcdsaRefusal> {
    parameters(
        "fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe\
         ffffffff0000000000000000ffffffff",
        "ffffffffffffffffffffffffffffffffffffffffffffffff\
         c7634d81f4372ddf581a0db248b0a77aecec196accc52973",
        "b3312fa7e23ee7e4988e056be3f82d19181d9c6efe814112\
         0314088f5013875ac656398d8a2ed19d2a85c8edd3ec2aef",
        "aa87ca22be8b05378eb1c71ef320ad746e1d3b628ba79b98\
         59f741e082542a385502f25dbf55296c3a545e3872760ab7",
        "3617de4a96262c6f5d9e98bf9292dc29f8f41dbd289a147c\
         e9da3113b5f0b8c00a60b1ce1d7e819d7a431d7c90ea0e5f",
    )
}

fn parameters<const N: usize>(
    p: &str,
    n: &str,
    b: &str,
    gx: &str,
    gy: &str,
) -> Result<Parameters<N>, EcdsaRefusal> {
    let field = Modulus::new(hex(p)).ok_or(EcdsaRefusal::ArithmeticFailed)?;
    let order = Modulus::new(hex(n)).ok_or(EcdsaRefusal::ArithmeticFailed)?;
    let one = field.to_montgomery(&Uint::from_u64(1));
    Ok(Parameters {
        b: field.to_montgomery(&hex(b)),
        generator: Point {
            x: field.to_montgomery(&hex(gx)),
            y: field.to_montgomery(&hex(gy)),
            z: one,
        },
        field,
        order,
    })
}

/// Reads one of the hex constants above.
///
/// Only ever called with the literals in this file. A mistyped digit would
/// produce a curve nothing verifies on, which is what the generator and vector
/// tests below are for.
fn hex<const N: usize>(text: &str) -> Uint<N> {
    let digits = text.as_bytes();
    let mut bytes = [0u8; MAX_FIELD_BYTES];
    let len = (digits.len() / 2).min(MAX_FIELD_BYTES);
    for i in 0..len {
        let high = nibble(digits.get(2 * i).copied().unwrap_or(b'0'));
        let low = nibble(digits.get(2 * i + 1).copied().unwrap_or(b'0'));
        if let Some(slot) = bytes.get_mut(i) {
            *slot = (high << 4) | low;
        }
    }
    Uint::from_be_bytes(bytes.get(..len).unwrap_or(&[])).unwrap_or_else(Uint::zero)
}

fn nibble(digit: u8) -> u8 {
    match digit {
        b'0'..=b'9' => digit - b'0',
        b'a'..=b'f' => digit - b'a' + 10,
        b'A'..=b'F' => digit - b'A' + 10,
        _ => 0,
    }
}

/// A point in Jacobian coordinates, its field elements in Montgomery form.
///
/// `z == 0` is the point at infinity, which is the only representation of it.
#[derive(Clone, Copy)]
struct Point<const N: usize> {
    x: Uint<N>,
    y: Uint<N>,
    z: Uint<N>,
}

impl<const N: usize> Point<N> {
    fn infinity() -> Self {
        Self {
            x: Uint::zero(),
            y: Uint::zero(),
            z: Uint::zero(),
        }
    }

    fn is_infinity(&self) -> bool {
        self.z.is_zero()
    }
}

/// Doubling, `a = -3` (Bernstein–Lange `dbl-2001-b`).
///
/// Doubling the point at infinity gives the point at infinity: `z = 0` makes
/// `delta` zero and `z3 = (y + 0)^2 - y^2 - 0 = 0`.
fn double<const N: usize>(f: &Modulus<N>, p: &Point<N>) -> Point<N> {
    let delta = f.mul(&p.z, &p.z);
    let gamma = f.mul(&p.y, &p.y);
    let beta = f.mul(&p.x, &gamma);

    let difference = f.sub(&p.x, &delta);
    let sum = f.add(&p.x, &delta);
    let product = f.mul(&difference, &sum);
    let alpha = f.add(&f.add(&product, &product), &product);

    let beta2 = f.add(&beta, &beta);
    let beta4 = f.add(&beta2, &beta2);
    let beta8 = f.add(&beta4, &beta4);
    let x3 = f.sub(&f.mul(&alpha, &alpha), &beta8);

    let yz = f.add(&p.y, &p.z);
    let z3 = f.sub(&f.sub(&f.mul(&yz, &yz), &gamma), &delta);

    let gamma2 = f.mul(&gamma, &gamma);
    let gamma8 = {
        let two = f.add(&gamma2, &gamma2);
        let four = f.add(&two, &two);
        f.add(&four, &four)
    };
    let y3 = f.sub(&f.mul(&alpha, &f.sub(&beta4, &x3)), &gamma8);

    Point {
        x: x3,
        y: y3,
        z: z3,
    }
}

/// Addition (Bernstein–Lange `add-2007-bl`), with both degenerate cases
/// handled: equal points fall back to doubling, and points summing to infinity
/// fall out of the formula as `z3 = 0`.
fn add<const N: usize>(f: &Modulus<N>, a: &Point<N>, b: &Point<N>) -> Point<N> {
    if a.is_infinity() {
        return *b;
    }
    if b.is_infinity() {
        return *a;
    }

    let z1z1 = f.mul(&a.z, &a.z);
    let z2z2 = f.mul(&b.z, &b.z);
    let u1 = f.mul(&a.x, &z2z2);
    let u2 = f.mul(&b.x, &z1z1);
    let s1 = f.mul(&f.mul(&a.y, &b.z), &z2z2);
    let s2 = f.mul(&f.mul(&b.y, &a.z), &z1z1);

    let h = f.sub(&u2, &u1);
    let difference = f.sub(&s2, &s1);
    let rr = f.add(&difference, &difference);

    if h.is_zero() {
        if rr.is_zero() {
            // The same point: `add-2007-bl` divides by zero here.
            return double(f, a);
        }
        // Opposite points: the sum is the point at infinity.
        return Point::infinity();
    }

    let h2 = f.add(&h, &h);
    let i = f.mul(&h2, &h2);
    let j = f.mul(&h, &i);
    let v = f.mul(&u1, &i);

    let v2 = f.add(&v, &v);
    let x3 = f.sub(&f.sub(&f.mul(&rr, &rr), &j), &v2);
    let s1j = f.mul(&s1, &j);
    let y3 = f.sub(&f.mul(&rr, &f.sub(&v, &x3)), &f.add(&s1j, &s1j));

    let z1z2 = f.add(&a.z, &b.z);
    let z3 = f.mul(&f.sub(&f.sub(&f.mul(&z1z2, &z1z2), &z1z1), &z2z2), &h);

    Point {
        x: x3,
        y: y3,
        z: z3,
    }
}

/// `u1 G + u2 Q` by Shamir's trick: one doubling per bit, shared between the
/// two scalars, and one addition from a four-entry table.
///
/// Half the doublings of two separate scalar multiplications, and the table is
/// four points rather than a window, which keeps the stack small enough that a
/// P-384 verification never allocates.
fn double_scalar_multiply<const N: usize>(
    parameters: &Parameters<N>,
    u1: &Uint<N>,
    u2: &Uint<N>,
    q: &Point<N>,
) -> Point<N> {
    let f = &parameters.field;
    let table = [
        Point::infinity(),
        parameters.generator,
        *q,
        add(f, &parameters.generator, q),
    ];

    let mut accumulator = Point::infinity();
    for i in (0..parameters.order.bits()).rev() {
        accumulator = double(f, &accumulator);
        let index = usize::from(u1.bit(i)) | (usize::from(u2.bit(i)) << 1);
        if index != 0 {
            if let Some(addend) = table.get(index) {
                accumulator = add(f, &accumulator, addend);
            }
        }
    }
    accumulator
}

/// The affine `x` of a Jacobian point, or `None` at infinity.
fn affine_x<const N: usize>(f: &Modulus<N>, p: &Point<N>) -> Option<Uint<N>> {
    if p.is_infinity() {
        return None;
    }
    let z = f.from_montgomery(&p.z);
    let inverse = f.to_montgomery(&f.inverse_prime(&z)?);
    let inverse2 = f.mul(&inverse, &inverse);
    Some(f.from_montgomery(&f.mul(&p.x, &inverse2)))
}

/// Checks a candidate public key against the curve equation and the range
/// rules. FIPS 186-4 §6.4.2's steps 2 and 3, and appendix A.4.2's partial
/// public key validation.
fn check_membership<const N: usize>(
    parameters: &Parameters<N>,
    x: &[u8],
    y: &[u8],
) -> Result<(), EcdsaRefusal> {
    let f = &parameters.field;
    let x = Uint::<N>::from_be_bytes(x).ok_or(EcdsaRefusal::CoordinateOutOfRange)?;
    let y = Uint::<N>::from_be_bytes(y).ok_or(EcdsaRefusal::CoordinateOutOfRange)?;
    if x >= *f.value() || y >= *f.value() {
        return Err(EcdsaRefusal::CoordinateOutOfRange);
    }
    if x.is_zero() && y.is_zero() {
        // The point at infinity has no affine coordinates; `(0, 0)` is the
        // encoding some callers reach for, and it is not on either curve
        // anyway. Named separately so the verdict can say so.
        return Err(EcdsaRefusal::PointAtInfinity);
    }

    let x = f.to_montgomery(&x);
    let y = f.to_montgomery(&y);
    let left = f.mul(&y, &y);
    let x3 = f.mul(&f.mul(&x, &x), &x);
    let three_x = f.add(&f.add(&x, &x), &x);
    let right = f.add(&f.sub(&x3, &three_x), &parameters.b);
    if left == right {
        Ok(())
    } else {
        Err(EcdsaRefusal::PointNotOnCurve)
    }
}

/// FIPS 186-4 §6.4: `e` is the integer of the leftmost `min(N, outlen)` bits
/// of the digest, where `N` is the order's bit length.
///
/// Both supported orders are a whole number of bytes — 256 and 384 bits — so
/// the truncation is a byte slice and no bit shift is needed. P-521, whose
/// order is 521 bits, is the NIST curve where that would stop being true, and
/// it is not supported.
fn scalar_from_digest<const N: usize>(
    order: &Modulus<N>,
    digest: &[u8],
) -> Result<Uint<N>, EcdsaRefusal> {
    if digest.is_empty() {
        return Err(EcdsaRefusal::DigestEmpty);
    }
    let take = digest.len().min(order.byte_len());
    let leading = digest.get(..take).ok_or(EcdsaRefusal::ArithmeticFailed)?;
    let value = Uint::<N>::from_be_bytes(leading).ok_or(EcdsaRefusal::ArithmeticFailed)?;
    Ok(order.reduce(&value))
}

fn verify_on<const N: usize>(
    parameters: &Parameters<N>,
    qx: &[u8],
    qy: &[u8],
    digest: &[u8],
    r: &[u8],
    s: &[u8],
) -> Result<(), EcdsaRefusal> {
    let order = &parameters.order;

    // Step 1, and it comes first on purpose: r and s in [1, n-1], before any
    // curve arithmetic touches them.
    let r = Uint::<N>::from_be_bytes(r).ok_or(EcdsaRefusal::ScalarOutOfRange)?;
    let s = Uint::<N>::from_be_bytes(s).ok_or(EcdsaRefusal::ScalarOutOfRange)?;
    if r.is_zero() || s.is_zero() || r >= *order.value() || s >= *order.value() {
        return Err(EcdsaRefusal::ScalarOutOfRange);
    }

    let e = scalar_from_digest(order, digest)?;
    let w = order
        .inverse_prime(&s)
        .ok_or(EcdsaRefusal::ArithmeticFailed)?;
    let u1 = order.mul_mod(&e, &w);
    let u2 = order.mul_mod(&r, &w);

    let f = &parameters.field;
    let qx = Uint::<N>::from_be_bytes(qx).ok_or(EcdsaRefusal::CoordinateOutOfRange)?;
    let qy = Uint::<N>::from_be_bytes(qy).ok_or(EcdsaRefusal::CoordinateOutOfRange)?;
    let q = Point {
        x: f.to_montgomery(&qx),
        y: f.to_montgomery(&qy),
        z: f.to_montgomery(&Uint::from_u64(1)),
    };

    let point = double_scalar_multiply(parameters, &u1, &u2, &q);
    let x = affine_x(f, &point).ok_or(EcdsaRefusal::ResultAtInfinity)?;
    if order.reduce(&x) == r {
        Ok(())
    } else {
        Err(EcdsaRefusal::SignatureMismatch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads a hex integer, padding an odd digit count on the left.
    ///
    /// The padding is not cosmetic: CAVP's public-key validation file writes
    /// its out-of-range coordinates with 65 digits, and dropping the odd
    /// nibble would silently turn one into an in-range 32-byte value.
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
            "SHA-1" => Some(DigestAlgorithm::Sha1),
            "SHA-256" => Some(DigestAlgorithm::Sha256),
            "SHA-384" => Some(DigestAlgorithm::Sha384),
            "SHA-512" => Some(DigestAlgorithm::Sha512),
            _ => None,
        }
    }

    /// The field primes are defined by an identity, not by their digits, so
    /// the digits are checked against the identity. A transposition anywhere
    /// in ninety-six hex characters fails here rather than three hundred lines
    /// later as an unexplained vector failure.
    #[test]
    fn the_field_primes_match_their_defining_identity() {
        // p256 = 2^256 - 2^224 + 2^192 + 2^96 - 1, built by repeated doubling
        // modulo nothing: `Uint` addition is exact until it overflows, and
        // 2^256 does not fit a four-limb value — so the identity is checked in
        // six limbs, where every term does.
        let two_to = |exponent: usize| {
            let mut value = Uint::<8>::from_u64(1);
            for _ in 0..exponent {
                value = value.add_carry(&value).0;
            }
            value
        };
        let one = Uint::<8>::from_u64(1);
        let p256_identity = two_to(256)
            .sub_borrow(&two_to(224))
            .0
            .add_carry(&two_to(192))
            .0
            .add_carry(&two_to(96))
            .0
            .sub_borrow(&one)
            .0;
        let p256_digits: Uint<8> =
            hex("ffffffff00000001000000000000000000000000ffffffffffffffffffffffff");
        assert_eq!(p256_identity, p256_digits, "P-256's p");

        let p384_identity = two_to(384)
            .sub_borrow(&two_to(128))
            .0
            .sub_borrow(&two_to(96))
            .0
            .add_carry(&two_to(32))
            .0
            .sub_borrow(&one)
            .0;
        let p384_digits: Uint<8> = hex(
            "fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe\
             ffffffff0000000000000000ffffffff",
        );
        assert_eq!(p384_identity, p384_digits, "P-384's p");
    }

    /// The generator has to be on its own curve, and the orders have to be the
    /// widths FIPS 186-4 gives. Both catch a mistyped constant without any
    /// external data.
    #[test]
    fn the_curve_constants_are_self_consistent() {
        let p256 = p256().expect("P-256 parameters");
        assert_eq!(p256.field.bits(), 256);
        assert_eq!(p256.order.bits(), 256);
        let gx = p256.field.from_montgomery(&p256.generator.x);
        let gy = p256.field.from_montgomery(&p256.generator.y);
        let mut x = [0u8; 32];
        let mut y = [0u8; 32];
        assert!(gx.to_be_bytes(&mut x) && gy.to_be_bytes(&mut y));
        assert_eq!(check_membership(&p256, &x, &y), Ok(()), "P-256's G");

        let p384 = p384().expect("P-384 parameters");
        assert_eq!(p384.field.bits(), 384);
        assert_eq!(p384.order.bits(), 384);
        let gx = p384.field.from_montgomery(&p384.generator.x);
        let gy = p384.field.from_montgomery(&p384.generator.y);
        let mut x = [0u8; 48];
        let mut y = [0u8; 48];
        assert!(gx.to_be_bytes(&mut x) && gy.to_be_bytes(&mut y));
        assert_eq!(check_membership(&p384, &x, &y), Ok(()), "P-384's G");
    }

    /// `n * G` is the point at infinity: the order really is the generator's
    /// order, which no amount of squinting at hex digits establishes.
    #[test]
    fn the_generator_has_the_order_the_constants_claim() {
        let p256 = p256().expect("P-256 parameters");
        let n = *p256.order.value();
        let infinity = double_scalar_multiply(&p256, &n, &Uint::zero(), &Point::infinity());
        assert!(infinity.is_infinity(), "n * G on P-256");

        let p384 = p384().expect("P-384 parameters");
        let n = *p384.order.value();
        let infinity = double_scalar_multiply(&p384, &n, &Uint::zero(), &Point::infinity());
        assert!(infinity.is_infinity(), "n * G on P-384");
    }

    struct Rfc6979Vector {
        algorithm: DigestAlgorithm,
        message: &'static str,
        r: &'static str,
        s: &'static str,
    }

    /// RFC 6979 appendix A.2.5: the P-256 key pair and its signatures. The
    /// RFC's subject is deterministic `k`, which this module never computes —
    /// what is used here is that it publishes a key and eight `(r, s)` pairs
    /// over two known messages, which is a verification vector whatever it was
    /// written for.
    const RFC6979_P256_UX: &str =
        "60FED4BA255A9D31C961EB74C6356D68C049B8923B61FA6CE669622E60F29FB6";
    const RFC6979_P256_UY: &str =
        "7903FE1008B8BC99A41AE9E95628BC64F2F1B20C2D7E9F5177A3C294D4462299";

    const RFC6979_P256: [Rfc6979Vector; 8] = [
        Rfc6979Vector {
            algorithm: DigestAlgorithm::Sha1,
            message: "sample",
            r: "61340C88C3AAEBEB4F6D667F672CA9759A6CCAA9FA8811313039EE4A35471D32",
            s: "6D7F147DAC089441BB2E2FE8F7A3FA264B9C475098FDCF6E00D7C996E1B8B7EB",
        },
        Rfc6979Vector {
            algorithm: DigestAlgorithm::Sha256,
            message: "sample",
            r: "EFD48B2AACB6A8FD1140DD9CD45E81D69D2C877B56AAF991C34D0EA84EAF3716",
            s: "F7CB1C942D657C41D436C7A1B6E29F65F3E900DBB9AFF4064DC4AB2F843ACDA8",
        },
        Rfc6979Vector {
            algorithm: DigestAlgorithm::Sha384,
            message: "sample",
            r: "0EAFEA039B20E9B42309FB1D89E213057CBF973DC0CFC8F129EDDDC800EF7719",
            s: "4861F0491E6998B9455193E34E7B0D284DDD7149A74B95B9261F13ABDE940954",
        },
        Rfc6979Vector {
            algorithm: DigestAlgorithm::Sha512,
            message: "sample",
            r: "8496A60B5E9B47C825488827E0495B0E3FA109EC4568FD3F8D1097678EB97F00",
            s: "2362AB1ADBE2B8ADF9CB9EDAB740EA6049C028114F2460F96554F61FAE3302FE",
        },
        Rfc6979Vector {
            algorithm: DigestAlgorithm::Sha1,
            message: "test",
            r: "0CBCC86FD6ABD1D99E703E1EC50069EE5C0B4BA4B9AC60E409E8EC5910D81A89",
            s: "01B9D7B73DFAA60D5651EC4591A0136F87653E0FD780C3B1BC872FFDEAE479B1",
        },
        Rfc6979Vector {
            algorithm: DigestAlgorithm::Sha256,
            message: "test",
            r: "F1ABB023518351CD71D881567B1EA663ED3EFCF6C5132B354F28D3B0B7D38367",
            s: "019F4113742A2B14BD25926B49C649155F267E60D3814B4C0CC84250E46F0083",
        },
        Rfc6979Vector {
            algorithm: DigestAlgorithm::Sha384,
            message: "test",
            r: "83910E8B48BB0C74244EBDF7F07A1C5413D61472BD941EF3920E623FBCCEBEB6",
            s: "8DDBEC54CF8CD5874883841D712142A56A8D0F218F5003CB0296B6B509619F2C",
        },
        Rfc6979Vector {
            algorithm: DigestAlgorithm::Sha512,
            message: "test",
            r: "461D93F31B6540894788FD206C07CFA0CC35F46FA3C91816FFF1040AD1581A04",
            s: "39AF9F15DE0DB8D97E72719C74820D304CE5226E32DEDAE67519E840D1194E55",
        },
    ];

    /// RFC 6979 appendix A.2.6: P-384, same shape.
    const RFC6979_P384_UX: &str = concat!(
        "EC3A4E415B4E19A4568618029F427FA5DA9A8BC4AE92E02E",
        "06AAE5286B300C64DEF8F0EA9055866064A254515480BC13"
    );
    const RFC6979_P384_UY: &str = concat!(
        "8015D9B72D7D57244EA8EF9AC0C621896708A59367F9DFB9",
        "F54CA84B3F1C9DB1288B231C3AE0D4FE7344FD2533264720"
    );

    const RFC6979_P384: [Rfc6979Vector; 8] = [
        Rfc6979Vector {
            algorithm: DigestAlgorithm::Sha1,
            message: "sample",
            r: concat!(
                "EC748D839243D6FBEF4FC5C4859A7DFFD7F3ABDDF7201454",
                "0C16D73309834FA37B9BA002899F6FDA3A4A9386790D4EB2"
            ),
            s: concat!(
                "A3BCFA947BEEF4732BF247AC17F71676CB31A847B9FF0CBC",
                "9C9ED4C1A5B3FACF26F49CA031D4857570CCB5CA4424A443"
            ),
        },
        Rfc6979Vector {
            algorithm: DigestAlgorithm::Sha256,
            message: "sample",
            r: concat!(
                "21B13D1E013C7FA1392D03C5F99AF8B30C570C6F98D4EA8E",
                "354B63A21D3DAA33BDE1E888E63355D92FA2B3C36D8FB2CD"
            ),
            s: concat!(
                "F3AA443FB107745BF4BD77CB3891674632068A10CA67E3D4",
                "5DB2266FA7D1FEEBEFDC63ECCD1AC42EC0CB8668A4FA0AB0"
            ),
        },
        Rfc6979Vector {
            algorithm: DigestAlgorithm::Sha384,
            message: "sample",
            r: concat!(
                "94EDBB92A5ECB8AAD4736E56C691916B3F88140666CE9FA7",
                "3D64C4EA95AD133C81A648152E44ACF96E36DD1E80FABE46"
            ),
            s: concat!(
                "99EF4AEB15F178CEA1FE40DB2603138F130E740A19624526",
                "203B6351D0A3A94FA329C145786E679E7B82C71A38628AC8"
            ),
        },
        Rfc6979Vector {
            algorithm: DigestAlgorithm::Sha512,
            message: "sample",
            r: concat!(
                "ED0959D5880AB2D869AE7F6C2915C6D60F96507F9CB3E047",
                "C0046861DA4A799CFE30F35CC900056D7C99CD7882433709"
            ),
            s: concat!(
                "512C8CCEEE3890A84058CE1E22DBC2198F42323CE8ACA913",
                "5329F03C068E5112DC7CC3EF3446DEFCEB01A45C2667FDD5"
            ),
        },
        Rfc6979Vector {
            algorithm: DigestAlgorithm::Sha1,
            message: "test",
            r: concat!(
                "4BC35D3A50EF4E30576F58CD96CE6BF638025EE624004A1F",
                "7789A8B8E43D0678ACD9D29876DAF46638645F7F404B11C7"
            ),
            s: concat!(
                "D5A6326C494ED3FF614703878961C0FDE7B2C278F9A65FD8",
                "C4B7186201A2991695BA1C84541327E966FA7B50F7382282"
            ),
        },
        Rfc6979Vector {
            algorithm: DigestAlgorithm::Sha256,
            message: "test",
            r: concat!(
                "6D6DEFAC9AB64DABAFE36C6BF510352A4CC27001263638E5",
                "B16D9BB51D451559F918EEDAF2293BE5B475CC8F0188636B"
            ),
            s: concat!(
                "2D46F3BECBCC523D5F1A1256BF0C9B024D879BA9E838144C",
                "8BA6BAEB4B53B47D51AB373F9845C0514EEFB14024787265"
            ),
        },
        Rfc6979Vector {
            algorithm: DigestAlgorithm::Sha384,
            message: "test",
            r: concat!(
                "8203B63D3C853E8D77227FB377BCF7B7B772E97892A80F36",
                "AB775D509D7A5FEB0542A7F0812998DA8F1DD3CA3CF023DB"
            ),
            s: concat!(
                "DDD0760448D42D8A43AF45AF836FCE4DE8BE06B485E9B61B",
                "827C2F13173923E06A739F040649A667BF3B828246BAA5A5"
            ),
        },
        Rfc6979Vector {
            algorithm: DigestAlgorithm::Sha512,
            message: "test",
            r: concat!(
                "A0D5D090C9980FAF3C2CE57B7AE951D31977DD11C775D314",
                "AF55F76C676447D06FB6495CD21B4B6E340FC236584FB277"
            ),
            s: concat!(
                "976984E59B4C77B0E8E4460DCA3D9F20E07B9BB1F63BEEFA",
                "F576F6B2E8B224634A2092CD3792E0159AD9CEE37659C736"
            ),
        },
    ];

    #[test]
    fn rfc_6979_appendix_a_2_vectors() {
        let mut ran = 0usize;
        for (curve, ux, uy, vectors) in [
            (Curve::P256, RFC6979_P256_UX, RFC6979_P256_UY, &RFC6979_P256),
            (Curve::P384, RFC6979_P384_UX, RFC6979_P384_UY, &RFC6979_P384),
        ] {
            let key = EcPublicKey::new(curve, &unhex(ux), &unhex(uy))
                .expect("the RFC's public key is on its curve");
            for vector in vectors {
                assert_eq!(
                    key.verify_message(
                        vector.algorithm,
                        vector.message.as_bytes(),
                        &unhex(vector.r),
                        &unhex(vector.s),
                    ),
                    Ok(()),
                    "{curve:?} {:?} over {:?}",
                    vector.algorithm,
                    vector.message
                );
                // And the same signature over the other message must not
                // verify, or the vector proves nothing.
                let other = if vector.message == "sample" {
                    "test"
                } else {
                    "sample"
                };
                assert_eq!(
                    key.verify_message(
                        vector.algorithm,
                        other.as_bytes(),
                        &unhex(vector.r),
                        &unhex(vector.s),
                    ),
                    Err(EcdsaRefusal::SignatureMismatch)
                );
                ran += 1;
            }
        }
        assert_eq!(ran, 16, "eight vectors on each of two curves");
    }

    /// NIST CAVP `SigVer.rsp` from `186-3ecdsatestvectors.zip`, P-256 and
    /// P-384. Every vector for a digest this crate implements.
    ///
    /// The failure classes are the file's own: a changed message, a changed
    /// `r`, a changed `s`, and a changed `Q`. The last is the one worth
    /// naming — some of those keys are no longer on the curve, and those are
    /// refused at construction rather than verified, which is the
    /// invalid-curve rejection this module is supposed to make. The count of
    /// those is asserted so that a regression which quietly stopped checking
    /// membership would fail here rather than pass.
    #[test]
    fn cavp_sigver_vectors() {
        const VECTORS: &str = include_str!("../tests/data/cavp/ecdsa_sigver_p256_p384.rsp");

        let mut curve = None;
        let mut algorithm = None;
        let mut message = Vec::new();
        let mut qx = Vec::new();
        let mut qy = Vec::new();
        let mut r = Vec::new();
        let mut s = Vec::new();

        let mut ran = 0usize;
        let mut skipped = 0usize;
        let mut accepted = 0usize;
        let mut rejected = 0usize;
        let mut off_curve = 0usize;

        for line in VECTORS.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(group) = line.strip_prefix('[').and_then(|g| g.strip_suffix(']')) {
                let mut parts = group.split(',');
                curve = match parts.next() {
                    Some("P-256") => Some(Curve::P256),
                    Some("P-384") => Some(Curve::P384),
                    _ => None,
                };
                algorithm = parts.next().and_then(algorithm_named);
                continue;
            }
            let Some((name, value)) = line.split_once(" = ") else {
                continue;
            };
            match name {
                "Msg" => message = unhex(value),
                "Qx" => qx = unhex(value),
                "Qy" => qy = unhex(value),
                "R" => r = unhex(value),
                "S" => s = unhex(value),
                "Result" => {
                    let (Some(curve), Some(algorithm)) = (curve, algorithm) else {
                        skipped += 1;
                        continue;
                    };
                    let expected_valid = value.starts_with('P');
                    let outcome = EcPublicKey::new(curve, &qx, &qy)
                        .and_then(|key| key.verify_message(algorithm, &message, &r, &s));
                    if matches!(
                        outcome,
                        Err(EcdsaRefusal::PointNotOnCurve | EcdsaRefusal::PointAtInfinity)
                    ) {
                        off_curve += 1;
                    }
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

        assert_eq!(ran, 120, "vectors run");
        assert_eq!(skipped, 30, "SHA-224 vectors, which this crate cannot hash");
        assert_eq!(accepted, 24, "Result = P");
        assert_eq!(rejected, 96, "Result = F");
        // Measured, and worth writing down rather than assuming: none of this
        // file's thirty "Q changed" vectors substitutes a point that is off
        // the curve. They swap in another valid public key, so they exercise
        // the arithmetic and not the membership check. The invalid-curve
        // rejection is gated by `cavp_pkv_vectors` below, which is the file
        // NIST publishes for exactly that.
        assert_eq!(off_curve, 0, "changed-Q vectors stay on the curve");
    }

    /// NIST CAVP `PKV.rsp`, P-256 and P-384: the published points that a
    /// verifier must refuse before it ever multiplies by one.
    ///
    /// Twelve per curve — four valid, four off the curve, four with a
    /// coordinate at or above `p`. The off-curve four are the invalid-curve
    /// attack in its published form.
    #[test]
    fn cavp_pkv_vectors() {
        const VECTORS: &str = include_str!("../tests/data/cavp/ecdsa_pkv_p256_p384.rsp");

        let mut curve = None;
        let mut qx = Vec::new();
        let mut qy = Vec::new();

        let mut valid = 0usize;
        let mut not_on_curve = 0usize;
        let mut out_of_range = 0usize;

        for line in VECTORS.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(group) = line.strip_prefix('[').and_then(|g| g.strip_suffix(']')) {
                curve = match group {
                    "P-256" => Some(Curve::P256),
                    "P-384" => Some(Curve::P384),
                    _ => None,
                };
                continue;
            }
            let Some((name, value)) = line.split_once(" = ") else {
                continue;
            };
            match name {
                "Qx" => qx = unhex(value),
                "Qy" => qy = unhex(value),
                "Result" => {
                    let Some(curve) = curve else { continue };
                    let outcome = EcPublicKey::new(curve, &qx, &qy);
                    if value.starts_with('P') {
                        assert!(outcome.is_ok(), "valid point refused: {outcome:?}");
                        valid += 1;
                    } else if value.contains("not on curve") {
                        assert_eq!(
                            outcome.err(),
                            Some(EcdsaRefusal::PointNotOnCurve),
                            "a point off the curve must be refused as such"
                        );
                        not_on_curve += 1;
                    } else {
                        // These coordinates are 65 hex digits: an integer
                        // above `p`, and not a well-formed coordinate for the
                        // curve either. Refused for the length, which is the
                        // earlier and stricter of the two reasons.
                        assert_eq!(
                            outcome.err(),
                            Some(EcdsaRefusal::CoordinateLength {
                                expected: curve.field_bytes(),
                                found: curve.field_bytes() + 1,
                            })
                        );
                        out_of_range += 1;
                    }
                }
                _ => {}
            }
        }

        assert_eq!(valid, 8, "valid points, four per curve");
        assert_eq!(not_on_curve, 8, "points off the curve");
        assert_eq!(out_of_range, 8, "coordinates at or above p");
    }

    /// A coordinate of the right *length* but not below `p`. CAVP's
    /// out-of-range points are all too long as well, so this is the case its
    /// file does not reach: `x = p` is thirty-two bytes and still not a
    /// coordinate.
    #[test]
    fn a_coordinate_at_the_field_prime_is_refused() {
        let p = unhex("ffffffff00000001000000000000000000000000ffffffffffffffffffffffff");
        assert_eq!(
            EcPublicKey::new(Curve::P256, &p, &unhex(RFC6979_P256_UY)).err(),
            Some(EcdsaRefusal::CoordinateOutOfRange)
        );
        assert_eq!(
            EcPublicKey::new(Curve::P256, &unhex(RFC6979_P256_UX), &p).err(),
            Some(EcdsaRefusal::CoordinateOutOfRange)
        );
    }

    #[test]
    fn a_point_off_the_curve_is_refused() {
        let x = unhex(RFC6979_P256_UX);
        let mut y = unhex(RFC6979_P256_UY);
        if let Some(last) = y.last_mut() {
            *last ^= 1;
        }
        assert_eq!(
            EcPublicKey::new(Curve::P256, &x, &y).err(),
            Some(EcdsaRefusal::PointNotOnCurve)
        );
    }

    /// A P-256 point offered as a P-384 one. It is a real point on a real
    /// curve, and it is not on this one.
    #[test]
    fn a_point_from_the_wrong_curve_is_refused() {
        let mut x = vec![0u8; 16];
        x.extend_from_slice(&unhex(RFC6979_P256_UX));
        let mut y = vec![0u8; 16];
        y.extend_from_slice(&unhex(RFC6979_P256_UY));
        assert_eq!(
            EcPublicKey::new(Curve::P384, &x, &y).err(),
            Some(EcdsaRefusal::PointNotOnCurve)
        );
        // And the same coordinates at their own width are refused for the
        // length, not silently padded.
        assert_eq!(
            EcPublicKey::new(
                Curve::P384,
                &unhex(RFC6979_P256_UX),
                &unhex(RFC6979_P256_UY)
            )
            .err(),
            Some(EcdsaRefusal::CoordinateLength {
                expected: 48,
                found: 32
            })
        );
    }

    #[test]
    fn the_point_at_infinity_is_refused() {
        assert_eq!(
            EcPublicKey::new(Curve::P256, &[0u8; 32], &[0u8; 32]).err(),
            Some(EcdsaRefusal::PointAtInfinity)
        );
    }

    /// `r` and `s` outside `[1, n-1]`, refused before any curve arithmetic.
    #[test]
    fn scalars_outside_the_group_order_are_refused() {
        let key = EcPublicKey::new(
            Curve::P256,
            &unhex(RFC6979_P256_UX),
            &unhex(RFC6979_P256_UY),
        )
        .expect("the RFC's key");
        let digest = DigestAlgorithm::Sha256.digest(b"sample");
        let valid_r = unhex("efd48b2aacb6a8fd1140dd9cd45e81d69d2c877b56aaf991c34d0ea84eaf3716");
        let valid_s = unhex("f7cb1c942d657c41d436c7a1b6e29f65f3e900dbb9aff4064dc4ab2f843acda8");
        let order = unhex("ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551");

        assert_eq!(
            key.verify(digest.as_bytes(), &valid_r, &valid_s),
            Ok(()),
            "the control"
        );
        for (r, s, what) in [
            (vec![0u8; 32], valid_s.clone(), "r = 0"),
            (valid_r.clone(), vec![0u8; 32], "s = 0"),
            (order.clone(), valid_s.clone(), "r = n"),
            (valid_r.clone(), order.clone(), "s = n"),
            (vec![0xffu8; 33], valid_s.clone(), "r wider than the field"),
            (valid_r.clone(), vec![0xffu8; 33], "s wider than the field"),
        ] {
            assert_eq!(
                key.verify(digest.as_bytes(), &r, &s),
                Err(EcdsaRefusal::ScalarOutOfRange),
                "{what}"
            );
        }
    }

    #[test]
    fn an_empty_digest_is_refused() {
        let key = EcPublicKey::new(
            Curve::P256,
            &unhex(RFC6979_P256_UX),
            &unhex(RFC6979_P256_UY),
        )
        .expect("the RFC's key");
        assert_eq!(
            key.verify(
                &[],
                &unhex("efd48b2aacb6a8fd1140dd9cd45e81d69d2c877b56aaf991c34d0ea84eaf3716"),
                &unhex("f7cb1c942d657c41d436c7a1b6e29f65f3e900dbb9aff4064dc4ab2f843acda8"),
            ),
            Err(EcdsaRefusal::DigestEmpty)
        );
    }
}
