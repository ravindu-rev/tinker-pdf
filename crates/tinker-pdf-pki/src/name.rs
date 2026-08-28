//! Distinguished names: what one is made of, and when two of them are the
//! same name.
//!
//! Chain building is a series of questions of exactly one shape — *is this
//! certificate's issuer that certificate's subject?* — so name comparison is
//! not a display concern here, it is the thing the chain is built out of. Get
//! it too strict and a valid chain does not link; get it too loose and a
//! certificate is accepted under a name its issuer never granted. Both are
//! failures of the same function.
//!
//! # What [`Name::matches`] does
//!
//! Two tiers, tried in order.
//!
//! 1. **Byte equality of the encodings.** Almost every real pair that should
//!    match, matches here: an issuer field is very often the literal bytes
//!    copied out of the issuer's subject field, and DER gives one value one
//!    encoding, so equal names are equal bytes far more often than not.
//! 2. **Position-by-position comparison.** Same number of relative
//!    distinguished names, in the same order; same number of attributes in
//!    each, in the same order; equal attribute-type OIDs; and values compared
//!    as text where both are text — case-folded, with runs of whitespace
//!    collapsed to a single space and leading and trailing whitespace
//!    dropped — and as exact encodings otherwise.
//!
//! Tier 2 is what makes a `PrintableString` subject match a `UTF8String`
//! issuer spelling the same name, which RFC 5280 §7.1 requires and which
//! reissued intermediates produce constantly.
//!
//! # What it does **not** do, stated plainly
//!
//! RFC 5280 §7.1 defers to LDAP's string preparation (RFC 4518) for the full
//! rule. This implements the part above and none of the rest, so:
//!
//! - **No Unicode normalisation.** RFC 4518 §2.2 maps to NFKC; a name written
//!   with a precomposed `é` will not match the same name written with `e` and
//!   a combining acute. Both are valid encodings of the same name and this
//!   says they differ.
//! - **No RFC 4518 mapping or prohibition steps.** Zero-width joiners, soft
//!   hyphens and the other characters §2.2 deletes are kept; non-ASCII
//!   whitespace other than what `char::is_whitespace` reports is not folded to
//!   a space; nothing is checked against §2.3's prohibited set.
//! - **No per-attribute matching rules.** X.520 gives each attribute type an
//!   equality rule, and a handful are case-*sensitive*. Everything textual is
//!   case-folded here, so this is looser than X.520 for those types, and the
//!   direction of the looseness is recorded rather than argued away.
//! - **No reordering inside a multi-valued relative distinguished name.** An
//!   RDN is a SET, so its members are semantically unordered; DER requires a
//!   SET OF to be sorted by encoding (X.690 §11.6), which makes the encoded
//!   order canonical for any conforming issuer. A pair that disagrees about
//!   the order is not matched, because reordering to compare would mean
//!   accepting an encoding DER forbids.
//! - **No `subjectAltName`.** Matching a name to a host, a mailbox or a URI is
//!   a different question with a different answer, and nothing in this crate
//!   asks it.
//! - **No name constraints** (RFC 5280 §4.2.1.10). Subtree containment is not
//!   name equality and is not attempted.
//!
//! The consequence worth naming: this can say two names differ when a fuller
//! implementation would say they match. That direction fails a chain that
//! should have linked, which surfaces as `Incomplete` in a verdict — visible,
//! and never as an acceptance.

use crate::der::{Budget, Cursor, DerError, Oid, Tag, Tlv};
use crate::oid;

/// One `AttributeTypeAndValue` (RFC 5280 §4.1.2.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attribute<'a> {
    oid: Oid<'a>,
    value: Tlv<'a>,
    text: AttributeText,
}

/// What an attribute's value turned out to be made of.
///
/// Three states rather than an `Option<String>`, because "this is not text"
/// and "this claims to be text and would not decode" are different facts and
/// a caller reporting on a certificate needs to be able to tell them apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttributeText {
    /// Decoded from one of the string types.
    Decoded(String),
    /// A string type whose octets this crate would not read, with the reason.
    Refused(DerError),
    /// Not a string type at all. The schema allows it; nothing here reads it.
    NotAString,
}

impl<'a> Attribute<'a> {
    /// The attribute type.
    #[must_use]
    pub const fn oid(&self) -> Oid<'a> {
        self.oid
    }

    /// The value exactly as encoded, which is what an exact match compares.
    #[must_use]
    pub const fn value(&self) -> Tlv<'a> {
        self.value
    }

    /// What the value decoded to, or why it did not.
    #[must_use]
    pub const fn text(&self) -> &AttributeText {
        &self.text
    }

    /// The decoded text, where there is some.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match &self.text {
            AttributeText::Decoded(text) => Some(text.as_str()),
            AttributeText::Refused(_) | AttributeText::NotAString => None,
        }
    }
}

/// One `RelativeDistinguishedName`: a SET of one or more attributes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rdn<'a> {
    attributes: Vec<Attribute<'a>>,
}

impl<'a> Rdn<'a> {
    /// The attributes, in encoded order.
    #[must_use]
    pub fn attributes(&self) -> &[Attribute<'a>] {
        &self.attributes
    }
}

/// A `Name`, which RFC 5280 §4.1.2.4 makes a `RDNSequence`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Name<'a> {
    der: &'a [u8],
    rdns: Vec<Rdn<'a>>,
}

impl<'a> Name<'a> {
    /// Reads a `Name` from the node holding it.
    pub fn parse(tlv: &Tlv<'a>, budget: &Budget) -> Result<Self, DerError> {
        tlv.require(Tag::Sequence)?;
        let mut rdns = Vec::new();
        let mut sequence = tlv.children(budget)?;
        while !sequence.is_empty() {
            let set = sequence.expect(Tag::Set)?;
            let mut members = set.children(budget)?;
            let mut attributes = Vec::new();
            while !members.is_empty() {
                let pair = members.expect(Tag::Sequence)?;
                let mut fields = pair.children(budget)?;
                let oid = fields.expect(Tag::Oid)?.as_oid()?;
                let value = fields.read()?;
                fields.finish()?;
                let text = if value.is_string() {
                    match value.as_string() {
                        Ok(text) => AttributeText::Decoded(text),
                        Err(error) => AttributeText::Refused(error),
                    }
                } else {
                    AttributeText::NotAString
                };
                attributes.push(Attribute { oid, value, text });
            }
            if attributes.is_empty() {
                // `SIZE (1..MAX)`: a relative distinguished name that
                // distinguishes nothing.
                return Err(DerError::UnexpectedEnd);
            }
            rdns.push(Rdn { attributes });
        }
        Ok(Self {
            der: tlv.raw(),
            rdns,
        })
    }

    /// The name's complete encoding, header included.
    ///
    /// This is the value a chain builder should key on wherever it can: DER
    /// gives one name one encoding, so equal bytes are conclusive and cost a
    /// comparison rather than a walk.
    #[must_use]
    pub const fn der(&self) -> &'a [u8] {
        self.der
    }

    /// The relative distinguished names, in encoded order — least specific
    /// first, which is the opposite of how RFC 4514 writes them out.
    #[must_use]
    pub fn rdns(&self) -> &[Rdn<'a>] {
        &self.rdns
    }

    /// Whether the name has no relative distinguished names at all.
    ///
    /// Legal, and meaningful: RFC 5280 §4.1.2.6 allows an empty subject on a
    /// certificate that identifies itself through a critical
    /// `subjectAltName`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rdns.is_empty()
    }

    /// The last value of an attribute type, or nothing.
    ///
    /// The *last*, because a `RDNSequence` runs from least specific to most,
    /// so the last `CN` is the one that names this entity rather than one of
    /// its containers.
    #[must_use]
    pub fn attribute(&self, wanted: Oid<'_>) -> Option<&str> {
        self.rdns
            .iter()
            .rev()
            .flat_map(|rdn| rdn.attributes.iter())
            .find(|attribute| attribute.oid.as_bytes() == wanted.as_bytes())
            .and_then(Attribute::as_str)
    }

    /// The most specific `commonName`.
    #[must_use]
    pub fn common_name(&self) -> Option<&str> {
        self.attribute(oid::AT_COMMON_NAME)
    }

    /// Whether these two are the same name, at the level this module's
    /// header describes.
    #[must_use]
    pub fn matches(&self, other: &Name<'_>) -> bool {
        if self.der == other.der {
            return true;
        }
        if self.rdns.len() != other.rdns.len() {
            return false;
        }
        self.rdns
            .iter()
            .zip(other.rdns.iter())
            .all(|(left, right)| rdn_matches(left, right))
    }

    /// The name written the way RFC 4514 §2 writes one.
    ///
    /// Relative distinguished names most specific first, joined by commas;
    /// several attributes in one of them joined by `+`; a type with a short
    /// name in §3's table written by that name, and every other type written
    /// as dotted decimal with its value as `#` and the hex of the value's
    /// complete encoding, which is what §2.4 requires rather than a
    /// convenience.
    ///
    /// For reading and reporting. It is **not** a comparison key: two names
    /// that [`Name::matches`] calls equal can print differently, and the
    /// hex form makes the printed string depend on the encoding rather than
    /// on the name.
    #[must_use]
    pub fn to_rfc4514(&self) -> String {
        let mut out = String::new();
        for rdn in self.rdns.iter().rev() {
            if !out.is_empty() {
                out.push(',');
            }
            let mut first = true;
            for attribute in &rdn.attributes {
                if !first {
                    out.push('+');
                }
                first = false;
                match (oid::attribute_short_name(attribute.oid), attribute.as_str()) {
                    (Some(short), Some(text)) => {
                        out.push_str(short);
                        out.push('=');
                        push_escaped(&mut out, text);
                    }
                    _ => {
                        out.push_str(&attribute.oid.to_dotted());
                        out.push_str("=#");
                        for byte in attribute.value.raw() {
                            out.push(hex_digit(byte >> 4));
                            out.push(hex_digit(byte & 0x0F));
                        }
                    }
                }
            }
        }
        out
    }
}

/// Whether two relative distinguished names carry the same attributes.
fn rdn_matches(left: &Rdn<'_>, right: &Rdn<'_>) -> bool {
    if left.attributes.len() != right.attributes.len() {
        return false;
    }
    left.attributes
        .iter()
        .zip(right.attributes.iter())
        .all(|(a, b)| {
            if a.oid.as_bytes() != b.oid.as_bytes() {
                return false;
            }
            match (&a.text, &b.text) {
                (AttributeText::Decoded(x), AttributeText::Decoded(y)) => fold(x) == fold(y),
                // One of them is not text this crate reads, so the only
                // honest comparison left is of the encodings themselves.
                _ => a.value.raw() == b.value.raw(),
            }
        })
}

/// Case-folds and collapses whitespace, the two steps of RFC 4518 this
/// implements.
fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    for character in text.chars().flat_map(char::to_lowercase) {
        if character.is_whitespace() {
            pending_space = true;
            continue;
        }
        if pending_space && !out.is_empty() {
            out.push(' ');
        }
        pending_space = false;
        out.push(character);
    }
    out
}

/// RFC 4514 §2.4's escaping.
fn push_escaped(out: &mut String, text: &str) {
    let count = text.chars().count();
    for (index, character) in text.chars().enumerate() {
        let leading = index == 0;
        let trailing = index + 1 == count;
        match character {
            // A NUL has no printable escape, so §2.4's hex-pair form is the
            // only one available for it.
            '\0' => out.push_str("\\00"),
            '"' | '+' | ',' | ';' | '<' | '>' | '\\' => {
                out.push('\\');
                out.push(character);
            }
            // Only at an end, where an unescaped one would be swallowed by a
            // reader trimming around the separators.
            ' ' if leading || trailing => out.push_str("\\ "),
            '#' if leading => out.push_str("\\#"),
            _ => out.push(character),
        }
    }
}

/// One hex digit, upper case, which is the case RFC 4514's examples use.
fn hex_digit(value: u8) -> char {
    match value {
        0..=9 => char::from(b'0'.wrapping_add(value)),
        _ => char::from(b'A'.wrapping_add(value.wrapping_sub(10))),
    }
}

/// Reads a `Name` from a cursor's next node.
pub(crate) fn read<'a>(cursor: &mut Cursor<'a, '_>, budget: &Budget) -> Result<Name<'a>, DerError> {
    let tlv = cursor.expect(Tag::Sequence)?;
    Name::parse(&tlv, budget)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::der::tests::unhex;

    /// Parses a standalone `Name` encoding.
    fn name(der: &[u8]) -> Name<'_> {
        let budget = Budget::default();
        let mut cursor = Cursor::new(der, &budget);
        let name = read(&mut cursor, &budget).expect("a Name");
        cursor.finish().expect("nothing trailing");
        name
    }

    /// `dc=com, dc=example, cn=Example CA` — RFC 5280 Appendix C.1's issuer,
    /// which is also its subject.
    fn c1_issuer() -> Vec<u8> {
        unhex(
            "30 43
             31 13 30 11 06 0A 09 92 26 89 93 F2 2C 64 01 19 16 03 63 6F 6D
             31 17 30 15 06 0A 09 92 26 89 93 F2 2C 64 01 19 16 07 65 78 61 6D 70 6C 65
             31 13 30 11 06 03 55 04 03 13 0A 45 78 61 6D 70 6C 65 20 43 41",
        )
    }

    #[test]
    fn the_appendix_c1_name_reads_as_its_three_rdns() {
        let der = c1_issuer();
        let name = name(&der);
        assert_eq!(name.rdns().len(), 3);
        assert_eq!(name.common_name(), Some("Example CA"));
        assert_eq!(name.attribute(oid::AT_DOMAIN_COMPONENT), Some("example"));
        assert_eq!(name.der(), &der[..]);
    }

    #[test]
    fn rfc_4514_writes_the_rdns_most_specific_first() {
        // RFC 5280 Appendix C.1 states this name as
        // `cn=Example CA,dc=example,dc=com`. The descriptors are
        // case-insensitive (RFC 4514 §3) and §3's own table spells them in
        // upper case, which is what this produces.
        let der = c1_issuer();
        assert_eq!(name(&der).to_rfc4514(), "CN=Example CA,DC=example,DC=com");
    }

    #[test]
    fn a_type_with_no_short_name_is_written_as_dotted_decimal_and_hex() {
        // `emailAddress=a@b.example` — real, common, and not in RFC 4514 §3's
        // table, so §2.4 says dotted decimal and a hex value.
        let der = unhex(
            "30 20 31 1E 30 1C
             06 09 2A 86 48 86 F7 0D 01 09 01
             16 0F 61 40 62 2E 65 78 61 6D 70 6C 65 2E 63 6F 6D",
        );
        assert_eq!(
            name(&der).to_rfc4514(),
            "1.2.840.113549.1.9.1=#160F6140622E6578616D706C652E636F6D"
        );
    }

    #[test]
    fn rfc_4514_special_characters_are_escaped() {
        // `CN=` with a value that needs every rule: a leading space, an
        // embedded comma and plus, a backslash, and a trailing space.
        let value = " a,b+c\\d ";
        let mut der = vec![
            0x30, 0x00, 0x31, 0x00, 0x30, 0x00, 0x06, 0x03, 0x55, 0x04, 0x03,
        ];
        der.push(0x0C);
        der.push(value.len() as u8);
        der.extend_from_slice(value.as_bytes());
        // Patch the three lengths now that the value's width is known.
        let value_len = 2 + value.len();
        der[5] = (5 + value_len) as u8;
        der[3] = (2 + 5 + value_len) as u8;
        der[1] = (2 + 2 + 5 + value_len) as u8;
        assert_eq!(name(&der).to_rfc4514(), "CN=\\ a\\,b\\+c\\\\d\\ ");
    }

    #[test]
    fn identical_encodings_match_without_walking_them() {
        let der = c1_issuer();
        let a = name(&der);
        let b = name(&der);
        assert!(a.matches(&b));
        assert!(a.matches(&a));
    }

    #[test]
    fn a_printable_string_matches_the_same_name_as_utf8() {
        // The same three RDNs, with the commonName re-encoded as a
        // UTF8String. Different bytes, one name — RFC 5280 §7.1's whole
        // point, and the case a reissued intermediate produces.
        let printable = c1_issuer();
        let mut utf8 = printable.clone();
        let at = utf8.len() - 12;
        assert_eq!(utf8[at], 0x13, "the commonName's PrintableString tag");
        utf8[at] = 0x0C;
        assert_ne!(printable, utf8);
        assert!(name(&printable).matches(&name(&utf8)));
    }

    #[test]
    fn case_and_inner_whitespace_do_not_distinguish_two_names() {
        let build = |cn: &str| {
            let mut der = vec![
                0x30, 0x00, 0x31, 0x00, 0x30, 0x00, 0x06, 0x03, 0x55, 0x04, 0x03, 0x13,
            ];
            der.push(cn.len() as u8);
            der.extend_from_slice(cn.as_bytes());
            let value_len = 2 + cn.len();
            der[5] = (5 + value_len) as u8;
            der[3] = (2 + 5 + value_len) as u8;
            der[1] = (2 + 2 + 5 + value_len) as u8;
            der
        };
        let plain = build("Example CA");
        let shouty = build("  EXAMPLE    ca  ");
        assert!(name(&plain).matches(&name(&shouty)));

        // And a different name is still a different name.
        let other = build("Example CB");
        assert!(!name(&plain).matches(&name(&other)));
    }

    #[test]
    fn a_different_rdn_count_never_matches() {
        let three = c1_issuer();
        // The same name minus its leading `dc=com`.
        let two = unhex(
            "30 2E
             31 17 30 15 06 0A 09 92 26 89 93 F2 2C 64 01 19 16 07 65 78 61 6D 70 6C 65
             31 13 30 11 06 03 55 04 03 13 0A 45 78 61 6D 70 6C 65 20 43 41",
        );
        assert!(!name(&three).matches(&name(&two)));
    }

    #[test]
    fn an_empty_rdn_is_refused_rather_than_skipped() {
        // `SEQUENCE { SET {} }`: a relative distinguished name of size zero,
        // which RFC 5280's `SIZE (1..MAX)` forbids.
        let der = unhex("30 02 31 00");
        let budget = Budget::default();
        let mut cursor = Cursor::new(&der, &budget);
        assert_eq!(read(&mut cursor, &budget), Err(DerError::UnexpectedEnd));
    }

    #[test]
    fn an_empty_name_is_legal_and_says_so() {
        let der = unhex("30 00");
        let name = name(&der);
        assert!(name.is_empty());
        assert_eq!(name.to_rfc4514(), "");
        assert_eq!(name.common_name(), None);
    }

    #[test]
    fn a_value_that_is_not_text_keeps_its_encoding_and_says_why() {
        // `CN` whose value is a NULL: legal DER, not a string type.
        let der = unhex("30 0B 31 09 30 07 06 03 55 04 03 05 00");
        let parsed = name(&der);
        let attribute = &parsed.rdns()[0].attributes()[0];
        assert_eq!(attribute.text(), &AttributeText::NotAString);
        assert_eq!(attribute.as_str(), None);
        assert_eq!(parsed.to_rfc4514(), "2.5.4.3=#0500");

        // And a string type whose octets will not decode names the reason
        // rather than going quiet: a PrintableString with a byte above 0x7F.
        let der = unhex("30 0D 31 0B 30 09 06 03 55 04 03 13 02 41 80");
        let parsed = name(&der);
        let attribute = &parsed.rdns()[0].attributes()[0];
        assert_eq!(
            attribute.text(),
            &AttributeText::Refused(DerError::NonAsciiString { tag: 19 })
        );
    }
}
