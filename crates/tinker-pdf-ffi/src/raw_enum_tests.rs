//! Every enum a caller hands over crosses as an `int` and is checked.
//!
//! A Rust `#[repr(C)] enum` that holds a number it does not declare is
//! undefined behaviour, and the bindings over this ABI pass integers: Ruby's
//! Fiddle and Go's typed constants take any integer their caller gives them.
//! Before these were `int`s, `tpdf_document_info` with a key of 8 read a jump
//! table past its end and the process died in the C call (review of lane 7C).
//! So the claim pinned here is the one the bindings rely on: a number outside
//! an enum is [`TpdfStatus::BadArgument`] naming that enum, from every entry
//! point and struct field that takes one, exactly as a null pointer is.

use std::ffi::{c_int, CStr, CString};
use std::ptr;

use crate::*;

fn last_error() -> String {
    let pointer = unsafe { tpdf_last_error_message() };
    if pointer.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned()
}

/// Asserts a call was refused as a bad argument whose message names `name`.
fn refused_naming(status: TpdfStatus, name: &str, what: &str) {
    assert_eq!(status, TpdfStatus::BadArgument, "{what}");
    let message = last_error();
    assert!(
        message.contains(name),
        "{what}: the message names {name}: {message:?}"
    );
}

/// Each enum a caller supplies, its variants counted from zero without a gap
/// -- which is what Java's `ordinal()` and every hand-written binding's table
/// assume -- each number read back as its own variant, and every number
/// around the range refused.
macro_rules! each_reads_its_own_numbers {
    ($($name:ident),+ $(,)?) => {
        $(
            let all = $name::ALL;
            assert!(!all.is_empty(), "{}", stringify!($name));
            for (index, variant) in all.iter().enumerate() {
                let number = *variant as c_int;
                assert_eq!(
                    usize::try_from(number).ok(),
                    Some(index),
                    "{}::{variant:?} is numbered from zero without a gap",
                    stringify!($name)
                );
                assert_eq!($name::from_raw(number), Some(*variant), "{}", stringify!($name));
                assert_eq!($name::checked(number, "probe"), Ok(*variant));
            }
            let past = c_int::try_from(all.len()).expect("a handful of variants");
            for outside in [-1, past, past + 1, 255, 256, c_int::MAX, c_int::MIN] {
                assert_eq!($name::from_raw(outside), None, "{} {outside}", stringify!($name));
                refused_naming(
                    $name::checked(outside, "probe").map(|_| TpdfStatus::Ok).unwrap_or_else(|s| s),
                    stringify!($name),
                    &format!("{} {outside}", stringify!($name)),
                );
            }
        )+
    };
}

#[test]
fn every_enum_a_caller_supplies_reads_its_own_numbers_and_refuses_every_other() {
    each_reads_its_own_numbers!(
        TpdfPixelFormat,
        TpdfWriteMode,
        TpdfDestKind,
        TpdfTargetKind,
        TpdfImageKind,
        TpdfInfoKey,
        TpdfTrapped,
        TpdfLabelStyle,
        TpdfPageBoundary,
        TpdfSanitiseList,
        TpdfFieldValueKind,
        TpdfBlendMode,
        TpdfSoftMask,
        TpdfMaskKind,
        TpdfDeviceSpace,
        TpdfTilingType,
        TpdfTagText,
    );
}

fn fixture() -> Vec<u8> {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/outline-3level.pdf");
    std::fs::read(path).expect("the fixture is in the tree")
}

fn open(bytes: &[u8]) -> *mut TpdfDocument {
    let mut doc = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_document_open(bytes.as_ptr(), bytes.len(), &mut doc) },
        TpdfStatus::Ok
    );
    doc
}

/// The read side and the editor: every parameter that names an enum.
#[test]
fn the_read_and_editor_calls_refuse_a_number_their_enum_does_not_declare() {
    let bytes = fixture();
    let doc = open(&bytes);

    // The reviewer's probe: keys 0 to 7 answer, 8 crashed.
    let mut text = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_document_info(doc, TpdfInfoKey::Producer as c_int, &mut text) },
        TpdfStatus::Ok
    );
    assert!(!text.is_null(), "the fixture names its producer");
    unsafe { tpdf_string_free(text) };
    for key in [8, 9, 100, -1, c_int::MAX] {
        let mut text = ptr::null_mut();
        refused_naming(
            unsafe { tpdf_document_info(doc, key, &mut text) },
            "TpdfInfoKey",
            &format!("info({key})"),
        );
        assert!(text.is_null(), "nothing handed over for {key}");
    }

    let (mut x0, mut y0, mut x1, mut y1) = (0.0, 0.0, 0.0, 0.0);
    refused_naming(
        unsafe { tpdf_page_boundary(doc, 0, 5, &mut x0, &mut y0, &mut x1, &mut y1) },
        "TpdfPageBoundary",
        "page_boundary(5)",
    );

    let mut bitmap = ptr::null_mut();
    refused_naming(
        unsafe { tpdf_page_render(doc, 0, 1.0, 4, &mut bitmap) },
        "TpdfPixelFormat",
        "render(4)",
    );
    assert!(bitmap.is_null());

    let mut editor = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_document_editor(doc, &mut editor) },
        TpdfStatus::Ok
    );
    unsafe { tpdf_document_free(doc) };

    let value = CString::new("A title").expect("no NUL");
    refused_naming(
        unsafe { tpdf_editor_set_info(editor, 8, value.as_ptr(), ptr::null_mut()) },
        "TpdfInfoKey",
        "set_info(8)",
    );
    let date = TpdfDate {
        year: 2026,
        month: 10,
        day: 3,
        hour: 0,
        minute: 0,
        second: 0,
        has_utc_offset: 0,
        utc_offset_minutes: 0,
    };
    refused_naming(
        unsafe { tpdf_editor_set_info_date(editor, -1, &date, ptr::null_mut()) },
        "TpdfInfoKey",
        "set_info_date(-1)",
    );
    refused_naming(
        unsafe { tpdf_editor_set_trapped(editor, 4, ptr::null_mut()) },
        "TpdfTrapped",
        "set_trapped(4)",
    );
    refused_naming(
        unsafe { tpdf_editor_set_page_boundary(editor, 0, 5, 0.0, 0.0, 10.0, 10.0) },
        "TpdfPageBoundary",
        "set_page_boundary(5)",
    );
    let ranges = [TpdfPageLabelRange {
        first_page: 0,
        style: 6,
        prefix: ptr::null(),
        start: 1,
    }];
    refused_naming(
        unsafe { tpdf_editor_set_page_labels(editor, ranges.as_ptr(), ranges.len()) },
        "TpdfLabelStyle",
        "a page-label range with style 6",
    );

    let mut options = std::mem::MaybeUninit::<TpdfWriteOptions>::uninit();
    assert_eq!(
        unsafe { tpdf_write_options_init(options.as_mut_ptr()) },
        TpdfStatus::Ok
    );
    let mut options = unsafe { options.assume_init() };
    options.mode = 2;
    let mut buffer = ptr::null_mut();
    refused_naming(
        unsafe { tpdf_editor_save(editor, &options, &mut buffer) },
        "TpdfWriteMode",
        "a save with mode 2",
    );
    assert!(buffer.is_null());

    // A sanitise report: `count` names no list for a number outside the
    // enum and so answers zero, while the two accessors refuse it.
    let what = TpdfSanitise {
        javascript: 0,
        actions: 0,
        embedded_files: 0,
        metadata: 1,
    };
    let mut report = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_editor_sanitise(editor, &what, &mut report) },
        TpdfStatus::Ok
    );
    let removed = TpdfSanitiseList::Removed as c_int;
    assert!(
        unsafe { tpdf_sanitise_report_count(report, removed) } > 0,
        "the fixture's /Info is removed, so the list the probe reads past is not empty"
    );
    assert_eq!(unsafe { tpdf_sanitise_report_count(report, 2) }, 0);
    assert_eq!(unsafe { tpdf_sanitise_report_count(report, -1) }, 0);
    let mut why = TpdfRemoval::Info;
    refused_naming(
        unsafe {
            tpdf_sanitise_report_entry(
                report,
                2,
                0,
                &mut why,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        },
        "TpdfSanitiseList",
        "sanitise_report_entry(2)",
    );
    let (mut data, mut len) = (ptr::null(), 0);
    refused_naming(
        unsafe { tpdf_sanitise_report_action(report, 2, 0, &mut data, &mut len) },
        "TpdfSanitiseList",
        "sanitise_report_action(2)",
    );
    unsafe { tpdf_sanitise_report_free(report) };
    unsafe { tpdf_editor_free(editor) };
}

/// The builder, form data and tags: every parameter and struct field that
/// names an enum, and a field the call does not read left alone.
#[test]
fn the_write_calls_refuse_a_number_their_enum_does_not_declare() {
    let mut builder = ptr::null_mut();
    assert_eq!(unsafe { tpdf_builder_new(&mut builder) }, TpdfStatus::Ok);
    let name = b"R";

    let pixels = [0u8; 1];
    let image = TpdfImage {
        kind: 3,
        width: 1,
        height: 1,
        data: pixels.as_ptr(),
        data_len: pixels.len(),
    };
    refused_naming(
        unsafe { tpdf_builder_add_image(builder, name.as_ptr(), name.len(), &image) },
        "TpdfImageKind",
        "an image of kind 3",
    );

    let mut state = std::mem::MaybeUninit::<TpdfExtGState>::uninit();
    assert_eq!(
        unsafe { tpdf_ext_gstate_init(state.as_mut_ptr()) },
        TpdfStatus::Ok
    );
    let defaults = unsafe { state.assume_init() };
    for (state, enum_name) in [
        (
            TpdfExtGState {
                soft_mask: 3,
                ..defaults
            },
            "TpdfSoftMask",
        ),
        (
            TpdfExtGState {
                has_blend_mode: 1,
                blend_mode: 16,
                ..defaults
            },
            "TpdfBlendMode",
        ),
        (
            TpdfExtGState {
                soft_mask: TpdfSoftMask::Group as c_int,
                mask_kind: 2,
                ..defaults
            },
            "TpdfMaskKind",
        ),
    ] {
        refused_naming(
            unsafe { tpdf_builder_add_ext_gstate(builder, name.as_ptr(), name.len(), &state) },
            enum_name,
            &format!("{state:?}"),
        );
    }
    // A blend mode the flag says is absent, and a mask kind no group mask
    // reads, are not read, so they are not judged either.
    let unread = TpdfExtGState {
        has_blend_mode: 0,
        blend_mode: 99,
        mask_kind: 99,
        ..defaults
    };
    assert_eq!(
        unsafe { tpdf_builder_add_ext_gstate(builder, name.as_ptr(), name.len(), &unread) },
        TpdfStatus::Ok
    );

    let content = b"0 0 m";
    let group = TpdfTransparencyGroup {
        color_space: 3,
        isolated: 0,
        knockout: 0,
    };
    refused_naming(
        unsafe {
            tpdf_builder_add_form(
                builder,
                b"Fm".as_ptr(),
                2,
                0.0,
                0.0,
                10.0,
                10.0,
                ptr::null(),
                &group,
                content.as_ptr(),
                content.len(),
            )
        },
        "TpdfDeviceSpace",
        "a group in colour space 3",
    );
    refused_naming(
        unsafe {
            tpdf_builder_add_tiling_pattern(
                builder,
                b"P".as_ptr(),
                1,
                0.0,
                0.0,
                10.0,
                10.0,
                10.0,
                10.0,
                ptr::null(),
                3,
                content.as_ptr(),
                content.len(),
            )
        },
        "TpdfTilingType",
        "tiling type 3",
    );

    let mut page = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_builder_begin_page(builder, 100.0, 100.0, &mut page) },
        TpdfStatus::Ok
    );
    let mut view = std::mem::MaybeUninit::<TpdfDestination>::uninit();
    assert_eq!(
        unsafe { tpdf_destination_init_fit(view.as_mut_ptr()) },
        TpdfStatus::Ok
    );
    let fit = unsafe { view.assume_init() };
    let page_target = |kind: c_int, view: TpdfDestination| TpdfTarget {
        kind,
        page_index: 0,
        view,
        uri: ptr::null(),
    };
    refused_naming(
        unsafe { tpdf_page_builder_link(page, 0.0, 0.0, 10.0, 10.0, &page_target(2, fit)) },
        "TpdfTargetKind",
        "a link target of kind 2",
    );
    let bad_view = TpdfDestination { kind: 8, ..fit };
    refused_naming(
        unsafe {
            tpdf_page_builder_link(
                page,
                0.0,
                0.0,
                10.0,
                10.0,
                &page_target(TpdfTargetKind::Page as c_int, bad_view),
            )
        },
        "TpdfDestKind",
        "a link view of kind 8",
    );
    unsafe { tpdf_page_builder_free(page) };

    let title = CString::new("Entry").expect("no NUL");
    let mut entry = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_outline_entry_new(title.as_ptr(), &mut entry) },
        TpdfStatus::Ok
    );
    refused_naming(
        unsafe {
            tpdf_outline_entry_set_target(
                entry,
                &page_target(TpdfTargetKind::Page as c_int, bad_view),
            )
        },
        "TpdfDestKind",
        "an outline view of kind 8",
    );
    unsafe { tpdf_outline_entry_free(entry) };
    unsafe { tpdf_builder_free(builder) };

    let mut data = ptr::null_mut();
    assert_eq!(unsafe { tpdf_form_data_new(&mut data) }, TpdfStatus::Ok);
    let field = CString::new("name").expect("no NUL");
    refused_naming(
        unsafe { tpdf_form_data_add_field(data, field.as_ptr(), 4, ptr::null(), 0) },
        "TpdfFieldValueKind",
        "a field value of kind 4",
    );
    unsafe { tpdf_form_data_free(data) };

    let mut tag = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_tag_new(b"P".as_ptr(), 1, &mut tag) },
        TpdfStatus::Ok
    );
    let text = CString::new("text").expect("no NUL");
    refused_naming(
        unsafe { tpdf_tag_set_text(tag, 5, text.as_ptr()) },
        "TpdfTagText",
        "tag text 5",
    );
    unsafe { tpdf_tag_free(tag) };
}
