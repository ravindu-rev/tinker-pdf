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

use tinker_pdf_cos::{limits, CosDocument, Dict, ObjRef, Object};

use crate::copies::{value, Copies};
use crate::{Document, FileRelationship, Page};

/// How many bytes one [`Document::associated_files`] or
/// [`Page::associated_files`] listing may copy out of the document.
///
/// `MAX_ANNOTATION_BYTES`'s reason exactly: an `/AF` array is read to
/// [`limits::MAX_ARRAY_LEN`] entries, every one of them may name the same file
/// specification, and that specification's `/Desc` may be one indirect string
/// as long as the file — so 90 KB asked for 256 MiB of descriptions, and a
/// 6 MB array for 64 GiB, before this. The listing spends one budget, charged
/// before each copy, and an entry it cannot pay for reads as absent **and
/// says so**: [`AssociatedFile::incomplete`]. A structure element's `/AF` is
/// read by the structure walk under that walk's own budget,
/// [`crate::structure::MAX_STRUCTURE_BYTES`].
///
/// A string costs its bytes before decoding and a name its bytes.
///
/// | | Bytes |
/// | --- | --- |
/// | The most any fixture in this repository spends: the one built to spend it | 64 MiB |
/// | The most any other fixture spends: `tests/pdf20.rs`'s catalog and page files | under 1 KiB |
/// | A 200-page comic archive | 0 |
/// | A 200-page fixed document | 0 |
/// | A 300-page reflowable book | 0 |
/// | **This cap** | **64 MiB** |
///
/// The three zeros are facts about what those paths write: none of them
/// associates a file with the catalog or a page. A real `/AF` names a
/// handful of files — a source spreadsheet, an XML invoice — whose names
/// and descriptions are tens of bytes each.
pub const MAX_ASSOCIATED_FILE_BYTES: usize = 64 << 20;

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
    /// Whether a string or name this entry carries was left unread because
    /// the listing had spent its copy budget — [`MAX_ASSOCIATED_FILE_BYTES`],
    /// or for a structure element's files the structure walk's
    /// [`crate::structure::MAX_STRUCTURE_BYTES`]. The fields it would have
    /// filled read `None`; the references, the relationship and `/Size` are
    /// read regardless, because none of them is a copy.
    pub incomplete: bool,
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
    let mut copies = Copies::new(MAX_ASSOCIATED_FILE_BYTES);
    files_in(
        doc,
        &entries[..entries.len().min(limits::MAX_ARRAY_LEN)],
        &mut copies,
    )
}

/// The file specifications among `entries`, in order, each string and name
/// charged to `copies`. The structure walk calls this with as many entries as
/// its values budget allows, and its own copy budget.
pub(crate) fn files_in(
    doc: &CosDocument,
    entries: &[Object],
    copies: &mut Copies,
) -> Vec<AssociatedFile> {
    entries
        .iter()
        .filter_map(|entry| file_of(doc, entry, copies))
        .collect()
}

fn file_of(doc: &CosDocument, entry: &Object, copies: &mut Copies) -> Option<AssociatedFile> {
    let refused = copies.refused();
    let specification = entry.as_objref();
    let resolved = doc.resolve(entry);
    let spec = resolved.as_dict()?;
    // 7.11.3: `/UF` is the Unicode name and wins over `/F` where both are.
    let filename = copies
        .text(doc, spec, b"UF")
        .or_else(|| copies.text(doc, spec, b"F"));
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
            Some((copies.name(doc, dict, b"Subtype"), size))
        })
        .unwrap_or((None, None));
    // Read from the name in place rather than from its copy, so a budget that
    // cannot pay for the text still says what the file is to its holder.
    let mut held = None;
    let relationship = value(doc, spec, b"AFRelationship", &mut held)
        .and_then(Object::as_name)
        .and_then(|name| doc.name_bytes(name))
        .and_then(|name| FileRelationship::from_name(&name));
    let relationship_name = copies.name(doc, spec, b"AFRelationship");
    let description = copies.text(doc, spec, b"Desc");
    Some(AssociatedFile {
        specification,
        filename,
        description,
        relationship,
        relationship_name,
        stream,
        mime_type,
        size,
        incomplete: copies.refused() > refused,
    })
}
