//! The builder's graphics resources as Python values: blend modes, device
//! spaces, tiling types and soft masks by their names.
//!
//! Conversions only; `DocumentBuilder` and `PageBuilder` in `lib.rs` call the
//! facade with what these build (ruling 11). Every name is the facade arm's,
//! lower-case and hyphenated.

use pyo3::exceptions::PyValueError;
use pyo3::PyResult;

use tinker_pdf::{BlendMode, DeviceSpace, MaskKind, StateMask, TilingType, TransparencyGroup};

/// A blend mode (11.3.5) from its name.
pub fn blend_mode(name: &str) -> PyResult<BlendMode> {
    Ok(match name {
        "normal" => BlendMode::Normal,
        "multiply" => BlendMode::Multiply,
        "screen" => BlendMode::Screen,
        "overlay" => BlendMode::Overlay,
        "darken" => BlendMode::Darken,
        "lighten" => BlendMode::Lighten,
        "color-dodge" => BlendMode::ColorDodge,
        "color-burn" => BlendMode::ColorBurn,
        "hard-light" => BlendMode::HardLight,
        "soft-light" => BlendMode::SoftLight,
        "difference" => BlendMode::Difference,
        "exclusion" => BlendMode::Exclusion,
        "hue" => BlendMode::Hue,
        "saturation" => BlendMode::Saturation,
        "color" => BlendMode::Color,
        "luminosity" => BlendMode::Luminosity,
        other => {
            return Err(PyValueError::new_err(format!(
                "blend_mode must be one of normal, multiply, screen, overlay, darken, \
                 lighten, color-dodge, color-burn, hard-light, soft-light, difference, \
                 exclusion, hue, saturation, color and luminosity, not {other:?}"
            )))
        }
    })
}

/// A device colour space from its name.
pub fn device_space(name: &str) -> PyResult<DeviceSpace> {
    Ok(match name {
        "gray" => DeviceSpace::Gray,
        "rgb" => DeviceSpace::Rgb,
        "cmyk" => DeviceSpace::Cmyk,
        other => {
            return Err(PyValueError::new_err(format!(
                "color space must be 'gray', 'rgb' or 'cmyk', not {other:?}"
            )))
        }
    })
}

/// A `(color_space, isolated, knockout)` group.
pub fn group((space, isolated, knockout): (String, bool, bool)) -> PyResult<TransparencyGroup> {
    Ok(TransparencyGroup {
        color_space: device_space(&space)?,
        isolated,
        knockout,
    })
}

/// A tiling type (Table 75) from its name.
pub fn tiling_type(name: &str) -> PyResult<TilingType> {
    Ok(match name {
        "constant-spacing" => TilingType::ConstantSpacing,
        "no-distortion" => TilingType::NoDistortion,
        "faster-tiling" => TilingType::FasterTiling,
        other => {
            return Err(PyValueError::new_err(format!(
                "tiling_type must be 'constant-spacing', 'no-distortion' or \
                 'faster-tiling', not {other:?}"
            )))
        }
    })
}

/// An `/SMask` from its name and, for a group, the form and backdrop: `None`
/// writes no entry, `"none"` writes `/SMask /None`, `"alpha"` and
/// `"luminosity"` a group mask over `mask_form`.
pub fn soft_mask<'a>(
    kind: Option<&str>,
    form: Option<&'a [u8]>,
    backdrop: Option<&'a [f64]>,
) -> PyResult<Option<StateMask<'a>>> {
    let group = |kind| match form {
        Some(form) => Ok(Some(StateMask::Group {
            kind,
            form,
            backdrop,
        })),
        None => Err(PyValueError::new_err(
            "a group soft mask names its form: pass mask_form",
        )),
    };
    match kind {
        None => Ok(None),
        Some("none") => Ok(Some(StateMask::None)),
        Some("alpha") => group(MaskKind::Alpha),
        Some("luminosity") => group(MaskKind::Luminosity),
        Some(other) => Err(PyValueError::new_err(format!(
            "soft_mask must be None, 'none', 'alpha' or 'luminosity', not {other:?}"
        ))),
    }
}
