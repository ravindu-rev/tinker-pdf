//! `GeneralName` and `GeneralNames` (RFC 5280 §4.2.1.6): the nine-way choice
//! a certificate, an ESS attribute or a timestamp uses wherever "a name" can be
//! something other than a distinguished name.
//!
//! ```text
//! GeneralNames ::= SEQUENCE SIZE (1..MAX) OF GeneralName
//!
//! GeneralName ::= CHOICE {
//!      otherName                 [0]  OtherName,
//!      rfc822Name                [1]  IA5String,
//!      dNSName                   [2]  IA5String,
//!      x400Address               [3]  ORAddress,
//!      directoryName             [4]  Name,
//!      ediPartyName              [5]  EDIPartyName,
//!      uniformResourceIdentifier [6]  IA5String,
//!      iPAddress                 [7]  OCTET STRING,
//!      registeredID              [8]  OBJECT IDENTIFIER }
//! ```
//!
//! RFC 5280's module is `IMPLICIT TAGS`, so every alternative's context tag
//! *replaces* its type's own — **except `directoryName`**, because `Name` is
//! itself a `CHOICE` and X.680 §31.2.7 makes a tag on a `CHOICE` explicit
//! whatever the module says. That one exception is the whole of what is easy
//! to get wrong here: read `[4]` implicitly and a directory name's first RDN
//! is taken for the whole name.
//!
//! # Where this crate meets one
//!
//! Three places carried the encoding undecoded until this module existed:
//! `authorityKeyIdentifier`'s `authorityCertIssuer`
//! ([`crate::x509::AuthorityKeyIdentifier::issuer`]), an ESS `IssuerSerial`
//! ([`crate::cms::EssCertId::issuer_serial_decoded`]), and the
//! `subjectAltName` and `issuerAltName` extensions
//! ([`crate::x509::Extensions::subject_alt_names`]). A timestamp's `tsa`
//! field is the fourth.
//!
//! # What is decoded, and what is carried
//!
//! The six alternatives with a type this crate reads are decoded: the three
//! IA5 strings, held to ASCII; the address, held to the four or sixteen octets
//! RFC 5280 §4.2.1.6 gives an address outside a name constraint; the OID; and
//! the directory name, through [`crate::name::Name`]. `otherName` is decoded as
//! far as its type OID and its value's encoding — what the value *is* depends
//! on the OID, and no OID here has a decoder behind it. `x400Address` and
//! `ediPartyName` are carried as their encodings: both are structures no PDF
//! signature has been seen to use, and reading them would be two parsers with
//! no data to hold them to.

use crate::der::{Budget, Class, Cursor, DerError, Limits, Oid, Tag, Tlv};
use crate::name::Name;

/// One `GeneralName`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GeneralName<'a> {
    /// `[0] otherName`: a type OID and a value whose syntax the OID defines,
    /// carried as the value's complete encoding.
    Other {
        /// `type-id`.
        type_id: Oid<'a>,
        /// The `[0] EXPLICIT` value's inner encoding.
        value: &'a [u8],
    },
    /// `[1] rfc822Name`: a mailbox.
    Rfc822(String),
    /// `[2] dNSName`.
    Dns(String),
    /// `[3] x400Address`, carried as its encoding.
    X400Address(&'a [u8]),
    /// `[4] directoryName`.
    Directory(Name<'a>),
    /// `[5] ediPartyName`, carried as its encoding.
    EdiParty(&'a [u8]),
    /// `[6] uniformResourceIdentifier`.
    Uri(String),
    /// `[7] iPAddress`: four octets for IPv4, sixteen for IPv6, network
    /// order.
    IpAddress(&'a [u8]),
    /// `[8] registeredID`.
    RegisteredId(Oid<'a>),
}

/// Why a `GeneralName` or `GeneralNames` could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GeneralNameError {
    /// The encoding itself.
    Der(DerError),
    /// A `GeneralNames` with no names in it, which `SIZE (1..MAX)` forbids.
    Empty,
    /// A node that is none of the nine alternatives: a context tag past
    /// `[8]`, or a node outside the context class.
    UnknownChoice {
        /// The node's class.
        class: Class,
        /// Its tag number.
        tag: u32,
    },
    /// An `iPAddress` that is neither four nor sixteen octets. Eight and
    /// thirty-two are an address *and mask*, which RFC 5280 §4.2.1.10 uses
    /// inside a name constraint and nowhere else.
    IpAddressLength {
        /// How many octets it held.
        octets: usize,
    },
}

impl From<DerError> for GeneralNameError {
    fn from(error: DerError) -> Self {
        Self::Der(error)
    }
}

impl core::fmt::Display for GeneralNameError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Der(error) => write!(f, "{error}"),
            Self::Empty => write!(f, "a GeneralNames with no names"),
            Self::UnknownChoice { class, tag } => {
                write!(f, "a GeneralName alternative {class:?} [{tag}]")
            }
            Self::IpAddressLength { octets } => {
                write!(f, "an iPAddress of {octets} octets, not 4 or 16")
            }
        }
    }
}

impl std::error::Error for GeneralNameError {}

/// Ceilings for a `GeneralNames` read on its own, outside the parse that
/// located it: a directory name is four levels under the alternative, and an
/// `otherName` value is not descended into.
const LIMITS: Limits = Limits::new(16, 4096);

impl<'a> GeneralName<'a> {
    /// Reads one alternative from the node carrying it.
    ///
    /// # Errors
    ///
    /// [`GeneralNameError`].
    pub fn parse(node: &Tlv<'a>, budget: &Budget) -> Result<Self, GeneralNameError> {
        if node.class() != Class::ContextSpecific {
            return Err(GeneralNameError::UnknownChoice {
                class: node.class(),
                tag: node.tag(),
            });
        }
        Ok(match node.tag() {
            0 => {
                // `OtherName ::= SEQUENCE { type-id OBJECT IDENTIFIER,
                // value [0] EXPLICIT ANY DEFINED BY type-id }`, implicitly
                // tagged.
                let sequence = node.implicit(Tag::Sequence);
                sequence.require(Tag::Sequence)?;
                let mut fields = sequence.children(budget)?;
                let type_id = fields.expect(Tag::Oid)?.as_oid()?;
                let value = fields
                    .context_optional(0)?
                    .ok_or(DerError::UnexpectedEnd)?
                    .explicit(budget)?;
                fields.finish()?;
                GeneralName::Other {
                    type_id,
                    value: value.raw(),
                }
            }
            1 => GeneralName::Rfc822(node.implicit(Tag::Ia5String).as_string()?),
            2 => GeneralName::Dns(node.implicit(Tag::Ia5String).as_string()?),
            3 => GeneralName::X400Address(constructed(node)?),
            // X.680 §31.2.7: a tag on a CHOICE is explicit, so the `Name` is
            // a node *inside* `[4]`, not `[4]` wearing the `Name`'s content.
            4 => GeneralName::Directory(Name::parse(&node.explicit(budget)?, budget)?),
            5 => GeneralName::EdiParty(constructed(node)?),
            6 => GeneralName::Uri(node.implicit(Tag::Ia5String).as_string()?),
            7 => {
                let octets = node.implicit(Tag::OctetString).as_octet_string()?;
                if octets.len() != 4 && octets.len() != 16 {
                    return Err(GeneralNameError::IpAddressLength {
                        octets: octets.len(),
                    });
                }
                GeneralName::IpAddress(octets)
            }
            8 => GeneralName::RegisteredId(node.implicit(Tag::Oid).as_oid()?),
            tag => {
                return Err(GeneralNameError::UnknownChoice {
                    class: node.class(),
                    tag,
                })
            }
        })
    }

    /// Reads exactly one `GeneralName` from `der` — the shape a timestamp's
    /// `tsa` field holds inside its explicit tag.
    ///
    /// # Errors
    ///
    /// [`GeneralNameError`], including trailing bytes.
    pub fn parse_one(der: &'a [u8]) -> Result<Self, GeneralNameError> {
        let budget = Budget::new(LIMITS);
        let mut cursor = Cursor::new(der, &budget);
        let node = cursor.read()?;
        cursor.finish()?;
        Self::parse(&node, &budget)
    }
}

/// The encoding of an alternative carried rather than decoded, which must at
/// least be the constructed form its type has.
fn constructed<'a>(node: &Tlv<'a>) -> Result<&'a [u8], GeneralNameError> {
    if !node.is_constructed() {
        return Err(DerError::WrongForm {
            tag: node.tag(),
            constructed: false,
        }
        .into());
    }
    Ok(node.raw())
}

impl core::fmt::Display for GeneralName<'_> {
    /// The prefixes are OpenSSL's `x509 -text` spellings, so a name printed
    /// here reads the way a person comparing it with a certificate dump
    /// expects; the address is written out in full, never compressed.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Other { type_id, .. } => write!(f, "othername:{}", type_id.to_dotted()),
            Self::Rfc822(mailbox) => write!(f, "email:{mailbox}"),
            Self::Dns(name) => write!(f, "DNS:{name}"),
            Self::X400Address(der) => write!(f, "X400Name:<{} octets>", der.len()),
            Self::Directory(name) => write!(f, "DirName:{}", name.to_rfc4514()),
            Self::EdiParty(der) => write!(f, "EdiPartyName:<{} octets>", der.len()),
            Self::Uri(uri) => write!(f, "URI:{uri}"),
            Self::IpAddress(octets) => {
                f.write_str("IP:")?;
                if let [a, b, c, d] = octets {
                    return write!(f, "{a}.{b}.{c}.{d}");
                }
                for (index, pair) in octets.chunks(2).enumerate() {
                    if index > 0 {
                        f.write_str(":")?;
                    }
                    let high = pair.first().copied().unwrap_or(0);
                    let low = pair.get(1).copied().unwrap_or(0);
                    write!(f, "{:x}", u16::from_be_bytes([high, low]))?;
                }
                Ok(())
            }
            Self::RegisteredId(oid) => write!(f, "RID:{}", oid.to_dotted()),
        }
    }
}

/// A `GeneralNames`: one or more names, in encoded order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneralNames<'a> {
    names: Vec<GeneralName<'a>>,
}

impl<'a> GeneralNames<'a> {
    /// Reads a `GeneralNames` SEQUENCE — a `subjectAltName`'s `extnValue`,
    /// or the `issuer` of an ESS `IssuerSerial`.
    ///
    /// # Errors
    ///
    /// [`GeneralNameError`], including trailing bytes after the SEQUENCE.
    pub fn parse(der: &'a [u8]) -> Result<Self, GeneralNameError> {
        let budget = Budget::new(LIMITS);
        let mut cursor = Cursor::new(der, &budget);
        let sequence = cursor.expect(Tag::Sequence)?;
        cursor.finish()?;
        Self::from_node(&sequence, &budget)
    }

    /// Reads the *content* of a `GeneralNames` whose own tag was replaced by
    /// an implicit one — `authorityCertIssuer [1] IMPLICIT GeneralNames`.
    ///
    /// # Errors
    ///
    /// [`GeneralNameError`].
    pub fn from_content(content: &'a [u8]) -> Result<Self, GeneralNameError> {
        let budget = Budget::new(LIMITS);
        Self::read_all(Cursor::new(content, &budget), &budget)
    }

    /// Reads a `GeneralNames` from the SEQUENCE node holding it.
    ///
    /// # Errors
    ///
    /// [`GeneralNameError`].
    pub fn from_node(sequence: &Tlv<'a>, budget: &Budget) -> Result<Self, GeneralNameError> {
        sequence.require(Tag::Sequence)?;
        Self::read_all(sequence.children(budget)?, budget)
    }

    fn read_all(mut cursor: Cursor<'a, '_>, budget: &Budget) -> Result<Self, GeneralNameError> {
        let mut names = Vec::new();
        while !cursor.is_empty() {
            names.push(GeneralName::parse(&cursor.read()?, budget)?);
        }
        if names.is_empty() {
            return Err(GeneralNameError::Empty);
        }
        Ok(Self { names })
    }

    /// The names, in encoded order.
    #[must_use]
    pub fn names(&self) -> &[GeneralName<'a>] {
        &self.names
    }

    /// The first directory name, which is what an `IssuerSerial` or an
    /// `authorityCertIssuer` names an issuer by in practice.
    #[must_use]
    pub fn directory_name(&self) -> Option<&Name<'a>> {
        self.names.iter().find_map(|name| match name {
            GeneralName::Directory(directory) => Some(directory),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unhex(text: &str) -> Vec<u8> {
        let digits: String = text.chars().filter(char::is_ascii_hexdigit).collect();
        digits
            .as_bytes()
            .chunks_exact(2)
            .filter_map(|pair| u8::from_str_radix(core::str::from_utf8(pair).ok()?, 16).ok())
            .collect()
    }

    /// RFC 5280 Appendix C.2's `subjectAltName`, octet for octet: "one
    /// alternative name — an electronic mail address".
    #[test]
    fn rfc_5280_c_2s_mailbox_is_read() {
        let der = unhex(
            "30 18 81 16 65 6E 64 2E 65 6E 74 69 74 79 40
             65 78 61 6D 70 6C 65 2E 63 6F 6D",
        );
        let names = GeneralNames::parse(&der).expect("it parses");
        assert_eq!(
            names.names(),
            [GeneralName::Rfc822("end.entity@example.com".into())]
        );
        assert_eq!(names.names()[0].to_string(), "email:end.entity@example.com");
    }

    /// A definite-length node, for building inputs without counting octets
    /// by hand.
    fn tlv(tag: u8, content: &[u8]) -> Vec<u8> {
        assert!(content.len() < 0x80, "short form only");
        let mut out = vec![tag, content.len() as u8];
        out.extend_from_slice(content);
        out
    }

    /// Every alternative this module decodes, in one sequence.
    #[test]
    fn each_decoded_alternative_reads_as_itself() {
        let directory = tlv(
            0x30,
            &tlv(
                0x31,
                &tlv(
                    0x30,
                    &[&unhex("06 03 55 04 03")[..], &tlv(0x0C, b"Root")].concat(),
                ),
            ),
        );
        let alternatives = [
            tlv(0x82, b"example.com"),
            tlv(0x86, b"https://example.com/"),
            tlv(0x87, &[192, 0, 2, 7]),
            tlv(
                0x87,
                &unhex("20 01 0D B8 00 00 00 00 00 00 00 00 00 00 00 07"),
            ),
            tlv(0x88, &unhex("2A 03 04")),
            tlv(0xA4, &directory),
        ]
        .concat();
        let der = tlv(0x30, &alternatives);
        let names = GeneralNames::parse(&der).expect("it parses");
        let rendered: Vec<String> = names.names().iter().map(ToString::to_string).collect();
        assert_eq!(
            rendered,
            [
                "DNS:example.com",
                "URI:https://example.com/",
                "IP:192.0.2.7",
                "IP:2001:db8:0:0:0:0:0:7",
                "RID:1.2.3.4",
                "DirName:CN=Root",
            ]
        );
        assert_eq!(
            names.directory_name().map(Name::to_rfc4514).as_deref(),
            Some("CN=Root")
        );

        // An `otherName` whose `[0]` is empty holds no value.
        let empty_other = tlv(
            0x30,
            &tlv(
                0xA0,
                &[&unhex("06 03 2A 03 05")[..], &[0xA0, 0x00]].concat(),
            ),
        );
        assert!(GeneralNames::parse(&empty_other).is_err());
    }

    #[test]
    fn an_other_name_keeps_its_type_and_its_value() {
        // 1.3.6.1.4.1.311.20.2.3 (a Microsoft UPN) with a UTF8String value.
        let der = unhex(
            "30 19 A0 17 06 0A 2B 06 01 04 01 82 37 14 02 03
                   A0 09 0C 07 75 40 65 78 2E 63 6F",
        );
        let names = GeneralNames::parse(&der).expect("it parses");
        match &names.names()[0] {
            GeneralName::Other { type_id, value } => {
                assert_eq!(type_id.to_dotted(), "1.3.6.1.4.1.311.20.2.3");
                assert_eq!(*value, &unhex("0C 07 75 40 65 78 2E 63 6F")[..]);
            }
            other => panic!("expected an otherName, got {other:?}"),
        }
    }

    /// `[4]` is explicit. Read implicitly, the first RDN's SET would be taken
    /// for the name's SEQUENCE — and refused, which is what this pins.
    #[test]
    fn a_directory_name_tagged_implicitly_is_refused() {
        let implicit = unhex("30 0F A4 0D 31 0B 30 09 06 03 55 04 03 0C 02 43 41");
        assert!(GeneralNames::parse(&implicit).is_err());
        let explicit = unhex("30 11 A4 0F 30 0D 31 0B 30 09 06 03 55 04 03 0C 02 43 41");
        let names = GeneralNames::parse(&explicit).expect("explicit parses");
        assert_eq!(
            names.directory_name().map(Name::to_rfc4514).as_deref(),
            Some("CN=CA")
        );
    }

    #[test]
    fn each_refusal_is_named() {
        assert_eq!(
            GeneralNames::parse(&[0x30, 0x00]),
            Err(GeneralNameError::Empty)
        );
        assert_eq!(
            GeneralNames::from_content(&[]),
            Err(GeneralNameError::Empty)
        );
        // `[9]`, past the nine.
        assert_eq!(
            GeneralNames::parse(&unhex("30 03 89 01 00")),
            Err(GeneralNameError::UnknownChoice {
                class: Class::ContextSpecific,
                tag: 9
            })
        );
        // A universal IA5String where a context tag belongs.
        assert!(matches!(
            GeneralNames::parse(&unhex("30 03 16 01 41")),
            Err(GeneralNameError::UnknownChoice {
                class: Class::Universal,
                ..
            })
        ));
        // A mailbox with a byte above 0x7F.
        assert!(matches!(
            GeneralNames::parse(&unhex("30 03 81 01 E9")),
            Err(GeneralNameError::Der(DerError::NonAsciiString { .. }))
        ));
        // An address and mask, which belongs in a name constraint.
        assert_eq!(
            GeneralNames::parse(&unhex("30 0A 87 08 C0 00 02 00 FF FF FF 00")),
            Err(GeneralNameError::IpAddressLength { octets: 8 })
        );
        // A primitive x400Address.
        assert!(matches!(
            GeneralNames::parse(&unhex("30 03 83 01 00")),
            Err(GeneralNameError::Der(DerError::WrongForm { .. }))
        ));
        // Trailing bytes after the sequence.
        assert!(GeneralNames::parse(&unhex("30 03 82 01 41 00")).is_err());
    }

    /// The `authorityCertIssuer [1] IMPLICIT GeneralNames` shape: the content
    /// with no SEQUENCE header of its own.
    #[test]
    fn the_content_of_an_implicitly_tagged_sequence_reads() {
        let content = unhex("A4 0F 30 0D 31 0B 30 09 06 03 55 04 03 0C 02 43 41");
        let names = GeneralNames::from_content(&content).expect("it parses");
        assert_eq!(names.names().len(), 1);
        assert!(names.directory_name().is_some());
    }

    #[test]
    fn no_prefix_of_a_real_sequence_reads_and_none_panics() {
        let der = unhex(
            "30 2C 81 16 65 6E 64 2E 65 6E 74 69 74 79 40 65 78 61 6D 70 6C 65 2E 63 6F 6D
                   A4 12 30 10 31 0E 30 0C 06 03 55 04 03 0C 05 53 69 67 6E 65",
        );
        assert!(GeneralNames::parse(&der).is_ok());
        for cut in 0..der.len() {
            assert!(
                GeneralNames::parse(&der[..cut]).is_err(),
                "a {cut}-octet prefix"
            );
        }
        for at in 0..der.len() {
            for bit in 0..8 {
                let mut spoiled = der.clone();
                spoiled[at] ^= 1 << bit;
                let _ = GeneralNames::parse(&spoiled);
                let _ = GeneralNames::from_content(&spoiled);
            }
        }
    }
}
