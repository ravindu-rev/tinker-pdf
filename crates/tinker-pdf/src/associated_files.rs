//! Associated files (ISO 32000-2 14.13): files connected to an object of
//! the document by that object's `/AF` array, each saying what it is to the
//! object through `/AFRelationship`.
//!
//! Read from the catalog (the document as a whole), a page, and a structure
//! element — the three places this engine also writes one. What is *not* read
//! is named in `docs/pdf20-deltas.md`: an `/AF` on an annotation or an
//! XObject, and a marked-content sequence's `/AF` tag with its `/MCAF`
//! property list (14.13.5, Table 409a).
//!
//! Sources, since the 2.0 text is not readable here: the PDF Association's
//! approved errata (`pdf-issues` at `b25fc23`) quote 14.13.1 — *"an AF entry
//! that shall be an array of file specification dictionaries … that contain
//! an AFRelationship entry"* — and Table 44's `/Subtype` requirement for an
//! embedded file stream used as one; the Arlington model (at `c48b363`) lists
//! the `/AF` holders and `/AFRelationship`'s values.

use tinker_pdf_cos::{decode_text_string, limits, CosDocument, Dict, ObjRef, Object};

use crate::{Document, FileRelationship, Page};

/// One associated file: a file specification dictionary in an `/AF` array
/// (14.13.1, 7.11.3).
///
/// Nothing is defaulted: a specification with no `/AFRelationship` reads
/// `relationship: None` and `relationship_name: None`, though the Arlington
/// model gives the key a default of `Unspecified`, because a file that states
/// its relationship and one that does not are different files — and 14.13.1
/// requires the entry.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct AssociatedFile {
    /// The file specification dictionary, when it is an indirect object.
    pub specification: Option<ObjRef>,
    /// `/UF`, or `/F` when there is no `/UF` (7.11.3): the name to offer when
    /// the file is saved out.
    pub filename: Option<String>,
    /// `/Desc`.
    pub description: Option<String>,
    /// `/AFRelationship`, when it is one of the values the errata and the
    /// Arlington model list.
    pub relationship: Option<FileRelationship>,
    /// `/AFRelationship` as written, whatever it is — an extension's name such
    /// as the Arlington model's `C2PA_Manifest` included.
    pub relationship_name: Option<String>,
    /// The embedded file stream (`/EF`, `/UF` preferred over `/F`), by
    /// reference: read its bytes through [`Document::cos`]. `None` for a file
    /// specification naming a file outside the document.
    pub stream: Option<ObjRef>,
    /// The stream's `/Subtype`: the MIME type, which Table 44 requires of an
    /// embedded file stream used as an associated file.
    pub mime_type: Option<String>,
    /// The stream's `/Params /Size`, when declared. Advisory — the stream is
    /// the truth.
    pub size: Option<i64>,
}

impl Document {
    /// The catalog's `/AF`: files associated with the document as a whole,
    /// in the array's order (14.13.1; the Arlington model lists the catalog
    /// among the dictionaries that may carry one).
    #[must_use]
    pub fn associated_files(&self) -> Vec<AssociatedFile> {
        let doc = &*self.inner;
        doc.catalog()
            .map(|catalog| associated_files_of(doc, &catalog))
            .unwrap_or_default()
    }
}

impl Page {
    /// This page's `/AF`: files associated with the page, in the array's
    /// order. Read from the page itself; the Arlington model does not make
    /// the entry inheritable.
    #[must_use]
    pub fn associated_files(&self) -> Vec<AssociatedFile> {
        let doc = &*self.doc;
        match doc.get(self.inner.reference) {
            Ok(object) => object
                .as_dict()
                .map(|dict| associated_files_of(doc, dict))
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }
}

/// The `/AF` array of `holder`, read. An entry that is not a dictionary is
/// skipped: it is not a file specification, and 14.13.1 makes every entry one.
fn associated_files_of(doc: &CosDocument, holder: &Dict) -> Vec<AssociatedFile> {
    let listed = doc.resolve_key(holder, doc.intern(b"AF"));
    let Some(entries) = listed.as_array() else {
        return Vec::new();
    };
    files_in(doc, &entries[..entries.len().min(limits::MAX_ARRAY_LEN)])
}

/// The file specifications among `entries`, in order. The structure walk
/// calls this with as many entries as its retention budget allows.
pub(crate) fn files_in(doc: &CosDocument, entries: &[Object]) -> Vec<AssociatedFile> {
    entries
        .iter()
        .filter_map(|entry| file_of(doc, entry))
        .collect()
}

fn file_of(doc: &CosDocument, entry: &Object) -> Option<AssociatedFile> {
    let specification = entry.as_objref();
    let resolved = doc.resolve(entry);
    let spec = resolved.as_dict()?;
    let text = |dict: &Dict, key: &[u8]| {
        doc.resolve_key(dict, doc.intern(key))
            .as_string()
            .map(|s| decode_text_string(&s.bytes))
    };
    let name = |dict: &Dict, key: &[u8]| {
        doc.resolve_key(dict, doc.intern(key))
            .as_name()
            .and_then(|name| doc.name_bytes(name))
            .map(|name| String::from_utf8_lossy(&name).into_owned())
    };
    // 7.11.3: `/UF` is the Unicode name and wins over `/F` where both are.
    let filename = text(spec, b"UF").or_else(|| text(spec, b"F"));
    let ef = doc.resolve_key(spec, doc.intern(b"EF"));
    let stream = ef.as_dict().and_then(|ef| {
        ef.get_ref(doc.intern(b"UF"))
            .or_else(|| ef.get_ref(doc.intern(b"F")))
    });
    let (mime_type, size) = stream
        .and_then(|reference| {
            let object = doc.get(reference).ok()?;
            let dict = object.as_dict()?;
            let params = doc.resolve_key(dict, doc.intern(b"Params"));
            let size = params
                .as_dict()
                .and_then(|params| doc.resolve_key(params, doc.intern(b"Size")).as_int());
            Some((name(dict, b"Subtype"), size))
        })
        .unwrap_or((None, None));
    let relationship_name = name(spec, b"AFRelationship");
    let relationship = relationship_name
        .as_deref()
        .and_then(|name| FileRelationship::from_name(name.as_bytes()));
    Some(AssociatedFile {
        specification,
        filename,
        description: text(spec, b"Desc"),
        relationship,
        relationship_name,
        stream,
        mime_type,
        size,
    })
}
