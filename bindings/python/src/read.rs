//! The read surface beyond pages: `/Info`, the outline, links, attachments,
//! XMP and warnings, and a page's command stream, as Python objects.
//!
//! Each class is the facade's own type with its fields as read-only
//! attributes (ruling 11): `OutlineItem` keeps its children, `Destination`
//! keeps its three arms apart (ruling 6), an absent value is `None` and an
//! empty one is an empty string. A byte string the file stores as bytes —
//! a destination name, a URI — is `bytes`, because 12.3.2.3 compares names
//! byte for byte and decoding one would be a guess.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyFloat, PyList};

use tinker_pdf::{
    Action, CommandBudgets, CommandField, CommandKind, CommandRecord, CommandStream, DestKind,
    Destination, Document, Trapped, Usage,
};

/// The `/Info` dictionary (14.3.3), decoded. `None` is the entry absent;
/// `""` is the producer writing an empty one.
#[pyclass(name = "Metadata", get_all, frozen)]
#[derive(Clone)]
pub struct PyMetadata {
    title: Option<String>,
    author: Option<String>,
    subject: Option<String>,
    keywords: Option<String>,
    creator: Option<String>,
    producer: Option<String>,
    creation_date: Option<String>,
    modification_date: Option<String>,
    /// "true", "false" or "unknown"; `None` when `/Trapped` is absent.
    trapped: Option<&'static str>,
}

#[pymethods]
impl PyMetadata {
    fn __repr__(&self) -> String {
        format!("<tinker_pdf.Metadata title={:?}>", self.title)
    }
}

pub fn metadata(document: &Document) -> PyMetadata {
    let metadata = document.metadata();
    PyMetadata {
        title: metadata.title,
        author: metadata.author,
        subject: metadata.subject,
        keywords: metadata.keywords,
        creator: metadata.creator,
        producer: metadata.producer,
        creation_date: metadata.creation_date,
        modification_date: metadata.modification_date,
        trapped: metadata.trapped.map(|trapped| match trapped {
            Trapped::True => "true",
            Trapped::False => "false",
            Trapped::Unknown => "unknown",
        }),
    }
}

/// How a destination positions its page (12.3.2.2, Table 151).
///
/// `kind` is one of "xyz", "fit", "fith", "fitv", "fitr", "fitb", "fitbh" and
/// "fitbv"; the numbers the kind does not use are `None`, and so is one the
/// file wrote as `null` ("retain the current value").
#[pyclass(name = "View", get_all, frozen)]
#[derive(Clone)]
pub struct PyView {
    kind: &'static str,
    left: Option<f64>,
    bottom: Option<f64>,
    right: Option<f64>,
    top: Option<f64>,
    zoom: Option<f64>,
}

#[pymethods]
impl PyView {
    fn __repr__(&self) -> String {
        format!("<tinker_pdf.View {}>", self.kind)
    }
}

fn view(kind: &DestKind) -> PyView {
    let mut view = PyView {
        kind: "fit",
        left: None,
        bottom: None,
        right: None,
        top: None,
        zoom: None,
    };
    match *kind {
        DestKind::Xyz { left, top, zoom } => {
            view.kind = "xyz";
            view.left = left;
            view.top = top;
            view.zoom = zoom;
        }
        DestKind::Fit => {}
        DestKind::FitH { top } => {
            view.kind = "fith";
            view.top = top;
        }
        DestKind::FitV { left } => {
            view.kind = "fitv";
            view.left = left;
        }
        DestKind::FitR {
            left,
            bottom,
            right,
            top,
        } => {
            view.kind = "fitr";
            view.left = Some(left);
            view.bottom = Some(bottom);
            view.right = Some(right);
            view.top = Some(top);
        }
        DestKind::FitB => view.kind = "fitb",
        DestKind::FitBH { top } => {
            view.kind = "fitbh";
            view.top = top;
        }
        DestKind::FitBV { left } => {
            view.kind = "fitbv";
            view.left = left;
        }
    }
    view
}

/// Where an outline entry or a link goes (12.3.2), one of three things that
/// are never collapsed into each other (ruling 6).
///
/// `kind` is "explicit" (with `page_index`, `page_ref` and `view`), "named"
/// (with `name`) or "uri" (with `uri`).
#[pyclass(name = "Destination", frozen)]
#[derive(Clone)]
pub struct PyDestination {
    inner: Destination,
}

#[pymethods]
impl PyDestination {
    #[getter]
    fn kind(&self) -> &'static str {
        match self.inner {
            Destination::Explicit { .. } => "explicit",
            Destination::Named(_) => "named",
            Destination::Uri(_) => "uri",
        }
    }

    /// The zero-based page, when the page reference resolved.
    #[getter]
    fn page_index(&self) -> Option<u32> {
        match self.inner {
            Destination::Explicit { page_index, .. } => page_index,
            _ => None,
        }
    }

    /// `(object number, generation)` of the page the file named, kept whether
    /// or not it resolved.
    #[getter]
    fn page_ref(&self) -> Option<(u32, u16)> {
        match self.inner {
            Destination::Explicit { page_ref, .. } => page_ref.map(|r| (r.num, r.gen)),
            _ => None,
        }
    }

    #[getter]
    fn view(&self) -> Option<PyView> {
        match &self.inner {
            Destination::Explicit { kind, .. } => Some(view(kind)),
            _ => None,
        }
    }

    #[getter]
    fn name<'py>(&self, py: Python<'py>) -> Option<Bound<'py, PyBytes>> {
        match &self.inner {
            Destination::Named(name) => Some(PyBytes::new(py, name)),
            _ => None,
        }
    }

    #[getter]
    fn uri<'py>(&self, py: Python<'py>) -> Option<Bound<'py, PyBytes>> {
        match &self.inner {
            Destination::Uri(uri) => Some(PyBytes::new(py, uri)),
            _ => None,
        }
    }

    fn __repr__(&self) -> String {
        format!("<tinker_pdf.Destination {}>", self.kind())
    }
}

fn destination(value: Option<&Destination>) -> Option<PyDestination> {
    value.map(|inner| PyDestination {
        inner: inner.clone(),
    })
}

/// What a link does (12.6.4).
///
/// `kind` is "goto", "gotor", "uri", "named", "launch" or "other". A `/Launch`
/// is reported, never executed.
#[pyclass(name = "Action", frozen)]
#[derive(Clone)]
pub struct PyAction {
    inner: Action,
}

#[pymethods]
impl PyAction {
    #[getter]
    fn kind(&self) -> &'static str {
        match self.inner {
            Action::GoTo(_) => "goto",
            Action::GoToR { .. } => "gotor",
            Action::Uri(_) => "uri",
            Action::Named(_) => "named",
            Action::Launch { .. } => "launch",
            Action::Other { .. } => "other",
        }
    }

    /// The destination of a `/GoTo`, and of a `/GoToR` that carries one.
    #[getter]
    fn destination(&self) -> Option<PyDestination> {
        match &self.inner {
            Action::GoTo(dest) => destination(Some(dest)),
            Action::GoToR { dest, .. } => destination(dest.as_ref()),
            _ => None,
        }
    }

    /// A `/URI` action's URI.
    #[getter]
    fn uri<'py>(&self, py: Python<'py>) -> Option<Bound<'py, PyBytes>> {
        match &self.inner {
            Action::Uri(uri) => Some(PyBytes::new(py, uri)),
            _ => None,
        }
    }

    /// A `/Named` action's viewer command.
    #[getter]
    fn name<'py>(&self, py: Python<'py>) -> Option<Bound<'py, PyBytes>> {
        match &self.inner {
            Action::Named(name) => Some(PyBytes::new(py, name)),
            _ => None,
        }
    }

    /// The file a `/GoToR` or `/Launch` names.
    #[getter]
    fn file<'py>(&self, py: Python<'py>) -> Option<Bound<'py, PyBytes>> {
        match &self.inner {
            Action::GoToR { file, .. } | Action::Launch { file } => {
                file.as_deref().map(|file| PyBytes::new(py, file))
            }
            _ => None,
        }
    }

    /// The `/S` of an action type this engine does not model.
    #[getter]
    fn subtype<'py>(&self, py: Python<'py>) -> Option<Bound<'py, PyBytes>> {
        match &self.inner {
            Action::Other { subtype } => Some(PyBytes::new(py, subtype)),
            _ => None,
        }
    }

    fn __repr__(&self) -> String {
        format!("<tinker_pdf.Action {}>", self.kind())
    }
}

/// One entry of the outline tree (12.3.3), with its children.
#[pyclass(name = "OutlineItem", get_all, frozen)]
#[derive(Clone)]
pub struct PyOutlineItem {
    title: String,
    /// Whether the entry was saved expanded (`/Count` positive).
    open: bool,
    /// `None` for an entry that is only a heading.
    destination: Option<PyDestination>,
    children: Vec<PyOutlineItem>,
}

#[pymethods]
impl PyOutlineItem {
    fn __repr__(&self) -> String {
        format!(
            "<tinker_pdf.OutlineItem {:?} children={}>",
            self.title,
            self.children.len()
        )
    }
}

fn outline_item(item: &tinker_pdf::OutlineItem) -> PyOutlineItem {
    PyOutlineItem {
        title: item.title.clone(),
        open: item.open,
        destination: destination(item.destination.as_ref()),
        children: item.children.iter().map(outline_item).collect(),
    }
}

pub fn outline(document: &Document) -> Vec<PyOutlineItem> {
    document.outline().iter().map(outline_item).collect()
}

/// One link annotation (12.5.6.5).
#[pyclass(name = "Link", get_all, frozen)]
#[derive(Clone)]
pub struct PyLink {
    /// `(x0, y0, x1, y1)`, corners ordered.
    rect: (f64, f64, f64, f64),
    /// `(object number, generation)` when `/Annots` named it indirectly.
    reference: Option<(u32, u16)>,
    /// `None` for a link with neither `/Dest` nor a usable `/A`.
    action: Option<PyAction>,
}

#[pymethods]
impl PyLink {
    fn __repr__(&self) -> String {
        format!(
            "<tinker_pdf.Link {:?} {}>",
            self.rect,
            self.action.as_ref().map_or("none", PyAction::kind)
        )
    }
}

pub fn links(document: &Document, index: u32) -> Option<Vec<PyLink>> {
    let page = document.page(index)?;
    Some(
        page.links()
            .into_iter()
            .map(|link| PyLink {
                rect: (link.rect.x0, link.rect.y0, link.rect.x1, link.rect.y1),
                reference: link.reference.map(|r| (r.num, r.gen)),
                action: link.target.map(|inner| PyAction { inner }),
            })
            .collect(),
    )
}

/// One file attached to the document (7.11.4).
///
/// Listing reads no bytes; `data()` reads them, through the stream reference
/// the facade hands back and `CosDocument::stream_decoded`, which is the
/// route the facade documents.
#[pyclass(name = "Attachment", frozen)]
pub struct PyAttachment {
    document: Document,
    inner: tinker_pdf::Attachment,
}

#[pymethods]
impl PyAttachment {
    /// The name it is filed under in `/Names /EmbeddedFiles`.
    #[getter]
    fn name(&self) -> &str {
        &self.inner.name
    }

    /// `/UF` or `/F`: the filename to offer when saving it out.
    #[getter]
    fn filename(&self) -> &str {
        &self.inner.filename
    }

    /// `/Desc`, or `None`.
    #[getter]
    fn description(&self) -> Option<&str> {
        self.inner.description.as_deref()
    }

    /// `/Params /Size`, or `None`. Advisory: the stream is the truth.
    #[getter]
    fn size(&self) -> Option<i64> {
        self.inner.size
    }

    /// `(object number, generation)` of the embedded file stream.
    #[getter]
    fn stream(&self) -> Option<(u32, u16)> {
        self.inner.stream.map(|r| (r.num, r.gen))
    }

    /// The file's bytes, decoded; `None` when the specification names no
    /// stream. Raises `ValueError` when the stream is there and cannot be
    /// read — "nothing here" and "something here, unreadable" differ.
    fn data<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyBytes>>> {
        let Some(stream) = self.inner.stream else {
            return Ok(None);
        };
        match self.document.cos().stream_decoded(stream) {
            Ok(bytes) => Ok(Some(PyBytes::new(py, &bytes))),
            Err(error) => Err(PyValueError::new_err(format!(
                "attachment {:?}: {} {} R: {error}",
                self.inner.name, stream.num, stream.gen
            ))),
        }
    }

    fn __repr__(&self) -> String {
        format!("<tinker_pdf.Attachment {:?}>", self.inner.name)
    }
}

pub fn attachments(document: &Document) -> Vec<PyAttachment> {
    document
        .attachments()
        .into_iter()
        .map(|inner| PyAttachment {
            document: document.clone(),
            inner,
        })
        .collect()
}

/// One thing the engine tolerated (ruling 10).
#[pyclass(name = "Warning", get_all, frozen)]
#[derive(Clone)]
pub struct PyWarning {
    /// The byte offset that triggered it.
    offset: u64,
    /// `(object number, generation)` being read when it happened, if known.
    object: Option<(u32, u16)>,
    /// The stable identifier, such as "header-not-at-start".
    kind: &'static str,
    /// The facade's own sentence for it.
    message: String,
}

#[pymethods]
impl PyWarning {
    fn __repr__(&self) -> String {
        format!("<tinker_pdf.Warning {} at {}>", self.kind, self.offset)
    }
}

pub fn warnings(document: &Document) -> Vec<PyWarning> {
    document
        .warnings()
        .into_iter()
        .map(|warning| PyWarning {
            offset: warning.offset,
            object: warning.object.map(|r| (r.num, r.gen)),
            kind: warning.kind.as_str(),
            message: warning.kind.to_string(),
        })
        .collect()
}

pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<PyMetadata>()?;
    module.add_class::<PyView>()?;
    module.add_class::<PyDestination>()?;
    module.add_class::<PyAction>()?;
    module.add_class::<PyOutlineItem>()?;
    module.add_class::<PyLink>()?;
    module.add_class::<PyAttachment>()?;
    module.add_class::<PyWarning>()?;
    module.add_class::<PyCommandKind>()?;
    module.add_class::<PyCommandField>()?;
    module.add_class::<PyCommandRecord>()?;
    module.add_class::<PyCommandBudgets>()?;
    module.add_class::<PyCommandStream>()?;
    module.add("COMMAND_STREAM_MAJOR", tinker_pdf::COMMAND_STREAM_MAJOR)?;
    module.add("COMMAND_STREAM_MINOR", tinker_pdf::COMMAND_STREAM_MINOR)?;
    Ok(())
}

// ---- the command stream (`docs/design/command-stream.md`, RC-18.6) ------------

/// A kind of command-stream record (`docs/design/command-stream.md`, "The
/// records"): its number on the wire, its name, and what each of its fields
/// holds, in their places.
#[pyclass(name = "CommandKind", frozen, eq, hash)]
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct PyCommandKind {
    inner: CommandKind,
}

#[pymethods]
impl PyCommandKind {
    /// Every kind this build knows, in the order of their numbers.
    #[staticmethod]
    fn all() -> Vec<PyCommandKind> {
        CommandKind::ALL
            .iter()
            .map(|&inner| PyCommandKind { inner })
            .collect()
    }

    /// The kind a wire number names, or `None` for a number this build does
    /// not know.
    #[staticmethod]
    fn from_number(number: u32) -> Option<PyCommandKind> {
        CommandKind::from_number(number).map(|inner| PyCommandKind { inner })
    }

    /// The kind's number on the wire.
    #[getter]
    fn number(&self) -> u32 {
        self.inner.number()
    }

    /// The kind's name, as the design names it: `"GlyphRun"`.
    #[getter]
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    /// What each field holds, in their order.
    #[getter]
    fn fields(&self) -> Vec<&'static str> {
        self.inner.fields().to_vec()
    }

    /// Whether the kind is the codec's — a command, or a definition a state
    /// is made of — rather than the facade's page and tables.
    #[getter]
    fn is_codec(&self) -> bool {
        self.inner.is_codec()
    }

    /// Whether the kind is one of a page's recorded calls.
    #[getter]
    fn is_command(&self) -> bool {
        self.inner.is_command()
    }

    /// The device method a replay calls for a record of this kind: its name
    /// in snake case, `"glyph_run"`.
    #[getter]
    fn method(&self) -> String {
        snake_case(self.inner.name())
    }

    fn __repr__(&self) -> String {
        format!(
            "<tinker_pdf.CommandKind {} ({})>",
            self.inner.name(),
            self.inner.number()
        )
    }
}

/// `name`, which is in upper camel case, in snake case.
fn snake_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    for (at, character) in name.chars().enumerate() {
        if character.is_ascii_uppercase() {
            if at > 0 {
                out.push('_');
            }
            out.push(character.to_ascii_lowercase());
        } else {
            out.push(character);
        }
    }
    out
}

/// One field of a record (`docs/design/command-stream.md`, "The form"): its
/// tag, and its value as Python holds it.
///
/// The tag is kept beside the value, because two fields Python would hold
/// alike are written apart: an unsigned and a signed integer, an absent value
/// and an empty one. Each tag has its constructor, so a record built here
/// writes the bytes the one read was written with. A real is a Python
/// `float`, its bits as written, a NaN's payload and a negative zero
/// included.
#[pyclass(name = "CommandField", frozen)]
#[derive(Clone)]
pub struct PyCommandField {
    inner: CommandField,
}

#[pymethods]
impl PyCommandField {
    /// An absent value, tag 0.
    #[staticmethod]
    fn nothing() -> PyCommandField {
        PyCommandField {
            inner: CommandField::Nothing,
        }
    }

    /// An unsigned integer, tag 1.
    #[staticmethod]
    fn unsigned(value: u64) -> PyCommandField {
        PyCommandField {
            inner: CommandField::Unsigned(value),
        }
    }

    /// A signed integer, tag 2.
    #[staticmethod]
    fn signed(value: i64) -> PyCommandField {
        PyCommandField {
            inner: CommandField::Signed(value),
        }
    }

    /// A real, tag 3.
    #[staticmethod]
    fn real(value: f64) -> PyCommandField {
        PyCommandField {
            inner: CommandField::Real(value),
        }
    }

    /// Bytes, tag 4.
    #[staticmethod]
    fn bytes(value: &[u8]) -> PyCommandField {
        PyCommandField {
            inner: CommandField::bytes(value),
        }
    }

    /// Reals, tag 5.
    #[staticmethod]
    fn reals(value: Vec<f64>) -> PyCommandField {
        PyCommandField {
            inner: CommandField::Reals(value),
        }
    }

    /// Unsigned integers, tag 6.
    #[staticmethod]
    fn naturals(value: Vec<u64>) -> PyCommandField {
        PyCommandField {
            inner: CommandField::Naturals(value),
        }
    }

    /// Byte strings, tag 7: a list of `bytes`.
    #[staticmethod]
    fn strings(value: Vec<Bound<'_, PyBytes>>) -> PyCommandField {
        PyCommandField {
            inner: CommandField::Strings(value.iter().map(|s| s.as_bytes().to_vec()).collect()),
        }
    }

    /// The tag's name, its constructor's: `"nothing"`, `"unsigned"`,
    /// `"signed"`, `"real"`, `"bytes"`, `"reals"`, `"naturals"` or
    /// `"strings"`.
    #[getter]
    fn tag_name(&self) -> &'static str {
        match self.inner {
            CommandField::Nothing => "nothing",
            CommandField::Unsigned(_) => "unsigned",
            CommandField::Signed(_) => "signed",
            CommandField::Real(_) => "real",
            CommandField::Bytes(_) => "bytes",
            CommandField::Reals(_) => "reals",
            CommandField::Naturals(_) => "naturals",
            CommandField::Strings(_) => "strings",
        }
    }

    /// The tag's number on the wire, 0 to 7.
    #[getter]
    fn tag(&self) -> u8 {
        self.inner.tag()
    }

    /// The value: `None`, an `int`, a `float`, `bytes`, a list of `float`s,
    /// a list of `int`s, or a list of `bytes`.
    #[getter]
    fn value<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        Ok(match &self.inner {
            CommandField::Nothing => py.None().into_bound(py),
            CommandField::Unsigned(value) => value.into_pyobject(py)?.into_any(),
            CommandField::Signed(value) => value.into_pyobject(py)?.into_any(),
            CommandField::Real(value) => PyFloat::new(py, *value).into_any(),
            CommandField::Bytes(bytes) => PyBytes::new(py, bytes).into_any(),
            CommandField::Reals(reals) => PyList::new(py, reals.iter().copied())?.into_any(),
            CommandField::Naturals(naturals) => {
                PyList::new(py, naturals.iter().copied())?.into_any()
            }
            CommandField::Strings(strings) => {
                PyList::new(py, strings.iter().map(|s| PyBytes::new(py, s)))?.into_any()
            }
        })
    }

    /// Equal where the two encode to the same bytes: a real by its bits.
    fn __eq__(&self, other: &PyCommandField) -> bool {
        self.inner == other.inner
    }

    fn __repr__(&self) -> String {
        format!("<tinker_pdf.CommandField {}>", self.tag_name())
    }
}

/// One record of a command stream: its kind's number and its fields, in the
/// places its kind names. A record of a kind this build does not know — one
/// a newer minor version added — keeps its number and fields, and names no
/// kind.
#[pyclass(name = "CommandRecord", frozen)]
#[derive(Clone)]
pub struct PyCommandRecord {
    inner: CommandRecord,
}

#[pymethods]
impl PyCommandRecord {
    /// A record of the kind numbered `kind` holding `fields`, in their
    /// places, for [`CommandStream.from_records`](PyCommandStream).
    #[new]
    fn new(kind: u32, fields: Vec<PyCommandField>) -> PyCommandRecord {
        PyCommandRecord {
            inner: CommandRecord {
                kind,
                fields: fields.into_iter().map(|field| field.inner).collect(),
            },
        }
    }

    /// The kind's number on the wire.
    #[getter]
    fn kind(&self) -> u32 {
        self.inner.kind
    }

    /// The kind, or `None` for a number this build does not know.
    #[getter]
    fn command_kind(&self) -> Option<PyCommandKind> {
        self.inner
            .command_kind()
            .map(|inner| PyCommandKind { inner })
    }

    /// The kind's name, or `None` for a number this build does not know.
    #[getter]
    fn name(&self) -> Option<&'static str> {
        self.inner.command_kind().map(CommandKind::name)
    }

    /// What each field holds, by its kind; empty for a kind this build does
    /// not know.
    #[getter]
    fn field_names(&self) -> Vec<&'static str> {
        self.inner
            .command_kind()
            .map_or_else(Vec::new, |kind| kind.fields().to_vec())
    }

    /// The fields, in their places.
    #[getter]
    fn fields(&self) -> Vec<PyCommandField> {
        self.inner
            .fields
            .iter()
            .map(|inner| PyCommandField {
                inner: inner.clone(),
            })
            .collect()
    }

    /// The field in the place the kind names `name`, or `None` where the kind
    /// names no such place or the record holds no field there.
    fn field(&self, name: &str) -> Option<PyCommandField> {
        let kind = self.inner.command_kind()?;
        let place = kind.fields().iter().position(|field| *field == name)?;
        self.inner.fields.get(place).map(|inner| PyCommandField {
            inner: inner.clone(),
        })
    }

    /// Equal where the two write the same bytes.
    fn __eq__(&self, other: &PyCommandRecord) -> bool {
        self.inner == other.inner
    }

    /// How many fields the record holds.
    fn __len__(&self) -> usize {
        self.inner.fields.len()
    }

    fn __repr__(&self) -> String {
        let name = self
            .inner
            .command_kind()
            .map_or("unknown", CommandKind::name);
        format!(
            "<tinker_pdf.CommandRecord {name} ({}) of {} fields>",
            self.inner.kind,
            self.inner.fields.len()
        )
    }
}

/// The most a command stream may make a reader hold
/// (`docs/design/command-stream.md`, "The reader's budgets"). A budget not
/// named is the design's default.
#[pyclass(name = "CommandBudgets", get_all, frozen)]
#[derive(Clone)]
pub struct PyCommandBudgets {
    /// Records in a stream, nested streams' included.
    records: u64,
    /// Bytes the read holds.
    bytes: u64,
    /// Streams nested in streams: a tiling cell's.
    depth: u32,
    /// Functions nested in functions, and a calculator's blocks in blocks.
    functions: u32,
    /// Colour spaces nested in colour spaces, and halftones in halftones.
    spaces: u32,
}

#[pymethods]
impl PyCommandBudgets {
    #[new]
    #[pyo3(signature = (records = None, bytes = None, depth = None, functions = None, spaces = None))]
    fn new(
        records: Option<u64>,
        bytes: Option<u64>,
        depth: Option<u32>,
        functions: Option<u32>,
        spaces: Option<u32>,
    ) -> PyCommandBudgets {
        let defaults = CommandBudgets::DEFAULT;
        PyCommandBudgets {
            records: records.unwrap_or(defaults.records),
            bytes: bytes.unwrap_or(defaults.bytes),
            depth: depth.unwrap_or(defaults.depth),
            functions: functions.unwrap_or(defaults.functions),
            spaces: spaces.unwrap_or(defaults.spaces),
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "<tinker_pdf.CommandBudgets records={} bytes={} depth={} functions={} spaces={}>",
            self.records, self.bytes, self.depth, self.functions, self.spaces
        )
    }
}

impl PyCommandBudgets {
    fn to_facade(&self) -> CommandBudgets {
        CommandBudgets {
            records: self.records,
            bytes: self.bytes,
            depth: self.depth,
            functions: self.functions,
            spaces: self.spaces,
        }
    }
}

/// A page's drawing read from its command stream
/// (`docs/design/command-stream.md`): its records in order, the bytes they
/// write back, and its replay through the engine's own renderer.
///
/// [`CommandStream.replay`](PyCommandStream) hands every record, in the
/// stream's order, to the device's method named for its kind in snake case
/// (`CommandKind.method`: `glyph_run`, `begin_group`, `image_decode`), a
/// record of a kind this build does not know to its `unknown`; a device
/// without the method for a kind is not called for it. A refusal of the
/// bytes raises `ValueError` with the reader's sentence.
#[pyclass(name = "CommandStream", frozen)]
pub struct PyCommandStream {
    inner: CommandStream,
}

/// The reader's refusal, as Python raises it.
fn unread(error: tinker_pdf::CommandStreamError) -> PyErr {
    PyValueError::new_err(error.to_string())
}

#[pymethods]
impl PyCommandStream {
    /// Reads a stream under the design's budgets.
    #[new]
    fn new(data: &[u8]) -> PyResult<PyCommandStream> {
        PyCommandStream::parse(data)
    }

    /// Reads a stream under the design's budgets.
    #[staticmethod]
    fn parse(data: &[u8]) -> PyResult<PyCommandStream> {
        CommandStream::parse(data)
            .map(|inner| PyCommandStream { inner })
            .map_err(unread)
    }

    /// Reads a stream under `budgets`.
    #[staticmethod]
    fn parse_with(data: &[u8], budgets: &PyCommandBudgets) -> PyResult<PyCommandStream> {
        CommandStream::parse_with(data, &budgets.to_facade())
            .map(|inner| PyCommandStream { inner })
            .map_err(unread)
    }

    /// The stream `records` write at this build's version, read back as a
    /// parse reads it: the encoder.
    #[staticmethod]
    fn from_records(records: Vec<PyCommandRecord>) -> PyResult<PyCommandStream> {
        let records: Vec<CommandRecord> = records.into_iter().map(|record| record.inner).collect();
        CommandStream::from_records(&records)
            .map(|inner| PyCommandStream { inner })
            .map_err(unread)
    }

    /// Every record before the stream's end, in order.
    #[getter]
    fn records(&self) -> Vec<PyCommandRecord> {
        self.inner
            .records()
            .iter()
            .map(|inner| PyCommandRecord {
                inner: inner.clone(),
            })
            .collect()
    }

    /// The stream's bytes, written back at its own version.
    fn to_bytes<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, &self.inner.to_bytes())
    }

    /// The stream's major and minor version.
    #[getter]
    fn version(&self) -> (u16, u16) {
        self.inner.version()
    }

    /// The usage whose optional-content states the stream's calls carry —
    /// `"view"`, `"print"` or `"export"` — or `None` for none.
    #[getter]
    fn usage(&self) -> Option<&'static str> {
        self.inner.usage().map(|usage| match usage {
            Usage::View => "view",
            Usage::Print => "print",
            Usage::Export => "export",
        })
    }

    /// Which page of its document the stream is of.
    #[getter]
    fn page_index(&self) -> u32 {
        self.inner.page_index()
    }

    /// The page's size in points after rotation.
    #[getter]
    fn size(&self) -> (f64, f64) {
        self.inner.size()
    }

    /// The size in pixels a render at `dpi` draws.
    #[pyo3(signature = (dpi = 72.0))]
    fn pixel_size(&self, dpi: f64) -> (u32, u32) {
        self.inner
            .pixel_size(&tinker_pdf::RenderOptions::at_dpi(dpi))
    }

    /// Draws the page at a resolution in dots per inch, as a render of it
    /// draws: the same pixels and the same warnings.
    #[pyo3(signature = (dpi = 72.0))]
    fn render(&self, py: Python<'_>, dpi: f64) -> crate::PyBitmap {
        let inner = py.detach(|| self.inner.render(&tinker_pdf::RenderOptions::at_dpi(dpi)));
        crate::PyBitmap { inner }
    }

    /// Hands every record, in the stream's order, to the method of `device`
    /// named for its kind (the class documentation), and a record of a kind
    /// this build does not know to its `unknown`. An exception a method
    /// raises ends the replay and is raised.
    fn replay(&self, device: &Bound<'_, PyAny>) -> PyResult<()> {
        for record in self.inner.records() {
            let method = record
                .command_kind()
                .map_or_else(|| "unknown".to_string(), |kind| snake_case(kind.name()));
            if device.hasattr(method.as_str())? {
                let argument = PyCommandRecord {
                    inner: record.clone(),
                };
                device.call_method1(method.as_str(), (argument,))?;
            }
        }
        Ok(())
    }

    /// How many records the stream holds before its end.
    fn __len__(&self) -> usize {
        self.inner.records().len()
    }

    fn __repr__(&self) -> String {
        let (major, minor) = self.inner.version();
        format!(
            "<tinker_pdf.CommandStream {major}.{minor}, page {}, {} records>",
            self.inner.page_index(),
            self.inner.records().len()
        )
    }
}
