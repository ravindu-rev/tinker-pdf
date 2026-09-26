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
}

#[pymethods]
impl PyOutlineEntry {
    #[new]
    #[pyo3(signature = (title, page = None, uri = None, open = false, children = Vec::new()))]
    fn new(
        title: String,
        page: Option<u32>,
        uri: Option<String>,
        open: bool,
        children: Vec<PyOutlineEntry>,
    ) -> PyOutlineEntry {
        PyOutlineEntry {
            title,
            page,
            uri,
            open,
            children,
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
                view: tinker_pdf::DestKind::Fit,
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

    /// Adds a link annotation over a rectangle (12.5.6.5).
    #[pyo3(signature = (x0, y0, x1, y1, page = None, uri = None))]
    #[allow(clippy::too_many_arguments)]
    fn link(
        &mut self,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        page: Option<u32>,
        uri: Option<String>,
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
                view: tinker_pdf::DestKind::Fit,
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

    /// Registers one of the standard 14 fonts under a resource name (9.6.2.2).
    fn add_base_font(&mut self, resource: &[u8], base_font: &[u8]) -> PyResult<()> {
        self.get()?.add_base_font(resource, base_font);
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
    Ok(())
}
