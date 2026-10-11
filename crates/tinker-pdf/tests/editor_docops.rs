//! Document operations on an existing document, each written by a typed
//! setter on `DocumentEditor` and read back by the reader this facade already
//! had (tier 5, "Document operations").
//!
//! # The shape of every test here
//!
//! Open a document this repository's builder wrote, edit it through the
//! editor the facade hands out, save it **both ways** — an incremental update
//! and a rewrite — reopen each through `Document::open`, and ask the public
//! read API for what was written. Then hand the saved file to the strict
//! validator, which reads it again with the leniency ladder off: a setter
//! whose output this reader repairs in silence would pass the first half and
//! not the second.
//!
//! Both save modes, because they build different object sets and a setter
//! that reached only one of them is the defect `page_operations.rs` was
//! written after.

use tinker_pdf::{
    AttachError, Date, DeletedObject, DestKind, Destination, Document, DocumentBuilder,
    DocumentEditor, Duplex, EmbeddedFile, EnforcedPreference, EntryHolder, LabelStyle,
    MetadataSync, NonFullScreenPageMode, OutlineEntry, PageBoundary, PageLabelError,
    PageLabelRange, PathStep, PrintScaling, ReadingDirection, Removal, RemovedEntry, Sanitise,
    SanitiseReport, Target, Trapped, ViewerPreferences, WriteMode, WriteOptions,
};

/// Three pages of text, a title, and nothing else the tests below write.
fn plain() -> Document {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    builder.set_info(b"Title", "Before");
    for index in 0..3 {
        builder.add_page(200.0, 300.0, |page| {
            page.text(b"F0", 12.0, 20.0, 150.0, &format!("page {index}"));
        });
    }
    Document::open(builder.finish()).expect("the builder's output opens")
}

const MODES: [WriteMode; 2] = [WriteMode::Incremental, WriteMode::Rewrite];

/// Saves `editor` in `mode`, reopens the file, and holds it to the strict
/// validator before handing it back.
#[track_caller]
fn saved(editor: &DocumentEditor, mode: WriteMode, compress: bool) -> (Vec<u8>, Document) {
    let bytes = editor.save(&WriteOptions {
        mode,
        compress,
        ..WriteOptions::default()
    });
    let reopened = Document::open(bytes.clone()).expect("the saved file reopens");
    let defects = reopened.validate();
    assert!(
        defects.is_empty(),
        "{mode:?}: the strict validator refused the output: {defects:?}"
    );
    (bytes, reopened)
}

fn date(year: i32, offset: Option<i32>) -> Date {
    Date {
        year,
        month: 3,
        day: 14,
        hour: 15,
        minute: 9,
        second: 26,
        utc_offset_minutes: offset,
    }
}

// ---- page labels ------------------------------------------------------------

fn labels() -> Vec<PageLabelRange> {
    vec![
        PageLabelRange {
            first_page: 0,
            style: LabelStyle::RomanLower,
            prefix: None,
            start: 1,
        },
        PageLabelRange {
            first_page: 2,
            style: LabelStyle::Decimal,
            prefix: Some("Anhang ".to_string()),
            start: 7,
        },
    ]
}

#[test]
fn page_labels_read_back_after_both_saves() {
    let doc = plain();
    assert!(doc.page_labels().is_empty());
    let mut editor = doc.editor();
    editor
        .set_page_labels(&labels())
        .expect("the ranges are writable");
    for mode in MODES {
        let (_, reopened) = saved(&editor, mode, false);
        assert_eq!(reopened.page_labels(), ["i", "ii", "Anhang 7"], "{mode:?}");
    }
}

/// A second call replaces the first: the old tree is deleted, not left beside
/// the new one, and an empty list removes the labels outright.
#[test]
fn page_labels_are_replaced_and_removed() {
    let doc = plain();
    let mut editor = doc.editor();
    editor.set_page_labels(&labels()).expect("writable");
    let first = editor.view().expect("a view");
    let old_root = first
        .catalog()
        .and_then(|c| c.get_ref(first.intern(b"PageLabels")))
        .expect("a tree");

    editor
        .set_page_labels(&[PageLabelRange {
            first_page: 0,
            style: LabelStyle::LettersUpper,
            prefix: Some("§".to_string()),
            start: 27,
        }])
        .expect("writable");
    assert_eq!(
        editor.get(old_root),
        Some(tinker_pdf::Object::Null),
        "the first tree is deleted"
    );
    for mode in MODES {
        let (_, reopened) = saved(&editor, mode, false);
        assert_eq!(reopened.page_labels(), ["§AA", "§BB", "§CC"], "{mode:?}");
    }

    editor
        .set_page_labels(&[])
        .expect("an empty list is accepted");
    for mode in MODES {
        let (_, reopened) = saved(&editor, mode, false);
        assert!(reopened.page_labels().is_empty(), "{mode:?}");
    }
}

/// Every refusal leaves the editor exactly as it was.
#[test]
fn page_label_refusals_write_nothing() {
    let doc = plain();
    let mut editor = doc.editor();
    let range = |first_page, start| PageLabelRange {
        first_page,
        style: LabelStyle::Decimal,
        prefix: None,
        start,
    };
    assert_eq!(
        editor.set_page_labels(&[range(1, 1)]),
        Err(PageLabelError::NoFirstPage)
    );
    assert_eq!(
        editor.set_page_labels(&[range(0, 1), range(3, 1)]),
        Err(PageLabelError::PastLastPage {
            first_page: 3,
            pages: 3
        })
    );
    assert_eq!(
        editor.set_page_labels(&[range(0, 0)]),
        Err(PageLabelError::StartBelowOne { first_page: 0 })
    );
    assert!(matches!(
        editor.set_page_labels(&[range(0, 1), range(0, 4)]),
        Err(PageLabelError::Tree(_))
    ));
    assert!(!editor.is_dirty(), "a refusal wrote something");
}

// ---- embedded files ---------------------------------------------------------

fn csv() -> EmbeddedFile {
    EmbeddedFile {
        name: "Übersicht".to_string(),
        filename: "übersicht.csv".to_string(),
        description: Some("The figures behind table 2".to_string()),
        mime_type: Some("text/csv".to_string()),
        created: Some(date(2026, Some(60))),
        modified: Some(date(2026, Some(0))),
        data: b"quarter,revenue\nQ1,12\nQ2,19\n".to_vec(),
    }
}

#[test]
fn an_embedded_file_reads_back_through_the_attachments_reader() {
    let doc = plain();
    let mut editor = doc.editor();
    let file = csv();
    let stream = editor.attach_file(&file).expect("attachable");
    for mode in MODES {
        for compress in [false, true] {
            let (_, reopened) = saved(&editor, mode, compress);
            let attachments = reopened.attachments();
            assert_eq!(attachments.len(), 1, "{mode:?}");
            let attachment = &attachments[0];
            assert_eq!(attachment.name, file.name);
            assert_eq!(attachment.filename, file.filename, "/UF wins over /F");
            assert_eq!(attachment.description, file.description);
            assert_eq!(attachment.size, Some(file.data.len() as i64));
            let at = attachment.stream.expect("the /EF stream");
            if mode == WriteMode::Incremental {
                assert_eq!(at, stream, "an update keeps the editor's numbering");
            }

            let cos = reopened.cos();
            assert_eq!(
                cos.stream_decoded(at).expect("the stream decodes"),
                file.data,
                "{mode:?} compress {compress}"
            );
            let object = cos.get(at).expect("the stream");
            let dict = &object.as_stream().expect("a stream").dict;
            let subtype = dict
                .get_name(cos.intern(b"Subtype"))
                .and_then(|n| cos.name_bytes(n));
            assert_eq!(subtype.as_deref(), Some(&b"text/csv"[..]));
            let params = cos.resolve_key(dict, cos.intern(b"Params"));
            let params = params.as_dict().expect("/Params");
            let checksum = params
                .get_string(cos.intern(b"CheckSum"))
                .expect("/CheckSum");
            assert_eq!(
                checksum.bytes,
                tinker_pdf_crypto::md5::md5(&file.data),
                "Table 45: MD5 of the unencoded bytes"
            );
            let when = |key: &[u8]| {
                params
                    .get_string(cos.intern(key))
                    .and_then(|s| tinker_pdf_cos::parse_date(&String::from_utf8_lossy(&s.bytes)))
            };
            assert_eq!(when(b"CreationDate"), file.created);
            assert_eq!(when(b"ModDate"), file.modified);
        }
    }
}

/// A second file joins the first in the tree; a name already taken is
/// refused and writes nothing.
#[test]
fn a_second_file_joins_the_first_and_a_taken_name_is_refused() {
    let doc = plain();
    let mut editor = doc.editor();
    editor.attach_file(&csv()).expect("attachable");
    let second = EmbeddedFile {
        name: "notes".to_string(),
        filename: "notes.txt".to_string(),
        data: b"n".to_vec(),
        ..EmbeddedFile::default()
    };
    editor.attach_file(&second).expect("attachable");

    let before = editor.checkpoint();
    let view = editor.view().expect("a view");
    assert_eq!(
        editor.attach_file(&csv()),
        Err(AttachError::NameTaken("Übersicht".to_string()))
    );
    let bad_mime = EmbeddedFile {
        name: "third".to_string(),
        mime_type: Some("text/plain; charset=utf-8".to_string()),
        ..EmbeddedFile::default()
    };
    assert!(matches!(
        editor.attach_file(&bad_mime),
        Err(AttachError::MimeType(_))
    ));
    let bad_date = EmbeddedFile {
        name: "fourth".to_string(),
        modified: Some(date(12_026, None)),
        ..EmbeddedFile::default()
    };
    assert!(matches!(
        editor.attach_file(&bad_date),
        Err(AttachError::Date(_))
    ));
    assert_eq!(
        format!("{:?}", editor.checkpoint()),
        format!("{before:?}"),
        "a refusal wrote something"
    );
    drop(view);

    for mode in MODES {
        let (_, reopened) = saved(&editor, mode, false);
        let names: Vec<String> = reopened.attachments().into_iter().map(|a| a.name).collect();
        assert_eq!(names, ["notes", "Übersicht"], "{mode:?}: byte order");
    }
}

// ---- the outline ------------------------------------------------------------

fn outline() -> Vec<OutlineEntry> {
    vec![
        OutlineEntry {
            title: "Kapitel Ü".to_string(),
            target: Some(Target::Page {
                index: 1,
                view: DestKind::Xyz {
                    left: Some(0.0),
                    top: Some(300.0),
                    zoom: None,
                },
            }),
            open: false,
            children: vec![OutlineEntry {
                title: "Section".to_string(),
                target: Some(Target::Page {
                    index: 2,
                    view: DestKind::Fit,
                }),
                open: false,
                children: Vec::new(),
            }],
        },
        OutlineEntry {
            title: "Elsewhere".to_string(),
            target: Some(Target::Uri("https://example.org/".to_string())),
            open: false,
            children: Vec::new(),
        },
    ]
}

#[test]
fn an_outline_reads_back_with_its_destination_kinds_intact() {
    let doc = plain();
    assert!(doc.outline().is_empty());
    let mut editor = doc.editor();
    assert!(editor.set_outline(&outline()));
    for mode in MODES {
        let (_, reopened) = saved(&editor, mode, false);
        let items = reopened.outline();
        assert_eq!(items.len(), 2, "{mode:?}");
        assert_eq!(items[0].title, "Kapitel Ü");
        assert!(!items[0].open, "a closed entry reads closed");
        match &items[0].destination {
            Some(Destination::Explicit {
                page_index, kind, ..
            }) => {
                assert_eq!(*page_index, Some(1));
                assert_eq!(
                    *kind,
                    DestKind::Xyz {
                        left: Some(0.0),
                        top: Some(300.0),
                        zoom: None
                    }
                );
            }
            other => panic!("{mode:?}: ruling 6 — an explicit destination, got {other:?}"),
        }
        assert_eq!(items[0].children.len(), 1);
        assert!(matches!(
            items[0].children[0].destination,
            Some(Destination::Explicit {
                page_index: Some(2),
                kind: DestKind::Fit,
                ..
            })
        ));
        assert_eq!(
            items[1].destination,
            Some(Destination::Uri(b"https://example.org/".to_vec())),
            "a URI stays a URI"
        );
    }
}

/// An outline on a document that had one replaces it item by item — the old
/// items are deleted, not left in the file with their titles — and the
/// destination names the page by reference, so moving the page afterwards
/// moves the destination with it.
#[test]
fn an_outline_replaces_the_old_one_and_follows_a_moved_page() {
    let mut builder = DocumentBuilder::new();
    for _ in 0..3 {
        builder.add_page(200.0, 300.0, |_| {});
    }
    assert!(builder.set_outline(vec![OutlineEntry {
        title: "OLD-TITLE".to_string(),
        target: None,
        open: false,
        children: Vec::new(),
    }]));
    let doc = Document::open(builder.finish()).expect("opens");
    assert_eq!(doc.outline().len(), 1);

    let mut editor = doc.editor();
    assert!(editor.set_outline(&outline()));
    assert!(editor.move_page(1, 0), "page 1 moves to the front");
    let (bytes, reopened) = saved(&editor, WriteMode::Rewrite, false);
    assert!(
        !String::from_utf8_lossy(&bytes).contains("OLD-TITLE"),
        "the replaced outline's items are gone from a rewrite"
    );
    assert!(matches!(
        reopened.outline()[0].destination,
        Some(Destination::Explicit {
            page_index: Some(0),
            ..
        })
    ));

    assert!(editor.set_outline(&[]), "an empty outline is accepted");
    for mode in MODES {
        let (_, reopened) = saved(&editor, mode, false);
        assert!(reopened.outline().is_empty(), "{mode:?}");
    }
}

#[test]
fn an_unwritable_outline_is_refused_whole() {
    let doc = plain();
    let mut editor = doc.editor();
    let bad = vec![OutlineEntry {
        title: "empty URI".to_string(),
        target: Some(Target::Uri(String::new())),
        open: false,
        children: Vec::new(),
    }];
    assert!(!editor.set_outline(&bad));
    assert!(!editor.is_dirty());
}

// ---- /Info and XMP ----------------------------------------------------------

const PACKET: &[u8] = b"<?xpacket begin=\"\xEF\xBB\xBF\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description rdf:about=\"\" \
xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">\
After</rdf:li></rdf:Alt></dc:title></rdf:Description></rdf:RDF></x:xmpmeta>\n\
<?xpacket end=\"w\"?>";

#[test]
fn every_info_entry_reads_back_typed() {
    let doc = plain();
    let mut editor = doc.editor();
    assert_eq!(editor.set_title("Après"), MetadataSync::Alone);
    assert_eq!(editor.set_author("Ada"), MetadataSync::Alone);
    assert_eq!(editor.set_subject("Σ subjects"), MetadataSync::Alone);
    assert_eq!(editor.set_keywords("a, b"), MetadataSync::Alone);
    assert_eq!(editor.set_creator("a word processor"), MetadataSync::Alone);
    assert_eq!(editor.set_producer("tinker-pdf"), MetadataSync::Alone);
    let created = date(2026, Some(-300));
    let modified = date(2026, None);
    assert_eq!(editor.set_creation_date(created), Some(MetadataSync::Alone));
    assert_eq!(
        editor.set_modification_date(modified),
        Some(MetadataSync::Alone)
    );
    assert_eq!(editor.set_trapped(Trapped::False), MetadataSync::Alone);
    assert_eq!(
        editor.set_creation_date(date(10_000, None)),
        None,
        "a year 7.9.4 has no digits for"
    );

    for mode in MODES {
        let (_, reopened) = saved(&editor, mode, false);
        let meta = reopened.metadata();
        assert_eq!(meta.title.as_deref(), Some("Après"), "{mode:?}");
        assert_eq!(meta.author.as_deref(), Some("Ada"));
        assert_eq!(meta.subject.as_deref(), Some("Σ subjects"));
        assert_eq!(meta.keywords.as_deref(), Some("a, b"));
        assert_eq!(meta.creator.as_deref(), Some("a word processor"));
        assert_eq!(meta.producer.as_deref(), Some("tinker-pdf"));
        assert_eq!(meta.created(), Some(created));
        assert_eq!(meta.modified(), Some(modified));
        assert_eq!(meta.trapped, Some(Trapped::False));
    }
}

/// The packet is written verbatim and never compressed, and each half of the
/// metadata says, on the way in, whether the other half was left as it was.
#[test]
fn a_caller_supplied_packet_is_written_verbatim_and_uncompressed() {
    let doc = plain();
    let mut editor = doc.editor();
    assert_eq!(
        editor.set_xmp_metadata(PACKET),
        Some(MetadataSync::OtherHalfUnchanged),
        "/Info already carries a /Title the packet was not checked against"
    );
    assert_eq!(
        editor.set_title("After"),
        MetadataSync::OtherHalfUnchanged,
        "and now a packet exists that /Info does not rewrite"
    );

    for mode in MODES {
        let (bytes, reopened) = saved(&editor, mode, true);
        assert_eq!(reopened.xmp_metadata().as_deref(), Some(PACKET), "{mode:?}");
        let cos = reopened.cos();
        let metadata = cos
            .catalog()
            .and_then(|c| c.get_ref(cos.intern(b"Metadata")))
            .expect("the catalog names the packet");
        let object = cos.get(metadata).expect("the stream");
        let dict = &object.as_stream().expect("a stream").dict;
        assert!(
            dict.get(cos.intern(b"Filter")).is_none(),
            "{mode:?}: a compressing save filtered the metadata stream"
        );
        assert_eq!(
            dict.get_name(cos.intern(b"Subtype"))
                .and_then(|n| cos.name_bytes(n))
                .as_deref(),
            Some(&b"XML"[..])
        );
        assert!(
            bytes.windows(PACKET.len()).any(|w| w == PACKET),
            "{mode:?}: the packet is in the file as bytes a scanner finds"
        );
    }

    // A document with no /Info at all gets a packet that disagrees with
    // nothing.
    let mut builder = DocumentBuilder::new();
    builder.add_page(100.0, 100.0, |_| {});
    let bare = Document::open(builder.finish()).expect("opens");
    let mut editor = bare.editor();
    assert_eq!(editor.set_xmp_metadata(PACKET), Some(MetadataSync::Alone));
}

// ---- viewer preferences -----------------------------------------------------

fn preferences() -> ViewerPreferences {
    ViewerPreferences {
        hide_toolbar: Some(true),
        hide_menubar: Some(false),
        hide_window_ui: Some(true),
        fit_window: Some(true),
        center_window: Some(true),
        display_doc_title: Some(true),
        non_full_screen_page_mode: Some(NonFullScreenPageMode::UseOutlines),
        direction: Some(ReadingDirection::RightToLeft),
        view_area: Some(PageBoundary::CropBox),
        view_clip: Some(PageBoundary::CropBox),
        print_area: Some(PageBoundary::TrimBox),
        print_clip: Some(PageBoundary::BleedBox),
        print_scaling: Some(PrintScaling::None),
        duplex: Some(Duplex::DuplexFlipShortEdge),
        pick_tray_by_pdf_size: Some(false),
        print_page_range: Some(vec![(1, 1), (3, 3)]),
        num_copies: Some(2),
        enforce: vec![EnforcedPreference::PrintScaling],
    }
}

#[test]
fn viewer_preferences_read_back_as_written() {
    let doc = plain();
    assert!(doc.viewer_preferences().is_empty());
    let mut editor = doc.editor();
    assert!(editor.set_viewer_preferences(&preferences()));
    for mode in MODES {
        let (_, reopened) = saved(&editor, mode, false);
        assert_eq!(reopened.viewer_preferences(), preferences(), "{mode:?}");
    }

    // Narrowing is writing: a field set to None is removed.
    let fewer = ViewerPreferences {
        direction: Some(ReadingDirection::LeftToRight),
        ..ViewerPreferences::default()
    };
    assert!(editor.set_viewer_preferences(&fewer));
    for mode in MODES {
        let (_, reopened) = saved(&editor, mode, false);
        assert_eq!(reopened.viewer_preferences(), fewer, "{mode:?}");
    }

    let backwards = ViewerPreferences {
        print_page_range: Some(vec![(3, 1)]),
        ..ViewerPreferences::default()
    };
    let before = format!("{:?}", editor.checkpoint());
    assert!(!editor.set_viewer_preferences(&backwards));
    assert_eq!(format!("{:?}", editor.checkpoint()), before);
}

// ---- trim, art and bleed boxes ----------------------------------------------

#[test]
fn production_boxes_read_back_after_both_saves() {
    let doc = plain();
    let page = doc.page(1).expect("page 1");
    assert_eq!(
        page.trim_box(),
        page.crop_box(),
        "the default is the crop box"
    );

    let mut editor = doc.editor();
    assert!(editor.set_trim_box(1, 10.0, 20.0, 190.0, 280.0));
    assert!(editor.set_art_box(1, 30.0, 40.0, 170.0, 260.0));
    assert!(
        editor.set_bleed_box(1, 205.0, 305.0, 5.0, 5.0),
        "corners are ordered"
    );
    assert!(editor.set_page_boundary(2, PageBoundary::TrimBox, -50.0, -50.0, 100.0, 100.0));
    assert!(!editor.set_trim_box(0, 10.0, 10.0, 10.0, 90.0), "no area");
    assert!(
        !editor.set_art_box(0, f64::NAN, 0.0, 1.0, 1.0),
        "not a number"
    );
    assert!(!editor.set_art_box(3, 0.0, 0.0, 1.0, 1.0), "no page 3");

    for mode in MODES {
        let (_, reopened) = saved(&editor, mode, false);
        let page = reopened.page(1).expect("page 1");
        assert_eq!(page.trim_box(), (10.0, 20.0, 190.0, 280.0), "{mode:?}");
        assert_eq!(page.art_box(), (30.0, 40.0, 170.0, 260.0));
        assert_eq!(
            page.bleed_box(),
            (5.0, 5.0, 200.0, 300.0),
            "written as given, read clipped to the media box (14.11.2.1)"
        );
        let other = reopened.page(2).expect("page 2");
        assert_eq!(other.trim_box(), (0.0, 0.0, 100.0, 100.0));
        assert_eq!(
            reopened.page(0).expect("page 0").trim_box(),
            (0.0, 0.0, 200.0, 300.0),
            "a page nobody touched keeps the default"
        );
    }
}

// ---- everything at once -----------------------------------------------------

/// Every setter on one editor, so no two of them can undo each other through
/// the catalog: each goes through `update_catalog`, and the last one to run
/// must not write back a catalog the first one had already changed.
#[test]
fn every_setter_on_one_editor_composes() {
    let doc = plain();
    let mut editor = doc.editor();
    editor.set_page_labels(&labels()).expect("labels");
    editor.attach_file(&csv()).expect("attachment");
    assert!(editor.set_outline(&outline()));
    let _ = editor.set_xmp_metadata(PACKET).expect("a catalog");
    let _ = editor.set_author("Ada");
    assert!(editor.set_viewer_preferences(&preferences()));
    assert!(editor.set_trim_box(0, 1.0, 1.0, 199.0, 299.0));

    for mode in MODES {
        let (_, reopened) = saved(&editor, mode, true);
        assert_eq!(reopened.page_labels(), ["i", "ii", "Anhang 7"], "{mode:?}");
        assert_eq!(reopened.attachments().len(), 1);
        assert_eq!(reopened.outline().len(), 2);
        assert_eq!(reopened.xmp_metadata().as_deref(), Some(PACKET));
        assert_eq!(reopened.metadata().author.as_deref(), Some("Ada"));
        assert_eq!(reopened.metadata().title.as_deref(), Some("Before"));
        assert_eq!(reopened.viewer_preferences(), preferences());
        assert_eq!(
            reopened.page(0).expect("page 0").trim_box(),
            (1.0, 1.0, 199.0, 299.0)
        );
        assert_eq!(reopened.page_count(), 3);
    }
}

// ---- sanitising what the editor wrote -----------------------------------------

/// `sanitise` through the facade, over what the setters above wrote: the
/// attachment, the packet and `/Info` leave, the outline's web link leaves
/// its item, and the report's every type is nameable from here.
#[test]
fn sanitising_takes_back_out_what_the_editor_wrote() {
    let doc = plain();
    let mut editor = doc.editor();
    editor.attach_file(&csv()).expect("attachment");
    let _ = editor.set_xmp_metadata(PACKET).expect("a catalog");
    let _ = editor.set_author("Ada");
    assert!(editor.set_outline(&outline()));

    let report: SanitiseReport = editor.sanitise(&Sanitise::ALL);
    assert!(report.removed.contains(&RemovedEntry {
        holder: EntryHolder::Trailer,
        path: vec![PathStep::Key(b"Info".to_vec())],
        what: Removal::Info,
    }));
    // The embedded file stream is deleted, for the attachment tree that was
    // the first removed entry to reach it.
    assert!(report
        .deleted
        .iter()
        .any(|d: &DeletedObject| d.what == Removal::EmbeddedFileTree));

    for mode in MODES {
        let (bytes, reopened) = saved(&editor, mode, false);
        assert!(reopened.attachments().is_empty(), "{mode:?}");
        assert_eq!(reopened.xmp_metadata(), None);
        assert_eq!(reopened.metadata(), tinker_pdf::Metadata::default());
        assert!(reopened.script_summary().is_empty());
        let items = reopened.outline();
        assert_eq!(items.len(), 2, "the outline itself stays");
        assert!(
            items[1].destination.is_none(),
            "the item whose target left the document goes nowhere now"
        );
        if mode == WriteMode::Rewrite {
            assert!(!bytes.windows(9).any(|w| w == b"quarter,r"), "the CSV left");
        }
    }
}
