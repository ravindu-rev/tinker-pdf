//! CMS `EnvelopedData` (RFC 5652 §6), which is what a PDF's public-key
//! security handler puts in `/Recipients` (ISO 32000-1 7.6.5).
//!
//! A signature's `SignedData` and an encrypted document's `EnvelopedData` are
//! the same shape used for opposite purposes, and the difference decides what
//! this module may do. A `SignedData` is read to find out whether something is
//! true; an `EnvelopedData` is read to find out **which of several sealed
//! copies of a key this caller can open**. So the whole of this module is
//! locating things — which recipient, which encrypted key, which algorithm —
//! and none of it is arithmetic.
//!
//! # The private key does not come in here
//!
//! Unwrapping a `KeyTransRecipientInfo` needs the recipient's RSA *private*
//! key. `docs/design/signatures.md` makes "no key material in the engine" a
//! non-goal-by-design for signing, and the same rule holds here for exactly
//! the same reason: a library that holds a private key has to be trusted with
//! key storage, passphrase handling and PKCS#8 parsing, none of which is PDF
//! work. So this module hands the caller the encrypted key and the identifier
//! saying whose it is, and the caller does the one operation only it can do.
//!
//! # What is deliberately not decoded
//!
//! `KeyAgreeRecipientInfo`, `KEKRecipientInfo`, `PasswordRecipientInfo` and
//! `OtherRecipientInfo` (§6.2.2 through §6.2.5) are recognised by their tag
//! and refused by name. Every PDF public-key handler in the wild uses key
//! transport, and a recipient shape read but never unwrappable would be a
//! parser surface bought for nothing.
//!
//! # And what has no corpus behind it at all
//!
//! **Not one of the 4 594 fetched corpus files uses the public-key handler.**
//! Every fixture this module has was written here. The `EnvelopedData` parsing
//! is checked against envelopes OpenSSL produced, which is real evidence about
//! the structure; the ISO 32000-1 7.6.5 key derivation on top of it is checked
//! against a second implementation written from the same clause by the same
//! author, which catches a transcription slip and cannot catch a misreading.
//! `docs/features/encryption.md` says so where a caller will see it.

use crate::der::{Budget, Class, Cursor, DerError, Int, Limits, Tag, Tlv};
use crate::oid;
use crate::x509::AlgorithmIdentifier;

/// Why an `EnvelopedData` could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnvelopedError {
    /// The encoding itself.
    Der(DerError),
    /// Bytes after the `ContentInfo` SEQUENCE.
    TrailingBytes,
    /// A `ContentInfo` whose `contentType` is not `id-envelopedData` (§6.1).
    UnsupportedContentType {
        /// The dotted OID that was there instead.
        oid: String,
    },
    /// §6.1 fixes the version at 0, 2, 3 or 4 depending on what the structure
    /// contains. A version outside that set is a structure this build has no
    /// reading for.
    UnsupportedVersion {
        /// What the file said.
        version: i64,
    },
    /// `recipientInfos` is `SET SIZE (1..MAX)` and this one is empty, so the
    /// message is addressed to nobody.
    NoRecipients,
    /// A `RecipientInfo` that is not a `KeyTransRecipientInfo` (§6.2.1).
    ///
    /// Named rather than skipped: a document addressed only to recipients this
    /// build cannot read is a document it cannot open, and saying which shape
    /// it met is the difference between a bug report and a shrug.
    UnsupportedRecipientKind {
        /// The context tag `[n]` that was there. §6.2 assigns 1 to
        /// key agreement, 2 to KEK, 3 to password and 4 to other.
        tag: u32,
    },
    /// A `RecipientIdentifier` that is neither an `IssuerAndSerialNumber` nor
    /// `[0] subjectKeyIdentifier` (§6.2.1).
    UnknownRecipientIdentifier {
        /// The class it carried.
        class: Class,
        /// The tag it carried.
        tag: u32,
    },
}

impl From<DerError> for EnvelopedError {
    fn from(error: DerError) -> Self {
        Self::Der(error)
    }
}

impl core::fmt::Display for EnvelopedError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            EnvelopedError::Der(error) => write!(f, "{error:?}"),
            EnvelopedError::TrailingBytes => f.write_str("bytes after the ContentInfo"),
            EnvelopedError::UnsupportedContentType { oid } => {
                write!(f, "content type {oid} is not id-envelopedData")
            }
            EnvelopedError::UnsupportedVersion { version } => {
                write!(f, "EnvelopedData version {version}")
            }
            EnvelopedError::NoRecipients => f.write_str("no recipients"),
            EnvelopedError::UnsupportedRecipientKind { tag } => {
                write!(f, "recipient kind [{tag}] is not key transport")
            }
            EnvelopedError::UnknownRecipientIdentifier { class, tag } => {
                write!(f, "recipient identifier {class:?} [{tag}]")
            }
        }
    }
}

impl std::error::Error for EnvelopedError {}

/// Which recipient a `KeyTransRecipientInfo` is for (§6.2.1).
///
/// The same two shapes a `SignerIdentifier` has, and for the same reason: a
/// producer copies whichever one the recipient's certificate makes cheap.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecipientIdentifier<'a> {
    /// `IssuerAndSerialNumber`.
    IssuerAndSerialNumber {
        /// The issuer, as its stored DER — compared rather than decoded,
        /// because matching is what it is for.
        issuer: &'a [u8],
        /// The serial number's content octets.
        serial: Int<'a>,
        /// The whole SEQUENCE.
        der: &'a [u8],
    },
    /// `[0] subjectKeyIdentifier`, IMPLICIT over an OCTET STRING.
    SubjectKeyIdentifier(&'a [u8]),
}

/// One sealed copy of the content-encryption key (§6.2.1).
#[derive(Clone, Debug)]
pub struct KeyTransRecipient<'a> {
    version: i64,
    rid: RecipientIdentifier<'a>,
    algorithm: AlgorithmIdentifier<'a>,
    encrypted_key: &'a [u8],
}

impl<'a> KeyTransRecipient<'a> {
    /// `version`, which §6.2.1 ties to which identifier shape is used: 0 with
    /// `issuerAndSerialNumber` and 2 with `subjectKeyIdentifier`.
    #[must_use]
    pub const fn version(&self) -> i64 {
        self.version
    }

    /// Whose key this copy is sealed to.
    #[must_use]
    pub const fn rid(&self) -> &RecipientIdentifier<'a> {
        &self.rid
    }

    /// The key-encryption algorithm.
    #[must_use]
    pub const fn algorithm(&self) -> AlgorithmIdentifier<'a> {
        self.algorithm
    }

    /// Whether the key-encryption algorithm is PKCS#1 key transport, which is
    /// the only one any PDF public-key handler uses.
    #[must_use]
    pub fn is_rsa(&self) -> bool {
        self.algorithm.oid() == oid::RSA_ENCRYPTION
    }

    /// The sealed key, for the caller's private-key operation.
    ///
    /// Opaque here by design: unsealing it is the one thing this crate refuses
    /// to be able to do.
    #[must_use]
    pub const fn encrypted_key(&self) -> &'a [u8] {
        self.encrypted_key
    }
}

/// An `EnvelopedData` (§6.1), read only as far as finding a recipient.
#[derive(Clone, Debug)]
pub struct EnvelopedData<'a> {
    version: i64,
    recipients: Vec<KeyTransRecipient<'a>>,
    unsupported: Vec<u32>,
    content_algorithm: AlgorithmIdentifier<'a>,
    encrypted_content: Option<&'a [u8]>,
    der: &'a [u8],
}

impl<'a> EnvelopedData<'a> {
    /// Reads a `ContentInfo` carrying an `EnvelopedData`.
    ///
    /// # Errors
    /// See [`EnvelopedError`].
    pub fn parse(der: &'a [u8]) -> Result<Self, EnvelopedError> {
        Self::parse_with(der, Limits::CMS)
    }

    /// Reads one under ceilings of the caller's choosing.
    ///
    /// # Errors
    /// See [`EnvelopedError`].
    pub fn parse_with(der: &'a [u8], limits: Limits) -> Result<Self, EnvelopedError> {
        let budget = Budget::new(limits);
        let mut outer = Cursor::new(der, &budget);
        let info = outer.expect(Tag::Sequence)?;
        if !outer.is_empty() {
            return Err(EnvelopedError::TrailingBytes);
        }

        let mut fields = info.children(&budget)?;
        let content_type = fields.read()?.as_oid()?;
        if content_type != oid::ID_ENVELOPED_DATA {
            return Err(EnvelopedError::UnsupportedContentType {
                oid: content_type.to_dotted(),
            });
        }
        let content = fields
            .context_optional(0)?
            .ok_or(EnvelopedError::UnsupportedVersion { version: -1 })?;
        let enveloped = content.explicit(&budget)?;
        enveloped.require(Tag::Sequence)?;
        fields.finish()?;

        Self::from_body(&enveloped, &budget, der)
    }

    fn from_body(body: &Tlv<'a>, budget: &Budget, der: &'a [u8]) -> Result<Self, EnvelopedError> {
        let mut fields = body.children(budget)?;
        let version = fields.read()?.as_integer()?.as_i64().unwrap_or(-1);
        if !matches!(version, 0 | 2 | 3 | 4) {
            return Err(EnvelopedError::UnsupportedVersion { version });
        }

        // `[0] originatorInfo` is optional and this build reads nothing from
        // it: it carries certificates and CRLs for key agreement, which is the
        // recipient shape refused below.
        let mut next = fields.read()?;
        if next.is_context(0) {
            next = fields.read()?;
        }

        // `recipientInfos SET OF RecipientInfo`.
        next.require(Tag::Set)?;
        let mut recipients = Vec::new();
        let mut unsupported = Vec::new();
        let mut infos = next.children(budget)?;
        while !infos.is_empty() {
            let info = infos.read()?;
            match read_recipient(&info, budget) {
                Ok(recipient) => recipients.push(recipient),
                Err(EnvelopedError::UnsupportedRecipientKind { tag }) => unsupported.push(tag),
                Err(error) => return Err(error),
            }
        }
        if recipients.is_empty() && unsupported.is_empty() {
            return Err(EnvelopedError::NoRecipients);
        }

        // `encryptedContentInfo`: the content type, the algorithm, and
        // `[0] encryptedContent` — which a PDF's `/Recipients` leaves absent,
        // because the "content" is the file key and it is not carried here.
        let encrypted = fields.read()?;
        encrypted.require(Tag::Sequence)?;
        let mut parts = encrypted.children(budget)?;
        let _content_type = parts.read()?.as_oid()?;
        let content_algorithm = AlgorithmIdentifier::parse(&parts.read()?, budget)?;
        let encrypted_content = parts.context_optional(0)?.map(|node| node.value());

        Ok(EnvelopedData {
            version,
            recipients,
            unsupported,
            content_algorithm,
            encrypted_content,
            der,
        })
    }

    /// `version` (§6.1).
    #[must_use]
    pub const fn version(&self) -> i64 {
        self.version
    }

    /// Every key-transport recipient, in the order the file lists them.
    #[must_use]
    pub fn recipients(&self) -> &[KeyTransRecipient<'a>] {
        &self.recipients
    }

    /// The context tags of the recipients this build declined to read.
    ///
    /// Non-empty and `recipients()` empty together means a message addressed
    /// only to shapes this build cannot open, which is a different answer from
    /// "no recipient matched the key you offered".
    #[must_use]
    pub fn unsupported_recipients(&self) -> &[u32] {
        &self.unsupported
    }

    /// The content-encryption algorithm the envelope declares.
    #[must_use]
    pub const fn content_algorithm(&self) -> AlgorithmIdentifier<'a> {
        self.content_algorithm
    }

    /// `encryptedContent`, absent in a PDF `/Recipients` envelope.
    #[must_use]
    pub const fn encrypted_content(&self) -> Option<&'a [u8]> {
        self.encrypted_content
    }

    /// The bytes this was read from.
    #[must_use]
    pub const fn der(&self) -> &'a [u8] {
        self.der
    }
}

fn read_recipient<'a>(
    info: &Tlv<'a>,
    budget: &Budget,
) -> Result<KeyTransRecipient<'a>, EnvelopedError> {
    // §6.2: `ktri` is the untagged SEQUENCE alternative; every other shape
    // carries a context tag naming which it is.
    if info.class() == Class::ContextSpecific {
        return Err(EnvelopedError::UnsupportedRecipientKind { tag: info.tag() });
    }
    info.require(Tag::Sequence)?;

    let mut fields = info.children(budget)?;
    let version = fields.read()?.as_integer()?.as_i64().unwrap_or(-1);

    let rid_node = fields.read()?;
    let rid = if rid_node.class() == Class::ContextSpecific {
        if !rid_node.is_context(0) {
            return Err(EnvelopedError::UnknownRecipientIdentifier {
                class: rid_node.class(),
                tag: rid_node.tag(),
            });
        }
        RecipientIdentifier::SubjectKeyIdentifier(rid_node.value())
    } else {
        rid_node.require(Tag::Sequence)?;
        let mut parts = rid_node.children(budget)?;
        let issuer = parts.read()?.raw();
        let serial = parts.read()?.as_integer()?;
        parts.finish()?;
        RecipientIdentifier::IssuerAndSerialNumber {
            issuer,
            serial,
            der: rid_node.raw(),
        }
    };

    let algorithm = AlgorithmIdentifier::parse(&fields.read()?, budget)?;
    let encrypted_key = fields.read()?.as_octet_string()?;
    fields.finish()?;

    Ok(KeyTransRecipient {
        version,
        rid,
        algorithm,
        encrypted_key,
    })
}
