//! Associated files (ISO 32000-2 14.13): files connected to an object of
//! the document by that object's `/AF` array, each saying what it is to the
//! object through `/AFRelationship`.
//!
//! Read from every holder this engine also writes one on: the catalog (the
//! document as a whole), a page, a structure element, the structure tree
//! root, an annotation and a form XObject — and from any other object by its
//! reference ([`Document::associated_files_of`]), since the Arlington model
//! lists image XObjects and document parts among the holders too — and from a
//! marked-content sequence tagged `/AF` whose named property list carries
//! `/MCAF` ([`Page::marked_content_associated_files`]).
//!
//! Sources, since the 2.0 text is not readable here: the PDF Association's
//! approved errata (`pdf-issues` at `b25fc23`) quote 14.13.1 — *"an AF entry
//! that shall be an array of file specification dictionaries … that contain
//! an AFRelationship entry"* — and Table 44's `/Subtype` requirement for an
//! embedded file stream used as one, and amend 14.13.5 with Table 409a, the
//! `/MCAF` entry of a marked-content property list; the Arlington model (at
//! `c48b363`) lists the `/AF` holders and `/AFRelationship`'s values.

use std::mem::size_of;

use tinker_pdf_content::{Device, FontSource, MarkedProps};
use tinker_pdf_cos::{limits, CosDocument, Dict, ObjRef, Object};

use crate::copies::{value, Copies};
use crate::resources::{read_resolved, PageResources};
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
/// | Any other fixture here: `tests/pdf20.rs`'s catalog and page files | under 1 KiB (estimate, not summed) |
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

impl Document {
    /// The `/AF` of the object `reference` names: files associated with an
    /// annotation, a form or image XObject, a structure element, a document
    /// part — any of the holders the Arlington model lists — in the array's
    /// order. Empty for an object with none, and for a reference that names
    /// no dictionary or stream.
    ///
    /// The catalog's and a page's have their own calls
    /// ([`Document::associated_files`], [`Page::associated_files`]); this is
    /// the one for the holders reached by reference, a form XObject's among
    /// them, whose reference is in its resource dictionary.
    #[must_use]
    pub fn associated_files_of(&self, reference: ObjRef) -> Vec<AssociatedFile> {
        let doc = &*self.inner;
        let Ok(object) = doc.get(reference) else {
            return Vec::new();
        };
        let holder = match &*object {
            Object::Dict(dict) => dict,
            Object::Stream(stream) => &stream.dict,
            _ => return Vec::new(),
        };
        associated_files_of(doc, holder)
    }

    /// The structure tree root's `/AF`: files associated with the document's
    /// logical structure as a whole (the Arlington model lists
    /// `StructTreeRoot` among the holders), in the array's order. Empty for a
    /// document with no structure tree.
    #[must_use]
    pub fn structure_associated_files(&self) -> Vec<AssociatedFile> {
        let doc = &*self.inner;
        let Some(catalog) = doc.catalog() else {
            return Vec::new();
        };
        let root = doc.resolve_key(&catalog, doc.intern(b"StructTreeRoot"));
        root.as_dict()
            .map(|root| associated_files_of(doc, root))
            .unwrap_or_default()
    }
}

/// One marked-content sequence tagged `/AF`, and the files its property list
/// associates with it (ISO 32000-2 14.13.5, Table 409a as the approved errata
/// add it).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct MarkedContentFiles {
    /// The property list's resource name: `MF1` in `/AF /MF1 BDC`.
    pub property: Vec<u8>,
    /// The form XObject whose content stream the `BDC` was written in, or
    /// `None` for the page's own content. The name is looked up in that
    /// stream's resources, which is where the property list is.
    pub form: Option<ObjRef>,
    /// `/MCAF`, read as every `/AF` array is: one [`AssociatedFile`] per file
    /// specification dictionary, in the array's order.
    pub files: Vec<AssociatedFile>,
    /// Whether an entry was left unread because the listing spent
    /// [`MAX_ASSOCIATED_FILE_BYTES`].
    pub incomplete: bool,
}

/// [`Page::marked_content_associated_files`]'s answer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct MarkedContentFileList {
    /// One entry per sequence, in the order the content draws them — a form
    /// drawn twice is two entries.
    pub sequences: Vec<MarkedContentFiles>,
    /// How many further sequences were not listed because the listing had
    /// spent [`MAX_ASSOCIATED_FILE_BYTES`].
    pub dropped: usize,
}

impl Page {
    /// The files each of this page's annotations is associated with: its
    /// `/AF`, one list per `/Annots` entry in the array's order — so the
    /// list at `i` belongs to [`Page::annotation_list`]'s annotation at `i`,
    /// inline annotations included, and is empty for one with no `/AF`.
    ///
    /// The Arlington model lists every annotation subtype among `/AF`'s
    /// holders. Read to the entries the annotation listing reads, under one
    /// [`MAX_ASSOCIATED_FILE_BYTES`] budget for the page, each file charged
    /// for its own record as well as its strings, since many annotations may
    /// name one long array; an annotation the budget ran out on reads with
    /// fewer files, and its files after the last one paid for are absent.
    #[must_use]
    pub fn annotation_associated_files(&self) -> Vec<Vec<AssociatedFile>> {
        let doc = &*self.doc;
        let Ok(object) = doc.get(self.inner.reference) else {
            return Vec::new();
        };
        let Some(page) = object.as_dict() else {
            return Vec::new();
        };
        let annots = doc.resolve_key(page, doc.intern(b"Annots"));
        let Some(entries) = annots.as_array() else {
            return Vec::new();
        };
        let mut copies = Copies::new(MAX_ASSOCIATED_FILE_BYTES);
        entries
            .iter()
            .take(crate::annotations::MAX_ANNOTS)
            .map(|entry| {
                let resolved = doc.resolve(entry);
                let Some(annotation) = resolved.as_dict() else {
                    return Vec::new();
                };
                let listed = doc.resolve_key(annotation, doc.intern(b"AF"));
                listed
                    .as_array()
                    .map(|files| charged_files(doc, files, &mut copies).0)
                    .unwrap_or_default()
            })
            .collect()
    }

    /// The marked-content sequences this page draws under the `/AF` tag whose
    /// named property list carries `/MCAF`, each with the files it lists —
    /// ISO 32000-2 14.13.5 as the approved errata amend it: *"The
    /// marked-content is connected with associated files only if the tag is
    /// AF and the named property list is defined according to Table 409a"*.
    ///
    /// So a sequence is listed only when both hold. An `/AF` sequence whose
    /// list is inline is not connected — the errata's NOTE 4: a file
    /// specification names its stream by reference, which a content stream
    /// cannot write, so *"named property resources are always used"* — nor is
    /// one whose named list has no `/MCAF`, nor a list carrying `/MCAF` under
    /// another tag. `DP` and `MP` mark points, not sequences, and are never
    /// read as one.
    ///
    /// Sequences inside form XObjects the page draws are included, the name
    /// looked up in the form's own resources as the interpreter does, and in
    /// drawing order. One [`MAX_ASSOCIATED_FILE_BYTES`] budget covers the
    /// listing, each sequence and each file charged for its record as well
    /// as its strings: past it a sequence is counted in
    /// [`MarkedContentFileList::dropped`], and one cut short is
    /// [`MarkedContentFiles::incomplete`].
    #[must_use]
    pub fn marked_content_associated_files(&self) -> MarkedContentFileList {
        let content = tinker_pdf_cos::pages::content_bytes(&self.doc, &self.inner);
        let resources = std::sync::Arc::new(PageResources::new(&self.doc, &self.inner, None));
        let mut device = MarkedFiles {
            doc: &self.doc,
            scope: std::sync::Arc::clone(&resources),
            outer: Vec::new(),
            copies: Copies::new(MAX_ASSOCIATED_FILE_BYTES),
            found: MarkedContentFileList::default(),
        };
        tinker_pdf_content::interpret(
            &content,
            tinker_pdf_content::Matrix::IDENTITY,
            &mut device,
            &*resources,
        );
        device.found
    }
}

/// The device behind [`Page::marked_content_associated_files`]: it follows
/// the interpreter into each form, so a property list's name is looked up in
/// the scope its `BDC` ran in.
struct MarkedFiles<'d> {
    doc: &'d CosDocument,
    scope: std::sync::Arc<PageResources>,
    outer: Vec<Option<std::sync::Arc<PageResources>>>,
    copies: Copies,
    found: MarkedContentFileList,
}

impl Device for MarkedFiles<'_> {
    fn begin_marked_content(
        &mut self,
        tag: &[u8],
        _visible: bool,
        _hidden_layer: Option<&str>,
        props: Option<&MarkedProps>,
    ) {
        if tag != b"AF" {
            return;
        }
        let Some((property, stream)) = props.and_then(|props| {
            props
                .associated_files
                .as_ref()
                .map(|name| (name, props.stream))
        }) else {
            return;
        };
        if !self
            .copies
            .charge(size_of::<MarkedContentFiles>().saturating_add(property.len()))
        {
            self.found.dropped = self.found.dropped.saturating_add(1);
            return;
        }
        // The list and its `/MCAF` read where they lie: a copy of either for
        // each sequence would be the list's size, uncharged, times the page's
        // sequences.
        let doc = self.doc;
        let copies = &mut self.copies;
        let (files, incomplete) = self
            .scope
            .with_property_list(property, |list| {
                let mcaf = list.get(doc.intern(b"MCAF"))?;
                read_resolved(doc, mcaf, |mcaf| {
                    mcaf.as_array()
                        .map(|entries| charged_files(doc, entries, copies))
                })
            })
            .flatten()
            .unwrap_or_default();
        // 14.7.4.2's packing, `num << 16 | gen`, undone; `0` is the page.
        let form = (stream != 0).then(|| {
            ObjRef::new(
                u32::try_from(stream >> 16).unwrap_or(u32::MAX),
                (stream & 0xFFFF) as u16,
            )
        });
        self.found.sequences.push(MarkedContentFiles {
            property: property.clone(),
            form,
            files,
            incomplete,
        });
    }

    fn begin_form(&mut self, _id: u64, name: &[u8]) -> bool {
        let nested = FontSource::form_scope(&*self.scope, name);
        self.outer
            .push(nested.map(|scope| std::mem::replace(&mut self.scope, scope)));
        true
    }

    fn end_form(&mut self, _id: u64) {
        if let Some(Some(outer)) = self.outer.pop() {
            self.scope = outer;
        }
    }
}

/// The file specifications among `entries`, each charged to `copies` for its
/// record before it is read, and whether the budget stopped the list short.
///
/// For the listings that read many arrays — a page's annotations, its
/// marked-content sequences — where one long array named many times would
/// otherwise cost a record per entry per naming whatever the strings cost.
fn charged_files(
    doc: &CosDocument,
    entries: &[Object],
    copies: &mut Copies,
) -> (Vec<AssociatedFile>, bool) {
    let mut files = Vec::new();
    for entry in entries.iter().take(limits::MAX_ARRAY_LEN) {
        if !copies.charge(size_of::<AssociatedFile>()) {
            return (files, true);
        }
        let refused = copies.refused();
        if let Some(file) = file_of(doc, entry, copies) {
            files.push(file);
        }
        if copies.refused() > refused {
            return (files, true);
        }
    }
    (files, false)
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
