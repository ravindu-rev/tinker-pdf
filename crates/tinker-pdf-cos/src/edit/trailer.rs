//! Trailer entries (7.5.5) the editor sets, and the document information
//! dictionary (14.3.3) through them.

use super::DocumentEditor;
use crate::name::Name;
use crate::object::{Dict, Object};
use crate::text_string::encode_text_string;

/// Trailer keys the writer computes for every save, which a caller's value
/// could only contradict: `/Size` and `/Prev` (7.5.5 Table 15), `/XRefStm`
/// (7.5.8.4), `/ID` (14.4 — the second string moves with each revision),
/// `/Encrypt` (an update inherits the file's scheme and a rewrite drops it),
/// and the cross-reference stream's own entries (7.5.8.2 Table 17, and
/// 7.3.8.2 Table 5 for the stream itself).
const WRITER_OWNED: [&[u8]; 11] = [
    b"Size",
    b"Prev",
    b"XRefStm",
    b"ID",
    b"Encrypt",
    b"Type",
    b"W",
    b"Index",
    b"Length",
    b"Filter",
    b"DecodeParms",
];

impl DocumentEditor {
    /// The trailer as a save will write it: the document's (merged across its
    /// revisions), with every entry this editor has set laid over it.
    ///
    /// An entry set to null is left out: 7.3.9 makes a null value the entry
    /// being absent — this is how [`DocumentEditor::sanitise`] takes `/Info`
    /// out. That is the trailer a rewrite writes; an incremental update
    /// writes [`DocumentEditor::update_trailer`].
    pub(crate) fn merged_trailer(&self) -> Dict {
        let mut trailer = self.doc.trailer().clone();
        for (key, value) in self.trailer.iter() {
            if matches!(value, Object::Null) {
                trailer = super::without(&trailer, *key);
            } else {
                trailer.insert(*key, value.clone());
            }
        }
        trailer
    }

    /// The trailer an incremental update writes: [`Self::merged_trailer`],
    /// with every entry this editor removed written as null where an earlier
    /// revision's trailer has it.
    ///
    /// Left out, the entry would be absent from the update's trailer, which
    /// 7.5.6 makes the whole story — but a reader that merges each revision's
    /// trailer with the ones before it, as this crate's does (newest first,
    /// so a key an update dropped is still found), would find the earlier
    /// revision's value and bring it back: an incremental sanitise left
    /// `/Info` readable whenever something else kept the dictionary alive.
    /// Written as null, the key is present in the newest trailer and 7.3.9
    /// reads it as absent, which both kinds of reader agree on.
    pub(crate) fn update_trailer(&self) -> Dict {
        let mut trailer = self.merged_trailer();
        for (key, value) in self.trailer.iter() {
            if matches!(value, Object::Null) && self.doc.trailer().contains_key(*key) {
                trailer.insert(*key, Object::Null);
            }
        }
        trailer
    }

    /// Sets one trailer entry (7.5.5 Table 15), which every save writes —
    /// incremental, rewrite and signed alike.
    ///
    /// Returns false, changing nothing, for a key the writer computes itself
    /// (`/Size`, `/Prev`, `/XRefStm`, `/ID`, `/Encrypt` and the
    /// cross-reference stream's `/Type`, `/W`, `/Index`, `/Length`,
    /// `/Filter`, `/DecodeParms`): a value set here would be overwritten or,
    /// worse, believed.
    ///
    /// [`Object::Null`] **removes** the entry (7.3.9: a null value is the
    /// entry absent): a rewrite leaves the key out, and an incremental update
    /// writes it as null, so a reader that falls back to an earlier
    /// revision's trailer for a missing key does not find the old value
    /// there.
    ///
    /// Part of the editor's state, so a [`crate::edit::EditCheckpoint`] and a
    /// rolled-back [`DocumentEditor::transaction`] restore it.
    pub fn set_trailer_entry(&mut self, key: Name, value: Object) -> bool {
        let Some(bytes) = self.doc.name_bytes(key) else {
            return false;
        };
        if WRITER_OWNED.contains(&bytes.as_ref()) {
            return false;
        }
        self.trailer.insert(key, value);
        true
    }

    /// Sets a document information entry (14.3.3), creating `/Info` when the
    /// document has none.
    ///
    /// The value is a text string, encoded for the version the document
    /// declares ([`crate::text_string::encode_text_string`]) — the same
    /// writer [`crate::build::DocumentBuilder::set_info`] uses.
    ///
    /// An existing `/Info` object is updated where it is. One written
    /// directly into the trailer — which 7.5.5 Table 15 says shall be an
    /// indirect reference — is moved into an object of its own, and a
    /// document with none gets a new one, named through
    /// [`DocumentEditor::set_trailer_entry`] so that saving and rolling back
    /// treat it like any other trailer entry.
    pub fn set_info(&mut self, key: &[u8], value: &str) {
        let key = self.intern(key);
        let value = Object::String(encode_text_string(value, self.text_version()));
        self.set_info_entry(key, value);
    }

    /// [`DocumentEditor::set_info`] for a value already built: a text string,
    /// or the name `/Trapped` holds (14.3.3 Table 349).
    pub(super) fn set_info_entry(&mut self, key: Name, value: Object) {
        match self.merged_trailer().get(Name::INFO).cloned() {
            Some(Object::Ref(info)) => {
                let mut dict = match self.get(info) {
                    Some(Object::Dict(dict)) => dict,
                    // A reference to something that is not a dictionary is
                    // not an information dictionary; the object is replaced.
                    _ => Dict::new(),
                };
                dict.insert(key, value);
                self.put(info, Object::Dict(dict));
            }
            existing => {
                let mut dict = match existing {
                    Some(Object::Dict(dict)) => dict,
                    _ => Dict::new(),
                };
                dict.insert(key, value);
                let info = self.allocate();
                self.put(info, Object::Dict(dict));
                self.trailer.insert(Name::INFO, Object::Ref(info));
            }
        }
    }
}
