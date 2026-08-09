//! Parity with the engine being replaced.
//!
//! These are ports of the assertions in Tinker's own suite —
//! `open_documents.rs`, `text_and_search.rs`, `outline.rs` — against the same
//! four fixtures. Ruling 12: parity is a `cargo test` invocation, not a
//! judgement call.
//!
//! The rendering half of the parity bar (`render_pages.rs`,
//! `visual_regression.rs`) arrives with Checkpoint B; what is here is
//! Checkpoint A: everything Tinker consumes except pixels.

use std::path::PathBuf;

use tinker_pdf::{AuthError, AuthLevel, Destination, Document};

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

fn open(name: &str) -> Document {
    Document::open(fixture(name)).unwrap_or_else(|e| panic!("opening {name}: {e}"))
}

// --- open_documents.rs -----------------------------------------------------

/// Tinker: `opens_a_pdf_and_reports_its_shape`.
#[test]
fn opens_a_pdf_and_reports_its_shape() {
    let doc = open("simple-text.pdf");

    assert_eq!(doc.page_count(), 3);
    assert!(!doc.is_encrypted());
    assert_eq!(doc.auth_level(), AuthLevel::None);
    assert!(doc.permissions().print());
    assert_eq!(doc.pdf_version().as_deref(), Some("PDF 1.7"));
}

/// Tinker: `page_geometry_covers_every_page`.
#[test]
fn page_geometry_covers_every_page() {
    let doc = open("simple-text.pdf");
    let pages = doc.pages();

    assert_eq!(pages.len(), 3);
    for (i, page) in pages.iter().enumerate() {
        assert_eq!(page.index(), i as u32, "indices are sequential from zero");
        let (w, h) = page.size();
        assert!((w - 595.0).abs() < 1.0, "A4 width, got {w}");
        assert!((h - 842.0).abs() < 1.0, "A4 height, got {h}");
    }
}

/// Tinker: `encrypted_documents_ask_for_a_password_before_anything_else`.
#[test]
fn encrypted_documents_need_their_password() {
    let mut doc = open("encrypted-aes256.pdf");
    assert!(doc.is_encrypted());

    assert_eq!(
        doc.authenticate("wrong"),
        Err(AuthError::WrongPassword),
        "a bad password is refused"
    );
    assert_eq!(doc.authenticate("open-sesame"), Ok(AuthLevel::User));
}

/// Tinker: `permission_flags_are_reported_under_user_authentication`.
///
/// The test that caught the bug in the engine being replaced: `/P`'s reserved
/// bits made its bitflags parse fail, and the fallback granted everything.
#[test]
fn permission_flags_are_reported_under_user_authentication() {
    let mut doc = open("permissions-noprint.pdf");
    assert_eq!(doc.authenticate("user"), Ok(AuthLevel::User));

    let p = doc.permissions();
    assert!(!p.print(), "this document forbids printing");
    assert!(p.copy(), "and permits copying");
}

/// Beyond Tinker's suite: the owner password is distinguishable, which its
/// engine could not manage.
#[test]
fn owner_authentication_is_distinguishable() {
    let mut doc = open("encrypted-aes256.pdf");
    assert_eq!(doc.authenticate("owner-secret"), Ok(AuthLevel::Owner));
    assert!(doc.auth_level() > AuthLevel::User);
    assert!(doc.permissions().print(), "the owner password lifts limits");
}

// --- text_and_search.rs ----------------------------------------------------

/// Tinker: extraction contains the written string, and the line's quad sits
/// inside the page.
#[test]
fn extraction_finds_the_pages_text() {
    let doc = open("simple-text.pdf");
    let page = doc.page(0).expect("a first page");
    let text = page.text();

    let plain = text.plain_text();
    assert!(
        plain.contains("Tinker fixture, page 1 of 3"),
        "extracted {plain:?}"
    );

    let line = text.lines().first().copied().expect("a line");
    let (x0, y0, x1, y1) = line.quad.bounds();
    let (w, h) = page.size();
    assert!(x0 >= 0.0 && x1 <= w, "line x range {x0}..{x1} within {w}");
    assert!(y0 >= 0.0 && y1 <= h, "line y range {y0}..{y1} within {h}");
    assert!(line.size > 0.0, "a line has a size");
}

/// Tinker: each page reports its own text.
#[test]
fn every_page_reports_its_own_text() {
    let doc = open("simple-text.pdf");
    for (i, page) in doc.pages().iter().enumerate() {
        let want = format!("page {} of 3", i + 1);
        let plain = page.text().plain_text();
        assert!(plain.contains(&want), "page {i} extracted {plain:?}");
    }
}

/// Tinker: `search_returns_one_hit_with_context`.
#[test]
fn search_finds_a_phrase_on_its_page() {
    let doc = open("simple-text.pdf");
    let page = doc.page(2).expect("the third page");
    let hits = page.text().search("page 3 of 3");

    assert_eq!(hits.len(), 1, "one hit on the page that has it");
    let (x0, y0, x1, y1) = hits.first().expect("a hit").bounds();
    assert!(x1 > x0 && y1 > y0, "the quad is non-degenerate");
}

/// Tinker: search is case-insensitive by default.
#[test]
fn search_is_case_insensitive() {
    let doc = open("simple-text.pdf");
    let text = doc.page(0).expect("a page").text();

    assert_eq!(text.search("tinker").len(), 1);
    assert_eq!(text.search("TINKER").len(), 1);
    assert_eq!(text.search("Tinker").len(), 1);
}

/// Tinker: an empty query finds nothing.
#[test]
fn an_empty_query_finds_nothing() {
    let doc = open("simple-text.pdf");
    assert!(doc.page(0).expect("a page").text().search("").is_empty());
}

/// Tinker: a page index past the end has no page.
#[test]
fn a_page_past_the_end_does_not_exist() {
    let doc = open("simple-text.pdf");
    assert!(doc.page(99).is_none());
}

// --- outline.rs ------------------------------------------------------------

/// Tinker: three-level nesting with zero-based absolute page numbers.
#[test]
fn the_outline_nests_with_zero_based_page_numbers() {
    let doc = open("outline-3level.pdf");
    let items = doc.outline();
    let flat = tinker_pdf::OutlineItem::flatten(&items);

    let seen: Vec<(u32, &str, Option<u32>)> = flat
        .iter()
        .map(|(depth, item)| {
            let page = match &item.destination {
                Some(Destination::Explicit { page_index, .. }) => *page_index,
                _ => None,
            };
            (*depth, item.title.as_str(), page)
        })
        .collect();

    for want in [
        (0u32, "Part One", Some(0u32)),
        (1, "Chapter 1", Some(1)),
        (2, "Section 1.1", Some(2)),
        (0, "Part Two", Some(4)),
    ] {
        assert!(seen.contains(&want), "expected {want:?}; got {seen:#?}");
    }
}

/// Tinker: documents without an outline return an empty vec, not an error.
#[test]
fn documents_without_an_outline_return_an_empty_one() {
    assert!(open("simple-text.pdf").outline().is_empty());
}

// --- engine behaviour beyond the ported suite ------------------------------

#[test]
fn a_clean_file_opens_without_leniency() {
    let doc = open("simple-text.pdf");
    assert_eq!(doc.ladder_level(), tinker_pdf::LadderLevel::Trust);
    assert!(
        doc.warnings().is_empty(),
        "an honest file provokes no warnings: {:?}",
        doc.warnings()
    );
}

#[test]
fn an_encrypted_documents_text_is_readable_once_unlocked() {
    let mut doc = open("encrypted-aes256.pdf");
    assert_eq!(doc.authenticate("open-sesame"), Ok(AuthLevel::User));

    let plain = doc.page(0).expect("a page").text().plain_text();
    assert!(
        plain.contains("encrypted with AES-256"),
        "decrypted text should be readable, got {plain:?}"
    );
}

#[test]
fn documents_are_usable_from_several_threads() {
    // The property that dissolves the actor model the previous engine forced:
    // a Document is Send + Sync and readable concurrently.
    let doc = open("simple-text.pdf");
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let doc = doc.clone();
            std::thread::spawn(move || {
                for _ in 0..5 {
                    let page = doc.page(0).expect("a page");
                    assert!(page.text().plain_text().contains("Tinker"));
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("no thread panicked");
    }
}
