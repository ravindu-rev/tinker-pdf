//! The builder's graphics resources are pinned the way the write surface is:
//! the same document built through the C ABI and through `DocumentBuilder`
//! must be **the same bytes**, and a call the facade refuses must leave the
//! document exactly as if it had not been made.

use super::*;
use crate::{
    tpdf_buffer_data, tpdf_buffer_free, tpdf_builder_add_base_font, tpdf_builder_add_image,
    tpdf_builder_begin_page, tpdf_builder_finish, tpdf_builder_free, tpdf_builder_new,
    tpdf_builder_push_page, tpdf_last_error_message, tpdf_page_builder_free, tpdf_page_builder_raw,
    TpdfBuffer, TpdfImage, TpdfImageKind,
};
use std::ffi::{CStr, CString};
use std::ptr;
use tinker_pdf::{Document, ImageData, PageBuilder};

fn last_error() -> String {
    let pointer = unsafe { tpdf_last_error_message() };
    if pointer.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned()
}

fn take_buffer(buffer: *mut TpdfBuffer) -> Vec<u8> {
    let mut len = 0;
    let data = unsafe { tpdf_buffer_data(buffer, &mut len) };
    let bytes = unsafe { std::slice::from_raw_parts(data, len) }.to_vec();
    unsafe { tpdf_buffer_free(buffer) };
    bytes
}

fn finish(builder: *mut TpdfBuilder) -> Vec<u8> {
    let mut buffer = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_builder_finish(builder, &mut buffer) },
        TpdfStatus::Ok,
        "{}",
        last_error()
    );
    unsafe { tpdf_builder_free(builder) };
    take_buffer(buffer)
}

fn begin(builder: *mut TpdfBuilder) -> *mut TpdfPageBuilder {
    let mut page = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_builder_begin_page(builder, 200.0, 200.0, &mut page) },
        TpdfStatus::Ok
    );
    page
}

fn push(builder: *mut TpdfBuilder, page: *mut TpdfPageBuilder) {
    assert_eq!(
        unsafe { tpdf_builder_push_page(builder, page) },
        TpdfStatus::Ok,
        "{}",
        last_error()
    );
    unsafe { tpdf_page_builder_free(page) };
}

fn raw(page: *mut TpdfPageBuilder, operators: &[u8]) {
    assert_eq!(
        unsafe { tpdf_page_builder_raw(page, operators.as_ptr(), operators.len()) },
        TpdfStatus::Ok
    );
}

fn ok(status: TpdfStatus) {
    assert_eq!(status, TpdfStatus::Ok, "{}", last_error());
}

const MASK_CONTENT: &[u8] = b"0.5 g 0 0 100 100 re f";
const PLAIN_CONTENT: &[u8] = b"0 0 1 rg 10 10 30 30 re f";
const CELL: &[u8] = b"1 0 0 rg 0 0 5 5 re f";
const MATRIX: [f64; 6] = [1.0, 0.0, 0.0, 1.0, 10.0, 10.0];
const PATTERN_MATRIX: [f64; 6] = [2.0, 0.0, 0.0, 2.0, 0.0, 0.0];
const BACKDROP: [f64; 1] = [0.5];
const GREY: [u8; 4] = [0, 85, 170, 255];

/// Every call of this module, made through the C ABI.
fn through_the_abi() -> Vec<u8> {
    let mut builder = ptr::null_mut();
    ok(unsafe { tpdf_builder_new_with_version(2, 0, &mut builder) });
    ok(unsafe { tpdf_builder_add_base_font(builder, b"F1".as_ptr(), 2, b"Helvetica".as_ptr(), 9) });
    let names = [
        CString::new("Euro").unwrap(),
        CString::new("uni0141").unwrap(),
    ];
    let name_pointers: Vec<*const c_char> = names.iter().map(|n| n.as_ptr()).collect();
    let widths = [556u16, 611];
    ok(unsafe {
        tpdf_builder_add_named_font(
            builder,
            b"F2".as_ptr(),
            2,
            b"Helvetica".as_ptr(),
            9,
            128,
            name_pointers.as_ptr(),
            2,
            widths.as_ptr(),
            2,
        )
    });
    let group = TpdfTransparencyGroup {
        color_space: TpdfDeviceSpace::Gray as c_int,
        isolated: 1,
        knockout: 0,
    };
    ok(unsafe {
        tpdf_builder_add_form(
            builder,
            b"Fm0".as_ptr(),
            3,
            0.0,
            0.0,
            100.0,
            100.0,
            MATRIX.as_ptr(),
            &group,
            MASK_CONTENT.as_ptr(),
            MASK_CONTENT.len(),
        )
    });
    ok(unsafe {
        tpdf_builder_add_form(
            builder,
            b"Fm1".as_ptr(),
            3,
            0.0,
            0.0,
            50.0,
            50.0,
            ptr::null(),
            ptr::null(),
            PLAIN_CONTENT.as_ptr(),
            PLAIN_CONTENT.len(),
        )
    });
    let mut state = std::mem::MaybeUninit::<TpdfExtGState>::uninit();
    ok(unsafe { tpdf_ext_gstate_init(state.as_mut_ptr()) });
    let mut state = unsafe { state.assume_init() };
    state.fill_alpha = 0.5;
    state.stroke_alpha = 0.25;
    state.has_blend_mode = 1;
    state.blend_mode = TpdfBlendMode::Multiply as c_int;
    state.soft_mask = TpdfSoftMask::Group as c_int;
    state.mask_kind = TpdfMaskKind::Luminosity as c_int;
    state.mask_form = b"Fm0".as_ptr();
    state.mask_form_len = 3;
    state.backdrop = BACKDROP.as_ptr();
    state.backdrop_len = 1;
    ok(unsafe { tpdf_builder_add_ext_gstate(builder, b"GS0".as_ptr(), 3, &state) });
    let mut off = std::mem::MaybeUninit::<TpdfExtGState>::uninit();
    ok(unsafe { tpdf_ext_gstate_init(off.as_mut_ptr()) });
    let mut off = unsafe { off.assume_init() };
    off.soft_mask = TpdfSoftMask::None as c_int;
    ok(unsafe { tpdf_builder_add_ext_gstate(builder, b"GS1".as_ptr(), 3, &off) });
    ok(unsafe {
        tpdf_builder_add_tiling_pattern(
            builder,
            b"P0".as_ptr(),
            2,
            0.0,
            0.0,
            5.0,
            5.0,
            8.0,
            8.0,
            PATTERN_MATRIX.as_ptr(),
            TpdfTilingType::NoDistortion as c_int,
            CELL.as_ptr(),
            CELL.len(),
        )
    });
    let image = TpdfImage {
        kind: TpdfImageKind::Gray8 as c_int,
        width: 2,
        height: 2,
        data: GREY.as_ptr(),
        data_len: GREY.len(),
    };
    ok(unsafe { tpdf_builder_add_image(builder, b"Im1".as_ptr(), 3, &image) });

    let page = begin(builder);
    ok(unsafe { tpdf_page_builder_set_bleed_box(page, 5.0, 5.0, 195.0, 195.0) });
    let characters = CString::new("\u{20ac}\u{141}").unwrap();
    let codes = [128u8, 129];
    ok(unsafe {
        tpdf_page_builder_encoded_text(
            page,
            b"F2".as_ptr(),
            2,
            12.0,
            20.0,
            170.0,
            0.5,
            1.5,
            codes.as_ptr(),
            2,
            characters.as_ptr(),
        )
    });
    raw(page, b"q");
    ok(unsafe { tpdf_page_builder_set_ext_gstate(page, b"GS0".as_ptr(), 3) });
    ok(unsafe { tpdf_page_builder_form(page, b"Fm1".as_ptr(), 3) });
    ok(unsafe { tpdf_page_builder_set_fill_pattern(page, b"P0".as_ptr(), 2) });
    raw(page, b"60 60 40 40 re f");
    ok(unsafe { tpdf_page_builder_set_stroke_pattern(page, b"P0".as_ptr(), 2) });
    raw(page, b"4 w 110 110 40 40 re S");
    ok(unsafe { tpdf_page_builder_set_ext_gstate(page, b"GS1".as_ptr(), 3) });
    raw(page, b"Q");
    ok(unsafe {
        crate::tpdf_page_builder_image(page, b"Im1".as_ptr(), 3, 150.0, 20.0, 20.0, 20.0)
    });
    push(builder, page);
    ok(unsafe { tpdf_builder_clear_image_resources(builder) });
    let page = begin(builder);
    ok(unsafe { tpdf_page_builder_form(page, b"Fm0".as_ptr(), 3) });
    push(builder, page);
    finish(builder)
}

/// The same document, against the facade.
fn against_the_facade() -> Vec<u8> {
    let mut builder = DocumentBuilder::with_version(2, 0);
    builder.add_base_font(b"F1", b"Helvetica");
    assert!(builder.add_named_font(b"F2", b"Helvetica", 128, &["Euro", "uni0141"], &[556, 611]));
    assert!(builder.add_form(
        b"Fm0",
        &FormXObject {
            bbox: [0.0, 0.0, 100.0, 100.0],
            matrix: Some(MATRIX),
            group: Some(TransparencyGroup {
                color_space: DeviceSpace::Gray,
                isolated: true,
                knockout: false,
            }),
            content: MASK_CONTENT,
        }
    ));
    assert!(builder.add_form(
        b"Fm1",
        &FormXObject {
            bbox: [0.0, 0.0, 50.0, 50.0],
            matrix: None,
            group: None,
            content: PLAIN_CONTENT,
        }
    ));
    assert!(builder.add_ext_gstate(
        b"GS0",
        &ExtGState {
            fill_alpha: Some(0.5),
            stroke_alpha: Some(0.25),
            blend_mode: Some(BlendMode::Multiply),
            soft_mask: Some(StateMask::Group {
                kind: MaskKind::Luminosity,
                form: b"Fm0",
                backdrop: Some(&BACKDROP),
            }),
        }
    ));
    assert!(builder.add_ext_gstate(
        b"GS1",
        &ExtGState {
            soft_mask: Some(StateMask::None),
            ..ExtGState::default()
        }
    ));
    assert!(builder.add_tiling_pattern(
        b"P0",
        &TilingPattern {
            bbox: [0.0, 0.0, 5.0, 5.0],
            x_step: 8.0,
            y_step: 8.0,
            matrix: Some(PATTERN_MATRIX),
            tiling_type: TilingType::NoDistortion,
            content: CELL,
        }
    ));
    assert!(builder.add_image(
        b"Im1",
        &ImageData::Gray8 {
            width: 2,
            height: 2,
            data: &GREY,
        }
    ));
    let mut page: PageBuilder = builder.begin_page(200.0, 200.0);
    page.set_bleed_box(5.0, 5.0, 195.0, 195.0);
    page.encoded_text(
        b"F2",
        12.0,
        20.0,
        170.0,
        (0.5, 1.5),
        &[128, 129],
        "\u{20ac}\u{141}",
    );
    page.raw(b"q");
    assert!(page.set_ext_gstate(b"GS0"));
    assert!(page.form(b"Fm1"));
    assert!(page.set_fill_pattern(b"P0"));
    page.raw(b"60 60 40 40 re f");
    assert!(page.set_stroke_pattern(b"P0"));
    page.raw(b"4 w 110 110 40 40 re S");
    assert!(page.set_ext_gstate(b"GS1"));
    page.raw(b"Q");
    page.image(b"Im1", 150.0, 20.0, 20.0, 20.0);
    builder.push_page(page);
    builder.clear_image_resources();
    let mut page = builder.begin_page(200.0, 200.0);
    assert!(page.form(b"Fm0"));
    builder.push_page(page);
    builder.finish()
}

#[test]
fn every_graphics_call_builds_the_facades_bytes() {
    let ours = through_the_abi();
    assert_eq!(
        ours,
        against_the_facade(),
        "the C ABI wrote a different document"
    );
    let document = Document::open(ours).expect("the document reopens");
    assert!(
        document.validate().is_empty(),
        "{:?}",
        document
            .validate()
            .iter()
            .map(|d| d.kind.as_str())
            .collect::<Vec<_>>()
    );
    assert_eq!(document.pdf_version(), "PDF 2.0");
    assert_eq!(document.page_count(), 2);
}

#[test]
fn the_initialised_state_is_the_facades_default() {
    let mut state = std::mem::MaybeUninit::<TpdfExtGState>::uninit();
    ok(unsafe { tpdf_ext_gstate_init(state.as_mut_ptr()) });
    let state = unsafe { state.assume_init() };
    assert!(state.fill_alpha.is_nan() && state.stroke_alpha.is_nan());
    assert_eq!(state.has_blend_mode, 0);
    assert_eq!(state.soft_mask, TpdfSoftMask::Absent as c_int);
    assert!(state.mask_form.is_null() && state.backdrop.is_null());

    let mut builder = ptr::null_mut();
    ok(unsafe { tpdf_builder_new(&mut builder) });
    ok(unsafe { tpdf_builder_add_ext_gstate(builder, b"GS".as_ptr(), 2, &state) });
    let page = begin(builder);
    ok(unsafe { tpdf_page_builder_set_ext_gstate(page, b"GS".as_ptr(), 2) });
    push(builder, page);

    let mut facade = DocumentBuilder::new();
    assert!(facade.add_ext_gstate(b"GS", &ExtGState::default()));
    let mut page = facade.begin_page(200.0, 200.0);
    assert!(page.set_ext_gstate(b"GS"));
    facade.push_page(page);
    assert_eq!(finish(builder), facade.finish());
}

#[test]
fn a_refused_call_registers_nothing_and_says_why() {
    let mut builder = ptr::null_mut();
    ok(unsafe { tpdf_builder_new(&mut builder) });
    let mut version = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_builder_new_with_version(256, 0, &mut version) },
        TpdfStatus::BadArgument
    );
    assert!(version.is_null());

    // Two names and one width.
    let name = CString::new("Euro").unwrap();
    let names = [name.as_ptr(), name.as_ptr()];
    let widths = [556u16];
    assert_eq!(
        unsafe {
            tpdf_builder_add_named_font(
                builder,
                b"F2".as_ptr(),
                2,
                b"Helvetica".as_ptr(),
                9,
                0,
                names.as_ptr(),
                2,
                widths.as_ptr(),
                1,
            )
        },
        TpdfStatus::EditRefused
    );
    assert!(last_error().contains("add_named_font"), "{}", last_error());
    assert_eq!(
        unsafe {
            tpdf_builder_add_named_font(
                builder,
                b"F2".as_ptr(),
                2,
                b"Helvetica".as_ptr(),
                9,
                256,
                names.as_ptr(),
                1,
                widths.as_ptr(),
                1,
            )
        },
        TpdfStatus::BadArgument
    );

    // An alpha past one; a mask over a form that is not a group.
    let mut state = std::mem::MaybeUninit::<TpdfExtGState>::uninit();
    ok(unsafe { tpdf_ext_gstate_init(state.as_mut_ptr()) });
    let mut state = unsafe { state.assume_init() };
    state.fill_alpha = 2.0;
    assert_eq!(
        unsafe { tpdf_builder_add_ext_gstate(builder, b"GS".as_ptr(), 2, &state) },
        TpdfStatus::EditRefused
    );
    assert!(last_error().contains("add_ext_gstate"), "{}", last_error());
    ok(unsafe {
        tpdf_builder_add_form(
            builder,
            b"Fm".as_ptr(),
            2,
            0.0,
            0.0,
            10.0,
            10.0,
            ptr::null(),
            ptr::null(),
            PLAIN_CONTENT.as_ptr(),
            PLAIN_CONTENT.len(),
        )
    });
    state.fill_alpha = f64::NAN;
    state.soft_mask = TpdfSoftMask::Group as c_int;
    state.mask_form = b"Fm".as_ptr();
    state.mask_form_len = 2;
    assert_eq!(
        unsafe { tpdf_builder_add_ext_gstate(builder, b"GS".as_ptr(), 2, &state) },
        TpdfStatus::EditRefused
    );

    // A degenerate box; a zero step.
    assert_eq!(
        unsafe {
            tpdf_builder_add_form(
                builder,
                b"Bad".as_ptr(),
                3,
                0.0,
                0.0,
                0.0,
                10.0,
                ptr::null(),
                ptr::null(),
                PLAIN_CONTENT.as_ptr(),
                PLAIN_CONTENT.len(),
            )
        },
        TpdfStatus::EditRefused
    );
    assert!(last_error().contains("add_form"), "{}", last_error());
    assert_eq!(
        unsafe {
            tpdf_builder_add_tiling_pattern(
                builder,
                b"P".as_ptr(),
                1,
                0.0,
                0.0,
                5.0,
                5.0,
                0.0,
                8.0,
                ptr::null(),
                TpdfTilingType::ConstantSpacing as c_int,
                CELL.as_ptr(),
                CELL.len(),
            )
        },
        TpdfStatus::EditRefused
    );
    assert!(
        last_error().contains("add_tiling_pattern"),
        "{}",
        last_error()
    );

    // Names nothing registered: each page call refused, writing nothing.
    let page = begin(builder);
    for (call, status) in [
        ("set_ext_gstate", unsafe {
            tpdf_page_builder_set_ext_gstate(page, b"GS".as_ptr(), 2)
        }),
        ("form", unsafe {
            tpdf_page_builder_form(page, b"Bad".as_ptr(), 3)
        }),
        ("set_fill_pattern", unsafe {
            tpdf_page_builder_set_fill_pattern(page, b"P".as_ptr(), 1)
        }),
        ("set_stroke_pattern", unsafe {
            tpdf_page_builder_set_stroke_pattern(page, b"P".as_ptr(), 1)
        }),
    ] {
        assert_eq!(status, TpdfStatus::EditRefused, "{call}");
    }
    assert!(
        last_error().contains("set_stroke_pattern"),
        "{}",
        last_error()
    );
    push(builder, page);

    // The document is what the calls that were taken make, and no more.
    let mut facade = DocumentBuilder::new();
    assert!(facade.add_form(
        b"Fm",
        &FormXObject {
            bbox: [0.0, 0.0, 10.0, 10.0],
            matrix: None,
            group: None,
            content: PLAIN_CONTENT,
        }
    ));
    let page = facade.begin_page(200.0, 200.0);
    facade.push_page(page);
    assert_eq!(finish(builder), facade.finish());
}

#[test]
fn null_and_spent_handles_are_refused_not_dereferenced() {
    let null_builder = ptr::null_mut();
    let null_page = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_builder_new_with_version(1, 7, ptr::null_mut()) },
        TpdfStatus::BadArgument
    );
    assert_eq!(
        unsafe { tpdf_builder_clear_image_resources(null_builder) },
        TpdfStatus::BadArgument
    );
    assert_eq!(
        unsafe {
            tpdf_builder_add_named_font(
                null_builder,
                b"F".as_ptr(),
                1,
                b"Helvetica".as_ptr(),
                9,
                0,
                ptr::null(),
                0,
                ptr::null(),
                0,
            )
        },
        TpdfStatus::BadArgument
    );
    assert_eq!(
        unsafe { tpdf_ext_gstate_init(ptr::null_mut()) },
        TpdfStatus::BadArgument
    );
    assert_eq!(
        unsafe { tpdf_builder_add_ext_gstate(null_builder, b"G".as_ptr(), 1, ptr::null()) },
        TpdfStatus::BadArgument
    );
    assert_eq!(
        unsafe {
            tpdf_builder_add_form(
                null_builder,
                b"F".as_ptr(),
                1,
                0.0,
                0.0,
                1.0,
                1.0,
                ptr::null(),
                ptr::null(),
                CELL.as_ptr(),
                CELL.len(),
            )
        },
        TpdfStatus::BadArgument
    );
    assert_eq!(
        unsafe {
            tpdf_builder_add_tiling_pattern(
                null_builder,
                b"P".as_ptr(),
                1,
                0.0,
                0.0,
                1.0,
                1.0,
                1.0,
                1.0,
                ptr::null(),
                TpdfTilingType::ConstantSpacing as c_int,
                CELL.as_ptr(),
                CELL.len(),
            )
        },
        TpdfStatus::BadArgument
    );
    assert_eq!(
        unsafe { tpdf_page_builder_set_bleed_box(null_page, 0.0, 0.0, 1.0, 1.0) },
        TpdfStatus::BadArgument
    );
    let characters = CString::new("a").unwrap();
    assert_eq!(
        unsafe {
            tpdf_page_builder_encoded_text(
                null_page,
                b"F".as_ptr(),
                1,
                12.0,
                0.0,
                0.0,
                0.0,
                0.0,
                b"a".as_ptr(),
                1,
                characters.as_ptr(),
            )
        },
        TpdfStatus::BadArgument
    );
    assert_eq!(
        unsafe { tpdf_page_builder_set_ext_gstate(null_page, b"G".as_ptr(), 1) },
        TpdfStatus::BadArgument
    );
    assert_eq!(
        unsafe { tpdf_page_builder_form(null_page, b"F".as_ptr(), 1) },
        TpdfStatus::BadArgument
    );
    assert_eq!(
        unsafe { tpdf_page_builder_set_fill_pattern(null_page, b"P".as_ptr(), 1) },
        TpdfStatus::BadArgument
    );
    assert_eq!(
        unsafe { tpdf_page_builder_set_stroke_pattern(null_page, b"P".as_ptr(), 1) },
        TpdfStatus::BadArgument
    );

    // A null resource name and a null state on a live handle.
    let mut builder = ptr::null_mut();
    ok(unsafe { tpdf_builder_new(&mut builder) });
    assert_eq!(
        unsafe { tpdf_builder_add_ext_gstate(builder, b"G".as_ptr(), 1, ptr::null()) },
        TpdfStatus::BadArgument
    );
    let page = begin(builder);
    assert_eq!(
        unsafe { tpdf_page_builder_form(page, ptr::null(), 0) },
        TpdfStatus::BadArgument
    );
    push(builder, page);

    // A finished builder is spent, and says which call spent it.
    let _ = finish_keeping(builder);
    assert_eq!(
        unsafe { tpdf_builder_clear_image_resources(builder) },
        TpdfStatus::SpentHandle
    );
    assert!(last_error().contains("finish"), "{}", last_error());
    unsafe { tpdf_builder_free(builder) };
}

/// Finishes without freeing the handle, for the spent-handle leg.
fn finish_keeping(builder: *mut TpdfBuilder) -> Vec<u8> {
    let mut buffer = ptr::null_mut();
    ok(unsafe { tpdf_builder_finish(builder, &mut buffer) });
    take_buffer(buffer)
}

/// The numbers the hand-written bindings transcribe.
#[test]
fn the_graphics_enums_have_the_numbers_the_bindings_transcribe() {
    let blend = [
        (TpdfBlendMode::Normal, 0),
        (TpdfBlendMode::Multiply, 1),
        (TpdfBlendMode::Screen, 2),
        (TpdfBlendMode::Overlay, 3),
        (TpdfBlendMode::Darken, 4),
        (TpdfBlendMode::Lighten, 5),
        (TpdfBlendMode::ColorDodge, 6),
        (TpdfBlendMode::ColorBurn, 7),
        (TpdfBlendMode::HardLight, 8),
        (TpdfBlendMode::SoftLight, 9),
        (TpdfBlendMode::Difference, 10),
        (TpdfBlendMode::Exclusion, 11),
        (TpdfBlendMode::Hue, 12),
        (TpdfBlendMode::Saturation, 13),
        (TpdfBlendMode::Color, 14),
        (TpdfBlendMode::Luminosity, 15),
    ];
    for (mode, number) in blend {
        assert_eq!(mode as i32, number, "{mode:?}");
        // And each crosses to the facade's arm of the same name.
        assert_eq!(format!("{:?}", super::blend(mode)), format!("{mode:?}"));
    }
    assert_eq!(TpdfSoftMask::Absent as i32, 0);
    assert_eq!(TpdfSoftMask::None as i32, 1);
    assert_eq!(TpdfSoftMask::Group as i32, 2);
    assert_eq!(TpdfMaskKind::Alpha as i32, 0);
    assert_eq!(TpdfMaskKind::Luminosity as i32, 1);
    assert_eq!(TpdfDeviceSpace::Gray as i32, 0);
    assert_eq!(TpdfDeviceSpace::Rgb as i32, 1);
    assert_eq!(TpdfDeviceSpace::Cmyk as i32, 2);
    assert_eq!(TpdfTilingType::ConstantSpacing as i32, 0);
    assert_eq!(TpdfTilingType::NoDistortion as i32, 1);
    assert_eq!(TpdfTilingType::FasterTiling as i32, 2);
}
