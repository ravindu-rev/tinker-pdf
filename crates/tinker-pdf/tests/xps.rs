//! Real XPS packages, and what this build does with one today (gap 30,
//! milestone 1).
//!
//! Every package under `tests/xps/` was written by a Microsoft serialiser, not
//! by anything here — see `tests/xps/README.md` for which one wrote which, and
//! why that matters enough to be the first milestone of the gap rather than a
//! fixture chore inside a later one. Gap 29 closed having never opened a `.cbz`
//! a real archiver produced, recorded the debt in three milestones, and had to
//! write it into the gap's closing section. This file is the shape of not
//! repeating that.
//!
//! Two of the tests below are `#[ignore]`d and **fail when run**. That is
//! deliberate and it is the milestone's deliverable: they pin the defect gap 30
//! exists to fix, so it has a name in the suite from the first commit rather
//! than from the commit that fixes it. Milestone 3's exit criterion is that
//! these two go green.

use std::path::PathBuf;

use tinker_pdf::{ArchiveRefusal, Document, OpenError};

// ---- the corpus ---------------------------------------------------------

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/xps")
}

fn package(name: &str) -> Vec<u8> {
    let path = corpus_dir().join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// Every package in the corpus, by name.
///
/// Written out rather than globbed, so a file deleted from the directory fails
/// a test instead of shrinking a loop to nothing — gap 29's milestone-6 lesson
/// about a sweep that reports whatever it happens to find.
const PACKAGES: &[&str] = &[
    "wpf-image-and-text.xps",
    "wpf-shapes-only.xps",
    "wpf-three-pages.xps",
    "wpf-gradients.xps",
    "wpf-tiled-brush.xps",
    "wpf-jpeg-image.xps",
    "xpsom-image-and-text.oxps",
    "xpsom-gradients.oxps",
];

// ---- what the corpus is -------------------------------------------------

/// Six is the number milestone 1 owes; eight is what it got.
#[test]
fn the_corpus_holds_at_least_six_genuine_packages() {
    assert!(
        PACKAGES.len() >= 6,
        "milestone 1 owes at least six packages, the list names {}",
        PACKAGES.len()
    );
    for name in PACKAGES {
        let bytes = package(name);
        assert!(
            bytes.starts_with(b"PK\x03\x04"),
            "{name} does not begin with a local file header"
        );
    }
}

/// Reads one named item of a package, inflating it if it is deflated.
///
/// A byte search over the archive would find only the stored items, and the
/// markup this file wants to look at is deflated in every package here.
fn part(archive_name: &str, item: &str) -> Vec<u8> {
    let bytes = package(archive_name);
    let mut archive = tinker_pdf_zip::Archive::open(&bytes, &tinker_pdf_zip::Limits::DEFAULT)
        .unwrap_or_else(|e| panic!("{archive_name} is not readable as a ZIP: {e:?}"));
    let index = archive
        .entries()
        .iter()
        .position(|e| e.name == item)
        .unwrap_or_else(|| panic!("{archive_name} has no item named {item}"));
    archive
        .read(index)
        .unwrap_or_else(|e| panic!("{archive_name}/{item}: {e:?}"))
        .into_owned()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Both dialects are present, and it is the **namespace** that says which.
///
/// This is the question the plan left open and made this milestone settle.
/// ECMA-388 Table D–4 keeps the `xps-` content types for OpenXPS, and some
/// sources report that Windows writes `oxps-`-prefixed ones instead. It does
/// not: `xpsom-*.oxps` carries `http://schemas.openxps.org/oxps/v1.0` on every
/// element and `…/fixedrepresentation` relationship, and the **same**
/// `application/vnd.ms-package.xps-…` content types as the XPS 1.0 packages,
/// byte for byte. So the obvious sniff is the wrong one — which is exactly what
/// the plan's decision 3 assumed and could not check.
#[test]
fn both_dialects_are_represented_and_the_content_type_does_not_tell_them_apart() {
    const MICROSOFT: &[u8] = b"http://schemas.microsoft.com/xps/2005/06";
    const OPENXPS: &[u8] = b"http://schemas.openxps.org/oxps/v1.0";

    for name in ["wpf-image-and-text.xps", "wpf-gradients.xps"] {
        let page = part(name, "Documents/1/Pages/1.fpage");
        assert!(contains(&page, MICROSOFT), "{name} is not XPS 1.0");
        assert!(!contains(&page, OPENXPS), "{name} is not OpenXPS as well");
    }
    for name in ["xpsom-image-and-text.oxps", "xpsom-gradients.oxps"] {
        let page = part(name, "Documents/1/Pages/1.fpage");
        assert!(contains(&page, OPENXPS), "{name} is not OpenXPS");
        assert!(!contains(&page, MICROSOFT), "{name} is not XPS 1.0 as well");
        let rels = part(name, "_rels/.rels");
        assert!(
            contains(
                &rels,
                b"http://schemas.openxps.org/oxps/v1.0/fixedrepresentation"
            ),
            "{name}'s package relationship carries the OpenXPS type"
        );
    }

    // And the content types are identical across the two dialects. Anything
    // keying on `oxps-` would find nothing in either.
    for name in PACKAGES {
        let types = part(name, "[Content_Types].xml");
        assert!(
            contains(
                &types,
                b"application/vnd.ms-package.xps-fixeddocumentsequence+xml"
            ),
            "{name} names the fixed-document-sequence content type"
        );
        assert!(
            !contains(&types, b"oxps-"),
            "{name}: Windows' OpenXPS output does **not** use `oxps-`-prefixed \
             content types; ECMA-388 Table D–4's `xps-` strings are what both \
             dialects carry, and the namespace is the only discriminator"
        );
    }
}

// ---- the two failures milestone 3 turns green ---------------------------

/// **Fails today, on purpose.** An XPS opens as a comic and shows a resource.
///
/// `Document::open` sniffs `PK\x03\x04` and hands everything to
/// `cbz::synthesise`, and one signature covers CBZ, XPS, EPUB, ODF, OOXML and
/// every JAR ever built. `wpf-image-and-text.xps` is a single 816 × 1056 fixed
/// page — 612 × 792 pt, US Letter to the point — carrying a 32 × 32 PNG
/// *resource* and a `<Glyphs>` run in an obfuscated Cascadia Mono.
///
/// What comes back is one page, the size of the PNG, with the markup, the
/// text, the font and the page size all discarded, and **no warning at all**,
/// because from `cbz.rs`'s point of view nothing went wrong: it found one image
/// entry and paged it. That is gap 17's blank page returned as success and gap
/// 18a's plausible photograph, arriving through the facade.
///
/// Milestone 3 is where this goes green: after its discrimination an `.xps` is
/// either read as a fixed document or refused by a name that is true.
#[test]
#[ignore = "gap 30 milestone 3 turns this green: it is the defect, pinned"]
fn an_xps_with_a_raster_resource_is_not_a_one_page_comic() {
    let document = Document::open(package("wpf-image-and-text.xps"))
        .expect("today it opens; after milestone 3 it opens as a fixed document");
    let page = document.page(0).expect("a first page");
    let (w, h) = page.size();
    assert_eq!(
        (w, h),
        (612.0, 792.0),
        "the fixed page states Width=\"816\" Height=\"1056\" in units of 1/96 \
         inch, which is 612 x 792 pt; today the page is the 32 x 32 PNG \
         resource at its own pixel size"
    );
    assert!(
        document
            .archive()
            .is_none_or(|report| !report.warnings().is_empty()),
        "a document synthesised from something that is not a comic archive \
         must not come back silent"
    );
}

/// **Fails today, on purpose.** The same document without a raster is refused.
///
/// `wpf-shapes-only.xps` is a fixed page of three filled `<Path>` elements and
/// nothing else — no image part anywhere in the package. The comic path finds
/// no entry it can page and refuses the whole thing as
/// `ArchiveRefusal::NoImages`, whose documentation reads *"a valid archive with
/// no image entries"*, said about a document that has a page.
///
/// The refusal is not the complaint; the *name* is. Milestone 3 makes this
/// either open as a one-page fixed document or refuse under a name that
/// describes an XPS.
#[test]
#[ignore = "gap 30 milestone 3 turns this green: it is the defect, pinned"]
fn an_xps_without_a_raster_resource_is_not_refused_as_having_no_images() {
    let opened = Document::open(package("wpf-shapes-only.xps"));
    assert_ne!(
        opened.err(),
        Some(OpenError::UnsupportedArchive(ArchiveRefusal::NoImages)),
        "`NoImages` is a true sentence about a comic archive and a false one \
         about a fixed document with one page in it"
    );
}

// ---- what actually happens today ----------------------------------------

/// The measurement the two ignored tests are the complaint about.
///
/// Written as its own passing test rather than left in a commit message,
/// because a defect described in prose is a defect nobody can watch. When
/// milestone 3 lands, **this test fails** and is deleted in the same commit
/// that un-ignores the two above — which is the point: the record of the old
/// behaviour has to break when the behaviour changes.
#[test]
fn today_an_xps_opens_as_a_comic_and_this_is_what_it_reports() {
    let document =
        Document::open(package("wpf-image-and-text.xps")).expect("today the sniff accepts it");
    assert_eq!(document.page_count(), 1, "one image entry, one page");
    let page = document.page(0).expect("a first page");
    assert_eq!(
        page.size(),
        (32.0, 32.0),
        "the page is the PNG resource at its own pixel size, not the 612 x 792 \
         pt the markup states"
    );
    let report = document
        .archive()
        .expect("today it is a synthesised comic, so it has an archive report");
    assert_eq!(
        report.warnings(),
        &[],
        "and nothing warns, which is the whole complaint"
    );
    assert_eq!(
        Document::open(package("wpf-shapes-only.xps")).err(),
        Some(OpenError::UnsupportedArchive(ArchiveRefusal::NoImages)),
        "and a fixed document with no raster part is refused as though it were \
         a comic with no pictures in it"
    );
}

/// Every package in the corpus is mis-read today, and in one of two ways.
///
/// The sweep exists so that "an XPS opens as a comic" is known to be true of
/// the whole corpus rather than of the one file the complaint was written
/// about. It also fixes the page counts, which are the count of *raster parts*
/// and have nothing to do with the documents: the three-page package reports
/// no pages at all, and the one-page tiled-brush package reports one, because
/// its two brushes share a single PNG.
#[test]
fn every_package_in_the_corpus_is_mis_read_today() {
    // (package, what today's build does)
    let expected: &[(&str, Result<u32, ArchiveRefusal>)] = &[
        ("wpf-image-and-text.xps", Ok(1)),
        ("wpf-shapes-only.xps", Err(ArchiveRefusal::NoImages)),
        ("wpf-three-pages.xps", Err(ArchiveRefusal::NoImages)),
        ("wpf-gradients.xps", Err(ArchiveRefusal::NoImages)),
        ("wpf-tiled-brush.xps", Ok(1)),
        ("wpf-jpeg-image.xps", Ok(1)),
        ("xpsom-image-and-text.oxps", Ok(1)),
        ("xpsom-gradients.oxps", Err(ArchiveRefusal::NoImages)),
    ];
    assert_eq!(
        expected.len(),
        PACKAGES.len(),
        "the sweep covers the corpus"
    );
    for (name, want) in expected {
        let got = match Document::open(package(name)) {
            Ok(document) => Ok(document.page_count()),
            Err(OpenError::UnsupportedArchive(why)) => Err(why),
            Err(other) => panic!("{name}: unexpected {other:?}"),
        };
        assert_eq!(&got, want, "{name}");
    }
}

// ---- the inventory cannot drift -----------------------------------------

/// `INVENTORY.tsv` describes the packages that are actually here.
///
/// `tests/xps/inventory.ps1` writes that file by reading the archives through
/// .NET's `System.IO.Compression`; this recomputes the same table through
/// `tinker-pdf-zip` and compares. So the inventory cannot drift from the files,
/// and two independent ZIP readers have to agree about every name, method and
/// size in the corpus — which is also the first time this repository's own
/// archive reader has been pointed at an archive it did not write.
///
/// The media-type column is *not* checked here: resolving one is OPC 7.2.3.5's
/// ordered algorithm over `[Content_Types].xml`, which is milestone 3's work
/// and does not exist yet. It is in the file because the milestone owes it, and
/// milestone 3's own round-trip criterion is where it becomes checkable.
#[test]
fn inventory_matches_the_packages() {
    let text = std::fs::read_to_string(corpus_dir().join("INVENTORY.tsv"))
        .expect("the committed inventory");
    let mut rows = text.lines();
    assert_eq!(
        rows.next(),
        Some("package\titem\tmedia_type\tmethod\tcompressed\tuncompressed"),
        "the inventory's header"
    );

    // The script writes the packages in name order; the constant above is in
    // the order they were produced.
    let mut ordered: Vec<&str> = PACKAGES.to_vec();
    ordered.sort_unstable();

    let mut seen = 0usize;
    for name in &ordered {
        let bytes = package(name);
        let archive = tinker_pdf_zip::Archive::open(&bytes, &tinker_pdf_zip::Limits::DEFAULT)
            .unwrap_or_else(|e| panic!("{name} is not readable as a ZIP: {e:?}"));
        assert_eq!(
            archive.warnings(),
            &[],
            "{name} should need no leniency from the archive reader"
        );
        // A conforming OPC package always has an end-of-central-directory
        // record, so none of these should reach the recovery rung. Asserted
        // rather than assumed, because the recovery scan is the path that
        // inflates during `open` and `84ee3b7` had just finished making it
        // charge for that.
        assert_eq!(
            archive.route(),
            tinker_pdf_zip::Route::CentralDirectory,
            "{name} should be read by the central-directory route"
        );
        for entry in archive.entries() {
            let row = rows
                .next()
                .unwrap_or_else(|| panic!("{name}: inventory ran out"));
            let mut field = row.split('\t');
            assert_eq!(field.next(), Some(*name), "row {seen}: package");
            assert_eq!(field.next(), Some(entry.name.as_str()), "row {seen}: item");
            let _media_type = field.next();
            let method = match entry.method {
                tinker_pdf_zip::Method::Stored => "stored",
                tinker_pdf_zip::Method::Deflated => "deflate",
                other => {
                    panic!("{name}: OPC 7.3.6 allows only stored and deflate, found {other:?}")
                }
            };
            assert_eq!(field.next(), Some(method), "row {seen}: method");
            assert_eq!(
                field.next().and_then(|v| v.parse::<u64>().ok()),
                Some(entry.compressed_size),
                "row {seen}: compressed size"
            );
            assert_eq!(
                field.next().and_then(|v| v.parse::<u64>().ok()),
                Some(entry.uncompressed_size),
                "row {seen}: uncompressed size"
            );
            seen += 1;
        }
    }
    assert_eq!(rows.next(), None, "the inventory has rows for nothing here");
    assert!(
        seen >= 40,
        "the corpus should hold rather more than {seen} parts"
    );
}

/// OPC 7.3.6 forbids encryption and every compression method but DEFLATE, and
/// `tinker-pdf-zip` already refuses both — so for once a specification and an
/// existing refusal agree exactly. Asserted over the real corpus rather than
/// assumed, and in both directions: no package uses a third method, and the
/// corpus as a whole carries both of the two, so a reader that handled only
/// one would fail something here.
#[test]
fn every_part_is_stored_or_deflated_and_the_corpus_carries_both() {
    let mut stored = 0usize;
    let mut deflated = 0usize;
    for name in PACKAGES {
        let bytes = package(name);
        let archive = tinker_pdf_zip::Archive::open(&bytes, &tinker_pdf_zip::Limits::DEFAULT)
            .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        for entry in archive.entries() {
            match entry.method {
                tinker_pdf_zip::Method::Stored => stored += 1,
                tinker_pdf_zip::Method::Deflated => deflated += 1,
                other => panic!("{name}/{}: OPC 7.3.6 allows neither {other:?}", entry.name),
            }
        }
    }
    assert!(
        stored > 0 && deflated > 0,
        "stored {stored}, deflated {deflated}"
    );
}

/// `[Content_Types].xml` is the **last** item of every package here.
///
/// OPC 7.3.7 leaves its position unconstrained — the brackets were chosen
/// precisely because they violate the part-name grammar, so it can never
/// collide with a part — and both Microsoft serialisers put it last. A reader
/// that assumes it is first is wrong on the first real file it meets, in either
/// dialect. Pinned here so milestone 3 cannot quietly acquire that assumption.
#[test]
fn the_content_types_item_is_last_in_every_package() {
    for name in PACKAGES {
        let bytes = package(name);
        let archive = tinker_pdf_zip::Archive::open(&bytes, &tinker_pdf_zip::Limits::DEFAULT)
            .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        let names: Vec<&str> = archive.entries().iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names.last(),
            Some(&"[Content_Types].xml"),
            "{name}: the content-types item is not last"
        );
        assert_eq!(
            names
                .iter()
                .filter(|n| **n == "[Content_Types].xml")
                .count(),
            1,
            "{name}: exactly one content-types item"
        );
    }
}
