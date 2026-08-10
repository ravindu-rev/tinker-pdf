//! Parity with the engine being replaced.
//!
//! These are ports of the assertions in Tinker's own suite —
//! `open_documents.rs`, `text_and_search.rs`, `outline.rs` — against the same
//! four fixtures. Ruling 12: parity is a `cargo test` invocation, not a
//! judgement call.
//!
//! The rendering assertions from `render_pages.rs` are here too, including
//! the A4-at-150-dpi size Tinker pins. `visual_regression.rs` is not: it
//! compares against goldens that would have to be regenerated against this
//! engine, which is integration work rather than parity work.

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
    let doc = open("encrypted-aes256.pdf");
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
    let doc = open("permissions-noprint.pdf");
    assert_eq!(doc.authenticate("user"), Ok(AuthLevel::User));

    let p = doc.permissions();
    assert!(!p.print(), "this document forbids printing");
    assert!(p.copy(), "and permits copying");
}

/// Beyond Tinker's suite: the owner password is distinguishable, which its
/// engine could not manage.
#[test]
fn owner_authentication_is_distinguishable() {
    let doc = open("encrypted-aes256.pdf");
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

// --- render_pages.rs -------------------------------------------------------

/// Tinker: `renders_a4_at_one_pixel_per_point`.
#[test]
fn renders_a4_at_one_pixel_per_point() {
    let doc = open("simple-text.pdf");
    let page = doc.page(0).expect("a page");
    let bitmap = page.render(&tinker_pdf::RenderOptions::default());

    assert_eq!((bitmap.width, bitmap.height), (595, 842));
    assert_eq!(bitmap.components(), 3, "colour renders are RGB");
    assert_eq!(bitmap.data.len(), 595 * 842 * 3);
}

/// Tinker: `scale_multiplies_both_dimensions`, including the 150 dpi case
/// whose outward rounding the previous engine left as folklore.
#[test]
fn scale_multiplies_both_dimensions() {
    let doc = open("simple-text.pdf");
    let page = doc.page(0).expect("a page");

    let doubled = page.render(&tinker_pdf::RenderOptions {
        scale: 2.0,
        ..tinker_pdf::RenderOptions::default()
    });
    assert_eq!((doubled.width, doubled.height), (1190, 1684));

    // A4 at 150 dpi is 842 × 150/72 = 1754.17, rounded outward.
    let at_150 = page.render(&tinker_pdf::RenderOptions::at_dpi(150.0));
    assert_eq!((at_150.width, at_150.height), (1240, 1755));
}

/// Tinker: `grayscale_renders_one_component_per_pixel`.
#[test]
fn grayscale_renders_one_component_per_pixel() {
    let doc = open("simple-text.pdf");
    let bitmap = doc
        .page(0)
        .expect("a page")
        .render(&tinker_pdf::RenderOptions {
            format: tinker_pdf::PixelFormat::Gray8,
            ..tinker_pdf::RenderOptions::default()
        });

    assert_eq!(bitmap.components(), 1);
    assert_eq!(bitmap.data.len(), (bitmap.width * bitmap.height) as usize);
}

/// Tinker: `rendering_the_same_page_twice_is_consistent`.
#[test]
fn rendering_the_same_page_twice_is_consistent() {
    let doc = open("simple-text.pdf");
    let page = doc.page(1).expect("a page");

    let first = page.render(&tinker_pdf::RenderOptions::default());
    let second = page.render(&tinker_pdf::RenderOptions::default());
    assert_eq!(first.data, second.data, "ruling 4: determinism");
}

/// Tinker: nonsense options are rejected rather than producing nonsense.
#[test]
fn nonsense_scales_produce_a_usable_bitmap() {
    let doc = open("simple-text.pdf");
    let page = doc.page(0).expect("a page");

    for scale in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let bitmap = page.render(&tinker_pdf::RenderOptions {
            scale,
            ..tinker_pdf::RenderOptions::default()
        });
        assert!(
            bitmap.width > 0 && bitmap.height > 0,
            "scale {scale} should fall back rather than produce nothing"
        );
    }
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
    let doc = open("encrypted-aes256.pdf");
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

/// Rendering must put ink on the page, not merely produce a bitmap of the
/// right size — a blank render passes every dimension assertion above.
///
/// Vector content, because the fixtures use base-14 fonts and carry no
/// embedded font program: there is no outline to draw for their text, and
/// substituting one needs a bundled face that this engine does not yet ship.
/// See `docs/STATUS.md`.
#[test]
fn a_rendered_page_actually_has_marks_on_it() {
    use tinker_pdf_cos::DocumentBuilder;

    let mut builder = DocumentBuilder::new();
    builder.add_page(100.0, 100.0, |page| {
        page.fill_rect(10.0, 10.0, 50.0, 30.0, 0.0);
    });
    let doc = Document::open(builder.finish()).expect("the built document opens");

    let bitmap = doc
        .page(0)
        .expect("a page")
        .render(&tinker_pdf::RenderOptions::default());

    let dark = bitmap.data.iter().filter(|&&b| b < 200).count();
    assert!(
        dark > 1000,
        "a 50x30 filled rectangle should darken thousands of subpixels, got {dark}"
    );
}

/// Regression: taking a page must not break authentication.
///
/// `Document` is `Clone` and every `Page` holds a clone of the same `Arc`, so
/// an `Arc::get_mut` inside `authenticate` fails the moment a caller has done
/// the most ordinary thing in the API — looked at a page — and reports
/// `NotEncrypted` for a document that is plainly encrypted.
#[test]
fn authenticating_still_works_after_taking_a_page() {
    let doc = open("encrypted-aes256.pdf");
    let _page = doc.page(0);
    assert_eq!(doc.authenticate("open-sesame"), Ok(AuthLevel::User));
}

/// A caller opening a file needs to tell "this is not a PDF" from "this is a
/// PDF and it wants a password". `Document::open` collapsed every distinct
/// failure into `NotAPdf`, so an encrypted document — which opens perfectly
/// well — was indistinguishable from a corrupt one.
#[test]
fn an_encrypted_document_says_it_wants_a_password() {
    use tinker_pdf::DocumentError;

    let doc = open("encrypted-aes256.pdf");
    assert_eq!(doc.readable(), Err(DocumentError::PasswordRequired));

    assert_eq!(doc.authenticate("open-sesame"), Ok(AuthLevel::User));
    assert_eq!(doc.readable(), Ok(()), "and is readable once it has one");
}

#[test]
fn an_unencrypted_document_is_readable_immediately() {
    assert_eq!(open("simple-text.pdf").readable(), Ok(()));
}

/// Empty input is a caller's bug far more often than a bad document, and
/// saying so is the difference between checking the file and checking the code.
#[test]
fn empty_bytes_are_distinguished_from_a_bad_document() {
    use tinker_pdf::{Document, OpenError};

    assert_eq!(Document::open(Vec::new()).unwrap_err(), OpenError::Empty);
    assert_eq!(
        Document::open(b"this is not a pdf at all".to_vec()).unwrap_err(),
        OpenError::NotAPdf
    );
}

/// Ruling 2 applies to text as much as to pixels. A page whose `Tf` names a
/// font the resource dictionary does not define extracted an empty string and
/// said nothing, so "this page has no text" and "this page has text this build
/// could not decode" were indistinguishable.
#[test]
fn extraction_reports_a_font_it_could_not_resolve() {
    use tinker_pdf::{Document, TextWarning};

    let bytes = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100]\n\
   /Resources << >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length 36 >>\nstream\n\
BT /Nonesuch 12 Tf 10 50 Td (hi) Tj ET\n\
endstream\nendobj\n\
trailer\n<< /Size 5 /Root 1 0 R >>\n%%EOF\n";

    let doc = Document::open(bytes.to_vec()).expect("it opens");
    let text = doc.page(0).expect("a page").text();

    assert!(text.plain_text().is_empty(), "nothing could be decoded");
    assert!(
        text.warnings
            .iter()
            .any(|w| matches!(w, TextWarning::UnknownFont { name } if name == "Nonesuch")),
        "and it says which font was missing: {:?}",
        text.warnings
    );
}

/// A page whose fonts all resolve reports nothing, so the warning list stays
/// meaningful rather than becoming noise every caller learns to ignore.
#[test]
fn extraction_of_a_sound_page_reports_nothing() {
    let text = open("simple-text.pdf").page(0).expect("a page").text();
    assert!(!text.plain_text().is_empty());
    assert!(text.warnings.is_empty(), "got {:?}", text.warnings);
}
