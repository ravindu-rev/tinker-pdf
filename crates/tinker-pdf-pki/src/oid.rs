//! The object identifiers this crate can name, written as their encodings.
//!
//! An OID has exactly one DER encoding (X.690 §8.19), so a table of content
//! octets is a table of comparable values: `algorithm.oid() == oid::RSA_ENCRYPTION`
//! is a slice comparison against nine bytes, with no arc decoding, no
//! allocation and no dotted-string parsing anywhere on the path. That is the
//! whole reason the encoding rather than the arcs is the representation.
//!
//! **Every constant here is checked.** [`Oid::from_content`] cannot validate
//! in a `const`, so `every_constant_is_a_well_formed_oid` below walks all of
//! them through [`Oid::parse`] and compares each against its dotted form
//! written out independently. A mistyped table entry would otherwise be
//! invisible: it would simply never match anything, and the symptom would be
//! an algorithm reported unsupported rather than a test failure.
//!
//! **What is here is what something reads.** An OID with no consumer is not
//! given a name, because a name in a table reads as support for the thing it
//! names. The signature algorithms below are named so a verdict can *report*
//! which one a certificate used; whether this engine can verify one is
//! milestones 4 and 5 of `docs/design/signatures.md`, and the two questions
//! are answered in different places on purpose.

use crate::der::Oid;

/// Writes a table entry, so an entry is one line and its arcs are beside it.
macro_rules! oids {
    ($( $(#[$meta:meta])* $name:ident = $dotted:literal, [$($byte:literal),* $(,)?]; )*) => {
        $(
            $(#[$meta])*
            #[doc = concat!("\n\n`", $dotted, "`")]
            pub const $name: Oid<'static> = Oid::from_content(&[$($byte),*]);
        )*

        /// Every constant above, with the arcs it is supposed to encode.
        #[cfg(test)]
        const TABLE: &[(&str, Oid<'static>, &str)] = &[
            $((stringify!($name), $name, $dotted),)*
        ];
    };
}

oids! {
    // ---- Signature algorithms (RFC 4055, RFC 5480, RFC 5758) ----

    /// PKCS#1's key transport algorithm, and the SubjectPublicKeyInfo
    /// algorithm under which the key is an `RSAPublicKey`.
    RSA_ENCRYPTION = "1.2.840.113549.1.1.1",
        [0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x01];
    /// RSASSA-PKCS1-v1_5 with SHA-1. Weak, and named so that a verdict can
    /// say which weak algorithm it was.
    SHA1_WITH_RSA = "1.2.840.113549.1.1.5",
        [0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x05];
    /// RSASSA-PSS. Named, not decoded: its parameters are a structure of
    /// their own and reading them is CMS work.
    RSASSA_PSS = "1.2.840.113549.1.1.10",
        [0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0A];
    SHA256_WITH_RSA = "1.2.840.113549.1.1.11",
        [0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0B];
    SHA384_WITH_RSA = "1.2.840.113549.1.1.12",
        [0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0C];
    SHA512_WITH_RSA = "1.2.840.113549.1.1.13",
        [0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0D];

    /// The SubjectPublicKeyInfo algorithm whose key is an elliptic-curve
    /// point and whose parameters name the curve (RFC 5480 §2.1.1).
    EC_PUBLIC_KEY = "1.2.840.10045.2.1",
        [0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x02, 0x01];
    ECDSA_WITH_SHA1 = "1.2.840.10045.4.1",
        [0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x01];
    ECDSA_WITH_SHA256 = "1.2.840.10045.4.3.2",
        [0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x02];
    ECDSA_WITH_SHA384 = "1.2.840.10045.4.3.3",
        [0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x03];
    ECDSA_WITH_SHA512 = "1.2.840.10045.4.3.4",
        [0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x04];

    // ---- Named curves (RFC 5480 §2.1.1.1) ----

    /// P-256, whose ANSI name is what certificates actually carry.
    SECP256R1 = "1.2.840.10045.3.1.7",
        [0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07];
    SECP384R1 = "1.3.132.0.34", [0x2B, 0x81, 0x04, 0x00, 0x22];
    SECP521R1 = "1.3.132.0.35", [0x2B, 0x81, 0x04, 0x00, 0x23];

    // ---- Digests (RFC 5754 names these for CMS) ----

    /// SHA-1. Present because signatures made with it exist and have to be
    /// reported, not because anything should be made with it.
    ID_SHA1 = "1.3.14.3.2.26", [0x2B, 0x0E, 0x03, 0x02, 0x1A];
    ID_SHA256 = "2.16.840.1.101.3.4.2.1",
        [0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01];
    ID_SHA384 = "2.16.840.1.101.3.4.2.2",
        [0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x02];
    ID_SHA512 = "2.16.840.1.101.3.4.2.3",
        [0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x03];

    // ---- CMS content types (RFC 5652 §4, §5.1) ----

    /// `id-data`: an octet string with no further structure, which is what
    /// every PDF signature encapsulates — emptily, for the detached
    /// subfilters (ISO 32000-1 12.8.3.3.1).
    ID_DATA = "1.2.840.113549.1.7.1",
        [0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x07, 0x01];
    /// `id-envelopedData`: what a PDF's public-key security handler puts in
    /// `/Recipients` (ISO 32000-1 7.6.5), read by [`crate::enveloped`].
    ID_ENVELOPED_DATA = "1.2.840.113549.1.7.3",
        [0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x07, 0x03];
    /// `id-signedData`: the only `ContentInfo` [`crate::cms`] reads.
    ID_SIGNED_DATA = "1.2.840.113549.1.7.2",
        [0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x07, 0x02];

    // ---- CMS attribute types (RFC 5652 §11, RFC 5035, RFC 3161) ----
    //
    // `AA_` rather than `AT_`: these are members of a `SignedAttributes` or
    // `UnsignedAttributes` set, which is a different namespace from the
    // `AT_` types above that name a component of a distinguished name. The
    // two would otherwise read as one table, and an OID from one is never
    // valid in the other. The RFCs spell them `id-contentType` and
    // `id-aa-timeStampToken`; the shared prefix here is what makes the set
    // legible as a set.

    /// `contentType` (§11.1): the signer's own statement of what they signed,
    /// which §5.3 requires to equal the `eContentType`.
    AA_CONTENT_TYPE = "1.2.840.113549.1.9.3",
        [0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x09, 0x03];
    /// `messageDigest` (§11.2): the digest a PDF verdict compares against its
    /// own digest of the `/ByteRange` spans.
    AA_MESSAGE_DIGEST = "1.2.840.113549.1.9.4",
        [0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x09, 0x04];
    /// `signingTime` (§11.3). Signed, and still only a claim: nothing checked
    /// it against a clock when it was written.
    AA_SIGNING_TIME = "1.2.840.113549.1.9.5",
        [0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x09, 0x05];
    /// `id-aa-signingCertificateV2` (RFC 5035 §3): digests of the
    /// certificates the signer says it used.
    AA_SIGNING_CERTIFICATE_V2 = "1.2.840.113549.1.9.16.2.47",
        [0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x09, 0x10, 0x02, 0x2F];
    /// `id-aa-timeStampToken` (RFC 3161 Appendix A): an unsigned attribute
    /// carrying a whole `ContentInfo` of its own.
    AA_TIMESTAMP_TOKEN = "1.2.840.113549.1.9.16.2.14",
        [0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x09, 0x10, 0x02, 0x0E];

    // ---- Attribute types in a distinguished name (RFC 5280 §A.1) ----

    AT_COMMON_NAME = "2.5.4.3", [0x55, 0x04, 0x03];
    AT_SURNAME = "2.5.4.4", [0x55, 0x04, 0x04];
    AT_SERIAL_NUMBER = "2.5.4.5", [0x55, 0x04, 0x05];
    AT_COUNTRY = "2.5.4.6", [0x55, 0x04, 0x06];
    AT_LOCALITY = "2.5.4.7", [0x55, 0x04, 0x07];
    AT_STATE_OR_PROVINCE = "2.5.4.8", [0x55, 0x04, 0x08];
    AT_STREET_ADDRESS = "2.5.4.9", [0x55, 0x04, 0x09];
    AT_ORGANISATION = "2.5.4.10", [0x55, 0x04, 0x0A];
    AT_ORGANISATIONAL_UNIT = "2.5.4.11", [0x55, 0x04, 0x0B];
    AT_TITLE = "2.5.4.12", [0x55, 0x04, 0x0C];
    AT_GIVEN_NAME = "2.5.4.42", [0x55, 0x04, 0x2A];
    AT_INITIALS = "2.5.4.43", [0x55, 0x04, 0x2B];
    AT_GENERATION_QUALIFIER = "2.5.4.44", [0x55, 0x04, 0x2C];
    AT_DN_QUALIFIER = "2.5.4.46", [0x55, 0x04, 0x2E];
    AT_PSEUDONYM = "2.5.4.65", [0x55, 0x04, 0x41];
    /// PKCS#9's `emailAddress`. Deprecated as a name attribute by RFC 5280
    /// §4.1.2.6 in favour of `subjectAltName`, and still everywhere.
    AT_EMAIL_ADDRESS = "1.2.840.113549.1.9.1",
        [0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x09, 0x01];
    /// `domainComponent`, which is how RFC 5280's own example certificates
    /// spell `example.com`.
    AT_DOMAIN_COMPONENT = "0.9.2342.19200300.100.1.25",
        [0x09, 0x92, 0x26, 0x89, 0x93, 0xF2, 0x2C, 0x64, 0x01, 0x19];
    /// `uid`, which RFC 4514 §3 lists among the types with a short name.
    AT_USER_ID = "0.9.2342.19200300.100.1.1",
        [0x09, 0x92, 0x26, 0x89, 0x93, 0xF2, 0x2C, 0x64, 0x01, 0x01];

    // ---- Certificate extensions (RFC 5280 §4.2) ----

    CE_SUBJECT_KEY_IDENTIFIER = "2.5.29.14", [0x55, 0x1D, 0x0E];
    CE_KEY_USAGE = "2.5.29.15", [0x55, 0x1D, 0x0F];
    CE_SUBJECT_ALT_NAME = "2.5.29.17", [0x55, 0x1D, 0x11];
    CE_ISSUER_ALT_NAME = "2.5.29.18", [0x55, 0x1D, 0x12];
    CE_BASIC_CONSTRAINTS = "2.5.29.19", [0x55, 0x1D, 0x13];
    CE_NAME_CONSTRAINTS = "2.5.29.30", [0x55, 0x1D, 0x1E];
    CE_CRL_DISTRIBUTION_POINTS = "2.5.29.31", [0x55, 0x1D, 0x1F];
    CE_CERTIFICATE_POLICIES = "2.5.29.32", [0x55, 0x1D, 0x20];
    CE_AUTHORITY_KEY_IDENTIFIER = "2.5.29.35", [0x55, 0x1D, 0x23];
    CE_EXT_KEY_USAGE = "2.5.29.37", [0x55, 0x1D, 0x25];
    /// `id-pe-authorityInfoAccess`, where OCSP and CA-issuer URLs live.
    PE_AUTHORITY_INFO_ACCESS = "1.3.6.1.5.5.7.1.1",
        [0x2B, 0x06, 0x01, 0x05, 0x05, 0x07, 0x01, 0x01];

    // ---- Extended key usage purposes (RFC 5280 §4.2.1.12) ----

    /// `anyExtendedKeyUsage`: the certificate declines to restrict itself.
    KP_ANY = "2.5.29.37.0", [0x55, 0x1D, 0x25, 0x00];
    KP_SERVER_AUTH = "1.3.6.1.5.5.7.3.1",
        [0x2B, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x01];
    KP_CLIENT_AUTH = "1.3.6.1.5.5.7.3.2",
        [0x2B, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x02];
    KP_CODE_SIGNING = "1.3.6.1.5.5.7.3.3",
        [0x2B, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x03];
    KP_EMAIL_PROTECTION = "1.3.6.1.5.5.7.3.4",
        [0x2B, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x04];
    /// The purpose a signature timestamp's authority certificate carries.
    KP_TIME_STAMPING = "1.3.6.1.5.5.7.3.8",
        [0x2B, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x08];
    KP_OCSP_SIGNING = "1.3.6.1.5.5.7.3.9",
        [0x2B, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x09];
}

/// The nine attribute types RFC 4514 §3 tabulates a short name for.
///
/// Nine and no more. `E` for `emailAddress` and `SERIALNUMBER` are
/// conventions rather than registrations, and inventing a short name puts a
/// string into a distinguished name that no other implementation produces.
const SHORT_NAMES: &[(Oid<'static>, &str)] = &[
    (AT_COMMON_NAME, "CN"),
    (AT_LOCALITY, "L"),
    (AT_STATE_OR_PROVINCE, "ST"),
    (AT_ORGANISATION, "O"),
    (AT_ORGANISATIONAL_UNIT, "OU"),
    (AT_COUNTRY, "C"),
    (AT_STREET_ADDRESS, "STREET"),
    (AT_DOMAIN_COMPONENT, "DC"),
    (AT_USER_ID, "UID"),
];

/// The short name RFC 4514 §3 gives an attribute type, where it gives one.
///
/// `None` is not a failure: RFC 4514 says an unrecognised type is written as
/// its dotted decimal, which is what [`crate::name`] does with the answer.
#[must_use]
pub fn attribute_short_name(oid: Oid<'_>) -> Option<&'static str> {
    SHORT_NAMES
        .iter()
        .find(|(known, _)| known.as_bytes() == oid.as_bytes())
        .map(|(_, name)| *name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// The table is data written by hand, so it is checked like data.
    ///
    /// Two independent statements of every entry — the octets and the dotted
    /// arcs — have to agree through a decoder that was written without
    /// looking at either. A typo in one of them fails here; a typo in both
    /// the same way is the gap this cannot close, and it is why the arcs are
    /// written beside the bytes rather than derived from them.
    #[test]
    fn every_constant_is_a_well_formed_oid() {
        for (name, oid, dotted) in TABLE {
            let parsed = Oid::parse(oid.as_bytes())
                .unwrap_or_else(|error| panic!("{name} is not a legal OID: {error}"));
            assert_eq!(parsed.to_dotted(), *dotted, "{name}");
        }
    }

    #[test]
    fn no_two_constants_encode_the_same_oid() {
        // A duplicate would make one of the two names unreachable through a
        // match, and a match on the unreachable one would silently never fire.
        let mut seen = BTreeSet::new();
        for (name, oid, _) in TABLE {
            assert!(seen.insert(oid.as_bytes()), "{name} duplicates an entry");
        }
    }

    #[test]
    fn short_names_cover_rfc_4514_and_stop_there() {
        assert_eq!(attribute_short_name(AT_COMMON_NAME), Some("CN"));
        assert_eq!(attribute_short_name(AT_DOMAIN_COMPONENT), Some("DC"));
        assert_eq!(attribute_short_name(AT_ORGANISATIONAL_UNIT), Some("OU"));
        // Real, common, and not in §3's table: the dotted form is the honest
        // answer rather than an invented abbreviation.
        assert_eq!(attribute_short_name(AT_EMAIL_ADDRESS), None);
        assert_eq!(attribute_short_name(AT_SERIAL_NUMBER), None);
    }
}
