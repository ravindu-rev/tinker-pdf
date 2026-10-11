//! RFC 3161 timestamp tokens: a `SignedData` whose content is a `TSTInfo`,
//! the authority's signed statement that a digest existed at a time.
//!
//! ```text
//! TimeStampToken ::= ContentInfo   -- id-signedData, eContentType id-ct-TSTInfo
//!
//! TSTInfo ::= SEQUENCE {
//!    version          INTEGER { v1(1) },
//!    policy           TSAPolicyId,             -- OBJECT IDENTIFIER
//!    messageImprint   MessageImprint,
//!    serialNumber     INTEGER,
//!    genTime          GeneralizedTime,
//!    accuracy         Accuracy         OPTIONAL,
//!    ordering         BOOLEAN          DEFAULT FALSE,
//!    nonce            INTEGER          OPTIONAL,
//!    tsa              [0] GeneralName  OPTIONAL,
//!    extensions       [1] IMPLICIT Extensions OPTIONAL }
//!
//! MessageImprint ::= SEQUENCE { hashAlgorithm AlgorithmIdentifier,
//!                               hashedMessage OCTET STRING }
//! ```
//!
//! # What this module establishes, and what it leaves to its caller
//!
//! It reads. A token is a `ContentInfo`, so the envelope is
//! [`crate::cms::ContentInfo`]'s and the signer, the certificates and the
//! signed attributes come out of it exactly as they do for a document
//! signature; what this adds is the content. Whether the token's signature
//! verifies, whether its imprint is the digest of what it claims to stamp, and
//! whether its certificate is one a timestamping authority may sign with are
//! questions for a verifier, and the facade's `verdict` module asks all three.
//!
//! # The time, and the one place it is not RFC 5280's
//!
//! `genTime` is a GeneralizedTime, and RFC 3161 §2.4.2 widens RFC 5280's
//! profile of it in exactly one way: **fractional seconds are allowed**, as
//! `.` and digits before the `Z`, with trailing zeros forbidden and a whole
//! second written with no point at all. [`TstInfo::time`] is the whole
//! second; [`TstInfo::nanoseconds`] is the fraction, read to nine digits.
//! A tenth digit, a trailing zero, a comma or a bare point is refused by
//! name, because each is an encoding §2.4.2 says the value must not have.
//!
//! `tsa` is `[0] GeneralName`, and the module's IMPLICIT default does not
//! reach it: `GeneralName` is a `CHOICE`, so the tag is explicit
//! ([`crate::general_name`] says why that matters).

use crate::cms::{digest_algorithm, CmsError, ContentInfo, DigestAlgorithm};
use crate::der::{Budget, Cursor, DerError, Int, Limits, Oid, Tag, TimeFault, Tlv};
use crate::general_name::{GeneralName, GeneralNameError};
use crate::oid;
use crate::x509::AlgorithmIdentifier;

/// Why a timestamp token could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TstError {
    /// The `ContentInfo` around it.
    Cms(CmsError),
    /// The `TSTInfo`'s own encoding.
    Der(DerError),
    /// An `eContentType` that is not `id-ct-TSTInfo`, by its dotted OID: a
    /// `SignedData`, and not a timestamp.
    NotTstInfo {
        /// The content type it carries instead.
        oid: String,
    },
    /// A detached `SignedData`: a token carries its `TSTInfo`, or it is not
    /// one (RFC 3161 §2.4.2).
    NoContent,
    /// A `version` other than 1.
    Version(u64),
    /// `accuracy`'s `millis` or `micros` outside `1..=999`.
    Accuracy,
    /// The `tsa` field.
    Name(GeneralNameError),
}

impl From<DerError> for TstError {
    fn from(error: DerError) -> Self {
        Self::Der(error)
    }
}

impl From<CmsError> for TstError {
    fn from(error: CmsError) -> Self {
        Self::Cms(error)
    }
}

impl From<GeneralNameError> for TstError {
    fn from(error: GeneralNameError) -> Self {
        Self::Name(error)
    }
}

impl core::fmt::Display for TstError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Cms(error) => write!(f, "the token's envelope: {error}"),
            Self::Der(error) => write!(f, "the TSTInfo: {error}"),
            Self::NotTstInfo { oid } => {
                write!(f, "a SignedData whose content is {oid}, not a TSTInfo")
            }
            Self::NoContent => write!(f, "a detached SignedData, which no token is"),
            Self::Version(version) => write!(f, "TSTInfo version {version}"),
            Self::Accuracy => write!(f, "an accuracy whose millis or micros is not 1 to 999"),
            Self::Name(error) => write!(f, "the TSTInfo's tsa: {error}"),
        }
    }
}

impl std::error::Error for TstError {}

/// `Accuracy ::= SEQUENCE { seconds INTEGER OPTIONAL, millis [0] INTEGER
/// (1..999) OPTIONAL, micros [1] INTEGER (1..999) OPTIONAL }`, each absent
/// field zero.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Accuracy {
    /// Whole seconds.
    pub seconds: u64,
    /// Milliseconds, 0 to 999.
    pub millis: u16,
    /// Microseconds, 0 to 999.
    pub micros: u16,
}

/// A `TSTInfo` (RFC 3161 §2.4.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TstInfo<'a> {
    policy: Oid<'a>,
    imprint_algorithm: AlgorithmIdentifier<'a>,
    imprint: &'a [u8],
    serial: Int<'a>,
    time: i64,
    nanoseconds: u32,
    accuracy: Option<Accuracy>,
    ordering: bool,
    nonce: Option<Int<'a>>,
    tsa: Option<GeneralName<'a>>,
    extensions: Option<&'a [u8]>,
    der: &'a [u8],
}

/// Ceilings for a `TSTInfo`: a dozen fields, an `Accuracy` and a
/// `GeneralName` that may be a directory name four levels down.
const LIMITS: Limits = Limits::new(16, 4096);

impl<'a> TstInfo<'a> {
    /// Reads a `TSTInfo` — the `eContent` octets of a token. DER, because it
    /// is what the authority's `messageDigest` is the digest of.
    ///
    /// # Errors
    ///
    /// [`TstError`].
    pub fn parse(der: &'a [u8]) -> Result<Self, TstError> {
        let budget = Budget::new(LIMITS);
        let mut cursor = Cursor::new(der, &budget);
        let sequence = cursor.expect(Tag::Sequence)?;
        cursor.finish()?;
        let mut fields = sequence.children(&budget)?;

        let version = fields.expect(Tag::Integer)?.as_integer()?.as_u64()?;
        if version != 1 {
            return Err(TstError::Version(version));
        }
        let policy = fields.expect(Tag::Oid)?.as_oid()?;

        let imprint_node = fields.expect(Tag::Sequence)?;
        let mut imprint_fields = imprint_node.children(&budget)?;
        let imprint_algorithm = AlgorithmIdentifier::parse(&imprint_fields.read()?, &budget)?;
        let imprint = imprint_fields.expect(Tag::OctetString)?.as_octet_string()?;
        imprint_fields.finish()?;

        let serial = fields.expect(Tag::Integer)?.as_integer()?;
        let (time, nanoseconds) = gen_time(&fields.expect(Tag::GeneralizedTime)?)?;

        let accuracy = match fields.expect_optional(Tag::Sequence)? {
            Some(node) => Some(read_accuracy(&node, &budget)?),
            None => None,
        };
        // DEFAULT FALSE, and an encoded FALSE is taken rather than refused,
        // as `crate::x509` takes an encoded `critical FALSE`: it means what
        // the omitted field means, so there is no second reading to choose.
        let ordering = match fields.expect_optional(Tag::Boolean)? {
            Some(flag) => flag.as_bool()?,
            None => false,
        };
        let nonce = match fields.expect_optional(Tag::Integer)? {
            Some(node) => Some(node.as_integer()?),
            None => None,
        };
        let tsa = match fields.context_optional(0)? {
            Some(tagged) => Some(GeneralName::parse(&tagged.explicit(&budget)?, &budget)?),
            None => None,
        };
        let extensions = fields.context_optional(1)?.map(|tagged| tagged.raw());
        fields.finish()?;

        Ok(Self {
            policy,
            imprint_algorithm,
            imprint,
            serial,
            time,
            nanoseconds,
            accuracy,
            ordering,
            nonce,
            tsa,
            extensions,
            der: sequence.raw(),
        })
    }

    /// The policy the authority issued the token under.
    #[must_use]
    pub const fn policy(&self) -> Oid<'a> {
        self.policy
    }

    /// `messageImprint.hashAlgorithm`, as encoded.
    #[must_use]
    pub const fn imprint_algorithm(&self) -> AlgorithmIdentifier<'a> {
        self.imprint_algorithm
    }

    /// `messageImprint.hashAlgorithm`, resolved.
    ///
    /// # Errors
    ///
    /// [`CmsError::UnknownDigestAlgorithm`] for an OID this crate has no
    /// digest for.
    pub fn imprint_digest(&self) -> Result<DigestAlgorithm, CmsError> {
        digest_algorithm(self.imprint_algorithm.oid()).ok_or_else(|| {
            CmsError::UnknownDigestAlgorithm {
                oid: self.imprint_algorithm.oid().to_dotted(),
            }
        })
    }

    /// `messageImprint.hashedMessage`: the digest the authority stamped.
    #[must_use]
    pub const fn imprint(&self) -> &'a [u8] {
        self.imprint
    }

    /// The token's serial number, unique per authority (§2.4.2).
    #[must_use]
    pub const fn serial(&self) -> Int<'a> {
        self.serial
    }

    /// `genTime`'s whole second, as Unix seconds.
    #[must_use]
    pub const fn time(&self) -> i64 {
        self.time
    }

    /// `genTime`'s fraction of a second, in nanoseconds.
    #[must_use]
    pub const fn nanoseconds(&self) -> u32 {
        self.nanoseconds
    }

    /// `accuracy`, where the token states one.
    #[must_use]
    pub const fn accuracy(&self) -> Option<Accuracy> {
        self.accuracy
    }

    /// `ordering`.
    #[must_use]
    pub const fn ordering(&self) -> bool {
        self.ordering
    }

    /// `nonce`, where the request carried one.
    #[must_use]
    pub const fn nonce(&self) -> Option<Int<'a>> {
        self.nonce
    }

    /// `tsa`: the authority's own name for itself, where it gives one.
    ///
    /// A hint, not an identity: RFC 3161 §2.4.2 says it "is to give a hint
    /// in identifying the name of the TSA", and the identity is the
    /// certificate that signed the token.
    #[must_use]
    pub const fn tsa(&self) -> Option<&GeneralName<'a>> {
        self.tsa.as_ref()
    }

    /// `extensions`, carried as the `[1]` node's encoding.
    #[must_use]
    pub const fn extensions_der(&self) -> Option<&'a [u8]> {
        self.extensions
    }

    /// The whole `TSTInfo` encoding — what the token's `messageDigest` is
    /// the digest of.
    #[must_use]
    pub const fn der(&self) -> &'a [u8] {
        self.der
    }
}

/// `genTime` (§2.4.2): `YYYYMMDDhhmmss[.s...]Z`.
fn gen_time(node: &Tlv<'_>) -> Result<(i64, u32), DerError> {
    let value = node.value();
    let fault = |fault| DerError::MalformedTime(fault);
    let Some((&b'Z', body)) = value.split_last() else {
        return Err(fault(TimeFault::NotZulu));
    };
    let (whole, fraction) = match body.iter().position(|&byte| byte == b'.') {
        Some(at) => (&body[..at], Some(&body[at + 1..])),
        None => (body, None),
    };
    let nanoseconds = match fraction {
        None => 0,
        Some(digits) => {
            // At least one digit, at most nine, the last not zero: §2.4.2's
            // "MUST omit all trailing zeros", and a fraction of zero is no
            // point at all.
            if digits.is_empty() || digits.len() > 9 || digits.last() == Some(&b'0') {
                return Err(fault(TimeFault::Length));
            }
            let mut value = 0u32;
            for &digit in digits {
                if !digit.is_ascii_digit() {
                    return Err(fault(TimeFault::NotDigits));
                }
                value = value * 10 + u32::from(digit - b'0');
            }
            // Nine digits at most, so this stays below 10^9.
            value * 10u32.pow(9 - digits.len() as u32)
        }
    };
    let mut profile = [0u8; 15];
    if whole.len() != 14 {
        return Err(fault(TimeFault::Length));
    }
    profile[..14].copy_from_slice(whole);
    profile[14] = b'Z';
    Ok((crate::der::parse_generalized_time(&profile)?, nanoseconds))
}

fn read_accuracy(node: &Tlv<'_>, budget: &Budget) -> Result<Accuracy, TstError> {
    let mut fields = node.children(budget)?;
    let mut accuracy = Accuracy::default();
    if let Some(seconds) = fields.expect_optional(Tag::Integer)? {
        accuracy.seconds = seconds.as_integer()?.as_u64()?;
    }
    for (tag, slot) in [(0u32, &mut accuracy.millis), (1, &mut accuracy.micros)] {
        if let Some(tagged) = fields.context_optional(tag)? {
            let value = tagged.implicit(Tag::Integer).as_integer()?.as_u64()?;
            if !(1..=999).contains(&value) {
                return Err(TstError::Accuracy);
            }
            // In range by the check just made.
            *slot = value as u16;
        }
    }
    fields.finish()?;
    Ok(accuracy)
}

/// A `TimeStampToken`: the envelope and the `TSTInfo` inside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimeStampToken<'a> {
    content: ContentInfo<'a>,
    info: TstInfo<'a>,
}

impl<'a> TimeStampToken<'a> {
    /// Reads a token: a `ContentInfo` under [`Limits::CMS`], whose
    /// `SignedData` encapsulates a `TSTInfo`.
    ///
    /// # Errors
    ///
    /// [`TstError`].
    pub fn parse(der: &'a [u8]) -> Result<Self, TstError> {
        let content = ContentInfo::parse(der)?;
        let encapsulated = content.signed_data().encap_content_info();
        if encapsulated.content_type() != oid::ID_CT_TST_INFO {
            return Err(TstError::NotTstInfo {
                oid: encapsulated.content_type().to_dotted(),
            });
        }
        let econtent = encapsulated.content().ok_or(TstError::NoContent)?;
        let info = TstInfo::parse(econtent)?;
        Ok(Self { content, info })
    }

    /// The envelope: signer, certificates and attributes, as for any
    /// `SignedData`.
    #[must_use]
    pub const fn content_info(&self) -> &ContentInfo<'a> {
        &self.content
    }

    /// The `TSTInfo`.
    #[must_use]
    pub const fn info(&self) -> &TstInfo<'a> {
        &self.info
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tlv(tag: u8, content: &[u8]) -> Vec<u8> {
        assert!(content.len() < 0x80, "short form only");
        let mut out = vec![tag, content.len() as u8];
        out.extend_from_slice(content);
        out
    }

    /// A `TSTInfo` with the fields given, in order, around a SHA-256
    /// imprint of 32 `0xAB` octets.
    fn info_with(time: &[u8], tail: &[Vec<u8>]) -> Vec<u8> {
        let algorithm = tlv(
            0x30,
            &[
                0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01,
            ],
        );
        let imprint = tlv(0x30, &[algorithm, tlv(0x04, &[0xAB; 32])].concat());
        let mut fields = vec![
            tlv(0x02, &[1]),
            tlv(0x06, &[0x2A, 0x03, 0x04, 0x01]),
            imprint,
            tlv(0x02, &[0x2A]),
            tlv(0x18, time),
        ];
        fields.extend_from_slice(tail);
        tlv(0x30, &fields.concat())
    }

    #[test]
    fn a_minimal_tstinfo_reads() {
        let der = info_with(b"20261002120000Z", &[]);
        let info = TstInfo::parse(&der).expect("it parses");
        assert_eq!(info.policy().to_dotted(), "1.2.3.4.1");
        assert_eq!(info.imprint_digest(), Ok(DigestAlgorithm::Sha256));
        assert_eq!(info.imprint(), &[0xAB; 32][..]);
        assert_eq!(info.serial().as_bytes(), &[0x2A]);
        // 2026-10-02T12:00:00Z.
        assert_eq!(info.time(), 1_790_942_400);
        assert_eq!(info.nanoseconds(), 0);
        assert_eq!(info.accuracy(), None);
        assert!(!info.ordering());
        assert!(info.nonce().is_none() && info.tsa().is_none());
        assert_eq!(info.der(), &der[..]);
    }

    #[test]
    fn every_optional_field_reads() {
        let accuracy = tlv(
            0x30,
            &[tlv(0x02, &[1]), tlv(0x80, &[0x01, 0xF4]), tlv(0x81, &[100])].concat(),
        );
        let name = tlv(0xA0, &tlv(0x86, b"https://tsa.example/"));
        let der = info_with(
            b"20261002120000.25Z",
            &[accuracy, tlv(0x01, &[0xFF]), tlv(0x02, &[0x01, 0x02]), name],
        );
        let info = TstInfo::parse(&der).expect("it parses");
        assert_eq!(info.nanoseconds(), 250_000_000);
        assert_eq!(
            info.accuracy(),
            Some(Accuracy {
                seconds: 1,
                millis: 500,
                micros: 100
            })
        );
        assert!(info.ordering());
        assert_eq!(
            info.nonce().map(|n| n.as_bytes().to_vec()),
            Some(vec![1, 2])
        );
        assert_eq!(
            info.tsa(),
            Some(&GeneralName::Uri("https://tsa.example/".into()))
        );
    }

    /// RFC 3161 §2.4.2's fraction rules, each refused by name.
    #[test]
    fn a_fraction_the_profile_forbids_is_refused() {
        for (time, fault) in [
            (&b"20261002120000.50Z"[..], TimeFault::Length),
            (b"20261002120000.Z", TimeFault::Length),
            (b"20261002120000.1234567891Z", TimeFault::Length),
            (b"20261002120000,5Z", TimeFault::Length),
            (b"20261002120000.5x5Z", TimeFault::NotDigits),
            (b"20261002120000", TimeFault::NotZulu),
            (b"20261002120000.5+0100", TimeFault::NotZulu),
        ] {
            let der = info_with(time, &[]);
            assert_eq!(
                TstInfo::parse(&der),
                Err(TstError::Der(DerError::MalformedTime(fault))),
                "{}",
                String::from_utf8_lossy(time)
            );
        }
        let nine = info_with(b"20261002120000.123456789Z", &[]);
        assert_eq!(
            TstInfo::parse(&nine).map(|i| i.nanoseconds()),
            Ok(123_456_789)
        );
    }

    #[test]
    fn each_refusal_is_named() {
        let mut version_two = info_with(b"20261002120000Z", &[]);
        version_two[4] = 2;
        assert_eq!(TstInfo::parse(&version_two), Err(TstError::Version(2)));

        let millis = tlv(0x30, &tlv(0x80, &[0x03, 0xE8]));
        assert_eq!(
            TstInfo::parse(&info_with(b"20261002120000Z", &[millis])),
            Err(TstError::Accuracy)
        );

        // `tsa` tagged implicitly: `[0]` wearing the URI's content.
        let implicit = tlv(0xA0, b"https://tsa.example/");
        assert!(matches!(
            TstInfo::parse(&info_with(b"20261002120000Z", &[implicit])),
            Err(TstError::Der(_) | TstError::Name(_))
        ));
    }

    #[test]
    fn no_prefix_reads_and_no_flip_panics() {
        let accuracy = tlv(0x30, &[tlv(0x02, &[1]), tlv(0x80, &[0x01, 0xF4])].concat());
        let der = info_with(b"20261002120000.25Z", &[accuracy, tlv(0x02, &[7])]);
        assert!(TstInfo::parse(&der).is_ok());
        for cut in 0..der.len() {
            assert!(TstInfo::parse(&der[..cut]).is_err(), "a {cut}-octet prefix");
        }
        for at in 0..der.len() {
            for bit in 0..8 {
                let mut spoiled = der.clone();
                spoiled[at] ^= 1 << bit;
                let _ = TstInfo::parse(&spoiled);
                let _ = TimeStampToken::parse(&spoiled);
            }
        }
    }
}
