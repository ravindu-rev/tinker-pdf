//! The forms surface: creating fields, and form data -- FDF and XFDF (12.7.8)
//! -- read, written and applied.
//!
//! Each function is one facade call (ruling 11). `DocumentEditor::add_field`
//! takes a `NewField` whose `NewFieldKind` has four arms with four different
//! payloads, so it crosses as four functions, one per arm, each taking that
//! arm's payload as plain arguments: a union of every arm's fields would be a
//! struct where most fields mean nothing for any one call, which is the kind
//! of layout a hand-written binding gets wrong. Its refusal, `AddFieldError`,
//! crosses as [`TpdfStatus::EditRefused`] with the facade's own sentence, as
//! every editor refusal does.
//!
//! Form data is an owned [`TpdfFormData`] on the `TpdfSignatures` pattern: read
//! from an FDF or an XFDF file, taken from a document's fields, or built a
//! field at a time; written back out as either format; applied to an editor.
//! A file this reader will not read, or data the format cannot carry, is the
//! one new status, [`TpdfStatus::FormDataRefused`], with the reader's own
//! sentence in [`crate::tpdf_last_error_message`].

use std::ffi::{c_char, c_int};

use tinker_pdf::form_data::{self, FieldData, FormData, FormDataWarning};
use tinker_pdf::{DocumentEditor, FieldValue, NewField, NewFieldKind, RadioButton, Rect};

use crate::{
    count, fill_status, hand_over_string, required_bytes, required_str, set_error, TpdfBuffer,
    TpdfDocument, TpdfEditor, TpdfFillReport, TpdfStatus,
};

/// The shape of a field's value (12.7.4), as form data and the field tree
/// read it.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpdfFieldValueKind {
    /// The field is named and given no value; an import leaves it alone.
    None = 0,
    /// A text string: a text field's value, or a choice field's one
    /// selection.
    Text = 1,
    /// A name: a check box's or radio group's state, `Off` or an export value.
    State = 2,
    /// Several selections of a multiple-choice list, each a text string.
    Many = 3,
}

/// What a form-data reader met and did not read, or read leniently
/// (ruling 10).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpdfFormDataWarningKind {
    /// A key or element this reader does not read -- `/Annots`, `/AP`,
    /// `<annots>`, `<value-richtext>` -- named once per place it was met.
    NotRead = 0,
    /// A field's `/V` that is neither a string, a name nor an array of them;
    /// the field is kept with no value.
    ValueUnreadable = 1,
    /// A `/Kids` entry already walked, or one past the depth cap: the walk
    /// stopped there.
    TreeCut = 2,
    /// A field whose fully qualified name is empty, which nothing can address;
    /// it is not read.
    Unnamed = 3,
}

/// One button of a radio group for [`tpdf_editor_add_radio_group`].
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TpdfRadioButton {
    /// The button's on state: its export value, and the name `/V` holds while
    /// it is the one selected (12.7.4.2.4). Not `Off`.
    pub export_value: *const c_char,
    /// The zero-based page the button is drawn on.
    pub page: u32,
    /// Where on that page, in default user space.
    pub x0: f64,
    /// The rectangle's other corners.
    pub y0: f64,
    /// See `x0`.
    pub x1: f64,
    /// See `x0`.
    pub y1: f64,
}

/// Form data: what an FDF or XFDF file says, or what one will be written
/// from. Opaque to callers.
pub struct TpdfFormData {
    inner: FormData,
}

/// A live editor, or the refusal.
unsafe fn editor_mut<'a>(editor: *mut TpdfEditor) -> Result<&'a mut DocumentEditor, TpdfStatus> {
    match unsafe { editor.as_mut() } {
        Some(editor) => Ok(&mut editor.inner),
        None => {
            set_error("null editor");
            Err(TpdfStatus::BadArgument)
        }
    }
}

/// An optional C string, `None` for null.
unsafe fn nullable_str(value: *const c_char, what: &str) -> Result<Option<String>, TpdfStatus> {
    if value.is_null() {
        Ok(None)
    } else {
        unsafe { required_str(value, what) }.map(Some)
    }
}

/// An array of `count` C strings.
unsafe fn strings(
    values: *const *const c_char,
    count: usize,
    what: &str,
) -> Result<Vec<String>, TpdfStatus> {
    if count == 0 {
        return Ok(Vec::new());
    }
    if values.is_null() {
        set_error(&format!("null {what} array"));
        return Err(TpdfStatus::BadArgument);
    }
    (0..count)
        .map(|index| unsafe { required_str(*values.add(index), what) })
        .collect()
}

/// Creates the field, writing its reference through the out pointers.
unsafe fn add(
    editor: *mut TpdfEditor,
    name: *const c_char,
    kind: impl FnOnce() -> Result<NewFieldKind, TpdfStatus>,
    flags: i64,
    font_size: f64,
    out_object: *mut u32,
    out_generation: *mut u16,
) -> TpdfStatus {
    let editor = match unsafe { editor_mut(editor) } {
        Ok(editor) => editor,
        Err(status) => return status,
    };
    let name = match unsafe { required_str(name, "field name") } {
        Ok(name) => name,
        Err(status) => return status,
    };
    let kind = match kind() {
        Ok(kind) => kind,
        Err(status) => return status,
    };
    let mut spec = NewField::new(name, kind);
    spec.flags = flags;
    spec.font_size = font_size;
    match editor.add_field(&spec) {
        Ok(reference) => {
            if let Some(slot) = unsafe { out_object.as_mut() } {
                *slot = reference.num;
            }
            if let Some(slot) = unsafe { out_generation.as_mut() } {
                *slot = reference.gen;
            }
            TpdfStatus::Ok
        }
        Err(error) => crate::refused("add_field", &error.to_string()),
    }
}

/// Creates a text field (12.7.4.3) merged with its one widget, drawn with
/// `/Helv` from the form's `/DR` so creating and filling lay a value out the
/// same way.
///
/// `value` is the initial value, or null for none; `max_len` is `/MaxLen`
/// when `has_max_len` is non-zero. `flags` are the caller's `/Ff` bits -- the
/// bits that decide what kind of field it is are the function's, and setting
/// them is refused -- and `font_size` is the `/DA` size, 0 for auto. The new
/// field's object is written through the out pointers, either of which may
/// be null. Refused, [`TpdfStatus::EditRefused`] with the facade's reason and
/// nothing written, for a malformed or taken name, a page past the end, a
/// rectangle with no area, a value the field would refuse, contradicting
/// flags or an unusable font size.
///
/// # Safety
///
/// `editor` must be a live handle, `name` null-terminated UTF-8 and `value`
/// null or null-terminated UTF-8.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn tpdf_editor_add_text_field(
    editor: *mut TpdfEditor,
    name: *const c_char,
    page: u32,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    value: *const c_char,
    has_max_len: c_int,
    max_len: u32,
    flags: i64,
    font_size: f64,
    out_object: *mut u32,
    out_generation: *mut u16,
) -> TpdfStatus {
    let kind = || {
        Ok(NewFieldKind::Text {
            page,
            rect: Rect { x0, y0, x1, y1 },
            value: unsafe { nullable_str(value, "value") }?,
            max_len: (has_max_len != 0).then_some(max_len),
        })
    };
    unsafe {
        add(
            editor,
            name,
            kind,
            flags,
            font_size,
            out_object,
            out_generation,
        )
    }
}

/// Creates a check box (12.7.4.2.3) merged with its one widget, with an
/// `/Off` appearance and one for `export_value`, its on state.
///
/// `checked` non-zero starts it ticked. Otherwise as
/// [`tpdf_editor_add_text_field`]; an export value of `Off` is refused.
///
/// # Safety
///
/// `editor` must be a live handle and `name` and `export_value` null-terminated
/// UTF-8.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn tpdf_editor_add_checkbox(
    editor: *mut TpdfEditor,
    name: *const c_char,
    page: u32,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    export_value: *const c_char,
    checked: c_int,
    flags: i64,
    font_size: f64,
    out_object: *mut u32,
    out_generation: *mut u16,
) -> TpdfStatus {
    let kind = || {
        Ok(NewFieldKind::Checkbox {
            page,
            rect: Rect { x0, y0, x1, y1 },
            export: unsafe { required_str(export_value, "export value") }?,
            checked: checked != 0,
        })
    };
    unsafe {
        add(
            editor,
            name,
            kind,
            flags,
            font_size,
            out_object,
            out_generation,
        )
    }
}

/// Creates a radio group (12.7.4.2.4): one field, and one widget per button.
///
/// `buttons` points to `count` buttons, at least one, with export values all
/// different; `selected` is the export value of the button that starts
/// selected, or null for none. The object written back is the field whose
/// kids are the buttons' widgets. Otherwise as
/// [`tpdf_editor_add_text_field`].
///
/// # Safety
///
/// `editor` must be a live handle, `name` null-terminated UTF-8, `buttons`
/// valid for `count` buttons whose `export_value`s are null-terminated UTF-8, and
/// `selected` null or null-terminated UTF-8.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn tpdf_editor_add_radio_group(
    editor: *mut TpdfEditor,
    name: *const c_char,
    buttons: *const TpdfRadioButton,
    count: usize,
    selected: *const c_char,
    flags: i64,
    font_size: f64,
    out_object: *mut u32,
    out_generation: *mut u16,
) -> TpdfStatus {
    let kind = || {
        if buttons.is_null() && count != 0 {
            set_error("null radio button array");
            return Err(TpdfStatus::BadArgument);
        }
        let mut facade = Vec::with_capacity(count);
        for index in 0..count {
            let button = unsafe { *buttons.add(index) };
            facade.push(RadioButton {
                export: unsafe { required_str(button.export_value, "export value") }?,
                page: button.page,
                rect: Rect {
                    x0: button.x0,
                    y0: button.y0,
                    x1: button.x1,
                    y1: button.y1,
                },
            });
        }
        Ok(NewFieldKind::Radio {
            buttons: facade,
            selected: unsafe { nullable_str(selected, "selected") }?,
        })
    };
    unsafe {
        add(
            editor,
            name,
            kind,
            flags,
            font_size,
            out_object,
            out_generation,
        )
    }
}

/// Creates a choice field (12.7.4.4) merged with its one widget: a combo box
/// when `combo` is non-zero, otherwise a list box.
///
/// `options` points to `option_count` strings, each its own export value and
/// display text; `editable` non-zero lets a combo box's text be typed as well
/// as picked, and is refused on a list box; `value` is the initial selection,
/// or null for none. Otherwise as [`tpdf_editor_add_text_field`].
///
/// # Safety
///
/// `editor` must be a live handle, `name` null-terminated UTF-8, `options`
/// valid for `option_count` null-terminated UTF-8 strings, and `value` null
/// or null-terminated UTF-8.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn tpdf_editor_add_choice_field(
    editor: *mut TpdfEditor,
    name: *const c_char,
    page: u32,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    options: *const *const c_char,
    option_count: usize,
    combo: c_int,
    editable: c_int,
    value: *const c_char,
    flags: i64,
    font_size: f64,
    out_object: *mut u32,
    out_generation: *mut u16,
) -> TpdfStatus {
    let kind = || {
        Ok(NewFieldKind::Choice {
            page,
            rect: Rect { x0, y0, x1, y1 },
            options: unsafe { strings(options, option_count, "option") }?,
            combo: combo != 0,
            editable: editable != 0,
            value: unsafe { nullable_str(value, "value") }?,
        })
    };
    unsafe {
        add(
            editor,
            name,
            kind,
            flags,
            font_size,
            out_object,
            out_generation,
        )
    }
}

// ---- form data ------------------------------------------------------------

/// Hands an owned form-data handle back through `out`.
unsafe fn hand_over(out: *mut *mut TpdfFormData, inner: FormData) -> TpdfStatus {
    match unsafe { out.as_mut() } {
        Some(slot) => {
            *slot = Box::into_raw(Box::new(TpdfFormData { inner }));
            TpdfStatus::Ok
        }
        None => {
            set_error("null pointer");
            TpdfStatus::BadArgument
        }
    }
}

/// A live form-data handle, or the refusal.
unsafe fn data<'a>(handle: *const TpdfFormData) -> Result<&'a FormData, TpdfStatus> {
    match unsafe { handle.as_ref() } {
        Some(handle) => Ok(&handle.inner),
        None => {
            set_error("null form data");
            Err(TpdfStatus::BadArgument)
        }
    }
}

/// One field of form data, or the refusal naming the index.
unsafe fn field<'a>(handle: *const TpdfFormData, index: u32) -> Result<&'a FieldData, TpdfStatus> {
    let fields = &unsafe { data(handle) }?.fields;
    fields.get(index as usize).ok_or_else(|| {
        set_error(&format!("no such form-data field: index {index}"));
        TpdfStatus::BadArgument
    })
}

/// The data a document's fields hold, in the tree's order: every terminal
/// field with a name, its value as the field tree reads it
/// (`FormData::from_fields` over `Document::form_fields`). An empty handle
/// for a document with no form.
///
/// # Safety
///
/// `doc` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_document_form_data(
    doc: *const TpdfDocument,
    out: *mut *mut TpdfFormData,
) -> TpdfStatus {
    let Some(doc) = (unsafe { doc.as_ref() }) else {
        set_error("null document");
        return TpdfStatus::BadArgument;
    };
    unsafe { hand_over(out, FormData::from_fields(&doc.inner.form_fields())) }
}

/// Reads an FDF file (12.7.8): every field of it or, on
/// [`TpdfStatus::FormDataRefused`], none.
///
/// # Safety
///
/// `bytes` must be valid for `len` bytes and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_form_data_read_fdf(
    bytes: *const u8,
    len: usize,
    out: *mut *mut TpdfFormData,
) -> TpdfStatus {
    let bytes = match unsafe { required_bytes(bytes, len, "FDF bytes") } {
        Ok(bytes) => bytes,
        Err(status) => return status,
    };
    match form_data::read_fdf(bytes) {
        Ok(inner) => unsafe { hand_over(out, inner) },
        Err(error) => refused_data("read_fdf", &error.to_string()),
    }
}

/// Reads an XFDF file: every field of it or, on
/// [`TpdfStatus::FormDataRefused`], none.
///
/// # Safety
///
/// `bytes` must be valid for `len` bytes and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_form_data_read_xfdf(
    bytes: *const u8,
    len: usize,
    out: *mut *mut TpdfFormData,
) -> TpdfStatus {
    let bytes = match unsafe { required_bytes(bytes, len, "XFDF bytes") } {
        Ok(bytes) => bytes,
        Err(status) => return status,
    };
    match form_data::read_xfdf(bytes) {
        Ok(inner) => unsafe { hand_over(out, inner) },
        Err(error) => refused_data("read_xfdf", &error.to_string()),
    }
}

/// A form-data refusal, with the reader's or writer's own sentence.
fn refused_data(call: &str, detail: &str) -> TpdfStatus {
    set_error(&format!("{call}: {detail}"));
    TpdfStatus::FormDataRefused
}

/// Empty form data, for [`tpdf_form_data_add_field`] to fill.
///
/// # Safety
///
/// `out` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_form_data_new(out: *mut *mut TpdfFormData) -> TpdfStatus {
    unsafe { hand_over(out, FormData::default()) }
}

/// Appends one field: its fully qualified name and its value, given as
/// `count` strings -- none for [`TpdfFieldValueKind::None`], exactly one for
/// `Text` and `State`, any number for `Many`. A count that does not fit the
/// kind is [`TpdfStatus::BadArgument`].
///
/// # Safety
///
/// `handle` must be a live handle, `name` null-terminated UTF-8, and `values`
/// valid for `count` null-terminated UTF-8 strings.
#[no_mangle]
pub unsafe extern "C" fn tpdf_form_data_add_field(
    handle: *mut TpdfFormData,
    name: *const c_char,
    kind: TpdfFieldValueKind,
    values: *const *const c_char,
    count: usize,
) -> TpdfStatus {
    let Some(handle) = (unsafe { handle.as_mut() }) else {
        set_error("null form data");
        return TpdfStatus::BadArgument;
    };
    let name = match unsafe { required_str(name, "field name") } {
        Ok(name) => name,
        Err(status) => return status,
    };
    let mut values = match unsafe { strings(values, count, "value") } {
        Ok(values) => values,
        Err(status) => return status,
    };
    let value = match (kind, values.len()) {
        (TpdfFieldValueKind::None, 0) => FieldValue::None,
        (TpdfFieldValueKind::Text, 1) => FieldValue::Text(values.remove(0)),
        (TpdfFieldValueKind::State, 1) => FieldValue::State(values.remove(0)),
        (TpdfFieldValueKind::Many, _) => FieldValue::Many(values),
        (kind, given) => {
            set_error(&format!(
                "a {kind:?} value takes {}, not {given} strings",
                match kind {
                    TpdfFieldValueKind::None => "no",
                    _ => "one",
                }
            ));
            return TpdfStatus::BadArgument;
        }
    };
    handle.inner.fields.push(FieldData { name, value });
    TpdfStatus::Ok
}

/// Sets the document the data belongs to: FDF's `/F`, XFDF's `<f href>`.
/// Recorded and written, never opened. Null clears it.
///
/// # Safety
///
/// `handle` must be a live handle and `source` null or null-terminated
/// UTF-8.
#[no_mangle]
pub unsafe extern "C" fn tpdf_form_data_set_source(
    handle: *mut TpdfFormData,
    source: *const c_char,
) -> TpdfStatus {
    let Some(handle) = (unsafe { handle.as_mut() }) else {
        set_error("null form data");
        return TpdfStatus::BadArgument;
    };
    match unsafe { nullable_str(source, "source") } {
        Ok(source) => {
            handle.inner.source = source;
            TpdfStatus::Ok
        }
        Err(status) => status,
    }
}

/// Hands written bytes back as a buffer.
unsafe fn hand_over_buffer(out: *mut *mut TpdfBuffer, inner: Vec<u8>) -> TpdfStatus {
    match unsafe { out.as_mut() } {
        Some(slot) => {
            *slot = Box::into_raw(Box::new(TpdfBuffer { inner }));
            TpdfStatus::Ok
        }
        None => {
            set_error("null pointer");
            TpdfStatus::BadArgument
        }
    }
}

/// Writes the data as an FDF file (12.7.8), the names a tree again, freed
/// with [`crate::tpdf_buffer_free`].
///
/// # Safety
///
/// `handle` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_form_data_to_fdf(
    handle: *const TpdfFormData,
    out: *mut *mut TpdfBuffer,
) -> TpdfStatus {
    match unsafe { data(handle) } {
        Ok(data) => unsafe { hand_over_buffer(out, data.to_fdf()) },
        Err(status) => status,
    }
}

/// Writes the data as an XFDF file, UTF-8, freed with
/// [`crate::tpdf_buffer_free`]. A name or value XML 1.0 cannot carry is
/// [`TpdfStatus::FormDataRefused`], naming the field.
///
/// # Safety
///
/// `handle` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_form_data_to_xfdf(
    handle: *const TpdfFormData,
    out: *mut *mut TpdfBuffer,
) -> TpdfStatus {
    let data = match unsafe { data(handle) } {
        Ok(data) => data,
        Err(status) => return status,
    };
    match data.to_xfdf() {
        Ok(text) => unsafe { hand_over_buffer(out, text.into_bytes()) },
        Err(error) => refused_data("to_xfdf", &error.to_string()),
    }
}

/// How many fields the data holds, or zero for a null handle.
///
/// # Safety
///
/// `handle` must be a live handle or null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_form_data_count(handle: *const TpdfFormData) -> u32 {
    unsafe { handle.as_ref() }.map_or(0, |handle| count(handle.inner.fields.len()))
}

/// A field's fully qualified name, freed with [`crate::tpdf_string_free`].
///
/// # Safety
///
/// `handle` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_form_data_field_name(
    handle: *const TpdfFormData,
    index: u32,
    out: *mut *mut c_char,
) -> TpdfStatus {
    match unsafe { field(handle, index) } {
        Ok(field) => unsafe { hand_over_string(out, Some(&field.name)) },
        Err(status) => status,
    }
}

/// The shape of a field's value.
///
/// # Safety
///
/// `handle` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_form_data_field_value_kind(
    handle: *const TpdfFormData,
    index: u32,
    out: *mut TpdfFieldValueKind,
) -> TpdfStatus {
    let field = match unsafe { field(handle, index) } {
        Ok(field) => field,
        Err(status) => return status,
    };
    let Some(slot) = (unsafe { out.as_mut() }) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    *slot = match &field.value {
        FieldValue::Text(_) => TpdfFieldValueKind::Text,
        FieldValue::State(_) => TpdfFieldValueKind::State,
        FieldValue::Many(_) => TpdfFieldValueKind::Many,
        FieldValue::None => TpdfFieldValueKind::None,
    };
    TpdfStatus::Ok
}

/// The strings a field's value is made of.
fn value_strings(value: &FieldValue) -> &[String] {
    match value {
        FieldValue::Text(text) | FieldValue::State(text) => std::slice::from_ref(text),
        FieldValue::Many(values) => values,
        FieldValue::None => &[],
    }
}

/// How many strings a field's value is made of: none for `None`, one for
/// `Text` and `State`, each selection for `Many`. Zero for a null handle or
/// an index past the end, which [`tpdf_form_data_field_value_kind`] tells
/// apart from a value with no strings.
///
/// # Safety
///
/// `handle` must be a live handle or null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_form_data_field_value_count(
    handle: *const TpdfFormData,
    index: u32,
) -> u32 {
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return 0;
    };
    handle
        .inner
        .fields
        .get(index as usize)
        .map_or(0, |field| count(value_strings(&field.value).len()))
}

/// The `string`th string of a field's value, freed with
/// [`crate::tpdf_string_free`]. Past the end is [`TpdfStatus::BadArgument`].
///
/// # Safety
///
/// `handle` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_form_data_field_value(
    handle: *const TpdfFormData,
    index: u32,
    string: u32,
    out: *mut *mut c_char,
) -> TpdfStatus {
    let field = match unsafe { field(handle, index) } {
        Ok(field) => field,
        Err(status) => return status,
    };
    match value_strings(&field.value).get(string as usize) {
        Some(value) => unsafe { hand_over_string(out, Some(value)) },
        None => {
            set_error(&format!("field {index} has no value string {string}"));
            TpdfStatus::BadArgument
        }
    }
}

/// The document the data belongs to, freed with [`crate::tpdf_string_free`];
/// **null on `Ok` when the data names none**.
///
/// # Safety
///
/// `handle` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_form_data_source(
    handle: *const TpdfFormData,
    out: *mut *mut c_char,
) -> TpdfStatus {
    match unsafe { data(handle) } {
        Ok(data) => unsafe { hand_over_string(out, data.source.as_deref()) },
        Err(status) => status,
    }
}

/// How many warnings the reader left, or zero for a null handle.
///
/// # Safety
///
/// `handle` must be a live handle or null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_form_data_warning_count(handle: *const TpdfFormData) -> u32 {
    unsafe { handle.as_ref() }.map_or(0, |handle| count(handle.inner.warnings.len()))
}

/// One warning: its kind, the key or element not read (null unless
/// `NotRead`), and the field it was met in (null for `Unnamed`; empty for the
/// file itself). Strings are freed with [`crate::tpdf_string_free`].
///
/// # Safety
///
/// `handle` must be a live handle and the out pointers valid.
#[no_mangle]
pub unsafe extern "C" fn tpdf_form_data_warning(
    handle: *const TpdfFormData,
    index: u32,
    out_kind: *mut TpdfFormDataWarningKind,
    out_what: *mut *mut c_char,
    out_field: *mut *mut c_char,
) -> TpdfStatus {
    let data = match unsafe { data(handle) } {
        Ok(data) => data,
        Err(status) => return status,
    };
    let Some(warning) = data.warnings.get(index as usize) else {
        set_error(&format!("no such form-data warning: index {index}"));
        return TpdfStatus::BadArgument;
    };
    if out_kind.is_null() || out_what.is_null() || out_field.is_null() {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    }
    let (kind, what, field) = match warning {
        FormDataWarning::NotRead { what, field } => (
            TpdfFormDataWarningKind::NotRead,
            Some(what.as_str()),
            Some(field.as_str()),
        ),
        FormDataWarning::ValueUnreadable { field } => (
            TpdfFormDataWarningKind::ValueUnreadable,
            None,
            Some(field.as_str()),
        ),
        FormDataWarning::TreeCut { field } => {
            (TpdfFormDataWarningKind::TreeCut, None, Some(field.as_str()))
        }
        FormDataWarning::Unnamed => (TpdfFormDataWarningKind::Unnamed, None, None),
        // The enum is non-exhaustive; a warning this projection predates is
        // still a warning, and saying nothing about it would be the silence
        // ruling 10 is against.
        _ => {
            set_error(&format!("a warning this build cannot spell: {warning:?}"));
            return TpdfStatus::BadArgument;
        }
    };
    unsafe { *out_kind = kind };
    let status = unsafe { hand_over_string(out_what, what) };
    if status != TpdfStatus::Ok {
        return status;
    }
    let status = unsafe { hand_over_string(out_field, field) };
    if status != TpdfStatus::Ok {
        unsafe { crate::tpdf_string_free(*out_what) };
        unsafe { *out_what = std::ptr::null_mut() };
    }
    status
}

/// Frees form data. Null is accepted and does nothing.
///
/// # Safety
///
/// `handle` must have come from one of this module's constructors and must
/// not be used afterwards.
#[no_mangle]
pub unsafe extern "C" fn tpdf_form_data_free(handle: *mut TpdfFormData) {
    if !handle.is_null() {
        drop(unsafe { Box::from_raw(handle) });
    }
}

/// Imports form data into an editor: every field with a value, all of them
/// or none of them (`form_data::apply`, through `set_field_values`).
///
/// The three outcomes are [`crate::tpdf_editor_fill_field`]'s: a non-`Ok`
/// status -- `NoSuchField`, `ValueRefused` or `FieldUnreadable`, the field
/// named in [`crate::tpdf_last_error_message`] -- means **nothing was
/// written**; `Ok` writes a [`TpdfFillReport`] through `out_report` (which
/// may be null) of the widgets that took a value and could not be drawn.
///
/// # Safety
///
/// `editor` and `handle` must be live handles and `out_report` a valid
/// pointer or null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_editor_apply_form_data(
    editor: *mut TpdfEditor,
    handle: *const TpdfFormData,
    out_report: *mut *mut TpdfFillReport,
) -> TpdfStatus {
    let editor = match unsafe { editor_mut(editor) } {
        Ok(editor) => editor,
        Err(status) => return status,
    };
    let data = match unsafe { data(handle) } {
        Ok(data) => data,
        Err(status) => return status,
    };
    match form_data::apply(editor, data) {
        Ok(skipped) => {
            if let Some(slot) = unsafe { out_report.as_mut() } {
                *slot = Box::into_raw(Box::new(TpdfFillReport { inner: skipped }));
            }
            TpdfStatus::Ok
        }
        Err(rejection) => {
            set_error(&format!("apply refused: {rejection}"));
            fill_status(rejection.reason)
        }
    }
}

#[cfg(test)]
mod tests;
