//! What a reader copies out of a document, and the budget it spends doing so.
//!
//! An indirect object is parsed once and may be named from every entry of an
//! array, every element of a structure tree and every namespace dictionary in
//! it, so a reader that hands back one copy of a string per mention turns a
//! file of kilobytes into gigabytes: `MAX_ANNOTATION_BYTES`'s reason, for the
//! readers ISO 32000-2 added — associated files, output intents, and the
//! structure tree's namespaces, identifiers and table headers.
//!
//! A [`Copies`] is charged **before** each copy is made, and an entry it
//! cannot pay for reads as absent; the reader that owns it says so (ruling
//! 10). A string costs its bytes before decoding and a name its bytes, the
//! rule `MAX_ANNOTATION_BYTES` set.

use std::sync::Arc;

use tinker_pdf_cos::{decode_text_string, CosDocument, Dict, Object};

/// A reader's copy budget, and how many copies it has refused.
pub(crate) struct Copies {
    left: usize,
    refused: usize,
}

impl Copies {
    pub(crate) fn new(budget: usize) -> Copies {
        Copies {
            left: budget,
            refused: 0,
        }
    }

    /// Takes `bytes` from the budget, or refuses and takes nothing.
    pub(crate) fn charge(&mut self, bytes: usize) -> bool {
        if bytes > self.left {
            self.refused = self.refused.saturating_add(1);
            return false;
        }
        self.left -= bytes;
        true
    }

    /// How many copies the budget has refused so far. A reader compares this
    /// before and after an entry to say whether that entry lost anything.
    pub(crate) fn refused(&self) -> usize {
        self.refused
    }

    /// A text string entry, decoded by 7.9.2.2's rules.
    pub(crate) fn text(&mut self, doc: &CosDocument, dict: &Dict, key: &[u8]) -> Option<String> {
        let mut held = None;
        let string = value(doc, dict, key, &mut held)?.as_string()?;
        if !self.charge(string.bytes.len()) {
            return None;
        }
        Some(decode_text_string(&string.bytes))
    }

    /// A byte string entry, undecoded: an identifier rather than text.
    pub(crate) fn bytes(&mut self, doc: &CosDocument, dict: &Dict, key: &[u8]) -> Option<Vec<u8>> {
        let mut held = None;
        let string = value(doc, dict, key, &mut held)?.as_string()?;
        self.string(&string.bytes)
    }

    /// A string already in hand, copied.
    pub(crate) fn string(&mut self, bytes: &[u8]) -> Option<Vec<u8>> {
        self.charge(bytes.len()).then(|| bytes.to_vec())
    }

    /// A name entry's bytes, as text.
    pub(crate) fn name(&mut self, doc: &CosDocument, dict: &Dict, key: &[u8]) -> Option<String> {
        let mut held = None;
        let name = value(doc, dict, key, &mut held)?.as_name()?;
        let bytes = doc.name_bytes(name)?;
        if !self.charge(bytes.len()) {
            return None;
        }
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }
}

/// `dict`'s value at `key`, with a reference followed — and a direct value
/// **borrowed** rather than copied, which `CosDocument::resolve` would do.
///
/// The difference is the whole point here: a direct string inside a shared
/// dictionary is the same bytes however many entries name the dictionary,
/// and resolving it copied them once per entry before a budget could refuse
/// anything. `held` keeps a followed reference's target alive for as long as
/// the borrow.
pub(crate) fn value<'v>(
    doc: &CosDocument,
    dict: &'v Dict,
    key: &[u8],
    held: &'v mut Option<Arc<Object>>,
) -> Option<&'v Object> {
    let raw = dict.get(doc.intern(key))?;
    if raw.as_objref().is_some() {
        Some(&**held.insert(doc.resolve(raw)))
    } else {
        Some(raw)
    }
}

/// Whether a text string's bytes decode to the empty string (7.9.2.2): no
/// bytes, or a byte-order mark and nothing after it. Every other string
/// decodes to at least one character — PDFDocEncoding maps every byte to one,
/// and a stray UTF-16 byte to U+FFFD — so this answers without decoding.
pub(crate) fn decodes_to_nothing(bytes: &[u8]) -> bool {
    matches!(bytes, [] | [0xFE, 0xFF] | [0xEF, 0xBB, 0xBF])
}

#[cfg(test)]
mod tests {
    use super::decodes_to_nothing;
    use tinker_pdf_cos::decode_text_string;

    /// The predicate is the decoder's own answer, over every short string
    /// that could be a byte-order mark or a fragment of one.
    #[test]
    fn decodes_to_nothing_agrees_with_the_decoder() {
        let alphabet = [0x00, 0x41, 0xBB, 0xBF, 0xEF, 0xFE, 0xFF];
        let mut cases: Vec<Vec<u8>> = vec![Vec::new()];
        for _ in 0..4 {
            let longer: Vec<Vec<u8>> = cases
                .iter()
                .flat_map(|case| {
                    alphabet.iter().map(move |byte| {
                        let mut next = case.clone();
                        next.push(*byte);
                        next
                    })
                })
                .collect();
            cases.extend(longer);
        }
        for case in cases {
            assert_eq!(
                decodes_to_nothing(&case),
                decode_text_string(&case).is_empty(),
                "{case:02X?}"
            );
        }
    }
}
