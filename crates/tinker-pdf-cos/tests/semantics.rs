//! Document semantics against the real fixtures: metadata, page geometry,
//! outlines and destinations.
//!
//! The outline assertions match Tinker's own `outline.rs` test exactly — the
//! same fixture, the same zero-based page numbers — because that suite is the
//! parity bar this engine is measured against.

use std::path::PathBuf;

use tinker_pdf_cos::{outline, pages, CosDocument, DestKind, Destination};

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

fn open(name: &str) -> CosDocument {
    CosDocument::open(fixture(name)).expect("the fixture opens")
}

#[test]
fn a4_geometry_is_reported_in_points() {
    let doc = open("simple-text.pdf");
    let pages = pages::collect(&doc);

    assert_eq!(pages.len(), 3);
    for (i, page) in pages.iter().enumerate() {
        assert_eq!(page.index, i as u32, "indices are sequential from zero");
        assert!(
            (page.media_box.width() - 595.0).abs() < 1.0,
            "A4 is 595 points wide, got {}",
            page.media_box.width()
        );
        assert!(
            (page.media_box.height() - 842.0).abs() < 1.0,
            "A4 is 842 points tall, got {}",
            page.media_box.height()
        );
        assert_eq!(page.rotation, 0);
        assert_eq!(
            page.crop_box, page.media_box,
            "an absent /CropBox defaults to /MediaBox"
        );
        assert_eq!(
            page.display_size(),
            (page.crop_box.width(), page.crop_box.height())
        );
    }
}

#[test]
fn page_count_agrees_with_the_walk() {
    assert_eq!(pages::count(&open("simple-text.pdf")), 3);
    assert_eq!(pages::count(&open("outline-3level.pdf")), 6);
}

#[test]
fn every_page_has_reachable_content() {
    let doc = open("simple-text.pdf");
    for page in pages::collect(&doc) {
        let bytes = pages::content_bytes(&doc, &page);
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            text.contains("BT"),
            "page {} should hold text operators",
            page.index
        );
    }
}

#[test]
fn the_version_string_matches_the_header() {
    let doc = open("simple-text.pdf");
    assert_eq!(outline::version_string(&doc).as_deref(), Some("PDF 1.7"));
}

/// The fixture's `/Info` holds `/Producer` and nothing else, so it pins the
/// absent half of the rule only. The present-and-empty half needs a document
/// no producer writes, and lives beside the code in `outline.rs`.
#[test]
fn metadata_reports_absent_for_keys_the_file_omits() {
    let doc = open("simple-text.pdf");
    let meta = outline::metadata(&doc);

    // The fixture is generated, so its producer is set and its subject is not.
    assert!(
        meta.producer.is_some() || meta.creator.is_some(),
        "a generated file names its producer or creator"
    );
    for (label, value) in [
        ("title", &meta.title),
        ("author", &meta.author),
        ("subject", &meta.subject),
        ("keywords", &meta.keywords),
    ] {
        assert_eq!(value, &None, "{label} is not in this file's /Info");
    }
}

/// The nesting and page numbers Tinker's own outline test pins.
#[test]
fn the_outline_nests_three_levels_with_correct_pages() {
    let doc = open("outline-3level.pdf");
    let items = outline::outline(&doc);
    let flat = tinker_pdf_cos::OutlineItem::flatten(&items);

    let seen: Vec<(u32, String, Option<u32>)> = flat
        .iter()
        .map(|(depth, item)| {
            let page = match &item.destination {
                Some(Destination::Explicit { page_index, .. }) => *page_index,
                _ => None,
            };
            (*depth, item.title.clone(), page)
        })
        .collect();

    let expected = [
        (0u32, "Part One", Some(0u32)),
        (1, "Chapter 1", Some(1)),
        (2, "Section 1.1", Some(2)),
        (0, "Part Two", Some(4)),
    ];

    for (depth, title, page) in expected {
        assert!(
            seen.iter()
                .any(|(d, t, p)| *d == depth && t == title && *p == page),
            "expected {title:?} at depth {depth} on page {page:?}; got {seen:#?}"
        );
    }
}

#[test]
fn a_document_without_an_outline_returns_an_empty_one() {
    let doc = open("simple-text.pdf");
    assert!(
        outline::outline(&doc).is_empty(),
        "no outline is an empty outline, not an error"
    );
}

/// Ruling 6, on real data: an explicit destination stays explicit.
#[test]
fn outline_destinations_are_explicit_not_named() {
    let doc = open("outline-3level.pdf");
    for item in tinker_pdf_cos::OutlineItem::flatten(&outline::outline(&doc)) {
        match &item.1.destination {
            Some(Destination::Explicit { kind, .. }) => {
                // The fixture writes /Fit destinations.
                assert!(
                    matches!(kind, DestKind::Fit | DestKind::Xyz { .. }),
                    "unexpected destination kind {kind:?}"
                );
            }
            Some(other) => panic!("an explicit destination was read as {other:?}"),
            None => panic!("outline entry {:?} lost its destination", item.1.title),
        }
    }
}

#[test]
fn page_labels_are_absent_when_the_document_defines_none() {
    let doc = open("simple-text.pdf");
    assert!(outline::page_labels(&doc, 3).is_empty());
}

#[test]
fn reading_semantics_adds_no_warnings_to_a_clean_file() {
    let doc = open("simple-text.pdf");
    let before = doc.warnings().len();

    let _ = pages::collect(&doc);
    let _ = outline::outline(&doc);
    let _ = outline::metadata(&doc);

    assert_eq!(
        doc.warnings().len(),
        before,
        "an honest file should provoke no leniency: {:?}",
        doc.warnings()
    );
}
