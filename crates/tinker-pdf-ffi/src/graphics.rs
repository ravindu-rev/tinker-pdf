//! The builder's graphics resources: graphics states, form XObjects and tiling
//! patterns registered on the document and invoked on a page; a font under a
//! named encoding and text in codes the caller chose; a page's bleed box; a
//! declared version; and the image list a later page stops inheriting.
//!
//! Each function is one `DocumentBuilder` or `PageBuilder` call (ruling 11).
//! Where the facade answers `bool`, `false` crosses as
//! [`TpdfStatus::EditRefused`] naming the call and its argument, as every
//! builder refusal does. The structs that cross carry numbers, flags and
//! pointers with lengths: [`TpdfExtGState`] spells `Option<f64>` as **NaN for
//! absent**, which is unambiguous because the facade refuses a non-finite
//! alpha, and `Option<BlendMode>` as a presence flag beside the mode, so
//! [`tpdf_ext_gstate_init`] is `ExtGState::default()` and a zeroed struct is
//! not (it is an alpha of 0, which makes everything invisible). An optional
//! `/Matrix` is a pointer to six doubles or null.
//!
//! Shadings and shading patterns are not here: a `Shading` carries a
//! `Function`, which is recursive (a stitching function holds functions) and
//! has a PostScript calculator arm, and is a sub-surface of its own.

use std::ffi::c_char;

use tinker_pdf::{
    BlendMode, DeviceSpace, DocumentBuilder, ExtGState, FormXObject, MaskKind, StateMask,
    TilingPattern, TilingType, TransparencyGroup,
};

use crate::{
    builder_mut, page_mut, refused, required_bytes, required_str, set_error, Consumable,
    TpdfBuilder, TpdfPageBuilder, TpdfStatus,
};

/// 11.3.5's sixteen blend modes, in Tables 136 and 137's order.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpdfBlendMode {
    /// `/Normal`.
    Normal = 0,
    /// `/Multiply`.
    Multiply = 1,
    /// `/Screen`.
    Screen = 2,
    /// `/Overlay`.
    Overlay = 3,
    /// `/Darken`.
    Darken = 4,
    /// `/Lighten`.
    Lighten = 5,
    /// `/ColorDodge`.
    ColorDodge = 6,
    /// `/ColorBurn`.
    ColorBurn = 7,
    /// `/HardLight`.
    HardLight = 8,
    /// `/SoftLight`.
    SoftLight = 9,
    /// `/Difference`.
    Difference = 10,
    /// `/Exclusion`.
    Exclusion = 11,
    /// `/Hue`.
    Hue = 12,
    /// `/Saturation`.
    Saturation = 13,
    /// `/Color`.
    Color = 14,
    /// `/Luminosity`.
    Luminosity = 15,
}

/// Which `/SMask` a graphics state writes (11.6.5.2), if any.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpdfSoftMask {
    /// No `/SMask` entry: the mask in force is inherited.
    Absent = 0,
    /// `/SMask /None`: the mask in force is turned off. Not the same as
    /// absent.
    None = 1,
    /// A mask built from a transparency-group form: `mask_kind`,
    /// `mask_form` and `backdrop` say which.
    Group = 2,
}

/// What a soft mask derives its alpha from (11.6.5.2).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpdfMaskKind {
    /// `/S /Alpha`.
    Alpha = 0,
    /// `/S /Luminosity`.
    Luminosity = 1,
}

/// A device colour space (8.6.4).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpdfDeviceSpace {
    /// `/DeviceGray`.
    Gray = 0,
    /// `/DeviceRGB`.
    Rgb = 1,
    /// `/DeviceCMYK`.
    Cmyk = 2,
}

/// `/TilingType` (Table 75). Counted from zero, as every enum on this
/// boundary is: `ConstantSpacing` writes `/TilingType 1`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpdfTilingType {
    /// 1: constant spacing.
    ConstantSpacing = 0,
    /// 2: no distortion.
    NoDistortion = 1,
    /// 3: constant spacing and faster tiling.
    FasterTiling = 2,
}

/// Graphics state parameters, for [`tpdf_builder_add_ext_gstate`] (Table
/// 58). Start from [`tpdf_ext_gstate_init`], which is every override absent.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TpdfExtGState {
    /// `/ca`, the non-stroking alpha; NaN writes none.
    pub fill_alpha: f64,
    /// `/CA`, the stroking alpha; NaN writes none.
    pub stroke_alpha: f64,
    /// Non-zero writes `/BM blend_mode`.
    pub has_blend_mode: i32,
    /// `/BM`, when `has_blend_mode` says so.
    pub blend_mode: TpdfBlendMode,
    /// Which `/SMask`, if any.
    pub soft_mask: TpdfSoftMask,
    /// `/S` of a [`TpdfSoftMask::Group`] mask.
    pub mask_kind: TpdfMaskKind,
    /// `/G` of a group mask: the resource name of a form registered with a
    /// transparency group. Ignored for the other two arms.
    pub mask_form: *const u8,
    /// Its length.
    pub mask_form_len: usize,
    /// `/BC` of a group mask, in the mask group's own colour space; null
    /// writes none.
    pub backdrop: *const f64,
    /// How many components `backdrop` has.
    pub backdrop_len: usize,
}

/// A form's `/Group` (11.6.6): a transparency group, for
/// [`tpdf_builder_add_form`].
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TpdfTransparencyGroup {
    /// `/CS`.
    pub color_space: TpdfDeviceSpace,
    /// `/I`, non-zero for isolated.
    pub isolated: i32,
    /// `/K`, non-zero for knockout.
    pub knockout: i32,
}

fn blend(mode: TpdfBlendMode) -> BlendMode {
    match mode {
        TpdfBlendMode::Normal => BlendMode::Normal,
        TpdfBlendMode::Multiply => BlendMode::Multiply,
        TpdfBlendMode::Screen => BlendMode::Screen,
        TpdfBlendMode::Overlay => BlendMode::Overlay,
        TpdfBlendMode::Darken => BlendMode::Darken,
        TpdfBlendMode::Lighten => BlendMode::Lighten,
        TpdfBlendMode::ColorDodge => BlendMode::ColorDodge,
        TpdfBlendMode::ColorBurn => BlendMode::ColorBurn,
        TpdfBlendMode::HardLight => BlendMode::HardLight,
        TpdfBlendMode::SoftLight => BlendMode::SoftLight,
        TpdfBlendMode::Difference => BlendMode::Difference,
        TpdfBlendMode::Exclusion => BlendMode::Exclusion,
        TpdfBlendMode::Hue => BlendMode::Hue,
        TpdfBlendMode::Saturation => BlendMode::Saturation,
        TpdfBlendMode::Color => BlendMode::Color,
        TpdfBlendMode::Luminosity => BlendMode::Luminosity,
    }
}

fn device_space(space: TpdfDeviceSpace) -> DeviceSpace {
    match space {
        TpdfDeviceSpace::Gray => DeviceSpace::Gray,
        TpdfDeviceSpace::Rgb => DeviceSpace::Rgb,
        TpdfDeviceSpace::Cmyk => DeviceSpace::Cmyk,
    }
}

/// NaN for absent, the C spelling of `Option<f64>` here.
fn present(value: f64) -> Option<f64> {
    (!value.is_nan()).then_some(value)
}

/// Six doubles, or the identity's `None` for null.
///
/// # Safety
///
/// `matrix` must be null or valid for six doubles.
unsafe fn matrix(matrix: *const f64) -> Option<[f64; 6]> {
    if matrix.is_null() {
        return None;
    }
    let slice = unsafe { std::slice::from_raw_parts(matrix, 6) };
    let mut out = [0.0; 6];
    out.copy_from_slice(slice);
    Some(out)
}

/// The page behind a handle and a registered resource name, or a refusal.
unsafe fn page_and_name<'a>(
    page: *mut TpdfPageBuilder,
    what: &str,
    resource: *const u8,
    resource_len: usize,
) -> Result<(&'a mut tinker_pdf::PageBuilder, &'a [u8]), TpdfStatus> {
    let page = unsafe { page_mut(page, what) }?;
    let resource = unsafe { required_bytes(resource, resource_len, "resource name") }?;
    Ok((page, resource))
}

/// A refusal of a page call that names an unregistered resource.
fn unregistered(call: &str, resource: &[u8]) -> TpdfStatus {
    refused(
        call,
        &format!(
            "nothing of that kind is registered as {:?}",
            String::from_utf8_lossy(resource)
        ),
    )
}

/// Starts a document whose header declares PDF `major.minor` (7.5.2) --
/// `DocumentBuilder::with_version`. [`crate::tpdf_builder_new`] declares the
/// writer's default, 1.7. Each part wider than a byte is
/// [`TpdfStatus::BadArgument`]; the two are 32-bit so a hand-written binding
/// has no narrow integer to pass.
///
/// # Safety
///
/// `out` must be a valid pointer to write a handle to.
#[no_mangle]
pub unsafe extern "C" fn tpdf_builder_new_with_version(
    major: u32,
    minor: u32,
    out: *mut *mut TpdfBuilder,
) -> TpdfStatus {
    if out.is_null() {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    }
    let (Ok(major), Ok(minor)) = (u8::try_from(major), u8::try_from(minor)) else {
        set_error(&format!("version {major}.{minor}: each part is a byte"));
        return TpdfStatus::BadArgument;
    };
    let handle = Box::new(TpdfBuilder {
        inner: Consumable::new(DocumentBuilder::with_version(major, minor)),
    });
    unsafe { *out = Box::into_raw(handle) };
    TpdfStatus::Ok
}

/// Stops later pages from inheriting the images registered so far --
/// `DocumentBuilder::clear_image_resources`. Nothing already written is
/// touched.
///
/// # Safety
///
/// `builder` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn tpdf_builder_clear_image_resources(
    builder: *mut TpdfBuilder,
) -> TpdfStatus {
    match unsafe { builder_mut(builder, "clear_image_resources") } {
        Ok(builder) => {
            builder.clear_image_resources();
            TpdfStatus::Ok
        }
        Err(status) => status,
    }
}

/// Registers one of the standard 14 under an `/Encoding` the caller wrote
/// (9.6.6.1) -- `DocumentBuilder::add_named_font`: `names` are `name_count`
/// glyph names, one per code from `first_code`, and `widths` their
/// `width_count` widths in thousandths of an em.
///
/// Refused, [`TpdfStatus::EditRefused`] with nothing registered, when the two
/// counts differ, either is zero, or the last code would be past 255;
/// `first_code` past 255 is [`TpdfStatus::BadArgument`].
///
/// # Safety
///
/// `builder` must be a live handle, the byte pointers valid for their
/// lengths, `names` valid for `name_count` null-terminated UTF-8 strings and
/// `widths` for `width_count` values.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn tpdf_builder_add_named_font(
    builder: *mut TpdfBuilder,
    resource: *const u8,
    resource_len: usize,
    base_font: *const u8,
    base_font_len: usize,
    first_code: u32,
    names: *const *const c_char,
    name_count: usize,
    widths: *const u16,
    width_count: usize,
) -> TpdfStatus {
    let builder = match unsafe { builder_mut(builder, "add_named_font") } {
        Ok(builder) => builder,
        Err(status) => return status,
    };
    let (Ok(resource), Ok(base_font)) = (
        unsafe { required_bytes(resource, resource_len, "resource name") },
        unsafe { required_bytes(base_font, base_font_len, "base font name") },
    ) else {
        return TpdfStatus::BadArgument;
    };
    let Ok(first) = u8::try_from(first_code) else {
        set_error(&format!("first code {first_code} is past 255"));
        return TpdfStatus::BadArgument;
    };
    if (names.is_null() && name_count != 0) || (widths.is_null() && width_count != 0) {
        set_error("null names or widths array");
        return TpdfStatus::BadArgument;
    }
    let mut owned = Vec::with_capacity(name_count);
    for index in 0..name_count {
        match unsafe { required_str(*names.add(index), "glyph name") } {
            Ok(name) => owned.push(name),
            Err(status) => return status,
        }
    }
    let borrowed: Vec<&str> = owned.iter().map(String::as_str).collect();
    let widths = if width_count == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(widths, width_count) }
    };
    if builder.add_named_font(resource, base_font, first, &borrowed, widths) {
        TpdfStatus::Ok
    } else {
        refused(
            "add_named_font",
            &format!("{name_count} names and {width_count} widths from code {first_code}"),
        )
    }
}

/// Fills `out` with every override absent -- `ExtGState::default()`: NaN for
/// both alphas, no blend mode, no `/SMask`. A zeroed struct is not this: it
/// is two alphas of 0.
///
/// # Safety
///
/// `out` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_ext_gstate_init(out: *mut TpdfExtGState) -> TpdfStatus {
    let Some(slot) = (unsafe { out.as_mut() }) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    *slot = TpdfExtGState {
        fill_alpha: f64::NAN,
        stroke_alpha: f64::NAN,
        has_blend_mode: 0,
        blend_mode: TpdfBlendMode::Normal,
        soft_mask: TpdfSoftMask::Absent,
        mask_kind: TpdfMaskKind::Alpha,
        mask_form: std::ptr::null(),
        mask_form_len: 0,
        backdrop: std::ptr::null(),
        backdrop_len: 0,
    };
    TpdfStatus::Ok
}

/// Registers a graphics state under a resource name (Table 58) --
/// `DocumentBuilder::add_ext_gstate`.
///
/// Refused, [`TpdfStatus::EditRefused`] with nothing registered, for an
/// alpha outside 11.6.4.4's range, a non-finite backdrop component, or a
/// group mask naming a form that is not registered or carries no `/Group`.
///
/// # Safety
///
/// `builder` must be a live handle, `resource` valid for `resource_len`
/// bytes, and `state` a valid pointer whose `mask_form` and `backdrop` are
/// valid for their lengths (or, for `backdrop`, null).
#[no_mangle]
pub unsafe extern "C" fn tpdf_builder_add_ext_gstate(
    builder: *mut TpdfBuilder,
    resource: *const u8,
    resource_len: usize,
    state: *const TpdfExtGState,
) -> TpdfStatus {
    let builder = match unsafe { builder_mut(builder, "add_ext_gstate") } {
        Ok(builder) => builder,
        Err(status) => return status,
    };
    let Ok(resource) = (unsafe { required_bytes(resource, resource_len, "resource name") }) else {
        return TpdfStatus::BadArgument;
    };
    let Some(state) = (unsafe { state.as_ref() }) else {
        set_error("null graphics state");
        return TpdfStatus::BadArgument;
    };
    let soft_mask = match state.soft_mask {
        TpdfSoftMask::Absent => None,
        TpdfSoftMask::None => Some(StateMask::None),
        TpdfSoftMask::Group => {
            let Ok(form) =
                (unsafe { required_bytes(state.mask_form, state.mask_form_len, "mask form") })
            else {
                return TpdfStatus::BadArgument;
            };
            let backdrop = if state.backdrop.is_null() {
                None
            } else {
                Some(unsafe { std::slice::from_raw_parts(state.backdrop, state.backdrop_len) })
            };
            Some(StateMask::Group {
                kind: match state.mask_kind {
                    TpdfMaskKind::Alpha => MaskKind::Alpha,
                    TpdfMaskKind::Luminosity => MaskKind::Luminosity,
                },
                form,
                backdrop,
            })
        }
    };
    let facade = ExtGState {
        fill_alpha: present(state.fill_alpha),
        stroke_alpha: present(state.stroke_alpha),
        blend_mode: (state.has_blend_mode != 0).then(|| blend(state.blend_mode)),
        soft_mask,
    };
    if builder.add_ext_gstate(resource, &facade) {
        TpdfStatus::Ok
    } else {
        refused("add_ext_gstate", &format!("{facade:?}"))
    }
}

/// Registers a form XObject under a resource name (8.10) --
/// `DocumentBuilder::add_form`. Its `/Resources` are the document's at this
/// moment. `matrix` is six doubles or null for the identity; `group` is the
/// transparency group or null for none.
///
/// Refused, [`TpdfStatus::EditRefused`] with nothing registered, for a
/// degenerate `/BBox` or a non-finite `/Matrix`.
///
/// # Safety
///
/// `builder` must be a live handle, `resource` and `content` valid for their
/// lengths, `matrix` null or valid for six doubles, and `group` null or valid.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn tpdf_builder_add_form(
    builder: *mut TpdfBuilder,
    resource: *const u8,
    resource_len: usize,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    matrix: *const f64,
    group: *const TpdfTransparencyGroup,
    content: *const u8,
    content_len: usize,
) -> TpdfStatus {
    let builder = match unsafe { builder_mut(builder, "add_form") } {
        Ok(builder) => builder,
        Err(status) => return status,
    };
    let (Ok(resource), Ok(content)) = (
        unsafe { required_bytes(resource, resource_len, "resource name") },
        unsafe { required_bytes(content, content_len, "content") },
    ) else {
        return TpdfStatus::BadArgument;
    };
    let form = FormXObject {
        bbox: [x0, y0, x1, y1],
        matrix: unsafe { self::matrix(matrix) },
        group: unsafe { group.as_ref() }.map(|group| TransparencyGroup {
            color_space: device_space(group.color_space),
            isolated: group.isolated != 0,
            knockout: group.knockout != 0,
        }),
        content,
    };
    if builder.add_form(resource, &form) {
        TpdfStatus::Ok
    } else {
        refused(
            "add_form",
            &format!("bbox [{x0} {y0} {x1} {y1}], matrix {:?}", form.matrix),
        )
    }
}

/// Registers a coloured tiling pattern under a resource name (8.7.3) --
/// `DocumentBuilder::add_tiling_pattern`. The cell's `/Resources` are the
/// document's at this moment; `matrix` is six doubles or null.
///
/// Refused, [`TpdfStatus::EditRefused`] with nothing registered, for a
/// degenerate `/BBox`, a zero or non-finite step, or a non-finite `/Matrix`.
///
/// # Safety
///
/// `builder` must be a live handle, `resource` and `content` valid for their
/// lengths, and `matrix` null or valid for six doubles.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn tpdf_builder_add_tiling_pattern(
    builder: *mut TpdfBuilder,
    resource: *const u8,
    resource_len: usize,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    x_step: f64,
    y_step: f64,
    matrix: *const f64,
    tiling_type: TpdfTilingType,
    content: *const u8,
    content_len: usize,
) -> TpdfStatus {
    let builder = match unsafe { builder_mut(builder, "add_tiling_pattern") } {
        Ok(builder) => builder,
        Err(status) => return status,
    };
    let (Ok(resource), Ok(content)) = (
        unsafe { required_bytes(resource, resource_len, "resource name") },
        unsafe { required_bytes(content, content_len, "content") },
    ) else {
        return TpdfStatus::BadArgument;
    };
    let pattern = TilingPattern {
        bbox: [x0, y0, x1, y1],
        x_step,
        y_step,
        matrix: unsafe { self::matrix(matrix) },
        tiling_type: match tiling_type {
            TpdfTilingType::ConstantSpacing => TilingType::ConstantSpacing,
            TpdfTilingType::NoDistortion => TilingType::NoDistortion,
            TpdfTilingType::FasterTiling => TilingType::FasterTiling,
        },
        content,
    };
    if builder.add_tiling_pattern(resource, &pattern) {
        TpdfStatus::Ok
    } else {
        refused(
            "add_tiling_pattern",
            &format!(
                "bbox [{x0} {y0} {x1} {y1}], steps {x_step} {y_step}, matrix {:?}",
                pattern.matrix
            ),
        )
    }
}

/// Sets `/BleedBox` (14.11.2) -- `PageBuilder::set_bleed_box`.
///
/// # Safety
///
/// `page` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn tpdf_page_builder_set_bleed_box(
    page: *mut TpdfPageBuilder,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
) -> TpdfStatus {
    match unsafe { page_mut(page, "set_bleed_box") } {
        Ok(page) => {
            page.set_bleed_box(x0, y0, x1, y1);
            TpdfStatus::Ok
        }
        Err(status) => status,
    }
}

/// Writes text the caller has already encoded, with a character and a word
/// spacing (9.3.2, 9.3.3) -- `PageBuilder::encoded_text`. `codes` are
/// written and not interpreted; `characters`, which they stand for, are
/// recorded and not written, so an embedded program is still subset to
/// what the page drew.
///
/// # Safety
///
/// `page` must be a live handle, `font` and `codes` valid for their lengths
/// and `characters` null-terminated UTF-8.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn tpdf_page_builder_encoded_text(
    page: *mut TpdfPageBuilder,
    font: *const u8,
    font_len: usize,
    size: f64,
    x: f64,
    y: f64,
    character_spacing: f64,
    word_spacing: f64,
    codes: *const u8,
    codes_len: usize,
    characters: *const c_char,
) -> TpdfStatus {
    let page = match unsafe { page_mut(page, "encoded_text") } {
        Ok(page) => page,
        Err(status) => return status,
    };
    let (Ok(font), Ok(codes)) = (
        unsafe { required_bytes(font, font_len, "font name") },
        unsafe { required_bytes(codes, codes_len, "codes") },
    ) else {
        return TpdfStatus::BadArgument;
    };
    let characters = match unsafe { required_str(characters, "characters") } {
        Ok(characters) => characters,
        Err(status) => return status,
    };
    page.encoded_text(
        font,
        size,
        x,
        y,
        (character_spacing, word_spacing),
        codes,
        &characters,
    );
    TpdfStatus::Ok
}

/// Applies a registered graphics state -- `gs`, `PageBuilder::set_ext_gstate`.
/// [`TpdfStatus::EditRefused`], writing nothing, when none is registered
/// under the name.
///
/// # Safety
///
/// `page` must be a live handle and `resource` valid for `resource_len`.
#[no_mangle]
pub unsafe extern "C" fn tpdf_page_builder_set_ext_gstate(
    page: *mut TpdfPageBuilder,
    resource: *const u8,
    resource_len: usize,
) -> TpdfStatus {
    let (page, resource) =
        match unsafe { page_and_name(page, "set_ext_gstate", resource, resource_len) } {
            Ok(both) => both,
            Err(status) => return status,
        };
    if page.set_ext_gstate(resource) {
        TpdfStatus::Ok
    } else {
        unregistered("set_ext_gstate", resource)
    }
}

/// Draws a registered form XObject -- `Do`, `PageBuilder::form`.
/// [`TpdfStatus::EditRefused`], writing nothing, when none is registered
/// under the name.
///
/// # Safety
///
/// `page` must be a live handle and `resource` valid for `resource_len`.
#[no_mangle]
pub unsafe extern "C" fn tpdf_page_builder_form(
    page: *mut TpdfPageBuilder,
    resource: *const u8,
    resource_len: usize,
) -> TpdfStatus {
    let (page, resource) = match unsafe { page_and_name(page, "form", resource, resource_len) } {
        Ok(both) => both,
        Err(status) => return status,
    };
    if page.form(resource) {
        TpdfStatus::Ok
    } else {
        unregistered("form", resource)
    }
}

/// Sets the non-stroking colour to a registered tiling pattern -- `cs` and
/// `scn`, `PageBuilder::set_fill_pattern`. [`TpdfStatus::EditRefused`],
/// writing nothing, when none is registered under the name.
///
/// # Safety
///
/// `page` must be a live handle and `resource` valid for `resource_len`.
#[no_mangle]
pub unsafe extern "C" fn tpdf_page_builder_set_fill_pattern(
    page: *mut TpdfPageBuilder,
    resource: *const u8,
    resource_len: usize,
) -> TpdfStatus {
    let (page, resource) =
        match unsafe { page_and_name(page, "set_fill_pattern", resource, resource_len) } {
            Ok(both) => both,
            Err(status) => return status,
        };
    if page.set_fill_pattern(resource) {
        TpdfStatus::Ok
    } else {
        unregistered("set_fill_pattern", resource)
    }
}

/// Sets the stroking colour to a registered tiling pattern -- `CS` and
/// `SCN`, `PageBuilder::set_stroke_pattern`. [`TpdfStatus::EditRefused`],
/// writing nothing, when none is registered under the name.
///
/// # Safety
///
/// `page` must be a live handle and `resource` valid for `resource_len`.
#[no_mangle]
pub unsafe extern "C" fn tpdf_page_builder_set_stroke_pattern(
    page: *mut TpdfPageBuilder,
    resource: *const u8,
    resource_len: usize,
) -> TpdfStatus {
    let (page, resource) =
        match unsafe { page_and_name(page, "set_stroke_pattern", resource, resource_len) } {
            Ok(both) => both,
            Err(status) => return status,
        };
    if page.set_stroke_pattern(resource) {
        TpdfStatus::Ok
    } else {
        unregistered("set_stroke_pattern", resource)
    }
}

#[cfg(test)]
mod tests;
