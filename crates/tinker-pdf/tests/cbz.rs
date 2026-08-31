//! A comic archive opens as a document (gap 29, milestone 5).
//!
//! # What each comparison here is worth, and what it is not
//!
//! Gap 18 milestone 7's scar is a comparison that passed because both of its
//! sides rendered through one defect, so it is worth writing down which parts
//! of these are shared before claiming any of them proves anything.
//!
//! The synthesised side goes: archive bytes → `tinker-pdf-zip` → magic-byte
//! classification → natural order → `png_image` or `jpeg_shape` →
//! `DocumentBuilder` → `CosDocument::open` → the renderer. The hand-built side
//! is **PDF source text**, assembled in `cbz_support` with no `DocumentBuilder`
//! anywhere, and read by the same parser and renderer. So the parser, the
//! renderer and the image codecs are shared; the archive reader, the ordering,
//! the classification, the page geometry and the whole of the synthesis are
//! not, and those are what this file is for.
//!
//! Where a picture is asserted, it is also asserted against **literal expected
//! pixels** written out here rather than only against another render, because
//! two blank pages compare equal.

mod cbz_support;

use cbz_support::{
    broken_png, distinct_pixels, grey_jpeg, one_image_page, pdf, rgb_png, stream, zip, Damage,
    ZipFile,
};
use tinker_pdf::cbz::{self, zip_limits, ArchiveWarning, ImageFormat, ZipEntryError, ZipWarning};
use tinker_pdf::{
    ArchiveRefusal, Bitmap, Container, Document, OpenError, PageDefect, RenderOptions, WriteMode,
    WriteOptions,
};
use tinker_pdf_filters::png_scan;

// ---- helpers -----------------------------------------------------------

fn open(archive: &[u8]) -> Document {
    Document::open(archive.to_vec()).expect("the archive opens as a document")
}

fn render(document: &Document, page: u32) -> Bitmap {
    document
        .page(page)
        .expect("a page")
        .render(&RenderOptions::default())
}

fn pixel(bitmap: &Bitmap, x: u32, y: u32) -> (u8, u8, u8) {
    let at = (y as usize) * bitmap.stride + (x as usize) * bitmap.components();
    let p = bitmap.data.get(at..at + 3).unwrap_or(&[0, 0, 0]);
    (p[0], p[1], p[2])
}

/// A page of `w` x `h` whose every pixel differs from every other.
fn page_png(w: u32, h: u32) -> Vec<u8> {
    rgb_png(w, h, &distinct_pixels(w, h))
}

/// The names of an opened archive's pages, in page order.
fn page_names(document: &Document) -> Vec<String> {
    document
        .archive()
        .expect("a synthesised document reports where its pages came from")
        .pages()
        .iter()
        .map(|p| p.name.clone())
        .collect()
}

// ---- The sniff ---------------------------------------------------------

/// `PK\x03\x04` at offset zero, and nowhere else.
///
/// A PDF may carry those four bytes anywhere — an embedded file, a compressed
/// object stream, a font program — and a sniff that searched for them would
/// turn a legal document into a refused archive.
#[test]
fn a_pdf_carrying_the_zip_signature_is_still_a_pdf() {
    // The signature appears twice: once inside a stream and once as a literal
    // string, which is where a `.docx` attachment would put it.
    let carrier = pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Contents 4 0 R >>".to_vec(),
        stream("", b"PK\x03\x04 and some more PK\x03\x04\n"),
    ]);
    assert!(
        carrier.windows(4).any(|w| w == b"PK\x03\x04"),
        "the fixture has to contain the signature or it proves nothing"
    );

    let document = Document::open(carrier).expect("an ordinary PDF");
    assert_eq!(document.page_count(), 1);
    assert!(
        document.archive().is_none(),
        "and it was parsed, not synthesised"
    );
}

/// The containers this build does not read are refused **by name**.
///
/// "This is a CBR and I do not read CBR" is a different sentence from "this is
/// not a PDF", and a host shows different things for them.
///
/// *Amended in tier 4's W-ARCHIVE lane.* The tar left this list when it gained
/// a reader, and moved to the test below rather than being deleted — a row
/// here may only ever move in that direction.
#[test]
fn a_cbr_and_a_cb7_are_refused_by_name() {
    for (what, bytes) in [
        (Container::Rar, b"Rar!\x1a\x07\x00rest of it".to_vec()),
        (Container::SevenZip, b"7z\xbc\xaf\x27\x1crest".to_vec()),
    ] {
        assert_eq!(cbz::container(&bytes), Some(what));
        assert_eq!(
            Document::open(bytes).err(),
            Some(OpenError::UnsupportedArchive(ArchiveRefusal::NotAZip)),
            "{what:?}"
        );
    }
}

/// A `ustar` magic behind a header that does not checksum is **recognised, read
/// and empty** — which is a different refusal from the one it used to get.
///
/// Before this build had a tar reader the answer was `NotAZip`, and the
/// distinction is worth a test rather than a line: a host that is told "this is
/// a container I do not read" shows a different thing from one told "I read
/// this and it holds no pages". POSIX 1003.1's header checksum is what
/// separates the two here — the block below has a magic and no valid sum, so
/// the walk ends before its first entry.
#[test]
fn a_tar_whose_first_header_does_not_checksum_holds_no_pages() {
    let mut block = vec![0u8; 1024];
    block[..8].copy_from_slice(b"page1.jp");
    block[257..262].copy_from_slice(b"ustar");
    assert_eq!(cbz::container(&block), Some(Container::Tar));
    assert_eq!(
        Document::open(block).err(),
        Some(OpenError::UnsupportedArchive(ArchiveRefusal::NoImages)),
    );
}

// ---- Pages, order and geometry -----------------------------------------

/// The exit criterion's own fixture: stored `p10, p1, p2`, paged 1, 2, 10.
///
/// The names are deliberately **unpadded**. A zero-padded archive sorts
/// identically under natural and lexicographic order, so a fixture with padded
/// names proves nothing about this — `cbz::tests` holds that fact as an
/// assertion of its own.
#[test]
fn an_archive_stored_as_p10_p1_p2_pages_as_one_two_ten() {
    let archive = zip(
        &[
            ZipFile::stored("p10.png", &page_png(4, 3)),
            ZipFile::stored("p1.png", &page_png(5, 3)),
            ZipFile::stored("p2.png", &page_png(6, 3)),
        ],
        Damage::None,
    );

    let document = open(&archive);
    assert_eq!(page_names(&document), ["p1.png", "p2.png", "p10.png"]);

    // The widths differ per page, so the order is visible in the geometry and
    // not only in a name the synthesiser could have copied from anywhere.
    let sizes: Vec<(f64, f64)> = document
        .pages()
        .iter()
        .map(tinker_pdf::Page::size)
        .collect();
    assert_eq!(sizes, [(5.0, 3.0), (6.0, 3.0), (4.0, 3.0)]);
}

/// Only image entries are pages, and everything else is silently skipped.
///
/// `ComicInfo.xml` and friends are metadata and noise: skipping them is correct
/// rather than lenient, and warning about each would bury the warnings that
/// matter.
#[test]
fn metadata_and_noise_are_not_pages_and_not_warnings() {
    let archive = zip(
        &[
            ZipFile::stored("ComicInfo.xml", b"<?xml version=\"1.0\"?><ComicInfo/>"),
            ZipFile::stored("pages/", b""),
            ZipFile::stored("pages/01.png", &page_png(3, 2)),
            ZipFile::stored(
                "Thumbs.db",
                &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1],
            ),
            ZipFile::stored("__MACOSX/._01.png", &[0x00, 0x05, 0x16, 0x07, 0x00, 0x02]),
            ZipFile::stored("pages/02.png", &page_png(3, 2)),
            ZipFile::stored(
                ".DS_Store",
                &[0x00, 0x00, 0x00, 0x01, b'B', b'u', b'd', b'1'],
            ),
        ],
        Damage::None,
    );

    let document = open(&archive);
    assert_eq!(document.page_count(), 2);
    assert_eq!(page_names(&document), ["pages/01.png", "pages/02.png"]);
    assert!(
        document.archive().expect("a report").warnings().is_empty(),
        "a healthy archive opens cleanly: {:?}",
        document.archive().expect("a report").warnings()
    );
}

/// The extension is a claim; the first bytes are a fact.
#[test]
fn a_png_called_jpg_is_read_as_the_png_it_is() {
    let archive = zip(
        &[ZipFile::stored("cover.jpg", &page_png(4, 6))],
        Damage::None,
    );
    let document = open(&archive);
    assert_eq!(document.page_count(), 1);
    // A PNG read as a JPEG would have no shape at all and would be a
    // placeholder of the fallback paper size.
    assert_eq!(
        document.page(0).expect("a page").size(),
        (4.0, 6.0),
        "the PNG's own dimensions, not US Letter"
    );
}

/// One image pixel is one PDF point, so `scale: 1.0` is the identity.
#[test]
fn a_page_is_the_images_own_pixel_size() {
    // Deliberately not square: an implementation that took the geometry from
    // the wrong axis renders a 7 x 3 page and fails here.
    let archive = zip(&[ZipFile::stored("p.png", &page_png(3, 7))], Damage::None);
    let document = open(&archive);
    let page = document.page(0).expect("a page");

    assert_eq!(page.size(), (3.0, 7.0));
    assert_eq!(page.media_box(), (0.0, 0.0, 3.0, 7.0));
    assert_eq!(page.rotation(), 0);

    let bitmap = render(&document, 0);
    assert_eq!((bitmap.width, bitmap.height), (3, 7));
}

/// `at_dpi(144)` doubles, which is what the 72-dpi premise means for a format
/// that has no physical size at all.
#[test]
fn twice_seventy_two_dpi_is_twice_the_pixels() {
    let archive = zip(&[ZipFile::stored("p.png", &page_png(3, 7))], Damage::None);
    let document = open(&archive);
    let bitmap = document
        .page(0)
        .expect("a page")
        .render(&RenderOptions::at_dpi(144.0));
    assert_eq!((bitmap.width, bitmap.height), (6, 14));
}

// ---- The picture -------------------------------------------------------

/// The default render is the archive's own pixels, one for one.
///
/// This is the strongest form available: the expected values are the bytes the
/// fixture put in, not another render. A comparison against a second document
/// can pass on two blank pages; this cannot.
#[test]
fn the_default_render_is_the_pixels_the_archive_holds() {
    let (w, h) = (5u32, 4u32);
    let pixels = distinct_pixels(w, h);
    let archive = zip(
        &[ZipFile::stored("p.png", &rgb_png(w, h, &pixels))],
        Damage::None,
    );

    let bitmap = render(&open(&archive), 0);
    assert_eq!((bitmap.width, bitmap.height), (w, h));
    for y in 0..h {
        for x in 0..w {
            let at = ((y * w + x) * 3) as usize;
            assert_eq!(
                pixel(&bitmap, x, y),
                (pixels[at], pixels[at + 1], pixels[at + 2]),
                "pixel ({x}, {y})"
            );
        }
    }
}

/// And the same page embedded by hand renders byte for byte the same.
///
/// The hand-built side is PDF source text with the raw samples as an
/// uncompressed `/DeviceRGB` stream — no `/Filter`, no predictor, no
/// `DocumentBuilder` — so it shares the parser and the renderer with the
/// synthesised side and nothing else.
#[test]
fn a_page_renders_identically_to_the_same_image_embedded_by_hand() {
    let (w, h) = (5u32, 4u32);
    let pixels = distinct_pixels(w, h);
    let archive = zip(
        &[ZipFile::stored("p.png", &rgb_png(w, h, &pixels))],
        Damage::None,
    );

    let synthesised = render(&open(&archive), 0);
    let by_hand = Document::open(one_image_page(
        w,
        h,
        &format!(
            "/Type /XObject /Subtype /Image /Width {w} /Height {h} \
             /BitsPerComponent 8 /ColorSpace /DeviceRGB"
        ),
        &pixels,
    ))
    .expect("the hand-written document opens");

    let expected = render(&by_hand, 0);
    assert_eq!(
        (synthesised.width, synthesised.height),
        (expected.width, expected.height)
    );
    assert_eq!(synthesised.data, expected.data, "byte for byte");
    assert!(
        synthesised.warnings.is_empty() && expected.warnings.is_empty(),
        "and neither was degraded"
    );
    // A blank page would satisfy the equality above, so the fixture is proved
    // to have painted something first.
    let distinct: std::collections::BTreeSet<_> = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .map(|(x, y)| pixel(&synthesised, x, y))
        .collect();
    assert_eq!(distinct.len(), (w * h) as usize, "every pixel differs");
}

/// A JPEG's bytes reach the page exactly as the archive holds them.
#[test]
fn a_jpeg_page_is_the_same_picture_as_the_jpeg_embedded_by_hand() {
    let jpeg = grey_jpeg(16, 8);
    let archive = zip(&[ZipFile::stored("p.jpg", &jpeg)], Damage::None);

    let document = open(&archive);
    assert_eq!(document.page(0).expect("a page").size(), (16.0, 8.0));

    let synthesised = render(&document, 0);
    let by_hand = Document::open(one_image_page(
        16,
        8,
        "/Type /XObject /Subtype /Image /Width 16 /Height 8 /BitsPerComponent 8 \
         /ColorSpace /DeviceGray /Filter /DCTDecode",
        &jpeg,
    ))
    .expect("the hand-written document opens");

    let expected = render(&by_hand, 0);
    assert_eq!(synthesised.data, expected.data, "byte for byte");
    // The fixture is one dark grey everywhere, so "it drew" has to be asserted
    // as a value rather than as a difference.
    let (r, g, b) = pixel(&synthesised, 8, 4);
    assert_eq!((r, g), (b, b), "grey, got ({r}, {g}, {b})");
    assert!((16..=48).contains(&r), "the decoded value, not paper: {r}");
}

// ---- The synthesised document ------------------------------------------

/// `Document::cos()` hands back the synthesised document: a real catalog, a
/// real page tree and a real image XObject per page.
#[test]
fn cos_hands_back_the_synthesised_document() {
    let archive = zip(
        &[
            ZipFile::stored("p1.png", &page_png(3, 4)),
            ZipFile::stored("p2.png", &page_png(3, 4)),
        ],
        Damage::None,
    );
    let document = open(&archive);
    let cos = document.cos();

    assert_eq!(tinker_pdf_cos::pages::count(cos), 2);
    let pages = tinker_pdf_cos::pages::collect(cos);
    assert_eq!(pages.len(), 2);

    let mut images = std::collections::BTreeSet::new();
    for page in &pages {
        let resources = page.resources.as_ref().expect("/Resources");
        let xobjects = cos.resolve_key(resources, cos.intern(b"XObject"));
        let xobjects = xobjects.as_dict().expect("/XObject");
        // Each page names its own image and no other: without that every page
        // inherits every image before it, which is quadratic.
        assert_eq!(xobjects.len(), 1, "one image per page");
        for (_, object) in xobjects.iter() {
            let image = cos.resolve(object);
            let stream = image.as_stream().expect("an image XObject stream");
            assert_eq!(
                stream.dict.get_name(tinker_pdf_cos::Name::TYPE),
                Some(cos.intern(b"XObject"))
            );
            if let tinker_pdf_cos::Object::Ref(r) = object {
                images.insert((r.num, r.gen));
            }
        }
    }
    assert_eq!(images.len(), 2, "and they are two different objects");
}

/// The report is present for a synthesised document and absent for a parsed
/// one, which is how a caller tells them apart without guessing.
#[test]
fn only_a_synthesised_document_reports_an_archive() {
    let archive = zip(&[ZipFile::stored("p.png", &page_png(2, 2))], Damage::None);
    assert!(open(&archive).archive().is_some());

    let ordinary = pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] >>".to_vec(),
    ]);
    assert!(Document::open(ordinary).expect("a PDF").archive().is_none());
}

/// The synthesised document survives a rewrite through the ordinary writer,
/// which is what `cbz_validated.rs` is pointed at.
#[test]
fn a_synthesised_document_saves_and_reopens() {
    let archive = zip(
        &[
            ZipFile::stored("p1.png", &page_png(4, 5)),
            ZipFile::stored("p2.jpg", &grey_jpeg(8, 8)),
        ],
        Damage::None,
    );
    let document = open(&archive);
    let saved = document.editor().save(&WriteOptions {
        mode: WriteMode::Rewrite,
        linearize: true,
        ..WriteOptions::default()
    });

    let reopened = Document::open(saved).expect("the saved document opens");
    assert_eq!(reopened.page_count(), 2);
    assert_eq!(reopened.page(0).expect("a page").size(), (4.0, 5.0));
    assert_eq!(reopened.page(1).expect("a page").size(), (8.0, 8.0));
    assert_eq!(
        render(&reopened, 0).data,
        render(&document, 0).data,
        "and it is the same picture after the round trip"
    );
}

// ---- Refusals, one fixture per variant ---------------------------------

#[test]
fn an_archive_of_nothing_but_comicinfo_is_refused_rather_than_opened_as_zero_pages() {
    let archive = zip(
        &[
            ZipFile::stored("ComicInfo.xml", b"<?xml version=\"1.0\"?><ComicInfo/>"),
            ZipFile::stored("notes/", b""),
        ],
        Damage::None,
    );
    assert_eq!(
        Document::open(archive).err(),
        Some(OpenError::UnsupportedArchive(ArchiveRefusal::NoImages)),
        "zero pages with no error is a failure wearing a success costume"
    );
}

#[test]
fn a_shredded_archive_is_refused_as_damaged() {
    let archive = zip(
        &[ZipFile::stored("p.png", &page_png(2, 2))],
        Damage::Shredded,
    );
    assert_eq!(
        Document::open(archive).err(),
        Some(OpenError::UnsupportedArchive(ArchiveRefusal::Damaged))
    );
}

#[test]
fn a_multi_disk_archive_is_refused_rather_than_read_in_part() {
    let archive = zip(
        &[ZipFile::stored("p.png", &page_png(2, 2))],
        Damage::MultiDisk,
    );
    assert_eq!(
        Document::open(archive).err(),
        Some(OpenError::UnsupportedArchive(ArchiveRefusal::MultiDisk))
    );
}

#[test]
fn a_zip64_sentinel_with_no_zip64_record_is_refused_by_name() {
    let archive = zip(
        &[ZipFile::stored("p.png", &page_png(2, 2))],
        Damage::Zip64Sentinel,
    );
    assert_eq!(
        Document::open(archive).err(),
        Some(OpenError::UnsupportedArchive(
            ArchiveRefusal::Zip64OutOfBounds
        ))
    );
}

/// Every entry that could have been a page is encrypted, so there is nothing
/// to page — and that is an archive-level answer rather than a book of grey
/// pages.
#[test]
fn an_entirely_encrypted_archive_is_refused_by_name() {
    let archive = zip(
        &[
            ZipFile::stored("p1.jpg", &grey_jpeg(8, 8)).encrypted(),
            ZipFile::stored("p2.jpg", &grey_jpeg(8, 8)).encrypted(),
            ZipFile::stored("ComicInfo.xml", b"<ComicInfo/>"),
        ],
        Damage::None,
    );
    assert_eq!(
        Document::open(archive).err(),
        Some(OpenError::UnsupportedArchive(ArchiveRefusal::Encrypted))
    );
}

/// One encrypted entry among readable ones keeps its page instead.
///
/// Gap 29's rule, and gap 17's before it: refusing the archive would throw
/// away the pages that decoded perfectly.
#[test]
fn one_encrypted_entry_among_readable_ones_is_a_page_and_not_a_refusal() {
    let archive = zip(
        &[
            ZipFile::stored("p1.png", &page_png(3, 4)),
            ZipFile::stored("p2.png", &page_png(3, 4)).encrypted(),
            ZipFile::stored("p3.png", &page_png(3, 4)),
        ],
        Damage::None,
    );

    let document = open(&archive);
    assert_eq!(document.page_count(), 3);
    assert_eq!(
        document.archive().expect("a report").pages()[1].defect,
        Some(PageDefect::EntryRefused(ZipEntryError::Encrypted))
    );
}

// ---- The placeholder page ----------------------------------------------

/// An entry that cannot become a page **still becomes a page**.
///
/// Skipping it produces a document that looks complete: three pages where the
/// archive holds four, every page correct, the page after the gap sitting
/// where the missing one was, and nothing anywhere saying so. A reader sees a
/// comic whose story jumps and blames the scan.
#[test]
fn an_unusable_entry_keeps_its_page_number() {
    let good = page_png(6, 8);
    let archive = zip(
        &[
            ZipFile::stored("p1.png", &good),
            ZipFile::stored("p2.gif", b"GIF89a\x06\x00\x08\x00\x00\x00\x00;"),
            ZipFile::stored("p3.png", &broken_png()),
            ZipFile::stored("p4.png", &good),
        ],
        Damage::None,
    );

    let document = open(&archive);
    assert_eq!(document.page_count(), 4, "the count is unchanged");
    let report = document.archive().expect("a report");
    assert_eq!(
        report.pages().iter().map(|p| p.defect).collect::<Vec<_>>(),
        [
            None,
            Some(PageDefect::UnsupportedFormat(ImageFormat::Gif)),
            Some(PageDefect::Undecodable),
            None,
        ]
    );
    assert_eq!(
        page_names(&document),
        ["p1.png", "p2.gif", "p3.png", "p4.png"]
    );

    // Page four is the real fourth page and not the third slid up.
    assert_eq!(render(&document, 3).data, render(&document, 0).data);

    // And every placeholder said so, by page number.
    let placeholders: Vec<u32> = report
        .warnings()
        .iter()
        .filter_map(|w| match w {
            ArchiveWarning::PlaceholderPage { page, .. } => Some(*page),
            _ => None,
        })
        .collect();
    assert_eq!(placeholders, [1, 2]);
}

/// A placeholder is a page of the right size carrying the neutral grey, which
/// is the same 0xBF the renderer paints over an image it could not decode.
#[test]
fn a_placeholder_is_the_books_own_size_and_the_neutral_grey() {
    let archive = zip(
        &[
            ZipFile::stored("p1.png", &page_png(9, 5)),
            ZipFile::stored("p2.webp", b"RIFF\x20\x00\x00\x00WEBPVP8 junk"),
        ],
        Damage::None,
    );

    let document = open(&archive);
    assert_eq!(
        document.page(1).expect("a page").size(),
        (9.0, 5.0),
        "a placeholder takes the book's size, not paper"
    );

    let bitmap = render(&document, 1);
    assert_eq!((bitmap.width, bitmap.height), (9, 5));
    for y in 0..5 {
        for x in 0..9 {
            assert_eq!(pixel(&bitmap, x, y), (0xBF, 0xBF, 0xBF), "({x}, {y})");
        }
    }
}

/// A book that never says what size it is falls back to paper, and only then.
#[test]
fn a_book_of_nothing_but_placeholders_falls_back_to_paper() {
    let archive = zip(
        &[
            ZipFile::stored("p1.gif", b"GIF89a\x02\x00\x02\x00\x00\x00\x00;"),
            ZipFile::stored("p2.bmp", &{
                let mut bmp = vec![0u8; 40];
                bmp[..2].copy_from_slice(b"BM");
                bmp
            }),
        ],
        Damage::None,
    );

    let document = open(&archive);
    assert_eq!(document.page_count(), 2);
    for page in document.pages() {
        assert_eq!(page.size(), (612.0, 792.0));
    }
    assert_eq!(
        document
            .archive()
            .expect("a report")
            .pages()
            .iter()
            .map(|p| p.defect)
            .collect::<Vec<_>>(),
        [
            Some(PageDefect::UnsupportedFormat(ImageFormat::Gif)),
            Some(PageDefect::UnsupportedFormat(ImageFormat::Bmp)),
        ]
    );
}

/// An entry whose checksum fails keeps its page, and the reason names the
/// checksum rather than the format.
///
/// The CRC is the **only** integrity evidence that will ever exist for bytes
/// this design passes through untouched, so a mismatch has to refuse the entry
/// rather than hand it over.
#[test]
fn a_corrupt_entry_is_a_placeholder_and_says_which_check_failed() {
    let archive = zip(
        &[
            ZipFile::stored("p1.png", &page_png(4, 4)),
            ZipFile::stored("p2.png", &page_png(4, 4)).corrupt(),
        ],
        Damage::None,
    );

    let document = open(&archive);
    assert_eq!(document.page_count(), 2);
    let defect = document.archive().expect("a report").pages()[1].defect;
    assert!(
        matches!(
            defect,
            Some(PageDefect::EntryRefused(
                ZipEntryError::ChecksumMismatch { .. }
            ))
        ),
        "{defect:?}"
    );
}

// ---- Deflated entries and the warning surface --------------------------

/// A deflated entry decodes, and it does **not** manufacture a warning.
///
/// ZIP data is raw DEFLATE by definition, so gap 29 made `inflate_raw` public
/// in its own milestone rather than letting the sniffing door raise a
/// `RawDeflateFallback` on every entry of every archive. This is that decision
/// visible from the top of the stack: if the ZIP reader were ever rewired to
/// `flate_decode`, a warning would appear here.
#[test]
fn a_deflated_archive_opens_cleanly() {
    let archive = zip(
        &[
            ZipFile::deflated("p1.png", &page_png(4, 4)),
            ZipFile::deflated("p2.png", &page_png(4, 4)),
        ],
        Damage::None,
    );

    let document = open(&archive);
    assert_eq!(document.page_count(), 2);
    assert!(
        document.archive().expect("a report").warnings().is_empty(),
        "{:?}",
        document.archive().expect("a report").warnings()
    );
}

// ---- Bounds ------------------------------------------------------------

/// The page cap fires by its own refusal, at its shipped value.
///
/// Built at 4 097 entries rather than by lowering the constant, because a cap
/// proved only against a lowered copy of itself has not been proved to fire —
/// which is gap 18a milestone 8's finding.
#[test]
fn a_page_count_past_the_cap_is_refused_by_name() {
    let page = page_png(1, 1);
    let files: Vec<ZipFile> = (0..cbz::MAX_CBZ_PAGES + 1)
        .map(|n| ZipFile::stored(&format!("p{n:05}.png"), &page))
        .collect();
    let archive = zip(&files, Damage::None);

    assert_eq!(
        Document::open(archive).err(),
        Some(OpenError::UnsupportedArchive(ArchiveRefusal::TooLarge))
    );
}

/// And exactly at the cap it opens, so the comparison is `>` and not `>=`.
#[test]
fn a_page_count_at_the_cap_still_opens() {
    let page = page_png(1, 1);
    let limits = cbz::Limits {
        max_pages: 4,
        ..cbz::Limits::DEFAULT
    };
    for (count, expected) in [(4usize, true), (5, false)] {
        let files: Vec<ZipFile> = (0..count)
            .map(|n| ZipFile::stored(&format!("p{n}.png"), &page))
            .collect();
        let archive = zip(&files, Damage::None);
        let result = cbz::synthesise(Container::Zip, &archive, &limits);
        assert_eq!(result.is_ok(), expected, "{count} pages against a cap of 4");
    }
}

/// The byte cap fires by its own refusal, charged before each page is built.
///
/// The charge is a number this test computes rather than reads back: both
/// overheads are public and a passed-through PNG contributes exactly its own
/// concatenated IDAT.
#[test]
fn a_synthesis_past_the_byte_cap_is_refused_by_name() {
    let pages: Vec<Vec<u8>> = (0..3).map(|n| page_png(4 + n, 5)).collect();
    let files: Vec<ZipFile> = pages
        .iter()
        .enumerate()
        .map(|(n, data)| ZipFile::stored(&format!("p{n}.png"), data))
        .collect();
    let archive = zip(&files, Damage::None);

    let charge: usize = cbz::DOCUMENT_OVERHEAD
        + pages
            .iter()
            .map(|p| cbz::PAGE_OVERHEAD + png_scan(p).expect("a PNG").idat.len())
            .sum::<usize>();

    let at_the_cap = cbz::Limits {
        max_synthesised: charge,
        ..cbz::Limits::DEFAULT
    };
    assert!(cbz::synthesise(Container::Zip, &archive, &at_the_cap).is_ok());

    let one_short = cbz::Limits {
        max_synthesised: charge - 1,
        ..cbz::Limits::DEFAULT
    };
    assert_eq!(
        cbz::synthesise(Container::Zip, &archive, &one_short).err(),
        Some(ArchiveRefusal::TooLarge)
    );
}

/// The charge is an over-estimate of the finished document, which is what
/// makes it a bound on the answer and not only on the work.
///
/// If a change to the writer ever made a page cost more than
/// [`cbz::PAGE_OVERHEAD`] this fails, rather than quietly letting a document
/// past the cap.
#[test]
fn the_synthesised_document_fits_inside_what_was_charged_for_it() {
    let page = page_png(8, 8);
    let idat = png_scan(&page).expect("a PNG").idat.len();
    for count in [1usize, 3, 50, 200] {
        let files: Vec<ZipFile> = (0..count)
            .map(|n| ZipFile::stored(&format!("p{n:04}.png"), &page))
            .collect();
        let archive = zip(&files, Damage::None);
        let charge = cbz::DOCUMENT_OVERHEAD + count * (cbz::PAGE_OVERHEAD + idat);

        let (pdf, report) = cbz::synthesise(Container::Zip, &archive, &cbz::Limits::DEFAULT)
            .expect("it synthesises");
        assert_eq!(report.synthesised_bytes(), pdf.len());
        assert!(
            pdf.len() <= charge,
            "{count} pages: {} bytes against a charge of {charge}",
            pdf.len()
        );
    }
}

/// The archive reader's own caps arrive here as `TooLarge` rather than as
/// something a caller has to translate.
#[test]
fn an_archive_reader_refusal_becomes_a_named_bound() {
    let page = page_png(1, 1);
    let limits = cbz::Limits {
        zip: tinker_pdf_zip::Limits {
            max_entries: 3,
            ..tinker_pdf_zip::Limits::DEFAULT
        },
        ..cbz::Limits::DEFAULT
    };
    let files: Vec<ZipFile> = (0..4)
        .map(|n| ZipFile::stored(&format!("p{n}.png"), &page))
        .collect();
    let archive = zip(&files, Damage::None);
    assert_eq!(
        cbz::synthesise(Container::Zip, &archive, &limits).err(),
        Some(ArchiveRefusal::TooLarge)
    );
}

// ---- Never panics ------------------------------------------------------

/// Truncating a good archive anywhere never panics.
///
/// The cheap half of a fuzz target, run on every `cargo test`, because a panic
/// introduced today should not wait on a session.
#[test]
fn truncating_a_good_archive_anywhere_never_panics() {
    let archive = zip(
        &[
            ZipFile::stored("p1.png", &page_png(3, 3)),
            ZipFile::deflated("p2.jpg", &grey_jpeg(8, 8)),
            ZipFile::stored("ComicInfo.xml", b"<ComicInfo/>"),
        ],
        Damage::None,
    );

    for cut in 0..archive.len() {
        let bytes = archive[..cut].to_vec();
        if let Ok(document) = Document::open(bytes) {
            for index in 0..document.page_count() {
                let _ = document.page(index).map(|p| p.size());
            }
        }
    }
}

/// And flipping any single byte never panics.
#[test]
fn flipping_any_single_byte_never_panics() {
    let archive = zip(
        &[
            ZipFile::stored("p1.png", &page_png(3, 3)),
            ZipFile::deflated("p2.png", &page_png(2, 4)),
        ],
        Damage::None,
    );

    for at in 0..archive.len() {
        let mut bytes = archive.clone();
        bytes[at] ^= 0xFF;
        if let Ok(document) = Document::open(bytes) {
            if let Some(page) = document.page(0) {
                let _ = page.render(&RenderOptions::default());
            }
        }
    }
}

/// Everything the archive reader tolerated reaches the caller (ruling 10).
///
/// **Found by gap 29 milestone 6's injection matrix, which is the only reason
/// it exists.** Deleting the loop in `synthesise` that carries
/// `Archive::warnings` into the report failed **nothing** in the workspace: a
/// comic whose directory had to be recovered by scanning, or whose entry name
/// was truncated, opened with an empty warning list and no test noticed.
///
/// That is gap 16's defect exactly — `let (gray, _) = ccitt_decode(...)`, where
/// "every leniency it took was invisible" — arriving in the plan that cites it.
/// The archive reader is careful about what it reports and none of that care
/// survives a call site that drops it, which is the failure mode of a warning
/// list that is *read* rather than *asserted*.
///
/// Two leniencies, from the two different places the reader records them: the
/// route it had to take, and what it had to do to an entry.
#[test]
fn what_the_archive_reader_tolerated_reaches_the_caller() {
    // A directory that is not there at all, so the local-header scan is the
    // route. The archive is whole up to the point the writer stopped, which is
    // what a half-uploaded comic looks like.
    let whole = zip(&[ZipFile::stored("p1.png", &page_png(4, 4))], Damage::None);
    let directory_at = whole
        .windows(4)
        .position(|w| w == b"PK\x01\x02")
        .expect("a central directory");
    let recovered = open(&whole[..directory_at]);
    let warnings = recovered.archive().expect("a report").warnings().to_vec();
    assert!(
        warnings.contains(&ArchiveWarning::Zip(ZipWarning::NoEndOfCentralDirectory)),
        "the route the reader had to take is not in the report: {warnings:?}"
    );
    assert!(
        warnings.contains(&ArchiveWarning::Zip(ZipWarning::RecoveredFromLocalHeaders)),
        "recovery happened silently: {warnings:?}"
    );
    assert_eq!(recovered.page_count(), 1, "the page survived the recovery");

    // And an entry-level leniency, which the reader records in the same list
    // from a different place: a name past the cap is truncated and kept.
    let long = format!("{}.png", "n".repeat(zip_limits::MAX_ZIP_NAME_LEN));
    let truncated = open(&zip(
        &[ZipFile::stored(&long, &page_png(4, 4))],
        Damage::None,
    ));
    let warnings = truncated.archive().expect("a report").warnings().to_vec();
    assert!(
        warnings.contains(&ArchiveWarning::Zip(ZipWarning::NameTruncated)),
        "an entry the reader had to shorten is not in the report: {warnings:?}"
    );

    // The other direction, or the assertions above pass on a build that
    // reports every warning there is: a healthy archive says nothing.
    let healthy = open(&zip(
        &[ZipFile::stored("p1.png", &page_png(4, 4))],
        Damage::None,
    ));
    assert!(
        healthy.archive().expect("a report").warnings().is_empty(),
        "a healthy archive reported a leniency it did not take"
    );
}

/// A page whose picture arrived and arrived damaged says so, and is still the
/// picture (ruling 10, and ruling 2's other half).
///
/// **The injection matrix found this one too**: `ArchiveWarning::DegradedImage`
/// was pushed at one site and asserted at none, so disabling the branch failed
/// nothing. That is milestone 2's `NoEndOfCentralDirectory` in reverse — there
/// a variant nothing pushed, here a variant nothing read — and both are the
/// same defect, which is a warning surface nobody has driven.
///
/// The distinction it carries is the one a host shows differently.
/// `PlaceholderPage` means there is no picture; `DegradedImage` means there is
/// one and it is not all there. A file that stops before its IEND is the
/// cheapest way to be exactly that: 15948 5.2's chunk structure says the file
/// is unfinished, and every pixel that did arrive is still good.
#[test]
fn a_page_that_arrived_damaged_says_so_and_is_still_a_page() {
    let good = page_png(6, 6);
    // IEND is the last twelve bytes: length, type, no data, CRC.
    let unfinished = &good[..good.len() - 12];

    let document = open(&zip(
        &[
            ZipFile::stored("p1.png", &good),
            ZipFile::stored("p2.png", unfinished),
        ],
        Damage::None,
    ));

    assert_eq!(document.page_count(), 2);
    let report = document.archive().expect("a report");
    assert_eq!(
        report.warnings(),
        [ArchiveWarning::DegradedImage { page: 1 }],
        "the damaged page is reported as degraded and the whole page is not"
    );
    assert_eq!(
        report.pages()[1].defect,
        None,
        "a degraded page is not a placeholder: the picture is there"
    );

    // And it *is* the picture: the same 6 x 6 raster, not a grey rectangle.
    let whole = render(&document, 0);
    let damaged = render(&document, 1);
    assert_eq!((damaged.width, damaged.height), (6, 6));
    assert_eq!(
        damaged.data, whole.data,
        "the pixels the file did carry are the pixels on the page"
    );
}

// ---- TIFF ---------------------------------------------------------------

/// A single-strip G4 TIFF: sixteen pixels wide, six rows,
/// `PhotometricInterpretation` 0, `MM` byte order.
///
/// Assembled from TIFF 6.0's own field layouts and coded from T.4 Table 1 and
/// T.6 Table 4 by `tinker_pdf_filters`' own fixture writer, then committed here
/// as bytes. There is no `.tif` in this repository and none is fetched, so
/// every byte of it has a line of specification behind it; what it is *for* in
/// this file is that a comic archive can hold one, and the picture it decodes
/// to is [`FAX_PATTERN`].
const G4_ONE_STRIP: [u8; 146] = [
    0x4D, 0x4D, 0x00, 0x2A, 0x00, 0x00, 0x00, 0x08, 0x00, 0x09, 0x01, 0x00, 0x00, 0x04, 0x00, 0x00,
    0x00, 0x01, 0x00, 0x00, 0x00, 0x10, 0x01, 0x01, 0x00, 0x04, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
    0x00, 0x06, 0x01, 0x02, 0x00, 0x03, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x01, 0x03,
    0x00, 0x03, 0x00, 0x00, 0x00, 0x01, 0x00, 0x04, 0x00, 0x00, 0x01, 0x06, 0x00, 0x03, 0x00, 0x00,
    0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x01, 0x11, 0x00, 0x04, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
    0x00, 0x7A, 0x01, 0x15, 0x00, 0x03, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x01, 0x16,
    0x00, 0x04, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x06, 0x01, 0x17, 0x00, 0x04, 0x00, 0x00,
    0x00, 0x01, 0x00, 0x00, 0x00, 0x18, 0x00, 0x00, 0x00, 0x00, 0x33, 0x14, 0xBB, 0x0C, 0x2C, 0x20,
    0xF0, 0x88, 0xB3, 0x65, 0x0E, 0x50, 0xE1, 0x11, 0xD1, 0x1D, 0x11, 0xD0, 0x20, 0x94, 0x44, 0x44,
    0x44, 0x58,
];

/// The same picture, the same coding, cut into **two** strips of three rows.
///
/// Which is the one thing that stops it passing through: two T.6 streams end
/// to end are not one, because the second strip's first row would be decoded
/// against the first strip's last. So this file takes the decoder and
/// `G4_ONE_STRIP` does not, and the two of them are what
/// `a_placed_tiff_renders_the_same_as_a_decoded_one` compares.
const G4_TWO_STRIPS: [u8; 164] = [
    0x4D, 0x4D, 0x00, 0x2A, 0x00, 0x00, 0x00, 0x08, 0x00, 0x09, 0x01, 0x00, 0x00, 0x04, 0x00, 0x00,
    0x00, 0x01, 0x00, 0x00, 0x00, 0x10, 0x01, 0x01, 0x00, 0x04, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
    0x00, 0x06, 0x01, 0x02, 0x00, 0x03, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x01, 0x03,
    0x00, 0x03, 0x00, 0x00, 0x00, 0x01, 0x00, 0x04, 0x00, 0x00, 0x01, 0x06, 0x00, 0x03, 0x00, 0x00,
    0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x01, 0x11, 0x00, 0x04, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00,
    0x00, 0x7A, 0x01, 0x15, 0x00, 0x03, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x01, 0x16,
    0x00, 0x04, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x03, 0x01, 0x17, 0x00, 0x04, 0x00, 0x00,
    0x00, 0x02, 0x00, 0x00, 0x00, 0x82, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x8A, 0x00, 0x00,
    0x00, 0x92, 0x00, 0x00, 0x00, 0x07, 0x00, 0x00, 0x00, 0x11, 0x33, 0x14, 0xBB, 0x0C, 0x2C, 0x20,
    0xF0, 0x00, 0x26, 0xA2, 0xD9, 0x43, 0x94, 0x38, 0x44, 0x74, 0x47, 0x44, 0x74, 0x08, 0x25, 0x11,
    0x11, 0x11, 0x16, 0x00,
];

/// The picture both TIFF fixtures hold: `1` is a black pixel.
///
/// Written out rather than derived, because it is the literal expectation both
/// of the tests below are held to — a comparison of two renders can pass on
/// two blank pages, and this is what stops that.
const FAX_PATTERN: [&str; 6] = [
    "0000000011111111",
    "0011110000111100",
    "0011000000001100",
    "1111111100000000",
    "0101010101010101",
    "0000000000000000",
];

/// Asserts a 16 x 6 bitmap against [`FAX_PATTERN`], pixel by pixel.
fn assert_is_the_fax(bitmap: &Bitmap, what: &str) {
    assert_eq!((bitmap.width, bitmap.height), (16, 6), "{what}");
    for (y, row) in FAX_PATTERN.iter().enumerate() {
        for (x, cell) in row.chars().enumerate() {
            let want = if cell == '1' {
                (0u8, 0u8, 0u8)
            } else {
                (255, 255, 255)
            };
            assert_eq!(
                pixel(bitmap, x as u32, y as u32),
                want,
                "{what}: pixel ({x}, {y})"
            );
        }
    }
}

/// A TIFF entry is a page, and the page is the picture the file holds.
///
/// The expected values are the fixture's own pattern rather than another
/// render, which is what the module note asks of every picture here.
#[test]
fn a_tiff_entry_becomes_a_page_with_the_pixels_the_file_holds() {
    let archive = zip(&[ZipFile::stored("scan.tif", &G4_ONE_STRIP)], Damage::None);
    let document = open(&archive);

    assert_eq!(document.page_count(), 1);
    let report = document
        .archive()
        .expect("a synthesised document has a report");
    assert_eq!(
        report.pages()[0].defect,
        None,
        "a TIFF is a page rather than a placeholder"
    );
    assert!(
        report.warnings().is_empty(),
        "nothing was tolerated: {:?}",
        report.warnings()
    );
    assert_eq!(document.page(0).expect("a page").size(), (16.0, 6.0));

    assert_is_the_fax(&render(&document, 0), "the placed route");
}

/// The exit criterion: **0 pixels different** between the two routes.
///
/// One strip passes through as a `/CCITTFaxDecode` stream carrying the file's
/// own bytes; two strips of the same picture cannot, so they are decoded to
/// eight-bit samples and re-deflated. Those are two entirely different
/// dictionaries, two different filters and two different bit depths reaching
/// the rasterizer, and the picture has to be the same one — otherwise the
/// pass-through is a second decoder wearing the first one's name.
///
/// Both sides are also held to [`FAX_PATTERN`], because two identical blank
/// pages compare equal.
#[test]
fn a_placed_tiff_renders_the_same_as_a_decoded_one() {
    let archive = zip(
        &[
            ZipFile::stored("1-placed.tif", &G4_ONE_STRIP),
            ZipFile::stored("2-decoded.tif", &G4_TWO_STRIPS),
        ],
        Damage::None,
    );
    let document = open(&archive);
    assert_eq!(document.page_count(), 2);

    let placed = render(&document, 0);
    let decoded = render(&document, 1);
    assert_is_the_fax(&placed, "the placed route");
    assert_is_the_fax(&decoded, "the decoded route");

    assert_eq!(
        (placed.width, placed.height),
        (decoded.width, decoded.height)
    );
    let differing = placed
        .data
        .iter()
        .zip(decoded.data.iter())
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(differing, 0, "the two routes drew different pictures");
}

/// The two routes are really two, which is the claim the test above would
/// satisfy just as well if both files decoded.
#[test]
fn the_two_tiff_fixtures_take_two_different_routes() {
    use tinker_pdf_cos::{tiff_image, TiffRoute};
    let limits = tinker_pdf_filters::Limits::new(1 << 22);
    assert_eq!(
        tiff_image(&G4_ONE_STRIP, &limits)
            .expect("one strip")
            .route(),
        TiffRoute::Placed
    );
    assert_eq!(
        tiff_image(&G4_TWO_STRIPS, &limits)
            .expect("two strips")
            .route(),
        TiffRoute::Decoded,
        "two T.6 streams end to end are not one"
    );
}

/// A TIFF is still classified as one, and is no longer refused for being one.
#[test]
fn a_tiff_is_recognised_by_its_magic_and_no_longer_refused() {
    assert_eq!(cbz::image_format(&G4_ONE_STRIP), Some(ImageFormat::Tiff));
    // `II` as well as `MM`, which is the sniff's whole job here.
    assert_eq!(
        cbz::image_format(b"II\x2a\x00\x08\x00\x00\x00"),
        Some(ImageFormat::Tiff)
    );

    let archive = zip(&[ZipFile::stored("p.tif", &G4_ONE_STRIP)], Damage::None);
    let report = open(&archive)
        .archive()
        .expect("a report")
        .warnings()
        .to_vec();
    assert!(
        !report.iter().any(|w| matches!(
            w,
            ArchiveWarning::PlaceholderPage {
                defect: PageDefect::UnsupportedFormat(ImageFormat::Tiff),
                ..
            }
        )),
        "the refusal row is gone: {report:?}"
    );
}

/// A TIFF that is a TIFF only as far as its header keeps its page number, the
/// way a broken PNG does.
#[test]
fn a_tiff_that_will_not_decode_is_a_placeholder_and_not_a_dropped_page() {
    let broken = b"MM\x00\x2a\x00\x00\x00\x08nothing that is a directory".to_vec();
    let document = open(&zip(
        &[
            ZipFile::stored("1.tif", &G4_ONE_STRIP),
            ZipFile::stored("2.tif", &broken),
            ZipFile::stored("3.tif", &G4_ONE_STRIP),
        ],
        Damage::None,
    ));

    assert_eq!(document.page_count(), 3, "the page count is unchanged");
    let report = document.archive().expect("a report");
    assert_eq!(report.pages()[1].defect, Some(PageDefect::Undecodable));
    assert_eq!(report.pages()[2].defect, None, "page 3 keeps its number");
    assert_is_the_fax(&render(&document, 2), "the page after the broken one");
}
