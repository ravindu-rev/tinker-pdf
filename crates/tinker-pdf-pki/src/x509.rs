//! X.509 certificates, to the depth RFC 5280 profiles them.
//!
//! A certificate is three things at once and this module keeps all three,
//! because the later milestones of `docs/design/signatures.md` need different
//! ones. It is a **set of decoded values** — who, from when to when, with
//! which key — which is what a verdict reports. It is a **set of encodings**
//! — the issuer's DER, the subject's DER, an extension's `extnValue` — which
//! is what chain building compares, since DER gives one value one spelling. And
//! it is a **signed object**, so [`Certificate::tbs`] and
//! [`Certificate::tbs_range`] hand back the exact bytes the signature covers.
//!
//! That last one is not a convenience. The signature on a certificate is over
//! the TBSCertificate's encoding *as it arrived*, not over a re-encoding of
//! the values read out of it, and a parser that cannot name those bytes forces
//! its caller to re-serialise — at which point any disagreement about DER
//! between reader and writer becomes a valid signature that fails to verify,
//! or worse. So the range is recorded while the walker is standing on it, and
//! `&der[cert.tbs_range()] == cert.tbs()` is asserted by test.
//!
//! # Where this is stricter than the wire, and where it is not
//!
//! Refused, because each is a disagreement about structure rather than a
//! variation in content:
//!
//! - extensions on a certificate that is not v3 (§4.1.2.9);
//! - a unique identifier on a v1 certificate (§4.1.2.8);
//! - an `Extensions` sequence with no extensions in it (§4.1.2.9's
//!   `SIZE (1..MAX)`);
//! - the same extension OID twice (§4.2's "MUST NOT include more than one
//!   instance");
//! - a `version` outside 0, 1 and 2.
//!
//! Accepted, and each is a deliberate leniency rather than an oversight:
//!
//! - **a DEFAULT encoded at its default value.** X.690 §11.5 says DER omits
//!   one, so `[0] EXPLICIT INTEGER 0` for v1 and `BOOLEAN FALSE` for
//!   `basicConstraints`' `cA` are both non-DER. Issuers emit the second
//!   routinely and refusing would discard certificates that verify perfectly
//!   well everywhere else; nothing downstream can tell the two encodings
//!   apart, so nothing downstream is misled. One rule, stated once, applied in
//!   both places.
//! - **an unrecognised critical extension.** RFC 5280 §4.2 requires a
//!   *validator* to reject the certificate; this is a parser, and a parser
//!   that refuses cannot report what it refused.
//!   [`Extensions::unrecognised_critical`] hands the list to whoever is
//!   deciding.
//! - **`notBefore` after `notAfter`.** Nonsense, and not a parse error: the
//!   values are what the certificate says, and a validity window that cannot
//!   contain any instant is [`Validity::contains`] returning false for every
//!   one.

use std::ops::Range;

use tinker_pdf_crypto::sha1::sha1;

use crate::der::{BitString, Budget, Cursor, DerError, Int, Limits, Oid, Tag, Tlv};
use crate::name::{self, Name};
use crate::oid;

/// Which version of the structure the certificate claims (§4.1.2.1).
///
/// The names are the specification's, and they are one more than the encoded
/// integer: a v3 certificate encodes `2`. The offset has been somebody's bug
/// in every implementation of this that has ever existed, so the encoded value
/// never leaves this enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Version {
    V1,
    V2,
    V3,
}

impl Version {
    /// The version an encoded `version` field names.
    #[must_use]
    pub const fn from_encoded(value: u64) -> Option<Self> {
        Some(match value {
            0 => Self::V1,
            1 => Self::V2,
            2 => Self::V3,
            _ => return None,
        })
    }

    /// What the field would encode as.
    #[must_use]
    pub const fn to_encoded(self) -> u64 {
        match self {
            Self::V1 => 0,
            Self::V2 => 1,
            Self::V3 => 2,
        }
    }
}

/// An `AlgorithmIdentifier` (§4.1.1.2): an OID and whatever it defines.
///
/// The parameters are kept as an undecoded node on purpose. What they mean is
/// a function of the OID — `NULL` for the PKCS#1 signature algorithms, a named
/// curve for `id-ecPublicKey`, a whole structure for RSASSA-PSS — so decoding
/// them here would mean this module knowing every algorithm anybody ever adds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlgorithmIdentifier<'a> {
    oid: Oid<'a>,
    parameters: Option<Tlv<'a>>,
    der: &'a [u8],
}

impl<'a> AlgorithmIdentifier<'a> {
    fn parse(tlv: &Tlv<'a>, budget: &Budget) -> Result<Self, DerError> {
        tlv.require(Tag::Sequence)?;
        let mut fields = tlv.children(budget)?;
        let oid = fields.expect(Tag::Oid)?.as_oid()?;
        let parameters = if fields.is_empty() {
            None
        } else {
            Some(fields.read()?)
        };
        fields.finish()?;
        Ok(Self {
            oid,
            parameters,
            der: tlv.raw(),
        })
    }

    /// Which algorithm.
    #[must_use]
    pub const fn oid(&self) -> Oid<'a> {
        self.oid
    }

    /// The parameters, undecoded, or nothing where the field was absent.
    #[must_use]
    pub const fn parameters(&self) -> Option<Tlv<'a>> {
        self.parameters
    }

    /// Whether the parameters are the explicit `NULL` the PKCS#1 algorithms
    /// call for (RFC 4055 §2.1), rather than absent or something else.
    #[must_use]
    pub fn parameters_are_null(&self) -> bool {
        self.parameters.is_some_and(|tlv| tlv.as_null().is_ok())
    }

    /// The whole `AlgorithmIdentifier` encoding.
    ///
    /// CMS compares a signature algorithm identifier against a certificate's,
    /// and the encodings are what it compares.
    #[must_use]
    pub const fn der(&self) -> &'a [u8] {
        self.der
    }
}

/// The window a certificate claims for itself (§4.1.2.5), as Unix seconds.
///
/// Seconds rather than a date type for the reason [`Tlv::as_time`] gives: the
/// questions are comparisons, and this crate has no clock. **Both ends are the
/// certificate's own claim**; nothing here checks them against the present,
/// because the present is the caller's to supply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Validity {
    /// `notBefore`, inclusive.
    pub not_before: i64,
    /// `notAfter`, inclusive (§4.1.2.5: "the last day on which ... is valid").
    pub not_after: i64,
}

impl Validity {
    fn parse(tlv: &Tlv<'_>, budget: &Budget) -> Result<Self, DerError> {
        tlv.require(Tag::Sequence)?;
        let mut fields = tlv.children(budget)?;
        let not_before = fields.read()?.as_time()?;
        let not_after = fields.read()?.as_time()?;
        fields.finish()?;
        Ok(Self {
            not_before,
            not_after,
        })
    }

    /// Whether an instant falls inside the window, both ends included.
    #[must_use]
    pub const fn contains(&self, seconds: i64) -> bool {
        seconds >= self.not_before && seconds <= self.not_after
    }
}

/// What could not be made of a `SubjectPublicKeyInfo`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyFault {
    /// The key BIT STRING does not hold a whole number of octets.
    NotWholeOctets,
    /// `rsaEncryption`, but the bits are not `SEQUENCE { INTEGER, INTEGER }`.
    RsaShape(DerError),
    /// A negative RSA modulus or exponent. Neither can be.
    RsaNegative,
    /// `id-ecPublicKey` with no point at all.
    EcEmptyPoint,
}

/// A public key, as far as the algorithm OID lets this crate read one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicKey<'a> {
    /// `rsaEncryption` (RFC 8017 A.1.1), split into its two integers as
    /// unsigned magnitudes — the sign octet a positive DER INTEGER carries is
    /// already gone.
    Rsa {
        modulus: &'a [u8],
        exponent: &'a [u8],
    },
    /// `id-ecPublicKey` (RFC 5480 §2). The point is the BIT STRING's octets
    /// exactly as encoded — compressed or uncompressed, unexamined, because
    /// deciding whether it is on the curve is arithmetic and belongs where
    /// the curve arithmetic is.
    ///
    /// `curve` is `None` when the parameters are not a named curve. RFC 5480
    /// §2.1.1 forbids the two alternatives that produce that, and reporting it
    /// as absent lets the refusal be made by whoever tried to verify with it,
    /// which is the only place a useful message can be written.
    Ec {
        curve: Option<Oid<'a>>,
        point: &'a [u8],
    },
    /// An algorithm OID this crate does not decode. The certificate is
    /// perfectly readable and its key is not; DSA certificates land here, as
    /// do Ed25519 ones.
    Unrecognised,
}

/// A `SubjectPublicKeyInfo` (§4.1.2.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubjectPublicKeyInfo<'a> {
    algorithm: AlgorithmIdentifier<'a>,
    key: BitString<'a>,
    parsed: PublicKey<'a>,
    der: &'a [u8],
}

impl<'a> SubjectPublicKeyInfo<'a> {
    fn parse(tlv: &Tlv<'a>, budget: &Budget) -> Result<Self, X509Error> {
        tlv.require(Tag::Sequence)?;
        let mut fields = tlv.children(budget)?;
        let algorithm = AlgorithmIdentifier::parse(&fields.read()?, budget)?;
        let key = fields.expect(Tag::BitString)?.as_bit_string()?;
        fields.finish()?;

        let bytes = key
            .whole_bytes()
            .map_err(|_| X509Error::BadPublicKey(KeyFault::NotWholeOctets))?;
        let parsed = if algorithm.oid == oid::RSA_ENCRYPTION {
            parse_rsa(bytes, budget)?
        } else if algorithm.oid == oid::EC_PUBLIC_KEY {
            if bytes.is_empty() {
                return Err(X509Error::BadPublicKey(KeyFault::EcEmptyPoint));
            }
            let curve = algorithm.parameters.and_then(|tlv| tlv.as_oid().ok());
            PublicKey::Ec {
                curve,
                point: bytes,
            }
        } else {
            PublicKey::Unrecognised
        };

        Ok(Self {
            algorithm,
            key,
            parsed,
            der: tlv.raw(),
        })
    }

    /// The algorithm the key belongs to.
    #[must_use]
    pub const fn algorithm(&self) -> AlgorithmIdentifier<'a> {
        self.algorithm
    }

    /// The `subjectPublicKey` BIT STRING, undecoded.
    #[must_use]
    pub const fn key(&self) -> BitString<'a> {
        self.key
    }

    /// The key, decoded as far as the algorithm allows.
    #[must_use]
    pub const fn public_key(&self) -> PublicKey<'a> {
        self.parsed
    }

    /// The whole `SubjectPublicKeyInfo` encoding.
    #[must_use]
    pub const fn der(&self) -> &'a [u8] {
        self.der
    }
}

/// Splits an `RSAPublicKey` (RFC 8017 A.1.1) into its two integers.
fn split_rsa<'a>(bytes: &'a [u8], budget: &Budget) -> Result<(Int<'a>, Int<'a>), DerError> {
    let mut cursor = Cursor::new(bytes, budget);
    let sequence = cursor.expect(Tag::Sequence)?;
    cursor.finish()?;
    let mut fields = sequence.children(budget)?;
    let modulus = fields.expect(Tag::Integer)?.as_integer()?;
    let exponent = fields.expect(Tag::Integer)?.as_integer()?;
    fields.finish()?;
    Ok((modulus, exponent))
}

/// Splits an `RSAPublicKey` into unsigned magnitudes, or says what stopped it.
fn parse_rsa<'a>(bytes: &'a [u8], budget: &Budget) -> Result<PublicKey<'a>, X509Error> {
    let (modulus, exponent) =
        split_rsa(bytes, budget).map_err(|e| X509Error::BadPublicKey(KeyFault::RsaShape(e)))?;
    let modulus = modulus
        .magnitude()
        .map_err(|_| X509Error::BadPublicKey(KeyFault::RsaNegative))?;
    let exponent = exponent
        .magnitude()
        .map_err(|_| X509Error::BadPublicKey(KeyFault::RsaNegative))?;
    Ok(PublicKey::Rsa { modulus, exponent })
}

/// `basicConstraints` (§4.2.1.9).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BasicConstraints {
    /// Whether the subject may sign certificates.
    pub ca: bool,
    /// How many intermediates may follow this one. `None` is "unconstrained",
    /// which §4.2.1.9 gives the field no way to distinguish from absent.
    pub path_len: Option<u32>,
}

/// `keyUsage` (§4.2.1.3), as the nine named bits.
///
/// Held as a word rather than nine booleans so that "no usage asserted" —
/// which §4.2.1.3 says a conforming CA must not produce and which exists
/// anyway — is a value rather than a shape.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyUsage(u16);

macro_rules! key_usage_bits {
    ($( $(#[$meta:meta])* $name:ident = $bit:literal; )*) => {
        impl KeyUsage {
            $(
                $(#[$meta])*
                #[doc = concat!("\n\nNamed bit ", stringify!($bit), " of §4.2.1.3.")]
                #[must_use]
                pub const fn $name(self) -> bool {
                    self.0 & (1u16 << $bit) != 0
                }
            )*
        }
    };
}

key_usage_bits! {
    digital_signature = 0;
    /// `nonRepudiation` in X.509's own naming, which RFC 5280 renamed.
    content_commitment = 1;
    key_encipherment = 2;
    data_encipherment = 3;
    key_agreement = 4;
    /// What an issuer needs before anything it signed can be a link in a
    /// chain.
    key_cert_sign = 5;
    crl_sign = 6;
    encipher_only = 7;
    decipher_only = 8;
}

impl KeyUsage {
    /// The bits as a word, bit *n* of §4.2.1.3 at `1 << n`.
    #[must_use]
    pub const fn bits(self) -> u16 {
        self.0
    }

    /// Whether the extension asserts nothing at all.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// `extKeyUsage` (§4.2.1.12).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExtendedKeyUsage<'a> {
    purposes: Vec<Oid<'a>>,
}

impl<'a> ExtendedKeyUsage<'a> {
    /// The purposes, in encoded order.
    #[must_use]
    pub fn purposes(&self) -> &[Oid<'a>] {
        &self.purposes
    }

    /// Whether a purpose is listed literally.
    ///
    /// Literally, and not "or `anyExtendedKeyUsage` is present" — §4.2.1.12
    /// leaves what `anyExtendedKeyUsage` means to the application, and folding
    /// it in here would make a certificate that asserts nothing specific look
    /// like one that asserts everything. [`ExtendedKeyUsage::allows_any`] is
    /// the other half of the question, asked separately.
    #[must_use]
    pub fn has(&self, purpose: Oid<'_>) -> bool {
        self.purposes
            .iter()
            .any(|listed| listed.as_bytes() == purpose.as_bytes())
    }

    /// Whether `anyExtendedKeyUsage` is among the purposes.
    #[must_use]
    pub fn allows_any(&self) -> bool {
        self.has(oid::KP_ANY)
    }
}

/// `authorityKeyIdentifier` (§4.2.1.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AuthorityKeyIdentifier<'a> {
    key_identifier: Option<&'a [u8]>,
    issuer: Option<&'a [u8]>,
    serial: Option<Int<'a>>,
}

impl<'a> AuthorityKeyIdentifier<'a> {
    /// The issuer's key identifier, which is what a chain builder matches
    /// against a candidate's `subjectKeyIdentifier`.
    #[must_use]
    pub const fn key_identifier(&self) -> Option<&'a [u8]> {
        self.key_identifier
    }

    /// `authorityCertIssuer`, as the raw `GeneralNames` encoding.
    ///
    /// Undecoded: a `GeneralName` is a nine-way choice and nothing in this
    /// milestone reads one. The bytes are here so a later one need not
    /// re-walk the certificate to find them.
    #[must_use]
    pub const fn issuer_der(&self) -> Option<&'a [u8]> {
        self.issuer
    }

    /// `authorityCertSerialNumber`.
    #[must_use]
    pub const fn serial(&self) -> Option<Int<'a>> {
        self.serial
    }
}

/// One extension, decoded or not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Extension<'a> {
    oid: Oid<'a>,
    critical: bool,
    value: &'a [u8],
    der: &'a [u8],
}

impl<'a> Extension<'a> {
    /// Which extension.
    #[must_use]
    pub const fn oid(&self) -> Oid<'a> {
        self.oid
    }

    /// Whether a reader that does not understand it must reject the
    /// certificate (§4.2).
    #[must_use]
    pub const fn is_critical(&self) -> bool {
        self.critical
    }

    /// The `extnValue` OCTET STRING's content: the DER of whatever the
    /// extension's own syntax is.
    #[must_use]
    pub const fn value(&self) -> &'a [u8] {
        self.value
    }

    /// The whole `Extension` encoding.
    #[must_use]
    pub const fn der(&self) -> &'a [u8] {
        self.der
    }
}

/// The extensions, kept whole and with five of them decoded.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Extensions<'a> {
    all: Vec<Extension<'a>>,
    basic_constraints: Option<BasicConstraints>,
    key_usage: Option<KeyUsage>,
    extended_key_usage: Option<ExtendedKeyUsage<'a>>,
    subject_key_identifier: Option<&'a [u8]>,
    authority_key_identifier: Option<AuthorityKeyIdentifier<'a>>,
}

/// The five this crate decodes, which is also what
/// [`Extensions::unrecognised_critical`] means by "recognised".
const DECODED: &[Oid<'static>] = &[
    oid::CE_BASIC_CONSTRAINTS,
    oid::CE_KEY_USAGE,
    oid::CE_EXT_KEY_USAGE,
    oid::CE_SUBJECT_KEY_IDENTIFIER,
    oid::CE_AUTHORITY_KEY_IDENTIFIER,
];

impl<'a> Extensions<'a> {
    /// Every extension, in encoded order.
    #[must_use]
    pub fn all(&self) -> &[Extension<'a>] {
        &self.all
    }

    /// The extension with this OID, decoded or not.
    #[must_use]
    pub fn find(&self, wanted: Oid<'_>) -> Option<&Extension<'a>> {
        self.all
            .iter()
            .find(|extension| extension.oid.as_bytes() == wanted.as_bytes())
    }

    /// `basicConstraints`, where present.
    #[must_use]
    pub const fn basic_constraints(&self) -> Option<BasicConstraints> {
        self.basic_constraints
    }

    /// `keyUsage`, where present.
    #[must_use]
    pub const fn key_usage(&self) -> Option<KeyUsage> {
        self.key_usage
    }

    /// `extKeyUsage`, where present.
    #[must_use]
    pub const fn extended_key_usage(&self) -> Option<&ExtendedKeyUsage<'a>> {
        self.extended_key_usage.as_ref()
    }

    /// `subjectKeyIdentifier`, where present.
    #[must_use]
    pub const fn subject_key_identifier(&self) -> Option<&'a [u8]> {
        self.subject_key_identifier
    }

    /// `authorityKeyIdentifier`, where present.
    #[must_use]
    pub const fn authority_key_identifier(&self) -> Option<AuthorityKeyIdentifier<'a>> {
        self.authority_key_identifier
    }

    /// The critical extensions this crate does not decode.
    ///
    /// RFC 5280 §4.2 makes an unrecognised critical extension a reason to
    /// reject the certificate. That is a validator's decision and this is a
    /// parser, so the list is handed over rather than acted on — and
    /// "recognised" is defined here as *decoded*, so an extension whose bytes
    /// are merely carried through does not count as understood.
    pub fn unrecognised_critical<'s>(&'s self) -> impl Iterator<Item = &'s Extension<'a>> + 's {
        self.all.iter().filter(|extension| {
            extension.critical
                && !DECODED
                    .iter()
                    .any(|known| known.as_bytes() == extension.oid.as_bytes())
        })
    }

    fn parse(tlv: &Tlv<'a>, budget: &Budget) -> Result<Self, X509Error> {
        tlv.require(Tag::Sequence)?;
        let mut out = Self::default();
        let mut sequence = tlv.children(budget)?;
        while !sequence.is_empty() {
            let node = sequence.expect(Tag::Sequence)?;
            let mut fields = node.children(budget)?;
            let extension_oid = fields.expect(Tag::Oid)?.as_oid()?;
            // DEFAULT FALSE, and see this module's header for why an encoded
            // `FALSE` is taken rather than refused.
            let critical = match fields.expect_optional(Tag::Boolean)? {
                Some(flag) => flag.as_bool()?,
                None => false,
            };
            let value = fields.expect(Tag::OctetString)?.as_octet_string()?;
            fields.finish()?;

            if out
                .all
                .iter()
                .any(|seen| seen.oid.as_bytes() == extension_oid.as_bytes())
            {
                return Err(X509Error::bad_extension(
                    extension_oid,
                    ExtensionFault::Duplicate,
                ));
            }
            out.all.push(Extension {
                oid: extension_oid,
                critical,
                value,
                der: node.raw(),
            });
            out.decode(extension_oid, value, budget)?;
        }
        if out.all.is_empty() {
            return Err(X509Error::EmptyExtensions);
        }
        Ok(out)
    }

    /// Decodes the five this crate understands, and ignores the rest.
    fn decode(
        &mut self,
        extension_oid: Oid<'a>,
        value: &'a [u8],
        budget: &Budget,
    ) -> Result<(), X509Error> {
        let named = |fault: ExtensionFault| X509Error::bad_extension(extension_oid, fault);
        let der = |error: DerError| named(ExtensionFault::Der(error));

        if extension_oid == oid::CE_BASIC_CONSTRAINTS {
            self.basic_constraints = Some(decode_basic_constraints(value, budget).map_err(der)?);
        } else if extension_oid == oid::CE_KEY_USAGE {
            self.key_usage = Some(decode_key_usage(value, budget).map_err(der)?);
        } else if extension_oid == oid::CE_EXT_KEY_USAGE {
            let purposes = decode_extended_key_usage(value, budget).map_err(der)?;
            if purposes.is_empty() {
                // `SIZE (1..MAX)`: an extension that permits nothing at all,
                // which is not the same as one that is absent.
                return Err(named(ExtensionFault::Empty));
            }
            self.extended_key_usage = Some(ExtendedKeyUsage { purposes });
        } else if extension_oid == oid::CE_SUBJECT_KEY_IDENTIFIER {
            self.subject_key_identifier = Some(decode_key_identifier(value, budget).map_err(der)?);
        } else if extension_oid == oid::CE_AUTHORITY_KEY_IDENTIFIER {
            self.authority_key_identifier = Some(decode_authority(value, budget).map_err(der)?);
        }
        Ok(())
    }
}

/// Reads an extension's `extnValue` as exactly one node and nothing else.
fn only<'a>(value: &'a [u8], budget: &Budget, tag: Tag) -> Result<Tlv<'a>, DerError> {
    let mut cursor = Cursor::new(value, budget);
    let tlv = cursor.expect(tag)?;
    cursor.finish()?;
    Ok(tlv)
}

/// `BasicConstraints ::= SEQUENCE { cA BOOLEAN DEFAULT FALSE,
/// pathLenConstraint INTEGER (0..MAX) OPTIONAL }` (§4.2.1.9).
fn decode_basic_constraints(value: &[u8], budget: &Budget) -> Result<BasicConstraints, DerError> {
    let sequence = only(value, budget, Tag::Sequence)?;
    let mut fields = sequence.children(budget)?;
    let mut out = BasicConstraints::default();
    if let Some(flag) = fields.expect_optional(Tag::Boolean)? {
        out.ca = flag.as_bool()?;
    }
    if let Some(limit) = fields.expect_optional(Tag::Integer)? {
        let limit = limit.as_integer()?.as_u64()?;
        // Refused rather than clamped: a path length past `u32` is not a
        // number an issuer meant, and clamping it would silently widen what
        // the certificate permits.
        out.path_len = Some(u32::try_from(limit).map_err(|_| DerError::IntegerTooLarge)?);
    }
    fields.finish()?;
    Ok(out)
}

/// `KeyUsage ::= BIT STRING { ... }` (§4.2.1.3), read as its nine named bits.
fn decode_key_usage(value: &[u8], budget: &Budget) -> Result<KeyUsage, DerError> {
    let bits = only(value, budget, Tag::BitString)?.as_bit_string()?;
    let mut usage = 0u16;
    for index in 0..9usize {
        if bits.bit(index) {
            usage |= 1u16 << index;
        }
    }
    Ok(KeyUsage(usage))
}

/// `ExtKeyUsageSyntax ::= SEQUENCE SIZE (1..MAX) OF KeyPurposeId` (§4.2.1.12).
fn decode_extended_key_usage<'a>(
    value: &'a [u8],
    budget: &Budget,
) -> Result<Vec<Oid<'a>>, DerError> {
    let sequence = only(value, budget, Tag::Sequence)?;
    let mut fields = sequence.children(budget)?;
    let mut purposes = Vec::new();
    while !fields.is_empty() {
        purposes.push(fields.expect(Tag::Oid)?.as_oid()?);
    }
    Ok(purposes)
}

/// `SubjectKeyIdentifier ::= KeyIdentifier ::= OCTET STRING` (§4.2.1.2).
fn decode_key_identifier<'a>(value: &'a [u8], budget: &Budget) -> Result<&'a [u8], DerError> {
    only(value, budget, Tag::OctetString)?.as_octet_string()
}

/// `AuthorityKeyIdentifier` (§4.2.1.1), whose three fields are IMPLICIT — each
/// wears a context tag over the content of its real type, so the tag is
/// replaced rather than wrapped.
fn decode_authority<'a>(
    value: &'a [u8],
    budget: &Budget,
) -> Result<AuthorityKeyIdentifier<'a>, DerError> {
    let sequence = only(value, budget, Tag::Sequence)?;
    let mut fields = sequence.children(budget)?;
    let mut out = AuthorityKeyIdentifier::default();
    if let Some(tagged) = fields.context_optional(0)? {
        out.key_identifier = Some(tagged.implicit(Tag::OctetString).as_octet_string()?);
    }
    if let Some(tagged) = fields.context_optional(1)? {
        out.issuer = Some(tagged.value());
    }
    if let Some(tagged) = fields.context_optional(2)? {
        out.serial = Some(tagged.implicit(Tag::Integer).as_integer()?);
    }
    fields.finish()?;
    Ok(out)
}

/// What was wrong with one extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtensionFault {
    /// A second instance of an OID already present (§4.2).
    Duplicate,
    /// A `SIZE (1..MAX)` list with nothing in it.
    Empty,
    /// The `extnValue` did not hold the extension's own syntax.
    Der(DerError),
}

/// Everything this module refuses a certificate for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum X509Error {
    /// The encoding itself.
    Der(DerError),
    /// Bytes after the `Certificate` SEQUENCE. A certificate is exactly one
    /// value, so trailing bytes mean the caller passed the wrong slice — or
    /// that something appended to a signed structure.
    TrailingBytes,
    /// A `version` outside 0, 1 and 2 (§4.1.2.1).
    UnsupportedVersion(u64),
    /// Extensions on a certificate that does not claim v3 (§4.1.2.9).
    ExtensionsRequireV3(Version),
    /// A unique identifier on a v1 certificate (§4.1.2.8).
    UniqueIdRequiresV2,
    /// An `Extensions` sequence with no extensions in it (§4.1.2.9).
    EmptyExtensions,
    /// The `SubjectPublicKeyInfo`.
    BadPublicKey(KeyFault),
    /// One extension, named by its dotted OID so the message says which.
    BadExtension { oid: String, fault: ExtensionFault },
}

impl X509Error {
    fn bad_extension(oid: Oid<'_>, fault: ExtensionFault) -> Self {
        Self::BadExtension {
            oid: oid.to_dotted(),
            fault,
        }
    }
}

impl From<DerError> for X509Error {
    fn from(error: DerError) -> Self {
        Self::Der(error)
    }
}

impl std::fmt::Display for X509Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Der(error) => write!(f, "{error}"),
            Self::TrailingBytes => write!(f, "bytes after the certificate"),
            Self::UnsupportedVersion(value) => write!(f, "certificate version {value}"),
            Self::ExtensionsRequireV3(version) => {
                write!(f, "extensions on a {version:?} certificate")
            }
            Self::UniqueIdRequiresV2 => write!(f, "a unique identifier on a v1 certificate"),
            Self::EmptyExtensions => write!(f, "an extensions field with no extensions"),
            Self::BadPublicKey(fault) => write!(f, "the public key: {fault:?}"),
            Self::BadExtension { oid, fault } => write!(f, "extension {oid}: {fault:?}"),
        }
    }
}

impl std::error::Error for X509Error {}

/// A parsed certificate.
///
/// Every borrowed slice points into the buffer handed to [`Certificate::parse`],
/// so the certificate is a view over those bytes rather than a copy of them —
/// which is what makes [`Certificate::tbs`] the *original* encoding rather
/// than a re-serialisation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Certificate<'a> {
    raw: &'a [u8],
    tbs: &'a [u8],
    tbs_range: Range<usize>,
    version: Version,
    serial: Int<'a>,
    inner_signature_algorithm: AlgorithmIdentifier<'a>,
    signature_algorithm: AlgorithmIdentifier<'a>,
    issuer: Name<'a>,
    subject: Name<'a>,
    validity: Validity,
    spki: SubjectPublicKeyInfo<'a>,
    issuer_unique_id: Option<BitString<'a>>,
    subject_unique_id: Option<BitString<'a>>,
    extensions: Extensions<'a>,
    signature: BitString<'a>,
}

impl<'a> Certificate<'a> {
    /// Reads a certificate under [`Limits::CERTIFICATE`].
    pub fn parse(der: &'a [u8]) -> Result<Self, X509Error> {
        Self::parse_with(der, Limits::CERTIFICATE)
    }

    /// Reads a certificate under ceilings of the caller's choosing.
    pub fn parse_with(der: &'a [u8], limits: Limits) -> Result<Self, X509Error> {
        let budget = Budget::new(limits);
        let mut outer = Cursor::new(der, &budget);
        let certificate = outer.expect(Tag::Sequence)?;
        if !outer.is_empty() {
            return Err(X509Error::TrailingBytes);
        }

        let mut top = certificate.children(&budget)?;
        let tbs_tlv = top.expect(Tag::Sequence)?;
        let signature_algorithm = AlgorithmIdentifier::parse(&top.read()?, &budget)?;
        let signature = top.expect(Tag::BitString)?.as_bit_string()?;
        top.finish()?;

        let mut tbs = tbs_tlv.children(&budget)?;
        let version = match tbs.context_optional(0)? {
            Some(tagged) => {
                let value = tagged.explicit(&budget)?.as_integer()?.as_u64()?;
                Version::from_encoded(value).ok_or(X509Error::UnsupportedVersion(value))?
            }
            None => Version::V1,
        };
        let serial = tbs.expect(Tag::Integer)?.as_integer()?;
        let inner_signature_algorithm = AlgorithmIdentifier::parse(&tbs.read()?, &budget)?;
        let issuer = name::read(&mut tbs, &budget)?;
        let validity = Validity::parse(&tbs.expect(Tag::Sequence)?, &budget)?;
        let subject = name::read(&mut tbs, &budget)?;
        let spki = SubjectPublicKeyInfo::parse(&tbs.expect(Tag::Sequence)?, &budget)?;

        let issuer_unique_id = match tbs.context_optional(1)? {
            Some(tagged) => Some(tagged.implicit(Tag::BitString).as_bit_string()?),
            None => None,
        };
        let subject_unique_id = match tbs.context_optional(2)? {
            Some(tagged) => Some(tagged.implicit(Tag::BitString).as_bit_string()?),
            None => None,
        };
        if version == Version::V1 && (issuer_unique_id.is_some() || subject_unique_id.is_some()) {
            return Err(X509Error::UniqueIdRequiresV2);
        }

        let extensions = match tbs.context_optional(3)? {
            Some(tagged) => {
                if version != Version::V3 {
                    return Err(X509Error::ExtensionsRequireV3(version));
                }
                Extensions::parse(&tagged.explicit(&budget)?, &budget)?
            }
            None => Extensions::default(),
        };
        tbs.finish()?;

        Ok(Self {
            raw: certificate.raw(),
            tbs: tbs_tlv.raw(),
            tbs_range: tbs_tlv.range(),
            version,
            serial,
            inner_signature_algorithm,
            signature_algorithm,
            issuer,
            subject,
            validity,
            spki,
            issuer_unique_id,
            subject_unique_id,
            extensions,
            signature,
        })
    }

    /// The whole `Certificate` encoding.
    #[must_use]
    pub const fn der(&self) -> &'a [u8] {
        self.raw
    }

    /// **The exact bytes the signature is over.**
    ///
    /// The `TBSCertificate`'s complete encoding, tag and length included, as
    /// it arrived. Verification digests these and nothing else; see this
    /// module's header for why it is these rather than a re-encoding.
    #[must_use]
    pub const fn tbs(&self) -> &'a [u8] {
        self.tbs
    }

    /// Where [`Certificate::tbs`] sits in the buffer that was parsed.
    #[must_use]
    pub fn tbs_range(&self) -> Range<usize> {
        self.tbs_range.clone()
    }

    /// The version claimed (§4.1.2.1).
    #[must_use]
    pub const fn version(&self) -> Version {
        self.version
    }

    /// The serial number, as encoded.
    ///
    /// Not widened to an integer: §4.1.2.2 allows twenty octets and issuers
    /// exceed that, and the value's job is to be compared and quoted rather
    /// than counted with.
    #[must_use]
    pub const fn serial(&self) -> Int<'a> {
        self.serial
    }

    /// The algorithm the *outer* `signatureAlgorithm` field names.
    #[must_use]
    pub const fn signature_algorithm(&self) -> AlgorithmIdentifier<'a> {
        self.signature_algorithm
    }

    /// The algorithm the `TBSCertificate`'s own `signature` field names.
    ///
    /// §4.1.2.3 requires it to equal the outer one, and
    /// [`Certificate::signature_algorithms_agree`] is that check — kept
    /// separate because a certificate where they differ is readable, and a
    /// reader that refuses cannot say what it refused.
    #[must_use]
    pub const fn inner_signature_algorithm(&self) -> AlgorithmIdentifier<'a> {
        self.inner_signature_algorithm
    }

    /// Whether §4.1.2.3's requirement holds, compared as encodings.
    #[must_use]
    pub fn signature_algorithms_agree(&self) -> bool {
        self.inner_signature_algorithm.der == self.signature_algorithm.der
    }

    /// The signature itself.
    #[must_use]
    pub const fn signature(&self) -> BitString<'a> {
        self.signature
    }

    /// Who issued this.
    #[must_use]
    pub const fn issuer(&self) -> &Name<'a> {
        &self.issuer
    }

    /// Who it is about.
    #[must_use]
    pub const fn subject(&self) -> &Name<'a> {
        &self.subject
    }

    /// The claimed validity window.
    #[must_use]
    pub const fn validity(&self) -> Validity {
        self.validity
    }

    /// The subject's public key.
    #[must_use]
    pub const fn subject_public_key_info(&self) -> SubjectPublicKeyInfo<'a> {
        self.spki
    }

    /// `issuerUniqueID` (§4.1.2.8), which nothing should be emitting.
    #[must_use]
    pub const fn issuer_unique_id(&self) -> Option<BitString<'a>> {
        self.issuer_unique_id
    }

    /// `subjectUniqueID` (§4.1.2.8).
    #[must_use]
    pub const fn subject_unique_id(&self) -> Option<BitString<'a>> {
        self.subject_unique_id
    }

    /// The extensions.
    #[must_use]
    pub const fn extensions(&self) -> &Extensions<'a> {
        &self.extensions
    }

    /// Whether the issuer and the subject are the same name.
    ///
    /// Self-*issued*, which is a statement about names and nothing more. A
    /// self-*signed* certificate is one where the signature also verifies
    /// under its own key, and that needs arithmetic this crate does not yet
    /// have (milestones 4 and 5).
    #[must_use]
    pub fn is_self_issued(&self) -> bool {
        self.issuer.matches(&self.subject)
    }

    /// The key identifier §4.2.1.2's method (1) defines: the SHA-1 of the
    /// `subjectPublicKey` BIT STRING's value, excluding the tag, the length
    /// and the unused-bits octet.
    ///
    /// Computed rather than read, and the two are different questions. A
    /// certificate's `subjectKeyIdentifier` extension is whatever its issuer
    /// chose to put there — §4.2.1.2 names two methods and permits others —
    /// so a chain builder that has an `authorityKeyIdentifier` to match and a
    /// candidate with no extension of its own needs the number the common
    /// method would have produced. This is that number, and it says nothing
    /// about what the certificate claims.
    ///
    /// SHA-1 is what §4.2.1.2 specifies. It is an identifier rather than a
    /// commitment: a chain link found this way is still checked by verifying
    /// the signature, so a collision here costs a lookup and not a trust
    /// decision.
    #[must_use]
    pub fn key_identifier_sha1(&self) -> [u8; 20] {
        sha1(self.spki.key.bytes())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::der::tests::unhex;
    use crate::name::AttributeText;

    /// Joins annotated hex lines into bytes.
    fn assemble(lines: &[&str]) -> Vec<u8> {
        lines.iter().flat_map(|line| unhex(line)).collect()
    }

    /// RFC 5280 Appendix C.1: the 578-byte self-signed RSA CA certificate.
    ///
    /// # Provenance, because this is third-party data (ruling 13)
    ///
    /// RFC 5280's appendix publishes this certificate as an *annotated* dump
    /// — one line per field, carrying that field's offset, its length, its
    /// type and its content — rather than as a plain hex block. What is below
    /// is that dump written back out as bytes, with the appendix's own offset
    /// in the comment on every line.
    ///
    /// The transcription checks itself, which is why it is admissible as
    /// evidence rather than as a fixture somebody typed. Every line's stated
    /// offset must equal the sum of the lengths of all the lines above it, and
    /// every container's declared length must equal the bytes between its
    /// header and the next field at its own level — 578 bytes in total, which
    /// is the size the appendix's prose states. Any single mistyped octet
    /// moves an offset or breaks a length, and
    /// [`the_appendix_c1_transcription_is_self_consistent`] asserts both.
    pub(crate) fn rfc5280_c1() -> Vec<u8> {
        assemble(C1)
    }

    /// RFC 5280 Appendix C.2: the 629-byte end-entity certificate the one
    /// above issued. Same provenance, same self-check.
    pub(crate) fn rfc5280_c2() -> Vec<u8> {
        assemble(C2)
    }

    /// One line per field of the appendix's dump, the appendix's own offset
    /// in each comment. See [`rfc5280_c1`] for what checks the transcription.
    const C1: &[&str] = &[
        "30 82 02 3e",                         // 0: Certificate SEQUENCE, 574 content bytes
        "30 82 01 a7",                         // 4: TBSCertificate SEQUENCE, 423 content bytes
        "a0 03",                               // 8: [0] EXPLICIT version
        "02 01 02",                            // 10: INTEGER 2 (v3)
        "02 01 11",                            // 13: INTEGER 17 (serialNumber)
        "30 0d",                               // 16: AlgorithmIdentifier SEQUENCE
        "06 09 2a 86 48 86 f7 0d 01 01 05",    // 18: OID sha1WithRSAEncryption
        "05 00",                               // 29: NULL parameters
        "30 43",                               // 31: Name SEQUENCE, 67 bytes
        "31 13",                               // 33: RelativeDistinguishedName SET
        "30 11",                               // 35: AttributeTypeAndValue SEQUENCE
        "06 0a 09 92 26 89 93 f2 2c 64 01 19", // 37: OID domainComponent 0.9.2342.19200300.100.1.25
        "16 03 63 6f 6d",                      // 49: IA5String 'com'
        "31 17",                               // 54: RelativeDistinguishedName SET
        "30 15",                               // 56: AttributeTypeAndValue SEQUENCE
        "06 0a 09 92 26 89 93 f2 2c 64 01 19", // 58: OID domainComponent
        "16 07 65 78 61 6d 70 6c 65",          // 70: IA5String 'example'
        "31 13",                               // 79: RelativeDistinguishedName SET
        "30 11",                               // 81: AttributeTypeAndValue SEQUENCE
        "06 03 55 04 03",                      // 83: OID commonName 2.5.4.3
        "13 0a 45 78 61 6d 70 6c 65 20 43 41", // 88: PrintableString 'Example CA'
        "30 1e",                               // 100: Validity SEQUENCE
        "17 0d 30 34 30 34 33 30 31 34 32 35 33 34 5a", // 102: UTCTime 2004-04-30 14:25:34Z
        "17 0d 30 35 30 34 33 30 31 34 32 35 33 34 5a", // 117: UTCTime 2005-04-30 14:25:34Z
        "30 43",                               // 132: Name SEQUENCE, 67 bytes
        "31 13",                               // 134: RelativeDistinguishedName SET
        "30 11",                               // 136: AttributeTypeAndValue SEQUENCE
        "06 0a 09 92 26 89 93 f2 2c 64 01 19", // 138: OID domainComponent 0.9.2342.19200300.100.1.25
        "16 03 63 6f 6d",                      // 150: IA5String 'com'
        "31 17",                               // 155: RelativeDistinguishedName SET
        "30 15",                               // 157: AttributeTypeAndValue SEQUENCE
        "06 0a 09 92 26 89 93 f2 2c 64 01 19", // 159: OID domainComponent
        "16 07 65 78 61 6d 70 6c 65",          // 171: IA5String 'example'
        "31 13",                               // 180: RelativeDistinguishedName SET
        "30 11",                               // 182: AttributeTypeAndValue SEQUENCE
        "06 03 55 04 03",                      // 184: OID commonName 2.5.4.3
        "13 0a 45 78 61 6d 70 6c 65 20 43 41", // 189: PrintableString 'Example CA'
        "30 81 9f",                            // 201: SubjectPublicKeyInfo SEQUENCE
        "30 0d",                               // 204: AlgorithmIdentifier SEQUENCE
        "06 09 2a 86 48 86 f7 0d 01 01 01",    // 206: OID rsaEncryption
        "05 00",                               // 217: NULL parameters
        "03 81 8d 00",                         // 219: BIT STRING, 0 unused bits
        "30 81 89",                            // 223: RSAPublicKey SEQUENCE
        "02 81 81",                            // 226: INTEGER modulus, 129 bytes
        "00 c2 d7 97 6d 28 70 aa 5b cf 23 2e 80 70 39 ee", // 229: the 1024-bit modulus
        "db 6f d5 2d d5 6a 4f 7a 34 2d f9 22 72 47 70 1d", // 245
        "ef 80 e9 ca 30 8c 00 c4 9a 6e 5b 45 b4 6e a5 e6", // 261
        "6c 94 0d fa 91 e9 40 fc 25 9d c7 b7 68 19 56 8f", // 277
        "11 70 6a d7 f1 c9 11 4f 3a 7e 3f 99 8d 6e 76 a5", // 293
        "74 5f 5e a4 55 53 e5 c7 68 36 53 c7 1d 3b 12 a6", // 309
        "85 fe bd 6e a1 ca df 35 50 ac 08 d7 b9 b4 7e 5c", // 325
        "fe e2 a3 2c d1 23 84 aa 98 c0 9b 66 18 9a 68 47", // 341
        "e9",                                  // 357
        "02 03 01 00 01",                      // 358: INTEGER 65537 (publicExponent)
        "a3 42",                               // 363: [3] EXPLICIT extensions
        "30 40",                               // 365: Extensions SEQUENCE
        "30 1d",                               // 367: Extension SEQUENCE (subjectKeyIdentifier)
        "06 03 55 1d 0e",                      // 369: OID subjectKeyIdentifier 2.5.29.14
        "04 16",                               // 374: OCTET STRING wrapper
        "04 14 08 68 af 85 33 c8 39 4a 7a f8 82 93 8e 70", // 376: OCTET STRING, the 20-byte key identifier
        "6a 4a 20 84 2c 32",                               // 392
        "30 0e",                                           // 398: Extension SEQUENCE (keyUsage)
        "06 03 55 1d 0f",                                  // 400: OID keyUsage 2.5.29.15
        "01 01 ff",                                        // 405: BOOLEAN TRUE (critical)
        "04 04",                                           // 408: OCTET STRING wrapper
        "03 02 01 06",    // 410: BIT STRING '0000011'B: keyCertSign, cRLSign
        "30 0f",          // 414: Extension SEQUENCE (basicConstraints)
        "06 03 55 1d 13", // 416: OID basicConstraints 2.5.29.19
        "01 01 ff",       // 421: BOOLEAN TRUE (critical)
        "04 05",          // 424: OCTET STRING wrapper
        "30 03 01 01 ff", // 426: SEQUENCE { BOOLEAN TRUE }: cA
        "30 0d",          // 431: AlgorithmIdentifier SEQUENCE (outer)
        "06 09 2a 86 48 86 f7 0d 01 01 05", // 433: OID sha1WithRSAEncryption
        "05 00",          // 444: NULL parameters
        "03 81 81 00",    // 446: BIT STRING signatureValue, 0 unused bits
        "6c f8 02 74 a6 61 e2 64 04 a6 54 0c 6c 72 13 ad", // 450: the 1024-bit signature
        "3c 47 fb f6 65 13 a9 85 90 33 ea 76 a3 26 d9 fc", // 466
        "d1 0e 15 5f 28 b7 ef 93 bf 3c f3 e2 3e 7c b9 52", // 482
        "fc 16 6e 29 aa e1 f4 7a 6f d5 7f ef b3 95 ca f3", // 498
        "66 88 83 4e a1 35 45 84 cb bc 9b b8 c8 ad c5 5e", // 514
        "46 d9 0b 0e 8d 80 e1 33 2b dc be 2b 92 7e 4a 43", // 530
        "a9 6a ef 8a 63 61 b3 6e 47 38 be e8 0d a3 67 5d", // 546
        "f3 fa 91 81 3c 92 bb c5 5f 25 25 eb 7c e7 d8 a1", // 562
    ];

    /// Appendix C.2, transcribed the same way.
    const C2: &[&str] = &[
        "30 82 02 71",                         // 0: Certificate SEQUENCE, 625 content bytes
        "30 82 01 da",                         // 4: TBSCertificate SEQUENCE, 474 content bytes
        "a0 03",                               // 8: [0] EXPLICIT version
        "02 01 02",                            // 10: INTEGER 2 (v3)
        "02 01 12",                            // 13: INTEGER 18 (serialNumber)
        "30 0d",                               // 16: AlgorithmIdentifier SEQUENCE
        "06 09 2a 86 48 86 f7 0d 01 01 05",    // 18: OID sha1WithRSAEncryption
        "05 00",                               // 29: NULL parameters
        "30 43",                               // 31: Name SEQUENCE, 67 bytes
        "31 13",                               // 33: RelativeDistinguishedName SET
        "30 11",                               // 35: AttributeTypeAndValue SEQUENCE
        "06 0a 09 92 26 89 93 f2 2c 64 01 19", // 37: OID domainComponent 0.9.2342.19200300.100.1.25
        "16 03 63 6f 6d",                      // 49: IA5String 'com'
        "31 17",                               // 54: RelativeDistinguishedName SET
        "30 15",                               // 56: AttributeTypeAndValue SEQUENCE
        "06 0a 09 92 26 89 93 f2 2c 64 01 19", // 58: OID domainComponent
        "16 07 65 78 61 6d 70 6c 65",          // 70: IA5String 'example'
        "31 13",                               // 79: RelativeDistinguishedName SET
        "30 11",                               // 81: AttributeTypeAndValue SEQUENCE
        "06 03 55 04 03",                      // 83: OID commonName 2.5.4.3
        "13 0a 45 78 61 6d 70 6c 65 20 43 41", // 88: PrintableString 'Example CA'
        "30 1e",                               // 100: Validity SEQUENCE
        "17 0d 30 34 30 39 31 35 31 31 34 38 32 31 5a", // 102: UTCTime 2004-09-15 11:48:21Z
        "17 0d 30 35 30 33 31 35 31 31 34 38 32 31 5a", // 117: UTCTime 2005-03-15 11:48:21Z
        "30 43",                               // 132: Name SEQUENCE, 67 bytes
        "31 13",                               // 134: RelativeDistinguishedName SET
        "30 11",                               // 136: AttributeTypeAndValue SEQUENCE
        "06 0a 09 92 26 89 93 f2 2c 64 01 19", // 138: OID domainComponent 0.9.2342.19200300.100.1.25
        "16 03 63 6f 6d",                      // 150: IA5String 'com'
        "31 17",                               // 155: RelativeDistinguishedName SET
        "30 15",                               // 157: AttributeTypeAndValue SEQUENCE
        "06 0a 09 92 26 89 93 f2 2c 64 01 19", // 159: OID domainComponent
        "16 07 65 78 61 6d 70 6c 65",          // 171: IA5String 'example'
        "31 13",                               // 180: RelativeDistinguishedName SET
        "30 11",                               // 182: AttributeTypeAndValue SEQUENCE
        "06 03 55 04 03",                      // 184: OID commonName 2.5.4.3
        "13 0a 45 6e 64 20 45 6e 74 69 74 79", // 189: PrintableString 'End Entity'
        "30 81 9f",                            // 201: SubjectPublicKeyInfo SEQUENCE
        "30 0d",                               // 204: AlgorithmIdentifier SEQUENCE
        "06 09 2a 86 48 86 f7 0d 01 01 01",    // 206: OID rsaEncryption
        "05 00",                               // 217: NULL parameters
        "03 81 8d 00",                         // 219: BIT STRING, 0 unused bits
        "30 81 89",                            // 223: RSAPublicKey SEQUENCE
        "02 81 81",                            // 226: INTEGER modulus, 129 bytes
        "00 e1 6a e4 03 30 97 02 3c f4 10 f3 b5 1e 4d 7f", // 229: the 1024-bit modulus
        "14 7b f6 f5 d0 78 e9 a4 8a f0 a3 75 ec ed b6 56", // 245
        "96 7f 88 99 85 9a f2 3e 68 77 87 eb 9e d1 9f c0", // 261
        "b4 17 dc ab 89 23 a4 1d 7e 16 23 4c 4f a8 4d f5", // 277
        "31 b8 7c aa e3 1a 49 09 f4 4b 26 db 27 67 30 82", // 293
        "12 01 4a e9 1a b6 c1 0c 53 8b 6c fc 2f 7a 43 ec", // 309
        "33 36 7e 32 b2 7b d5 aa cf 01 14 c6 12 ec 13 f2", // 325
        "2d 14 7a 8b 21 58 14 13 4c 46 a3 9a f2 16 95 ff", // 341
        "23",                                  // 357
        "02 03 01 00 01",                      // 358: INTEGER 65537 (publicExponent)
        "a3 75",                               // 363: [3] EXPLICIT extensions
        "30 73",                               // 365: Extensions SEQUENCE
        "30 21",                               // 367: Extension SEQUENCE (subjectAltName)
        "06 03 55 1d 11",                      // 369: OID subjectAltName 2.5.29.17
        "04 1a",                               // 374: OCTET STRING wrapper
        "30 18",                               // 376: GeneralNames SEQUENCE
        "81 16 65 6e 64 2e 65 6e 74 69 74 79 40 65 78 61", // 378: [1] rfc822Name 'end.entity@example.com'
        "6d 70 6c 65 2e 63 6f 6d",                         // 394
        "30 1d",          // 402: Extension SEQUENCE (subjectKeyIdentifier)
        "06 03 55 1d 0e", // 404: OID subjectKeyIdentifier 2.5.29.14
        "04 16",          // 409: OCTET STRING wrapper
        "04 14 17 7b 92 30 ff 44 d6 66 e1 90 10 22 6c 16", // 411: OCTET STRING, the 20-byte key identifier
        "4f c0 8e 41 dd 6d",                               // 427
        "30 1f",          // 433: Extension SEQUENCE (authorityKeyIdentifier)
        "06 03 55 1d 23", // 435: OID authorityKeyIdentifier 2.5.29.35
        "04 18",          // 440: OCTET STRING wrapper
        "30 16",          // 442: AuthorityKeyIdentifier SEQUENCE
        "80 14 08 68 af 85 33 c8 39 4a 7a f8 82 93 8e 70", // 444: [0] keyIdentifier, C.1's subjectKeyIdentifier
        "6a 4a 20 84 2c 32",                               // 460
        "30 0e",                                           // 466: Extension SEQUENCE (keyUsage)
        "06 03 55 1d 0f",                                  // 468: OID keyUsage 2.5.29.15
        "01 01 ff",                                        // 473: BOOLEAN TRUE (critical)
        "04 04",                                           // 476: OCTET STRING wrapper
        "03 02 06 c0", // 478: BIT STRING '11'B: digitalSignature, contentCommitment
        "30 0d",       // 482: AlgorithmIdentifier SEQUENCE (outer)
        "06 09 2a 86 48 86 f7 0d 01 01 05", // 484: OID sha1WithRSAEncryption
        "05 00",       // 495: NULL parameters
        "03 81 81 00", // 497: BIT STRING signatureValue, 0 unused bits
        "00 20 28 34 5b 68 32 01 bb 0a 36 0e ad 71 c5 95", // 501: the 1024-bit signature
        "1a e1 04 cf ae ad c7 62 14 a4 1b 36 31 c0 e2 0c", // 517
        "3d d9 1e c0 00 dc 10 a0 ba 85 6f 41 cb 62 7a b7", // 533
        "4c 63 81 26 5e d2 80 45 5e 33 e7 70 45 3b 39 3b", // 549
        "26 4a 9c 3b f2 26 36 69 08 79 bb fb 96 43 77 4b", // 565
        "61 8b a1 ab 91 64 e0 f3 37 61 3c 1a a3 a4 c9 8a", // 581
        "b2 bf 73 d4 4d e4 58 e4 62 ea bc 20 74 92 86 0e", // 597
        "ce 84 60 76 e9 73 bb c7 85 d3 91 45 ea 62 5d cd", // 613
    ];

    #[test]
    fn the_appendix_c1_transcription_is_self_consistent() {
        // The prose states the size; the dump states the offsets. Both are
        // checked, because either alone could be satisfied by a typo.
        let der = rfc5280_c1();
        assert_eq!(der.len(), 578, "the appendix says 578 bytes");
        assert_eq!(rfc5280_c2().len(), 629, "the appendix says 629 bytes");

        // Walking the whole thing must land on exactly the offsets the
        // appendix prints beside each field.
        let budget = Budget::default();
        let mut cursor = Cursor::new(&der, &budget);
        let certificate = cursor.read().expect("the Certificate SEQUENCE");
        assert_eq!(certificate.range(), 0..578);
        let mut top = certificate.children(&budget).expect("constructed");
        assert_eq!(top.read().expect("TBSCertificate").range(), 4..431);
        assert_eq!(top.read().expect("signatureAlgorithm").range(), 431..446);
        assert_eq!(top.read().expect("signatureValue").range(), 446..578);
        assert!(top.finish().is_ok());
    }

    #[test]
    fn the_appendix_c1_certificate_reads_to_the_values_it_states() {
        let der = rfc5280_c1();
        let certificate = Certificate::parse(&der).expect("RFC 5280's own example");

        // (a) "the serial number is 17"
        assert_eq!(certificate.version(), Version::V3);
        assert_eq!(certificate.serial().as_u64(), Ok(17));

        // (b) "signed with RSA and the SHA-1 hash algorithm"
        assert_eq!(certificate.signature_algorithm().oid(), oid::SHA1_WITH_RSA);
        assert!(certificate.signature_algorithm().parameters_are_null());
        assert!(certificate.signature_algorithms_agree());
        assert_eq!(certificate.signature().bytes().len(), 128);
        assert_eq!(certificate.signature().unused_bits(), 0);

        // (c) and (d) "cn=Example CA,dc=example,dc=com", both fields
        assert_eq!(
            certificate.issuer().to_rfc4514(),
            "CN=Example CA,DC=example,DC=com"
        );
        assert_eq!(
            certificate.subject().to_rfc4514(),
            "CN=Example CA,DC=example,DC=com"
        );
        assert!(certificate.is_self_issued());

        // (e) "issued on April 30, 2004 and expired on April 30, 2005"
        assert_eq!(certificate.validity().not_before, 1_083_335_134);
        assert_eq!(certificate.validity().not_after, 1_114_871_134);
        assert!(certificate.validity().contains(1_100_000_000));
        assert!(!certificate.validity().contains(1_083_335_133));
        assert!(!certificate.validity().contains(1_114_871_135));

        // (f) "a 1024-bit RSA public key"
        let spki = certificate.subject_public_key_info();
        assert_eq!(spki.algorithm().oid(), oid::RSA_ENCRYPTION);
        match spki.public_key() {
            PublicKey::Rsa { modulus, exponent } => {
                // 128 octets is 1024 bits; the DER INTEGER carried 129,
                // because the leading octet has its top bit set.
                assert_eq!(modulus.len(), 128);
                assert_eq!(modulus.first(), Some(&0xC2));
                assert_eq!(exponent, &[0x01, 0x00, 0x01]);
            }
            other => panic!("expected an RSA key, got {other:?}"),
        }

        // (g) "a subject key identifier extension generated using method (1)"
        let ski = certificate
            .extensions()
            .subject_key_identifier()
            .expect("the extension is present");
        assert_eq!(
            ski,
            &unhex("08 68 AF 85 33 C8 39 4A 7A F8 82 93 8E 70 6A 4A 20 84 2C 32")[..]
        );
        // Method (1) is SHA-1 over the key bits, so the extension the issuer
        // wrote and the number this crate computes have to be the same twenty
        // bytes. This is the one place milestone 2 exercises the crypto edge,
        // and RFC 5280's own certificate is the published vector for it.
        assert_eq!(&certificate.key_identifier_sha1()[..], ski);

        // (h) "a CA certificate (as indicated through the basic constraints
        // extension)"
        assert_eq!(
            certificate.extensions().basic_constraints(),
            Some(BasicConstraints {
                ca: true,
                path_len: None
            })
        );
        let usage = certificate
            .extensions()
            .key_usage()
            .expect("keyUsage is present");
        assert!(usage.key_cert_sign());
        assert!(usage.crl_sign());
        assert!(!usage.digital_signature());

        // Three extensions, two of them critical, none of them unrecognised.
        assert_eq!(certificate.extensions().all().len(), 3);
        assert_eq!(
            certificate
                .extensions()
                .all()
                .iter()
                .filter(|e| e.is_critical())
                .count(),
            2
        );
        assert_eq!(certificate.extensions().unrecognised_critical().count(), 0);
        assert!(certificate.issuer_unique_id().is_none());
        assert!(certificate.subject_unique_id().is_none());
    }

    #[test]
    fn the_tbs_range_names_the_bytes_a_signature_covers() {
        let der = rfc5280_c1();
        let certificate = Certificate::parse(&der).expect("parses");
        // The appendix puts the TBSCertificate at offset 4 with 423 content
        // bytes behind a four-byte header.
        assert_eq!(certificate.tbs_range(), 4..431);
        assert_eq!(&der[certificate.tbs_range()], certificate.tbs());
        // And it is the *original* encoding: the tag and length are included,
        // because that is what gets digested.
        assert_eq!(certificate.tbs().first(), Some(&0x30));
        assert_eq!(certificate.tbs().len(), 427);
        assert_eq!(certificate.der(), &der[..]);
    }

    #[test]
    fn the_appendix_c2_certificate_reads_to_the_values_it_states() {
        let der = rfc5280_c2();
        let certificate = Certificate::parse(&der).expect("RFC 5280's own example");

        // (a) "the serial number is 18"
        assert_eq!(certificate.serial().as_u64(), Ok(18));
        // (c) and (d): issued by the C.1 CA, about a different subject.
        assert_eq!(
            certificate.issuer().to_rfc4514(),
            "CN=Example CA,DC=example,DC=com"
        );
        assert_eq!(
            certificate.subject().to_rfc4514(),
            "CN=End Entity,DC=example,DC=com"
        );
        assert!(!certificate.is_self_issued());

        // (e) "valid from September 15, 2004 through March 15, 2005"
        assert_eq!(certificate.validity().not_before, 1_095_248_901);
        assert_eq!(certificate.validity().not_after, 1_110_887_301);

        // (g) "an end entity certificate, as the basic constraints extension
        // is not present"
        assert_eq!(certificate.extensions().basic_constraints(), None);

        // (h) "an authority key identifier extension matching the subject key
        // identifier of the certificate in appendix C.1"
        let authority = certificate
            .extensions()
            .authority_key_identifier()
            .expect("the extension is present");
        let issuer_der = rfc5280_c1();
        let issuer = Certificate::parse(&issuer_der).expect("parses");
        assert_eq!(
            authority.key_identifier(),
            issuer.extensions().subject_key_identifier()
        );
        assert!(authority.issuer_der().is_none());
        assert!(authority.serial().is_none());
        // C.2's own subjectKeyIdentifier turns out to be method (1) as well —
        // the appendix says so only about C.1 — so the same SHA-1 that pins
        // C.1's transcription pins this certificate's key bits too. Every one
        // of the 129 modulus octets has to be right for this to hold.
        assert_eq!(
            certificate.extensions().subject_key_identifier(),
            Some(&certificate.key_identifier_sha1()[..])
        );
        // The issuer's subject is this certificate's issuer, which is the
        // link a chain is built out of.
        assert!(certificate.issuer().matches(issuer.subject()));

        // (i) "one alternative name -- an electronic mail address"
        let san = certificate
            .extensions()
            .find(oid::CE_SUBJECT_ALT_NAME)
            .expect("subjectAltName is present");
        assert!(!san.is_critical());
        // Carried, not decoded: `GeneralNames` is a later milestone's job, so
        // the bytes are handed over exactly as they arrived.
        assert_eq!(
            san.value(),
            &unhex(
                "30 18 81 16 65 6E 64 2E 65 6E 74 69 74 79 40
                 65 78 61 6D 70 6C 65 2E 63 6F 6D"
            )[..]
        );
        // And it is critical-but-unknown territory if it ever became
        // critical, which is exactly what this reports.
        assert_eq!(certificate.extensions().unrecognised_critical().count(), 0);

        let usage = certificate
            .extensions()
            .key_usage()
            .expect("keyUsage is present");
        assert!(usage.digital_signature());
        assert!(usage.content_commitment());
        assert!(!usage.key_cert_sign());
        assert_eq!(usage.bits(), 0b11);
    }

    /// Rebuilds C.1 with one span replaced, so a refusal test differs from
    /// the certificate it is testing by exactly that span.
    ///
    /// The two outer lengths — the `Certificate` and the `TBSCertificate` —
    /// move with the edit here, because every edit is inside both. Anything
    /// nested between them is the caller's to patch, and each test that needs
    /// to says which byte it is patching and why.
    fn c1_with(at: usize, replacing: usize, bytes: &[u8]) -> Vec<u8> {
        let original = rfc5280_c1();
        let mut out = original[..at].to_vec();
        out.extend_from_slice(bytes);
        out.extend_from_slice(&original[at + replacing..]);
        let delta = bytes.len().wrapping_sub(replacing);
        for length_at in [2usize, 6] {
            let was = (usize::from(out[length_at]) << 8) | usize::from(out[length_at + 1]);
            let now = was.wrapping_add(delta);
            out[length_at] = (now >> 8) as u8;
            out[length_at + 1] = (now & 0xFF) as u8;
        }
        out
    }

    #[test]
    fn trailing_bytes_after_a_certificate_are_refused() {
        let mut der = rfc5280_c1();
        der.push(0x00);
        assert_eq!(Certificate::parse(&der), Err(X509Error::TrailingBytes));
    }

    #[test]
    fn a_version_this_profile_does_not_know_is_refused_by_number() {
        // `[0] EXPLICIT INTEGER 3`, in the place of v3's `2`.
        let der = c1_with(8, 5, &unhex("A0 03 02 01 03"));
        assert_eq!(
            Certificate::parse(&der),
            Err(X509Error::UnsupportedVersion(3))
        );
    }

    #[test]
    fn extensions_on_a_certificate_that_is_not_v3_are_refused() {
        // The `[0]` version field removed, so the certificate defaults to v1
        // — and still carries the `[3]` extensions C.1 was written with.
        let der = c1_with(8, 5, &[]);
        assert_eq!(
            Certificate::parse(&der),
            Err(X509Error::ExtensionsRequireV3(Version::V1))
        );
    }

    #[test]
    fn a_repeated_extension_is_refused_and_names_itself() {
        // basicConstraints a second time: bytes 414..431 are that extension's
        // whole encoding, appended inside the extensions sequence.
        let original = rfc5280_c1();
        let duplicate = original[414..431].to_vec();
        let mut der = c1_with(431, 0, &duplicate);
        // The two containers between the TBSCertificate and the insertion
        // point: the `[3]` wrapper at 363 and the Extensions SEQUENCE at 365.
        der[364] += 17;
        der[366] += 17;
        assert_eq!(
            Certificate::parse(&der),
            Err(X509Error::BadExtension {
                oid: "2.5.29.19".to_string(),
                fault: ExtensionFault::Duplicate,
            })
        );
    }

    #[test]
    fn a_certificate_with_no_extensions_at_all_is_a_v1_certificate() {
        // The `[0]` version and the `[3]` extensions both removed: what is
        // left is a legal v1 certificate, which is the shape a root from the
        // 1990s has.
        let original = rfc5280_c1();
        let mut der = original[..8].to_vec();
        der.extend_from_slice(&original[13..363]);
        der.extend_from_slice(&original[431..]);
        // 574 - 73 and 423 - 73, written out rather than computed, so wrong
        // arithmetic here fails as a parse error rather than agreeing with
        // itself.
        der[2] = 0x01;
        der[3] = 0xF5;
        der[6] = 0x01;
        der[7] = 0x5E;

        let certificate = Certificate::parse(&der).expect("a v1 certificate");
        assert_eq!(certificate.version(), Version::V1);
        assert!(certificate.extensions().all().is_empty());
        assert_eq!(certificate.extensions().basic_constraints(), None);
        assert_eq!(certificate.serial().as_u64(), Ok(17));
        assert_eq!(certificate.subject().common_name(), Some("Example CA"));
    }

    #[test]
    fn an_unrecognised_key_algorithm_leaves_the_certificate_readable() {
        // The DSA OID 1.2.840.10040.4.1 in place of rsaEncryption, which is
        // the shape RFC 5280's own Appendix C.3 certificate has. Two octets
        // shorter, so the two containers above it shrink to match.
        let mut der = c1_with(206, 11, &unhex("06 07 2A 86 48 CE 38 04 01"));
        der[203] -= 2; // SubjectPublicKeyInfo, whose length is a long form
        der[205] -= 2; // its AlgorithmIdentifier
        let certificate = Certificate::parse(&der).expect("still a certificate");
        assert_eq!(
            certificate.subject_public_key_info().public_key(),
            PublicKey::Unrecognised
        );
        // Everything else still reads, which is the point of degrading here
        // rather than refusing: a certificate whose key this crate cannot use
        // still has a name, a serial and a validity window to report.
        assert_eq!(certificate.serial().as_u64(), Ok(17));
        assert_eq!(certificate.subject().common_name(), Some("Example CA"));
    }

    #[test]
    fn a_malformed_rsa_key_is_refused_rather_than_reported_as_a_key() {
        // The modulus INTEGER re-tagged as an OCTET STRING: same length, so
        // nothing else in the certificate moves, and `rsaEncryption` now
        // announces a key whose bits are not an `RSAPublicKey`.
        let der = c1_with(226, 1, &[0x04]);
        assert!(matches!(
            Certificate::parse(&der),
            Err(X509Error::BadPublicKey(KeyFault::RsaShape(_)))
        ));
    }

    #[test]
    fn a_truncated_certificate_is_refused_at_every_length() {
        // Ruling 1: no length of a real certificate's prefix may panic, and
        // every one of them must refuse rather than invent a value.
        let full = rfc5280_c1();
        for cut in 0..full.len() {
            assert!(
                Certificate::parse(&full[..cut]).is_err(),
                "a {cut}-byte prefix parsed as a whole certificate"
            );
        }
        assert!(Certificate::parse(&full).is_ok());
    }

    #[test]
    fn a_certificate_nested_past_the_cap_is_refused_rather_than_walked() {
        let der = rfc5280_c1();
        assert_eq!(
            Certificate::parse_with(&der, Limits::new(2, 65_536)),
            Err(X509Error::Der(DerError::DepthExceeded))
        );
        assert_eq!(
            Certificate::parse_with(&der, Limits::new(32, 4)),
            Err(X509Error::Der(DerError::NodeBudgetExceeded))
        );
    }

    #[test]
    fn an_extension_whose_value_is_not_its_own_syntax_is_refused_by_name() {
        // basicConstraints' inner SEQUENCE re-tagged as a NULL: the
        // `extnValue` no longer holds what §4.2.1.9 defines, and the refusal
        // says which extension rather than only which rule.
        let der = c1_with(426, 1, &[0x05]);
        assert!(matches!(
            Certificate::parse(&der),
            Err(X509Error::BadExtension { ref oid, .. }) if oid == "2.5.29.19"
        ));
    }

    #[test]
    fn a_key_usage_bit_string_is_read_from_the_left() {
        // All nine named bits set — `'111111111'B`, seven unused — so every
        // accessor is exercised against its own position rather than against
        // zero. One octet longer than C.1's, so the three containers above it
        // grow by one.
        let mut der = c1_with(408, 6, &unhex("04 05 03 03 07 FF 80"));
        der[399] += 1; // the keyUsage Extension SEQUENCE, which starts at 398
        der[366] += 1; // the Extensions SEQUENCE
        der[364] += 1; // the [3] wrapper
        let certificate = Certificate::parse(&der).expect("parses");
        let usage = certificate.extensions().key_usage().expect("present");
        assert!(usage.digital_signature());
        assert!(usage.content_commitment());
        assert!(usage.key_encipherment());
        assert!(usage.data_encipherment());
        assert!(usage.key_agreement());
        assert!(usage.key_cert_sign());
        assert!(usage.crl_sign());
        assert!(usage.encipher_only());
        assert!(usage.decipher_only());
        assert_eq!(usage.bits(), 0b1_1111_1111);
        assert!(!usage.is_empty());
    }

    #[test]
    fn a_name_attribute_that_is_not_text_survives_the_parse() {
        // The subject's commonName value replaced with a NULL: legal DER, not
        // a string, and ten octets shorter — so the three containers around
        // it shrink to match.
        let mut der = c1_with(189, 12, &unhex("05 00"));
        der[183] -= 10; // the AttributeTypeAndValue SEQUENCE
        der[181] -= 10; // the RelativeDistinguishedName SET
        der[133] -= 10; // the subject Name SEQUENCE
        let certificate = Certificate::parse(&der).expect("parses");
        assert_eq!(certificate.subject().common_name(), None);
        let attribute = &certificate.subject().rdns()[2].attributes()[0];
        assert_eq!(attribute.text(), &AttributeText::NotAString);
        // And a subject whose commonName is not readable no longer matches
        // the issuer's, which is the conservative direction: a link that
        // cannot be established is reported, never assumed.
        assert!(!certificate.is_self_issued());
    }

    #[test]
    fn the_extensions_a_caller_must_decide_about_are_the_ones_not_decoded() {
        // A critical extension this crate carries but does not read —
        // `certificatePolicies` — is what §4.2 makes a validator's problem,
        // and reporting it is the whole of this parser's job about it.
        let original = rfc5280_c1();
        // `SEQUENCE { OID 2.5.29.32, BOOLEAN TRUE, OCTET STRING {} }`
        let extra = unhex("30 0A 06 03 55 1D 20 01 01 FF 04 00");
        let mut der = c1_with(431, 0, &extra);
        der[364] += 12;
        der[366] += 12;
        assert_eq!(original.len() + 12, der.len());

        let certificate = Certificate::parse(&der).expect("parses");
        assert_eq!(certificate.extensions().all().len(), 4);
        let unrecognised: Vec<_> = certificate
            .extensions()
            .unrecognised_critical()
            .map(|extension| extension.oid().to_dotted())
            .collect();
        assert_eq!(unrecognised, vec!["2.5.29.32".to_string()]);
    }

    /// Writes the seeds `fuzz/corpus/pki_der/` carries, so the seeds and the
    /// certificates the tests above assert against cannot drift apart.
    ///
    /// Run with `--ignored` when a fixture changes; the corpus is committed,
    /// and a run that rewrites it is a diff to look at rather than to apply
    /// blindly. This is the arrangement `crypt` and `cff` already use, and the
    /// reason is the one that file records: a hand-laid corpus that no longer
    /// reaches what it was chosen for looks exactly like one that does.
    ///
    /// Seven seeds, each chosen for a region of the parser a mutation is
    /// unlikely to reach on its own.
    #[test]
    #[ignore = "writes into fuzz/corpus/, which is committed"]
    fn write_the_fuzz_seeds() {
        let base =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/pki_der");
        std::fs::create_dir_all(&base).expect("the corpus directory is creatable");

        // A v1 certificate: no `[0]`, no `[3]`, which is a different path
        // through `parse_with` than every other seed takes.
        let original = rfc5280_c1();
        let mut v1 = original[..8].to_vec();
        v1.extend_from_slice(&original[13..363]);
        v1.extend_from_slice(&original[431..]);
        v1[2] = 0x01;
        v1[3] = 0xF5;
        v1[6] = 0x01;
        v1[7] = 0x5E;

        // An algorithm OID nothing here decodes, so `PublicKey::Unrecognised`
        // is reached from a certificate that is otherwise entirely valid.
        let mut unrecognised = c1_with(206, 11, &unhex("06 07 2A 86 48 CE 38 04 01"));
        unrecognised[203] -= 2;
        unrecognised[205] -= 2;

        // Forty nested SEQUENCEs: past `Limits::CERTIFICATE`'s depth cap and
        // far past the one-level cap the target's second pass uses. Eighty
        // bytes, which is the point — depth is cheap and the cap is what makes
        // it bounded.
        let mut deep = Vec::new();
        for level in 0..40u8 {
            deep.push(0x30);
            deep.push(2 * (39 - level));
        }

        // Legal BER, refused DER: the indefinite length, with the
        // end-of-contents pair this crate never goes looking for.
        let indefinite = unhex("30 80 02 01 05 00 00");

        // A bare `Name`, so the distinguished-name reader and the string
        // decoders are reachable without a whole certificate in front of them.
        let name_only = unhex(
            "30 43
             31 13 30 11 06 0A 09 92 26 89 93 F2 2C 64 01 19 16 03 63 6F 6D
             31 17 30 15 06 0A 09 92 26 89 93 F2 2C 64 01 19 16 07 65 78 61 6D 70 6C 65
             31 13 30 11 06 03 55 04 03 13 0A 45 78 61 6D 70 6C 65 20 43 41",
        );

        for (name, bytes) in [
            ("rfc5280-c1-self-signed-ca", rfc5280_c1()),
            ("rfc5280-c2-end-entity", rfc5280_c2()),
            ("v1-no-extensions", v1),
            ("unrecognised-key-algorithm", unrecognised),
            ("deep-nesting", deep),
            ("indefinite-length", indefinite),
            ("name-only", name_only),
        ] {
            std::fs::write(base.join(name), bytes).expect("the corpus directory is there");
        }
    }
}
