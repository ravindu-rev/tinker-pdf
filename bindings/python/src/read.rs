//! The read surface beyond pages: `/Info`, the outline, links, attachments,
//! XMP and warnings, as Python objects.
//!
//! Each class is the facade's own type with its fields as read-only
//! attributes (ruling 11): `OutlineItem` keeps its children, `Destination`
//! keeps its three arms apart (ruling 6), an absent value is `None` and an
//! empty one is an empty string. A byte string the file stores as bytes —
//! a destination name, a URI — is `bytes`, because 12.3.2.3 compares names
//! byte for byte and decoding one would be a guess.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyBytes;

use tinker_pdf::{Action, DestKind, Destination, Document, Trapped};

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
    Ok(())
}
