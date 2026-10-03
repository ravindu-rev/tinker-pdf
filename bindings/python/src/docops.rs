//! The editor's document operations, as Python values: page labels,
//! embedded files, dates, `/Trapped`, the page boundaries, and the sanitise
//! report.
//!
//! Conversions only — the `Editor` methods in `lib.rs` call the facade with
//! what these build (ruling 11). A metadata write answers with what it did to
//! the other statement of the same metadata, as "alone" or
//! "other-half-unchanged", because an `/Info` entry and an XMP packet that
//! disagree are a document that says two things and the caller is owed the
//! warning.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyBytes;

use tinker_pdf::{
    Date, EntryHolder, LabelStyle, MetadataSync, PageBoundary, PageLabelRange, PathStep, Removal,
    SanitiseReport, Trapped,
};

/// A page-label style from its name.
pub fn label_style(name: &str) -> PyResult<LabelStyle> {
    Ok(match name {
        "decimal" => LabelStyle::Decimal,
        "roman-upper" => LabelStyle::RomanUpper,
        "roman-lower" => LabelStyle::RomanLower,
        "letters-upper" => LabelStyle::LettersUpper,
        "letters-lower" => LabelStyle::LettersLower,
        "none" => LabelStyle::None,
        other => {
            return Err(PyValueError::new_err(format!(
                "style must be decimal, roman-upper, roman-lower, letters-upper, \
                 letters-lower or none, not {other:?}"
            )))
        }
    })
}

/// `(first_page, style, prefix, start)` tuples as the facade's ranges.
pub fn label_ranges(
    ranges: Vec<(u32, String, Option<String>, u32)>,
) -> PyResult<Vec<PageLabelRange>> {
    ranges
        .into_iter()
        .map(|(first_page, style, prefix, start)| {
            Ok(PageLabelRange {
                first_page,
                style: label_style(&style)?,
                prefix,
                start,
            })
        })
        .collect()
}

/// A date tuple `(year, month, day, hour, minute, second, utc_offset_minutes)`,
/// the offset `None` for an unspecified zone.
pub type DateTuple = (i32, u8, u8, u8, u8, u8, Option<i32>);

pub fn date((year, month, day, hour, minute, second, utc_offset_minutes): DateTuple) -> Date {
    Date {
        year,
        month,
        day,
        hour,
        minute,
        second,
        utc_offset_minutes,
    }
}

/// "alone" or "other-half-unchanged".
pub fn sync(sync: MetadataSync) -> &'static str {
    match sync {
        MetadataSync::Alone => "alone",
        MetadataSync::OtherHalfUnchanged => "other-half-unchanged",
    }
}

/// `/Trapped` from its name.
pub fn trapped(name: &str) -> PyResult<Trapped> {
    Ok(match name {
        "true" => Trapped::True,
        "false" => Trapped::False,
        "unknown" => Trapped::Unknown,
        other => {
            return Err(PyValueError::new_err(format!(
                "trapped must be true, false or unknown, not {other:?}"
            )))
        }
    })
}

/// A page boundary from its name: "media", "crop", "bleed", "trim" or "art".
pub fn boundary(name: &str) -> PyResult<PageBoundary> {
    Ok(match name {
        "media" => PageBoundary::MediaBox,
        "crop" => PageBoundary::CropBox,
        "bleed" => PageBoundary::BleedBox,
        "trim" => PageBoundary::TrimBox,
        "art" => PageBoundary::ArtBox,
        other => {
            return Err(PyValueError::new_err(format!(
                "boundary must be media, crop, bleed, trim or art, not {other:?}"
            )))
        }
    })
}

fn removal(what: &Removal) -> &'static str {
    match what {
        Removal::JavaScript => "javascript",
        Removal::DocumentJavaScript => "document-javascript",
        Removal::CalculationOrder => "calculation-order",
        Removal::XfaForm => "xfa-form",
        Removal::Action(_) => "action",
        Removal::EmbeddedFileTree => "embedded-file-tree",
        Removal::EmbeddedFile => "embedded-file",
        Removal::Info => "info",
        Removal::Metadata => "metadata",
    }
}

fn action<'py>(py: Python<'py>, what: &Removal) -> Option<Bound<'py, PyBytes>> {
    match what {
        Removal::Action(subtype) => Some(PyBytes::new(py, subtype)),
        _ => None,
    }
}

/// Everything a sanitise took out.
///
/// `removed` is a list of `(holder, path, what, action)`: `holder` is
/// `(object number, generation)` or `None` for the trailer, `path` the keys
/// (`bytes`) and array positions (`int`, counted in the array as it was) from
/// the holder down to the removed value, `what` why — "javascript",
/// "document-javascript", "calculation-order", "xfa-form", "action",
/// "embedded-file-tree", "embedded-file", "info" or "metadata" — and `action`
/// the `/S` of an "action". `deleted` is a list of `(object, what, action)`.
/// Together they account for every change the pass made.
#[pyclass(name = "SanitiseReport", frozen)]
pub struct PySanitiseReport {
    inner: SanitiseReport,
}

impl PySanitiseReport {
    pub fn new(inner: SanitiseReport) -> PySanitiseReport {
        PySanitiseReport { inner }
    }
}

type Removed<'py> = (
    Option<(u32, u16)>,
    Vec<Py<PyAny>>,
    &'static str,
    Option<Bound<'py, PyBytes>>,
);

#[pymethods]
impl PySanitiseReport {
    #[getter]
    fn removed<'py>(&self, py: Python<'py>) -> PyResult<Vec<Removed<'py>>> {
        self.inner
            .removed
            .iter()
            .map(|entry| {
                let holder = match entry.holder {
                    EntryHolder::Trailer => None,
                    EntryHolder::Object(r) => Some((r.num, r.gen)),
                };
                let path = entry
                    .path
                    .iter()
                    .map(|step| -> PyResult<Py<PyAny>> {
                        Ok(match step {
                            PathStep::Key(key) => PyBytes::new(py, key).into_any().unbind(),
                            PathStep::Index(at) => at.into_pyobject(py)?.into_any().unbind(),
                        })
                    })
                    .collect::<PyResult<Vec<_>>>()?;
                Ok((holder, path, removal(&entry.what), action(py, &entry.what)))
            })
            .collect()
    }

    #[getter]
    #[allow(clippy::type_complexity)]
    fn deleted<'py>(
        &self,
        py: Python<'py>,
    ) -> Vec<((u32, u16), &'static str, Option<Bound<'py, PyBytes>>)> {
        self.inner
            .deleted
            .iter()
            .map(|entry| {
                (
                    (entry.object.num, entry.object.gen),
                    removal(&entry.what),
                    action(py, &entry.what),
                )
            })
            .collect()
    }

    /// Whether the pass found nothing to take out.
    fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    fn __repr__(&self) -> String {
        format!(
            "<tinker_pdf.SanitiseReport removed={} deleted={}>",
            self.inner.removed.len(),
            self.inner.deleted.len()
        )
    }
}
