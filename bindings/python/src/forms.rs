//! The forms surface as Python values: field rectangles, radio buttons, and
//! `FormData` -- FDF and XFDF (12.7.8) read, written and applied.
//!
//! Conversions only; the `Editor` and `Document` methods in `lib.rs` call the
//! facade with what these build (ruling 11). A value's shape crosses as its
//! arm's name -- "none", "text", "state" or "many" -- beside the strings it
//! is made of, and a reader's warning as its arm's name beside the key or
//! element it did not read and the field it met it in.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyBytes;

use tinker_pdf::form_data::{self, FieldData, FormData, FormDataWarning};
use tinker_pdf::{FieldValue, RadioButton, Rect};

/// `(x0, y0, x1, y1)` as the facade's rectangle.
pub fn rect((x0, y0, x1, y1): (f64, f64, f64, f64)) -> Rect {
    Rect { x0, y0, x1, y1 }
}

/// One radio button as Python spells it: `(export, page, (x0, y0, x1, y1))`.
pub type Button = (String, u32, (f64, f64, f64, f64));

/// A form-data warning as Python spells it: `(kind, what, field)`.
pub type Warning = (&'static str, Option<String>, Option<String>);

/// Button tuples as the facade's buttons.
pub fn buttons(buttons: Vec<Button>) -> Vec<RadioButton> {
    buttons
        .into_iter()
        .map(|(export, page, area)| RadioButton {
            export,
            page,
            rect: rect(area),
        })
        .collect()
}

/// A value's shape and strings, as the facade's `FieldValue`.
fn value(kind: &str, mut values: Vec<String>) -> PyResult<FieldValue> {
    Ok(match (kind, values.len()) {
        ("none", 0) => FieldValue::None,
        ("text", 1) => FieldValue::Text(values.remove(0)),
        ("state", 1) => FieldValue::State(values.remove(0)),
        ("many", _) => FieldValue::Many(values),
        ("none" | "text" | "state", given) => {
            return Err(PyValueError::new_err(format!(
                "a {kind} value takes {} strings, not {given}",
                if kind == "none" { "no" } else { "one" }
            )))
        }
        (other, _) => {
            return Err(PyValueError::new_err(format!(
                "kind must be none, text, state or many, not {other:?}"
            )))
        }
    })
}

/// A value as its arm's name and its strings.
fn spelled(value: &FieldValue) -> (&'static str, Vec<String>) {
    match value {
        FieldValue::Text(text) => ("text", vec![text.clone()]),
        FieldValue::State(state) => ("state", vec![state.clone()]),
        FieldValue::Many(values) => ("many", values.clone()),
        FieldValue::None => ("none", Vec::new()),
    }
}

/// What an FDF or XFDF file says, or what one will be written from.
#[pyclass(name = "FormData")]
pub struct PyFormData {
    pub inner: FormData,
}

impl PyFormData {
    pub fn new(inner: FormData) -> Self {
        PyFormData { inner }
    }
}

#[pymethods]
impl PyFormData {
    /// Empty form data, for `add_field` to fill.
    #[new]
    fn empty() -> Self {
        PyFormData::new(FormData::default())
    }

    /// Reads an FDF file: every field of it, or ValueError and none.
    #[staticmethod]
    fn read_fdf(data: &[u8]) -> PyResult<Self> {
        form_data::read_fdf(data)
            .map(PyFormData::new)
            .map_err(|error| PyValueError::new_err(format!("read_fdf: {error}")))
    }

    /// Reads an XFDF file: every field of it, or ValueError and none.
    #[staticmethod]
    fn read_xfdf(data: &[u8]) -> PyResult<Self> {
        form_data::read_xfdf(data)
            .map(PyFormData::new)
            .map_err(|error| PyValueError::new_err(format!("read_xfdf: {error}")))
    }

    /// Every field as `(name, kind, values)`: kind "none", "text", "state" or
    /// "many", and the strings the value is made of.
    #[getter]
    fn fields(&self) -> Vec<(String, &'static str, Vec<String>)> {
        self.inner
            .fields
            .iter()
            .map(|field| {
                let (kind, values) = spelled(&field.value);
                (field.name.clone(), kind, values)
            })
            .collect()
    }

    /// Appends one field. "none" takes no strings, "text" and "state" one
    /// each, "many" any number.
    fn add_field(&mut self, name: String, kind: &str, values: Vec<String>) -> PyResult<()> {
        let value = value(kind, values)?;
        self.inner.fields.push(FieldData { name, value });
        Ok(())
    }

    /// The document the data belongs to: FDF's `/F`, XFDF's `<f href>`.
    /// Recorded and written, never opened.
    #[getter]
    fn source(&self) -> Option<String> {
        self.inner.source.clone()
    }

    #[setter]
    fn set_source(&mut self, source: Option<String>) {
        self.inner.source = source;
    }

    /// What the reader did not read, as `(kind, what, field)`: kind
    /// "not-read", "value-unreadable", "tree-cut" or "unnamed"; `what` the
    /// key or element not read (None unless "not-read"); `field` the field it
    /// was met in (None for "unnamed", "" for the file itself).
    #[getter]
    fn warnings(&self) -> PyResult<Vec<Warning>> {
        self.inner
            .warnings
            .iter()
            .map(|warning| {
                Ok(match warning {
                    FormDataWarning::NotRead { what, field } => {
                        ("not-read", Some(what.clone()), Some(field.clone()))
                    }
                    FormDataWarning::ValueUnreadable { field } => {
                        ("value-unreadable", None, Some(field.clone()))
                    }
                    FormDataWarning::TreeCut { field } => ("tree-cut", None, Some(field.clone())),
                    FormDataWarning::Unnamed => ("unnamed", None, None),
                    other => {
                        return Err(PyValueError::new_err(format!(
                            "a warning this build cannot spell: {other:?}"
                        )))
                    }
                })
            })
            .collect()
    }

    /// The data as an FDF file (12.7.8).
    fn to_fdf<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, &self.inner.to_fdf())
    }

    /// The data as an XFDF file; ValueError, naming the field, for a value
    /// XML 1.0 cannot carry.
    fn to_xfdf(&self) -> PyResult<String> {
        self.inner
            .to_xfdf()
            .map_err(|error| PyValueError::new_err(format!("to_xfdf: {error}")))
    }

    fn __len__(&self) -> usize {
        self.inner.fields.len()
    }

    fn __repr__(&self) -> String {
        format!(
            "<tinker_pdf.FormData fields={} warnings={}>",
            self.inner.fields.len(),
            self.inner.warnings.len()
        )
    }
}
