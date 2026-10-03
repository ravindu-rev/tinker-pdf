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

use tinker_pdf_cos::{decode_text_string, limits, CosDocument, Dict, ObjRef};

use crate::{Document, Page};

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

/// The `/OutputIntents` array of `holder`, read.
fn intents_in(doc: &CosDocument, holder: &Dict) -> Vec<OutputIntent> {
    let listed = doc.resolve_key(holder, doc.intern(b"OutputIntents"));
    let Some(entries) = listed.as_array() else {
        return Vec::new();
    };
    entries
        .iter()
        .take(limits::MAX_ARRAY_LEN)
        .filter_map(|entry| {
            let resolved = doc.resolve(entry);
            let intent = resolved.as_dict()?;
            let destination_profile = intent.get_ref(doc.intern(b"DestOutputProfile"));
            let components = destination_profile.and_then(|profile| {
                let stream = doc.get(profile).ok()?;
                let dict = stream.as_dict()?;
                let n = doc.resolve_key(dict, doc.intern(b"N")).as_int()?;
                u32::try_from(n).ok()
            });
            Some(OutputIntent {
                subtype: doc
                    .resolve_key(intent, doc.intern(b"S"))
                    .as_name()
                    .and_then(|name| doc.name_bytes(name))
                    .map(|name| String::from_utf8_lossy(&name).into_owned()),
                output_condition_identifier: text(doc, intent, b"OutputConditionIdentifier"),
                output_condition: text(doc, intent, b"OutputCondition"),
                registry_name: text(doc, intent, b"RegistryName"),
                info: text(doc, intent, b"Info"),
                destination_profile,
                components,
            })
        })
        .collect()
}

/// A text string entry, decoded by 7.9.2.2's rules.
fn text(doc: &CosDocument, dict: &Dict, key: &[u8]) -> Option<String> {
    doc.resolve_key(dict, doc.intern(key))
        .as_string()
        .map(|s| decode_text_string(&s.bytes))
}
