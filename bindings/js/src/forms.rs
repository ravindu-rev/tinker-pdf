//! The forms surface: creating fields, and form data -- FDF and XFDF
//! (12.7.8) -- read, written and applied.
//!
//! One facade call each (ruling 11). `DocumentEditor::add_field` takes a
//! `NewField` whose kind has four arms, so it crosses as four methods, one per
//! arm, each taking that arm's payload; each answers the new field's
//! `[objectNumber, generation]`. `flags` is the facade's `i64`, so a
//! `BigInt`, as `rotatePage`'s degrees are. A value's shape crosses as its
//! arm's name -- `"none"`, `"text"`, `"state"` or `"many"` -- beside the
//! strings it is made of, and a reader's warning as its arm's name beside the
//! key or element it did not read and the field it met it in.

use js_sys::{Array, Object, Reflect};
use wasm_bindgen::prelude::*;

use tinker_pdf::form_data::{self, FieldData, FormData, FormDataWarning};
use tinker_pdf::{FieldValue, NewField, NewFieldKind, RadioButton, Rect};

use crate::{refused, PdfDocument, PdfEditor, PdfSkippedWidget};

/// One button of a radio group, for `addRadioGroup`.
#[wasm_bindgen]
pub struct PdfRadioButton {
    inner: RadioButton,
}

#[wasm_bindgen]
impl PdfRadioButton {
    /// `exportValue` is the button's on state, the name `/V` holds while it
    /// is the one selected (12.7.4.2.4); the rectangle is on zero-based page
    /// `page`, in default user space.
    #[wasm_bindgen(constructor)]
    pub fn new(
        export_value: String,
        page: u32,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
    ) -> PdfRadioButton {
        PdfRadioButton {
            inner: RadioButton {
                export: export_value,
                page,
                rect: Rect { x0, y0, x1, y1 },
            },
        }
    }
}

/// A value's shape and strings, as the facade's `FieldValue`.
fn value(kind: &str, mut values: Vec<String>) -> Result<FieldValue, JsError> {
    Ok(match (kind, values.len()) {
        ("none", 0) => FieldValue::None,
        ("text", 1) => FieldValue::Text(values.remove(0)),
        ("state", 1) => FieldValue::State(values.remove(0)),
        ("many", _) => FieldValue::Many(values),
        ("none" | "text" | "state", given) => {
            return Err(JsError::new(&format!(
                "a {kind} value takes {} strings, not {given}",
                if kind == "none" { "no" } else { "one" }
            )))
        }
        (other, _) => {
            return Err(JsError::new(&format!(
                "kind must be none, text, state or many, not {other:?}"
            )))
        }
    })
}

/// A value as its arm's name and its strings.
fn spelled(value: &FieldValue) -> (&'static str, &[String]) {
    match value {
        FieldValue::Text(text) => ("text", std::slice::from_ref(text)),
        FieldValue::State(state) => ("state", std::slice::from_ref(state)),
        FieldValue::Many(values) => ("many", values),
        FieldValue::None => ("none", &[]),
    }
}

fn set(object: &Object, key: &str, value: &JsValue) -> Result<(), JsError> {
    Reflect::set(object, &JsValue::from_str(key), value)
        .map(|_| ())
        .map_err(|_| JsError::new("could not build the result object"))
}

fn optional(value: Option<&str>) -> JsValue {
    value.map_or(JsValue::UNDEFINED, JsValue::from_str)
}

/// What an FDF or XFDF file says, or what one will be written from.
#[wasm_bindgen]
pub struct PdfFormData {
    inner: FormData,
}

#[wasm_bindgen]
impl PdfFormData {
    /// Empty form data, for `addField` to fill.
    #[wasm_bindgen(constructor)]
    pub fn new() -> PdfFormData {
        PdfFormData {
            inner: FormData::default(),
        }
    }

    /// Reads an FDF file: every field of it, or a throw and none.
    #[wasm_bindgen(js_name = readFdf)]
    pub fn read_fdf(bytes: &[u8]) -> Result<PdfFormData, JsError> {
        form_data::read_fdf(bytes)
            .map(|inner| PdfFormData { inner })
            .map_err(|error| JsError::new(&format!("readFdf: {error}")))
    }

    /// Reads an XFDF file: every field of it, or a throw and none.
    #[wasm_bindgen(js_name = readXfdf)]
    pub fn read_xfdf(bytes: &[u8]) -> Result<PdfFormData, JsError> {
        form_data::read_xfdf(bytes)
            .map(|inner| PdfFormData { inner })
            .map_err(|error| JsError::new(&format!("readXfdf: {error}")))
    }

    /// Every field as `{ name, kind, values }`: kind `"none"`, `"text"`,
    /// `"state"` or `"many"`, and the strings the value is made of.
    #[wasm_bindgen(getter)]
    pub fn fields(&self) -> Result<Array, JsError> {
        let out = Array::new();
        for field in &self.inner.fields {
            let (kind, values) = spelled(&field.value);
            let object = Object::new();
            set(&object, "name", &JsValue::from_str(&field.name))?;
            set(&object, "kind", &JsValue::from_str(kind))?;
            let strings = Array::new();
            for value in values {
                strings.push(&JsValue::from_str(value));
            }
            set(&object, "values", &strings)?;
            out.push(&object);
        }
        Ok(out)
    }

    /// Appends one field: `"none"` takes no strings, `"text"` and `"state"`
    /// one each, `"many"` any number.
    #[wasm_bindgen(js_name = addField)]
    pub fn add_field(
        &mut self,
        name: String,
        kind: &str,
        values: Vec<String>,
    ) -> Result<(), JsError> {
        let value = value(kind, values)?;
        self.inner.fields.push(FieldData { name, value });
        Ok(())
    }

    /// The document the data belongs to: FDF's `/F`, XFDF's `<f href>`, or
    /// `undefined`. Recorded and written, never opened.
    #[wasm_bindgen(getter)]
    pub fn source(&self) -> Option<String> {
        self.inner.source.clone()
    }

    /// Sets the source; `undefined` or `null` clears it.
    #[wasm_bindgen(setter)]
    pub fn set_source(&mut self, source: Option<String>) {
        self.inner.source = source;
    }

    /// What the reader did not read, as `{ kind, what, field }`: kind
    /// `"not-read"`, `"value-unreadable"`, `"tree-cut"` or `"unnamed"`;
    /// `what` the key or element not read (`undefined` unless
    /// `"not-read"`); `field` the field it was met in (`undefined` for
    /// `"unnamed"`, `""` for the file itself).
    #[wasm_bindgen(getter)]
    pub fn warnings(&self) -> Result<Array, JsError> {
        let out = Array::new();
        for warning in &self.inner.warnings {
            let (kind, what, field) = match warning {
                FormDataWarning::NotRead { what, field } => {
                    ("not-read", Some(what.as_str()), Some(field.as_str()))
                }
                FormDataWarning::ValueUnreadable { field } => {
                    ("value-unreadable", None, Some(field.as_str()))
                }
                FormDataWarning::TreeCut { field } => ("tree-cut", None, Some(field.as_str())),
                FormDataWarning::Unnamed => ("unnamed", None, None),
                other => {
                    return Err(JsError::new(&format!(
                        "a warning this build cannot spell: {other:?}"
                    )))
                }
            };
            let object = Object::new();
            set(&object, "kind", &JsValue::from_str(kind))?;
            set(&object, "what", &optional(what))?;
            set(&object, "field", &optional(field))?;
            out.push(&object);
        }
        Ok(out)
    }

    /// The data as an FDF file (12.7.8): a copy, never a view into wasm.
    #[wasm_bindgen(js_name = toFdf)]
    pub fn to_fdf(&self) -> Vec<u8> {
        self.inner.to_fdf()
    }

    /// The data as an XFDF file; throws, naming the field, for a value XML
    /// 1.0 cannot carry.
    #[wasm_bindgen(js_name = toXfdf)]
    pub fn to_xfdf(&self) -> Result<String, JsError> {
        self.inner
            .to_xfdf()
            .map_err(|error| JsError::new(&format!("toXfdf: {error}")))
    }

    /// How many fields the data holds.
    #[wasm_bindgen(getter)]
    pub fn length(&self) -> u32 {
        u32::try_from(self.inner.fields.len()).unwrap_or(u32::MAX)
    }
}

impl Default for PdfFormData {
    fn default() -> Self {
        PdfFormData::new()
    }
}

impl PdfEditor {
    /// `DocumentEditor::add_field` with the field built from its parts.
    fn add_field(
        &mut self,
        name: String,
        kind: NewFieldKind,
        flags: i64,
        font_size: f64,
    ) -> Result<Vec<u32>, JsError> {
        let mut spec = NewField::new(name, kind);
        spec.flags = flags;
        spec.font_size = font_size;
        self.inner
            .add_field(&spec)
            .map(|r| vec![r.num, u32::from(r.gen)])
            .map_err(|e| refused("addField", &e.to_string()))
    }
}

#[wasm_bindgen]
impl PdfEditor {
    /// Creates a text field (12.7.4.3) merged with its one widget; returns
    /// its `[objectNumber, generation]`. `value` and `maxLen` are `undefined`
    /// for neither; `flags` (a `BigInt`) the caller's `/Ff` bits, `0n` for
    /// none; `fontSize` the `/DA` size, 0 for auto. Throws with the facade's
    /// reason, creating nothing, when the field is refused.
    #[wasm_bindgen(js_name = addTextField)]
    #[allow(clippy::too_many_arguments)]
    pub fn add_text_field(
        &mut self,
        name: String,
        page: u32,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        value: Option<String>,
        max_len: Option<u32>,
        flags: i64,
        font_size: f64,
    ) -> Result<Vec<u32>, JsError> {
        let kind = NewFieldKind::Text {
            page,
            rect: Rect { x0, y0, x1, y1 },
            value,
            max_len,
        };
        self.add_field(name, kind, flags, font_size)
    }

    /// Creates a check box (12.7.4.2.3) whose on state is `exportValue`,
    /// ticked when `checked`. Otherwise as `addTextField`.
    #[wasm_bindgen(js_name = addCheckbox)]
    #[allow(clippy::too_many_arguments)]
    pub fn add_checkbox(
        &mut self,
        name: String,
        page: u32,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        export_value: String,
        checked: bool,
        flags: i64,
        font_size: f64,
    ) -> Result<Vec<u32>, JsError> {
        let kind = NewFieldKind::Checkbox {
            page,
            rect: Rect { x0, y0, x1, y1 },
            export: export_value,
            checked,
        };
        self.add_field(name, kind, flags, font_size)
    }

    /// Creates a radio group (12.7.4.2.4): one field and one widget per
    /// button; `selected` is the export value that starts selected, or
    /// `undefined`. Otherwise as `addTextField`.
    #[wasm_bindgen(js_name = addRadioGroup)]
    pub fn add_radio_group(
        &mut self,
        name: String,
        buttons: Vec<PdfRadioButton>,
        selected: Option<String>,
        flags: i64,
        font_size: f64,
    ) -> Result<Vec<u32>, JsError> {
        let kind = NewFieldKind::Radio {
            buttons: buttons.into_iter().map(|button| button.inner).collect(),
            selected,
        };
        self.add_field(name, kind, flags, font_size)
    }

    /// Creates a choice field (12.7.4.4): a combo box when `combo`, a list
    /// box otherwise; `editable` lets a combo box's text be typed; `value` is
    /// the initial selection or `undefined`. Otherwise as `addTextField`.
    #[wasm_bindgen(js_name = addChoiceField)]
    #[allow(clippy::too_many_arguments)]
    pub fn add_choice_field(
        &mut self,
        name: String,
        page: u32,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        options: Vec<String>,
        combo: bool,
        editable: bool,
        value: Option<String>,
        flags: i64,
        font_size: f64,
    ) -> Result<Vec<u32>, JsError> {
        let kind = NewFieldKind::Choice {
            page,
            rect: Rect { x0, y0, x1, y1 },
            options,
            combo,
            editable,
            value,
        };
        self.add_field(name, kind, flags, font_size)
    }

    /// Imports form data: every field with a value, all of them or none
    /// (`form_data::apply`). Throws, naming the field and writing nothing,
    /// when one would not take its value; otherwise returns the widgets that
    /// took a value and could not be drawn, as `fillField` does.
    #[wasm_bindgen(js_name = applyFormData)]
    pub fn apply_form_data(
        &mut self,
        data: &PdfFormData,
    ) -> Result<Vec<PdfSkippedWidget>, JsError> {
        match form_data::apply(&mut self.inner, &data.inner) {
            Ok(skipped) => Ok(skipped
                .into_iter()
                .map(|inner| PdfSkippedWidget { inner })
                .collect()),
            Err(rejection) => Err(JsError::new(&format!("apply refused: {rejection}"))),
        }
    }
}

#[wasm_bindgen]
impl PdfDocument {
    /// The data the document's fields hold, in the tree's order: every
    /// terminal field with a name and its value (`FormData::from_fields`).
    #[wasm_bindgen(js_name = formData)]
    pub fn form_data(&self) -> PdfFormData {
        PdfFormData {
            inner: FormData::from_fields(&self.inner.form_fields()),
        }
    }
}
