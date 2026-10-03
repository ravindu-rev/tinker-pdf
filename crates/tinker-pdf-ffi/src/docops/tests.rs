//! The document operations are pinned the way the write surface is: the
//! same edits made through the C ABI and through `DocumentEditor` directly
//! must save **the same bytes**, and a sanitise must report the same entries.

use super::*;
use crate::{
    tpdf_buffer_data, tpdf_buffer_free, tpdf_document_editor, tpdf_document_free,
    tpdf_document_open, tpdf_editor_free, tpdf_editor_save, tpdf_last_error_message,
    tpdf_outline_entry_free, tpdf_outline_entry_new, tpdf_outline_entry_set_target,
    tpdf_write_options_init, TpdfDestKind, TpdfDestination, TpdfTarget, TpdfTargetKind,
    TpdfWriteOptions,
};
use std::ffi::{CStr, CString};
use tinker_pdf::{DestKind, Document, OutlineEntry, Target, WriteOptions};

fn outline_fixture() -> Vec<u8> {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/outline-3level.pdf");
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
    let mut len = 0;
    let data = unsafe { tpdf_buffer_data(buffer, &mut len) };
    let bytes = unsafe { std::slice::from_raw_parts(data, len) }.to_vec();
    unsafe { tpdf_buffer_free(buffer) };
    bytes
}

const CREATED: TpdfDate = TpdfDate {
    year: 2026,
    month: 10,
    day: 3,
    hour: 12,
    minute: 0,
    second: 0,
    has_utc_offset: 1,
    utc_offset_minutes: 0,
};

fn created() -> Date {
    Date {
        year: 2026,
        month: 10,
        day: 3,
        hour: 12,
        minute: 0,
        second: 0,
        utc_offset_minutes: Some(0),
    }
}

const PACKET: &[u8] = b"<x:xmpmeta xmlns:x='adobe:ns:meta/'/>";

/// Every operation, through the C ABI.
fn operate_through_the_abi(bytes: &[u8]) -> Vec<u8> {
    let editor = editor_over(bytes);
    let prefix = CString::new("A-").expect("no nul");
    let ranges = [
        TpdfPageLabelRange {
            first_page: 0,
            style: TpdfLabelStyle::RomanLower as c_int,
            prefix: ptr::null(),
            start: 1,
        },
        TpdfPageLabelRange {
            first_page: 2,
            style: TpdfLabelStyle::Decimal as c_int,
            prefix: prefix.as_ptr(),
            start: 1,
        },
    ];
    assert_eq!(
        unsafe { tpdf_editor_set_page_labels(editor, ranges.as_ptr(), ranges.len()) },
        TpdfStatus::Ok
    );

    let name = CString::new("data.csv").expect("no nul");
    let description = CString::new("the numbers").expect("no nul");
    let mime = CString::new("text/csv").expect("no nul");
    let data = b"a,b\n1,2\n";
    let file = TpdfEmbeddedFile {
        name: name.as_ptr(),
        filename: name.as_ptr(),
        description: description.as_ptr(),
        mime_type: mime.as_ptr(),
        created: &CREATED,
        modified: ptr::null(),
        data: data.as_ptr(),
        data_len: data.len(),
    };
    let (mut num, mut gen) = (0, 9);
    assert_eq!(
        unsafe { tpdf_editor_attach_file(editor, &file, &mut num, &mut gen) },
        TpdfStatus::Ok
    );
    assert!(
        num > 0 && gen == 0,
        "the file specification is a new object"
    );

    let title = CString::new("Document operations").expect("no nul");
    let author = CString::new("tinker-pdf").expect("no nul");
    let mut sync = TpdfMetadataSync::OtherHalfUnchanged;
    assert_eq!(
        unsafe {
            tpdf_editor_set_info(
                editor,
                TpdfInfoKey::Title as c_int,
                title.as_ptr(),
                &mut sync,
            )
        },
        TpdfStatus::Ok
    );
    assert_eq!(sync, TpdfMetadataSync::Alone, "no XMP packet yet");
    assert_eq!(
        unsafe {
            tpdf_editor_set_info(
                editor,
                TpdfInfoKey::Author as c_int,
                author.as_ptr(),
                ptr::null_mut(),
            )
        },
        TpdfStatus::Ok
    );
    assert_eq!(
        unsafe {
            tpdf_editor_set_info_date(
                editor,
                TpdfInfoKey::CreationDate as c_int,
                &CREATED,
                ptr::null_mut(),
            )
        },
        TpdfStatus::Ok
    );
    assert_eq!(
        unsafe { tpdf_editor_set_trapped(editor, TpdfTrapped::False as c_int, ptr::null_mut()) },
        TpdfStatus::Ok
    );
    assert_eq!(
        unsafe { tpdf_editor_set_xmp_metadata(editor, PACKET.as_ptr(), PACKET.len(), &mut sync) },
        TpdfStatus::Ok
    );
    assert_eq!(
        sync,
        TpdfMetadataSync::OtherHalfUnchanged,
        "/Info now has entries the packet was not checked against"
    );
    assert_eq!(
        unsafe {
            tpdf_editor_set_page_boundary(
                editor,
                0,
                TpdfPageBoundary::TrimBox as c_int,
                10.0,
                10.0,
                585.0,
                832.0,
            )
        },
        TpdfStatus::Ok
    );
    assert_eq!(
        unsafe {
            tpdf_editor_set_page_boundary(
                editor,
                1,
                TpdfPageBoundary::BleedBox as c_int,
                0.0,
                0.0,
                595.0,
                842.0,
            )
        },
        TpdfStatus::Ok
    );

    let entry_title = CString::new("Only entry").expect("no nul");
    let mut entry = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_outline_entry_new(entry_title.as_ptr(), &mut entry) },
        TpdfStatus::Ok
    );
    let target = TpdfTarget {
        kind: TpdfTargetKind::Page as c_int,
        page_index: 3,
        view: TpdfDestination {
            kind: TpdfDestKind::FitH as c_int,
            left: f64::NAN,
            bottom: f64::NAN,
            right: f64::NAN,
            top: 700.0,
            zoom: f64::NAN,
        },
        uri: ptr::null(),
    };
    assert_eq!(
        unsafe { tpdf_outline_entry_set_target(entry, &target) },
        TpdfStatus::Ok
    );
    let entries = [entry];
    assert_eq!(
        unsafe { tpdf_editor_set_outline(editor, entries.as_ptr(), 1) },
        TpdfStatus::Ok
    );
    assert_eq!(
        unsafe { tpdf_editor_set_outline(editor, entries.as_ptr(), 1) },
        TpdfStatus::SpentHandle,
        "the entry was consumed by the first call"
    );
    unsafe { tpdf_outline_entry_free(entry) };

    let saved = save(editor);
    unsafe { tpdf_editor_free(editor) };
    saved
}

/// The same operations, against the facade.
fn operate_through_the_facade(bytes: &[u8]) -> Vec<u8> {
    let document = Document::open(bytes.to_vec()).expect("opens");
    let mut editor = document.editor();
    editor
        .set_page_labels(&[
            PageLabelRange {
                first_page: 0,
                style: LabelStyle::RomanLower,
                prefix: None,
                start: 1,
            },
            PageLabelRange {
                first_page: 2,
                style: LabelStyle::Decimal,
                prefix: Some("A-".to_string()),
                start: 1,
            },
        ])
        .expect("labels");
    editor
        .attach_file(&EmbeddedFile {
            name: "data.csv".to_string(),
            filename: "data.csv".to_string(),
            description: Some("the numbers".to_string()),
            mime_type: Some("text/csv".to_string()),
            created: Some(created()),
            modified: None,
            data: b"a,b\n1,2\n".to_vec(),
        })
        .expect("attached");
    let _ = editor.set_title("Document operations");
    let _ = editor.set_author("tinker-pdf");
    let _ = editor.set_creation_date(created());
    let _ = editor.set_trapped(Trapped::False);
    let _ = editor.set_xmp_metadata(PACKET);
    assert!(editor.set_page_boundary(0, PageBoundary::TrimBox, 10.0, 10.0, 585.0, 832.0));
    assert!(editor.set_page_boundary(1, PageBoundary::BleedBox, 0.0, 0.0, 595.0, 842.0));
    assert!(editor.set_outline(&[OutlineEntry {
        title: "Only entry".to_string(),
        target: Some(Target::Page {
            index: 3,
            view: DestKind::FitH { top: Some(700.0) },
        }),
        open: false,
        children: Vec::new(),
    }]));
    editor.save(&WriteOptions::default())
}

#[test]
fn the_document_operations_through_the_abi_are_byte_equal_to_the_facade() {
    let fixture = outline_fixture();
    let abi = operate_through_the_abi(&fixture);
    let facade = operate_through_the_facade(&fixture);
    assert_eq!(
        abi, facade,
        "the C ABI added, dropped or reordered something"
    );

    // And the operations did what they say, read back through the facade:
    // the equality above is about something rather than about two no-ops.
    let document = Document::open(abi).expect("the saved document opens");
    assert!(document.validate().is_empty(), "{:?}", document.validate());
    assert_eq!(document.page_labels()[..3], ["i", "ii", "A-1"]);
    assert_eq!(document.attachments().len(), 1);
    assert_eq!(
        document.metadata().title.as_deref(),
        Some("Document operations")
    );
    assert_eq!(document.metadata().trapped, Some(Trapped::False));
    assert_eq!(document.xmp_metadata().as_deref(), Some(PACKET));
    assert_eq!(
        document.page(0).expect("page 0").trim_box(),
        (10.0, 10.0, 585.0, 832.0)
    );
    assert_eq!(document.outline().len(), 1);
}

#[test]
fn a_page_boundary_reads_back_through_the_abi() {
    let saved = operate_through_the_abi(&outline_fixture());
    let facade = Document::open(saved.clone()).expect("opens");
    let mut doc = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_document_open(saved.as_ptr(), saved.len(), &mut doc) },
        TpdfStatus::Ok
    );
    for index in 0..2 {
        for boundary in [
            TpdfPageBoundary::MediaBox,
            TpdfPageBoundary::CropBox,
            TpdfPageBoundary::BleedBox,
            TpdfPageBoundary::TrimBox,
            TpdfPageBoundary::ArtBox,
        ] {
            let (mut x0, mut y0, mut x1, mut y1) = (0.0, 0.0, 0.0, 0.0);
            assert_eq!(
                unsafe {
                    tpdf_page_boundary(
                        doc,
                        index,
                        boundary as c_int,
                        &mut x0,
                        &mut y0,
                        &mut x1,
                        &mut y1,
                    )
                },
                TpdfStatus::Ok
            );
            let expected = facade
                .page(index)
                .expect("page")
                .boundary(boundary.to_facade());
            assert_eq!((x0, y0, x1, y1), expected, "page {index} {boundary:?}");
        }
    }
    assert_eq!(
        unsafe {
            tpdf_page_boundary(
                doc,
                99,
                TpdfPageBoundary::MediaBox as c_int,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        },
        TpdfStatus::NoSuchPage
    );
    unsafe { tpdf_document_free(doc) };
}

#[test]
fn each_refusal_writes_nothing_and_says_why() {
    let fixture = outline_fixture();
    let editor = editor_over(&fixture);

    // 12.4.2: the tree shall include page 0.
    let ranges = [TpdfPageLabelRange {
        first_page: 1,
        style: TpdfLabelStyle::Decimal as c_int,
        prefix: ptr::null(),
        start: 1,
    }];
    assert_eq!(
        unsafe { tpdf_editor_set_page_labels(editor, ranges.as_ptr(), 1) },
        TpdfStatus::EditRefused
    );
    assert!(
        last_error().contains("page 0"),
        "the facade's own reason crosses: {}",
        last_error()
    );

    let name = CString::new("twice").expect("no nul");
    let file = TpdfEmbeddedFile {
        name: name.as_ptr(),
        filename: name.as_ptr(),
        description: ptr::null(),
        mime_type: ptr::null(),
        created: ptr::null(),
        modified: ptr::null(),
        data: ptr::null(),
        data_len: 0,
    };
    assert_eq!(
        unsafe { tpdf_editor_attach_file(editor, &file, ptr::null_mut(), ptr::null_mut()) },
        TpdfStatus::Ok,
        "an empty file is a file"
    );
    assert_eq!(
        unsafe { tpdf_editor_attach_file(editor, &file, ptr::null_mut(), ptr::null_mut()) },
        TpdfStatus::EditRefused
    );
    assert!(
        last_error().contains("already attached"),
        "{}",
        last_error()
    );

    let bad_month = TpdfDate {
        month: 300,
        ..CREATED
    };
    assert_eq!(
        unsafe {
            tpdf_editor_set_info_date(
                editor,
                TpdfInfoKey::ModificationDate as c_int,
                &bad_month,
                ptr::null_mut(),
            )
        },
        TpdfStatus::BadArgument,
        "a month that does not fit a byte is refused, not truncated"
    );
    let value = CString::new("x").expect("no nul");
    assert_eq!(
        unsafe {
            tpdf_editor_set_info(
                editor,
                TpdfInfoKey::CreationDate as c_int,
                value.as_ptr(),
                ptr::null_mut(),
            )
        },
        TpdfStatus::BadArgument
    );
    assert_eq!(
        unsafe {
            tpdf_editor_set_info_date(
                editor,
                TpdfInfoKey::Title as c_int,
                &CREATED,
                ptr::null_mut(),
            )
        },
        TpdfStatus::BadArgument
    );
    assert_eq!(
        unsafe { tpdf_editor_set_trapped(editor, TpdfTrapped::Absent as c_int, ptr::null_mut()) },
        TpdfStatus::BadArgument
    );
    assert_eq!(
        unsafe {
            tpdf_editor_set_page_boundary(
                editor,
                0,
                TpdfPageBoundary::ArtBox as c_int,
                5.0,
                5.0,
                5.0,
                9.0,
            )
        },
        TpdfStatus::EditRefused,
        "a rectangle with no area"
    );
    assert!(
        last_error().contains("set_page_boundary"),
        "{}",
        last_error()
    );
    assert_eq!(
        unsafe {
            tpdf_editor_set_page_boundary(
                editor,
                99,
                TpdfPageBoundary::ArtBox as c_int,
                0.0,
                0.0,
                9.0,
                9.0,
            )
        },
        TpdfStatus::EditRefused
    );
    unsafe { tpdf_editor_free(editor) };
}

/// A document with something for every sanitise flag to take out: a
/// JavaScript open action, a URI link, an attachment and `/Info`.
fn hazardous() -> Vec<u8> {
    let operated = operate_through_the_facade(&outline_fixture());
    let document = Document::open(operated).expect("opens");
    let mut editor = document.editor();
    let (open_action, s, js, javascript) = (
        editor.intern(b"OpenAction"),
        editor.intern(b"S"),
        editor.intern(b"JS"),
        editor.intern(b"JavaScript"),
    );
    let mut action = tinker_pdf::Dict::new();
    action.insert(s, tinker_pdf::Object::Name(javascript));
    action.insert(
        js,
        tinker_pdf::Object::String(tinker_pdf::PdfString::literal(b"app.alert(1)".to_vec())),
    );
    assert!(editor.update_catalog(|catalog| {
        catalog.insert(open_action, tinker_pdf::Object::Dict(action));
    }));
    editor.save(&WriteOptions::default())
}

#[test]
fn a_sanitise_reports_through_the_abi_what_the_facade_reports() {
    let bytes = hazardous();
    let facade = {
        let document = Document::open(bytes.clone()).expect("opens");
        let mut editor = document.editor();
        let report = editor.sanitise(&Sanitise::ALL);
        (report, editor.save(&WriteOptions::default()))
    };
    assert!(
        !facade.0.is_empty(),
        "the fixture has something to take out"
    );

    let editor = editor_over(&bytes);
    let all = TpdfSanitise {
        javascript: 1,
        actions: 1,
        embedded_files: 1,
        metadata: 1,
    };
    let mut report = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_editor_sanitise(editor, &all, &mut report) },
        TpdfStatus::Ok
    );
    assert_eq!(
        save(editor),
        facade.1,
        "the sanitised bytes are the facade's"
    );
    unsafe { tpdf_editor_free(editor) };

    let expected = &facade.0;
    assert_eq!(
        unsafe { tpdf_sanitise_report_count(report, TpdfSanitiseList::Removed as c_int) } as usize,
        expected.removed.len()
    );
    assert_eq!(
        unsafe { tpdf_sanitise_report_count(report, TpdfSanitiseList::Deleted as c_int) } as usize,
        expected.deleted.len()
    );
    for (index, entry) in expected.removed.iter().enumerate() {
        let index = index as u32;
        let (mut what, mut has, mut num, mut gen) = (TpdfRemoval::Info, 9, 0, 0);
        assert_eq!(
            unsafe {
                tpdf_sanitise_report_entry(
                    report,
                    TpdfSanitiseList::Removed as c_int,
                    index,
                    &mut what,
                    &mut has,
                    &mut num,
                    &mut gen,
                )
            },
            TpdfStatus::Ok
        );
        assert_eq!(what, TpdfRemoval::of(&entry.what));
        match entry.holder {
            EntryHolder::Trailer => assert_eq!(has, 0),
            EntryHolder::Object(r) => assert_eq!((has, num, gen), (1, r.num, r.gen)),
        }
        assert_eq!(
            unsafe { tpdf_sanitise_report_path_count(report, index) } as usize,
            entry.path.len()
        );
        for (step, expected_step) in entry.path.iter().enumerate() {
            let (mut is_index, mut position) = (9, 0);
            let (mut key, mut key_len) = (ptr::null(), 0);
            assert_eq!(
                unsafe {
                    tpdf_sanitise_report_path_step(
                        report,
                        index,
                        step as u32,
                        &mut is_index,
                        &mut position,
                        &mut key,
                        &mut key_len,
                    )
                },
                TpdfStatus::Ok
            );
            match expected_step {
                PathStep::Key(bytes) => {
                    assert_eq!(is_index, 0);
                    let read = unsafe { std::slice::from_raw_parts(key, key_len) };
                    assert_eq!(read, bytes.as_slice());
                }
                PathStep::Index(at) => assert_eq!((is_index, position), (1, *at as u64)),
            }
        }
        let (mut data, mut len) = (ptr::null(), 0);
        assert_eq!(
            unsafe {
                tpdf_sanitise_report_action(
                    report,
                    TpdfSanitiseList::Removed as c_int,
                    index,
                    &mut data,
                    &mut len,
                )
            },
            TpdfStatus::Ok
        );
        match &entry.what {
            Removal::Action(subtype) => {
                assert_eq!(
                    unsafe { std::slice::from_raw_parts(data, len) },
                    &subtype[..]
                )
            }
            _ => assert!(data.is_null()),
        }
    }
    for (index, entry) in expected.deleted.iter().enumerate() {
        let (mut what, mut has, mut num, mut gen) = (TpdfRemoval::Info, 9, 0, 0);
        assert_eq!(
            unsafe {
                tpdf_sanitise_report_entry(
                    report,
                    TpdfSanitiseList::Deleted as c_int,
                    index as u32,
                    &mut what,
                    &mut has,
                    &mut num,
                    &mut gen,
                )
            },
            TpdfStatus::Ok
        );
        assert_eq!(what, TpdfRemoval::of(&entry.what));
        assert_eq!((has, num, gen), (1, entry.object.num, entry.object.gen));
    }
    let past = u32::try_from(expected.removed.len()).expect("small");
    assert_eq!(
        unsafe {
            tpdf_sanitise_report_entry(
                report,
                TpdfSanitiseList::Removed as c_int,
                past,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        },
        TpdfStatus::BadArgument
    );
    unsafe { tpdf_sanitise_report_free(report) };
}

#[test]
fn null_handles_across_the_document_operations_are_refused() {
    let null_editor: *mut TpdfEditor = ptr::null_mut();
    let date = CREATED;
    let value = CString::new("x").expect("no nul");
    let all = TpdfSanitise {
        javascript: 1,
        actions: 1,
        embedded_files: 1,
        metadata: 1,
    };
    unsafe {
        assert_eq!(
            tpdf_editor_set_page_labels(null_editor, ptr::null(), 0),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_editor_attach_file(null_editor, ptr::null(), ptr::null_mut(), ptr::null_mut()),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_editor_set_outline(null_editor, ptr::null(), 0),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_editor_set_info(
                null_editor,
                TpdfInfoKey::Title as c_int,
                value.as_ptr(),
                ptr::null_mut()
            ),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_editor_set_info_date(
                null_editor,
                TpdfInfoKey::CreationDate as c_int,
                &date,
                ptr::null_mut()
            ),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_editor_set_trapped(null_editor, TpdfTrapped::True as c_int, ptr::null_mut()),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_editor_set_xmp_metadata(null_editor, PACKET.as_ptr(), 1, ptr::null_mut()),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_editor_set_page_boundary(
                null_editor,
                0,
                TpdfPageBoundary::TrimBox as c_int,
                0.0,
                0.0,
                1.0,
                1.0
            ),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_page_boundary(
                ptr::null(),
                0,
                TpdfPageBoundary::TrimBox as c_int,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut()
            ),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_editor_sanitise(null_editor, &all, &mut ptr::null_mut()),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_sanitise_report_count(ptr::null(), TpdfSanitiseList::Removed as c_int),
            0
        );
        assert_eq!(
            tpdf_sanitise_report_entry(
                ptr::null(),
                TpdfSanitiseList::Deleted as c_int,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut()
            ),
            TpdfStatus::BadArgument
        );
        let (mut data, mut len) = (ptr::null(), 0);
        assert_eq!(
            tpdf_sanitise_report_action(
                ptr::null(),
                TpdfSanitiseList::Removed as c_int,
                0,
                &mut data,
                &mut len
            ),
            TpdfStatus::BadArgument
        );
        assert_eq!(tpdf_sanitise_report_path_count(ptr::null(), 0), 0);
        assert_eq!(
            tpdf_sanitise_report_path_step(
                ptr::null(),
                0,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut()
            ),
            TpdfStatus::BadArgument
        );
        tpdf_sanitise_report_free(ptr::null_mut());
    }

    // A live editor with a null argument is refused too.
    let editor = editor_over(&outline_fixture());
    unsafe {
        assert_eq!(
            tpdf_editor_attach_file(editor, ptr::null(), ptr::null_mut(), ptr::null_mut()),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_editor_set_page_labels(editor, ptr::null(), 1),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_editor_set_info(
                editor,
                TpdfInfoKey::Title as c_int,
                ptr::null(),
                ptr::null_mut()
            ),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_editor_set_xmp_metadata(editor, ptr::null(), 4, ptr::null_mut()),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_editor_sanitise(editor, ptr::null(), &mut ptr::null_mut()),
            TpdfStatus::BadArgument
        );
        tpdf_editor_free(editor);
    }
}

/// Transcribed by hand into every binding, so pinned here.
#[test]
fn the_document_operation_enums_have_the_numbers_the_bindings_transcribe() {
    assert_eq!(TpdfLabelStyle::Decimal as i32, 0);
    assert_eq!(TpdfLabelStyle::RomanUpper as i32, 1);
    assert_eq!(TpdfLabelStyle::RomanLower as i32, 2);
    assert_eq!(TpdfLabelStyle::LettersUpper as i32, 3);
    assert_eq!(TpdfLabelStyle::LettersLower as i32, 4);
    assert_eq!(TpdfLabelStyle::None as i32, 5);
    assert_eq!(TpdfMetadataSync::Alone as i32, 0);
    assert_eq!(TpdfMetadataSync::OtherHalfUnchanged as i32, 1);
    assert_eq!(TpdfPageBoundary::MediaBox as i32, 0);
    assert_eq!(TpdfPageBoundary::CropBox as i32, 1);
    assert_eq!(TpdfPageBoundary::BleedBox as i32, 2);
    assert_eq!(TpdfPageBoundary::TrimBox as i32, 3);
    assert_eq!(TpdfPageBoundary::ArtBox as i32, 4);
    assert_eq!(TpdfRemoval::JavaScript as i32, 0);
    assert_eq!(TpdfRemoval::DocumentJavaScript as i32, 1);
    assert_eq!(TpdfRemoval::CalculationOrder as i32, 2);
    assert_eq!(TpdfRemoval::XfaForm as i32, 3);
    assert_eq!(TpdfRemoval::Action as i32, 4);
    assert_eq!(TpdfRemoval::EmbeddedFileTree as i32, 5);
    assert_eq!(TpdfRemoval::EmbeddedFile as i32, 6);
    assert_eq!(TpdfRemoval::Info as i32, 7);
    assert_eq!(TpdfRemoval::Metadata as i32, 8);
    assert_eq!(TpdfSanitiseList::Removed as i32, 0);
    assert_eq!(TpdfSanitiseList::Deleted as i32, 1);
    // `TpdfDate` is eight `int32_t`s and nothing else, so a hand-written
    // binding's layout cannot be off by a padding byte.
    assert_eq!(std::mem::size_of::<TpdfDate>(), 32);
}
