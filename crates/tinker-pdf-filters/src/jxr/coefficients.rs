//! ITU-T T.832 8.7 to 8.12: the entropy-coded coefficient layers.
//!
//! Milestone 2 of `docs/design/jpeg-xr.md` fills this in. Until it does, a
//! codestream whose headers parsed is refused by name rather than returned as
//! a blank raster — 8.7's DC, LP and HP layers carry every sample there is,
//! so a decoder without them has decoded nothing and must not say otherwise.

#![deny(clippy::float_arithmetic)]

use super::bitstream::BitReader;
use super::headers::CodedImageHeaders;
use super::{JxrError, JxrRefusal, JxrWarning};

/// The reconstructed image planes, one sample array per component, each in
/// its own `ExtendedWidth[i]` x `ExtendedHeight[i]` geometry (6.2).
#[allow(dead_code)] // Milestone 2 fills these; milestone 4's colour.rs reads them.
pub(crate) struct Planes {
    pub(crate) samples: Vec<Vec<i32>>,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

pub(crate) fn decode_image(
    _r: &mut BitReader<'_>,
    _h: &CodedImageHeaders,
    _warnings: &mut Vec<JxrWarning>,
) -> Result<Planes, JxrError> {
    Err(JxrError::Unsupported(JxrRefusal::CoefficientLayers))
}
