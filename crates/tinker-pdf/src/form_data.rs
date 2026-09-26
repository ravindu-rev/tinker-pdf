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
//! reader's, and its fuzzers'), and its `/Kids` walk carries a visited set and
//! the field tree's own depth cap, `tinker_pdf_cos::limits::MAX_NEST_DEPTH` —
//! the same bound [`crate::Document::form_fields`] walks a document's tree
//! under, so nothing that tree could hold is refused here. An XFDF is parsed
//! by `tinker-pdf-xml` under its default limits, which bound nesting and the
//! event total, and refuse a document type declaration outright.

use std::collections::HashSet;

use tinker_pdf_cos::{
    decode_text_string, encode_text_string, limits, CosDocument, DocumentEditor, Field, FieldValue,
    FillError, FillRejection, Name, ObjRef, Object, PdfString, SkippedWidget,
};
use tinker_pdf_xml::{Event, Limits, Source};

/// The XFDF namespace, which an `<xfdf>` element normally declares.
pub const XFDF_NAMESPACE: &str = "http://ns.adobe.com/xfdf/";

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
    /// A `/Kids` entry already walked, or one past the depth cap: the tree is
    /// not a tree, and the walk stopped there.
    TreeCut {
        /// The field whose kids were cut.
        field: String,
    },
    /// A field entry with no name and no kids, which nothing can address.
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
        for node in &tree.roots {
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
        for node in &tree.roots {
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
/// [`FormDataError::Encrypted`], and [`FormDataError::NoFdfDictionary`] for a
/// root with no `/FDF`.
pub fn read_fdf(bytes: &[u8]) -> Result<FormData, FormDataError> {
    let doc = CosDocument::open(bytes.to_vec()).map_err(|_| FormDataError::NotFdf)?;
    if doc.is_encrypted() {
        return Err(FormDataError::Encrypted);
    }
    let root = doc.catalog().ok_or(FormDataError::NoFdfDictionary)?;
    let fdf = doc.resolve_key(&root, doc.intern(b"FDF"));
    let fdf = fdf.as_dict().ok_or(FormDataError::NoFdfDictionary)?;

    let mut data = FormData::default();
    let fields_key = doc.intern(b"Fields");
    let source_key = doc.intern(b"F");
    for (key, _) in fdf.iter() {
        if *key != fields_key && *key != source_key {
            data.warnings.push(FormDataWarning::NotRead {
                what: name_text(&doc, *key),
                field: String::new(),
            });
        }
    }
    data.source = file_name(&doc, &doc.resolve_key(fdf, source_key));

    let fields = doc.resolve_key(fdf, fields_key);
    let mut visited = HashSet::new();
    if let Some(roots) = fields.as_array() {
        for entry in roots {
            read_fdf_field(&doc, entry, "", 0, &mut visited, &mut data);
        }
    }
    Ok(data)
}

/// Reads an XFDF file: the core named in the module comment.
///
/// # Errors
///
/// [`FormDataError::Xml`] for markup the XML reader refuses, and
/// [`FormDataError::NotXfdf`] for a root element that is not `<xfdf>`.
pub fn read_xfdf(bytes: &[u8]) -> Result<FormData, FormDataError> {
    let source = Source::new(bytes).map_err(|e| FormDataError::Xml(e.to_string()))?;
    let mut data = FormData::default();

    // The element names from the root down; an element this reader does not
    // read is on the path as `None`, and so is everything inside it.
    let mut path: Vec<Option<String>> = Vec::new();
    // One entry per open `<field>`: its qualified name, the `<value>`s
    // gathered so far, and whether a `<field>` has opened inside it.
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
                        data.source = element.attribute(None, "href").map(str::to_string);
                        true
                    }
                    ("xfdf", "fields") => true,
                    ("fields" | "field", "field") => {
                        let prefix = open.last().map_or("", |f| f.name.as_str());
                        // 12.7.3.2's rule, as the field-tree reader applies
                        // it: a node with no name contributes nothing.
                        let name = match (element.attribute(None, "name"), prefix.is_empty()) {
                            (Some(own), true) => own.to_string(),
                            (Some(own), false) => format!("{prefix}.{own}"),
                            (None, _) => prefix.to_string(),
                        };
                        if let Some(parent) = open.last_mut() {
                            parent.has_kids = true;
                        }
                        open.push(OpenField {
                            name,
                            values: None,
                            has_kids: false,
                        });
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
                    data.warnings.push(FormDataWarning::NotRead {
                        what: local.to_string(),
                        field: open.last().map(|f| f.name.clone()).unwrap_or_default(),
                    });
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
                            FieldValue::Text(list.pop().unwrap_or_default())
                        }
                        Some(list) => FieldValue::Many(list),
                        // A field with fields beneath it and no value of its
                        // own is a group, and says nothing about itself.
                        None if field.has_kids => continue,
                        None => FieldValue::None,
                    };
                    data.fields.push(FieldData {
                        name: field.name,
                        value,
                    });
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
    Ok(data)
}

/// A `<field>` element being read.
struct OpenField {
    name: String,
    values: Option<Vec<String>>,
    has_kids: bool,
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
/// [`FillError::NoSuchField`]; a selection of more than one value is
/// [`FillError::ValueRefused`], because this build fills one value per field.
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
        pairs.push((field.name.as_str(), value));
    }
    editor.set_field_values(&pairs)
}

// ---- the name tree both writers share ------------------------------------

/// One node of the tree the qualified names describe.
struct Node<'a> {
    partial: &'a str,
    value: Option<&'a FieldValue>,
    kids: Vec<Node<'a>>,
}

struct Tree<'a> {
    roots: Vec<Node<'a>>,
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
        let mut roots: Vec<Node<'a>> = Vec::new();
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
            insert(&mut roots, &parts, &field.value);
        }
        Tree { roots }
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

fn insert<'a>(level: &mut Vec<Node<'a>>, parts: &[&'a str], value: &'a FieldValue) {
    let Some((first, rest)) = parts.split_first() else {
        return;
    };
    if rest.is_empty() {
        // A leaf: joins a group of that name that has no value yet — unless
        // it has none to give, see `can_parent` — or stands beside whatever
        // else carries the name.
        let joins = !matches!(value, FieldValue::None);
        if let Some(node) = level
            .iter_mut()
            .find(|n| joins && n.partial == *first && n.value.is_none() && !n.kids.is_empty())
        {
            node.value = Some(value);
        } else {
            level.push(Node {
                partial: first,
                value: Some(value),
                kids: Vec::new(),
            });
        }
        return;
    }
    let index = match level
        .iter()
        .position(|n| n.partial == *first && can_parent(n))
    {
        Some(index) => index,
        None => {
            level.push(Node {
                partial: first,
                value: None,
                kids: Vec::new(),
            });
            level.len() - 1
        }
    };
    if let Some(node) = level.get_mut(index) {
        insert(&mut node.kids, rest, value);
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
    if !node.kids.is_empty() {
        out.extend_from_slice(b" /Kids [");
        for kid in &node.kids {
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

fn read_fdf_field(
    doc: &CosDocument,
    entry: &Object,
    prefix: &str,
    depth: u32,
    visited: &mut HashSet<ObjRef>,
    data: &mut FormData,
) {
    if let Some(r) = entry.as_objref() {
        if !visited.insert(r) {
            data.warnings.push(FormDataWarning::TreeCut {
                field: prefix.to_string(),
            });
            return;
        }
    }
    let resolved = doc.resolve(entry);
    let Some(dict) = resolved.as_dict() else {
        return;
    };

    let own = doc
        .resolve_key(dict, doc.intern(b"T"))
        .as_string()
        .map(|s| decode_text_string(&s.bytes));
    let name = match (&own, prefix.is_empty()) {
        (Some(own), true) => own.clone(),
        (Some(own), false) => format!("{prefix}.{own}"),
        (None, _) => prefix.to_string(),
    };

    for (key, _) in dict.iter() {
        let bytes = doc.name_bytes(*key).unwrap_or_default();
        if !FIELD_KEYS_READ.contains(&bytes.as_ref()) {
            data.warnings.push(FormDataWarning::NotRead {
                what: name_text(doc, *key),
                field: name.clone(),
            });
        }
    }

    let kids = doc.resolve_key(dict, Name::KIDS);
    let kids = kids.as_array().filter(|k| !k.is_empty());
    let value_key = doc.intern(b"V");
    if dict.get(value_key).is_some() || kids.is_none() {
        let value = read_value(doc, dict.get(value_key));
        if matches!(value, FieldValue::None) && dict.get(value_key).is_some() {
            data.warnings.push(FormDataWarning::ValueUnreadable {
                field: name.clone(),
            });
        }
        if name.is_empty() && own.is_none() {
            data.warnings.push(FormDataWarning::Unnamed);
        } else {
            data.fields.push(FieldData {
                name: name.clone(),
                value,
            });
        }
    }
    if let Some(kids) = kids {
        // The field tree's own bound: nothing a document's tree could hold is
        // refused here, and nothing deeper is followed.
        if depth >= limits::MAX_NEST_DEPTH {
            data.warnings.push(FormDataWarning::TreeCut { field: name });
            return;
        }
        for kid in kids {
            read_fdf_field(doc, kid, &name, depth + 1, visited, data);
        }
    }
}

/// A `/V` as the field-tree reader types one (12.7.8.3.2): a text string, a
/// name, or an array of either.
fn read_value(doc: &CosDocument, value: Option<&Object>) -> FieldValue {
    let Some(value) = value else {
        return FieldValue::None;
    };
    match doc.resolve(value).as_ref() {
        Object::String(text) => FieldValue::Text(decode_text_string(&text.bytes)),
        Object::Name(name) => FieldValue::State(name_text(doc, *name)),
        Object::Array(items) => {
            let values: Vec<String> = items
                .iter()
                .filter_map(|item| match doc.resolve(item).as_ref() {
                    Object::String(text) => Some(decode_text_string(&text.bytes)),
                    Object::Name(name) => Some(name_text(doc, *name)),
                    _ => None,
                })
                .collect();
            if values.is_empty() {
                FieldValue::None
            } else {
                FieldValue::Many(values)
            }
        }
        _ => FieldValue::None,
    }
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
    for kid in &node.kids {
        write_xfdf_node(out, kid);
    }
    out.push_str("</field>\n");
}
