//! Form data exchange: FDF (ISO 32000-1 12.7.8) and XFDF, both directions.
//!
//! Feature documentation: `docs/features/forms.md`.
//!
//! An FDF file is a PDF-syntax file whose root carries an `/FDF` dictionary,
//! and whose `/Fields` array is a field tree of its own — `/T` partial names,
//! `/Kids`, a `/V` on the leaves. XFDF is the same tree as XML: nested
//! `<field name="…">` elements under `<fields>`, a `<value>` in each leaf.
//! Both are read into one [`FormData`] — fully qualified names (12.7.3.2) and
//! the [`FieldValue`] the field-tree reader already uses — and both are
//! written from one, so a value exported from a document and imported into
//! another passes through exactly one model.
//!
//! **Held to the field tree this reader builds.** Export starts from
//! [`crate::Document::form_fields`] or [`crate::DocumentEditor::fields`] —
//! the walk that joins `/T` with periods and inherits `/V` — and import sinks
//! into [`crate::DocumentEditor::set_field_values`], which resolves names
//! against the same walk and refuses the whole import on the first field that
//! will not take its value. So a name in an FDF means what the same string
//! means to `fields()`, and a value a field would refuse from a user is refused
//! here too.
//!
//! # What is read, and what is named rather than read
//!
//! FDF: the `/FDF` dictionary's `/Fields` and `/F`. A field's `/V` as a text
//! string, a name (a button's state) or an array of either (a multiple
//! selection). What an FDF may also carry — `/Annots`, `/Pages` templates,
//! `/JavaScript`, a field's `/AP`, `/Ff`, `/SetFf`, `/Opt` and rich-text
//! `/RV` — is not read, and each key met is named in
//! [`FormData::warnings`] rather than skipped in silence (ruling 10). An
//! encrypted FDF is refused ([`FormDataError::Encrypted`]).
//!
//! XFDF: **the commonly documented core**, because the specification that
//! defines it — Adobe's *XML Forms Data Format Specification*, standardised
//! as ISO 19444-1 — was not available to this build. `<xfdf>`, `<f href>`,
//! `<fields>`, nested `<field name>`, and `<value>`, repeated for a multiple
//! selection. `<annots>`, `<ids>`, `<value-richtext>` and anything else are
//! named in the warnings and not read. A value is text: XFDF does not say
//! whether `On` is a button state or a word, and the field it lands in decides
//! — which is what [`crate::DocumentEditor::fill_field`] does with it.
//!
//! # Bounds
//!
//! An FDF is parsed by the same object reader every PDF is (ruling 1 is that
//! reader's, and its fuzzers'), and its `/Kids` walk carries the field tree's
//! own depth cap, `tinker_pdf_cos::limits::MAX_NEST_DEPTH` — the same bound
//! [`crate::Document::form_fields`] walks a document's tree under, so nothing
//! that tree could hold is refused here — and a visited set holding **every
//! object the walk descends through**: a field, a `/Kids` array, and each link
//! of a reference chain to either. A `/Kids` array two fields share would
//! otherwise be walked once per parent, and an array whose own entries name it
//! as their `/Kids` is walked `2^256` times. An XFDF is parsed by
//! `tinker-pdf-xml` under its default limits, which bound nesting and the
//! event total, and refuse a document type declaration outright.
//!
//! Neither bound says anything about what the reader **hands back**, and that
//! is where a small file becomes a large allocation: a qualified name repeats
//! every ancestor's partial name, a warning names the field it was met in, and
//! an FDF's `/T` or `/V` may be one indirect object every field shares. So
//! both readers spend one budget, [`MAX_FORM_DATA_BYTES`], charged before each
//! copy is made, and a file that asks for more is refused whole
//! ([`FormDataError::TooLarge`]). The qualified name being built is one buffer
//! the walk extends and truncates, not a string per level, so the walk itself
//! holds one name however deep it goes.

use std::collections::{HashMap, HashSet};
use std::mem::size_of;
use std::sync::Arc;

use tinker_pdf_cos::{
    decode_text_string, encode_text_string, limits, CosDocument, DocumentEditor, Field, FieldValue,
    FillError, FillRejection, Name, Object, PdfString, SkippedWidget,
};
use tinker_pdf_xml::{Event, Limits, Source};

/// The XFDF namespace, which an `<xfdf>` element normally declares.
pub const XFDF_NAMESPACE: &str = "http://ns.adobe.com/xfdf/";

/// How many bytes reading one FDF or XFDF file may hand back.
///
/// Everything a [`FormData`] holds is a copy, and a copy is where a small
/// file becomes a large allocation. A qualified name repeats every ancestor's
/// partial name; a [`FormDataWarning`] names the field it was met in; and an
/// FDF's `/T` or `/V` may be one indirect object that every field names. The
/// review of the exchange row found the readers had no answer for that, and
/// the shapes it named, measured with a counting allocator over the read
/// before this cap existed: a 66 KiB FDF of one field with a 32 KiB name and
/// four thousand keys this reader does not read peaked at 184 MB, each
/// warning holding the name; a 12 KiB FDF whose 127 inline nested fields
/// share one 8 KiB `/T` at 252 MB, and a 22 KiB one whose 256 nested fields
/// are indirect objects at 1.05 GB, the name growing a copy of it per level;
/// a 142 KiB FDF of five thousand fields sharing one 16 KiB `/V` at 394 MB;
/// and a 48 KiB XFDF of the first shape at 148 MB. Each is linear in both of
/// its factors — the shared `/T` quadratic in its depth — so megabytes of
/// input ask for terabytes.
///
/// So each read spends one budget, charged before each copy is made. A field
/// costs its [`FieldData`], its name and its value's text; a warning its own
/// size and the text it carries; the source's name its text; and an FDF `/T`
/// that is an indirect reference its decoded text **each time it is read**,
/// because any number of fields may share it and the name is built from it
/// whether or not a field ends up carrying that name. A `/T` written inline is
/// not charged: the walk reads each object once, so its bytes are the file's.
/// A file that asks for more is **refused whole**, [`FormDataError::TooLarge`],
/// rather than read in part: an import is every field of the file or none of
/// them, and the part of a file that fitted imports as a different form.
///
/// | | Bytes |
/// | --- | --- |
/// | The most any fixture in this repository spends: the one built to spend it | 64 MiB |
/// | The honest form beside it: ten thousand fields, 46-byte names, 312-byte values | 4 140 000 |
/// | The most any other file `tests/form_data.rs` reads spends: a name ten thousand partial names deep | 20 125 |
/// | The most a hand-written fixture in `tests/form_data/` spends | 933 |
/// | A 200-page comic archive | 0 |
/// | A 200-page fixed document | 0 |
/// | A 300-page reflowable book | 0 |
/// | **This cap** | **64 MiB** |
///
/// Measured on 2 October 2026 over every read `tests/form_data.rs` makes. The
/// three zeros are facts about those paths: none of them reads form data. A
/// form is the yardstick that matters, and at 64 MiB a form of ten thousand
/// fields could give each a sixty-byte name and a six-kilobyte value and
/// still be read. `a_file_that_asks_for_more_than_the_budget_is_refused` in
/// `tests/form_data.rs` builds each of the shapes above and is what fires it.
pub const MAX_FORM_DATA_BYTES: usize = 64 << 20;

/// One field's value, by fully qualified name (12.7.3.2).
#[derive(Clone, Debug, PartialEq)]
pub struct FieldData {
    /// The fully qualified name: partial names joined with periods.
    pub name: String,
    /// The value. [`FieldValue::None`] is a field the file names and gives no
    /// value, which an import leaves alone.
    pub value: FieldValue,
}

/// What an FDF or XFDF file says, or what one will be written from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FormData {
    /// The fields, in the order the file's tree gives them.
    pub fields: Vec<FieldData>,
    /// The document the data belongs to: FDF's `/F` (12.7.8.3.1 Table 243),
    /// XFDF's `<f href>`. Recorded and written, never opened.
    pub source: Option<String>,
    /// What the file carried and this reader did not read, or read leniently
    /// (ruling 10).
    pub warnings: Vec<FormDataWarning>,
}

/// Something a form data file carried that was not read, or not read as
/// written.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FormDataWarning {
    /// A key of the FDF dictionary, or of a field, or an XFDF element, that
    /// this reader does not read: `/Annots`, `/AP`, `<annots>`,
    /// `<value-richtext>`, … Named once per place it was met.
    NotRead {
        /// The key or element name.
        what: String,
        /// The field it was met in, or empty for the file itself.
        field: String,
    },
    /// A field's `/V` that is neither a string, a name nor an array of them —
    /// a number, a dictionary, a stream. The field is kept with no value.
    ValueUnreadable {
        /// The field it belongs to.
        field: String,
    },
    /// A `/Kids` entry already walked — or a `/Kids` array another field
    /// already has — or one past the depth cap: the tree is not a tree, and
    /// the walk stopped there.
    TreeCut {
        /// The field whose kids were cut.
        field: String,
    },
    /// A field whose fully qualified name is empty — an FDF entry with no
    /// `/T` or an empty one and no named ancestor, an XFDF `<field>` with no
    /// `name` or an empty one and no named ancestor — which nothing can
    /// address. It is not read: handed to [`apply`] as `""`, it would land
    /// in whichever of the document's fields has no name.
    Unnamed,
}

/// Why a form data file could not be read, or written.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FormDataError {
    /// The bytes do not parse as a PDF-syntax file at all.
    NotFdf,
    /// The file parses, but its root has no `/FDF` dictionary (12.7.8.3.1).
    NoFdfDictionary,
    /// The FDF is encrypted; reading one needs its key, and this reader does
    /// not take one.
    Encrypted,
    /// The XML is not well formed, or refused under the reader's bounds.
    Xml(String),
    /// Well-formed XML whose root is not `<xfdf>`.
    NotXfdf,
    /// A name or value XML 1.0 cannot carry — a control character other than
    /// tab, line feed and carriage return — so no XFDF can hold it. The field
    /// is named.
    NotRepresentable(String),
    /// Reading the file would hand back more than [`MAX_FORM_DATA_BYTES`] of
    /// names, values and warnings. Refused whole, because part of a form's
    /// data imports as a different form.
    TooLarge,
}

impl core::fmt::Display for FormDataError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FormDataError::NotFdf => f.write_str("not an FDF file"),
            FormDataError::NoFdfDictionary => f.write_str("the file has no /FDF dictionary"),
            FormDataError::Encrypted => f.write_str("the FDF is encrypted"),
            FormDataError::Xml(error) => write!(f, "not well-formed XML: {error}"),
            FormDataError::NotXfdf => f.write_str("the XML's root is not <xfdf>"),
            FormDataError::NotRepresentable(field) => {
                write!(f, "{field:?} holds a character XML cannot carry")
            }
            FormDataError::TooLarge => write!(
                f,
                "the file asks for more than {MAX_FORM_DATA_BYTES} bytes of form data"
            ),
        }
    }
}

impl std::error::Error for FormDataError {}

impl FormData {
    /// The data a document's fields hold, in the tree's order: every
    /// terminal field with a name, its value as the field-tree reader reads
    /// it.
    ///
    /// A field with no name — no `/T` anywhere up its tree — cannot be
    /// addressed by an import and is left out.
    #[must_use]
    pub fn from_fields(fields: &[Field]) -> FormData {
        FormData {
            fields: fields
                .iter()
                .filter(|field| !field.name.is_empty())
                .map(|field| FieldData {
                    name: field.name.clone(),
                    value: field.value.clone(),
                })
                .collect(),
            source: None,
            warnings: Vec::new(),
        }
    }

    /// Writes this data as an FDF file (12.7.8).
    ///
    /// The names become a tree again — `a.b` and `a.c` are two kids of one
    /// `a` — because 12.7.8.3.2 makes `/T` a *partial* name. A text value is
    /// a text string (7.9.2.2), a state a name, several selections an array
    /// of text strings. A cross-reference table is written although FDF makes
    /// it optional, so the file opens without a repair.
    #[must_use]
    pub fn to_fdf(&self) -> Vec<u8> {
        let tree = Tree::of(&self.fields);
        let mut body = Vec::new();
        body.extend_from_slice(b"<< /FDF << /Fields [");
        for node in &tree.roots.nodes {
            body.push(b' ');
            write_fdf_node(&mut body, node);
        }
        body.extend_from_slice(b" ]");
        if let Some(source) = &self.source {
            body.extend_from_slice(b" /F ");
            write_string(&mut body, &encode_text_string(source, (1, 7)));
        }
        body.extend_from_slice(b" >> >>");

        let mut out = Vec::new();
        // 12.7.8.2: the header names FDF, and the second line's four bytes
        // above 127 are 7.5.2's advice to transfer software, as for a PDF.
        out.extend_from_slice(b"%FDF-1.2\n%\xe2\xe3\xcf\xd3\n");
        let offset = out.len();
        out.extend_from_slice(b"1 0 obj\n");
        out.extend_from_slice(&body);
        out.extend_from_slice(b"\nendobj\n");
        let xref = out.len();
        out.extend_from_slice(b"xref\n0 2\n0000000000 65535 f \n");
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        out.extend_from_slice(b"trailer\n<< /Size 2 /Root 1 0 R >>\nstartxref\n");
        out.extend_from_slice(format!("{xref}\n%%EOF\n").as_bytes());
        out
    }

    /// Writes this data as XFDF.
    ///
    /// Nested `<field>` elements, one `<value>` per selection. A carriage
    /// return is written as `&#13;`, because XML 1.0 turns a literal one into
    /// a line feed on the way in (2.11) and the value would come back
    /// changed; tab, line feed and carriage return in a name, which sits in
    /// an attribute, likewise, because 3.3.3 turns those into spaces.
    ///
    /// # Errors
    ///
    /// [`FormDataError::NotRepresentable`] for a name or value holding a
    /// character XML 1.0 has no way to carry at all (2.2): the other C0
    /// controls. Written some other way, the value would come back different,
    /// and an exchange format that changes a value is worse than a refusal.
    pub fn to_xfdf(&self) -> Result<String, FormDataError> {
        for field in &self.fields {
            let values = match &field.value {
                FieldValue::Text(text) | FieldValue::State(text) => vec![text.as_str()],
                FieldValue::Many(values) => values.iter().map(String::as_str).collect(),
                _ => Vec::new(),
            };
            let clean = |text: &str| text.chars().all(xml_char);
            if !clean(&field.name) || !values.iter().all(|v| clean(v)) {
                return Err(FormDataError::NotRepresentable(field.name.clone()));
            }
        }
        if self
            .source
            .as_deref()
            .is_some_and(|s| !s.chars().all(xml_char))
        {
            return Err(FormDataError::NotRepresentable(String::new()));
        }

        let tree = Tree::of(&self.fields);
        let mut out = String::new();
        out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        out.push_str("<xfdf xmlns=\"");
        out.push_str(XFDF_NAMESPACE);
        out.push_str("\" xml:space=\"preserve\">\n");
        if let Some(source) = &self.source {
            out.push_str("<f href=\"");
            escape_xml(&mut out, source, true);
            out.push_str("\"/>\n");
        }
        out.push_str("<fields>\n");
        for node in &tree.roots.nodes {
            write_xfdf_node(&mut out, node);
        }
        out.push_str("</fields>\n</xfdf>\n");
        Ok(out)
    }
}

/// Reads an FDF file (12.7.8).
///
/// # Errors
///
/// [`FormDataError::NotFdf`] for bytes the object reader cannot open,
/// [`FormDataError::Encrypted`], [`FormDataError::NoFdfDictionary`] for a
/// root with no `/FDF`, and [`FormDataError::TooLarge`] for a file that asks
/// for more than [`MAX_FORM_DATA_BYTES`].
pub fn read_fdf(bytes: &[u8]) -> Result<FormData, FormDataError> {
    let doc = CosDocument::open(bytes.to_vec()).map_err(|_| FormDataError::NotFdf)?;
    if doc.is_encrypted() {
        return Err(FormDataError::Encrypted);
    }
    let root = doc.catalog().ok_or(FormDataError::NoFdfDictionary)?;
    let fdf = doc.resolve_key(&root, doc.intern(b"FDF"));
    let fdf = fdf.as_dict().ok_or(FormDataError::NoFdfDictionary)?;

    let mut answer = Answer::new();
    let fields_key = doc.intern(b"Fields");
    let source_key = doc.intern(b"F");
    for (key, _) in fdf.iter() {
        if *key != fields_key && *key != source_key {
            answer.not_read(name_text(&doc, *key), "")?;
        }
    }
    if let Some(source) = file_name(&doc, &doc.resolve_key(fdf, source_key)) {
        answer.charge(source.len())?;
        answer.data.source = Some(source);
    }

    let mut visited = HashSet::new();
    let mut name = String::new();
    if let Some(fields) = fdf.get(fields_key) {
        match walked(&doc, fields, &mut visited) {
            Some(roots) => {
                for entry in roots.as_array().unwrap_or_default() {
                    read_fdf_field(&doc, entry, &mut name, 0, &mut visited, &mut answer)?;
                }
            }
            None => answer.tree_cut("")?,
        }
    }
    Ok(answer.data)
}

/// Reads an XFDF file: the core named in the module comment.
///
/// # Errors
///
/// [`FormDataError::Xml`] for markup the XML reader refuses,
/// [`FormDataError::NotXfdf`] for a root element that is not `<xfdf>`, and
/// [`FormDataError::TooLarge`] for a file that asks for more than
/// [`MAX_FORM_DATA_BYTES`].
pub fn read_xfdf(bytes: &[u8]) -> Result<FormData, FormDataError> {
    let source = Source::new(bytes).map_err(|e| FormDataError::Xml(e.to_string()))?;
    let mut answer = Answer::new();

    // The element names from the root down; an element this reader does not
    // read is on the path as `None`, and so is everything inside it.
    let mut path: Vec<Option<String>> = Vec::new();
    // The qualified name of the innermost open `<field>`: one buffer, each
    // field appending its partial name and truncating it again on the way
    // out, so a field nested two hundred deep holds one name rather than two
    // hundred growing copies of it.
    let mut name = String::new();
    // One entry per open `<field>`.
    let mut open: Vec<OpenField> = Vec::new();
    // The text of the `<value>` being read.
    let mut value: Option<String> = None;
    let mut root_seen = false;

    for event in source.reader(&Limits::DEFAULT) {
        let event = event.map_err(|e| FormDataError::Xml(e.to_string()))?;
        match event {
            Event::Start(element) => {
                let local = element.local();
                if !root_seen {
                    let namespace = element.namespace();
                    let ours = namespace.is_none() || namespace == Some(XFDF_NAMESPACE);
                    if local != "xfdf" || !ours {
                        return Err(FormDataError::NotXfdf);
                    }
                    root_seen = true;
                    path.push(Some(local.to_string()));
                    continue;
                }
                let parent = match path.last() {
                    Some(Some(parent)) => parent.as_str(),
                    // Inside something unread: walked for nesting, not read.
                    _ => {
                        path.push(None);
                        continue;
                    }
                };
                let read = match (parent, local) {
                    ("xfdf", "f") => {
                        if let Some(href) = element.attribute(None, "href") {
                            answer.charge(href.len())?;
                            answer.data.source = Some(href.to_string());
                        }
                        true
                    }
                    ("xfdf", "fields") => true,
                    ("fields" | "field", "field") => {
                        if let Some(parent) = open.last_mut() {
                            parent.has_kids = true;
                        }
                        open.push(OpenField {
                            mark: name.len(),
                            values: None,
                            has_kids: false,
                        });
                        // 12.7.3.2's rule, as the field-tree reader applies
                        // it: a node with no name contributes nothing.
                        if let Some(own) = element.attribute(None, "name") {
                            extend(&mut name, own);
                        }
                        true
                    }
                    ("field", "value") => {
                        value = Some(String::new());
                        true
                    }
                    _ => false,
                };
                if read {
                    path.push(Some(local.to_string()));
                } else {
                    answer.not_read(local.to_string(), &name)?;
                    path.push(None);
                }
            }
            Event::End(_) => match path.pop().flatten().as_deref() {
                Some("value") => {
                    if let (Some(text), Some(field)) = (value.take(), open.last_mut()) {
                        field.values.get_or_insert_with(Vec::new).push(text);
                    }
                }
                Some("field") => {
                    let Some(field) = open.pop() else {
                        continue;
                    };
                    let value = match field.values {
                        Some(mut list) if list.len() == 1 => {
                            Some(FieldValue::Text(list.pop().unwrap_or_default()))
                        }
                        Some(list) => Some(FieldValue::Many(list)),
                        // A field with fields beneath it and no value of its
                        // own is a group, and says nothing about itself.
                        None if field.has_kids => None,
                        None => Some(FieldValue::None),
                    };
                    if let Some(value) = value {
                        answer.charge(value_cost(&value))?;
                        answer.field(&name, value)?;
                    }
                    name.truncate(field.mark);
                }
                _ => {}
            },
            Event::Text(text) | Event::Cdata(text) => {
                if matches!(path.last(), Some(Some(last)) if last == "value") {
                    if let Some(value) = value.as_mut() {
                        value.push_str(&text);
                    }
                }
            }
            Event::Comment(_) | Event::Instruction { .. } => {}
        }
    }
    if !root_seen {
        return Err(FormDataError::NotXfdf);
    }
    Ok(answer.data)
}

/// A `<field>` element being read.
struct OpenField {
    /// The length of the qualified name before this field's partial name
    /// was appended, which is where it is truncated back to.
    mark: usize,
    values: Option<Vec<String>>,
    has_kids: bool,
}

/// Appends a partial name to a qualified one, as 12.7.3.2 joins them: with a
/// period, unless nothing named comes before it.
fn extend(name: &mut String, partial: &str) {
    if !name.is_empty() {
        name.push('.');
    }
    name.push_str(partial);
}

/// What one read has built, and what it has left to spend of
/// [`MAX_FORM_DATA_BYTES`].
struct Answer {
    data: FormData,
    left: usize,
}

impl Answer {
    fn new() -> Answer {
        Answer {
            data: FormData::default(),
            left: MAX_FORM_DATA_BYTES,
        }
    }

    /// Takes `bytes` from the budget, or refuses the file.
    fn charge(&mut self, bytes: usize) -> Result<(), FormDataError> {
        self.left = self
            .left
            .checked_sub(bytes)
            .ok_or(FormDataError::TooLarge)?;
        Ok(())
    }

    /// A field, its value already paid for. A field nothing can address — an
    /// empty qualified name — is named in the warnings instead.
    fn field(&mut self, name: &str, value: FieldValue) -> Result<(), FormDataError> {
        if name.is_empty() {
            return self.warn(0, || FormDataWarning::Unnamed);
        }
        self.charge(size_of::<FieldData>().saturating_add(name.len()))?;
        self.data.fields.push(FieldData {
            name: name.to_string(),
            value,
        });
        Ok(())
    }

    /// A warning costing its own size and `text` bytes, built only once it
    /// has been paid for.
    fn warn(
        &mut self,
        text: usize,
        warning: impl FnOnce() -> FormDataWarning,
    ) -> Result<(), FormDataError> {
        self.charge(size_of::<FormDataWarning>().saturating_add(text))?;
        self.data.warnings.push(warning());
        Ok(())
    }

    fn not_read(&mut self, what: String, field: &str) -> Result<(), FormDataError> {
        self.warn(what.len().saturating_add(field.len()), || {
            FormDataWarning::NotRead {
                what,
                field: field.to_string(),
            }
        })
    }

    fn tree_cut(&mut self, field: &str) -> Result<(), FormDataError> {
        self.warn(field.len(), || FormDataWarning::TreeCut {
            field: field.to_string(),
        })
    }

    fn value_unreadable(&mut self, field: &str) -> Result<(), FormDataError> {
        self.warn(field.len(), || FormDataWarning::ValueUnreadable {
            field: field.to_string(),
        })
    }
}

/// What a value costs against the budget: its text, and for a selection
/// each entry's `String` as well. The same for both readers, so a value read
/// from one format costs what it costs read back from the other.
fn value_cost(value: &FieldValue) -> usize {
    match value {
        FieldValue::Text(text) | FieldValue::State(text) => text.len(),
        FieldValue::Many(values) => values.iter().fold(0usize, |sum, value| {
            sum.saturating_add(size_of::<String>().saturating_add(value.len()))
        }),
        _ => 0,
    }
}

/// Imports form data into an editor: every field with a value, all of them
/// or none of them.
///
/// Through [`DocumentEditor::set_field_values`], so a name resolves against
/// the same field tree export walked, a value a field would refuse from a
/// user is refused, and the first refusal rolls back every field before it.
/// A text or a state goes in as it is — a check box takes the name of the
/// state to show — and a multiple selection of one goes in as that one. A
/// field the data names with no value is left alone.
///
/// # Errors
///
/// [`FillRejection`] naming the first field that would not take its value,
/// with nothing written. A field the document does not have is
/// [`FillError::NoSuchField`], and so is the empty name: the field-tree
/// walk gives `""` to every field with no `/T` up its tree, so it addresses
/// whichever of them comes first rather than a field the data meant — the
/// readers never produce one ([`FormDataWarning::Unnamed`]), and a
/// [`FormData`] built by hand is held to the same rule. A selection of more
/// than one value is [`FillError::ValueRefused`], because this build fills
/// one value per field.
pub fn apply(
    editor: &mut DocumentEditor,
    data: &FormData,
) -> Result<Vec<SkippedWidget>, FillRejection> {
    let mut pairs: Vec<(&str, &str)> = Vec::new();
    for field in &data.fields {
        let value = match &field.value {
            FieldValue::Text(text) | FieldValue::State(text) => text.as_str(),
            FieldValue::Many(values) => match values.as_slice() {
                [one] => one.as_str(),
                _ => {
                    return Err(FillRejection {
                        field: field.name.clone(),
                        reason: FillError::ValueRefused,
                    })
                }
            },
            _ => continue,
        };
        if field.name.is_empty() {
            return Err(FillRejection {
                field: String::new(),
                reason: FillError::NoSuchField,
            });
        }
        pairs.push((field.name.as_str(), value));
    }
    editor.set_field_values(&pairs)
}

// ---- the name tree both writers share ------------------------------------

/// One node of the tree the qualified names describe.
struct Node<'a> {
    partial: &'a str,
    value: Option<&'a FieldValue>,
    kids: Level<'a>,
}

/// The nodes of one level, in first-seen order, and an index into them.
#[derive(Default)]
struct Level<'a> {
    nodes: Vec<Node<'a>>,
    /// For each partial name, the first node carrying it that can be a
    /// parent ([`can_parent`]). Once there is one it stays the answer: nodes
    /// are only ever appended, and a node's value only ever goes from none
    /// to a value that can parent. So finding where a name descends is one
    /// probe of this map, where it was a scan of every sibling before it —
    /// forty thousand flat names took seven seconds to write that way, and
    /// ten times as many would have taken a hundred times as long.
    parents: HashMap<&'a str, usize>,
}

impl<'a> Level<'a> {
    /// Appends a node, recorded as its name's parent if it is the first
    /// that can be one, and answers its index.
    fn push(&mut self, node: Node<'a>) -> usize {
        let index = self.nodes.len();
        if can_parent(&node) {
            self.parents.entry(node.partial).or_insert(index);
        }
        self.nodes.push(node);
        index
    }
}

struct Tree<'a> {
    roots: Level<'a>,
    /// How many existing nodes the inserts compared a partial name with: at
    /// most one per partial name, the node its level's index names, which is
    /// what makes writing linear in the names. A scan of the level compares
    /// it with every sibling before it.
    #[cfg_attr(not(test), allow(dead_code))]
    compared: usize,
}

/// How deep the written tree goes before the rest of a name stays whole.
///
/// Not a cap on what is written — every name is written, exactly — but on
/// how it is *spelled*: a name of more partial names than this has its last
/// ones kept together, periods and all, in the deepest `/T` or `name`, and
/// reads back as the same qualified name because a reader joins partial
/// names with periods. It is a quarter of the object parser's nesting bound,
/// `limits::MAX_NEST_DEPTH`, so an inline FDF tree (a dictionary and a
/// `/Kids` array a level) and an XFDF one (an element a level, under
/// `tinker-pdf-xml`'s `MAX_XML_DEPTH`, which is the same 256) both read back
/// whole — and so writing a name of a million periods, which a caller can
/// hand over, does not recurse a million frames deep.
const WRITTEN_DEPTH: usize = limits::MAX_NEST_DEPTH as usize / 4;

impl<'a> Tree<'a> {
    /// The tree `fields` describe, in first-seen order.
    ///
    /// Each field is its own leaf: a name that repeats gets a second node
    /// beside the first rather than overwriting it, so what is written is
    /// everything that was given.
    fn of(fields: &'a [FieldData]) -> Tree<'a> {
        let mut roots = Level::default();
        let mut compared = 0;
        for field in fields {
            // A name with an empty partial name in it — a leading, trailing
            // or doubled period — is written whole. Split, the empty `/T`
            // would read back as no name at all, which is how a reader joins
            // one (12.7.3.2: a node without `/T` contributes nothing), and
            // `.x` would come back as `x`.
            let parts: Vec<&str> = if field.name.split('.').any(str::is_empty) {
                vec![field.name.as_str()]
            } else {
                field.name.splitn(WRITTEN_DEPTH, '.').collect()
            };
            insert(&mut roots, &parts, &field.value, &mut compared);
        }
        Tree { roots, compared }
    }
}

/// Whether a node can hold kids and still read back as the field it is.
///
/// A reader takes a node with kids and no value for a group, so a field whose
/// value is *none* cannot also be a parent: it stands beside a group of the
/// same name instead, and both read back. Any other value can — `/V` beside
/// `/Kids`, `<value>` beside `<field>` — and does.
fn can_parent(node: &Node<'_>) -> bool {
    !matches!(node.value, Some(FieldValue::None))
}

fn insert<'a>(
    level: &mut Level<'a>,
    parts: &[&'a str],
    value: &'a FieldValue,
    compared: &mut usize,
) {
    let Some((first, rest)) = parts.split_first() else {
        return;
    };
    let parent = level.parents.get(first).copied();
    if parent.is_some() {
        *compared += 1;
    }
    if rest.is_empty() {
        // A leaf: joins a group of that name that has no value yet — unless
        // it has none to give, see `can_parent` — or stands beside whatever
        // else carries the name. A group with no value is always its name's
        // parent, because one is only made where nothing else could be.
        if !matches!(value, FieldValue::None) {
            if let Some(node) = parent
                .and_then(|index| level.nodes.get_mut(index))
                .filter(|node| node.value.is_none())
            {
                node.value = Some(value);
                return;
            }
        }
        level.push(Node {
            partial: first,
            value: Some(value),
            kids: Level::default(),
        });
        return;
    }
    let index = match parent {
        Some(index) => index,
        None => level.push(Node {
            partial: first,
            value: None,
            kids: Level::default(),
        }),
    };
    if let Some(node) = level.nodes.get_mut(index) {
        insert(&mut node.kids, rest, value, compared);
    }
}

// ---- FDF ----------------------------------------------------------------

fn write_fdf_node(out: &mut Vec<u8>, node: &Node<'_>) {
    out.extend_from_slice(b"<< /T ");
    write_string(out, &encode_text_string(node.partial, (1, 7)));
    match node.value {
        Some(FieldValue::Text(text)) => {
            out.extend_from_slice(b" /V ");
            write_string(out, &encode_text_string(text, (1, 7)));
        }
        Some(FieldValue::State(state)) => {
            out.extend_from_slice(b" /V ");
            write_name(out, state.as_bytes());
        }
        Some(FieldValue::Many(values)) => {
            out.extend_from_slice(b" /V [");
            for value in values {
                out.push(b' ');
                write_string(out, &encode_text_string(value, (1, 7)));
            }
            out.extend_from_slice(b" ]");
        }
        _ => {}
    }
    if !node.kids.nodes.is_empty() {
        out.extend_from_slice(b" /Kids [");
        for kid in &node.kids.nodes {
            out.push(b' ');
            write_fdf_node(out, kid);
        }
        out.extend_from_slice(b" ]");
    }
    out.extend_from_slice(b" >>");
}

/// A string as 7.3.4 spells it: a literal for printable ASCII, escaped where
/// 7.3.4.2 says; hex for anything else, which is what a marked text string
/// is written as anyway.
fn write_string(out: &mut Vec<u8>, string: &PdfString) {
    let printable = string.bytes.iter().all(|b| (0x20..0x7F).contains(b));
    if printable && !string.hex {
        out.push(b'(');
        for &byte in &string.bytes {
            if matches!(byte, b'(' | b')' | b'\\') {
                out.push(b'\\');
            }
            out.push(byte);
        }
        out.push(b')');
    } else {
        out.push(b'<');
        for byte in &string.bytes {
            out.extend_from_slice(format!("{byte:02X}").as_bytes());
        }
        out.push(b'>');
    }
}

/// A name as 7.3.5 spells it: regular characters as themselves, everything
/// else — delimiters, whitespace, `#` and bytes outside `!`..`~` — as `#xx`.
fn write_name(out: &mut Vec<u8>, bytes: &[u8]) {
    out.push(b'/');
    for &byte in bytes {
        let regular = (0x21..0x7F).contains(&byte)
            && !matches!(
                byte,
                b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%' | b'#'
            );
        if regular {
            out.push(byte);
        } else {
            out.extend_from_slice(format!("#{byte:02X}").as_bytes());
        }
    }
}

fn name_text(doc: &CosDocument, name: Name) -> String {
    doc.name_bytes(name)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default()
}

/// A file specification's name: a string, or a dictionary's `/UF` or `/F`
/// (7.11).
fn file_name(doc: &CosDocument, spec: &Object) -> Option<String> {
    match spec {
        Object::String(text) => Some(decode_text_string(&text.bytes)),
        Object::Dict(dict) => [b"UF".as_slice(), b"F"].iter().find_map(|key| {
            doc.resolve_key(dict, doc.intern(key))
                .as_string()
                .map(|s| decode_text_string(&s.bytes))
        }),
        _ => None,
    }
}

/// The keys of an FDF field dictionary this reader reads (12.7.8.3.2 Table
/// 246): the rest are named in the warnings.
const FIELD_KEYS_READ: [&[u8]; 3] = [b"T", b"V", b"Kids"];

/// An object as the walk holds it: borrowed where it was written inline,
/// shared where it was loaded, and never deep-copied — a direct `/Kids` array
/// copied at every level is the whole subtree beneath it copied once per
/// ancestor, held all at once down the recursion.
enum Held<'o> {
    Inline(&'o Object),
    Loaded(Arc<Object>),
}

impl std::ops::Deref for Held<'_> {
    type Target = Object;

    fn deref(&self) -> &Object {
        match self {
            Held::Inline(object) => object,
            Held::Loaded(object) => object,
        }
    }
}

/// `object`, for the walk to descend through: resolved one reference at a
/// time, every object number on the way marked as walked, and `None` when
/// one of them already was. Marking only the first reference would let two
/// references to a third reach the same array twice. A chain past
/// `limits::MAX_RESOLVE_DEPTH` is null, as [`CosDocument::resolve`] makes it.
fn walked<'o>(
    doc: &CosDocument,
    object: &'o Object,
    visited: &mut HashSet<u32>,
) -> Option<Held<'o>> {
    let Some(mut reference) = object.as_objref() else {
        return Some(Held::Inline(object));
    };
    for _ in 0..limits::MAX_RESOLVE_DEPTH {
        if !visited.insert(reference.num) {
            return None;
        }
        let Ok(loaded) = doc.get(reference) else {
            break;
        };
        match loaded.as_objref() {
            Some(next) => reference = next,
            None => return Some(Held::Loaded(loaded)),
        }
    }
    Some(Held::Loaded(Arc::new(Object::Null)))
}

/// `object` resolved, for an entry the walk reads rather than descends
/// through — a `/T`, a `/V` — which any number of fields may share, so it is
/// not marked; what it costs is charged where it is copied instead.
fn held<'o>(doc: &CosDocument, object: &'o Object) -> Held<'o> {
    match object.as_objref() {
        Some(_) => Held::Loaded(doc.resolve(object)),
        None => Held::Inline(object),
    }
}

/// One entry of a `/Fields` or `/Kids` array. `name` is the qualified name of
/// its parent on the way in, and is again on the way out.
fn read_fdf_field(
    doc: &CosDocument,
    entry: &Object,
    name: &mut String,
    depth: u32,
    visited: &mut HashSet<u32>,
    answer: &mut Answer,
) -> Result<(), FormDataError> {
    let Some(resolved) = walked(doc, entry, visited) else {
        return answer.tree_cut(name);
    };
    let Some(dict) = resolved.as_dict() else {
        return Ok(());
    };

    let mark = name.len();
    if let Some(title) = dict.get(doc.intern(b"T")) {
        let own = held(doc, title)
            .as_string()
            .map(|s| decode_text_string(&s.bytes));
        if let Some(own) = own {
            // An inline /T is read once, because the walk reads its field
            // once; an indirect one may be every field's /T, and is paid for
            // each time it is read.
            if title.as_objref().is_some() {
                answer.charge(own.len())?;
            }
            extend(name, &own);
        }
    }
    let read = read_fdf_entries(doc, dict, name, depth, visited, answer);
    name.truncate(mark);
    read
}

/// The rest of a field dictionary, once its name is known.
fn read_fdf_entries(
    doc: &CosDocument,
    dict: &tinker_pdf_cos::Dict,
    name: &mut String,
    depth: u32,
    visited: &mut HashSet<u32>,
    answer: &mut Answer,
) -> Result<(), FormDataError> {
    for (key, _) in dict.iter() {
        let bytes = doc.name_bytes(*key).unwrap_or_default();
        if !FIELD_KEYS_READ.contains(&bytes.as_ref()) {
            answer.not_read(String::from_utf8_lossy(&bytes).into_owned(), name)?;
        }
    }

    // A /Kids array another field already has is cut rather than walked
    // again: it is the same fields a second time, and an array whose own
    // entries name it as their /Kids is the same fields 2^256 times.
    let (kids, cut) = match dict.get(Name::KIDS).map(|kids| walked(doc, kids, visited)) {
        None => (None, false),
        Some(None) => (None, true),
        Some(Some(kids)) => (Some(kids), false),
    };
    let kids = kids
        .as_deref()
        .and_then(Object::as_array)
        .filter(|k| !k.is_empty());
    let value = dict.get(doc.intern(b"V"));
    if value.is_some() || (kids.is_none() && !cut) {
        let read = read_value(doc, value, answer)?;
        if matches!(read, FieldValue::None) && value.is_some() {
            answer.value_unreadable(name)?;
        }
        answer.field(name, read)?;
    }
    if cut {
        return answer.tree_cut(name);
    }
    if let Some(kids) = kids {
        // The field tree's own bound: nothing a document's tree could hold is
        // refused here, and nothing deeper is followed.
        if depth >= limits::MAX_NEST_DEPTH {
            return answer.tree_cut(name);
        }
        for kid in kids {
            read_fdf_field(doc, kid, name, depth + 1, visited, answer)?;
        }
    }
    Ok(())
}

/// A `/V` as the field-tree reader types one (12.7.8.3.2): a text string, a
/// name, or an array of either — each copy paid for before the next is made,
/// so an array naming one shared string a million times stops at the budget
/// rather than after the millionth copy.
fn read_value(
    doc: &CosDocument,
    value: Option<&Object>,
    answer: &mut Answer,
) -> Result<FieldValue, FormDataError> {
    let Some(value) = value else {
        return Ok(FieldValue::None);
    };
    Ok(match &*held(doc, value) {
        Object::String(text) => {
            let text = decode_text_string(&text.bytes);
            answer.charge(text.len())?;
            FieldValue::Text(text)
        }
        Object::Name(name) => {
            let state = name_text(doc, *name);
            answer.charge(state.len())?;
            FieldValue::State(state)
        }
        Object::Array(items) => {
            let mut values = Vec::new();
            for item in items {
                let text = match &*held(doc, item) {
                    Object::String(text) => decode_text_string(&text.bytes),
                    Object::Name(name) => name_text(doc, *name),
                    _ => continue,
                };
                answer.charge(size_of::<String>().saturating_add(text.len()))?;
                values.push(text);
            }
            if values.is_empty() {
                FieldValue::None
            } else {
                FieldValue::Many(values)
            }
        }
        _ => FieldValue::None,
    })
}

// ---- XFDF ---------------------------------------------------------------

/// Whether XML 1.0 can carry `c` at all (2.2 `Char`).
fn xml_char(c: char) -> bool {
    !matches!(c, '\u{0}'..='\u{8}' | '\u{B}' | '\u{C}' | '\u{E}'..='\u{1F}' | '\u{FFFE}' | '\u{FFFF}')
}

/// Escapes text for element content, or for a double-quoted attribute.
fn escape_xml(out: &mut String, text: &str, attribute: bool) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attribute => out.push_str("&quot;"),
            // 2.11: a literal carriage return reads back as a line feed.
            '\r' => out.push_str("&#13;"),
            // 3.3.3: whitespace in an attribute reads back as a space.
            '\t' if attribute => out.push_str("&#9;"),
            '\n' if attribute => out.push_str("&#10;"),
            c => out.push(c),
        }
    }
}

fn write_xfdf_node(out: &mut String, node: &Node<'_>) {
    out.push_str("<field name=\"");
    escape_xml(out, node.partial, true);
    out.push_str("\">");
    match node.value {
        Some(FieldValue::Text(text)) | Some(FieldValue::State(text)) => {
            out.push_str("<value>");
            escape_xml(out, text, false);
            out.push_str("</value>");
        }
        Some(FieldValue::Many(values)) => {
            for value in values {
                out.push_str("<value>");
                escape_xml(out, value, false);
                out.push_str("</value>");
            }
        }
        _ => {}
    }
    for kid in &node.kids.nodes {
        write_xfdf_node(out, kid);
    }
    out.push_str("</field>\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writing names back into a tree compares each partial name with one
    /// sibling at most, however many siblings there are.
    ///
    /// The review of the exchange row found the insert scanning its whole
    /// level for every name: forty thousand flat fields took seven seconds to
    /// write, ten times as many would take a hundred times as long, and a file
    /// of them reads back in a fraction of a second — so reading a large file
    /// and writing it out again took minutes. Counted rather than timed,
    /// because a clock passes on a fast machine with the scan put back.
    #[test]
    fn writing_names_compares_each_partial_name_once() {
        let field = |name: String| FieldData {
            name,
            value: FieldValue::Text("v".into()),
        };
        let mut fields: Vec<FieldData> = (0..5_000).map(|i| field(format!("f{i}"))).collect();
        for group in 0..50 {
            fields.extend((0..100).map(|kid| field(format!("g{group}.k{kid}"))));
        }
        // And every flat name again: a second leaf beside the first.
        fields.extend((0..5_000).map(|i| field(format!("f{i}"))));

        let tree = Tree::of(&fields);
        let partials: usize = fields.iter().map(|f| f.name.split('.').count()).sum();
        assert!(
            tree.compared <= partials,
            "{} comparisons for {partials} partial names",
            tree.compared
        );
        // The same tree a scan built: each repeat a leaf of its own, each
        // group's kids under the one group.
        assert_eq!(tree.roots.nodes.len(), 5_000 + 50 + 5_000);
        let groups: Vec<&Node<'_>> = tree
            .roots
            .nodes
            .iter()
            .filter(|node| node.partial.starts_with('g'))
            .collect();
        assert_eq!(groups.len(), 50);
        assert!(groups.iter().all(|group| group.kids.nodes.len() == 100));
    }
}
