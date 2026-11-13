//! The facade reading of the validate-on-save helper
//! (`docs/design/validate-on-save.md`): how a facade test, and a test module of
//! the C ABI or the CLI, holds what it wrote.
//!
//! A facade test reaches it with `mod written_support;`; a `src` test module of
//! the facade, the C ABI or the CLI by `#[path]`, the facade's own through the
//! `#[cfg(test)]` self-alias in `src/lib.rs`. It opens the bytes with
//! `Document::open`, authenticates, and runs `Document::validate`, which is
//! `tinker_pdf_cos::validate` over the same parse the cos reading makes, and
//! then each profile the document claims, by that profile's own validator,
//! within the coverage this build implements. Every finding goes to the policy
//! in `crates/tinker-pdf-cos/tests/written_support/held.rs`, which the cos
//! reading compiles too. The doors hand back the document they opened — their
//! own copy, so nothing a test pins moves when it starts calling one.
//!
//! `Document::open` converts what it does not read as a PDF, so this reading
//! fails a document whose `Document::archive()` is `Some`: it holds the bytes
//! it was handed, read as a PDF, or nothing. What a conversion synthesised is
//! held by [`synthesised`], over the PDF it wrote.

#![allow(dead_code)]

#[path = "../../../tinker-pdf-cos/tests/written_support/held.rs"]
pub mod held;

use std::sync::Arc;

use tinker_pdf::{
    ConformanceFinding, Defect, DefectKind, Document, FindingKind, PdfACoverage, PdfAVerdict,
    PdfUaCoverage, PdfUaVerdict, Recipient, Tier, PDFA_STAGED,
};

/// How an encrypted document is opened: the two ways the facade authenticates.
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

/// A finding a test writes on purpose, for [`expecting`]. Typed, so the
/// compiler checks each name and a test states a kind's fields as well.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expected {
    /// A strict-validator finding, of either tier.
    Defect(DefectKind),
    /// A finding of the PDF/A validator, under the claim the document makes.
    PdfA(FindingKind),
    /// A finding of the PDF/UA validator, under the claim the document makes.
    PdfUa(FindingKind),
    /// A finding of the PDF/X validator, under the claim the document makes.
    PdfX(FindingKind),
}

/// Holds a document this engine wrote from nothing it did not write: built by
/// `DocumentBuilder`, or saved from an input that was itself held as authored.
/// Every verdict is empty, which includes having opened on `Trust` with no
/// repair.
#[track_caller]
pub fn authored<'k>(label: &str, bytes: impl Into<Handed<'k>>) -> Document {
    let (doc, read) = read(label, "authored", bytes.into());
    held::authored(label, "authored", &read);
    doc
}

/// Holds `output` as a save of `input`, a file this engine did not write: a
/// committed fixture, a corpus file, a hand-built or damaged file. See
/// `held::rewritten`.
#[track_caller]
pub fn rewritten<'i, 'o>(
    label: &str,
    input: impl Into<Handed<'i>>,
    output: impl Into<Handed<'o>>,
) -> Document {
    let (_, before) = read(&format!("{label}, its input"), "rewritten", input.into());
    let (doc, after) = read(label, "rewritten", output.into());
    held::rewritten(label, &before, &after);
    doc
}

/// Holds a document damaged on purpose to exactly the findings `expected`
/// lists. An empty list is [`authored`].
#[track_caller]
pub fn expecting<'k>(label: &str, bytes: impl Into<Handed<'k>>, expected: &[Expected]) -> Document {
    let (doc, read) = read(label, "expecting", bytes.into());
    let wants: Vec<held::Want> = expected.iter().map(want).collect();
    held::expecting(label, &read, &wants);
    doc
}

/// Holds what a conversion synthesised: the PDF behind `document` is held as
/// [`authored`], read again from its bytes through this reading, and
/// `document` itself is handed back, its report with it.
#[track_caller]
pub fn synthesised(label: &str, document: Document) -> Document {
    if document.archive().is_none() {
        held::refuse(
            label,
            "synthesised",
            "`Document::archive()` is `None`, so nothing was synthesised: hold a PDF's \
             bytes by `authored` or `rewritten`",
        );
    }
    let (_, read) = read(label, "synthesised", Handed::from(document.cos().bytes()));
    held::authored(label, "synthesised", &read);
    document
}

/// Every finding this reading holds a document to, held to nothing: how a
/// test compares this reading with the cos reading.
#[track_caller]
pub fn items<'k>(label: &str, bytes: impl Into<Handed<'k>>) -> Vec<held::Item> {
    read(label, "items", bytes.into()).1.items
}

/// Opens, authenticates and validates one document.
#[track_caller]
fn read(label: &str, door: &str, handed: Handed<'_>) -> (Document, held::Read) {
    let bytes = Arc::clone(&handed.bytes);
    let doc = match Document::open(Arc::clone(&bytes)) {
        Ok(doc) => doc,
        Err(error) => held::refuse(label, door, &format!("the bytes do not open: {error:?}")),
    };
    if doc.archive().is_some() {
        held::refuse(
            label,
            door,
            "the bytes are not a PDF: `Document::open` synthesised one from them, which \
             `synthesised` holds",
        );
    }
    unlock(label, door, &doc, handed.key);
    let mut items: Vec<held::Item> = doc.validate().iter().map(item).collect();
    let mut notes = Vec::new();
    profiles(&doc, &mut items, &mut notes);
    let read = held::Read {
        bytes,
        items,
        notes,
    };
    (doc, read)
}

/// Authenticates an encrypted document with its key, or with the empty user
/// password when it was handed over without one, which `Document::open` leaves
/// untried: `validate` reads a stream it cannot decrypt as one that does not
/// decode.
#[track_caller]
fn unlock(label: &str, door: &str, doc: &Document, key: Option<Key<'_>>) {
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

/// Each profile the document claims, held within the coverage this build
/// implements.
///
/// The claim is the validator's own reading of it, taken from the verdict that
/// is held. Each claim is first read by the validator's cheapest request that
/// still reads it — PDF/A's and PDF/UA's metadata group, and PDF/X's whole
/// request, which runs no rule for a file that claims nothing — so a document
/// that claims nothing costs no rule group.
fn profiles(doc: &Document, items: &mut Vec<held::Item>, notes: &mut Vec<(&'static str, String)>) {
    if claims_pdfa(&doc.validate_pdfa_with(PdfACoverage::METADATA)) {
        let verdict = doc.validate_pdfa();
        if claims_pdfa(&verdict) {
            // A claim with no flavour is one `flavour_of` could not read: a part
            // ISO 19005 does not define, which is a finding of its own.
            let claim = verdict
                .flavour
                .map_or_else(|| "an unread PDF/A claim".to_string(), |f| f.to_string());
            hold("PDF/A", &claim, &verdict.findings, items);
            notes.push((
                "PDF/A",
                format!(
                    "{claim}; ran {}; {} rules staged",
                    verdict.coverage,
                    PDFA_STAGED.len()
                ),
            ));
        }
    }

    if claims_pdfua(&doc.validate_pdfua_with(PdfUaCoverage::METADATA)) {
        let verdict = doc.validate_pdfua();
        if claims_pdfua(&verdict) {
            let claim = verdict
                .part
                .map_or_else(|| "an unread PDF/UA claim".to_string(), |p| p.to_string());
            hold("PDF/UA", &claim, &verdict.findings, items);
            notes.push((
                "PDF/UA",
                format!(
                    "{claim}; ran {}; {} clauses abstained",
                    verdict.coverage,
                    verdict.abstained.len()
                ),
            ));
        }
    }

    let verdict = doc.validate_pdfx();
    if let Some(claimed) = &verdict.claim {
        let claim = verdict
            .flavour
            .map_or_else(|| format!("PDF/X `{}`", claimed.version), |f| f.to_string());
        hold("PDF/X", &claim, &verdict.findings, items);
        let level = if verdict.flavour.is_some_and(|f| f.is_validated()) {
            "a level this build validates"
        } else {
            "a level no rule runs under"
        };
        notes.push((
            "PDF/X",
            format!(
                "{claim}, {level}; ran {}; {} clauses abstained",
                verdict.coverage,
                verdict.abstained.len()
            ),
        ));
    }
}

/// A document claims PDF/A unless no flavour was read and the verdict says
/// there was no packet or the packet names no part. Written as the absence of a
/// claim, so a malformed-claim kind added later is held rather than passed.
fn claims_pdfa(verdict: &PdfAVerdict) -> bool {
    verdict.flavour.is_some()
        || !verdict.findings.iter().any(|finding| {
            matches!(
                finding.kind,
                FindingKind::MetadataMissing | FindingKind::NoFlavourClaimed
            )
        })
}

/// A document claims PDF/UA unless its packet carries no `pdfuaid:part`; a part
/// ISO 14289 does not define is a claim, and a finding.
fn claims_pdfua(verdict: &PdfUaVerdict) -> bool {
    !verdict
        .findings
        .iter()
        .any(|finding| finding.kind == FindingKind::PdfUaIdentifierMissing)
}

/// One profile's findings as the policy reads them. None is the writer's alone,
/// and a class is the claim, the clause and the kind's variant, which is what
/// survives renumbering.
fn hold(
    verdict: &'static str,
    claim: &str,
    findings: &[ConformanceFinding],
    items: &mut Vec<held::Item>,
) {
    for finding in findings {
        let exact = format!("{:?}", finding.kind);
        let variant = exact
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .next()
            .unwrap_or_default()
            .to_string();
        items.push(held::Item {
            verdict,
            owned: false,
            class: format!("{claim} {} {variant}", finding.clause),
            exact,
            at: None,
            shown: format!("{claim} {finding}"),
        });
    }
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
    let (verdict, exact) = match expected {
        Expected::Defect(kind) => (kind.tier().as_str(), format!("{kind:?}")),
        Expected::PdfA(kind) => ("PDF/A", format!("{kind:?}")),
        Expected::PdfUa(kind) => ("PDF/UA", format!("{kind:?}")),
        Expected::PdfX(kind) => ("PDF/X", format!("{kind:?}")),
    };
    held::Want { verdict, exact }
}
