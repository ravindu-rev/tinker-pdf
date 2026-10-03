//! Tagged writing through the C ABI is pinned by byte equality with the
//! facade: the same elements opened and closed through both build the same
//! document, and a refused call leaves it as if it had not been made.

use super::*;
use crate::{
    tpdf_buffer_data, tpdf_buffer_free, tpdf_builder_add_base_font, tpdf_builder_begin_page,
    tpdf_builder_finish, tpdf_builder_free, tpdf_builder_new, tpdf_builder_push_page,
    tpdf_last_error_message, tpdf_page_builder_fill_rect, tpdf_page_builder_free,
    tpdf_page_builder_text, TpdfBuffer,
};
use std::ffi::{CStr, CString};
use std::ptr;
use tinker_pdf::{Document, DocumentBuilder};

fn last_error() -> String {
    let pointer = unsafe { tpdf_last_error_message() };
    if pointer.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned()
}

fn ok(status: TpdfStatus) {
    assert_eq!(status, TpdfStatus::Ok, "{}", last_error());
}

fn finish(builder: *mut TpdfBuilder) -> Vec<u8> {
    let mut buffer: *mut TpdfBuffer = ptr::null_mut();
    ok(unsafe { tpdf_builder_finish(builder, &mut buffer) });
    unsafe { tpdf_builder_free(builder) };
    let mut len = 0;
    let data = unsafe { tpdf_buffer_data(buffer, &mut len) };
    let bytes = unsafe { std::slice::from_raw_parts(data, len) }.to_vec();
    unsafe { tpdf_buffer_free(buffer) };
    bytes
}

fn begin(builder: *mut TpdfBuilder) -> *mut TpdfPageBuilder {
    let mut page = ptr::null_mut();
    ok(unsafe { tpdf_builder_begin_page(builder, 200.0, 200.0, &mut page) });
    page
}

fn push(builder: *mut TpdfBuilder, page: *mut TpdfPageBuilder) {
    ok(unsafe { tpdf_builder_push_page(builder, page) });
    unsafe { tpdf_page_builder_free(page) };
}

fn text(page: *mut TpdfPageBuilder, size: f64, y: f64, words: &str) {
    let words = CString::new(words).expect("no nul");
    ok(unsafe { tpdf_page_builder_text(page, b"F1".as_ptr(), 2, size, 20.0, y, words.as_ptr()) });
}

fn tag(kind: &[u8]) -> *mut TpdfTag {
    let mut tag = ptr::null_mut();
    ok(unsafe { tpdf_tag_new(kind.as_ptr(), kind.len(), &mut tag) });
    tag
}

fn set(tag: *mut TpdfTag, which: TpdfTagText, value: &str) {
    let value = CString::new(value).expect("no nul");
    ok(unsafe { tpdf_tag_set_text(tag, which as c_int, value.as_ptr()) });
}

/// Opens `tag` on `page` and frees the handle: the page holds its own copy.
fn open(page: *mut TpdfPageBuilder, tag: *mut TpdfTag) {
    ok(unsafe { tpdf_page_builder_open_tag(page, tag) });
    unsafe { tpdf_tag_free(tag) };
}

fn close(page: *mut TpdfPageBuilder) {
    ok(unsafe { tpdf_page_builder_close_tag(page) });
}

/// The parity script's document, through the C ABI.
fn through_the_abi() -> Vec<u8> {
    let mut builder = ptr::null_mut();
    ok(unsafe { tpdf_builder_new(&mut builder) });
    ok(unsafe { tpdf_builder_add_base_font(builder, b"F1".as_ptr(), 2, b"Helvetica".as_ptr(), 9) });
    let language = CString::new("en-GB").expect("no nul");
    ok(unsafe { tpdf_builder_set_language(builder, language.as_ptr()) });
    ok(unsafe { tpdf_builder_map_role(builder, b"Heading".as_ptr(), 7, b"H1".as_ptr(), 2) });

    let one = begin(builder);
    let heading = tag(b"Heading");
    set(heading, TpdfTagText::Title, "Introduction");
    open(one, heading);
    text(one, 14.0, 170.0, "Tagged parity");
    close(one);
    let paragraph = tag(b"P");
    set(paragraph, TpdfTagText::Lang, "fr");
    set(paragraph, TpdfTagText::ActualText, "Bonjour");
    open(one, paragraph);
    text(one, 12.0, 150.0, "Bon");
    push(builder, one);

    let two = begin(builder);
    text(two, 12.0, 170.0, "jour");
    close(two);
    assert_eq!(
        unsafe { tpdf_page_builder_close_tag(two) },
        TpdfStatus::EditRefused,
        "nothing is open"
    );
    assert!(last_error().contains("close_tag"), "{}", last_error());
    let figure = tag(b"Figure");
    set(figure, TpdfTagText::Alt, "A grey square");
    open(two, figure);
    ok(unsafe { tpdf_page_builder_fill_rect(two, 20.0, 100.0, 40.0, 40.0, 0.5) });
    close(two);
    let span = tag(b"Span");
    set(span, TpdfTagText::Expansion, "Portable Document Format");
    ok(unsafe { tpdf_tag_set_id(span, b"pdf-1".as_ptr(), 5) });
    open(two, span);
    text(two, 12.0, 80.0, "PDF");
    close(two);
    let empty = tag(b"Div");
    ok(unsafe { tpdf_tag_keep_empty(empty) });
    open(two, empty);
    close(two);
    // One handle opens both halves of the keyed element.
    let half = tag(b"P");
    ok(unsafe { tpdf_tag_set_key(half, 7, 1) });
    ok(unsafe { tpdf_page_builder_open_tag(two, half) });
    text(two, 12.0, 60.0, "read second");
    close(two);
    ok(unsafe { tpdf_tag_set_key(half, 7, 0) });
    open(two, half);
    text(two, 12.0, 40.0, "read first");
    close(two);
    push(builder, two);
    finish(builder)
}

/// The same document, against the facade.
fn against_the_facade() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    builder.set_language("en-GB");
    assert!(builder.map_role(b"Heading", b"H1"));
    let mut one = builder.begin_page(200.0, 200.0);
    assert!(one.open_tag(&Tag::new(b"Heading").title("Introduction")));
    one.text(b"F1", 14.0, 20.0, 170.0, "Tagged parity");
    assert!(one.close_tag());
    assert!(one.open_tag(&Tag::new(b"P").lang("fr").actual_text("Bonjour")));
    one.text(b"F1", 12.0, 20.0, 150.0, "Bon");
    builder.push_page(one);
    let mut two = builder.begin_page(200.0, 200.0);
    two.text(b"F1", 12.0, 20.0, 170.0, "jour");
    assert!(two.close_tag());
    assert!(!two.close_tag());
    assert!(two.open_tag(&Tag::new(b"Figure").alt("A grey square")));
    two.fill_rect(20.0, 100.0, 40.0, 40.0, 0.5);
    assert!(two.close_tag());
    assert!(two.open_tag(
        &Tag::new(b"Span")
            .expansion("Portable Document Format")
            .id(b"pdf-1")
    ));
    two.text(b"F1", 12.0, 20.0, 80.0, "PDF");
    assert!(two.close_tag());
    assert!(two.open_tag(&Tag::new(b"Div").keep_empty()));
    assert!(two.close_tag());
    assert!(two.open_tag(&Tag::new(b"P").keyed(7, 1)));
    two.text(b"F1", 12.0, 20.0, 60.0, "read second");
    assert!(two.close_tag());
    assert!(two.open_tag(&Tag::new(b"P").keyed(7, 0)));
    two.text(b"F1", 12.0, 20.0, 40.0, "read first");
    assert!(two.close_tag());
    builder.push_page(two);
    builder.finish()
}

#[test]
fn tagged_writing_builds_the_facades_bytes() {
    let ours = through_the_abi();
    assert_eq!(
        ours,
        against_the_facade(),
        "the C ABI wrote a different document"
    );
    let document = Document::open(ours).expect("the document reopens");
    assert!(document.validate().is_empty());
    let structure = document.structure().expect("the document is tagged");
    assert!(
        structure.warnings.is_empty(),
        "the tree reads back clean: {:?}",
        structure.warnings
    );
}

#[test]
fn refusals_write_nothing_and_say_why() {
    let mut builder = ptr::null_mut();
    ok(unsafe { tpdf_builder_new(&mut builder) });
    ok(unsafe { tpdf_builder_add_base_font(builder, b"F1".as_ptr(), 2, b"Helvetica".as_ptr(), 9) });
    assert_eq!(
        unsafe { tpdf_builder_map_role(builder, b"P".as_ptr(), 1, b"P".as_ptr(), 1) },
        TpdfStatus::EditRefused
    );
    assert!(last_error().contains("map_role"), "{}", last_error());
    let page = begin(builder);
    assert_eq!(
        unsafe { tpdf_page_builder_close_tag(page) },
        TpdfStatus::EditRefused
    );
    // Open until the depth refuses; every refused open owes a close that
    // closes nothing, so the closes balance either way.
    let deep = tag(b"Div");
    let mut opened = 0usize;
    let mut refusals = 0usize;
    for _ in 0..512 {
        match unsafe { tpdf_page_builder_open_tag(page, deep) } {
            TpdfStatus::Ok => opened += 1,
            TpdfStatus::EditRefused => refusals += 1,
            other => panic!("{other:?}: {}", last_error()),
        }
    }
    assert!(
        opened > 0 && refusals > 0,
        "{opened} opened, {refusals} refused"
    );
    assert!(last_error().contains("open_tag"), "{}", last_error());
    unsafe { tpdf_tag_free(deep) };
    text(page, 12.0, 100.0, "deep");
    for _ in 0..512 {
        ok(unsafe { tpdf_page_builder_close_tag(page) });
    }
    push(builder, page);

    let mut facade = DocumentBuilder::new();
    facade.add_base_font(b"F1", b"Helvetica");
    assert!(!facade.map_role(b"P", b"P"));
    let mut page = facade.begin_page(200.0, 200.0);
    for _ in 0..512 {
        let _ = page.open_tag(&Tag::new(b"Div"));
    }
    page.text(b"F1", 12.0, 20.0, 100.0, "deep");
    for _ in 0..512 {
        assert!(page.close_tag());
    }
    facade.push_page(page);
    assert_eq!(finish(builder), facade.finish());
}

#[test]
fn null_handles_are_refused_not_dereferenced() {
    let mut out = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_tag_new(ptr::null(), 0, &mut out) },
        TpdfStatus::BadArgument
    );
    assert_eq!(
        unsafe { tpdf_tag_new(b"P".as_ptr(), 1, ptr::null_mut()) },
        TpdfStatus::BadArgument
    );
    let value = CString::new("x").expect("no nul");
    assert_eq!(
        unsafe { tpdf_tag_set_text(ptr::null_mut(), TpdfTagText::Title as c_int, value.as_ptr()) },
        TpdfStatus::BadArgument
    );
    assert_eq!(
        unsafe { tpdf_tag_set_id(ptr::null_mut(), b"i".as_ptr(), 1) },
        TpdfStatus::BadArgument
    );
    assert_eq!(
        unsafe { tpdf_tag_set_key(ptr::null_mut(), 1, 0) },
        TpdfStatus::BadArgument
    );
    assert_eq!(
        unsafe { tpdf_tag_keep_empty(ptr::null_mut()) },
        TpdfStatus::BadArgument
    );
    unsafe { tpdf_tag_free(ptr::null_mut()) };
    let live = tag(b"P");
    assert_eq!(
        unsafe { tpdf_tag_set_text(live, TpdfTagText::Lang as c_int, ptr::null()) },
        TpdfStatus::BadArgument
    );
    assert_eq!(
        unsafe { tpdf_page_builder_open_tag(ptr::null_mut(), live) },
        TpdfStatus::BadArgument
    );
    unsafe { tpdf_tag_free(live) };
    assert_eq!(
        unsafe { tpdf_page_builder_close_tag(ptr::null_mut()) },
        TpdfStatus::BadArgument
    );
    assert_eq!(
        unsafe { tpdf_builder_set_language(ptr::null_mut(), value.as_ptr()) },
        TpdfStatus::BadArgument
    );
    assert_eq!(
        unsafe { tpdf_builder_map_role(ptr::null_mut(), b"A".as_ptr(), 1, b"P".as_ptr(), 1) },
        TpdfStatus::BadArgument
    );
    let mut builder = ptr::null_mut();
    ok(unsafe { tpdf_builder_new(&mut builder) });
    let page = begin(builder);
    assert_eq!(
        unsafe { tpdf_page_builder_open_tag(page, ptr::null()) },
        TpdfStatus::BadArgument
    );
    push(builder, page);
    let _ = finish(builder);
}

#[test]
fn the_tag_text_enum_has_the_numbers_the_bindings_transcribe() {
    assert_eq!(TpdfTagText::Title as i32, 0);
    assert_eq!(TpdfTagText::Lang as i32, 1);
    assert_eq!(TpdfTagText::Alt as i32, 2);
    assert_eq!(TpdfTagText::ActualText as i32, 3);
    assert_eq!(TpdfTagText::Expansion as i32, 4);
}
