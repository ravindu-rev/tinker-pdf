//! The builder's graphics resources: graphics states, form XObjects and tiling
//! patterns registered on the document and invoked on a page; a font under a
//! named encoding and text in codes the caller chose; a page's bleed box; a
//! declared version; and the image list a later page stops inheriting.
//!
//! One facade call each (ruling 11). Blend modes, colour spaces and tiling
//! types cross by their facade arm's name, lower-case and hyphenated. A
//! graphics state is a `PdfExtGState` built with setters from every override
//! absent, as `PdfWriteOptions` is built from the engine's defaults.

use wasm_bindgen::prelude::*;

use tinker_pdf::{
    BlendMode, DeviceSpace, ExtGState, FormXObject, MaskKind, StateMask, TilingPattern, TilingType,
    TransparencyGroup,
};

use crate::{refused, PdfBuilder, PdfPageBuilder};

fn blend_mode(name: &str) -> Result<BlendMode, JsError> {
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
            return Err(JsError::new(&format!(
                "blend mode must be one of normal, multiply, screen, overlay, darken, \
                 lighten, color-dodge, color-burn, hard-light, soft-light, difference, \
                 exclusion, hue, saturation, color and luminosity, not {other:?}"
            )))
        }
    })
}

fn device_space(name: &str) -> Result<DeviceSpace, JsError> {
    Ok(match name {
        "gray" => DeviceSpace::Gray,
        "rgb" => DeviceSpace::Rgb,
        "cmyk" => DeviceSpace::Cmyk,
        other => {
            return Err(JsError::new(&format!(
                "colour space must be 'gray', 'rgb' or 'cmyk', not {other:?}"
            )))
        }
    })
}

fn tiling_type(name: &str) -> Result<TilingType, JsError> {
    Ok(match name {
        "constant-spacing" => TilingType::ConstantSpacing,
        "no-distortion" => TilingType::NoDistortion,
        "faster-tiling" => TilingType::FasterTiling,
        other => {
            return Err(JsError::new(&format!(
                "tiling type must be 'constant-spacing', 'no-distortion' or \
                 'faster-tiling', not {other:?}"
            )))
        }
    })
}

/// Six numbers, or the identity for `undefined`.
fn matrix(matrix: Option<Vec<f64>>) -> Result<Option<[f64; 6]>, JsError> {
    matrix
        .map(|numbers| {
            <[f64; 6]>::try_from(numbers.as_slice()).map_err(|_| {
                JsError::new(&format!("a matrix is six numbers, not {}", numbers.len()))
            })
        })
        .transpose()
}

/// Which `/SMask`, owning what a group mask borrows.
#[derive(Clone)]
enum Mask {
    Absent,
    None,
    Group {
        kind: MaskKind,
        form: Vec<u8>,
        backdrop: Option<Vec<f64>>,
    },
}

/// Graphics state parameters (Table 58), every override absent until set.
#[wasm_bindgen]
pub struct PdfExtGState {
    fill_alpha: Option<f64>,
    stroke_alpha: Option<f64>,
    blend_mode: Option<BlendMode>,
    mask: Mask,
}

#[wasm_bindgen]
impl PdfExtGState {
    /// Every override absent: `ExtGState::default()`.
    #[wasm_bindgen(constructor)]
    pub fn new() -> PdfExtGState {
        PdfExtGState {
            fill_alpha: None,
            stroke_alpha: None,
            blend_mode: None,
            mask: Mask::Absent,
        }
    }

    /// `/ca`, the non-stroking alpha.
    #[wasm_bindgen(js_name = setFillAlpha)]
    pub fn set_fill_alpha(&mut self, alpha: f64) {
        self.fill_alpha = Some(alpha);
    }

    /// `/CA`, the stroking alpha.
    #[wasm_bindgen(js_name = setStrokeAlpha)]
    pub fn set_stroke_alpha(&mut self, alpha: f64) {
        self.stroke_alpha = Some(alpha);
    }

    /// `/BM`, by name: `"normal"`, `"multiply"`, ... `"luminosity"`.
    #[wasm_bindgen(js_name = setBlendMode)]
    pub fn set_blend_mode(&mut self, name: &str) -> Result<(), JsError> {
        self.blend_mode = Some(blend_mode(name)?);
        Ok(())
    }

    /// `/SMask /None`: turns off the mask in force, which is not the same as
    /// writing no `/SMask`.
    #[wasm_bindgen(js_name = setSoftMaskNone)]
    pub fn set_soft_mask_none(&mut self) {
        self.mask = Mask::None;
    }

    /// A mask over a form registered with a transparency group: `kind` is
    /// `"alpha"` or `"luminosity"`, `backdrop` the `/BC` in the group's own
    /// colour space, or `undefined`.
    #[wasm_bindgen(js_name = setSoftMaskGroup)]
    pub fn set_soft_mask_group(
        &mut self,
        kind: &str,
        form: &[u8],
        backdrop: Option<Vec<f64>>,
    ) -> Result<(), JsError> {
        let kind = match kind {
            "alpha" => MaskKind::Alpha,
            "luminosity" => MaskKind::Luminosity,
            other => {
                return Err(JsError::new(&format!(
                    "a mask kind is 'alpha' or 'luminosity', not {other:?}"
                )))
            }
        };
        self.mask = Mask::Group {
            kind,
            form: form.to_vec(),
            backdrop,
        };
        Ok(())
    }
}

impl Default for PdfExtGState {
    fn default() -> Self {
        PdfExtGState::new()
    }
}

#[wasm_bindgen]
impl PdfBuilder {
    /// Starts a document whose header declares PDF `major.minor` (7.5.2);
    /// `new PdfBuilder()` declares the writer's default.
    #[wasm_bindgen(js_name = withVersion)]
    pub fn with_version(major: u8, minor: u8) -> PdfBuilder {
        PdfBuilder {
            inner: Some(tinker_pdf::DocumentBuilder::with_version(major, minor)),
        }
    }

    /// Registers one of the standard 14 under an `/Encoding` the caller wrote
    /// (9.6.6.1): glyph `names` for the codes from `firstCode`, and their
    /// `widths` in thousandths of an em.
    #[wasm_bindgen(js_name = addNamedFont)]
    pub fn add_named_font(
        &mut self,
        resource: &[u8],
        base_font: &[u8],
        first_code: u8,
        names: Vec<String>,
        widths: Vec<u16>,
    ) -> Result<(), JsError> {
        let borrowed: Vec<&str> = names.iter().map(String::as_str).collect();
        if self
            .get()?
            .add_named_font(resource, base_font, first_code, &borrowed, &widths)
        {
            Ok(())
        } else {
            Err(refused(
                "addNamedFont",
                &format!(
                    "{} names and {} widths from code {first_code}",
                    names.len(),
                    widths.len()
                ),
            ))
        }
    }

    /// Registers a graphics state under a resource name (Table 58).
    #[wasm_bindgen(js_name = addExtGState)]
    pub fn add_ext_gstate(&mut self, resource: &[u8], state: &PdfExtGState) -> Result<(), JsError> {
        let soft_mask = match &state.mask {
            Mask::Absent => None,
            Mask::None => Some(StateMask::None),
            Mask::Group {
                kind,
                form,
                backdrop,
            } => Some(StateMask::Group {
                kind: *kind,
                form,
                backdrop: backdrop.as_deref(),
            }),
        };
        let facade = ExtGState {
            fill_alpha: state.fill_alpha,
            stroke_alpha: state.stroke_alpha,
            blend_mode: state.blend_mode,
            soft_mask,
        };
        if self.get()?.add_ext_gstate(resource, &facade) {
            Ok(())
        } else {
            Err(refused("addExtGState", &format!("{facade:?}")))
        }
    }

    /// Registers a form XObject (8.10). `matrix` is six numbers or
    /// `undefined` for the identity; `groupSpace` (`"gray"`, `"rgb"` or
    /// `"cmyk"`) makes it a transparency group with `isolated` and
    /// `knockout`, and `undefined` makes it none.
    #[wasm_bindgen(js_name = addForm)]
    #[allow(clippy::too_many_arguments)]
    pub fn add_form(
        &mut self,
        resource: &[u8],
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        content: &[u8],
        matrix: Option<Vec<f64>>,
        group_space: Option<String>,
        isolated: bool,
        knockout: bool,
    ) -> Result<(), JsError> {
        let group = group_space
            .map(|space| {
                Ok::<_, JsError>(TransparencyGroup {
                    color_space: device_space(&space)?,
                    isolated,
                    knockout,
                })
            })
            .transpose()?;
        let form = FormXObject {
            bbox: [x0, y0, x1, y1],
            matrix: self::matrix(matrix)?,
            group,
            content,
        };
        if self.get()?.add_form(resource, &form) {
            Ok(())
        } else {
            Err(refused(
                "addForm",
                &format!("bbox [{x0} {y0} {x1} {y1}], matrix {:?}", form.matrix),
            ))
        }
    }

    /// Registers a coloured tiling pattern (8.7.3); `tilingType` is
    /// `"constant-spacing"`, `"no-distortion"` or `"faster-tiling"`.
    #[wasm_bindgen(js_name = addTilingPattern)]
    #[allow(clippy::too_many_arguments)]
    pub fn add_tiling_pattern(
        &mut self,
        resource: &[u8],
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        x_step: f64,
        y_step: f64,
        tiling_type: &str,
        content: &[u8],
        matrix: Option<Vec<f64>>,
    ) -> Result<(), JsError> {
        let pattern = TilingPattern {
            bbox: [x0, y0, x1, y1],
            x_step,
            y_step,
            matrix: self::matrix(matrix)?,
            tiling_type: self::tiling_type(tiling_type)?,
            content,
        };
        if self.get()?.add_tiling_pattern(resource, &pattern) {
            Ok(())
        } else {
            Err(refused(
                "addTilingPattern",
                &format!(
                    "bbox [{x0} {y0} {x1} {y1}], steps {x_step} {y_step}, matrix {:?}",
                    pattern.matrix
                ),
            ))
        }
    }

    /// Stops later pages from inheriting the images registered so far.
    #[wasm_bindgen(js_name = clearImageResources)]
    pub fn clear_image_resources(&mut self) -> Result<(), JsError> {
        self.get()?.clear_image_resources();
        Ok(())
    }
}

/// A page call that named a resource nothing is registered under.
fn unregistered(call: &str, resource: &[u8]) -> JsError {
    refused(
        call,
        &format!(
            "nothing of that kind is registered as {:?}",
            String::from_utf8_lossy(resource)
        ),
    )
}

#[wasm_bindgen]
impl PdfPageBuilder {
    /// Sets this page's `/BleedBox` (14.11.2).
    #[wasm_bindgen(js_name = setBleedBox)]
    pub fn set_bleed_box(&mut self, x0: f64, y0: f64, x1: f64, y1: f64) -> Result<(), JsError> {
        self.get()?.set_bleed_box(x0, y0, x1, y1);
        Ok(())
    }

    /// Writes text in codes the caller chose, with a character and a word
    /// spacing (9.3.2, 9.3.3); `characters` are what the codes stand for,
    /// recorded and not written.
    #[wasm_bindgen(js_name = encodedText)]
    #[allow(clippy::too_many_arguments)]
    pub fn encoded_text(
        &mut self,
        font: &[u8],
        size: f64,
        x: f64,
        y: f64,
        character_spacing: f64,
        word_spacing: f64,
        codes: &[u8],
        characters: &str,
    ) -> Result<(), JsError> {
        self.get()?.encoded_text(
            font,
            size,
            x,
            y,
            (character_spacing, word_spacing),
            codes,
            characters,
        );
        Ok(())
    }

    /// Applies a registered graphics state (`gs`).
    #[wasm_bindgen(js_name = setExtGState)]
    pub fn set_ext_gstate(&mut self, resource: &[u8]) -> Result<(), JsError> {
        if self.get()?.set_ext_gstate(resource) {
            Ok(())
        } else {
            Err(unregistered("setExtGState", resource))
        }
    }

    /// Draws a registered form XObject (`Do`).
    #[wasm_bindgen]
    pub fn form(&mut self, resource: &[u8]) -> Result<(), JsError> {
        if self.get()?.form(resource) {
            Ok(())
        } else {
            Err(unregistered("form", resource))
        }
    }

    /// Sets the non-stroking colour to a registered tiling pattern.
    #[wasm_bindgen(js_name = setFillPattern)]
    pub fn set_fill_pattern(&mut self, resource: &[u8]) -> Result<(), JsError> {
        if self.get()?.set_fill_pattern(resource) {
            Ok(())
        } else {
            Err(unregistered("setFillPattern", resource))
        }
    }

    /// Sets the stroking colour to a registered tiling pattern.
    #[wasm_bindgen(js_name = setStrokePattern)]
    pub fn set_stroke_pattern(&mut self, resource: &[u8]) -> Result<(), JsError> {
        if self.get()?.set_stroke_pattern(resource) {
            Ok(())
        } else {
            Err(unregistered("setStrokePattern", resource))
        }
    }
}
