//! Output intents (ISO 32000-1 14.11.5): the catalog's, and since PDF 2.0 a
//! page's own.
//!
//! A reader of what the file says, and nothing more. The PDF/A validator
//! keeps its own reading in `pdfa/colour.rs`, which judges an intent against
//! the part it was claimed under; this one reports every intent whatever its
//! subtype, and judges none, so the two answer different questions and
//! neither is built on the other.
//!
//! Sources, since ISO 32000-2's text is not readable here: the Arlington
//! model's `PageObject` and `OutputIntents` tables (`tsv/latest`, at
//! `c48b363`) make `/OutputIntents` a 2.0 page entry, not inheritable, with
//! Table 401's keys; the PDF Association's `pdf20examples` (at `c20f2c1`)
//! carry a file whose page-level intent, in its own words, *"can override the
//! output intent for the document in the catalog"*.

use tinker_pdf_cos::{limits, CosDocument, Dict, ObjRef};

use crate::copies::Copies;
use crate::{Document, Page};

/// How many bytes one [`Document::output_intents`] or
/// [`Page::output_intents`] listing may copy out of the document.
///
/// `MAX_ANNOTATION_BYTES`'s reason: `/OutputIntents` is read to
/// [`limits::MAX_ARRAY_LEN`] entries, every one of them may name the same
/// intent dictionary, and its `/Info` — or any of its four strings — may be
/// one indirect string as long as the file, copied once per entry. The
/// listing spends one budget, charged before each copy, and an entry it
/// cannot pay for reads as absent **and says so**:
/// [`OutputIntent::incomplete`].
///
/// A string costs its bytes before decoding and a name its bytes.
///
/// | | Bytes |
/// | --- | --- |
/// | The most any fixture in this repository spends: the one built to spend it | 64 MiB |
/// | The most any other fixture spends: an archival intent's `/Info` and three names | under 1 KiB |
/// | A 200-page comic archive | 0 |
/// | A 200-page fixed document | 0 |
/// | A 300-page reflowable book | 0 |
/// | **This cap** | **64 MiB** |
///
/// The three zeros are facts about what those paths write: none of them
/// writes an output intent unless it is asked for an archival profile, and
/// then one, whose strings are a condition's name and a sentence.
pub const MAX_OUTPUT_INTENT_BYTES: usize = 64 << 20;

/// One output intent dictionary (ISO 32000-1 14.11.5 Table 365).
///
/// Every field is what the dictionary says, or `None` where it says nothing
/// usable; nothing is defaulted, because an intent missing its required
/// `/OutputConditionIdentifier` and one stating it are different files.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct OutputIntent {
    /// `/S`: `GTS_PDFX`, `GTS_PDFA1`, `ISO_PDFE1`, or another. `None` when
    /// it is absent or not a name.
    pub subtype: Option<String>,
    /// `/OutputConditionIdentifier`, which Table 365 requires.
    pub output_condition_identifier: Option<String>,
    /// `/OutputCondition`.
    pub output_condition: Option<String>,
    /// `/RegistryName`.
    pub registry_name: Option<String>,
    /// `/Info`.
    pub info: Option<String>,
    /// `/DestOutputProfile`, the ICC profile stream, by reference — read its
    /// bytes through [`Document::cos`]. A profile written directly rather
    /// than as Table 365's indirect stream names nothing and is `None`.
    pub destination_profile: Option<ObjRef>,
    /// That stream's `/N`: how many components its colour space has.
    pub components: Option<u32>,
    /// Whether a string or name this entry carries was left unread because
    /// the listing had spent [`MAX_OUTPUT_INTENT_BYTES`]. The fields it would
    /// have filled read `None`; the profile and its `/N` are read regardless,
    /// because neither is a copy.
    pub incomplete: bool,
}

impl Document {
    /// The catalog's `/OutputIntents`, in the array's order (14.11.5).
    ///
    /// The document-wide intents: what every page's colours are meant for,
    /// unless a page says otherwise ([`Page::output_intents`]). An entry that
    /// is not a dictionary is skipped; empty when the catalog has none.
    #[must_use]
    pub fn output_intents(&self) -> Vec<OutputIntent> {
        let doc = &*self.inner;
        doc.catalog()
            .map(|catalog| intents_in(doc, &catalog))
            .unwrap_or_default()
    }
}

impl Page {
    /// This page's own `/OutputIntents` (PDF 2.0), in the array's order.
    ///
    /// Empty for a page that states none, which is every page before 2.0:
    /// the page then has the catalog's ([`Document::output_intents`]).
    /// Read from the page itself only — the Arlington model does not make the
    /// entry inheritable, so a value on a `/Pages` node describes no page.
    ///
    /// **Not merged with the catalog's.** A page's intents override the
    /// document's for that page, as the PDF Association's example says; how
    /// the two combine when they name different subtypes is not in a source
    /// this build could read, so the two lists are handed back as written and
    /// the combination is the caller's.
    #[must_use]
    pub fn output_intents(&self) -> Vec<OutputIntent> {
        let doc = &*self.doc;
        match doc.get(self.inner.reference) {
            Ok(object) => object
                .as_dict()
                .map(|dict| intents_in(doc, dict))
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }
}

/// The `/OutputIntents` array of `holder`, read, within
/// [`MAX_OUTPUT_INTENT_BYTES`].
fn intents_in(doc: &CosDocument, holder: &Dict) -> Vec<OutputIntent> {
    let listed = doc.resolve_key(holder, doc.intern(b"OutputIntents"));
    let Some(entries) = listed.as_array() else {
        return Vec::new();
    };
    let mut copies = Copies::new(MAX_OUTPUT_INTENT_BYTES);
    entries
        .iter()
        .take(limits::MAX_ARRAY_LEN)
        .filter_map(|entry| {
            let refused = copies.refused();
            let resolved = doc.resolve(entry);
            let intent = resolved.as_dict()?;
            let destination_profile = intent.get_ref(doc.intern(b"DestOutputProfile"));
            let components = destination_profile.and_then(|profile| {
                let stream = doc.get(profile).ok()?;
                let dict = stream.as_dict()?;
                let n = doc.resolve_key(dict, doc.intern(b"N")).as_int()?;
                u32::try_from(n).ok()
            });
            let subtype = copies.name(doc, intent, b"S");
            let output_condition_identifier =
                copies.text(doc, intent, b"OutputConditionIdentifier");
            let output_condition = copies.text(doc, intent, b"OutputCondition");
            let registry_name = copies.text(doc, intent, b"RegistryName");
            let info = copies.text(doc, intent, b"Info");
            Some(OutputIntent {
                subtype,
                output_condition_identifier,
                output_condition,
                registry_name,
                info,
                destination_profile,
                components,
                incomplete: copies.refused() > refused,
            })
        })
        .collect()
}
