//! X.690 DER: tag, length, value, walked without trusting a byte of it.
//!
//! Everything above this file — certificates today, CMS tomorrow — is a shape
//! drawn over this walker, so this is where the never-panic obligation
//! (ruling 1) is actually discharged. Three properties carry it, and each is
//! a decision rather than a habit.
//!
//! **Nothing is indexed.** Every read goes through `slice::get` and every
//! offset through a checked or saturating operation, so a length field that
//! claims four gigabytes inside a two-hundred-byte buffer produces
//! [`DerError::LengthOverrun`] rather than a panic in release and a different
//! panic in debug.
//!
//! **Nothing recurses.** [`Cursor::read`] reads one node and stops; descending
//! is the caller asking for [`Tlv::children`], which hands back another
//! cursor. A parser built this way cannot overflow the stack on a deeply
//! nested input, and the depth cap in [`Limits`] then exists for the *other*
//! reason: to bound how much work a small input can demand.
//!
//! **Nothing is allocated per node.** A [`Tlv`] is four words of borrowed
//! slice and three scalars. Allocation happens where a value must be
//! *decoded* rather than located — [`Tlv::as_string`] builds a `String`
//! because a BMPString is not UTF-8 — and nowhere else.
//!
//! # DER, not BER
//!
//! X.509 is DER by definition (RFC 5280 §4.1), and DER is the subset of BER
//! with exactly one encoding per value. That subset is enforced rather than
//! assumed, because "accept BER too" is how a parser ends up with two readings
//! of the same bytes — and two readings of a signed structure is a signature
//! bypass, not a leniency. So:
//!
//! - **indefinite lengths are refused by name** ([`DerError::IndefiniteLength`],
//!   X.690 §8.1.3.6 and §10.1) rather than guessed at by hunting for the
//!   end-of-contents pair;
//! - a long-form length that would fit the short form, or one with a leading
//!   zero, is [`DerError::NonMinimalLength`] (§10.1);
//! - `0xFF` as a first length octet is [`DerError::ReservedLength`] (§8.1.3.5);
//! - a high-tag-number form used for a tag below 31, or with a leading
//!   `0x80`, is [`DerError::NonMinimalTag`] (§8.1.2.4.2);
//! - an INTEGER with a redundant leading `0x00` or `0xFF` is
//!   [`DerError::NonMinimalInteger`] (§8.3.2);
//! - a BOOLEAN whose content is neither `0x00` nor `0xFF` is
//!   [`DerError::NonCanonicalBoolean`] (§11.1).
//!
//! # What is deliberately *not* enforced, and why
//!
//! Two DER rules are checked nowhere here, and both omissions are choices.
//!
//! **Unused bits in a BIT STRING are not required to be zero** (§11.2.1). The
//! count is range-checked (0 to 7, [`DerError::BitStringUnusedBits`]) because
//! a count above 7 makes the value's length ambiguous; the padding bits
//! themselves are left alone because certificates from real issuers set them
//! and nothing downstream reads them — [`BitString::bit`] masks by index
//! rather than by trailing byte.
//!
//! **SET OF ordering is not checked** (§11.6). Verifying it means comparing
//! every element's encoding against the next, which is work proportional to
//! the input for a property no consumer here depends on: a multi-valued
//! relative distinguished name is compared as encoded bytes, and out-of-order
//! elements simply fail to match rather than being silently reordered.
//! [`crate::name`] says the same thing from the other side.

use core::cell::Cell;
use core::fmt;

/// How deep the walker may descend, and how many nodes it will read.
///
/// Both ceilings are needed and they bound different things. A DER node costs
/// at least two bytes, so an *n*-byte input holds at most *n*/2 nodes and the
/// node ceiling is therefore about bounding work relative to a budget the
/// caller sets rather than relative to whatever the input happens to be —
/// a 10 MB certificate would otherwise buy five million nodes' worth of
/// decoding before anything said no.
///
/// Depth is the ceiling that matters more, and for a reason the byte count
/// hides: nesting also costs two bytes a level, so 200 bytes of `30 82 …`
/// nests a hundred levels deep and 200 kilobytes nests a hundred thousand.
/// This walker does not recurse, so that is not a stack overflow here — but
/// every consumer above it is written as though a certificate were shallow,
/// and a cap is what makes that true rather than hoped for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Levels below the outermost node. Reaching it is
    /// [`DerError::DepthExceeded`].
    pub max_depth: u32,
    /// Nodes read across the whole parse. Reaching it is
    /// [`DerError::NodeBudgetExceeded`].
    pub max_nodes: u32,
}

impl Limits {
    /// The ceilings a certificate is parsed under.
    ///
    /// **Depth 32.** RFC 5280's own structures bottom out at eight: a
    /// certificate holds a TBSCertificate holds an extensions `[3]` holds the
    /// extensions SEQUENCE holds one extension holds its `extnValue` OCTET
    /// STRING holds, for `certificatePolicies` (§4.2.1.4), a policy
    /// information SEQUENCE holding a qualifier SEQUENCE holding a
    /// `UserNotice`. Thirty-two is that with room for a structure nobody here
    /// anticipated, and far below anything a consumer would call deep.
    ///
    /// **65 536 nodes.** The 578-byte certificate in RFC 5280 Appendix C.1 is
    /// 58 nodes. A certificate large enough to need more than sixty-five
    /// thousand is not a certificate this engine is going to make sense of,
    /// and the ceiling is the point at which saying so is cheaper than
    /// continuing.
    pub const CERTIFICATE: Self = Self {
        max_depth: 32,
        max_nodes: 65_536,
    };

    /// Ceilings of the caller's choosing.
    #[must_use]
    pub const fn new(max_depth: u32, max_nodes: u32) -> Self {
        Self {
            max_depth,
            max_nodes,
        }
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self::CERTIFICATE
    }
}

/// One parse's allowance, shared by every cursor that descends into it.
///
/// The node count is per *parse* rather than per cursor, which is the only
/// counting that bounds anything: a budget reset on each descent would let a
/// wide-and-shallow structure spend without limit. Interior mutability rather
/// than `&mut` because a [`Tlv`] handed back from a parent cursor must be able
/// to open a child cursor while the parent is still alive.
#[derive(Debug)]
pub struct Budget {
    limits: Limits,
    spent: Cell<u32>,
}

impl Budget {
    /// A fresh allowance.
    #[must_use]
    pub const fn new(limits: Limits) -> Self {
        Self {
            limits,
            spent: Cell::new(0),
        }
    }

    /// The ceilings this budget was opened with.
    #[must_use]
    pub const fn limits(&self) -> Limits {
        self.limits
    }

    /// How many nodes have been read so far.
    #[must_use]
    pub fn spent(&self) -> u32 {
        self.spent.get()
    }

    /// Charges one node, or refuses.
    fn charge(&self) -> Result<(), DerError> {
        let spent = self.spent.get();
        if spent >= self.limits.max_nodes {
            return Err(DerError::NodeBudgetExceeded);
        }
        self.spent.set(spent.saturating_add(1));
        Ok(())
    }
}

impl Default for Budget {
    fn default() -> Self {
        Self::new(Limits::CERTIFICATE)
    }
}

/// A tag's class (X.690 §8.1.2.2, Table 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Class {
    Universal,
    Application,
    ContextSpecific,
    Private,
}

/// The universal tag numbers X.509 and CMS actually use (X.690 §8.4).
///
/// Not the whole of Table 1: a tag this crate has no reader for is not made
/// into a name here, because a name with no decoder behind it reads like
/// support. Anything else arrives as a raw number on [`Tlv::tag`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tag {
    Boolean,
    Integer,
    BitString,
    OctetString,
    Null,
    Oid,
    Utf8String,
    Sequence,
    Set,
    PrintableString,
    /// T61String. See [`Tlv::as_string`] for what this crate does with one.
    TeletexString,
    Ia5String,
    UtcTime,
    GeneralizedTime,
    UniversalString,
    BmpString,
}

impl Tag {
    /// The tag number, as X.690 Table 1 assigns it.
    #[must_use]
    pub const fn number(self) -> u32 {
        match self {
            Self::Boolean => 1,
            Self::Integer => 2,
            Self::BitString => 3,
            Self::OctetString => 4,
            Self::Null => 5,
            Self::Oid => 6,
            Self::Utf8String => 12,
            Self::Sequence => 16,
            Self::Set => 17,
            Self::PrintableString => 19,
            Self::TeletexString => 20,
            Self::Ia5String => 22,
            Self::UtcTime => 23,
            Self::GeneralizedTime => 24,
            Self::UniversalString => 28,
            Self::BmpString => 30,
        }
    }

    /// Whether DER encodes this type constructed.
    ///
    /// SEQUENCE and SET always are; every other type here is primitive,
    /// because DER forbids the constructed form for simple types (X.690
    /// §10.2) — which is what keeps a segmented OCTET STRING from being a
    /// second spelling of the same value.
    #[must_use]
    pub const fn is_constructed(self) -> bool {
        matches!(self, Self::Sequence | Self::Set)
    }

    /// The universal tag with this number, where this crate names one.
    #[must_use]
    pub const fn from_number(number: u32) -> Option<Self> {
        Some(match number {
            1 => Self::Boolean,
            2 => Self::Integer,
            3 => Self::BitString,
            4 => Self::OctetString,
            5 => Self::Null,
            6 => Self::Oid,
            12 => Self::Utf8String,
            16 => Self::Sequence,
            17 => Self::Set,
            19 => Self::PrintableString,
            20 => Self::TeletexString,
            22 => Self::Ia5String,
            23 => Self::UtcTime,
            24 => Self::GeneralizedTime,
            28 => Self::UniversalString,
            30 => Self::BmpString,
            _ => return None,
        })
    }
}

/// Everything this crate refuses to read, with the reason it refused.
///
/// There is no variant meaning "malformed": a refusal that cannot say which
/// rule was broken is a refusal nobody can act on, and the one place a caller
/// most needs to know is a signature that failed to verify because its
/// certificate would not parse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DerError {
    /// The buffer ended inside a tag or a length.
    Truncated,
    /// A definite length that reaches past the end of the enclosing value.
    LengthOverrun,
    /// X.690 §8.1.3.6's `0x80`. Legal BER, forbidden in DER (§10.1), and never
    /// guessed at here — the end-of-contents pair it would need is a second
    /// way to encode where a value stops.
    IndefiniteLength,
    /// A long-form length that the short form encodes, or one with leading
    /// zero octets (§10.1).
    NonMinimalLength,
    /// `0xFF` as the first length octet, reserved by §8.1.3.5.
    ReservedLength,
    /// A length whose value does not fit this machine's `usize`.
    LengthTooLarge,
    /// A tag number past `u32`.
    TagTooLarge,
    /// The high-tag-number form used below 31, or with a leading `0x80`
    /// (§8.1.2.4.2).
    NonMinimalTag,
    /// [`Limits::max_depth`] reached.
    DepthExceeded,
    /// [`Limits::max_nodes`] reached.
    NodeBudgetExceeded,
    /// Bytes left over after the structure that was supposed to fill them.
    TrailingBytes,
    /// A cursor ran out where a value was required.
    UnexpectedEnd,
    /// A different tag than the grammar allows here.
    UnexpectedTag { expected: u32, found: u32 },
    /// The right tag number in the wrong class.
    UnexpectedClass { expected: Class, found: Class },
    /// A primitive where the type is constructed, or the reverse (§10.2).
    WrongForm { tag: u32, constructed: bool },
    /// BOOLEAN content that is neither `0x00` nor `0xFF` (§11.1), or is not
    /// exactly one octet.
    NonCanonicalBoolean,
    /// An INTEGER with no content octets, or with a redundant leading `0x00`
    /// or `0xFF` (§8.3.2).
    NonMinimalInteger,
    /// An INTEGER too wide for the requested Rust type.
    IntegerTooLarge,
    /// A negative INTEGER where the grammar admits only a non-negative one.
    NegativeInteger,
    /// A BIT STRING with no content at all: even an empty one carries its
    /// unused-bit count (§8.6.2.3).
    BitStringEmpty,
    /// An unused-bit count above 7, or above 0 with no data octets.
    BitStringUnusedBits { count: u8 },
    /// A BIT STRING with unused bits where the grammar requires a whole
    /// number of octets — a public key, a signature.
    BitStringNotWhole,
    /// NULL with content octets.
    NullNotEmpty,
    /// An OBJECT IDENTIFIER with no content octets (§8.19.1).
    OidEmpty,
    /// An OID whose final octet still sets the continuation bit, so the last
    /// subidentifier never ends.
    OidUnterminated,
    /// A subidentifier with a leading `0x80`, which pads a value that already
    /// had a shorter encoding (§8.19.2).
    OidNonMinimalArc,
    /// A subidentifier past `u64`. Real arcs are small; one that is not is
    /// refused rather than truncated into a different OID.
    OidArcTooLarge,
    /// A string type whose octets are not valid UTF-8.
    BadUtf8String,
    /// A PrintableString or IA5String with a byte at or above `0x80`.
    NonAsciiString { tag: u32 },
    /// A BMPString of odd length, or a UniversalString whose length is not a
    /// multiple of four.
    BadWideString { tag: u32 },
    /// A wide string code unit that is not a Unicode scalar value — an
    /// unpaired surrogate in a BMPString, or a value past `U+10FFFF`.
    BadWideChar { tag: u32 },
    /// A tag this crate does not read as text.
    NotAString { tag: u32 },
    /// A time this crate will not read. See [`TimeFault`].
    MalformedTime(TimeFault),
}

/// Why a UTCTime or GeneralizedTime was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeFault {
    /// Not the one length RFC 5280 permits: 13 octets for UTCTime
    /// (§4.1.2.5.1), 15 for GeneralizedTime (§4.1.2.5.2). Both profiles
    /// require seconds, and GeneralizedTime forbids fractional seconds, so
    /// every other length is a form this profile does not admit.
    Length,
    /// A non-digit where the format has a digit.
    NotDigits,
    /// A final octet that is not `Z`. RFC 5280 requires Zulu; a local-time
    /// offset would make the value depend on a zone table this engine does
    /// not carry.
    NotZulu,
    /// A field outside its range: month past 12, day past the month's length,
    /// hour past 23, minute or second past 59.
    ///
    /// Second 60 is refused with the rest. X.680 admits it for a leap second;
    /// no certificate has ever been observed using one, and accepting it
    /// would mean deciding which instant it maps to.
    FieldOutOfRange,
}

impl fmt::Display for DerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => write!(f, "the encoding ends inside a tag or a length"),
            Self::LengthOverrun => write!(f, "a length reaches past the end of its container"),
            Self::IndefiniteLength => {
                write!(f, "an indefinite length: legal BER, forbidden in DER")
            }
            Self::NonMinimalLength => write!(f, "a length not in its shortest form"),
            Self::ReservedLength => write!(f, "0xFF as a first length octet is reserved"),
            Self::LengthTooLarge => write!(f, "a length wider than this machine's usize"),
            Self::TagTooLarge => write!(f, "a tag number wider than u32"),
            Self::NonMinimalTag => write!(f, "a tag number not in its shortest form"),
            Self::DepthExceeded => write!(f, "nesting past the depth ceiling"),
            Self::NodeBudgetExceeded => write!(f, "more nodes than the budget allows"),
            Self::TrailingBytes => write!(f, "bytes left over after the structure"),
            Self::UnexpectedEnd => write!(f, "a required value is missing"),
            Self::UnexpectedTag { expected, found } => {
                write!(f, "expected tag {expected}, found {found}")
            }
            Self::UnexpectedClass { expected, found } => {
                write!(f, "expected a {expected:?} tag, found a {found:?} one")
            }
            Self::WrongForm { tag, constructed } => write!(
                f,
                "tag {tag} encoded {} where DER requires the other form",
                if *constructed {
                    "constructed"
                } else {
                    "primitive"
                }
            ),
            Self::NonCanonicalBoolean => write!(f, "a BOOLEAN that is neither 0x00 nor 0xFF"),
            Self::NonMinimalInteger => write!(f, "an INTEGER not in its shortest form"),
            Self::IntegerTooLarge => write!(f, "an INTEGER too wide for the value asked for"),
            Self::NegativeInteger => write!(f, "a negative INTEGER where none is allowed"),
            Self::BitStringEmpty => write!(f, "a BIT STRING with no unused-bit count"),
            Self::BitStringUnusedBits { count } => {
                write!(f, "a BIT STRING claiming {count} unused bits")
            }
            Self::BitStringNotWhole => {
                write!(f, "a BIT STRING with unused bits where octets are required")
            }
            Self::NullNotEmpty => write!(f, "a NULL with content"),
            Self::OidEmpty => write!(f, "an OBJECT IDENTIFIER with no content"),
            Self::OidUnterminated => write!(f, "an OBJECT IDENTIFIER whose last arc never ends"),
            Self::OidNonMinimalArc => write!(f, "an OBJECT IDENTIFIER arc with a leading 0x80"),
            Self::OidArcTooLarge => write!(f, "an OBJECT IDENTIFIER arc wider than u64"),
            Self::BadUtf8String => write!(f, "a UTF8String that is not valid UTF-8"),
            Self::NonAsciiString { tag } => write!(f, "a non-ASCII byte in a tag-{tag} string"),
            Self::BadWideString { tag } => write!(f, "a tag-{tag} string of the wrong width"),
            Self::BadWideChar { tag } => {
                write!(f, "a tag-{tag} string code unit that is not a character")
            }
            Self::NotAString { tag } => write!(f, "tag {tag} is not a string type"),
            Self::MalformedTime(fault) => write!(f, "a time this profile refuses: {fault:?}"),
        }
    }
}

impl std::error::Error for DerError {}

/// One tag-length-value, located rather than decoded.
///
/// Both slices borrow the buffer the cursor was opened on, and [`Tlv::start`]
/// is the offset of the tag octet within *that* buffer however deeply nested
/// this node is — which is what lets a caller name an exact byte range for
/// something it will later digest. `raw` is the whole encoding, header
/// included, so `raw.len() - value.len()` is the header width.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tlv<'a> {
    class: Class,
    constructed: bool,
    tag: u32,
    value: &'a [u8],
    raw: &'a [u8],
    start: usize,
    depth: u32,
}

impl<'a> Tlv<'a> {
    /// The tag's class.
    #[must_use]
    pub const fn class(&self) -> Class {
        self.class
    }

    /// Whether the encoding is constructed.
    #[must_use]
    pub const fn is_constructed(&self) -> bool {
        self.constructed
    }

    /// The tag number, within its class.
    #[must_use]
    pub const fn tag(&self) -> u32 {
        self.tag
    }

    /// The universal tag, where this is one and this crate names it.
    #[must_use]
    pub fn universal_tag(&self) -> Option<Tag> {
        match self.class {
            Class::Universal => Tag::from_number(self.tag),
            _ => None,
        }
    }

    /// The content octets.
    #[must_use]
    pub const fn value(&self) -> &'a [u8] {
        self.value
    }

    /// The complete encoding, header included.
    #[must_use]
    pub const fn raw(&self) -> &'a [u8] {
        self.raw
    }

    /// Where the tag octet sits in the buffer the outermost cursor was opened
    /// on.
    #[must_use]
    pub const fn start(&self) -> usize {
        self.start
    }

    /// One past the last octet of this encoding, in the same coordinates.
    #[must_use]
    pub const fn end(&self) -> usize {
        self.start.saturating_add(self.raw.len())
    }

    /// This node's byte range in the buffer the outermost cursor was opened
    /// on.
    #[must_use]
    pub const fn range(&self) -> core::ops::Range<usize> {
        self.start()..self.end()
    }

    /// How many levels below the outermost node this one sits.
    #[must_use]
    pub const fn depth(&self) -> u32 {
        self.depth
    }

    /// Whether this is the context-specific tag `[n]`, of either form.
    #[must_use]
    pub fn is_context(&self, n: u32) -> bool {
        self.class == Class::ContextSpecific && self.tag == n
    }

    /// A cursor over the content, for a constructed node.
    ///
    /// Also the one place depth is charged, because descending is the only
    /// thing that increases it.
    pub fn children<'b>(&self, budget: &'b Budget) -> Result<Cursor<'a, 'b>, DerError> {
        if !self.constructed {
            return Err(DerError::WrongForm {
                tag: self.tag,
                constructed: false,
            });
        }
        let depth = self.depth.saturating_add(1);
        if depth > budget.limits.max_depth {
            return Err(DerError::DepthExceeded);
        }
        let header = self.raw.len().saturating_sub(self.value.len());
        Ok(Cursor {
            data: self.value,
            base: self.start.saturating_add(header),
            at: 0,
            depth,
            budget,
        })
    }

    /// The single node inside an `[n] EXPLICIT` wrapper.
    ///
    /// An explicit context tag is a constructed node holding exactly one
    /// value; anything else in it is [`DerError::TrailingBytes`] rather than
    /// a value silently ignored.
    pub fn explicit(&self, budget: &Budget) -> Result<Tlv<'a>, DerError> {
        let mut inner = self.children(budget)?;
        let one = inner.read()?;
        inner.finish()?;
        Ok(one)
    }

    /// Re-reads this node's content as though it were the body of an
    /// `[n] IMPLICIT` tag of the given universal type.
    ///
    /// An implicit tag replaces the type's own tag octet, so the content is
    /// already in the right shape and only the label was overwritten. This
    /// hands back a [`Tlv`] wearing the universal tag, positioned exactly
    /// where it was, so the ordinary accessors apply.
    #[must_use]
    pub fn implicit(&self, tag: Tag) -> Tlv<'a> {
        Tlv {
            class: Class::Universal,
            constructed: self.constructed,
            tag: tag.number(),
            value: self.value,
            raw: self.raw,
            start: self.start,
            depth: self.depth,
        }
    }

    /// Checks that this node carries the universal tag named, in the form DER
    /// requires for it.
    pub fn require(&self, tag: Tag) -> Result<(), DerError> {
        if self.class != Class::Universal {
            return Err(DerError::UnexpectedClass {
                expected: Class::Universal,
                found: self.class,
            });
        }
        if self.tag != tag.number() {
            return Err(DerError::UnexpectedTag {
                expected: tag.number(),
                found: self.tag,
            });
        }
        if self.constructed != tag.is_constructed() {
            return Err(DerError::WrongForm {
                tag: self.tag,
                constructed: self.constructed,
            });
        }
        Ok(())
    }

    /// A BOOLEAN (§11.1).
    pub fn as_bool(&self) -> Result<bool, DerError> {
        self.require(Tag::Boolean)?;
        match self.value {
            [0x00] => Ok(false),
            [0xFF] => Ok(true),
            _ => Err(DerError::NonCanonicalBoolean),
        }
    }

    /// An INTEGER, checked minimal and left in two's complement.
    pub fn as_integer(&self) -> Result<Int<'a>, DerError> {
        self.require(Tag::Integer)?;
        Int::from_content(self.value)
    }

    /// An OCTET STRING's content.
    pub fn as_octet_string(&self) -> Result<&'a [u8], DerError> {
        self.require(Tag::OctetString)?;
        Ok(self.value)
    }

    /// A NULL, which has no content at all.
    pub fn as_null(&self) -> Result<(), DerError> {
        self.require(Tag::Null)?;
        if self.value.is_empty() {
            Ok(())
        } else {
            Err(DerError::NullNotEmpty)
        }
    }

    /// A BIT STRING (§8.6), unused-bit count range-checked.
    pub fn as_bit_string(&self) -> Result<BitString<'a>, DerError> {
        self.require(Tag::BitString)?;
        BitString::from_content(self.value)
    }

    /// An OBJECT IDENTIFIER, validated so that every later comparison against
    /// it is a comparison between two well-formed encodings.
    pub fn as_oid(&self) -> Result<Oid<'a>, DerError> {
        self.require(Tag::Oid)?;
        Oid::parse(self.value)
    }

    /// A string, from whichever of the string types this node carries.
    ///
    /// # What each type costs to read honestly
    ///
    /// **UTF8String, IA5String, PrintableString** are exact. IA5 is IA5
    /// (ASCII), and anything at or above `0x80` is
    /// [`DerError::NonAsciiString`].
    ///
    /// PrintableString's *repertoire* — X.680 Table 10's letters, digits and
    /// eleven punctuation marks — is deliberately **not** enforced. Real
    /// issuers put `@`, `_` and `*` in one, and refusing those would throw
    /// away a name this crate can otherwise compare byte for byte. The width
    /// check stays, because a byte above `0x7F` has no defined meaning here at
    /// all.
    ///
    /// **BMPString** is UCS-2 big-endian and **UniversalString** is UCS-4
    /// big-endian; both are decoded exactly, and a code unit that is not a
    /// Unicode scalar value — a lone surrogate, most often — is
    /// [`DerError::BadWideChar`] rather than a replacement character, because
    /// a name silently altered is a name that matches the wrong certificate.
    ///
    /// **TeletexString is an approximation, and this is the one place in this
    /// file where the answer is not derived from a specification.** T.61 is a
    /// multi-byte encoding with escape sequences and floating diacritics, and
    /// nothing that puts a TeletexString in a certificate today means T.61 by
    /// it — the near-universal practice is ISO 8859-1, which is what is
    /// decoded here. A certificate that genuinely meant T.61 for a byte above
    /// `0x7F` gets the wrong character from this function. It is recorded
    /// rather than hidden, and the exact encoding stays reachable through
    /// [`Tlv::value`] for a caller that needs to compare rather than display.
    pub fn as_string(&self) -> Result<String, DerError> {
        let Some(tag) = self.universal_tag() else {
            return Err(DerError::NotAString { tag: self.tag });
        };
        if self.constructed {
            return Err(DerError::WrongForm {
                tag: self.tag,
                constructed: true,
            });
        }
        match tag {
            Tag::Utf8String => core::str::from_utf8(self.value)
                .map(str::to_owned)
                .map_err(|_| DerError::BadUtf8String),
            Tag::PrintableString | Tag::Ia5String => {
                if self.value.iter().any(|b| *b >= 0x80) {
                    return Err(DerError::NonAsciiString { tag: self.tag });
                }
                Ok(self.value.iter().map(|b| char::from(*b)).collect())
            }
            Tag::TeletexString => Ok(self.value.iter().map(|b| char::from(*b)).collect()),
            Tag::BmpString => decode_wide(self.value, 2, self.tag),
            Tag::UniversalString => decode_wide(self.value, 4, self.tag),
            _ => Err(DerError::NotAString { tag: self.tag }),
        }
    }

    /// Whether this node carries one of the types [`Tlv::as_string`] reads.
    #[must_use]
    pub fn is_string(&self) -> bool {
        !self.constructed
            && matches!(
                self.universal_tag(),
                Some(
                    Tag::Utf8String
                        | Tag::PrintableString
                        | Tag::Ia5String
                        | Tag::TeletexString
                        | Tag::BmpString
                        | Tag::UniversalString
                )
            )
    }

    /// A UTCTime or GeneralizedTime, as seconds since 1970-01-01T00:00:00Z.
    ///
    /// An integer rather than a date because the only questions asked of a
    /// certificate's validity are comparisons — is this before that, is the
    /// signing time inside the window — and an integer answers them without a
    /// calendar type, without a zone table, and without the platform clock
    /// ruling 4 keeps out of this tree. Negative for instants before 1970,
    /// which a certificate should never carry and this function will
    /// nonetheless compute rather than refuse.
    ///
    /// **UTCTime's two-digit year is RFC 5280 §4.1.2.5.1's rule, not X.680's**:
    /// 00 through 49 are 2000 through 2049, and 50 through 99 are 1950 through
    /// 1999. The sliding-window reading some libraries use — relative to the
    /// current year — is refused on sight, because it makes a certificate's
    /// meaning depend on when it is read, and this crate has no clock to slide
    /// against even if it wanted one.
    pub fn as_time(&self) -> Result<i64, DerError> {
        match self.universal_tag() {
            Some(Tag::UtcTime) => {
                self.require(Tag::UtcTime)?;
                parse_utc_time(self.value)
            }
            Some(Tag::GeneralizedTime) => {
                self.require(Tag::GeneralizedTime)?;
                parse_generalized_time(self.value)
            }
            _ => Err(DerError::NotAString { tag: self.tag }),
        }
    }
}

/// Decodes a fixed-width big-endian string of Unicode code points.
fn decode_wide(data: &[u8], width: usize, tag: u32) -> Result<String, DerError> {
    if data.len() % width != 0 {
        return Err(DerError::BadWideString { tag });
    }
    let mut out = String::with_capacity(data.len() / width);
    for unit in data.chunks_exact(width) {
        let mut value = 0u32;
        for byte in unit {
            // At most four shifts of eight, so `value` never leaves `u32`.
            value = (value << 8) | u32::from(*byte);
        }
        let Some(character) = char::from_u32(value) else {
            return Err(DerError::BadWideChar { tag });
        };
        out.push(character);
    }
    Ok(out)
}

/// An INTEGER's content octets: two's complement, big-endian, minimal.
///
/// Kept as bytes rather than widened into a Rust integer because the two
/// INTEGERs that matter most here do not fit one. A certificate's serial
/// number is up to twenty octets by RFC 5280 §4.1.2.2 and longer in practice,
/// and an RSA modulus is hundreds; both are identifiers to be compared and
/// operands to be handed on, not numbers to be counted with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Int<'a> {
    content: &'a [u8],
}

impl<'a> Int<'a> {
    /// Validates §8.3.2's minimal form and wraps the content.
    fn from_content(content: &'a [u8]) -> Result<Self, DerError> {
        let (Some(first), second) = (content.first().copied(), content.get(1).copied()) else {
            return Err(DerError::NonMinimalInteger);
        };
        if let Some(second) = second {
            // Nine leading zero bits, or nine leading one bits: either way the
            // first octet carries nothing the second does not.
            if (first == 0x00 && second & 0x80 == 0) || (first == 0xFF && second & 0x80 != 0) {
                return Err(DerError::NonMinimalInteger);
            }
        }
        Ok(Self { content })
    }

    /// The content octets exactly as encoded.
    #[must_use]
    pub const fn as_bytes(&self) -> &'a [u8] {
        self.content
    }

    /// Whether the value is negative — the sign bit of the first octet.
    #[must_use]
    pub fn is_negative(&self) -> bool {
        self.content.first().is_some_and(|b| b & 0x80 != 0)
    }

    /// The unsigned magnitude, with the sign octet a positive value needs
    /// removed.
    ///
    /// A 2048-bit RSA modulus encodes as 257 octets whose first is `0x00`,
    /// because the real first octet has its top bit set and INTEGER is signed.
    /// That octet is not part of the number and every consumer of a modulus
    /// wants it gone.
    pub fn magnitude(&self) -> Result<&'a [u8], DerError> {
        if self.is_negative() {
            return Err(DerError::NegativeInteger);
        }
        match self.content {
            [0x00, rest @ ..] => Ok(rest),
            rest => Ok(rest),
        }
    }

    /// The value as `u64`, where it is non-negative and fits.
    pub fn as_u64(&self) -> Result<u64, DerError> {
        let magnitude = self.magnitude()?;
        if magnitude.len() > 8 {
            return Err(DerError::IntegerTooLarge);
        }
        let mut value = 0u64;
        for byte in magnitude {
            value = (value << 8) | u64::from(*byte);
        }
        Ok(value)
    }

    /// The value as `i64`, sign extended, where it fits.
    pub fn as_i64(&self) -> Result<i64, DerError> {
        if self.content.len() > 8 {
            return Err(DerError::IntegerTooLarge);
        }
        let fill = if self.is_negative() { u64::MAX } else { 0 };
        let mut value = fill;
        for byte in self.content {
            value = (value << 8) | u64::from(*byte);
        }
        Ok(value as i64)
    }

    /// The content as lower-case hex, which is how a serial number is written
    /// wherever one is written down.
    #[must_use]
    pub fn to_hex(&self) -> String {
        let mut out = String::with_capacity(self.content.len().saturating_mul(2));
        for byte in self.content {
            out.push(nibble(byte >> 4));
            out.push(nibble(byte & 0x0F));
        }
        out
    }
}

/// One hex digit, lower case.
fn nibble(value: u8) -> char {
    match value {
        0..=9 => char::from(b'0'.wrapping_add(value)),
        _ => char::from(b'a'.wrapping_add(value.wrapping_sub(10))),
    }
}

/// A BIT STRING: its unused-bit count and the octets that carry the bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BitString<'a> {
    unused: u8,
    bytes: &'a [u8],
}

impl<'a> BitString<'a> {
    /// Splits §8.6.2's leading unused-bit count off the content.
    fn from_content(content: &'a [u8]) -> Result<Self, DerError> {
        let Some((unused, bytes)) = content.split_first() else {
            return Err(DerError::BitStringEmpty);
        };
        if *unused > 7 || (*unused > 0 && bytes.is_empty()) {
            return Err(DerError::BitStringUnusedBits { count: *unused });
        }
        Ok(Self {
            unused: *unused,
            bytes,
        })
    }

    /// How many bits of the final octet are padding.
    #[must_use]
    pub const fn unused_bits(&self) -> u8 {
        self.unused
    }

    /// The octets, padding bits included.
    #[must_use]
    pub const fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// How many bits the value actually holds.
    #[must_use]
    pub fn bit_len(&self) -> usize {
        self.bytes
            .len()
            .saturating_mul(8)
            .saturating_sub(self.unused as usize)
    }

    /// Bit `index`, numbered as X.680 numbers a named bit list: bit 0 is the
    /// most significant bit of the first octet.
    ///
    /// Out of range is `false` rather than an error, which is the reading
    /// RFC 5280 §4.2.1.3 requires — a `KeyUsage` encoded short has its
    /// trailing bits unset, not undefined.
    #[must_use]
    pub fn bit(&self, index: usize) -> bool {
        if index >= self.bit_len() {
            return false;
        }
        let byte = index / 8;
        let shift = 7 - (index % 8);
        self.bytes.get(byte).is_some_and(|b| (b >> shift) & 1 == 1)
    }

    /// The octets, where the value is a whole number of them.
    ///
    /// A public key or a signature is octets that happen to be carried in a
    /// BIT STRING; one with unused bits is not a shorter key, it is a
    /// malformed one.
    pub fn whole_bytes(&self) -> Result<&'a [u8], DerError> {
        if self.unused == 0 {
            Ok(self.bytes)
        } else {
            Err(DerError::BitStringNotWhole)
        }
    }
}

/// An OBJECT IDENTIFIER, held as its content octets.
///
/// The encoding *is* the comparable value: DER gives an OID exactly one
/// encoding (X.690 §8.19), so two OIDs are equal when their content octets
/// are, and a table of constants can be written as byte strings and compared
/// against with no decoding, no allocation and no arc arithmetic at all. The
/// arcs are recovered only when something has to be printed.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Oid<'a> {
    content: &'a [u8],
}

impl<'a> Oid<'a> {
    /// Validates §8.19's encoding rules and wraps the content octets.
    pub fn parse(content: &'a [u8]) -> Result<Self, DerError> {
        if content.is_empty() {
            return Err(DerError::OidEmpty);
        }
        if content.last().is_some_and(|b| b & 0x80 != 0) {
            return Err(DerError::OidUnterminated);
        }
        let mut starting = true;
        let mut width = 0u32;
        for byte in content {
            if starting && *byte == 0x80 {
                // §8.19.2: the leading octet of a subidentifier is never 0x80,
                // because that pads a value the shorter form already encodes.
                return Err(DerError::OidNonMinimalArc);
            }
            starting = *byte & 0x80 == 0;
            width = if starting { 0 } else { width.saturating_add(1) };
            // A subidentifier of `width` continuation octets plus its
            // terminator carries `7 * (width + 1)` bits, so eight continuation
            // octets carry sixty-three and always fit, and nine carry seventy
            // and may not.
            if width > 8 {
                return Err(DerError::OidArcTooLarge);
            }
        }
        Ok(Self { content })
    }

    /// Wraps content octets written out in this crate's own tables.
    ///
    /// Not validated, because a `const` cannot be. Every constant in
    /// [`crate::oid`] is walked through [`Oid::parse`] by a test in that
    /// module, so a mistyped table entry fails `cargo test` rather than
    /// quietly failing to match anything.
    #[must_use]
    pub const fn from_content(content: &'a [u8]) -> Self {
        Self { content }
    }

    /// The content octets.
    #[must_use]
    pub const fn as_bytes(&self) -> &'a [u8] {
        self.content
    }

    /// The arcs, in order.
    ///
    /// The first content octet packs the first two arcs as `40 * a + b`
    /// (§8.19.4), so this yields two values before it has consumed one
    /// subidentifier.
    #[must_use]
    pub const fn arcs(&self) -> Arcs<'a> {
        Arcs {
            content: self.content,
            at: 0,
            pending: None,
            first: true,
        }
    }

    /// The dotted-decimal form, for anything a person reads.
    #[must_use]
    pub fn to_dotted(&self) -> String {
        let mut out = String::new();
        for arc in self.arcs() {
            if !out.is_empty() {
                out.push('.');
            }
            let mut digits = [0u8; 20];
            let mut at = digits.len();
            let mut value = arc;
            loop {
                at = at.saturating_sub(1);
                if let Some(slot) = digits.get_mut(at) {
                    *slot = b'0'.wrapping_add((value % 10) as u8);
                }
                value /= 10;
                if value == 0 || at == 0 {
                    break;
                }
            }
            for digit in digits.get(at..).unwrap_or_default() {
                out.push(char::from(*digit));
            }
        }
        out
    }
}

impl fmt::Debug for Oid<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Oid({})", self.to_dotted())
    }
}

impl fmt::Display for Oid<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_dotted())
    }
}

/// [`Oid::arcs`]'s iterator.
#[derive(Clone, Copy, Debug)]
pub struct Arcs<'a> {
    content: &'a [u8],
    at: usize,
    /// The second arc, held back while the first is yielded.
    pending: Option<u64>,
    first: bool,
}

impl Iterator for Arcs<'_> {
    type Item = u64;

    fn next(&mut self) -> Option<u64> {
        if let Some(second) = self.pending.take() {
            return Some(second);
        }
        let mut value = 0u64;
        loop {
            let byte = *self.content.get(self.at)?;
            self.at = self.at.saturating_add(1);
            // `Oid::parse` refused anything that could overflow here, and
            // `from_content` is covered by the table's own test.
            value = value.wrapping_shl(7) | u64::from(byte & 0x7F);
            if byte & 0x80 == 0 {
                break;
            }
        }
        if self.first {
            self.first = false;
            let leading: u64 = match value {
                0..=39 => 0,
                40..=79 => 1,
                _ => 2,
            };
            self.pending = Some(value.saturating_sub(leading.saturating_mul(40)));
            return Some(leading);
        }
        Some(value)
    }
}

/// A position inside a definite-length encoding, and everything needed to read
/// the next node from it.
///
/// Copy because reading is `&mut self` on a cursor a caller owns; two cursors
/// over the same bytes are two independent positions, which is what makes a
/// look-ahead a clone rather than a saved index.
#[derive(Clone, Copy, Debug)]
pub struct Cursor<'a, 'b> {
    data: &'a [u8],
    /// Where `data[0]` sits in the buffer the outermost cursor was opened on.
    base: usize,
    at: usize,
    depth: u32,
    budget: &'b Budget,
}

impl<'a, 'b> Cursor<'a, 'b> {
    /// A cursor over a whole buffer, at depth zero.
    #[must_use]
    pub const fn new(data: &'a [u8], budget: &'b Budget) -> Self {
        Self {
            data,
            base: 0,
            at: 0,
            depth: 0,
            budget,
        }
    }

    /// Whether everything has been read.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.at >= self.data.len()
    }

    /// What has not been read yet.
    #[must_use]
    pub fn remaining(&self) -> &'a [u8] {
        self.data.get(self.at.min(self.data.len())..).unwrap_or(&[])
    }

    /// The next unread octet's offset, in the outermost buffer's coordinates.
    #[must_use]
    pub fn offset(&self) -> usize {
        self.base.saturating_add(self.at)
    }

    /// Reads the next node, whatever it is.
    ///
    /// Not `next`, and not an `Iterator`: running out is
    /// [`DerError::UnexpectedEnd`] rather than `None`, because inside a
    /// SEQUENCE whose grammar demands another field, running out is the
    /// defect and an iterator would report it as a clean end.
    pub fn read(&mut self) -> Result<Tlv<'a>, DerError> {
        let start = self.at;
        let rest = self.remaining();
        if rest.is_empty() {
            return Err(DerError::UnexpectedEnd);
        }
        self.budget.charge()?;

        let (class, constructed, tag, after_tag) = read_tag(rest)?;
        let (length, after_length) = read_length(rest, after_tag)?;
        let end = after_length
            .checked_add(length)
            .ok_or(DerError::LengthOverrun)?;
        let raw = rest.get(..end).ok_or(DerError::LengthOverrun)?;
        let value = rest.get(after_length..end).ok_or(DerError::LengthOverrun)?;

        self.at = start.saturating_add(end);
        Ok(Tlv {
            class,
            constructed,
            tag,
            value,
            raw,
            start: self.base.saturating_add(start),
            depth: self.depth,
        })
    }

    /// Reads the next node's class, form and tag without consuming it.
    ///
    /// `None` at the end, so an OPTIONAL field is a look before a leap rather
    /// than a parse whose failure is thrown away.
    #[must_use]
    pub fn peek(&self) -> Option<Result<(Class, bool, u32), DerError>> {
        let rest = self.remaining();
        if rest.is_empty() {
            return None;
        }
        Some(read_tag(rest).map(|(class, constructed, tag, _)| (class, constructed, tag)))
    }

    /// Reads the next node and requires the universal tag named.
    pub fn expect(&mut self, tag: Tag) -> Result<Tlv<'a>, DerError> {
        let tlv = self.read()?;
        tlv.require(tag)?;
        Ok(tlv)
    }

    /// Reads the next node if it carries the universal tag named, and leaves
    /// the cursor untouched if it does not.
    pub fn expect_optional(&mut self, tag: Tag) -> Result<Option<Tlv<'a>>, DerError> {
        match self.peek() {
            None => Ok(None),
            Some(Err(error)) => Err(error),
            Some(Ok((class, _, number))) => {
                if class != Class::Universal || number != tag.number() {
                    return Ok(None);
                }
                self.expect(tag).map(Some)
            }
        }
    }

    /// Reads the next node if it carries the context-specific tag `[n]`, and
    /// leaves the cursor untouched if it does not.
    pub fn context_optional(&mut self, n: u32) -> Result<Option<Tlv<'a>>, DerError> {
        match self.peek() {
            None => Ok(None),
            Some(Err(error)) => Err(error),
            Some(Ok((class, _, number))) => {
                if class != Class::ContextSpecific || number != n {
                    return Ok(None);
                }
                self.read().map(Some)
            }
        }
    }

    /// Requires that nothing is left.
    pub fn finish(self) -> Result<(), DerError> {
        if self.is_empty() {
            Ok(())
        } else {
            Err(DerError::TrailingBytes)
        }
    }
}

/// Reads an identifier octet and any high-tag-number continuation (§8.1.2).
///
/// Returns the class, the form, the tag number and how many octets it took.
fn read_tag(data: &[u8]) -> Result<(Class, bool, u32, usize), DerError> {
    let first = *data.first().ok_or(DerError::Truncated)?;
    let class = match first >> 6 {
        0 => Class::Universal,
        1 => Class::Application,
        2 => Class::ContextSpecific,
        _ => Class::Private,
    };
    let constructed = first & 0x20 != 0;
    let low = first & 0x1F;
    if low != 0x1F {
        return Ok((class, constructed, u32::from(low), 1));
    }

    // §8.1.2.4: the high-tag-number form, base 128, most significant first.
    let mut tag = 0u32;
    let mut at = 1usize;
    loop {
        let byte = *data.get(at).ok_or(DerError::Truncated)?;
        if at == 1 && byte == 0x80 {
            return Err(DerError::NonMinimalTag);
        }
        tag = tag
            .checked_mul(128)
            .and_then(|t| t.checked_add(u32::from(byte & 0x7F)))
            .ok_or(DerError::TagTooLarge)?;
        at = at.saturating_add(1);
        if byte & 0x80 == 0 {
            break;
        }
    }
    if tag < 0x1F {
        // §8.1.2.3: below 31 the low-tag-number form is the only encoding.
        return Err(DerError::NonMinimalTag);
    }
    Ok((class, constructed, tag, at))
}

/// Reads a length octet and any long form (§8.1.3), starting at `from`.
///
/// Returns the length and where the content begins.
fn read_length(data: &[u8], from: usize) -> Result<(usize, usize), DerError> {
    let first = *data.get(from).ok_or(DerError::Truncated)?;
    let after_first = from.saturating_add(1);
    if first < 0x80 {
        return Ok((usize::from(first), after_first));
    }
    if first == 0x80 {
        return Err(DerError::IndefiniteLength);
    }
    if first == 0xFF {
        return Err(DerError::ReservedLength);
    }

    let count = usize::from(first & 0x7F);
    let end = after_first.checked_add(count).ok_or(DerError::Truncated)?;
    let octets = data.get(after_first..end).ok_or(DerError::Truncated)?;
    if octets.first().is_some_and(|b| *b == 0x00) {
        // §10.1: no leading zero octets, so one length has one encoding.
        return Err(DerError::NonMinimalLength);
    }
    let mut length = 0usize;
    for byte in octets {
        length = length
            .checked_mul(256)
            .and_then(|l| l.checked_add(usize::from(*byte)))
            .ok_or(DerError::LengthTooLarge)?;
    }
    if length < 0x80 {
        // §10.1 again, from the other side: the short form encodes this.
        return Err(DerError::NonMinimalLength);
    }
    Ok((length, end))
}

/// `YYMMDDHHMMSSZ`, with RFC 5280 §4.1.2.5.1's century rule.
fn parse_utc_time(data: &[u8]) -> Result<i64, DerError> {
    if data.len() != 13 {
        return Err(DerError::MalformedTime(TimeFault::Length));
    }
    let two = |at: usize| digits(data, at, 2);
    let year = two(0)?;
    // 00..=49 is 2000..=2049 and 50..=99 is 1950..=1999. Fixed, not sliding:
    // a certificate must not mean something different next year.
    let year = if year < 50 { 2000 + year } else { 1900 + year };
    seconds_from(
        year,
        two(2)?,
        two(4)?,
        two(6)?,
        two(8)?,
        two(10)?,
        data.get(12).copied(),
    )
}

/// `YYYYMMDDHHMMSSZ` — RFC 5280 §4.1.2.5.2's only admitted form.
fn parse_generalized_time(data: &[u8]) -> Result<i64, DerError> {
    if data.len() != 15 {
        return Err(DerError::MalformedTime(TimeFault::Length));
    }
    seconds_from(
        digits(data, 0, 4)?,
        digits(data, 4, 2)?,
        digits(data, 6, 2)?,
        digits(data, 8, 2)?,
        digits(data, 10, 2)?,
        digits(data, 12, 2)?,
        data.get(14).copied(),
    )
}

/// `count` decimal digits at `at`.
fn digits(data: &[u8], at: usize, count: usize) -> Result<i64, DerError> {
    let end = at.saturating_add(count);
    let slice = data
        .get(at..end)
        .ok_or(DerError::MalformedTime(TimeFault::Length))?;
    let mut value = 0i64;
    for byte in slice {
        if !byte.is_ascii_digit() {
            return Err(DerError::MalformedTime(TimeFault::NotDigits));
        }
        value = value
            .saturating_mul(10)
            .saturating_add(i64::from(byte.wrapping_sub(b'0')));
    }
    Ok(value)
}

/// Range-checks a broken-down UTC time and folds it to a Unix second.
fn seconds_from(
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
    zone: Option<u8>,
) -> Result<i64, DerError> {
    if zone != Some(b'Z') {
        return Err(DerError::MalformedTime(TimeFault::NotZulu));
    }
    if !(1..=12).contains(&month)
        || !(1..=days_in_month(year, month)).contains(&day)
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0..=59).contains(&second)
    {
        return Err(DerError::MalformedTime(TimeFault::FieldOutOfRange));
    }
    let days = days_from_civil(year, month, day);
    Ok(days
        .saturating_mul(86_400)
        .saturating_add(hour.saturating_mul(3_600))
        .saturating_add(minute.saturating_mul(60))
        .saturating_add(second))
}

/// Whether a year has a 29th of February, by the proleptic Gregorian rule.
fn is_leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// How many days a month has.
fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Days from 1970-01-01 to `year-month-day`, proleptic Gregorian.
///
/// The era arithmetic, rather than a loop over years, because a loop over
/// years is a loop whose trip count the input chooses. Every operation is
/// integer division and multiplication inside `i64` for any year a four-digit
/// GeneralizedTime can name, so there is nothing here to overflow and no
/// floating point for ruling 4 to object to.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    // March-based years, so a leap day is the last day rather than a day in
    // the middle that every later month has to know about.
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_index = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Hex with whitespace, for tests that want to read like a dump.
    pub(crate) fn unhex(text: &str) -> Vec<u8> {
        let mut out = Vec::new();
        let mut high: Option<u8> = None;
        for character in text.chars() {
            let Some(value) = character.to_digit(16) else {
                continue;
            };
            let value = value as u8;
            match high.take() {
                None => high = Some(value),
                Some(first) => out.push((first << 4) | value),
            }
        }
        assert!(high.is_none(), "an odd number of hex digits");
        out
    }

    fn one(data: &[u8]) -> Result<Tlv<'_>, DerError> {
        let budget = Budget::default();
        let mut cursor = Cursor::new(data, &budget);
        cursor.read()
    }

    #[test]
    fn a_short_form_sequence_reads() {
        let data = unhex("30 03 02 01 07");
        let budget = Budget::default();
        let mut cursor = Cursor::new(&data, &budget);
        let sequence = cursor.expect(Tag::Sequence).expect("a SEQUENCE");
        assert_eq!(sequence.range(), 0..5);
        assert!(cursor.finish().is_ok());

        let mut inner = sequence.children(&budget).expect("constructed");
        let integer = inner.expect(Tag::Integer).expect("an INTEGER");
        assert_eq!(integer.as_integer().expect("minimal").as_u64(), Ok(7));
        // The child's coordinates are the outer buffer's, which is what makes
        // a nested byte range quotable.
        assert_eq!(integer.range(), 2..5);
        assert_eq!(integer.depth(), 1);
    }

    #[test]
    fn a_long_form_length_reads() {
        let mut data = unhex("04 81 80");
        data.extend(std::iter::repeat_n(0xAA, 128));
        let tlv = one(&data).expect("a 128-byte OCTET STRING");
        assert_eq!(tlv.as_octet_string().expect("primitive").len(), 128);
    }

    #[test]
    fn a_truncated_length_is_refused() {
        // The long form promises two more octets and the buffer has one.
        assert_eq!(one(&unhex("30 82 01")), Err(DerError::Truncated));
        // And a header with no length octet at all.
        assert_eq!(one(&unhex("30")), Err(DerError::Truncated));
    }

    #[test]
    fn a_length_past_the_buffer_is_refused() {
        assert_eq!(one(&unhex("04 05 01 02")), Err(DerError::LengthOverrun));
        // The long form reaching four gigabytes out of a four-byte buffer.
        assert_eq!(
            one(&unhex("04 84 FF FF FF FF")),
            Err(DerError::LengthOverrun)
        );
    }

    #[test]
    fn an_indefinite_length_is_refused_by_name() {
        // Legal BER: SEQUENCE, indefinite, one INTEGER, end-of-contents. This
        // crate does not go looking for the terminator.
        assert_eq!(
            one(&unhex("30 80 02 01 05 00 00")),
            Err(DerError::IndefiniteLength)
        );
    }

    #[test]
    fn a_non_minimal_length_is_refused() {
        // 0x81 0x05 spells five, which the short form already spells.
        assert_eq!(
            one(&unhex("04 81 05 01 02 03 04 05")),
            Err(DerError::NonMinimalLength)
        );
        // A leading zero octet in the long form.
        assert_eq!(one(&unhex("04 82 00 80")), Err(DerError::NonMinimalLength));
    }

    #[test]
    fn a_reserved_length_octet_is_refused() {
        assert_eq!(one(&unhex("04 FF")), Err(DerError::ReservedLength));
    }

    #[test]
    fn a_high_tag_number_reads_and_a_padded_one_does_not() {
        // [31] primitive, context-specific: 0x9F 0x1F, empty.
        let data = unhex("9F 1F 00");
        let tlv = one(&data).expect("a high tag number");
        assert_eq!(tlv.tag(), 31);
        assert_eq!(tlv.class(), Class::ContextSpecific);
        // The same tag with a redundant leading octet.
        assert_eq!(one(&unhex("9F 80 1F 00")), Err(DerError::NonMinimalTag));
        // Tag 5 written the long way, which the low-tag form encodes.
        assert_eq!(one(&unhex("9F 05 00")), Err(DerError::NonMinimalTag));
    }

    #[test]
    fn nesting_past_the_cap_is_refused() {
        // Twelve nested one-byte SEQUENCEs, walked under a cap of four.
        let mut data = Vec::new();
        for depth in 0..12u8 {
            data.push(0x30);
            data.push(2 * (11 - depth));
        }
        let budget = Budget::new(Limits::new(4, 1_000));
        let mut cursor = Cursor::new(&data, &budget);
        let mut node = cursor.read().expect("the outermost SEQUENCE");
        let mut reached = 0u32;
        loop {
            match node.children(&budget) {
                Ok(mut inner) => match inner.read() {
                    Ok(next) => {
                        node = next;
                        reached = reached.saturating_add(1);
                    }
                    Err(error) => panic!("unexpected {error:?}"),
                },
                Err(error) => {
                    assert_eq!(error, DerError::DepthExceeded);
                    break;
                }
            }
        }
        assert_eq!(reached, 4, "four descents, then the cap");
    }

    #[test]
    fn the_node_budget_stops_a_wide_structure() {
        let data = unhex("05 00").repeat(40);
        let budget = Budget::new(Limits::new(8, 10));
        let mut cursor = Cursor::new(&data, &budget);
        for _ in 0..10 {
            cursor.read().expect("within budget");
        }
        assert_eq!(cursor.read(), Err(DerError::NodeBudgetExceeded));
        assert_eq!(budget.spent(), 10);
    }

    #[test]
    fn a_boolean_is_canonical_or_refused() {
        assert_eq!(one(&unhex("01 01 FF")).and_then(|t| t.as_bool()), Ok(true));
        assert_eq!(one(&unhex("01 01 00")).and_then(|t| t.as_bool()), Ok(false));
        // BER would read 0x01 as true; DER §11.1 has exactly one spelling.
        assert_eq!(
            one(&unhex("01 01 01")).and_then(|t| t.as_bool()),
            Err(DerError::NonCanonicalBoolean)
        );
        assert_eq!(
            one(&unhex("01 00")).and_then(|t| t.as_bool()),
            Err(DerError::NonCanonicalBoolean)
        );
    }

    #[test]
    fn a_negative_integer_reads_as_one_and_refuses_a_magnitude() {
        let data = unhex("02 01 FF");
        let tlv = one(&data).expect("an INTEGER");
        let integer = tlv.as_integer().expect("minimal");
        assert!(integer.is_negative());
        assert_eq!(integer.as_i64(), Ok(-1));
        assert_eq!(integer.as_u64(), Err(DerError::NegativeInteger));
        assert_eq!(integer.magnitude(), Err(DerError::NegativeInteger));

        // A serial number written negative by an issuer who forgot the sign
        // octet: -128, not 128.
        let data = unhex("02 01 80");
        let tlv = one(&data).expect("an INTEGER");
        assert_eq!(tlv.as_integer().and_then(|i| i.as_i64()), Ok(-128));
    }

    #[test]
    fn a_padded_integer_is_refused() {
        assert_eq!(
            one(&unhex("02 02 00 7F")).and_then(|t| t.as_integer()),
            Err(DerError::NonMinimalInteger)
        );
        assert_eq!(
            one(&unhex("02 02 FF 80")).and_then(|t| t.as_integer()),
            Err(DerError::NonMinimalInteger)
        );
        assert_eq!(
            one(&unhex("02 00")).and_then(|t| t.as_integer()),
            Err(DerError::NonMinimalInteger)
        );
        // The sign octet a positive value genuinely needs stays legal, and
        // `magnitude` takes it back off.
        let data = unhex("02 02 00 80");
        let tlv = one(&data).expect("an INTEGER");
        let integer = tlv.as_integer().expect("minimal");
        assert_eq!(integer.magnitude(), Ok(&[0x80u8][..]));
        assert_eq!(integer.as_u64(), Ok(128));
    }

    #[test]
    fn an_integer_wider_than_u64_is_refused_rather_than_truncated() {
        let data = unhex("02 09 01 02 03 04 05 06 07 08 09");
        let tlv = one(&data).expect("an INTEGER");
        assert_eq!(
            tlv.as_integer().and_then(|i| i.as_u64()),
            Err(DerError::IntegerTooLarge)
        );
        assert_eq!(
            tlv.as_integer().expect("minimal").to_hex(),
            "010203040506070809"
        );
    }

    #[test]
    fn a_bit_string_with_too_many_unused_bits_is_refused() {
        assert_eq!(
            one(&unhex("03 02 08 FF")).and_then(|t| t.as_bit_string()),
            Err(DerError::BitStringUnusedBits { count: 8 })
        );
        // Unused bits with nothing to be unused in.
        assert_eq!(
            one(&unhex("03 01 03")).and_then(|t| t.as_bit_string()),
            Err(DerError::BitStringUnusedBits { count: 3 })
        );
        // Not even the count.
        assert_eq!(
            one(&unhex("03 00")).and_then(|t| t.as_bit_string()),
            Err(DerError::BitStringEmpty)
        );
    }

    #[test]
    fn bit_string_bits_are_numbered_from_the_left() {
        // RFC 5280 Appendix C.1's keyUsage: '0000011'B, one unused bit.
        let data = unhex("03 02 01 06");
        let tlv = one(&data).expect("a BIT STRING");
        let bits = tlv.as_bit_string().expect("in range");
        assert_eq!(bits.bit_len(), 7);
        assert!(!bits.bit(0), "digitalSignature");
        assert!(bits.bit(5), "keyCertSign");
        assert!(bits.bit(6), "cRLSign");
        assert!(!bits.bit(7), "past the end is unset, not undefined");
        assert_eq!(bits.whole_bytes(), Err(DerError::BitStringNotWhole));
    }

    #[test]
    fn an_oid_round_trips_to_dotted_decimal() {
        // rsaEncryption, whose 113549 arc needs three continuation octets.
        let data = unhex("06 09 2A 86 48 86 F7 0D 01 01 01");
        let tlv = one(&data).expect("an OID");
        let oid = tlv.as_oid().expect("valid");
        assert_eq!(oid.to_dotted(), "1.2.840.113549.1.1.1");

        // domainComponent, whose 19200300 arc needs four.
        let data = unhex("06 0A 09 92 26 89 93 F2 2C 64 01 19");
        let tlv = one(&data).expect("an OID");
        assert_eq!(
            tlv.as_oid().expect("valid").to_dotted(),
            "0.9.2342.19200300.100.1.25"
        );

        // The first octet packs two arcs, and 2.5 is 85 rather than 45.
        let data = unhex("06 03 55 1D 0E");
        let tlv = one(&data).expect("an OID");
        assert_eq!(tlv.as_oid().expect("valid").to_dotted(), "2.5.29.14");

        // A first arc of 2 with a large second arc: 2.100.3.
        let data = unhex("06 03 81 34 03");
        let tlv = one(&data).expect("an OID");
        assert_eq!(tlv.as_oid().expect("valid").to_dotted(), "2.100.3");
    }

    #[test]
    fn an_oid_whose_last_arc_never_ends_is_refused() {
        assert_eq!(
            one(&unhex("06 02 2A 86")).and_then(|t| t.as_oid()),
            Err(DerError::OidUnterminated)
        );
        assert_eq!(
            one(&unhex("06 00")).and_then(|t| t.as_oid()),
            Err(DerError::OidEmpty)
        );
        // A padded arc: 0x80 leads a subidentifier that says nothing.
        assert_eq!(
            one(&unhex("06 03 2A 80 01")).and_then(|t| t.as_oid()),
            Err(DerError::OidNonMinimalArc)
        );
        // Nine continuation octets and a terminator: seventy bits of arc.
        assert_eq!(
            one(&unhex("06 0B 2A 81 81 81 81 81 81 81 81 81 01")).and_then(|t| t.as_oid()),
            Err(DerError::OidArcTooLarge)
        );
    }

    #[test]
    fn strings_decode_by_their_own_tag() {
        let data = unhex("0C 05 68 C3 A9 6C 6F");
        let utf8 = one(&data).expect("a UTF8String");
        assert_eq!(utf8.as_string().as_deref(), Ok("hélo"));

        let data = unhex("13 0A 45 78 61 6D 70 6C 65 20 43 41");
        let printable = one(&data).expect("printable");
        assert_eq!(printable.as_string().as_deref(), Ok("Example CA"));

        // BMPString: 'Aé' as UCS-2 big-endian.
        let data = unhex("1E 04 00 41 00 E9");
        let bmp = one(&data).expect("a BMPString");
        assert_eq!(bmp.as_string().as_deref(), Ok("Aé"));

        // UniversalString: 'A' as UCS-4 big-endian.
        let data = unhex("1C 04 00 00 00 41");
        let universal = one(&data).expect("a UniversalString");
        assert_eq!(universal.as_string().as_deref(), Ok("A"));

        // TeletexString, read as ISO 8859-1 — the approximation this file
        // records rather than hides.
        let data = unhex("14 02 41 E9");
        let teletex = one(&data).expect("a TeletexString");
        assert_eq!(teletex.as_string().as_deref(), Ok("Aé"));
    }

    #[test]
    fn malformed_strings_are_refused_rather_than_repaired() {
        assert_eq!(
            one(&unhex("0C 02 FF FE")).and_then(|t| t.as_string()),
            Err(DerError::BadUtf8String)
        );
        assert_eq!(
            one(&unhex("16 02 41 80")).and_then(|t| t.as_string()),
            Err(DerError::NonAsciiString { tag: 22 })
        );
        assert_eq!(
            one(&unhex("1E 03 00 41 00")).and_then(|t| t.as_string()),
            Err(DerError::BadWideString { tag: 30 })
        );
        // D800 is a surrogate, which UCS-2 does not have.
        assert_eq!(
            one(&unhex("1E 02 D8 00")).and_then(|t| t.as_string()),
            Err(DerError::BadWideChar { tag: 30 })
        );
        assert_eq!(
            one(&unhex("05 00")).and_then(|t| t.as_string()),
            Err(DerError::NotAString { tag: 5 })
        );
    }

    #[test]
    fn utc_time_uses_the_profile_century_rule() {
        // RFC 5280 Appendix C.1's notBefore.
        let data = unhex("17 0D 30 34 30 34 33 30 31 34 32 35 33 34 5A");
        let tlv = one(&data).expect("a UTCTime");
        // 2004-04-30T14:25:34Z: 12 538 days from the epoch by the era
        // arithmetic above, and never by a clock.
        assert_eq!(tlv.as_time(), Ok(1_083_335_134));

        // 49 is 2049 and 50 is 1950, and neither depends on the year it is
        // read in.
        let y49 = unhex("17 0D 34 39 30 31 30 31 30 30 30 30 30 30 5A");
        let y50 = unhex("17 0D 35 30 30 31 30 31 30 30 30 30 30 30 5A");
        assert_eq!(one(&y49).and_then(|t| t.as_time()), Ok(2_493_072_000));
        assert_eq!(one(&y50).and_then(|t| t.as_time()), Ok(-631_152_000));
    }

    #[test]
    fn generalized_time_reads_and_the_epoch_lands_on_zero() {
        let epoch = unhex("18 0F 31 39 37 30 30 31 30 31 30 30 30 30 30 30 5A");
        assert_eq!(one(&epoch).and_then(|t| t.as_time()), Ok(0));
        // A leap day, so the month table is exercised rather than assumed.
        let leap = unhex("18 0F 32 30 32 30 30 32 32 39 31 32 30 30 30 30 5A");
        assert_eq!(one(&leap).and_then(|t| t.as_time()), Ok(1_582_977_600));
    }

    #[test]
    fn malformed_times_are_refused_by_reason() {
        // No seconds, which RFC 5280 §4.1.2.5.1 requires.
        let short = unhex("17 0B 30 34 30 34 33 30 31 34 32 35 5A");
        assert_eq!(
            one(&short).and_then(|t| t.as_time()),
            Err(DerError::MalformedTime(TimeFault::Length))
        );
        // A local-time offset instead of Zulu.
        let offset = unhex("17 0D 30 34 30 34 33 30 31 34 32 35 33 34 2B");
        assert_eq!(
            one(&offset).and_then(|t| t.as_time()),
            Err(DerError::MalformedTime(TimeFault::NotZulu))
        );
        // Month 13.
        let month = unhex("17 0D 30 34 31 33 33 30 31 34 32 35 33 34 5A");
        assert_eq!(
            one(&month).and_then(|t| t.as_time()),
            Err(DerError::MalformedTime(TimeFault::FieldOutOfRange))
        );
        // The 29th of February in a year that has none.
        let day = unhex("18 0F 32 30 32 31 30 32 32 39 30 30 30 30 30 30 5A");
        assert_eq!(
            one(&day).and_then(|t| t.as_time()),
            Err(DerError::MalformedTime(TimeFault::FieldOutOfRange))
        );
        // A letter where a digit belongs.
        let letters = unhex("17 0D 30 34 30 34 33 30 31 34 32 35 33 5A 5A");
        assert_eq!(
            one(&letters).and_then(|t| t.as_time()),
            Err(DerError::MalformedTime(TimeFault::NotDigits))
        );
        // Second 60: X.680's leap second, refused deliberately.
        let leap_second = unhex("17 0D 30 34 30 34 33 30 31 34 32 35 36 30 5A");
        assert_eq!(
            one(&leap_second).and_then(|t| t.as_time()),
            Err(DerError::MalformedTime(TimeFault::FieldOutOfRange))
        );
    }

    #[test]
    fn a_wrong_form_is_refused_in_both_directions() {
        // A constructed OCTET STRING: BER's segmented form, forbidden in DER.
        assert_eq!(
            one(&unhex("24 03 04 01 41")).and_then(|t| t.as_octet_string()),
            Err(DerError::WrongForm {
                tag: 4,
                constructed: true
            })
        );
        // A primitive SEQUENCE.
        assert_eq!(
            one(&unhex("10 00")).and_then(|t| {
                t.require(Tag::Sequence)?;
                Ok(())
            }),
            Err(DerError::WrongForm {
                tag: 16,
                constructed: false
            })
        );
        // And descending into something primitive.
        let budget = Budget::default();
        let data = unhex("04 01 41");
        let tlv = one(&data).expect("an OCTET STRING");
        assert!(matches!(
            tlv.children(&budget),
            Err(DerError::WrongForm { .. })
        ));
    }

    #[test]
    fn explicit_and_implicit_tags_read_differently() {
        let budget = Budget::default();

        // `[0] EXPLICIT INTEGER 2`, the certificate version field.
        let data = unhex("A0 03 02 01 02");
        let mut cursor = Cursor::new(&data, &budget);
        let wrapper = cursor.read().expect("a context tag");
        assert!(wrapper.is_context(0));
        let inner = wrapper.explicit(&budget).expect("one value inside");
        assert_eq!(inner.as_integer().and_then(|i| i.as_u64()), Ok(2));

        // Two values inside an explicit wrapper is trailing data, not a
        // choice between them.
        let data = unhex("A0 06 02 01 02 02 01 03");
        let mut cursor = Cursor::new(&data, &budget);
        let wrapper = cursor.read().expect("a context tag");
        assert_eq!(wrapper.explicit(&budget), Err(DerError::TrailingBytes));

        // `[0] IMPLICIT OCTET STRING`, as authorityKeyIdentifier writes its
        // keyIdentifier: the tag was replaced, the content was not.
        let data = unhex("80 03 01 02 03");
        let mut cursor = Cursor::new(&data, &budget);
        let tagged = cursor.read().expect("a context tag");
        assert_eq!(
            tagged.implicit(Tag::OctetString).as_octet_string(),
            Ok(&[1u8, 2, 3][..])
        );
    }

    #[test]
    fn trailing_bytes_are_refused_rather_than_ignored() {
        let data = unhex("05 00 05 00");
        let budget = Budget::default();
        let mut cursor = Cursor::new(&data, &budget);
        cursor.read().expect("the first NULL");
        assert_eq!(cursor.finish(), Err(DerError::TrailingBytes));
        assert_eq!(
            one(&unhex("05 01 00")).and_then(|t| t.as_null()),
            Err(DerError::NullNotEmpty)
        );
    }

    #[test]
    fn an_empty_cursor_says_so_rather_than_panicking() {
        let budget = Budget::default();
        let mut cursor = Cursor::new(&[], &budget);
        assert!(cursor.peek().is_none());
        assert_eq!(cursor.read(), Err(DerError::UnexpectedEnd));
        assert_eq!(cursor.expect_optional(Tag::Integer), Ok(None));
        assert_eq!(cursor.context_optional(0), Ok(None));
    }

    #[test]
    fn every_prefix_of_a_real_certificate_refuses_rather_than_panics() {
        // Ruling 1 as a unit test rather than only as a fuzz target: the
        // fuzzer finds inputs nobody wrote down, and this pins the family
        // every truncation lands in.
        let full = crate::x509::tests::rfc5280_c1();
        for cut in 0..full.len() {
            let budget = Budget::default();
            let mut cursor = Cursor::new(&full[..cut], &budget);
            while let Ok(node) = cursor.read() {
                if node.is_constructed() {
                    if let Ok(mut inner) = node.children(&budget) {
                        while inner.read().is_ok() {}
                    }
                }
            }
        }
    }
}
