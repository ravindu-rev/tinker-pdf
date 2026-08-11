//! The Python binding.
//!
//! PyO3 directly over the facade rather than through the C ABI, which would
//! only add a second error translation. Rendering and text extraction release
//! the GIL — safe because the engine is `Send + Sync`, and it is what makes a
//! thread pool over pages actually parallel in Python.
//!
//! Scope and packaging: `docs/plans/13-bindings.md`.

use pyo3::exceptions::{PyIndexError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyBytes;

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
    Ok(())
}
