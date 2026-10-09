//! The read surface is pinned the way the signature surface is: by
//! **equality with the facade**. Every value read through the C ABI is
//! compared with what `Document` itself answers about the same bytes, so a
//! projection that invented, dropped or reordered anything fails here rather
//! than in a host language.

use super::*;
use crate::{tpdf_buffer_data, tpdf_buffer_free, tpdf_document_free, tpdf_string_free};
use std::ffi::CStr;
use tinker_pdf::{DocumentBuilder, EmbeddedFile, LabelStyle, OutlineEntry, PageLabelRange, Target};

/// A document carrying every shape the read surface reads: `/Info` entries
/// (one of them empty), a nested outline with explicit, URI and named targets
/// and a heading with none, links of two kinds, page labels, an attachment
/// and an XMP packet.
pub(crate) fn rich_document() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    let mut one = builder.begin_page(200.0, 200.0);
    one.text(b"F1", 12.0, 20.0, 170.0, "Links");
    assert!(one.link(
        10.0,
        10.0,
        60.0,
        30.0,
        &Target::Uri("https://example.org/a%20b".to_string())
    ));
    assert!(one.link(
        70.0,
        10.0,
        120.0,
        30.0,
        &Target::Page {
            index: 1,
            view: DestKind::Xyz {
                left: Some(10.0),
                top: None,
                zoom: Some(1.5),
            },
        }
    ));
    assert!(one.link(
        130.0,
        10.0,
        190.0,
        30.0,
        &Target::Named(b"chapter-two".to_vec())
    ));
    builder.push_page(one);
    let two = builder.begin_page(200.0, 200.0);
    builder.push_page(two);
    assert!(builder.add_named_destination(
        b"chapter-two",
        1,
        DestKind::FitR {
            left: 1.0,
            bottom: 2.0,
            right: 3.0,
            top: 4.0,
        }
    ));
    builder.set_info(b"Title", "Read surface");
    builder.set_info(b"Author", "");
    assert!(builder.set_outline(vec![
        OutlineEntry {
            title: "Part one".to_string(),
            target: None,
            open: true,
            children: vec![OutlineEntry {
                title: "Chapter one".to_string(),
                target: Some(Target::Page {
                    index: 0,
                    view: DestKind::FitH { top: Some(150.0) },
                }),
                open: false,
                children: Vec::new(),
            }],
        },
        OutlineEntry {
            title: "Elsewhere \u{2014} \u{fc}n\u{ef}code".to_string(),
            target: Some(Target::Uri("https://example.org/".to_string())),
            open: false,
            children: Vec::new(),
        },
        OutlineEntry {
            title: "By name".to_string(),
            target: Some(Target::Named(b"chapter-two".to_vec())),
            open: false,
            children: Vec::new(),
        },
    ]));
    let built = builder.finish();

    let document = Document::open(built).expect("the built document opens");
    let mut editor = document.editor();
    editor
        .attach_file(&EmbeddedFile {
            name: "data.csv".to_string(),
            filename: "data.csv".to_string(),
            description: Some("the numbers".to_string()),
            mime_type: Some("text/csv".to_string()),
            data: b"a,b\n1,2\n".to_vec(),
            ..EmbeddedFile::default()
        })
        .expect("the attachment is filed");
    editor
        .set_page_labels(&[PageLabelRange {
            first_page: 0,
            style: LabelStyle::RomanLower,
            prefix: Some("p-".to_string()),
            start: 1,
        }])
        .expect("the labels are written");
    let _ = editor.set_xmp_metadata(b"<x:xmpmeta xmlns:x='adobe:ns:meta/'/>");
    editor.save(&tinker_pdf::WriteOptions::default())
}

/// The canonical outline fixture with five bytes in front of its header, which
/// every offset it stores is then short by: an open that tolerates it and says
/// so with a warning.
pub(crate) fn shifted_document() -> Vec<u8> {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/outline-3level.pdf");
    let mut bytes = b"JUNK\n".to_vec();
    bytes.extend(std::fs::read(path).expect("the fixture is in the tree"));
    bytes
}

fn open(bytes: &[u8]) -> *mut TpdfDocument {
    let mut doc = ptr::null_mut();
    let status = unsafe { crate::tpdf_document_open(bytes.as_ptr(), bytes.len(), &mut doc) };
    assert_eq!(status, TpdfStatus::Ok);
    doc
}

/// A string the ABI handed over, as Rust's, freed; `None` for null.
fn take(text: *mut c_char) -> Option<String> {
    if text.is_null() {
        return None;
    }
    let value = unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned();
    unsafe { tpdf_string_free(text) };
    Some(value)
}

fn bytes_of(data: *const u8, len: usize) -> Option<Vec<u8>> {
    if data.is_null() {
        None
    } else {
        Some(unsafe { std::slice::from_raw_parts(data, len) }.to_vec())
    }
}

/// The facade's `Option<f64>` against the ABI's NaN-for-null.
fn same_number(abi: f64, facade: Option<f64>) -> bool {
    match facade {
        None => abi.is_nan(),
        Some(value) => abi == value,
    }
}

fn same_view(abi: &TpdfDestination, facade: &DestKind) -> bool {
    match *facade {
        DestKind::Xyz { left, top, zoom } => {
            abi.kind == TpdfDestKind::Xyz as c_int
                && same_number(abi.left, left)
                && same_number(abi.top, top)
                && same_number(abi.zoom, zoom)
        }
        DestKind::Fit => abi.kind == TpdfDestKind::Fit as c_int,
        DestKind::FitH { top } => {
            abi.kind == TpdfDestKind::FitH as c_int && same_number(abi.top, top)
        }
        DestKind::FitV { left } => {
            abi.kind == TpdfDestKind::FitV as c_int && same_number(abi.left, left)
        }
        DestKind::FitR {
            left,
            bottom,
            right,
            top,
        } => {
            abi.kind == TpdfDestKind::FitR as c_int
                && abi.left == left
                && abi.bottom == bottom
                && abi.right == right
                && abi.top == top
        }
        DestKind::FitB => abi.kind == TpdfDestKind::FitB as c_int,
        DestKind::FitBH { top } => {
            abi.kind == TpdfDestKind::FitBH as c_int && same_number(abi.top, top)
        }
        DestKind::FitBV { left } => {
            abi.kind == TpdfDestKind::FitBV as c_int && same_number(abi.left, left)
        }
    }
}

fn assert_same_destination(
    abi: &TpdfDestinationRead,
    bytes: Option<Vec<u8>>,
    facade: Option<&Destination>,
    what: &str,
) {
    match facade {
        None => {
            assert_eq!(abi.kind, TpdfDestinationKind::Absent, "{what}");
            assert_eq!(bytes, None, "{what}");
        }
        Some(Destination::Explicit {
            page_index,
            page_ref,
            kind,
        }) => {
            assert_eq!(abi.kind, TpdfDestinationKind::Explicit, "{what}");
            assert_eq!(
                (abi.has_page_index == 1).then_some(abi.page_index),
                *page_index,
                "{what}"
            );
            assert_eq!(
                (abi.has_page_ref == 1).then_some((abi.page_object, abi.page_generation)),
                page_ref.map(|r| (r.num, r.gen)),
                "{what}"
            );
            assert!(
                same_view(&abi.view, kind),
                "{what}: {:?} vs {kind:?}",
                abi.view
            );
            assert_eq!(bytes, None, "{what}");
        }
        Some(Destination::Named(name)) => {
            assert_eq!(abi.kind, TpdfDestinationKind::Named, "{what}");
            assert_eq!(bytes.as_deref(), Some(name.as_slice()), "{what}");
        }
        Some(Destination::Uri(uri)) => {
            assert_eq!(abi.kind, TpdfDestinationKind::Uri, "{what}");
            assert_eq!(bytes.as_deref(), Some(uri.as_slice()), "{what}");
        }
    }
}

const KEYS: [TpdfInfoKey; 8] = [
    TpdfInfoKey::Title,
    TpdfInfoKey::Author,
    TpdfInfoKey::Subject,
    TpdfInfoKey::Keywords,
    TpdfInfoKey::Creator,
    TpdfInfoKey::Producer,
    TpdfInfoKey::CreationDate,
    TpdfInfoKey::ModificationDate,
];

fn info_through_the_abi(doc: *const TpdfDocument) -> Vec<Option<String>> {
    KEYS.iter()
        .map(|key| {
            let mut out = ptr::null_mut();
            assert_eq!(
                unsafe { tpdf_document_info(doc, *key as c_int, &mut out) },
                TpdfStatus::Ok
            );
            take(out)
        })
        .collect()
}

/// Every label, through one `TpdfPageLabels` handle; an index past its end is
/// refused rather than read as "no label".
fn labels_through_the_abi(doc: *const TpdfDocument) -> Vec<String> {
    let mut labels = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_document_page_labels(doc, &mut labels) },
        TpdfStatus::Ok
    );
    let count = unsafe { tpdf_page_labels_count(labels) };
    let read = (0..count)
        .map(|index| {
            let mut out = ptr::null_mut();
            assert_eq!(
                unsafe { tpdf_page_label_text(labels, index, &mut out) },
                TpdfStatus::Ok
            );
            take(out).expect("a label is never null")
        })
        .collect();
    let mut out = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_page_label_text(labels, count, &mut out) },
        TpdfStatus::BadArgument
    );
    assert!(out.is_null(), "nothing is written past the end");
    unsafe { tpdf_page_labels_free(labels) };
    read
}

#[test]
fn info_version_labels_and_xmp_read_the_same_through_the_abi() {
    for bytes in [rich_document(), shifted_document()] {
        let facade = Document::open(bytes.clone()).expect("opens");
        let doc = open(&bytes);
        let metadata = facade.metadata();
        assert_eq!(
            info_through_the_abi(doc),
            vec![
                metadata.title.clone(),
                metadata.author.clone(),
                metadata.subject.clone(),
                metadata.keywords.clone(),
                metadata.creator.clone(),
                metadata.producer.clone(),
                metadata.creation_date.clone(),
                metadata.modification_date.clone(),
            ]
        );

        let mut trapped = TpdfTrapped::Unknown;
        assert_eq!(
            unsafe { tpdf_document_trapped(doc, &mut trapped) },
            TpdfStatus::Ok
        );
        assert_eq!(
            trapped,
            match metadata.trapped {
                None => TpdfTrapped::Absent,
                Some(Trapped::True) => TpdfTrapped::True,
                Some(Trapped::False) => TpdfTrapped::False,
                Some(Trapped::Unknown) => TpdfTrapped::Unknown,
            }
        );

        let mut version = ptr::null_mut();
        assert_eq!(
            unsafe { tpdf_document_pdf_version(doc, &mut version) },
            TpdfStatus::Ok
        );
        assert_eq!(take(version), Some(facade.pdf_version()));

        assert_eq!(labels_through_the_abi(doc), facade.page_labels());

        let mut xmp = ptr::null_mut();
        assert_eq!(
            unsafe { tpdf_document_xmp_metadata(doc, &mut xmp) },
            TpdfStatus::Ok
        );
        let mut len = 0;
        let data = unsafe { tpdf_buffer_data(xmp, &mut len) };
        assert_eq!(bytes_of(data, len), facade.xmp_metadata());
        unsafe { tpdf_buffer_free(xmp) };
        unsafe { tpdf_document_free(doc) };
    }

    // The rich document says what it was given, which is what makes the
    // equality above a statement about something rather than about two
    // absences: an empty `/Author` is an empty string and not a null, a
    // labelled page has its label, and the packet is there.
    let bytes = rich_document();
    let doc = open(&bytes);
    let info = info_through_the_abi(doc);
    assert_eq!(info[0].as_deref(), Some("Read surface"));
    assert_eq!(info[1].as_deref(), Some(""), "empty is not absent");
    assert_eq!(info[2], None, "absent is not empty");
    assert_eq!(labels_through_the_abi(doc), ["p-i", "p-ii"]);
    let mut xmp = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_document_xmp_metadata(doc, &mut xmp) },
        TpdfStatus::Ok
    );
    assert!(!xmp.is_null(), "the packet the editor wrote is read back");
    unsafe { tpdf_buffer_free(xmp) };
    unsafe { tpdf_document_free(doc) };

    // And the shifted fixture has none: null on Ok, not a failure, and an
    // empty label handle rather than a refusal.
    let bytes = shifted_document();
    let doc = open(&bytes);
    let mut xmp = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_document_xmp_metadata(doc, &mut xmp) },
        TpdfStatus::Ok
    );
    assert!(xmp.is_null());
    assert!(labels_through_the_abi(doc).is_empty());
    unsafe { tpdf_document_free(doc) };
}

/// The labels are read once, when the handle is built, and every indexed read
/// after that is a lookup in the handle's own copy: the document is freed
/// before the first label is asked for, so a read that went back to it — the
/// per-index walk this handle replaced, which built every label to answer
/// one — could not answer at all.
#[test]
fn the_page_label_handle_is_one_walk_and_outlives_its_document() {
    let mut builder = DocumentBuilder::new();
    for _ in 0..300 {
        let page = builder.begin_page(100.0, 100.0);
        builder.push_page(page);
    }
    let document = Document::open(builder.finish()).expect("the built document opens");
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
                first_page: 4,
                style: LabelStyle::Decimal,
                prefix: Some("A-".to_string()),
                start: 1,
            },
        ])
        .expect("the labels are written");
    let bytes = editor.save(&tinker_pdf::WriteOptions::default());
    let facade = Document::open(bytes.clone()).expect("the labelled document opens");

    let doc = open(&bytes);
    let mut labels = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_document_page_labels(doc, &mut labels) },
        TpdfStatus::Ok
    );
    unsafe { tpdf_document_free(doc) };

    assert_eq!(unsafe { tpdf_page_labels_count(labels) }, 300);
    let read: Vec<String> = (0..300)
        .map(|index| {
            let mut out = ptr::null_mut();
            assert_eq!(
                unsafe { tpdf_page_label_text(labels, index, &mut out) },
                TpdfStatus::Ok
            );
            take(out).expect("a label")
        })
        .collect();
    unsafe { tpdf_page_labels_free(labels) };
    assert_eq!(read, facade.page_labels());
    assert_eq!(read[..5], ["i", "ii", "iii", "iv", "A-1"]);
    assert_eq!(read[299], "A-296");
}

#[test]
fn the_outline_reads_the_same_through_the_abi() {
    for bytes in [rich_document(), shifted_document()] {
        let facade = Document::open(bytes.clone()).expect("opens");
        let tree = facade.outline();
        let flat = OutlineItem::flatten(&tree);
        assert!(!flat.is_empty(), "both documents carry an outline");

        let doc = open(&bytes);
        let mut outline = ptr::null_mut();
        assert_eq!(
            unsafe { tpdf_document_outline(doc, &mut outline) },
            TpdfStatus::Ok
        );
        unsafe { tpdf_document_free(doc) };
        assert_eq!(unsafe { tpdf_outline_count(outline) } as usize, flat.len());
        for (index, (depth, item)) in flat.iter().enumerate() {
            let index = index as u32;
            let (mut abi_depth, mut abi_open) = (99, 99);
            assert_eq!(
                unsafe { tpdf_outline_item(outline, index, &mut abi_depth, &mut abi_open) },
                TpdfStatus::Ok
            );
            assert_eq!(abi_depth, *depth);
            assert_eq!(abi_open, c_int::from(item.open));
            let mut title = ptr::null_mut();
            assert_eq!(
                unsafe { tpdf_outline_title(outline, index, &mut title) },
                TpdfStatus::Ok
            );
            assert_eq!(take(title).as_deref(), Some(item.title.as_str()));
            let mut read = destination_read(None);
            assert_eq!(
                unsafe { tpdf_outline_destination(outline, index, &mut read) },
                TpdfStatus::Ok
            );
            let (mut data, mut len) = (ptr::null(), 0);
            assert_eq!(
                unsafe { tpdf_outline_destination_bytes(outline, index, &mut data, &mut len) },
                TpdfStatus::Ok
            );
            assert_same_destination(
                &read,
                bytes_of(data, len),
                item.destination.as_ref(),
                &item.title,
            );
        }
        let past = flat.len() as u32;
        let mut title = ptr::null_mut();
        assert_eq!(
            unsafe { tpdf_outline_title(outline, past, &mut title) },
            TpdfStatus::BadArgument,
            "an index past the end is a wrong argument, not an absent title"
        );
        unsafe { tpdf_outline_free(outline) };
    }

    // Every destination arm the writer can produce is reached, so the
    // equality above covers each rather than one.
    let facade = Document::open(rich_document()).expect("opens");
    let kinds: Vec<_> = OutlineItem::flatten(&facade.outline())
        .iter()
        .map(|(_, item)| destination_read(item.destination.as_ref()).kind)
        .collect();
    for kind in [
        TpdfDestinationKind::Absent,
        TpdfDestinationKind::Explicit,
        TpdfDestinationKind::Named,
    ] {
        assert!(kinds.contains(&kind), "{kind:?} in {kinds:?}");
    }
}

#[test]
fn links_read_the_same_through_the_abi() {
    let bytes = rich_document();
    let facade = Document::open(bytes.clone()).expect("opens");
    let expected = facade.page(0).expect("page 0").links();
    assert_eq!(expected.len(), 3);

    let doc = open(&bytes);
    let mut links = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_page_links(doc, 0, &mut links) },
        TpdfStatus::Ok
    );
    let mut nowhere = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_page_links(doc, 2, &mut nowhere) },
        TpdfStatus::NoSuchPage
    );
    unsafe { tpdf_document_free(doc) };

    assert_eq!(unsafe { tpdf_links_count(links) } as usize, expected.len());
    let mut kinds = Vec::new();
    for (index, link) in expected.iter().enumerate() {
        let index = index as u32;
        let (mut x0, mut y0, mut x1, mut y1) = (0.0, 0.0, 0.0, 0.0);
        assert_eq!(
            unsafe { tpdf_link_rect(links, index, &mut x0, &mut y0, &mut x1, &mut y1) },
            TpdfStatus::Ok
        );
        assert_eq!(
            [x0, y0, x1, y1],
            [link.rect.x0, link.rect.y0, link.rect.x1, link.rect.y1]
        );

        let (mut present, mut num, mut gen) = (9, 0, 0);
        assert_eq!(
            unsafe { tpdf_link_reference(links, index, &mut present, &mut num, &mut gen) },
            TpdfStatus::Ok
        );
        assert_eq!(
            (present == 1).then_some((num, gen)),
            link.reference.map(|r| (r.num, r.gen))
        );

        let mut kind = TpdfActionKind::Other;
        let mut read = destination_read(None);
        assert_eq!(
            unsafe { tpdf_link_action(links, index, &mut kind, &mut read) },
            TpdfStatus::Ok
        );
        let (mut data, mut len) = (ptr::null(), 0);
        assert_eq!(
            unsafe { tpdf_link_action_bytes(links, index, &mut data, &mut len) },
            TpdfStatus::Ok
        );
        let action_bytes = bytes_of(data, len);
        assert_eq!(
            unsafe { tpdf_link_destination_bytes(links, index, &mut data, &mut len) },
            TpdfStatus::Ok
        );
        let destination = bytes_of(data, len);
        match &link.target {
            Some(Action::GoTo(dest)) => {
                assert_eq!(kind, TpdfActionKind::GoTo);
                assert_eq!(action_bytes, None);
                assert_same_destination(&read, destination, Some(dest), "goto");
            }
            Some(Action::Uri(uri)) => {
                assert_eq!(kind, TpdfActionKind::Uri);
                assert_eq!(action_bytes.as_deref(), Some(uri.as_slice()));
                assert_same_destination(&read, destination, None, "uri");
            }
            other => panic!("the fixture writes no {other:?}"),
        }
        kinds.push(kind);
    }
    assert!(kinds.contains(&TpdfActionKind::Uri));
    assert!(kinds.contains(&TpdfActionKind::GoTo));
    let mut kind = TpdfActionKind::Absent;
    let mut read = destination_read(None);
    assert_eq!(
        unsafe { tpdf_link_action(links, 3, &mut kind, &mut read) },
        TpdfStatus::BadArgument
    );
    unsafe { tpdf_links_free(links) };
}

#[test]
fn attachments_read_the_same_through_the_abi_with_their_bytes() {
    let bytes = rich_document();
    let facade = Document::open(bytes.clone()).expect("opens");
    let expected = facade.attachments();
    assert_eq!(expected.len(), 1);

    let doc = open(&bytes);
    let mut attachments = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_document_attachments(doc, &mut attachments) },
        TpdfStatus::Ok
    );
    // Freed before the bytes are asked for: the handle holds its own
    // document, which is the property its documentation promises.
    unsafe { tpdf_document_free(doc) };
    assert_eq!(unsafe { tpdf_attachments_count(attachments) }, 1);

    let attachment = &expected[0];
    let mut text = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_attachment_name(attachments, 0, &mut text) },
        TpdfStatus::Ok
    );
    assert_eq!(take(text).as_deref(), Some(attachment.name.as_str()));
    assert_eq!(
        unsafe { tpdf_attachment_filename(attachments, 0, &mut text) },
        TpdfStatus::Ok
    );
    assert_eq!(take(text).as_deref(), Some(attachment.filename.as_str()));
    assert_eq!(
        unsafe { tpdf_attachment_description(attachments, 0, &mut text) },
        TpdfStatus::Ok
    );
    assert_eq!(take(text), attachment.description);
    let (mut present, mut size) = (9, 0);
    assert_eq!(
        unsafe { tpdf_attachment_size(attachments, 0, &mut present, &mut size) },
        TpdfStatus::Ok
    );
    assert_eq!((present == 1).then_some(size), attachment.size);

    let mut buffer = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_attachment_data(attachments, 0, &mut buffer) },
        TpdfStatus::Ok
    );
    let mut len = 0;
    let data = unsafe { tpdf_buffer_data(buffer, &mut len) };
    assert_eq!(bytes_of(data, len).as_deref(), Some(&b"a,b\n1,2\n"[..]));
    let stream = attachment.stream.expect("the attachment has a stream");
    assert_eq!(
        bytes_of(data, len),
        facade.cos().stream_decoded(stream).ok(),
        "the bytes are the facade's own route to them"
    );
    unsafe { tpdf_buffer_free(buffer) };

    let mut buffer = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_attachment_data(attachments, 1, &mut buffer) },
        TpdfStatus::BadArgument
    );
    unsafe { tpdf_attachments_free(attachments) };
}

#[test]
fn warnings_read_the_same_through_the_abi() {
    let bytes = shifted_document();
    let facade = Document::open(bytes.clone()).expect("opens");
    let expected = facade.warnings();
    assert!(
        expected
            .iter()
            .any(|w| w.kind.as_str() == "header-not-at-start"),
        "the shifted header is tolerated and reported: {expected:?}"
    );

    let doc = open(&bytes);
    let mut warnings = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_document_warnings(doc, &mut warnings) },
        TpdfStatus::Ok
    );
    unsafe { tpdf_document_free(doc) };
    assert_eq!(
        unsafe { tpdf_warnings_count(warnings) } as usize,
        expected.len()
    );
    for (index, warning) in expected.iter().enumerate() {
        let index = index as u32;
        let (mut offset, mut has, mut num, mut gen) = (0, 9, 0, 0);
        assert_eq!(
            unsafe {
                tpdf_warning_location(warnings, index, &mut offset, &mut has, &mut num, &mut gen)
            },
            TpdfStatus::Ok
        );
        assert_eq!(offset, warning.offset);
        assert_eq!(
            (has == 1).then_some((num, gen)),
            warning.object.map(|r| (r.num, r.gen))
        );
        let mut text = ptr::null_mut();
        assert_eq!(
            unsafe { tpdf_warning_kind(warnings, index, &mut text) },
            TpdfStatus::Ok
        );
        assert_eq!(take(text).as_deref(), Some(warning.kind.as_str()));
        assert_eq!(
            unsafe { tpdf_warning_message(warnings, index, &mut text) },
            TpdfStatus::Ok
        );
        assert_eq!(take(text), Some(warning.kind.to_string()));
    }
    unsafe { tpdf_warnings_free(warnings) };

    // A clean document answers with an empty list, not a failure.
    let clean = rich_document();
    let doc = open(&clean);
    let mut warnings = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_document_warnings(doc, &mut warnings) },
        TpdfStatus::Ok
    );
    assert_eq!(unsafe { tpdf_warnings_count(warnings) }, 0);
    unsafe { tpdf_warnings_free(warnings) };
    unsafe { tpdf_document_free(doc) };
}

/// Every entry point refuses a null handle rather than dereferencing it, and
/// every free accepts one.
#[test]
fn null_handles_across_the_read_surface_are_refused() {
    let null_doc: *const TpdfDocument = ptr::null();
    let mut text = ptr::null_mut();
    let mut buffer = ptr::null_mut();
    let mut trapped = TpdfTrapped::Absent;
    let (mut data, mut len) = (ptr::null(), 0);
    let mut read = destination_read(None);
    let mut kind = TpdfActionKind::Absent;
    unsafe {
        assert_eq!(
            tpdf_document_info(null_doc, TpdfInfoKey::Title as c_int, &mut text),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_document_trapped(null_doc, &mut trapped),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_document_pdf_version(null_doc, &mut text),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_document_page_labels(null_doc, &mut ptr::null_mut()),
            TpdfStatus::BadArgument
        );
        assert_eq!(tpdf_page_labels_count(ptr::null()), 0);
        assert_eq!(
            tpdf_page_label_text(ptr::null(), 0, &mut text),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_document_xmp_metadata(null_doc, &mut buffer),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_document_outline(null_doc, &mut ptr::null_mut()),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_page_links(null_doc, 0, &mut ptr::null_mut()),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_document_attachments(null_doc, &mut ptr::null_mut()),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_document_warnings(null_doc, &mut ptr::null_mut()),
            TpdfStatus::BadArgument
        );

        assert_eq!(tpdf_outline_count(ptr::null()), 0);
        assert_eq!(
            tpdf_outline_item(ptr::null(), 0, ptr::null_mut(), ptr::null_mut()),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_outline_title(ptr::null(), 0, &mut text),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_outline_destination(ptr::null(), 0, &mut read),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_outline_destination_bytes(ptr::null(), 0, &mut data, &mut len),
            TpdfStatus::BadArgument
        );

        assert_eq!(tpdf_links_count(ptr::null()), 0);
        assert_eq!(
            tpdf_link_rect(
                ptr::null(),
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut()
            ),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_link_reference(
                ptr::null(),
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut()
            ),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_link_action(ptr::null(), 0, &mut kind, &mut read),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_link_action_bytes(ptr::null(), 0, &mut data, &mut len),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_link_destination_bytes(ptr::null(), 0, &mut data, &mut len),
            TpdfStatus::BadArgument
        );

        assert_eq!(tpdf_attachments_count(ptr::null()), 0);
        assert_eq!(
            tpdf_attachment_name(ptr::null(), 0, &mut text),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_attachment_filename(ptr::null(), 0, &mut text),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_attachment_description(ptr::null(), 0, &mut text),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_attachment_size(ptr::null(), 0, ptr::null_mut(), ptr::null_mut()),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_attachment_data(ptr::null(), 0, &mut buffer),
            TpdfStatus::BadArgument
        );

        assert_eq!(tpdf_warnings_count(ptr::null()), 0);
        assert_eq!(
            tpdf_warning_location(
                ptr::null(),
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut()
            ),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_warning_kind(ptr::null(), 0, &mut text),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_warning_message(ptr::null(), 0, &mut text),
            TpdfStatus::BadArgument
        );

        tpdf_page_labels_free(ptr::null_mut());
        tpdf_outline_free(ptr::null_mut());
        tpdf_links_free(ptr::null_mut());
        tpdf_attachments_free(ptr::null_mut());
        tpdf_warnings_free(ptr::null_mut());
    }

    // And a live handle with a null out pointer is refused too, not written
    // through.
    let bytes = rich_document();
    let doc = open(&bytes);
    unsafe {
        assert_eq!(
            tpdf_document_info(doc, TpdfInfoKey::Title as c_int, ptr::null_mut()),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_document_trapped(doc, ptr::null_mut()),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_document_xmp_metadata(doc, ptr::null_mut()),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_document_page_labels(doc, ptr::null_mut()),
            TpdfStatus::BadArgument
        );
        let mut labels = ptr::null_mut();
        assert_eq!(tpdf_document_page_labels(doc, &mut labels), TpdfStatus::Ok);
        assert_eq!(
            tpdf_page_label_text(labels, 0, ptr::null_mut()),
            TpdfStatus::BadArgument
        );
        tpdf_page_labels_free(labels);
        let mut links = ptr::null_mut();
        assert_eq!(tpdf_page_links(doc, 0, &mut links), TpdfStatus::Ok);
        assert_eq!(
            tpdf_link_action(links, 0, ptr::null_mut(), &mut read),
            TpdfStatus::BadArgument
        );
        assert_eq!(
            tpdf_link_action_bytes(links, 0, ptr::null_mut(), &mut len),
            TpdfStatus::BadArgument
        );
        tpdf_links_free(links);
        tpdf_document_free(doc);
    }
}

/// The enums are transcribed by hand into every binding, so their numbers are
/// pinned here the way the signature enums' are.
#[test]
fn the_read_enums_have_the_numbers_the_bindings_transcribe() {
    for (key, number) in KEYS.iter().zip(0..) {
        assert_eq!(*key as i32, number, "{key:?}");
    }
    assert_eq!(TpdfTrapped::Absent as i32, 0);
    assert_eq!(TpdfTrapped::True as i32, 1);
    assert_eq!(TpdfTrapped::False as i32, 2);
    assert_eq!(TpdfTrapped::Unknown as i32, 3);
    assert_eq!(TpdfDestinationKind::Absent as i32, 0);
    assert_eq!(TpdfDestinationKind::Explicit as i32, 1);
    assert_eq!(TpdfDestinationKind::Named as i32, 2);
    assert_eq!(TpdfDestinationKind::Uri as i32, 3);
    assert_eq!(TpdfActionKind::Absent as i32, 0);
    assert_eq!(TpdfActionKind::GoTo as i32, 1);
    assert_eq!(TpdfActionKind::GoToR as i32, 2);
    assert_eq!(TpdfActionKind::Uri as i32, 3);
    assert_eq!(TpdfActionKind::Named as i32, 4);
    assert_eq!(TpdfActionKind::Launch as i32, 5);
    assert_eq!(TpdfActionKind::Other as i32, 6);
}
