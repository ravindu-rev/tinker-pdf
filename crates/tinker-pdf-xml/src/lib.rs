//! XML 1.0 with namespaces: bytes in, events out, and no PDF vocabulary
//! anywhere — nor any XPS vocabulary, which is the same rule read twice.
//!
//! Scope, design and exit criteria: `docs/plans/gaps/30-xps.md`, milestone 2.
//!
//! The eighth leaf. It exists because three different parts of an OPC package
//! are XML — the content-types item, every relationships part, and the fixed
//! payload itself — and because there is no XML parser in this repository and
//! CONTRIBUTING rule 1 says there will not be a third-party one. Ruling 8's
//! August 2026 amendment makes the test of a leaf the definition rather than
//! the list: *a leaf is any crate that takes bytes and plain parameters and
//! returns bytes and values, whatever the list says.* A markup reader is a
//! textbook one, and nothing below knows what a page, a part or an object is.
//!
//! # The four things worth knowing before reading further
//!
//! **`<!DOCTYPE` is refused by name, before one byte past it is read.** Not
//! bounded — refused. ECMA-388 9.3.2 [M2.71]: *"DTD content MUST NOT be used in
//! the XML markup defined in this Standard, and consumers MUST instantiate an
//! error condition when encountering DTD content."* So billion laughs, the
//! quadratic-blowup variant, an external entity and a parameter entity in the
//! internal subset are each [`Error::DoctypeUnsupported`] rather than a cap
//! being hit, and `tests/bombs.rs` asserts each of them **by that name**. A
//! test that only asserted "refused" could not tell a structural defence from a
//! depth counter that happened to trip, and the difference between those two is
//! the whole of this decision.
//!
//! What is left cannot expand. The five predefined entities and both radixes of
//! numeric character reference each produce exactly one character from at least
//! four bytes of source, so decoded text is never longer than the text it came
//! from.
//!
//! **It is a pull parser, and an empty-element tag produces two events.**
//! `<a/>` is [`Event::Start`] followed by [`Event::End`], so a caller matching
//! on starts and ends never has to special-case it —
//! [`Element::was_empty`] is there for a caller that wants to know anyway. A
//! tree would allocate the whole of a part before anything looked at it, and a
//! fixed page is the one part of the format gap 30 is for whose size is chosen
//! by the file.
//!
//! **Namespace declarations are resolved, not reported.** `xmlns` and
//! `xmlns:p` are consumed: they do not appear in [`Element::attributes`],
//! because Namespaces in XML says they are not attributes. `xml:lang` and
//! `xml:space` **do** appear, as ordinary attributes in the XML namespace, and
//! nothing here acts on either — a consumer that wants to honour `xml:space`
//! has every character it needs, and one that does not is not silently having
//! whitespace removed underneath it.
//!
//! **Encoding is detected, not expected.** Milestone 1 measured two Microsoft
//! producers writing the same format: WPF puts the UTF-8 signature on every
//! part and the XPS object model puts one on none. A reader that required a
//! byte order mark would refuse every OpenXPS file Windows writes; one that
//! required its absence would refuse every XPS file it writes.
//! [`Source::new`] takes both, and UTF-16 in both byte orders.
//!
//! # Using it
//!
//! ```
//! use tinker_pdf_xml::{Event, Limits, Source};
//!
//! let source = Source::new(b"<page size=\"a4\">text</page>")?;
//! let mut names = Vec::new();
//! for event in source.reader(&Limits::DEFAULT) {
//!     if let Event::Start(element) = event? {
//!         names.push(element.local().to_string());
//!     }
//! }
//! assert_eq!(names, ["page"]);
//! # Ok::<(), tinker_pdf_xml::Error>(())
//! ```

#![forbid(unsafe_code)]

pub mod limits;
mod scan;
mod text;

#[cfg(test)]
mod tests;

use std::borrow::Cow;
use std::fmt;

use scan::Cursor;

/// The namespace the `xml` prefix is bound to, always, without a declaration.
pub const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";

/// The namespace namespace declarations themselves live in. Nothing may be
/// bound to it and nothing may use it as a prefix.
pub const XMLNS_NAMESPACE: &str = "http://www.w3.org/2000/xmlns/";

/// Resource ceilings, defaulting to the constants in [`limits`].
///
/// Separate fields rather than one number because they bound different things
/// and one of them is a **total**: [`Limits::max_tokens`] is spent across a
/// whole part and never refunded, and the other three bound one element, one
/// name or one nesting. Reading a per-item cap as the total is the mistake
/// `MAX_SCRIPT_TOTAL` and `MAX_TILE_WORK` each exist to name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Element nesting. See [`limits::MAX_XML_DEPTH`].
    pub max_depth: usize,
    /// Attributes on one element, namespace declarations included. See
    /// [`limits::MAX_XML_ATTRIBUTES`].
    pub max_attributes: usize,
    /// Bytes of one qualified name; a longer one refuses rather than
    /// truncating. See [`limits::MAX_XML_NAME_LEN`].
    pub max_name_len: usize,
    /// Events one part may produce. **The total.** See
    /// [`limits::MAX_XML_TOKENS`].
    pub max_tokens: usize,
}

impl Limits {
    /// The constants in [`limits`], which is what [`Default`] hands back.
    pub const DEFAULT: Self = Self {
        max_depth: limits::MAX_XML_DEPTH,
        max_attributes: limits::MAX_XML_ATTRIBUTES,
        max_name_len: limits::MAX_XML_NAME_LEN,
        max_tokens: limits::MAX_XML_TOKENS,
    };
}

impl Default for Limits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// How the bytes encoded their characters, as detected rather than as declared.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Encoding {
    /// With or without the signature form of the byte order mark; §4.3.3 makes
    /// this the default when nothing says otherwise.
    Utf8,
    Utf16LittleEndian,
    Utf16BigEndian,
}

/// Which construct ran off the end of the input.
///
/// Named rather than collapsed into one "unterminated", because the four of
/// them fail in different places and a caller reporting to a human wants to say
/// which: an unclosed comment swallows the rest of a page, an unclosed
/// attribute value swallows the rest of a tag, and an unclosed element means
/// the file simply stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Construct {
    Comment,
    CdataSection,
    Instruction,
    Declaration,
    /// A start or end tag: the `>` never arrived.
    Tag,
    AttributeValue,
    /// An `&` with no `;` after it.
    Reference,
    /// The input ended with elements still open.
    Element,
}

impl fmt::Display for Construct {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Comment => "a comment",
            Self::CdataSection => "a CDATA section",
            Self::Instruction => "a processing instruction",
            Self::Declaration => "the XML declaration",
            Self::Tag => "a tag",
            Self::AttributeValue => "an attribute value",
            Self::Reference => "a reference",
            Self::Element => "an element",
        })
    }
}

/// Why the input was refused, one variant per reason.
///
/// The set is deliberately wide. Gap 29's milestone 2 found that *"this archive
/// promised 5 000 bytes and produced 4 000"* and *"checksum wrong"* both
/// refused, and that a test asserting only "refused" could not tell them apart
/// — so it could not tell a working check from a deleted one either. The same
/// argument applies to every well-formedness rule here: a mismatched end tag, a
/// duplicate attribute, an undeclared prefix and an unterminated construct are
/// four different things a producer did wrong, and collapsing them would make
/// this crate's tests unable to see three of the four.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Error {
    /// The bytes are not UTF-8, and nothing said they were anything else.
    NotUtf8,
    /// A byte order mark said UTF-16 and the units are not valid UTF-16: an odd
    /// length, or a lone surrogate.
    NotUtf16,
    /// An encoding this crate does not decode, named in the declaration or by a
    /// UTF-32 byte order mark. UTF-8 and UTF-16 are the two XML requires.
    UnsupportedEncoding,
    /// `version` in the XML declaration is not `1.0`. XML 1.1 has a different
    /// `Char` production and different line-end rules; refusing it by name is
    /// honest where reading it as 1.0 would be a guess.
    UnsupportedVersion,
    /// A character §2.2's `Char` production does not admit — a NUL, a C0
    /// control other than tab, newline and carriage return, `U+FFFE` or
    /// `U+FFFF`.
    IllegalCharacter,
    /// A construct began and the input ended before it did.
    Unterminated(Construct),
    /// **`<!DOCTYPE`.** Refused before one byte past it is read, per ECMA-388
    /// 9.3.2 [M2.71]. This is the variant billion laughs, the quadratic-blowup
    /// variant, an external entity and an internal-subset parameter entity are
    /// each refused as — none of them reaches a cap, because none of them
    /// reaches the grammar that would expand it.
    DoctypeUnsupported,
    /// `<!ENTITY`, `<!ELEMENT`, `<!ATTLIST` or `<!NOTATION` outside a document
    /// type declaration, which is the only place any of them may appear.
    MarkupDeclaration,
    /// The XML declaration is not `<?xml version="1.0" …?>`: a pseudo-attribute
    /// out of order, one that is not one of the three, or a missing `?>`.
    MalformedDeclaration,
    /// A processing instruction whose target is `xml` in any case, which XML
    /// reserves.
    ReservedTarget,
    /// Not a `Name` (§2.3), or a qualified name with an empty half or two
    /// colons.
    MalformedName,
    /// A tag whose syntax is not `<name (S attribute)* S? ('/')? >` — most
    /// often two attributes with no whitespace between them.
    MalformedTag,
    /// An attribute name with no `=` after it.
    ExpectedEquals,
    /// An attribute value that does not begin with `"` or `'`.
    UnquotedAttributeValue,
    /// A literal `<` inside an attribute value, which §3.1 forbids. Usually the
    /// real fault is a start tag whose `>` is missing.
    LessThanInAttributeValue,
    /// `</b>` where `</a>` was owed, or an end tag with nothing open.
    MismatchedEndTag,
    /// Two attributes on one element with the same name — either the same
    /// qualified name, or two different prefixes bound to the same namespace
    /// (Namespaces §5.3, which is the half a raw-name comparison misses).
    DuplicateAttribute,
    /// A prefix used and never bound in scope, or one bound and then undeclared
    /// by an enclosing element's `xmlns:p=""`.
    UndeclaredPrefix,
    /// A declaration Namespaces reserves: `xmlns:xmlns`, `xmlns:xml` bound to
    /// anything but the XML namespace, any other prefix bound to the XML or
    /// xmlns namespace, or `xmlns` used as a prefix in a name.
    ReservedNamespace,
    /// A reference to an entity that is not one of the five predefined ones.
    /// There is no table to look a sixth up in, because building one would mean
    /// having parsed a DTD.
    UnknownEntity,
    /// A numeric character reference naming a surrogate, a value past
    /// `U+10FFFF`, or a scalar §2.2 does not admit.
    BadCharacterReference,
    /// Character data before the root element, which the prolog has no room
    /// for.
    TextBeforeRoot,
    /// Anything but whitespace, a comment or a processing instruction after the
    /// root element's end tag — a second root, most often.
    ContentAfterRoot,
    /// The input held a prolog and no element.
    NoRootElement,
    /// [`Limits::max_depth`]. Per part, and not the total.
    DepthCap,
    /// [`Limits::max_attributes`]. Per element, and not the total.
    AttributeCap,
    /// [`Limits::max_name_len`]. Per name, and not the total.
    NameCap,
    /// [`Limits::max_tokens`]. **The total**, which a per-item cap is not.
    TokenCap,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotUtf8 => f.write_str("not UTF-8"),
            Self::NotUtf16 => f.write_str("not UTF-16"),
            Self::UnsupportedEncoding => f.write_str("an encoding this reader does not decode"),
            Self::UnsupportedVersion => f.write_str("an XML version other than 1.0"),
            Self::IllegalCharacter => f.write_str("a character XML 1.0 does not admit"),
            Self::Unterminated(what) => write!(f, "{what} was never terminated"),
            Self::DoctypeUnsupported => {
                f.write_str("a document type declaration, which is refused rather than parsed")
            }
            Self::MarkupDeclaration => f.write_str("a markup declaration outside a DTD"),
            Self::MalformedDeclaration => f.write_str("a malformed XML declaration"),
            Self::ReservedTarget => f.write_str("a processing instruction targeting `xml`"),
            Self::MalformedName => f.write_str("not a name"),
            Self::MalformedTag => f.write_str("a malformed tag"),
            Self::ExpectedEquals => f.write_str("an attribute name with no `=` after it"),
            Self::UnquotedAttributeValue => f.write_str("an unquoted attribute value"),
            Self::LessThanInAttributeValue => f.write_str("a literal `<` in an attribute value"),
            Self::MismatchedEndTag => f.write_str("an end tag that does not match its start tag"),
            Self::DuplicateAttribute => f.write_str("two attributes with one name"),
            Self::UndeclaredPrefix => f.write_str("a namespace prefix that is not in scope"),
            Self::ReservedNamespace => f.write_str("a namespace declaration XML reserves"),
            Self::UnknownEntity => f.write_str("an entity that was never declared"),
            Self::BadCharacterReference => f.write_str("a character reference naming no character"),
            Self::TextBeforeRoot => f.write_str("character data before the root element"),
            Self::ContentAfterRoot => f.write_str("content after the root element"),
            Self::NoRootElement => f.write_str("no root element"),
            Self::DepthCap => f.write_str("elements nested past the depth cap"),
            Self::AttributeCap => f.write_str("more attributes on one element than the cap allows"),
            Self::NameCap => f.write_str("a name past the length cap"),
            Self::TokenCap => f.write_str("more events than the part's cap allows"),
        }
    }
}

impl std::error::Error for Error {}

/// Everything this crate tolerated, as data rather than a log line (ruling 10).
///
/// The set is closed and each variant is recorded **at most once per part**, in
/// first-occurrence order: a page with two hundred carriage returns inside
/// comments yields one warning, not two hundred. That is the contract
/// `tinker_pdf_zip::Warning` and `tinker_pdf_filters::Warning` already carry,
/// and it is what keeps "it parsed" and "it parsed cleanly" distinguishable for
/// a format whose element count is chosen by the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Warning {
    /// UTF-16 with no byte order mark, recognised from Appendix F's `3C 00` /
    /// `00 3C` shape. §4.3.3 makes the mark mandatory for a UTF-16 entity, so
    /// this reader is inferring rather than reading.
    Utf16WithoutByteOrderMark,
    /// The declaration's `encoding` names a different encoding from the one the
    /// bytes are in. The bytes win — a byte order mark is evidence and a
    /// declaration is a claim — and this says the claim was set aside.
    EncodingDeclarationIgnored,
    /// `xmlns:p=""`, which Namespaces 1.0 forbids and 1.1 legalised as
    /// undeclaring a prefix. Taken as the 1.1 meaning: `p` is unbound from here
    /// down, and using it is [`Error::UndeclaredPrefix`].
    PrefixUndeclared,
    /// `--` inside a comment, which §2.5 forbids. The comment still ends at its
    /// first `-->`, which is the only place it could end.
    DoubleHyphenInComment,
    /// `]]>` in character data outside a CDATA section, which §2.4's `CharData`
    /// production excludes. The text is unambiguous and is kept: refusing a
    /// whole part over it would lose a page for a sequence with only one
    /// possible meaning.
    CdataCloseInContent,
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Utf16WithoutByteOrderMark => "UTF-16 with no byte order mark",
            Self::EncodingDeclarationIgnored => "the declared encoding is not the one in the bytes",
            Self::PrefixUndeclared => "a prefix was undeclared, which XML 1.0 does not allow",
            Self::DoubleHyphenInComment => "`--` inside a comment",
            Self::CdataCloseInContent => "`]]>` in character data",
        })
    }
}

/// An element or attribute name, with its namespace resolved.
///
/// Equality is **expanded-name equality**: the namespace and the local part,
/// never the prefix. That is what Namespaces in XML means by "the same name",
/// and it is the comparison [`Error::DuplicateAttribute`] needs — two prefixes
/// bound to one namespace produce one name, however differently they are
/// spelled. End-tag matching does not use it, because XML 1.0 requires an end
/// tag to repeat the start tag's *spelling*, and the parser compares the
/// qualified forms there.
#[derive(Clone, Debug)]
pub struct Name<'a> {
    qualified: &'a str,
    colon: Option<usize>,
    namespace: Option<Cow<'a, str>>,
}

impl<'a> Name<'a> {
    /// The name as the source spells it, prefix and all.
    #[must_use]
    pub fn qualified(&self) -> &'a str {
        self.qualified
    }

    /// The prefix, or `None` for an unprefixed name.
    #[must_use]
    pub fn prefix(&self) -> Option<&'a str> {
        self.colon.map(|at| self.qualified.get(..at).unwrap_or(""))
    }

    /// The part after the colon, or the whole name when there is none.
    #[must_use]
    pub fn local(&self) -> &'a str {
        match self.colon {
            Some(at) => self.qualified.get(at + 1..).unwrap_or(""),
            None => self.qualified,
        }
    }

    /// The namespace this name resolved to.
    ///
    /// `None` is a real answer rather than a failure: an unprefixed attribute
    /// is never in the default namespace, and an unprefixed element is not in
    /// one either when no default namespace is declared.
    #[must_use]
    pub fn namespace(&self) -> Option<&str> {
        self.namespace.as_deref()
    }
}

impl PartialEq for Name<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.local() == other.local() && self.namespace() == other.namespace()
    }
}

impl Eq for Name<'_> {}

/// One attribute, with its value decoded and normalised.
///
/// Namespace declarations are not here: `xmlns` and `xmlns:p` are consumed by
/// the parser, because Namespaces in XML says they are not attributes.
/// `xml:lang` and `xml:space` **are** here, in [`XML_NAMESPACE`], untouched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attribute<'a> {
    name: Name<'a>,
    value: Cow<'a, str>,
}

impl<'a> Attribute<'a> {
    #[must_use]
    pub fn name(&self) -> &Name<'a> {
        &self.name
    }

    /// References resolved and §3.3.3's whitespace mapping applied: every
    /// literal tab, newline and carriage return in the source is a space here,
    /// and every one that arrived as a character reference is not.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    #[must_use]
    pub fn local(&self) -> &'a str {
        self.name.local()
    }

    #[must_use]
    pub fn prefix(&self) -> Option<&'a str> {
        self.name.prefix()
    }

    #[must_use]
    pub fn namespace(&self) -> Option<&str> {
        self.name.namespace()
    }
}

/// A start tag: its name, its attributes, and whether it closed itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Element<'a> {
    name: Name<'a>,
    attributes: Vec<Attribute<'a>>,
    empty: bool,
}

impl<'a> Element<'a> {
    #[must_use]
    pub fn name(&self) -> &Name<'a> {
        &self.name
    }

    #[must_use]
    pub fn local(&self) -> &'a str {
        self.name.local()
    }

    #[must_use]
    pub fn prefix(&self) -> Option<&'a str> {
        self.name.prefix()
    }

    #[must_use]
    pub fn namespace(&self) -> Option<&str> {
        self.name.namespace()
    }

    #[must_use]
    pub fn attributes(&self) -> &[Attribute<'a>] {
        &self.attributes
    }

    /// The value of the attribute with that expanded name.
    ///
    /// `namespace` is `None` for the unprefixed form, which is what almost
    /// every attribute in a real document is — an unprefixed attribute is never
    /// in the default namespace, so passing the element's own namespace here
    /// would find nothing, and that is the mistake this signature is shaped to
    /// prevent rather than to allow.
    #[must_use]
    pub fn attribute(&self, namespace: Option<&str>, local: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|a| a.local() == local && a.namespace() == namespace)
            .map(Attribute::value)
    }

    /// Whether the source wrote `<a/>` rather than `<a></a>`.
    ///
    /// Both produce a [`Event::Start`] and then an [`Event::End`], so a caller
    /// never needs this to be correct; it is here because a caller that wants
    /// to reproduce the source's spelling has no other way to know.
    #[must_use]
    pub fn was_empty(&self) -> bool {
        self.empty
    }
}

/// One thing the reader found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event<'a> {
    /// A start tag, or the opening half of an empty-element tag.
    Start(Element<'a>),
    /// An end tag, or the closing half of an empty-element tag. The name is
    /// resolved in the scope the element itself opened, which is the scope it
    /// is still in when it closes.
    End(Name<'a>),
    /// Character data, references resolved and line ends normalised.
    /// Whitespace between elements arrives here too — the reader has no way to
    /// know whether it is significant and does not guess.
    Text(Cow<'a, str>),
    /// The contents of a `<![CDATA[…]]>` section, verbatim but for line ends.
    /// A separate event rather than more [`Event::Text`], because the source
    /// said something different and a caller reproducing it needs to know.
    Cdata(Cow<'a, str>),
    /// The contents of a `<!--…-->`, without the delimiters.
    Comment(Cow<'a, str>),
    /// A `<?target value?>`. The XML declaration is not one of these: it is
    /// consumed by [`Source`] and reported through [`Source::encoding`].
    Instruction {
        target: &'a str,
        value: Cow<'a, str>,
    },
}

/// Decoded text, and the encoding it was decoded from.
///
/// Two steps rather than one because the two have different lifetimes: UTF-16
/// input has to be transcoded into a buffer somebody owns, and the events a
/// [`Reader`] hands back borrow from that buffer. A caller holds the `Source`
/// for as long as it holds events, which is the same shape
/// `tinker_pdf_zip::Archive` has for the same reason.
#[derive(Clone, Debug)]
pub struct Source<'a> {
    text: Cow<'a, str>,
    encoding: Encoding,
    warnings: Vec<Warning>,
}

impl<'a> Source<'a> {
    /// Detects the encoding, decodes, and checks every character against §2.2.
    ///
    /// The character sweep is done here, once, over the whole input rather than
    /// per construct. Every construct is bound by the same production, so one
    /// sweep is both cheaper and impossible to get partially right — an illegal
    /// byte inside a comment is exactly as illegal as one inside an attribute
    /// value, and a reader that checked only the second would pass every
    /// fixture anybody would think to write.
    ///
    /// # Errors
    ///
    /// [`Error::NotUtf8`], [`Error::NotUtf16`], [`Error::UnsupportedEncoding`]
    /// or [`Error::IllegalCharacter`].
    pub fn new(bytes: &'a [u8]) -> Result<Self, Error> {
        let (text, encoding, warning) = text::decode(bytes)?;
        if text::illegal_character(&text).is_some() {
            return Err(Error::IllegalCharacter);
        }
        Ok(Source {
            text,
            encoding,
            warnings: warning.into_iter().collect(),
        })
    }

    /// The decoded characters, with the byte order mark removed.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// What the bytes were in, which is not necessarily what they said they
    /// were in — see [`Warning::EncodingDeclarationIgnored`].
    #[must_use]
    pub fn encoding(&self) -> Encoding {
        self.encoding
    }

    /// What decoding tolerated. A [`Reader`] starts its own list from this one,
    /// so a caller that reads events never has to merge two lists.
    #[must_use]
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }

    /// A pull parser over this text.
    #[must_use]
    pub fn reader(&self, limits: &Limits) -> Reader<'_> {
        Reader {
            cursor: Cursor::new(&self.text),
            limits: *limits,
            encoding: self.encoding,
            stage: Stage::Declaration,
            open: Vec::new(),
            scope: Vec::new(),
            pending: None,
            tokens: 0,
            warnings: self.warnings.clone(),
        }
    }
}

/// One open element: its spelling, and how many namespace bindings it pushed.
struct Open<'a> {
    qualified: &'a str,
    bindings: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Declaration,
    Prolog,
    Content,
    Epilog,
    Done,
    Failed,
}

/// The pull parser.
///
/// It is an [`Iterator`] over `Result<Event, Error>`, and it is **fused by
/// refusal**: the first error is yielded once and then the iteration ends. A
/// caller that keeps asking after an error gets `None` rather than a second
/// attempt at the same broken input.
pub struct Reader<'a> {
    cursor: Cursor<'a>,
    limits: Limits,
    encoding: Encoding,
    stage: Stage,
    open: Vec<Open<'a>>,
    /// Every namespace binding in scope, innermost last. `""` is the default
    /// namespace; a `None` URI is a prefix that was undeclared.
    scope: Vec<(&'a str, Option<Cow<'a, str>>)>,
    /// The end event an empty-element tag still owes.
    pending: Option<Name<'a>>,
    tokens: usize,
    warnings: Vec<Warning>,
}

impl<'a> Iterator for Reader<'a> {
    type Item = Result<Event<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.step() {
            Ok(event) => event.map(Ok),
            Err(error) => {
                self.stage = Stage::Failed;
                Some(Err(error))
            }
        }
    }
}

impl<'a> Reader<'a> {
    /// Where the reader is, in bytes from the start of the decoded text.
    ///
    /// After a refusal this is where the refusal happened, which is what makes
    /// `<!DOCTYPE` refused *before one byte past it is read* a checkable claim
    /// rather than a described one.
    #[must_use]
    pub fn offset(&self) -> usize {
        self.cursor.offset()
    }

    /// How many events have been produced, which is what
    /// [`Limits::max_tokens`] bounds.
    #[must_use]
    pub fn events(&self) -> usize {
        self.tokens
    }

    /// How many elements are open right now, which is what
    /// [`Limits::max_depth`] bounds.
    #[must_use]
    pub fn depth(&self) -> usize {
        self.open.len()
    }

    /// What this reader tolerated, deduplicated, in first-occurrence order.
    #[must_use]
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }

    fn warn(&mut self, warning: Warning) {
        if !self.warnings.contains(&warning) {
            self.warnings.push(warning);
        }
    }

    fn step(&mut self) -> Result<Option<Event<'a>>, Error> {
        loop {
            match self.stage {
                Stage::Done | Stage::Failed => return Ok(None),
                Stage::Declaration => {
                    self.declaration()?;
                    self.stage = Stage::Prolog;
                }
                Stage::Prolog => {
                    if let Some(event) = self.prolog()? {
                        return Ok(Some(event));
                    }
                }
                Stage::Content => return self.content().map(Some),
                Stage::Epilog => return self.epilog(),
            }
        }
    }

    /// Spends one token and hands the event back.
    fn emit(&mut self, event: Event<'a>) -> Result<Event<'a>, Error> {
        if self.tokens >= self.limits.max_tokens {
            return Err(Error::TokenCap);
        }
        self.tokens += 1;
        Ok(event)
    }

    /// `<?xml version="1.0" encoding="…" standalone="…"?>`, if it is there.
    ///
    /// It is only a declaration at offset zero. `<?xml …?>` anywhere else is a
    /// processing instruction with a reserved target, which is
    /// [`Error::ReservedTarget`] rather than a second declaration — and milestone
    /// 1 found four spellings of this line across eight real files, including
    /// none at all, so nothing here is required to be present.
    fn declaration(&mut self) -> Result<(), Error> {
        if !self.cursor.starts_with("<?xml") {
            return Ok(());
        }
        // `<?xmlfoo?>` is a processing instruction, not a declaration. `<?xml?>`
        // is a declaration and a broken one, and saying so is more use than
        // calling it an instruction with a reserved target.
        let after = self.cursor.rest().get(5..).unwrap_or("");
        match after.chars().next() {
            Some(c) if text::is_space(c) || c == '?' => {}
            _ => return Ok(()),
        }
        self.cursor.eat("<?xml");

        let mut seen = 0usize;
        loop {
            self.cursor.skip_space();
            if self.cursor.eat("?>") {
                break;
            }
            if self.cursor.at_end() {
                return Err(Error::Unterminated(Construct::Declaration));
            }
            let name = self
                .cursor
                .name(self.limits.max_name_len)
                .map_err(|_| Error::MalformedDeclaration)?;
            self.cursor.skip_space();
            if !self.cursor.eat("=") {
                return Err(Error::MalformedDeclaration);
            }
            self.cursor.skip_space();
            let value = self
                .cursor
                .attribute_value()
                .map_err(|_| Error::MalformedDeclaration)?;

            match (seen, name) {
                (0, "version") => {
                    if value != "1.0" {
                        return Err(Error::UnsupportedVersion);
                    }
                }
                (1, "encoding") => self.check_encoding(value)?,
                (1 | 2, "standalone") => {
                    if value != "yes" && value != "no" {
                        return Err(Error::MalformedDeclaration);
                    }
                }
                _ => return Err(Error::MalformedDeclaration),
            }
            seen += 1;
        }
        if seen == 0 {
            return Err(Error::MalformedDeclaration);
        }
        Ok(())
    }

    /// The declaration's `encoding`, against the encoding the bytes are in.
    ///
    /// Case-insensitively, because milestone 1 measured both `utf-8` and
    /// `UTF-8` across one corpus from one vendor. A name this crate does not
    /// decode is refused by name; a name it decodes that disagrees with the
    /// bytes is a warning, because a byte order mark is evidence and a
    /// declaration is a claim.
    fn check_encoding(&mut self, declared: &str) -> Result<(), Error> {
        let lower = declared.to_ascii_lowercase();
        let named = match lower.as_str() {
            "utf-8" | "utf8" => Encoding::Utf8,
            "utf-16" => match self.encoding {
                // Unmarked `UTF-16` names neither byte order, so it agrees with
                // whichever one the bytes turned out to be.
                Encoding::Utf16LittleEndian | Encoding::Utf16BigEndian => self.encoding,
                Encoding::Utf8 => Encoding::Utf16BigEndian,
            },
            "utf-16le" => Encoding::Utf16LittleEndian,
            "utf-16be" => Encoding::Utf16BigEndian,
            _ => return Err(Error::UnsupportedEncoding),
        };
        if named != self.encoding {
            self.warn(Warning::EncodingDeclarationIgnored);
        }
        Ok(())
    }

    /// Everything before the root element: whitespace, comments and processing
    /// instructions, and then the root.
    fn prolog(&mut self) -> Result<Option<Event<'a>>, Error> {
        self.cursor.skip_space();
        if self.cursor.at_end() {
            return Err(Error::NoRootElement);
        }
        if let Some(event) = self.aside()? {
            return Ok(Some(event));
        }
        if self.cursor.starts_with("</") {
            return Err(Error::MismatchedEndTag);
        }
        // A CDATA section is character data, and the prolog has no room for
        // any; `<!` anything else is a markup declaration, which only a DTD may
        // hold. Both are named rather than left to fall into the tag parser,
        // where they would come back as "not a name".
        if self.cursor.starts_with("<![CDATA[") {
            return Err(Error::TextBeforeRoot);
        }
        if self.cursor.starts_with("<!") {
            return Err(Error::MarkupDeclaration);
        }
        if self.cursor.starts_with("<") {
            let element = self.start_tag()?;
            self.stage = Stage::Content;
            return self.emit(Event::Start(element)).map(Some);
        }
        Err(Error::TextBeforeRoot)
    }

    /// Inside the root element.
    fn content(&mut self) -> Result<Event<'a>, Error> {
        if let Some(name) = self.pending.take() {
            self.close_top();
            return self.emit(Event::End(name));
        }
        if self.cursor.at_end() {
            return Err(Error::Unterminated(Construct::Element));
        }
        if self.cursor.starts_with("</") {
            let name = self.end_tag()?;
            self.close_top();
            return self.emit(Event::End(name));
        }
        if let Some(event) = self.aside()? {
            return Ok(event);
        }
        if self.cursor.starts_with("<![CDATA[") {
            self.cursor.eat("<![CDATA[");
            let raw = self.cursor.until("]]>", Construct::CdataSection)?;
            return self.emit(Event::Cdata(text::line_ends(raw)));
        }
        if self.cursor.starts_with("<!") {
            return Err(Error::MarkupDeclaration);
        }
        if self.cursor.starts_with("<") {
            let element = self.start_tag()?;
            return self.emit(Event::Start(element));
        }
        let rest = self.cursor.rest();
        let end = rest.find('<').unwrap_or(rest.len());
        let raw = rest.get(..end).unwrap_or("");
        self.cursor.eat(raw);
        if raw.contains("]]>") {
            self.warn(Warning::CdataCloseInContent);
        }
        let decoded = text::value(raw, false)?;
        self.emit(Event::Text(decoded))
    }

    /// After the root element's end tag.
    fn epilog(&mut self) -> Result<Option<Event<'a>>, Error> {
        self.cursor.skip_space();
        if self.cursor.at_end() {
            self.stage = Stage::Done;
            return Ok(None);
        }
        // `<!DOCTYPE` here too, and by the same name: a declaration after the
        // root is as much DTD content as one before it, and a reader that
        // checked only the prolog would parse the one place nothing looks.
        if let Some(event) = self.aside()? {
            return Ok(Some(event));
        }
        Err(Error::ContentAfterRoot)
    }

    /// A comment or a processing instruction, either of which is legal in all
    /// three stages — and the two markup declarations that are legal in none.
    ///
    /// **`<!DOCTYPE` is refused here, and the cursor does not move.** That is
    /// the whole of this crate's answer to entity expansion: not a budget, but
    /// never entering the grammar that has one. [`Reader::offset`] afterwards
    /// is the offset of the `<`, which is what makes "before one byte past it
    /// is read" something a test can assert rather than something a comment can
    /// claim.
    fn aside(&mut self) -> Result<Option<Event<'a>>, Error> {
        if self.cursor.starts_with("<!DOCTYPE") {
            return Err(Error::DoctypeUnsupported);
        }
        if self.cursor.starts_with("<!--") {
            self.cursor.eat("<!--");
            let raw = self.cursor.until("-->", Construct::Comment)?;
            if raw.contains("--") || raw.ends_with('-') {
                self.warn(Warning::DoubleHyphenInComment);
            }
            return self.emit(Event::Comment(text::line_ends(raw))).map(Some);
        }
        for declaration in ["<!ENTITY", "<!ELEMENT", "<!ATTLIST", "<!NOTATION"] {
            if self.cursor.starts_with(declaration) {
                return Err(Error::MarkupDeclaration);
            }
        }
        if self.cursor.starts_with("<?") {
            self.cursor.eat("<?");
            let target = self.cursor.name(self.limits.max_name_len)?;
            if target.eq_ignore_ascii_case("xml") {
                return Err(Error::ReservedTarget);
            }
            let spaced = self.cursor.skip_space();
            if self.cursor.eat("?>") {
                return self
                    .emit(Event::Instruction {
                        target,
                        value: Cow::Borrowed(""),
                    })
                    .map(Some);
            }
            if !spaced {
                return Err(Error::MalformedDeclaration);
            }
            let raw = self.cursor.until("?>", Construct::Instruction)?;
            return self
                .emit(Event::Instruction {
                    target,
                    value: text::line_ends(raw),
                })
                .map(Some);
        }
        Ok(None)
    }

    /// `<name (S attribute)* S? '/'? '>'`.
    fn start_tag(&mut self) -> Result<Element<'a>, Error> {
        if self.open.len() >= self.limits.max_depth {
            return Err(Error::DepthCap);
        }
        if !self.cursor.eat("<") {
            return Err(Error::MalformedTag);
        }
        let qualified = self.cursor.name(self.limits.max_name_len)?;

        let mut raw: Vec<(&'a str, Cow<'a, str>)> = Vec::new();
        let empty;
        loop {
            let spaced = self.cursor.skip_space();
            if self.cursor.eat("/>") {
                empty = true;
                break;
            }
            if self.cursor.eat(">") {
                empty = false;
                break;
            }
            if self.cursor.at_end() {
                return Err(Error::Unterminated(Construct::Tag));
            }
            // `<a b="1"c="2"/>` is not well formed and the only thing that says
            // so is that nothing was consumed between the two.
            if !spaced {
                return Err(Error::MalformedTag);
            }
            if raw.len() >= self.limits.max_attributes {
                return Err(Error::AttributeCap);
            }
            let name = self.cursor.name(self.limits.max_name_len)?;
            self.cursor.skip_space();
            if !self.cursor.eat("=") {
                return Err(Error::ExpectedEquals);
            }
            self.cursor.skip_space();
            let literal = self.cursor.attribute_value()?;
            raw.push((name, text::value(literal, true)?));
        }

        // XML 1.0's own rule, before namespaces get a look in: no element may
        // carry the same *spelling* twice. It catches a repeated `xmlns:p` as
        // well, which the expanded comparison below cannot, because a namespace
        // declaration is not an attribute by the time that runs.
        for (index, (name, _)) in raw.iter().enumerate() {
            if raw.iter().take(index).any(|(earlier, _)| earlier == name) {
                return Err(Error::DuplicateAttribute);
            }
        }

        let bindings = self.declare_namespaces(&raw)?;

        // And then Namespaces §5.3, which is a different rule with the same
        // name: two prefixes bound to one namespace produce one attribute name,
        // however differently they are spelled.
        let mut attributes: Vec<Attribute<'a>> = Vec::new();
        for (name, value) in raw {
            if name == "xmlns" || name.starts_with("xmlns:") {
                continue;
            }
            let resolved = self.resolve(name, false)?;
            if attributes.iter().any(|a| a.name == resolved) {
                return Err(Error::DuplicateAttribute);
            }
            attributes.push(Attribute {
                name: resolved,
                value,
            });
        }

        let name = self.resolve(qualified, true)?;
        self.open.push(Open {
            qualified,
            bindings,
        });
        if empty {
            self.pending = Some(name.clone());
        }
        Ok(Element {
            name,
            attributes,
            empty,
        })
    }

    /// Pushes this element's namespace bindings and says how many there were.
    fn declare_namespaces(&mut self, raw: &[(&'a str, Cow<'a, str>)]) -> Result<usize, Error> {
        let mut pushed = 0;
        for (name, value) in raw {
            let prefix = if *name == "xmlns" {
                ""
            } else if let Some(prefix) = name.strip_prefix("xmlns:") {
                prefix
            } else {
                continue;
            };
            if prefix.contains(':') {
                return Err(Error::MalformedName);
            }
            let uri: &str = value;
            // Namespaces §3: the two reserved namespaces, in all four ways a
            // file can get at them.
            if prefix == "xmlns" || uri == XMLNS_NAMESPACE {
                return Err(Error::ReservedNamespace);
            }
            // One test, four cases: `xmlns:xml` bound to anything else, any
            // other prefix bound to the XML namespace, the default namespace
            // bound to it, and `xmlns:xml=""`, which would undeclare a prefix
            // that cannot be undeclared.
            if (prefix == "xml") != (uri == XML_NAMESPACE) {
                return Err(Error::ReservedNamespace);
            }
            let bound = if uri.is_empty() {
                if !prefix.is_empty() {
                    // Legal in Namespaces 1.1 and not in 1.0. Taken as 1.1's
                    // meaning rather than refused, because the meaning is
                    // unambiguous and refusing would lose a page over it.
                    self.warn(Warning::PrefixUndeclared);
                }
                None
            } else {
                Some(value.clone())
            };
            self.scope.push((prefix, bound));
            pushed += 1;
        }
        Ok(pushed)
    }

    /// A qualified name against the bindings in scope.
    ///
    /// `default` says whether an unprefixed name takes the default namespace,
    /// and it is `false` for every attribute. That is not a shortcut: Namespaces
    /// §6.2 is explicit that the default namespace does not apply to attribute
    /// names, and a reader that applied it would give `Width` and `x:Key` the
    /// same namespace and then find duplicates that are not there.
    fn resolve(&self, qualified: &'a str, default: bool) -> Result<Name<'a>, Error> {
        match qualified.find(':') {
            Some(at) => {
                let prefix = qualified.get(..at).unwrap_or("");
                let local = qualified.get(at + 1..).unwrap_or("");
                if prefix.is_empty() || local.is_empty() || local.contains(':') {
                    return Err(Error::MalformedName);
                }
                if prefix == "xmlns" {
                    return Err(Error::ReservedNamespace);
                }
                let namespace = self.lookup(prefix).ok_or(Error::UndeclaredPrefix)?;
                Ok(Name {
                    qualified,
                    colon: Some(at),
                    namespace: Some(namespace),
                })
            }
            None => Ok(Name {
                qualified,
                colon: None,
                namespace: if default { self.lookup("") } else { None },
            }),
        }
    }

    /// The innermost binding for a prefix, or `None` when there is none.
    ///
    /// Innermost-last and searched backwards, so an inner declaration shadows
    /// an outer one — and [`Reader::close_top`] truncates rather than removing
    /// by name, so a shadowed binding comes back when the shadowing element
    /// closes.
    fn lookup(&self, prefix: &str) -> Option<Cow<'a, str>> {
        if prefix == "xml" {
            return Some(Cow::Borrowed(XML_NAMESPACE));
        }
        self.scope
            .iter()
            .rev()
            .find(|(bound, _)| *bound == prefix)
            .and_then(|(_, uri)| uri.clone())
    }

    /// `</name S? >`, matched against the open element's own spelling.
    ///
    /// XML 1.0 §3 requires the end tag to repeat the start tag's `Name`, prefix
    /// included, so this is a literal comparison and not an expanded-name one.
    /// The name handed back is resolved, because the caller wants a name and
    /// not a spelling.
    fn end_tag(&mut self) -> Result<Name<'a>, Error> {
        self.cursor.eat("</");
        let qualified = self.cursor.name(self.limits.max_name_len)?;
        self.cursor.skip_space();
        if !self.cursor.eat(">") {
            if self.cursor.at_end() {
                return Err(Error::Unterminated(Construct::Tag));
            }
            return Err(Error::MalformedTag);
        }
        match self.open.last() {
            Some(open) if open.qualified == qualified => {}
            _ => return Err(Error::MismatchedEndTag),
        }
        self.resolve(qualified, true)
    }

    /// Closes the innermost element and unbinds what it declared.
    fn close_top(&mut self) {
        if let Some(open) = self.open.pop() {
            let keep = self.scope.len().saturating_sub(open.bindings);
            self.scope.truncate(keep);
        }
        if self.open.is_empty() {
            self.stage = Stage::Epilog;
        }
    }
}
