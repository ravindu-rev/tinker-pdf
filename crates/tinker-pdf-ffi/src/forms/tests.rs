//! The forms surface is pinned the way the write surface is: the same fields
//! created through the C ABI and through `DocumentEditor::add_field` must
//! save **the same bytes**, form data written through the C ABI must be the
//! bytes `FormData` writes, read back as the fields `form_data` reads, and
//! applied as the same document `form_data::apply` makes.

use super::*;
use crate::{
    tpdf_buffer_data, tpdf_buffer_free, tpdf_document_editor, tpdf_document_free,
    tpdf_document_open, tpdf_editor_free, tpdf_editor_save, tpdf_fill_report_count,
    tpdf_fill_report_free, tpdf_last_error_message, tpdf_string_free, tpdf_write_options_init,
    TpdfWriteOptions,
};
use std::ffi::{CStr, CString};
use std::ptr;
use tinker_pdf::{Document, WriteOptions};

fn form_fixture() -> Vec<u8> {
    let path =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/form-fields.pdf");
    std::fs::read(path).expect("the fixture is in the tree")
}

fn last_error() -> String {
    let pointer = unsafe { tpdf_last_error_message() };
    if pointer.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned()
}

fn editor_over(bytes: &[u8]) -> *mut TpdfEditor {
    let mut doc = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_document_open(bytes.as_ptr(), bytes.len(), &mut doc) },
        TpdfStatus::Ok
    );
    let mut editor = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_document_editor(doc, &mut editor) },
        TpdfStatus::Ok
    );
    unsafe { tpdf_document_free(doc) };
    editor
}

fn take_buffer(buffer: *mut TpdfBuffer) -> Vec<u8> {
    let mut len = 0;
    let data = unsafe { tpdf_buffer_data(buffer, &mut len) };
    let bytes = unsafe { std::slice::from_raw_parts(data, len) }.to_vec();
    unsafe { tpdf_buffer_free(buffer) };
    bytes
}

fn save(editor: *mut TpdfEditor) -> Vec<u8> {
    let mut options = std::mem::MaybeUninit::<TpdfWriteOptions>::uninit();
    assert_eq!(
        unsafe { tpdf_write_options_init(options.as_mut_ptr()) },
        TpdfStatus::Ok
    );
    let options = unsafe { options.assume_init() };
    let mut buffer = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_editor_save(editor, &options, &mut buffer) },
        TpdfStatus::Ok
    );
    take_buffer(buffer)
}

fn take_string(pointer: *mut c_char) -> Option<String> {
    if pointer.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    unsafe { tpdf_string_free(pointer) };
    Some(text)
}

fn c(text: &str) -> CString {
    CString::new(text).expect("no nul")
}

/// The four kinds of field, created through the C ABI.
fn add_every_kind_through_the_abi(editor: *mut TpdfEditor) {
    let (mut num, mut gen) = (0, 9);
    let (name, value) = (c("person.given"), c("Ada"));
    assert_eq!(
        unsafe {
            tpdf_editor_add_text_field(
                editor,
                name.as_ptr(),
                0,
                300.0,
                700.0,
                500.0,
                720.0,
                value.as_ptr(),
                1,
                20,
                0,
                0.0,
                &mut num,
                &mut gen,
            )
        },
        TpdfStatus::Ok,
        "{}",
        last_error()
    );
    assert!(num > 0 && gen == 0, "the field is a new object");

    let (name, export) = (c("subscribe"), c("Yes"));
    assert_eq!(
        unsafe {
            tpdf_editor_add_checkbox(
                editor,
                name.as_ptr(),
                0,
                300.0,
                660.0,
                315.0,
                675.0,
                export.as_ptr(),
                1,
                0,
                0.0,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        },
        TpdfStatus::Ok,
        "{}",
        last_error()
    );

    let (name, small, large) = (c("size"), c("small"), c("large"));
    let buttons = [
        TpdfRadioButton {
            export_value: small.as_ptr(),
            page: 0,
            x0: 300.0,
            y0: 620.0,
            x1: 315.0,
            y1: 635.0,
        },
        TpdfRadioButton {
            export_value: large.as_ptr(),
            page: 0,
            x0: 330.0,
            y0: 620.0,
            x1: 345.0,
            y1: 635.0,
        },
    ];
    assert_eq!(
        unsafe {
            tpdf_editor_add_radio_group(
                editor,
                name.as_ptr(),
                buttons.as_ptr(),
                buttons.len(),
                large.as_ptr(),
                0,
                0.0,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        },
        TpdfStatus::Ok,
        "{}",
        last_error()
    );

    let (name, red, green) = (c("shade"), c("red"), c("green"));
    let options = [red.as_ptr(), green.as_ptr()];
    assert_eq!(
        unsafe {
            tpdf_editor_add_choice_field(
                editor,
                name.as_ptr(),
                0,
                300.0,
                580.0,
                400.0,
                600.0,
                options.as_ptr(),
                options.len(),
                1,
                0,
                green.as_ptr(),
                0,
                12.0,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        },
        TpdfStatus::Ok,
        "{}",
        last_error()
    );
}

/// The same four, against the facade.
fn add_every_kind_against_the_facade(editor: &mut DocumentEditor) {
    let mut text = NewField::new(
        "person.given",
        NewFieldKind::Text {
            page: 0,
            rect: Rect {
                x0: 300.0,
                y0: 700.0,
                x1: 500.0,
                y1: 720.0,
            },
            value: Some("Ada".to_string()),
            max_len: Some(20),
        },
    );
    text.flags = 0;
    editor.add_field(&text).expect("the text field is created");
    editor
        .add_field(&NewField::new(
            "subscribe",
            NewFieldKind::Checkbox {
                page: 0,
                rect: Rect {
                    x0: 300.0,
                    y0: 660.0,
                    x1: 315.0,
                    y1: 675.0,
                },
                export: "Yes".to_string(),
                checked: true,
            },
        ))
        .expect("the check box is created");
    editor
        .add_field(&NewField::new(
            "size",
            NewFieldKind::Radio {
                buttons: vec![
                    RadioButton {
                        export: "small".to_string(),
                        page: 0,
                        rect: Rect {
                            x0: 300.0,
                            y0: 620.0,
                            x1: 315.0,
                            y1: 635.0,
                        },
                    },
                    RadioButton {
                        export: "large".to_string(),
                        page: 0,
                        rect: Rect {
                            x0: 330.0,
                            y0: 620.0,
                            x1: 345.0,
                            y1: 635.0,
                        },
                    },
                ],
                selected: Some("large".to_string()),
            },
        ))
        .expect("the radio group is created");
    let mut choice = NewField::new(
        "shade",
        NewFieldKind::Choice {
            page: 0,
            rect: Rect {
                x0: 300.0,
                y0: 580.0,
                x1: 400.0,
                y1: 600.0,
            },
            options: vec!["red".to_string(), "green".to_string()],
            combo: true,
            editable: false,
            value: Some("green".to_string()),
        },
    );
    choice.font_size = 12.0;
    editor
        .add_field(&choice)
        .expect("the choice field is created");
}

#[test]
fn every_kind_of_field_saves_the_facades_bytes() {
    let fixture = form_fixture();
    let editor = editor_over(&fixture);
    add_every_kind_through_the_abi(editor);
    let through_the_abi = save(editor);
    unsafe { tpdf_editor_free(editor) };

    let mut facade = Document::open(fixture).expect("opens").editor();
    add_every_kind_against_the_facade(&mut facade);
    let against_the_facade = facade.save(&WriteOptions::default());
    assert_eq!(through_the_abi, against_the_facade);

    let reopened = Document::open(through_the_abi).expect("the result reopens");
    assert!(reopened.validate().is_empty(), "{:?}", reopened.validate());
    let names: Vec<String> = reopened.form_fields().into_iter().map(|f| f.name).collect();
    for name in ["person.given", "subscribe", "size", "shade"] {
        assert!(names.iter().any(|n| n == name), "{name} in {names:?}");
    }
}

#[test]
fn a_refused_field_writes_nothing_and_says_why() {
    let fixture = form_fixture();
    let editor = editor_over(&fixture);
    let before = save(editor);
    let name = c("name");
    // `name` is the fixture's own field: taken.
    assert_eq!(
        unsafe {
            tpdf_editor_add_text_field(
                editor,
                name.as_ptr(),
                0,
                0.0,
                0.0,
                10.0,
                10.0,
                ptr::null(),
                0,
                0,
                0,
                0.0,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        },
        TpdfStatus::EditRefused
    );
    assert!(last_error().contains("add_field"), "{}", last_error());
    assert!(last_error().contains("exists"), "{}", last_error());
    // A rectangle with no area, a page past the end, an `Off` export, no
    // buttons: each refused and each naming the facade's reason.
    let fresh = c("fresh");
    assert_eq!(
        unsafe {
            tpdf_editor_add_text_field(
                editor,
                fresh.as_ptr(),
                0,
                5.0,
                5.0,
                5.0,
                9.0,
                ptr::null(),
                0,
                0,
                0,
                0.0,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        },
        TpdfStatus::EditRefused
    );
    assert!(last_error().contains("no area"), "{}", last_error());
    let off = c("Off");
    assert_eq!(
        unsafe {
            tpdf_editor_add_checkbox(
                editor,
                fresh.as_ptr(),
                7,
                0.0,
                0.0,
                10.0,
                10.0,
                off.as_ptr(),
                0,
                0,
                0.0,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        },
        TpdfStatus::EditRefused
    );
    assert_eq!(
        unsafe {
            tpdf_editor_add_radio_group(
                editor,
                fresh.as_ptr(),
                ptr::null(),
                0,
                ptr::null(),
                0,
                0.0,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        },
        TpdfStatus::EditRefused
    );
    assert!(last_error().contains("needs a button"), "{}", last_error());
    assert_eq!(save(editor), before, "nothing was written");
    unsafe { tpdf_editor_free(editor) };
}

fn document_data(bytes: &[u8]) -> *mut TpdfFormData {
    let mut doc = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_document_open(bytes.as_ptr(), bytes.len(), &mut doc) },
        TpdfStatus::Ok
    );
    let mut data = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_document_form_data(doc, &mut data) },
        TpdfStatus::Ok
    );
    unsafe { tpdf_document_free(doc) };
    data
}

/// Every field of a handle, as the facade's own type.
fn fields_of(handle: *const TpdfFormData) -> Vec<FieldData> {
    (0..unsafe { tpdf_form_data_count(handle) })
        .map(|index| {
            let mut name = ptr::null_mut();
            assert_eq!(
                unsafe { tpdf_form_data_field_name(handle, index, &mut name) },
                TpdfStatus::Ok
            );
            let mut kind = TpdfFieldValueKind::None;
            assert_eq!(
                unsafe { tpdf_form_data_field_value_kind(handle, index, &mut kind) },
                TpdfStatus::Ok
            );
            let strings: Vec<String> =
                (0..unsafe { tpdf_form_data_field_value_count(handle, index) })
                    .map(|string| {
                        let mut out = ptr::null_mut();
                        assert_eq!(
                            unsafe { tpdf_form_data_field_value(handle, index, string, &mut out) },
                            TpdfStatus::Ok
                        );
                        take_string(out).expect("a value string")
                    })
                    .collect();
            let value = match kind {
                TpdfFieldValueKind::None => FieldValue::None,
                TpdfFieldValueKind::Text => FieldValue::Text(strings[0].clone()),
                TpdfFieldValueKind::State => FieldValue::State(strings[0].clone()),
                TpdfFieldValueKind::Many => FieldValue::Many(strings),
            };
            FieldData {
                name: take_string(name).expect("a name"),
                value,
            }
        })
        .collect()
}

#[test]
fn a_documents_form_data_is_the_facades_and_writes_its_bytes() {
    let fixture = form_fixture();
    let mut filled = Document::open(fixture).expect("opens").editor();
    filled.fill_field("notes", "carried").expect("fills");
    filled.set_checkbox("agree", true);
    let filled = filled.save(&WriteOptions::default());

    let handle = document_data(&filled);
    let facade = FormData::from_fields(&Document::open(filled).expect("opens").form_fields());
    assert_eq!(fields_of(handle), facade.fields);
    assert!(!facade.fields.is_empty());

    let mut buffer = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_form_data_to_fdf(handle, &mut buffer) },
        TpdfStatus::Ok
    );
    let fdf = take_buffer(buffer);
    assert_eq!(fdf, facade.to_fdf());
    assert_eq!(
        unsafe { tpdf_form_data_to_xfdf(handle, &mut buffer) },
        TpdfStatus::Ok
    );
    let xfdf = take_buffer(buffer);
    assert_eq!(xfdf, facade.to_xfdf().expect("writes").into_bytes());
    unsafe { tpdf_form_data_free(handle) };

    // And both read back as the facade reads them.
    let mut read = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_form_data_read_fdf(fdf.as_ptr(), fdf.len(), &mut read) },
        TpdfStatus::Ok
    );
    assert_eq!(
        fields_of(read),
        form_data::read_fdf(&fdf).expect("reads").fields
    );
    unsafe { tpdf_form_data_free(read) };
    assert_eq!(
        unsafe { tpdf_form_data_read_xfdf(xfdf.as_ptr(), xfdf.len(), &mut read) },
        TpdfStatus::Ok
    );
    assert_eq!(
        fields_of(read),
        form_data::read_xfdf(&xfdf).expect("reads").fields
    );
    unsafe { tpdf_form_data_free(read) };
}

#[test]
fn data_built_a_field_at_a_time_is_the_facades() {
    let mut handle = ptr::null_mut();
    assert_eq!(unsafe { tpdf_form_data_new(&mut handle) }, TpdfStatus::Ok);
    let (notes, value) = (c("notes"), c("from data"));
    let values = [value.as_ptr()];
    assert_eq!(
        unsafe {
            tpdf_form_data_add_field(
                handle,
                notes.as_ptr(),
                TpdfFieldValueKind::Text as c_int,
                values.as_ptr(),
                1,
            )
        },
        TpdfStatus::Ok
    );
    let (agree, on) = (c("agree"), c("On"));
    let states = [on.as_ptr()];
    assert_eq!(
        unsafe {
            tpdf_form_data_add_field(
                handle,
                agree.as_ptr(),
                TpdfFieldValueKind::State as c_int,
                states.as_ptr(),
                1,
            )
        },
        TpdfStatus::Ok
    );
    let (list, a, b) = (c("list"), c("a"), c("b"));
    let many = [a.as_ptr(), b.as_ptr()];
    assert_eq!(
        unsafe {
            tpdf_form_data_add_field(
                handle,
                list.as_ptr(),
                TpdfFieldValueKind::Many as c_int,
                many.as_ptr(),
                2,
            )
        },
        TpdfStatus::Ok
    );
    let empty = c("empty");
    assert_eq!(
        unsafe {
            tpdf_form_data_add_field(
                handle,
                empty.as_ptr(),
                TpdfFieldValueKind::None as c_int,
                ptr::null(),
                0,
            )
        },
        TpdfStatus::Ok
    );
    // A count that does not fit the kind is refused, and nothing is added.
    assert_eq!(
        unsafe {
            tpdf_form_data_add_field(
                handle,
                empty.as_ptr(),
                TpdfFieldValueKind::Text as c_int,
                ptr::null(),
                0,
            )
        },
        TpdfStatus::BadArgument
    );
    assert_eq!(
        unsafe {
            tpdf_form_data_add_field(
                handle,
                empty.as_ptr(),
                TpdfFieldValueKind::None as c_int,
                many.as_ptr(),
                2,
            )
        },
        TpdfStatus::BadArgument
    );
    let source = c("form-fields.pdf");
    assert_eq!(
        unsafe { tpdf_form_data_set_source(handle, source.as_ptr()) },
        TpdfStatus::Ok
    );

    let facade = FormData {
        fields: vec![
            FieldData {
                name: "notes".to_string(),
                value: FieldValue::Text("from data".to_string()),
            },
            FieldData {
                name: "agree".to_string(),
                value: FieldValue::State("On".to_string()),
            },
            FieldData {
                name: "list".to_string(),
                value: FieldValue::Many(vec!["a".to_string(), "b".to_string()]),
            },
            FieldData {
                name: "empty".to_string(),
                value: FieldValue::None,
            },
        ],
        source: Some("form-fields.pdf".to_string()),
        warnings: Vec::new(),
    };
    assert_eq!(fields_of(handle), facade.fields);
    let mut out = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_form_data_source(handle, &mut out) },
        TpdfStatus::Ok
    );
    assert_eq!(take_string(out).as_deref(), Some("form-fields.pdf"));
    let mut buffer = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_form_data_to_fdf(handle, &mut buffer) },
        TpdfStatus::Ok
    );
    assert_eq!(take_buffer(buffer), facade.to_fdf());

    // Null clears the source, and a cleared source reads as null on Ok.
    assert_eq!(
        unsafe { tpdf_form_data_set_source(handle, ptr::null()) },
        TpdfStatus::Ok
    );
    out = c("x").into_raw();
    let stale = out;
    assert_eq!(
        unsafe { tpdf_form_data_source(handle, &mut out) },
        TpdfStatus::Ok
    );
    assert!(out.is_null());
    drop(unsafe { CString::from_raw(stale) });
    unsafe { tpdf_form_data_free(handle) };
}

#[test]
fn applying_data_is_the_facades_apply_all_or_nothing() {
    let fixture = form_fixture();
    let mut text = XFDF_NOTES.as_bytes().to_vec();
    let mut handle = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_form_data_read_xfdf(text.as_ptr(), text.len(), &mut handle) },
        TpdfStatus::Ok
    );
    let editor = editor_over(&fixture);
    let mut report = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_editor_apply_form_data(editor, handle, &mut report) },
        TpdfStatus::Ok,
        "{}",
        last_error()
    );
    assert_eq!(unsafe { tpdf_fill_report_count(report) }, 0);
    unsafe { tpdf_fill_report_free(report) };
    let through_the_abi = save(editor);
    unsafe { tpdf_editor_free(editor) };

    let mut facade = Document::open(fixture.clone()).expect("opens").editor();
    let data = form_data::read_xfdf(XFDF_NOTES.as_bytes()).expect("reads");
    form_data::apply(&mut facade, &data).expect("applies");
    assert_eq!(through_the_abi, facade.save(&WriteOptions::default()));
    unsafe { tpdf_form_data_free(handle) };

    // A field the document does not have: NoSuchField, named, nothing written.
    text = XFDF_NOTES
        .replace("<field name=\"agree\">", "<field name=\"nowhere\">")
        .into_bytes();
    assert_eq!(
        unsafe { tpdf_form_data_read_xfdf(text.as_ptr(), text.len(), &mut handle) },
        TpdfStatus::Ok
    );
    let editor = editor_over(&fixture);
    let before = save(editor);
    assert_eq!(
        unsafe { tpdf_editor_apply_form_data(editor, handle, ptr::null_mut()) },
        TpdfStatus::NoSuchField
    );
    assert!(last_error().contains("nowhere"), "{}", last_error());
    assert_eq!(save(editor), before, "nothing was written");
    unsafe { tpdf_editor_free(editor) };
    unsafe { tpdf_form_data_free(handle) };
}

const XFDF_NOTES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<xfdf xmlns=\"http://ns.adobe.com/xfdf/\"><fields>\
<field name=\"notes\"><value>from an XFDF</value></field>\
<field name=\"agree\"><value>On</value></field>\
</fields><annots/></xfdf>";

/// An FDF whose reader leaves every kind of warning: `/Status` it does not
/// read, a `/V` that is a number, a field with no `/T` and nothing above it to
/// name it, and a field whose `/Kids` is itself.
const FDF_EVERY_WARNING: &str = "%FDF-1.2\n\
1 0 obj\n\
<< /FDF << /Fields [ << /T (number) /V 12 >> << /V (no name) >> 2 0 R ] /Status (x) >> >>\n\
endobj\n\
2 0 obj\n\
<< /T (loop) /Kids [ 2 0 R ] >>\n\
endobj\n\
trailer\n\
<< /Root 1 0 R >>\n\
%%EOF\n";

/// Every warning a handle carries equals the facade's, arm and strings.
fn warnings_cross(handle: *const TpdfFormData, facade: &[FormDataWarning]) {
    assert_eq!(
        unsafe { tpdf_form_data_warning_count(handle) },
        count(facade.len())
    );
    for (index, warning) in facade.iter().enumerate() {
        let mut kind = TpdfFormDataWarningKind::Unnamed;
        let (mut what, mut field) = (ptr::null_mut(), ptr::null_mut());
        assert_eq!(
            unsafe {
                tpdf_form_data_warning(handle, index as u32, &mut kind, &mut what, &mut field)
            },
            TpdfStatus::Ok
        );
        let (what, field) = (take_string(what), take_string(field));
        let expected = match warning {
            FormDataWarning::NotRead { what, field } => (
                TpdfFormDataWarningKind::NotRead,
                Some(what.clone()),
                Some(field.clone()),
            ),
            FormDataWarning::ValueUnreadable { field } => (
                TpdfFormDataWarningKind::ValueUnreadable,
                None,
                Some(field.clone()),
            ),
            FormDataWarning::TreeCut { field } => {
                (TpdfFormDataWarningKind::TreeCut, None, Some(field.clone()))
            }
            FormDataWarning::Unnamed => (TpdfFormDataWarningKind::Unnamed, None, None),
            other => panic!("a warning this test predates: {other:?}"),
        };
        assert_eq!((kind, what, field), expected, "warning {index}");
    }
}

#[test]
fn what_the_reader_set_aside_crosses_as_warnings() {
    let text = XFDF_NOTES.as_bytes();
    let mut handle = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_form_data_read_xfdf(text.as_ptr(), text.len(), &mut handle) },
        TpdfStatus::Ok
    );
    let facade = form_data::read_xfdf(text).expect("reads");
    assert!(
        !facade.warnings.is_empty(),
        "<annots> is not read, and says so"
    );
    warnings_cross(handle, &facade.warnings);
    // Past the end: BadArgument.
    let mut kind = TpdfFormDataWarningKind::Unnamed;
    let (mut what, mut field) = (ptr::null_mut(), ptr::null_mut());
    assert_eq!(
        unsafe { tpdf_form_data_warning(handle, 99, &mut kind, &mut what, &mut field) },
        TpdfStatus::BadArgument
    );
    unsafe { tpdf_form_data_free(handle) };

    // And every other arm, each spelled as its own kind with its own strings.
    let text = FDF_EVERY_WARNING.as_bytes();
    assert_eq!(
        unsafe { tpdf_form_data_read_fdf(text.as_ptr(), text.len(), &mut handle) },
        TpdfStatus::Ok
    );
    let facade = form_data::read_fdf(text).expect("reads");
    for arm in [
        |w: &FormDataWarning| matches!(w, FormDataWarning::NotRead { .. }),
        |w: &FormDataWarning| matches!(w, FormDataWarning::ValueUnreadable { .. }),
        |w: &FormDataWarning| matches!(w, FormDataWarning::TreeCut { .. }),
        |w: &FormDataWarning| matches!(w, FormDataWarning::Unnamed),
    ] {
        assert!(
            facade.warnings.iter().any(arm),
            "the fixture reaches every arm: {:?}",
            facade.warnings
        );
    }
    warnings_cross(handle, &facade.warnings);
    unsafe { tpdf_form_data_free(handle) };
}

#[test]
fn bytes_that_are_not_form_data_are_refused_with_the_readers_reason() {
    let mut handle = ptr::null_mut();
    let garbage = b"not form data at all";
    assert_eq!(
        unsafe { tpdf_form_data_read_fdf(garbage.as_ptr(), garbage.len(), &mut handle) },
        TpdfStatus::FormDataRefused
    );
    assert!(last_error().starts_with("read_fdf: "), "{}", last_error());
    assert!(handle.is_null());
    let html = b"<html/>";
    assert_eq!(
        unsafe { tpdf_form_data_read_xfdf(html.as_ptr(), html.len(), &mut handle) },
        TpdfStatus::FormDataRefused
    );
    assert!(last_error().contains("xfdf"), "{}", last_error());

    // A value XML 1.0 cannot carry is refused by the XFDF writer, by name.
    assert_eq!(unsafe { tpdf_form_data_new(&mut handle) }, TpdfStatus::Ok);
    let (name, bell) = (c("bell"), c("\u{7}"));
    let values = [bell.as_ptr()];
    assert_eq!(
        unsafe {
            tpdf_form_data_add_field(
                handle,
                name.as_ptr(),
                TpdfFieldValueKind::Text as c_int,
                values.as_ptr(),
                1,
            )
        },
        TpdfStatus::Ok
    );
    let mut buffer = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_form_data_to_xfdf(handle, &mut buffer) },
        TpdfStatus::FormDataRefused
    );
    assert!(last_error().contains("bell"), "{}", last_error());
    assert!(buffer.is_null());
    unsafe { tpdf_form_data_free(handle) };
}

#[test]
fn null_and_out_of_range_are_refused_not_dereferenced() {
    let mut out = ptr::null_mut();
    let mut buffer = ptr::null_mut();
    let mut kind = TpdfFieldValueKind::None;
    let name = c("x");
    assert_eq!(
        unsafe { tpdf_document_form_data(ptr::null(), &mut out) },
        TpdfStatus::BadArgument
    );
    assert_eq!(
        unsafe { tpdf_form_data_new(ptr::null_mut()) },
        TpdfStatus::BadArgument
    );
    assert_eq!(
        unsafe { tpdf_form_data_read_fdf(ptr::null(), 0, &mut out) },
        TpdfStatus::BadArgument
    );
    assert_eq!(
        unsafe { tpdf_form_data_read_xfdf(ptr::null(), 0, &mut out) },
        TpdfStatus::BadArgument
    );
    assert_eq!(
        unsafe {
            tpdf_form_data_add_field(
                ptr::null_mut(),
                name.as_ptr(),
                kind as c_int,
                ptr::null(),
                0,
            )
        },
        TpdfStatus::BadArgument
    );
    assert_eq!(
        unsafe { tpdf_form_data_set_source(ptr::null_mut(), ptr::null()) },
        TpdfStatus::BadArgument
    );
    assert_eq!(
        unsafe { tpdf_form_data_to_fdf(ptr::null(), &mut buffer) },
        TpdfStatus::BadArgument
    );
    assert_eq!(
        unsafe { tpdf_form_data_to_xfdf(ptr::null(), &mut buffer) },
        TpdfStatus::BadArgument
    );
    assert_eq!(unsafe { tpdf_form_data_count(ptr::null()) }, 0);
    assert_eq!(unsafe { tpdf_form_data_warning_count(ptr::null()) }, 0);
    assert_eq!(
        unsafe { tpdf_form_data_field_value_count(ptr::null(), 0) },
        0
    );
    let mut text = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_form_data_field_name(ptr::null(), 0, &mut text) },
        TpdfStatus::BadArgument
    );
    assert_eq!(
        unsafe { tpdf_form_data_field_value_kind(ptr::null(), 0, &mut kind) },
        TpdfStatus::BadArgument
    );
    assert_eq!(
        unsafe { tpdf_form_data_field_value(ptr::null(), 0, 0, &mut text) },
        TpdfStatus::BadArgument
    );
    assert_eq!(
        unsafe { tpdf_form_data_source(ptr::null(), &mut text) },
        TpdfStatus::BadArgument
    );
    let mut warning = TpdfFormDataWarningKind::Unnamed;
    assert_eq!(
        unsafe { tpdf_form_data_warning(ptr::null(), 0, &mut warning, &mut text, &mut text) },
        TpdfStatus::BadArgument
    );
    assert_eq!(
        unsafe { tpdf_editor_apply_form_data(ptr::null_mut(), ptr::null(), ptr::null_mut()) },
        TpdfStatus::BadArgument
    );
    for status in [
        unsafe {
            tpdf_editor_add_text_field(
                ptr::null_mut(),
                name.as_ptr(),
                0,
                0.0,
                0.0,
                1.0,
                1.0,
                ptr::null(),
                0,
                0,
                0,
                0.0,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        },
        unsafe {
            tpdf_editor_add_checkbox(
                ptr::null_mut(),
                name.as_ptr(),
                0,
                0.0,
                0.0,
                1.0,
                1.0,
                name.as_ptr(),
                0,
                0,
                0.0,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        },
        unsafe {
            tpdf_editor_add_radio_group(
                ptr::null_mut(),
                name.as_ptr(),
                ptr::null(),
                0,
                ptr::null(),
                0,
                0.0,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        },
        unsafe {
            tpdf_editor_add_choice_field(
                ptr::null_mut(),
                name.as_ptr(),
                0,
                0.0,
                0.0,
                1.0,
                1.0,
                ptr::null(),
                0,
                0,
                0,
                ptr::null(),
                0,
                0.0,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        },
    ] {
        assert_eq!(status, TpdfStatus::BadArgument);
    }
    unsafe { tpdf_form_data_free(ptr::null_mut()) };

    // An index past the end is BadArgument, and a value string past the end
    // of a real field too.
    let fixture = form_fixture();
    let handle = document_data(&fixture);
    let past = unsafe { tpdf_form_data_count(handle) };
    assert_eq!(
        unsafe { tpdf_form_data_field_name(handle, past, &mut text) },
        TpdfStatus::BadArgument
    );
    assert_eq!(
        unsafe { tpdf_form_data_field_value(handle, 0, 99, &mut text) },
        TpdfStatus::BadArgument
    );
    unsafe { tpdf_form_data_free(handle) };
}

#[test]
fn the_forms_enums_have_the_numbers_the_bindings_transcribe() {
    assert_eq!(TpdfFieldValueKind::None as i32, 0);
    assert_eq!(TpdfFieldValueKind::Text as i32, 1);
    assert_eq!(TpdfFieldValueKind::State as i32, 2);
    assert_eq!(TpdfFieldValueKind::Many as i32, 3);
    assert_eq!(TpdfFormDataWarningKind::NotRead as i32, 0);
    assert_eq!(TpdfFormDataWarningKind::ValueUnreadable as i32, 1);
    assert_eq!(TpdfFormDataWarningKind::TreeCut as i32, 2);
    assert_eq!(TpdfFormDataWarningKind::Unnamed as i32, 3);
}
