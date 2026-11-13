//! The cos reading of the validate-on-save helper
//! (`docs/design/validate-on-save.md`): how a `tinker-pdf-cos` test holds what
//! it wrote.
//!
//! A test reaches it with `mod written_support;`, and a `src` test module of
//! this crate by `#[path]`, through the `#[cfg(test)]` self-alias in
//! `src/lib.rs`. It opens the bytes with `CosDocument::open`, authenticates,
//! runs `tinker_pdf_cos::validate` and hands each finding to the policy in
//! `held.rs`, which the facade reading compiles too. The doors hand back the
//! document they opened — their own copy, so nothing a test pins moves when it
//! starts calling one.
//!
//! The profile validators are the facade's, and this crate cannot reach the
//! facade even for its tests. So this reading does not pass what it cannot
//! hold: a document that might claim PDF/A, PDF/UA or PDF/X fails here,
//! naming the facade tree, whose reading holds the claim.

#![allow(dead_code)]

#[path = "held.rs"]
pub mod held;

use std::sync::Arc;

use tinker_pdf_cos::{CosDocument, Defect, DefectKind, Name, Recipient, Tier};

/// How an encrypted document is opened: the two ways this crate authenticates.
pub enum Key<'k> {
    /// A password, user or owner (7.6.4).
    Password(&'k str),
    /// The caller's key for a public-key document (7.6.5).
    Recipient(&'k dyn Recipient),
}

/// An encrypted document and the key that opens it.
pub struct Locked<'k> {
    /// The file's bytes.
    pub bytes: Vec<u8>,
    /// What opens them.
    pub key: Key<'k>,
}

/// What a door is handed: a document's bytes, with their key when they are
/// locked. Built from the bytes or from a [`Locked`].
pub struct Handed<'k> {
    bytes: Arc<[u8]>,
    key: Option<Key<'k>>,
}

impl From<Vec<u8>> for Handed<'_> {
    fn from(bytes: Vec<u8>) -> Self {
        Handed {
            bytes: bytes.into(),
            key: None,
        }
    }
}

impl From<&Vec<u8>> for Handed<'_> {
    fn from(bytes: &Vec<u8>) -> Self {
        Handed::from(bytes.as_slice())
    }
}

impl From<&[u8]> for Handed<'_> {
    fn from(bytes: &[u8]) -> Self {
        Handed {
            bytes: bytes.into(),
            key: None,
        }
    }
}

impl From<Arc<[u8]>> for Handed<'_> {
    fn from(bytes: Arc<[u8]>) -> Self {
        Handed { bytes, key: None }
    }
}

impl<'k> From<Locked<'k>> for Handed<'k> {
    fn from(locked: Locked<'k>) -> Self {
        Handed {
            bytes: locked.bytes.into(),
            key: Some(locked.key),
        }
    }
}

/// A finding a test writes on purpose, for [`expecting`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expected {
    /// A strict-validator finding, of either tier.
    Defect(DefectKind),
}

/// Holds a document this engine wrote from nothing it did not write: built by
/// `DocumentBuilder`, or saved from an input that was itself held as authored.
/// Every verdict is empty, which includes having opened on `Trust` with no
/// repair.
#[track_caller]
pub fn authored<'k>(label: &str, bytes: impl Into<Handed<'k>>) -> CosDocument {
    let (doc, read) = read(label, "authored", bytes.into());
    held::authored(label, "authored", &read);
    doc
}

/// Holds `output` as a save of `input`, a file this engine did not write: a
/// committed fixture, a hand-built or damaged file. See `held::rewritten`.
#[track_caller]
pub fn rewritten<'i, 'o>(
    label: &str,
    input: impl Into<Handed<'i>>,
    output: impl Into<Handed<'o>>,
) -> CosDocument {
    let (_, before) = read(&format!("{label}, its input"), "rewritten", input.into());
    let (doc, after) = read(label, "rewritten", output.into());
    held::rewritten(label, &before, &after);
    doc
}

/// Holds a document damaged on purpose to exactly the findings `expected`
/// lists. An empty list is [`authored`].
#[track_caller]
pub fn expecting<'k>(
    label: &str,
    bytes: impl Into<Handed<'k>>,
    expected: &[Expected],
) -> CosDocument {
    let (doc, read) = read(label, "expecting", bytes.into());
    let wants: Vec<held::Want> = expected.iter().map(want).collect();
    held::expecting(label, &read, &wants);
    doc
}

/// Every finding this reading holds a document to, held to nothing: how a
/// test compares this reading with the facade's.
#[track_caller]
pub fn items<'k>(label: &str, bytes: impl Into<Handed<'k>>) -> Vec<held::Item> {
    read(label, "items", bytes.into()).1.items
}

/// Opens, authenticates and validates one document.
#[track_caller]
fn read(label: &str, door: &str, handed: Handed<'_>) -> (CosDocument, held::Read) {
    let bytes = Arc::clone(&handed.bytes);
    let doc = match CosDocument::open(Arc::clone(&bytes)) {
        Ok(doc) => doc,
        Err(error) => held::refuse(label, door, &format!("the bytes do not open: {error:?}")),
    };
    unlock(label, door, &doc, handed.key);
    if let Some(claim) = claim(&doc) {
        held::refuse(
            label,
            door,
            &format!(
                "{claim}, so it may claim PDF/A, PDF/UA or PDF/X, and the cos reading runs \
                 no profile validator: hold it from the facade tree, through \
                 `crates/tinker-pdf/tests/written_support`"
            ),
        );
    }
    let items = tinker_pdf_cos::validate(&doc).iter().map(item).collect();
    let read = held::Read {
        bytes,
        items,
        notes: Vec::new(),
    };
    (doc, read)
}

/// Authenticates an encrypted document with its key, or with the empty user
/// password when it was handed over without one: `validate` reads a stream it
/// cannot decrypt as one that does not decode.
#[track_caller]
fn unlock(label: &str, door: &str, doc: &CosDocument, key: Option<Key<'_>>) {
    match (doc.is_encrypted(), key) {
        (false, None) => {}
        (false, Some(_)) => held::refuse(
            label,
            door,
            "it was handed over with a key and is not encrypted",
        ),
        (true, Some(Key::Password(password))) => {
            if let Err(error) = doc.authenticate(password) {
                held::refuse(
                    label,
                    door,
                    &format!("its password does not open it: {error:?}"),
                );
            }
        }
        (true, Some(Key::Recipient(recipient))) => {
            if let Err(error) = doc.authenticate_with_recipient(recipient) {
                held::refuse(
                    label,
                    door,
                    &format!("its recipient does not open it: {error:?}"),
                );
            }
        }
        (true, None) => {
            if doc.authenticate("").is_err() {
                held::refuse(
                    label,
                    door,
                    "it is encrypted and the empty user password does not open it: hand it \
                     over as `Locked { bytes, key }`",
                );
            }
        }
    }
}

/// Whether the document might claim a profile, over-reading every claim the
/// facade's readers make: an identification schema's prefix or namespace in
/// the catalog's decoded `/Metadata` packet, in ASCII or in UTF-16 of either
/// byte order, or a `/GTS_PDFXVersion` key in `/Info` whatever its value.
fn claim(doc: &CosDocument) -> Option<String> {
    if let Some(packet) = tinker_pdf_cos::xmp_metadata(doc) {
        for spelling in [
            "pdfaid",
            "pdfuaid",
            "www.aiim.org/pdfa/ns/id/",
            "www.aiim.org/pdfua/ns/id/",
        ] {
            if spelled(&packet, spelling) {
                return Some(format!("its XMP packet names `{spelling}`"));
            }
        }
    }
    let info = doc.resolve_key(doc.trailer(), Name::INFO);
    let version = doc.intern(b"GTS_PDFXVersion");
    if info
        .as_dict()
        .is_some_and(|dict| dict.contains_key(version))
    {
        return Some("its `/Info` carries `/GTS_PDFXVersion`".to_string());
    }
    None
}

/// Whether `text`, which is ASCII, occurs in `packet` as ASCII or as UTF-16 of
/// either byte order.
fn spelled(packet: &[u8], text: &str) -> bool {
    let ascii = text.as_bytes().to_vec();
    let little: Vec<u8> = text.bytes().flat_map(|byte| [byte, 0]).collect();
    let big: Vec<u8> = text.bytes().flat_map(|byte| [0, byte]).collect();
    [ascii, little, big].iter().any(|needle| {
        packet
            .windows(needle.len())
            .any(|window| window == needle.as_slice())
    })
}

/// One strict finding as the policy reads it: the structure tier is the
/// writer's, and the kind's label is what survives renumbering, and what a
/// failure names it by.
fn item(defect: &Defect) -> held::Item {
    let tier = defect.kind.tier();
    held::Item {
        verdict: tier.as_str(),
        owned: tier == Tier::Structure,
        exact: format!("{:?}", defect.kind),
        class: defect.kind.as_str().to_string(),
        at: defect.offset,
        shown: format!("[{}] {defect}", defect.kind.as_str()),
    }
}

fn want(expected: &Expected) -> held::Want {
    match expected {
        Expected::Defect(kind) => held::Want {
            verdict: kind.tier().as_str(),
            exact: format!("{kind:?}"),
        },
    }
}
