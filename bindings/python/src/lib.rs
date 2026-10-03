//! The Python binding.
//!
//! PyO3 directly over the facade rather than through the C ABI, which would
//! only add a second error translation. Rendering and text extraction release
//! the GIL — safe because the engine is `Send + Sync`, and it is what makes a
//! thread pool over pages actually parallel in Python.
//!
//! Scope and packaging: `docs/features/bindings.md`.

use pyo3::exceptions::{PyIndexError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyBytes;

mod docops;
mod forms;
mod graphics;
mod read;
mod signatures;

/// Everything a binding may not invent, gathered where it can be seen.
///
/// Ruling 11: a binding projects the facade and adds no defaults of its own.
/// So every optional argument below starts from `WriteOptions::default()` and
/// overrides only what the caller named — a Python caller who passes nothing
/// writes the file a Rust caller who passes nothing writes, byte for byte,
/// which is the property the write-parity suite exists to check.
mod write {
    use pyo3::exceptions::PyValueError;
    use pyo3::PyResult;

    /// The facade's `DestKind` from its name and the five numbers, `None`
    /// being the file's `null` (12.3.2.2 Table 151).
    ///
    /// All eight arms, as the C ABI's `TpdfDestination` carries them. `/FitR`
    /// takes four numbers that are never `null`, so a missing one is refused
    /// rather than written as zero.
    pub fn view(name: &str, numbers: [Option<f64>; 5]) -> PyResult<tinker_pdf::DestKind> {
        use tinker_pdf::DestKind;
        let [left, bottom, right, top, zoom] = numbers;
        Ok(match name {
            "xyz" => DestKind::Xyz { left, top, zoom },
            "fit" => DestKind::Fit,
            "fith" => DestKind::FitH { top },
            "fitv" => DestKind::FitV { left },
            "fitr" => match (left, bottom, right, top) {
                (Some(left), Some(bottom), Some(right), Some(top)) => DestKind::FitR {
                    left,
                    bottom,
                    right,
                    top,
                },
                _ => {
                    return Err(PyValueError::new_err(
                        "view 'fitr' needs left, bottom, right and top",
                    ))
                }
            },
            "fitb" => DestKind::FitB,
            "fitbh" => DestKind::FitBH { top },
            "fitbv" => DestKind::FitBV { left },
            other => {
                return Err(PyValueError::new_err(format!(
                    "view must be one of xyz, fit, fith, fitv, fitr, fitb, fitbh and \
                     fitbv, not {other:?}"
                )))
            }
        })
    }

    /// The facade's write mode from its name, or a refusal naming both.
    pub fn mode(name: &str) -> PyResult<tinker_pdf::WriteMode> {
        match name {
            "rewrite" => Ok(tinker_pdf::WriteMode::Rewrite),
            "incremental" => Ok(tinker_pdf::WriteMode::Incremental),
            other => Err(PyValueError::new_err(format!(
                "mode must be 'rewrite' or 'incremental', not {other:?}"
            ))),
        }
    }
}

/// An open PDF document.
#[pyclass(name = "Document")]
pub struct PyDocument {
    inner: tinker_pdf::Document,
}

/// A rendered page.
#[pyclass(name = "Bitmap")]
pub struct PyBitmap {
    inner: tinker_pdf::Bitmap,
}

#[pymethods]
impl PyDocument {
    /// Opens a document from bytes.
    #[new]
    fn new(data: &[u8]) -> PyResult<PyDocument> {
        tinker_pdf::Document::open(data.to_vec())
            .map(|inner| PyDocument { inner })
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    /// Opens a document from a file.
    #[staticmethod]
    fn open(path: &str) -> PyResult<PyDocument> {
        let bytes = std::fs::read(path)?;
        PyDocument::new(&bytes)
    }

    /// The number of pages.
    #[getter]
    fn page_count(&self) -> u32 {
        self.inner.page_count()
    }

    /// Whether the document is encrypted.
    #[getter]
    fn is_encrypted(&self) -> bool {
        self.inner.is_encrypted()
    }

    /// Tries a password, returning "none", "user" or "owner".
    fn authenticate(&mut self, password: &str) -> PyResult<&'static str> {
        match self.inner.authenticate(password) {
            Ok(tinker_pdf::AuthLevel::Owner) => Ok("owner"),
            Ok(tinker_pdf::AuthLevel::User) => Ok("user"),
            Ok(tinker_pdf::AuthLevel::None) => Ok("none"),
            Err(e) => Err(PyValueError::new_err(format!("{e:?}"))),
        }
    }

    /// Whether the document permits printing. PDF permissions are advisory:
    /// a document saying printing is denied is asking, not enforcing.
    fn may_print(&self) -> bool {
        self.inner.permissions().print()
    }

    /// A page's size in points.
    fn page_size(&self, index: u32) -> PyResult<(f64, f64)> {
        self.inner
            .page(index)
            .map(|p| p.size())
            .ok_or_else(|| PyIndexError::new_err("no such page"))
    }

    /// A page's text.
    ///
    /// Releases the GIL: extraction is pure computation over shared immutable
    /// state, so several pages can be read at once from Python threads.
    fn page_text(&self, py: Python<'_>, index: u32) -> PyResult<String> {
        let page = self
            .inner
            .page(index)
            .ok_or_else(|| PyIndexError::new_err("no such page"))?;
        Ok(py.detach(|| page.text().plain_text()))
    }

    /// Supplies a font for documents that embed none.
    ///
    /// Without one such a document extracts its text perfectly and draws none
    /// of it: the standard-14 metrics are built in, the outlines are not. The
    /// engine bundles no faces and reads no font directories, so a host that
    /// wants text drawn says where to find it.
    ///
    /// `regular` is required; the other three fall back to it.
    #[pyo3(signature = (regular, bold = None, italic = None, bold_italic = None))]
    fn set_fonts(
        &mut self,
        regular: Vec<u8>,
        bold: Option<Vec<u8>>,
        italic: Option<Vec<u8>>,
        bold_italic: Option<Vec<u8>>,
    ) {
        let mut provider = tinker_pdf::SimpleFontProvider::new(regular);
        if let Some(bytes) = bold {
            provider = provider.with_bold(bytes);
        }
        if let Some(bytes) = italic {
            provider = provider.with_italic(bytes);
        }
        if let Some(bytes) = bold_italic {
            provider = provider.with_bold_italic(bytes);
        }
        self.inner = self.inner.clone().with_fonts(std::sync::Arc::new(provider));
    }

    /// Renders a page at a resolution in dots per inch.
    #[pyo3(signature = (index, dpi = 72.0))]
    fn render(&self, py: Python<'_>, index: u32, dpi: f64) -> PyResult<PyBitmap> {
        let page = self
            .inner
            .page(index)
            .ok_or_else(|| PyIndexError::new_err("no such page"))?;
        let inner = py.detach(|| page.render(&tinker_pdf::RenderOptions::at_dpi(dpi)));
        Ok(PyBitmap { inner })
    }

    /// The strict structural validator's findings, as rule names (ruling 13).
    ///
    /// An empty list is a clean document. This is the check that keeps four
    /// byte-identical outputs from being identically wrong: the write-parity
    /// suite compares four surfaces' bytes to each other, and agreement alone
    /// would be satisfied by four copies of a broken file.
    fn validate(&self) -> Vec<String> {
        self.inner
            .validate()
            .into_iter()
            .map(|defect| defect.kind.as_str().to_string())
            .collect()
    }

    /// The `/Info` dictionary (14.3.3), decoded: a `Metadata` whose absent
    /// entries are `None` and whose empty ones are `""`.
    #[getter]
    fn metadata(&self) -> read::PyMetadata {
        read::metadata(&self.inner)
    }

    /// The version, as "PDF 1.7": the later of the header's and the
    /// catalog's, never absent.
    #[getter]
    fn pdf_version(&self) -> String {
        self.inner.pdf_version()
    }

    /// Every page's label (12.4.2), or an empty list when the document
    /// defines none.
    fn page_labels(&self) -> Vec<String> {
        self.inner.page_labels()
    }

    /// The outline tree (12.3.3); empty when the document has none.
    fn outline(&self) -> Vec<read::PyOutlineItem> {
        read::outline(&self.inner)
    }

    /// A page's link annotations, in `/Annots` order (12.5.6.5).
    fn links(&self, index: u32) -> PyResult<Vec<read::PyLink>> {
        read::links(&self.inner, index).ok_or_else(|| PyIndexError::new_err("no such page"))
    }

    /// Every file attached to the document (7.11.4), in name order.
    fn attachments(&self) -> Vec<read::PyAttachment> {
        read::attachments(&self.inner)
    }

    /// The XMP packet (14.3.2), unparsed, or `None`.
    fn xmp_metadata<'py>(&self, py: Python<'py>) -> Option<Bound<'py, PyBytes>> {
        self.inner
            .xmp_metadata()
            .map(|packet| PyBytes::new(py, &packet))
    }

    /// One of a page's boundaries (14.11.2) — "media", "crop", "bleed",
    /// "trim" or "art" — as `(x0, y0, x1, y1)`, resolved the way the reader
    /// resolves an absent one.
    fn page_box(&self, index: u32, boundary: &str) -> PyResult<(f64, f64, f64, f64)> {
        let boundary = docops::boundary(boundary)?;
        self.inner
            .page(index)
            .map(|page| page.boundary(boundary))
            .ok_or_else(|| PyIndexError::new_err("no such page"))
    }

    /// The document's digital signatures (12.8), read and checked against
    /// the file — not verified; that is `verify_signatures`.
    fn signatures(&self) -> Vec<signatures::PySignature> {
        signatures::signatures(&self.inner)
    }

    /// What every signature turns out to prove, in `signatures()`'s order.
    ///
    /// `anchors` is required: an empty `TrustAnchors` is how a caller says it
    /// trusts nothing. `at` is the instant to judge certificate validity at,
    /// in seconds since the Unix epoch; `None` judges nothing, because
    /// "expired" is a claim about a moment the caller has to name.
    #[pyo3(signature = (anchors, at = None))]
    fn verify_signatures(
        &self,
        anchors: &signatures::PyTrustAnchors,
        at: Option<i64>,
    ) -> Vec<signatures::PyVerdict> {
        signatures::verify(&self.inner, anchors, at)
    }

    /// Everything the engine has tolerated so far, in order (ruling 10).
    ///
    /// Reading a page can tolerate more, so asking again later may answer with
    /// more.
    fn warnings(&self) -> Vec<read::PyWarning> {
        read::warnings(&self.inner)
    }

    /// The data the document's fields hold, in the tree's order: every
    /// terminal field with a name and its value (`FormData::from_fields`),
    /// ready to write out as FDF or XFDF.
    fn form_data(&self) -> forms::PyFormData {
        forms::PyFormData::new(tinker_pdf::form_data::FormData::from_fields(
            &self.inner.form_fields(),
        ))
    }

    /// An editor over this document.
    ///
    /// Independent of the `Document` it came from: the editor holds its own
    /// reference to the shared object store, so this document may be dropped
    /// first and the editor still saves correctly.
    fn editor(&self) -> PyEditor {
        PyEditor {
            inner: self.inner.editor(),
        }
    }

    fn __len__(&self) -> usize {
        self.inner.page_count() as usize
    }

    fn __repr__(&self) -> String {
        format!(
            "<tinker_pdf.Document pages={} encrypted={}>",
            self.inner.page_count(),
            self.inner.is_encrypted()
        )
    }
}

/// A widget a fill wrote a value for and could not draw.
///
/// **The fourth outcome.** A fill has three answers, not two: it raises when
/// nothing was written at all, returns an empty list when the value was
/// written and every widget drawn, and returns a non-empty one when the value
/// was written and these widgets were left showing whatever they showed
/// before, because 12.5.2's required `/Rect` is missing from them. Ruling 2
/// degrades rather than failing; ruling 10 makes the degradation name the
/// object it happened to, which is what this carries.
#[pyclass(name = "SkippedWidget")]
#[derive(Clone)]
pub struct PySkippedWidget {
    inner: tinker_pdf::SkippedWidget,
}

#[pymethods]
impl PySkippedWidget {
    /// The widget annotation's object number.
    #[getter]
    fn object_number(&self) -> u32 {
        self.inner.widget.num
    }

    /// Its generation number.
    #[getter]
    fn generation(&self) -> u16 {
        self.inner.widget.gen
    }

    /// What is wrong with it: `"rect-missing"`.
    #[getter]
    fn reason(&self) -> &'static str {
        match self.inner.reason {
            tinker_pdf::WidgetDefect::RectMissing => "rect-missing",
        }
    }

    fn __str__(&self) -> String {
        self.inner.to_string()
    }

    fn __repr__(&self) -> String {
        format!("<tinker_pdf.SkippedWidget {}>", self.inner)
    }
}

/// An editor's state, taken as a value.
///
/// A value, not an open transaction: taking one changes nothing, dropping one
/// commits nothing because nothing was pending, and
/// [`PyEditor::restore`] is idempotent.
#[pyclass(name = "Checkpoint")]
pub struct PyCheckpoint {
    inner: tinker_pdf::EditCheckpoint,
}

#[pymethods]
impl PyCheckpoint {
    fn __repr__(&self) -> String {
        "<tinker_pdf.Checkpoint>".to_string()
    }
}

/// The context manager `Editor.transaction()` returns.
///
/// **Nothing but checkpoint, host-language control flow, restore.** Entering
/// takes a checkpoint; leaving with an exception restores it and lets the
/// exception through; leaving normally does nothing, because nothing was
/// pending. The semantics is the facade's — the same two functions
/// `DocumentEditor::transaction` calls — and Python supplies only the `with`.
///
/// The exception is never swallowed: `__exit__` returns `False`, so a body
/// that raises still raises. Rolling back and hiding why would be the worst of
/// both.
#[pyclass(name = "Transaction")]
pub struct PyTransaction {
    editor: Py<PyEditor>,
    mark: Option<tinker_pdf::EditCheckpoint>,
}

#[pymethods]
impl PyTransaction {
    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    #[pyo3(signature = (exc_type = None, exc_value = None, traceback = None))]
    fn __exit__(
        &mut self,
        py: Python<'_>,
        exc_type: Option<Bound<'_, PyAny>>,
        exc_value: Option<Bound<'_, PyAny>>,
        traceback: Option<Bound<'_, PyAny>>,
    ) -> PyResult<bool> {
        let _ = (exc_value, traceback);
        if exc_type.is_some() {
            if let Some(mark) = self.mark.take() {
                self.editor.bind(py).borrow_mut().inner.restore(&mark);
            }
        }
        // False: never suppress. A rollback that also hid the reason would be
        // the worst of both.
        Ok(false)
    }

    fn __repr__(&self) -> String {
        "<tinker_pdf.Transaction>".to_string()
    }
}

/// Edits layered over an open document.
#[pyclass(name = "Editor")]
pub struct PyEditor {
    inner: tinker_pdf::DocumentEditor,
}

/// The one place a `bool` or `None` from the facade becomes a Python
/// exception.
///
/// The facade answers `bool` and names no reason, so neither does this; what
/// it can still say is which call refused and what it was given, which is the
/// difference between a debuggable failure and a `False` nobody checked.
fn refused(call: &str, detail: &str) -> PyErr {
    PyValueError::new_err(format!("{call} refused: {detail}"))
}

/// A page call that named a resource nothing is registered under.
fn unregistered(call: &str, resource: &[u8]) -> PyErr {
    refused(
        call,
        &format!(
            "nothing of that kind is registered as {:?}",
            String::from_utf8_lossy(resource)
        ),
    )
}

impl PyEditor {
    /// `DocumentEditor::add_field` with the field built from its parts, the
    /// new field's reference as a pair.
    fn add_field(
        &mut self,
        name: String,
        kind: tinker_pdf::NewFieldKind,
        flags: i64,
        font_size: f64,
    ) -> PyResult<(u32, u16)> {
        let mut spec = tinker_pdf::NewField::new(name, kind);
        spec.flags = flags;
        spec.font_size = font_size;
        self.inner
            .add_field(&spec)
            .map(|reference| (reference.num, reference.gen))
            .map_err(|error| refused("add_field", &error.to_string()))
    }
}

#[pymethods]
impl PyEditor {
    /// Whether anything has been changed.
    #[getter]
    fn is_dirty(&self) -> bool {
        self.inner.is_dirty()
    }

    /// How many pages the document has as this editor sees it, which is not
    /// the document's own count once a page has been inserted or deleted here.
    #[getter]
    fn page_count(&self) -> usize {
        self.inner.page_refs().len()
    }

    /// The form's fields, as `(name, value)` pairs in document order.
    fn fields(&self) -> Vec<(String, String)> {
        self.inner
            .fields()
            .into_iter()
            .map(|f| (f.name, f.value.as_text()))
            .collect()
    }

    /// Removes a page.
    fn delete_page(&mut self, index: u32) -> PyResult<()> {
        if self.inner.delete_page(index) {
            Ok(())
        } else {
            Err(refused("delete_page", &format!("index {index}")))
        }
    }

    /// Moves a page to a new position.
    fn move_page(&mut self, from: u32, to: u32) -> PyResult<()> {
        if self.inner.move_page(from, to) {
            Ok(())
        } else {
            Err(refused("move_page", &format!("from {from} to {to}")))
        }
    }

    /// Rotates a page by a quarter-turn multiple, relative to its current
    /// rotation.
    fn rotate_page(&mut self, index: u32, degrees: i64) -> PyResult<()> {
        if self.inner.rotate_page(index, degrees) {
            Ok(())
        } else {
            Err(refused(
                "rotate_page",
                &format!("index {index}, {degrees} degrees"),
            ))
        }
    }

    /// Inserts a blank page at `index`, which may equal the page count to
    /// append.
    fn insert_page(&mut self, index: u32, width: f64, height: f64) -> PyResult<()> {
        if self.inner.insert_page(index, width, height).is_some() {
            Ok(())
        } else {
            Err(refused(
                "insert_page",
                &format!("index {index}, {width} by {height}"),
            ))
        }
    }

    /// Sets a page's `/CropBox` (14.11.2), in the page's own user space.
    fn set_crop_box(&mut self, index: u32, x0: f64, y0: f64, x1: f64, y1: f64) -> PyResult<()> {
        if self.inner.set_crop_box(index, x0, y0, x1, y1) {
            Ok(())
        } else {
            Err(refused(
                "set_crop_box",
                &format!("index {index}, [{x0} {y0} {x1} {y1}]"),
            ))
        }
    }

    /// Appends operators to a page's content stream.
    fn append_content(&mut self, page: u32, operators: &[u8]) -> PyResult<()> {
        if self.inner.append_content(page, operators) {
            Ok(())
        } else {
            Err(refused("append_content", &format!("page {page}")))
        }
    }

    /// Fills a text or choice field, returning the widgets it could not draw.
    ///
    /// Raises when **nothing** was written; returns a list — empty or not —
    /// when the value was written. See [`PySkippedWidget`] for why an empty
    /// list and a non-empty one are both successes.
    fn fill_field(&mut self, name: &str, value: &str) -> PyResult<Vec<PySkippedWidget>> {
        match self.inner.fill_field(name, value) {
            Ok(skipped) => Ok(skipped
                .into_iter()
                .map(|inner| PySkippedWidget { inner })
                .collect()),
            Err(error) => Err(PyValueError::new_err(format!("{name}: {error}"))),
        }
    }

    /// Ticks or clears a checkbox.
    fn set_checkbox(&mut self, name: &str, on: bool) -> PyResult<()> {
        if self.inner.set_checkbox(name, on) {
            Ok(())
        } else {
            Err(refused("set_checkbox", &format!("field {name:?}")))
        }
    }

    /// Selects one option of a radio group (12.7.4.2).
    fn select_radio(&mut self, name: &str, option: &str) -> PyResult<()> {
        if self.inner.select_radio(name, option) {
            Ok(())
        } else {
            Err(refused(
                "select_radio",
                &format!("field {name:?}, option {option:?}"),
            ))
        }
    }

    /// Takes this editor's state as a value, for `restore` to put back.
    fn checkpoint(&self) -> PyCheckpoint {
        PyCheckpoint {
            inner: self.inner.checkpoint(),
        }
    }

    /// Puts this editor back to what a checkpoint recorded.
    ///
    /// Idempotent: restoring twice is restoring once. The checkpoint is
    /// borrowed rather than consumed, so one can undo several attempts.
    fn restore(&mut self, checkpoint: &PyCheckpoint) {
        self.inner.restore(&checkpoint.inner);
    }

    /// A `with` block whose body either lands together or not at all.
    ///
    /// Sugar over `checkpoint` and `restore` and nothing else, so its
    /// semantics is the facade's. An exception in the body restores the editor
    /// and then propagates.
    fn transaction(slf: Py<Self>, py: Python<'_>) -> PyTransaction {
        let mark = slf.bind(py).borrow().inner.checkpoint();
        PyTransaction {
            editor: slf,
            mark: Some(mark),
        }
    }

    /// Saves the edited document.
    ///
    /// Every argument defaults to the facade's own default, so passing nothing
    /// writes what a Rust caller passing nothing writes. `entropy` is 48
    /// caller-supplied bytes and there is no default for it: this binding does
    /// not invent randomness (ruling 11), and encryption without it is a
    /// refusal rather than a guess.
    #[pyo3(signature = (
        mode = None,
        linearize = None,
        version = None,
        object_streams = None,
        compress = None,
        garbage_collect = None,
        user_password = None,
        owner_password = None,
        permissions = None,
        entropy = None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn save<'py>(
        &self,
        py: Python<'py>,
        mode: Option<&str>,
        linearize: Option<bool>,
        version: Option<(u8, u8)>,
        object_streams: Option<bool>,
        compress: Option<bool>,
        garbage_collect: Option<bool>,
        user_password: Option<String>,
        owner_password: Option<String>,
        permissions: Option<i32>,
        entropy: Option<Vec<u8>>,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let mut options = tinker_pdf::WriteOptions::default();
        if let Some(mode) = mode {
            options.mode = write::mode(mode)?;
        }
        if let Some(value) = linearize {
            options.linearize = value;
        }
        if let Some(value) = version {
            options.version = value;
        }
        if let Some(value) = object_streams {
            options.object_streams = value;
        }
        if let Some(value) = compress {
            options.compress = value;
        }
        if let Some(value) = garbage_collect {
            options.garbage_collect = value;
        }

        let wants_encryption = user_password.is_some()
            || owner_password.is_some()
            || permissions.is_some()
            || entropy.is_some();
        if wants_encryption {
            let Some(entropy) = entropy else {
                return Err(PyValueError::new_err(
                    "encryption needs 48 bytes of caller-supplied entropy; this \
                     binding does not invent randomness",
                ));
            };
            let Ok(entropy) = <[u8; 48]>::try_from(entropy.as_slice()) else {
                return Err(PyValueError::new_err(format!(
                    "entropy must be exactly 48 bytes, not {}",
                    entropy.len()
                )));
            };
            options.encryption = Some(tinker_pdf::Encryption {
                user_password: user_password.unwrap_or_default(),
                owner_password: owner_password.unwrap_or_default(),
                permissions: permissions.unwrap_or(-1),
                entropy,
            });
        }

        // Writing is pure computation over owned state, so the GIL goes back
        // for the duration -- the same bargain `render` and `page_text` make.
        let bytes = py.detach(|| self.inner.save(&options));
        Ok(PyBytes::new(py, &bytes))
    }

    /// Sets the page labels (12.4.2), replacing any: a list of
    /// `(first_page, style, prefix, start)` with `style` one of "decimal",
    /// "roman-upper", "roman-lower", "letters-upper", "letters-lower" and
    /// "none", and `prefix` `None` for no `/P`. Raises with the facade's own
    /// reason, writing nothing, when the ranges are refused.
    fn set_page_labels(&mut self, ranges: Vec<(u32, String, Option<String>, u32)>) -> PyResult<()> {
        let ranges = docops::label_ranges(ranges)?;
        self.inner
            .set_page_labels(&ranges)
            .map_err(|e| refused("set_page_labels", &e.to_string()))
    }

    /// Embeds a file (7.11.4) and returns its file specification's
    /// `(object number, generation)`. Dates are `(year, month, day, hour,
    /// minute, second, utc_offset_minutes)`, the offset `None` for an
    /// unspecified zone.
    #[pyo3(signature = (
        name, filename, data, description = None, mime_type = None, created = None,
        modified = None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn attach_file(
        &mut self,
        name: String,
        filename: String,
        data: Vec<u8>,
        description: Option<String>,
        mime_type: Option<String>,
        created: Option<docops::DateTuple>,
        modified: Option<docops::DateTuple>,
    ) -> PyResult<(u32, u16)> {
        let file = tinker_pdf::EmbeddedFile {
            name,
            filename,
            description,
            mime_type,
            created: created.map(docops::date),
            modified: modified.map(docops::date),
            data,
        };
        self.inner
            .attach_file(&file)
            .map(|r| (r.num, r.gen))
            .map_err(|e| refused("attach_file", &e.to_string()))
    }

    /// Replaces the outline (12.3.3) with these entries.
    fn set_outline(&mut self, entries: Vec<PyOutlineEntry>) -> PyResult<()> {
        let entries = entries
            .iter()
            .map(PyOutlineEntry::to_facade)
            .collect::<PyResult<Vec<_>>>()?;
        if self.inner.set_outline(&entries) {
            Ok(())
        } else {
            Err(refused(
                "set_outline",
                "the tree is deeper or wider than this engine's own reader walks",
            ))
        }
    }

    /// Sets `/Info /Title`; answers what it did to the XMP packet, "alone"
    /// or "other-half-unchanged".
    fn set_title(&mut self, value: &str) -> &'static str {
        docops::sync(self.inner.set_title(value))
    }

    /// Sets `/Info /Author`.
    fn set_author(&mut self, value: &str) -> &'static str {
        docops::sync(self.inner.set_author(value))
    }

    /// Sets `/Info /Subject`.
    fn set_subject(&mut self, value: &str) -> &'static str {
        docops::sync(self.inner.set_subject(value))
    }

    /// Sets `/Info /Keywords`.
    fn set_keywords(&mut self, value: &str) -> &'static str {
        docops::sync(self.inner.set_keywords(value))
    }

    /// Sets `/Info /Creator`.
    fn set_creator(&mut self, value: &str) -> &'static str {
        docops::sync(self.inner.set_creator(value))
    }

    /// Sets `/Info /Producer`.
    fn set_producer(&mut self, value: &str) -> &'static str {
        docops::sync(self.inner.set_producer(value))
    }

    /// Sets `/Info /CreationDate`; raises when the date cannot be spelled.
    fn set_creation_date(&mut self, date: docops::DateTuple) -> PyResult<&'static str> {
        self.inner
            .set_creation_date(docops::date(date))
            .map(docops::sync)
            .ok_or_else(|| refused("set_creation_date", &format!("{date:?}")))
    }

    /// Sets `/Info /ModDate`; raises when the date cannot be spelled.
    fn set_modification_date(&mut self, date: docops::DateTuple) -> PyResult<&'static str> {
        self.inner
            .set_modification_date(docops::date(date))
            .map(docops::sync)
            .ok_or_else(|| refused("set_modification_date", &format!("{date:?}")))
    }

    /// Sets `/Info /Trapped`: "true", "false" or "unknown".
    fn set_trapped(&mut self, value: &str) -> PyResult<&'static str> {
        Ok(docops::sync(
            self.inner.set_trapped(docops::trapped(value)?),
        ))
    }

    /// Makes `packet` the XMP metadata (14.3.2), verbatim and uncompressed.
    fn set_xmp_metadata(&mut self, packet: &[u8]) -> PyResult<&'static str> {
        self.inner
            .set_xmp_metadata(packet)
            .map(docops::sync)
            .ok_or_else(|| refused("set_xmp_metadata", "the document has no catalog"))
    }

    /// Sets one of a page's boundaries (14.11.2): "media", "crop", "bleed",
    /// "trim" or "art".
    #[allow(clippy::too_many_arguments)]
    fn set_page_boundary(
        &mut self,
        index: u32,
        boundary: &str,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
    ) -> PyResult<()> {
        let boundary = docops::boundary(boundary)?;
        if self
            .inner
            .set_page_boundary(index, boundary, x0, y0, x1, y1)
        {
            Ok(())
        } else {
            Err(refused(
                "set_page_boundary",
                &format!("page {index}, [{x0} {y0} {x1} {y1}]"),
            ))
        }
    }

    /// Sets a page's `/BleedBox`.
    fn set_bleed_box(&mut self, index: u32, x0: f64, y0: f64, x1: f64, y1: f64) -> PyResult<()> {
        self.set_page_boundary(index, "bleed", x0, y0, x1, y1)
    }

    /// Sets a page's `/TrimBox`.
    fn set_trim_box(&mut self, index: u32, x0: f64, y0: f64, x1: f64, y1: f64) -> PyResult<()> {
        self.set_page_boundary(index, "trim", x0, y0, x1, y1)
    }

    /// Sets a page's `/ArtBox`.
    fn set_art_box(&mut self, index: u32, x0: f64, y0: f64, x1: f64, y1: f64) -> PyResult<()> {
        self.set_page_boundary(index, "art", x0, y0, x1, y1)
    }

    /// Takes out what the flags name and reports every change it made. With
    /// no flag set it takes out nothing, which is `Sanitise::default()`;
    /// all four is `Sanitise::ALL`.
    #[pyo3(signature = (javascript = false, actions = false, embedded_files = false, metadata = false))]
    fn sanitise(
        &mut self,
        javascript: bool,
        actions: bool,
        embedded_files: bool,
        metadata: bool,
    ) -> docops::PySanitiseReport {
        docops::PySanitiseReport::new(self.inner.sanitise(&tinker_pdf::Sanitise {
            javascript,
            actions,
            embedded_files,
            metadata,
        }))
    }

    /// Creates a text field (12.7.4.3) merged with its one widget, and
    /// returns the field's `(object number, generation)`.
    ///
    /// `rect` is `(x0, y0, x1, y1)` on page `page`; `value` the initial value
    /// and `max_len` the `/MaxLen`, `None` for neither; `flags` the caller's
    /// `/Ff` bits and `font_size` the `/DA` size, 0 for auto -- both
    /// `NewField::new`'s own defaults. Raises with the facade's reason,
    /// creating nothing, when the field is refused.
    #[pyo3(signature = (name, page, rect, value = None, max_len = None, flags = 0, font_size = 0.0))]
    #[allow(clippy::too_many_arguments)]
    fn add_text_field(
        &mut self,
        name: String,
        page: u32,
        rect: (f64, f64, f64, f64),
        value: Option<String>,
        max_len: Option<u32>,
        flags: i64,
        font_size: f64,
    ) -> PyResult<(u32, u16)> {
        let kind = tinker_pdf::NewFieldKind::Text {
            page,
            rect: forms::rect(rect),
            value,
            max_len,
        };
        self.add_field(name, kind, flags, font_size)
    }

    /// Creates a check box (12.7.4.2.3) whose on state is `export`, ticked
    /// when `checked`. Otherwise as `add_text_field`.
    #[pyo3(signature = (name, page, rect, export, checked, flags = 0, font_size = 0.0))]
    #[allow(clippy::too_many_arguments)]
    fn add_checkbox(
        &mut self,
        name: String,
        page: u32,
        rect: (f64, f64, f64, f64),
        export: String,
        checked: bool,
        flags: i64,
        font_size: f64,
    ) -> PyResult<(u32, u16)> {
        let kind = tinker_pdf::NewFieldKind::Checkbox {
            page,
            rect: forms::rect(rect),
            export,
            checked,
        };
        self.add_field(name, kind, flags, font_size)
    }

    /// Creates a radio group (12.7.4.2.4): one field and one widget per
    /// button, each `(export, page, (x0, y0, x1, y1))`; `selected` is the
    /// export value that starts selected, `None` for none. Otherwise as
    /// `add_text_field`.
    #[pyo3(signature = (name, buttons, selected = None, flags = 0, font_size = 0.0))]
    fn add_radio_group(
        &mut self,
        name: String,
        buttons: Vec<forms::Button>,
        selected: Option<String>,
        flags: i64,
        font_size: f64,
    ) -> PyResult<(u32, u16)> {
        let kind = tinker_pdf::NewFieldKind::Radio {
            buttons: forms::buttons(buttons),
            selected,
        };
        self.add_field(name, kind, flags, font_size)
    }

    /// Creates a choice field (12.7.4.4): a combo box when `combo`, a list
    /// box otherwise. `options` are each their own export value and display
    /// text; `editable` lets a combo box's text be typed; `value` is the
    /// initial selection. Otherwise as `add_text_field`.
    #[pyo3(signature = (
        name, page, rect, options, combo, editable = false, value = None, flags = 0,
        font_size = 0.0
    ))]
    #[allow(clippy::too_many_arguments)]
    fn add_choice_field(
        &mut self,
        name: String,
        page: u32,
        rect: (f64, f64, f64, f64),
        options: Vec<String>,
        combo: bool,
        editable: bool,
        value: Option<String>,
        flags: i64,
        font_size: f64,
    ) -> PyResult<(u32, u16)> {
        let kind = tinker_pdf::NewFieldKind::Choice {
            page,
            rect: forms::rect(rect),
            options,
            combo,
            editable,
            value,
        };
        self.add_field(name, kind, flags, font_size)
    }

    /// Imports form data: every field with a value, all of them or none
    /// (`form_data::apply`). Raises, naming the field and writing nothing,
    /// when one would not take its value; otherwise returns the widgets that
    /// took a value and could not be drawn, as `fill_field` does.
    fn apply_form_data(&mut self, data: &forms::PyFormData) -> PyResult<Vec<PySkippedWidget>> {
        match tinker_pdf::form_data::apply(&mut self.inner, &data.inner) {
            Ok(skipped) => Ok(skipped
                .into_iter()
                .map(|inner| PySkippedWidget { inner })
                .collect()),
            Err(rejection) => Err(PyValueError::new_err(format!("apply refused: {rejection}"))),
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "<tinker_pdf.Editor pages={} dirty={}>",
            self.inner.page_refs().len(),
            self.inner.is_dirty()
        )
    }
}

#[pymethods]
impl PyBitmap {
    /// Width in pixels.
    #[getter]
    fn width(&self) -> u32 {
        self.inner.width
    }

    /// Height in pixels.
    #[getter]
    fn height(&self) -> u32 {
        self.inner.height
    }

    /// Bytes per row.
    #[getter]
    fn stride(&self) -> usize {
        self.inner.stride
    }

    /// Bytes per pixel.
    #[getter]
    fn components(&self) -> usize {
        self.inner.components()
    }

    /// The pixels.
    ///
    /// A `bytes` object, so it is owned by Python and outlives the bitmap.
    #[getter]
    fn data<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, &self.inner.data)
    }

    fn __repr__(&self) -> String {
        format!(
            "<tinker_pdf.Bitmap {}x{} {}bpp>",
            self.inner.width,
            self.inner.height,
            self.inner.components() * 8
        )
    }
}

/// One outline entry (12.3.3).
///
/// `target` is a page index, or a string beginning `http`-or-anything for a
/// URI, or absent — 12.3.3 makes `/Dest` optional and an entry without one is
/// a real shape rather than a degraded one, because a part title above three
/// chapters often points nowhere itself.
#[pyclass(name = "OutlineEntry")]
#[derive(Clone)]
pub struct PyOutlineEntry {
    /// The visible text.
    #[pyo3(get, set)]
    pub title: String,
    /// A zero-based page index to point at, or `None`.
    #[pyo3(get, set)]
    pub page: Option<u32>,
    /// A URI to point at, or `None`. Set at most one of this and `page`.
    #[pyo3(get, set)]
    pub uri: Option<String>,
    /// Whether the entry is shown expanded. Ignored for an entry with no
    /// children: 12.3.3 spells this as the sign of `/Count`, which an entry
    /// with nothing beneath it does not carry.
    #[pyo3(get, set)]
    pub open: bool,
    /// Nested entries.
    #[pyo3(get, set)]
    pub children: Vec<PyOutlineEntry>,
    /// How the page is positioned, for a page target: one of "xyz", "fit",
    /// "fith", "fitv", "fitr", "fitb", "fitbh" and "fitbv" (12.3.2.2 Table
    /// 151), with the numbers below. `None` for a number is the file's `null`,
    /// "retain the current value".
    #[pyo3(get, set)]
    pub view: String,
    /// The view's left edge.
    #[pyo3(get, set)]
    pub left: Option<f64>,
    /// The view's bottom edge (`/FitR` only).
    #[pyo3(get, set)]
    pub bottom: Option<f64>,
    /// The view's right edge (`/FitR` only).
    #[pyo3(get, set)]
    pub right: Option<f64>,
    /// The view's top edge.
    #[pyo3(get, set)]
    pub top: Option<f64>,
    /// The view's magnification (`/XYZ` only).
    #[pyo3(get, set)]
    pub zoom: Option<f64>,
}

#[pymethods]
impl PyOutlineEntry {
    #[new]
    #[pyo3(signature = (
        title, page = None, uri = None, open = false, children = Vec::new(),
        view = "fit".to_string(), left = None, bottom = None, right = None, top = None,
        zoom = None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        title: String,
        page: Option<u32>,
        uri: Option<String>,
        open: bool,
        children: Vec<PyOutlineEntry>,
        view: String,
        left: Option<f64>,
        bottom: Option<f64>,
        right: Option<f64>,
        top: Option<f64>,
        zoom: Option<f64>,
    ) -> PyOutlineEntry {
        PyOutlineEntry {
            title,
            page,
            uri,
            open,
            children,
            view,
            left,
            bottom,
            right,
            top,
            zoom,
        }
    }

    fn __repr__(&self) -> String {
        format!("<tinker_pdf.OutlineEntry {:?}>", self.title)
    }
}

impl PyOutlineEntry {
    /// The facade's own entry, or a refusal.
    ///
    /// 12.5.6.5 and 12.3.3 make `/Dest` and `/A` alternatives and call an
    /// entry carrying both malformed, so naming a page *and* a URI is refused
    /// here rather than silently resolved in favour of one.
    fn to_facade(&self) -> PyResult<tinker_pdf::OutlineEntry> {
        let target = match (self.page, &self.uri) {
            (Some(_), Some(_)) => {
                return Err(PyValueError::new_err(format!(
                    "outline entry {:?} names both a page and a URI; 12.3.3 \
                     makes them alternatives",
                    self.title
                )))
            }
            (Some(index), None) => Some(tinker_pdf::Target::Page {
                index,
                view: write::view(
                    &self.view,
                    [self.left, self.bottom, self.right, self.top, self.zoom],
                )?,
            }),
            (None, Some(uri)) => Some(tinker_pdf::Target::Uri(uri.clone())),
            (None, None) => None,
        };
        Ok(tinker_pdf::OutlineEntry {
            title: self.title.clone(),
            target,
            open: self.open,
            children: self
                .children
                .iter()
                .map(PyOutlineEntry::to_facade)
                .collect::<PyResult<Vec<_>>>()?,
        })
    }
}

/// A page being drawn, owned until it is pushed.
///
/// Born from a builder or not at all: there is no constructor, because a page
/// whose resource names were never resolved against a builder is a page whose
/// names mean nothing.
#[pyclass(name = "PageBuilder")]
pub struct PyPageBuilder {
    /// `None` once pushed. `push_page` consumes in Rust, and a Python object
    /// that had been consumed would otherwise be a live handle to nothing.
    inner: Option<tinker_pdf::PageBuilder>,
}

impl PyPageBuilder {
    fn get(&mut self) -> PyResult<&mut tinker_pdf::PageBuilder> {
        self.inner.as_mut().ok_or_else(|| {
            PyValueError::new_err(
                "this page was already pushed; its drawing is in the document \
                 now, so drawing on it again would write into nothing",
            )
        })
    }
}

#[pymethods]
impl PyPageBuilder {
    /// Draws text with a registered font.
    fn text(&mut self, font: &[u8], size: f64, x: f64, y: f64, text: &str) -> PyResult<()> {
        self.get()?.text(font, size, x, y, text);
        Ok(())
    }

    /// Fills a rectangle in device grey, from black (0) to white (1).
    fn fill_rect(&mut self, x: f64, y: f64, w: f64, h: f64, grey: f64) -> PyResult<()> {
        self.get()?.fill_rect(x, y, w, h, grey);
        Ok(())
    }

    /// Draws a registered image into the given rectangle.
    fn image(&mut self, resource: &[u8], x: f64, y: f64, w: f64, h: f64) -> PyResult<()> {
        self.get()?.image(resource, x, y, w, h);
        Ok(())
    }

    /// Sets the non-stroking colour.
    fn set_fill_rgb(&mut self, r: f64, g: f64, b: f64) -> PyResult<()> {
        self.get()?.set_fill_rgb(r, g, b);
        Ok(())
    }

    /// Sets the **stroking** colour. `RG`, not `rg`.
    fn set_stroke_rgb(&mut self, r: f64, g: f64, b: f64) -> PyResult<()> {
        self.get()?.set_stroke_rgb(r, g, b);
        Ok(())
    }

    /// Sets this page's `/CropBox` (7.7.3.3).
    fn set_crop_box(&mut self, x0: f64, y0: f64, x1: f64, y1: f64) -> PyResult<()> {
        self.get()?.set_crop_box(x0, y0, x1, y1);
        Ok(())
    }

    /// Appends content-stream operators verbatim.
    fn raw(&mut self, operators: &[u8]) -> PyResult<()> {
        self.get()?.raw(operators);
        Ok(())
    }

    /// Sets this page's `/BleedBox` (14.11.2).
    fn set_bleed_box(&mut self, x0: f64, y0: f64, x1: f64, y1: f64) -> PyResult<()> {
        self.get()?.set_bleed_box(x0, y0, x1, y1);
        Ok(())
    }

    /// Writes text in codes the caller chose, with `spacing` as
    /// `(character, word)` (9.3.2, 9.3.3). `codes` are written and not
    /// interpreted; `characters`, what they stand for, are recorded and not
    /// written, so an embedded program is still subset to what was drawn.
    #[allow(clippy::too_many_arguments)]
    fn encoded_text(
        &mut self,
        font: &[u8],
        size: f64,
        x: f64,
        y: f64,
        spacing: (f64, f64),
        codes: &[u8],
        characters: &str,
    ) -> PyResult<()> {
        self.get()?
            .encoded_text(font, size, x, y, spacing, codes, characters);
        Ok(())
    }

    /// Applies a registered graphics state (`gs`); raises when none is
    /// registered under the name.
    fn set_ext_gstate(&mut self, resource: &[u8]) -> PyResult<()> {
        if self.get()?.set_ext_gstate(resource) {
            Ok(())
        } else {
            Err(unregistered("set_ext_gstate", resource))
        }
    }

    /// Draws a registered form XObject (`Do`); raises when none is
    /// registered under the name.
    fn form(&mut self, resource: &[u8]) -> PyResult<()> {
        if self.get()?.form(resource) {
            Ok(())
        } else {
            Err(unregistered("form", resource))
        }
    }

    /// Sets the non-stroking colour to a registered tiling pattern.
    fn set_fill_pattern(&mut self, resource: &[u8]) -> PyResult<()> {
        if self.get()?.set_fill_pattern(resource) {
            Ok(())
        } else {
            Err(unregistered("set_fill_pattern", resource))
        }
    }

    /// Sets the stroking colour to a registered tiling pattern.
    fn set_stroke_pattern(&mut self, resource: &[u8]) -> PyResult<()> {
        if self.get()?.set_stroke_pattern(resource) {
            Ok(())
        } else {
            Err(unregistered("set_stroke_pattern", resource))
        }
    }

    /// Adds a link annotation over a rectangle (12.5.6.5).
    ///
    /// A page link is positioned as `view` says, with the same keywords
    /// `OutlineEntry` takes; the default is "fit".
    #[pyo3(signature = (
        x0, y0, x1, y1, page = None, uri = None, view = "fit", left = None, bottom = None,
        right = None, top = None, zoom = None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn link(
        &mut self,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        page: Option<u32>,
        uri: Option<String>,
        view: &str,
        left: Option<f64>,
        bottom: Option<f64>,
        right: Option<f64>,
        top: Option<f64>,
        zoom: Option<f64>,
    ) -> PyResult<()> {
        let target = match (page, uri) {
            (Some(_), Some(_)) | (None, None) => {
                return Err(PyValueError::new_err(
                    "a link names exactly one of `page` and `uri`; 12.5.6.5 \
                     makes /Dest and /A alternatives",
                ))
            }
            (Some(index), None) => tinker_pdf::Target::Page {
                index,
                view: write::view(view, [left, bottom, right, top, zoom])?,
            },
            (None, Some(uri)) => tinker_pdf::Target::Uri(uri),
        };
        if self.get()?.link(x0, y0, x1, y1, &target) {
            Ok(())
        } else {
            Err(refused("link", &format!("[{x0} {y0} {x1} {y1}]")))
        }
    }

    fn __repr__(&self) -> String {
        match self.inner {
            Some(_) => "<tinker_pdf.PageBuilder>".to_string(),
            None => "<tinker_pdf.PageBuilder pushed>".to_string(),
        }
    }
}

/// Assembles a document from pages, fonts and images.
#[pyclass(name = "DocumentBuilder")]
pub struct PyBuilder {
    /// `None` once finished. `finish` consumes in Rust; a second finish is a
    /// refusal rather than a second document.
    inner: Option<tinker_pdf::DocumentBuilder>,
}

impl PyBuilder {
    fn get(&mut self) -> PyResult<&mut tinker_pdf::DocumentBuilder> {
        self.inner
            .as_mut()
            .ok_or_else(|| PyValueError::new_err("this builder was already finished"))
    }
}

#[pymethods]
impl PyBuilder {
    /// Starts a document.
    #[new]
    fn new() -> PyBuilder {
        PyBuilder {
            inner: Some(tinker_pdf::DocumentBuilder::new()),
        }
    }

    /// Starts a document whose header declares PDF `major.minor` (7.5.2);
    /// `DocumentBuilder()` declares the writer's default.
    #[staticmethod]
    fn with_version(major: u8, minor: u8) -> PyBuilder {
        PyBuilder {
            inner: Some(tinker_pdf::DocumentBuilder::with_version(major, minor)),
        }
    }

    /// Registers one of the standard 14 fonts under a resource name (9.6.2.2).
    fn add_base_font(&mut self, resource: &[u8], base_font: &[u8]) -> PyResult<()> {
        self.get()?.add_base_font(resource, base_font);
        Ok(())
    }

    /// Registers one of the standard 14 under an `/Encoding` the caller wrote
    /// (9.6.6.1): glyph `names` for the codes from `first_code`, and their
    /// `widths` in thousandths of an em. Raises, registering nothing, when
    /// the lists differ in length, either is empty, or the codes run past 255.
    fn add_named_font(
        &mut self,
        resource: &[u8],
        base_font: &[u8],
        first_code: u8,
        names: Vec<String>,
        widths: Vec<u16>,
    ) -> PyResult<()> {
        let borrowed: Vec<&str> = names.iter().map(String::as_str).collect();
        if self
            .get()?
            .add_named_font(resource, base_font, first_code, &borrowed, &widths)
        {
            Ok(())
        } else {
            Err(refused(
                "add_named_font",
                &format!(
                    "{} names and {} widths from code {first_code}",
                    names.len(),
                    widths.len()
                ),
            ))
        }
    }

    /// Registers a graphics state (Table 58). Every argument left `None`
    /// writes no entry. `soft_mask` is `"none"` for `/SMask /None`, or
    /// `"alpha"` / `"luminosity"` for a mask over `mask_form`, a form
    /// registered with a transparency group, with `backdrop` its `/BC`.
    #[pyo3(signature = (
        resource, fill_alpha = None, stroke_alpha = None, blend_mode = None, soft_mask = None,
        mask_form = None, backdrop = None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn add_ext_gstate(
        &mut self,
        resource: &[u8],
        fill_alpha: Option<f64>,
        stroke_alpha: Option<f64>,
        blend_mode: Option<&str>,
        soft_mask: Option<&str>,
        mask_form: Option<Vec<u8>>,
        backdrop: Option<Vec<f64>>,
    ) -> PyResult<()> {
        let state = tinker_pdf::ExtGState {
            fill_alpha,
            stroke_alpha,
            blend_mode: blend_mode.map(graphics::blend_mode).transpose()?,
            soft_mask: graphics::soft_mask(soft_mask, mask_form.as_deref(), backdrop.as_deref())?,
        };
        if self.get()?.add_ext_gstate(resource, &state) {
            Ok(())
        } else {
            Err(refused("add_ext_gstate", &format!("{state:?}")))
        }
    }

    /// Registers a form XObject (8.10): `bbox` as `(x0, y0, x1, y1)`,
    /// `matrix` six numbers or `None` for the identity, `group` a
    /// `(color_space, isolated, knockout)` transparency group or `None`.
    #[pyo3(signature = (resource, bbox, content, matrix = None, group = None))]
    fn add_form(
        &mut self,
        resource: &[u8],
        bbox: (f64, f64, f64, f64),
        content: &[u8],
        matrix: Option<[f64; 6]>,
        group: Option<(String, bool, bool)>,
    ) -> PyResult<()> {
        let form = tinker_pdf::FormXObject {
            bbox: [bbox.0, bbox.1, bbox.2, bbox.3],
            matrix,
            group: group.map(graphics::group).transpose()?,
            content,
        };
        if self.get()?.add_form(resource, &form) {
            Ok(())
        } else {
            Err(refused(
                "add_form",
                &format!("bbox {bbox:?}, matrix {matrix:?}"),
            ))
        }
    }

    /// Registers a coloured tiling pattern (8.7.3): `tiling_type` is
    /// `"constant-spacing"`, `"no-distortion"` or `"faster-tiling"`.
    #[pyo3(signature = (resource, bbox, x_step, y_step, tiling_type, content, matrix = None))]
    #[allow(clippy::too_many_arguments)]
    fn add_tiling_pattern(
        &mut self,
        resource: &[u8],
        bbox: (f64, f64, f64, f64),
        x_step: f64,
        y_step: f64,
        tiling_type: &str,
        content: &[u8],
        matrix: Option<[f64; 6]>,
    ) -> PyResult<()> {
        let pattern = tinker_pdf::TilingPattern {
            bbox: [bbox.0, bbox.1, bbox.2, bbox.3],
            x_step,
            y_step,
            matrix,
            tiling_type: graphics::tiling_type(tiling_type)?,
            content,
        };
        if self.get()?.add_tiling_pattern(resource, &pattern) {
            Ok(())
        } else {
            Err(refused(
                "add_tiling_pattern",
                &format!("bbox {bbox:?}, steps {x_step} {y_step}, matrix {matrix:?}"),
            ))
        }
    }

    /// Stops later pages from inheriting the images registered so far.
    fn clear_image_resources(&mut self) -> PyResult<()> {
        self.get()?.clear_image_resources();
        Ok(())
    }

    /// Embeds a TrueType or CFF font program under a resource name.
    fn add_embedded_font(
        &mut self,
        resource: &[u8],
        base_font: &[u8],
        program: &[u8],
    ) -> PyResult<()> {
        if self.get()?.add_embedded_font(resource, base_font, program) {
            Ok(())
        } else {
            Err(refused(
                "add_embedded_font",
                "the font program was not usable",
            ))
        }
    }

    /// Whether embedded fonts are subsetted to the glyphs actually drawn.
    fn set_subset_fonts(&mut self, subset: bool) -> PyResult<()> {
        self.get()?.set_subset_fonts(subset);
        Ok(())
    }

    /// Registers an image under a resource name.
    ///
    /// `kind` is `"jpeg"`, `"rgb8"` or `"gray8"`. JPEG bytes are placed **as
    /// they are** and never re-encoded — recompression is generational quality
    /// loss the caller cannot undo — so `width` and `height` are read from the
    /// bytes and ignored here.
    #[pyo3(signature = (resource, data, kind, width = 0, height = 0))]
    fn add_image(
        &mut self,
        resource: &[u8],
        data: &[u8],
        kind: &str,
        width: u32,
        height: u32,
    ) -> PyResult<()> {
        let described = match kind {
            "jpeg" => tinker_pdf::ImageData::Jpeg(data),
            "rgb8" => tinker_pdf::ImageData::Rgb8 {
                width,
                height,
                data,
            },
            "gray8" => tinker_pdf::ImageData::Gray8 {
                width,
                height,
                data,
            },
            other => {
                return Err(PyValueError::new_err(format!(
                    "kind must be 'jpeg', 'rgb8' or 'gray8', not {other:?}"
                )))
            }
        };
        if self.get()?.add_image(resource, &described) {
            Ok(())
        } else {
            Err(refused(
                "add_image",
                &format!("{kind}, {width} by {height}, {} bytes", data.len()),
            ))
        }
    }

    /// Sets an `/Info` field, such as `Title` or `Author`.
    fn set_info(&mut self, key: &[u8], value: &str) -> PyResult<()> {
        self.get()?.set_info(key, value);
        Ok(())
    }

    /// Sets the document outline from a list of top-level entries (12.3.3).
    fn set_outline(&mut self, entries: Vec<PyOutlineEntry>) -> PyResult<()> {
        let converted = entries
            .iter()
            .map(PyOutlineEntry::to_facade)
            .collect::<PyResult<Vec<_>>>()?;
        if self.get()?.set_outline(converted) {
            Ok(())
        } else {
            Err(refused(
                "set_outline",
                "the tree is deeper or wider than this engine's own reader walks",
            ))
        }
    }

    /// Starts a page, owned by the caller until `push_page` takes it.
    ///
    /// **The resource snapshot happens here.** A font or image registered
    /// after this call is invisible to this page — the same timing the Rust
    /// closure form imposes, because `add_page` calls this.
    fn begin_page(&mut self, width: f64, height: f64) -> PyResult<PyPageBuilder> {
        Ok(PyPageBuilder {
            inner: Some(self.get()?.begin_page(width, height)),
        })
    }

    /// Adds a page the caller has finished drawing.
    ///
    /// Consumes it: a second push of the same page is a refusal rather than a
    /// second page. Pages arrive in the order they are pushed.
    fn push_page(&mut self, page: &mut PyPageBuilder) -> PyResult<()> {
        let Some(drawn) = page.inner.take() else {
            return Err(PyValueError::new_err("this page was already pushed"));
        };
        self.get()?.push_page(drawn);
        Ok(())
    }

    /// Finishes the document and returns its bytes.
    ///
    /// Consumes the builder: a second call is a refusal rather than a second
    /// document.
    fn finish<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let Some(document) = self.inner.take() else {
            return Err(PyValueError::new_err("this builder was already finished"));
        };
        let bytes = py.detach(|| document.finish());
        Ok(PyBytes::new(py, &bytes))
    }

    fn __repr__(&self) -> String {
        match self.inner {
            Some(_) => "<tinker_pdf.DocumentBuilder>".to_string(),
            None => "<tinker_pdf.DocumentBuilder finished>".to_string(),
        }
    }
}

/// A from-scratch, pure-Rust PDF engine.
///
/// The function is not named for the module because that would shadow the
/// engine crate of the same name inside this file.
#[pymodule]
#[pyo3(name = "tinker_pdf")]
fn module_init(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add("__version__", tinker_pdf::VERSION)?;
    module.add_class::<PyDocument>()?;
    module.add_class::<PyBitmap>()?;
    module.add_class::<PyEditor>()?;
    module.add_class::<PyCheckpoint>()?;
    module.add_class::<PyTransaction>()?;
    module.add_class::<PySkippedWidget>()?;
    module.add_class::<PyBuilder>()?;
    module.add_class::<PyPageBuilder>()?;
    module.add_class::<PyOutlineEntry>()?;
    read::register(module)?;
    module.add_class::<docops::PySanitiseReport>()?;
    signatures::register(module)?;
    module.add_class::<forms::PyFormData>()?;
    Ok(())
}
