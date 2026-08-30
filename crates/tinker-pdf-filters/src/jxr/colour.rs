//! ITU-T T.832 9.10: output formatting.
//!
//! Milestone 4 of `docs/design/jpeg-xr.md`.

#![deny(clippy::float_arithmetic)]

use super::coefficients::Planes;
use super::container::JxrPixelFormat;
use super::headers::CodedImageHeaders;
use super::JxrError;

pub(crate) fn format_output(
    _planes: &Planes,
    _h: &CodedImageHeaders,
    _format: JxrPixelFormat,
) -> Result<Vec<u8>, JxrError> {
    Ok(Vec::new())
}
