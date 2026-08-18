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
//! *Amended, 18 August 2026, milestone 3.* The two tests that were `#[ignore]`d
//! here are **green**, and
//! `today_an_xps_opens_as_a_comic_and_this_is_what_it_reports` — which passed
//! while asserting the wrong answers, so the defect could be watched rather
//! than described — is deleted, because a record of old behaviour that does not
//! break when the behaviour changes is not a record. The sweep that said every
//! package is mis-read now says what each one is, and the inventory's media
//! type column is checked for the first time, through OPC 7.2.3.5.

use std::path::PathBuf;

use tinker_pdf::xps::opc::PartName;
use tinker_pdf::{ArchiveRefusal, ArchiveWarning, Dialect, Document, OpenError, RenderOptions};

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

// ---- the two failures milestone 3 turned green --------------------------

/// **Was the defect, pinned.** An XPS opened as a comic and showed a resource.
///
/// `Document::open` sniffs `PK\x03\x04` and used to hand everything to
/// `cbz::synthesise`, and one signature covers CBZ, XPS, EPUB, ODF, OOXML and
/// every JAR ever built. `wpf-image-and-text.xps` is a single 816 × 1056 fixed
/// page — 612 × 792 pt, US Letter to the point — carrying a 32 × 32 PNG
/// *resource* and a `<Glyphs>` run in an obfuscated Cascadia Mono.
///
/// What came back was one page, the size of the PNG, with the markup, the
/// text, the font and the page size all discarded, and **no warning at all**,
/// because from `cbz.rs`'s point of view nothing went wrong: it found one image
/// entry and paged it. That is gap 17's blank page returned as success and gap
/// 18a's plausible photograph, arriving through the facade.
///
/// Milestone 3 is where it went green: an `.xps` is now read as a fixed
/// document, or refused by a name that is true.
///
/// *Amended, milestone 8.* The last assertion used to be that the document
/// **must not come back silent**, and that was the right assertion for five
/// milestones: while anything on the page was still owed, silence could only
/// mean the original defect — an XPS read as a comic, which complains about
/// nothing because from `cbz.rs`'s side nothing went wrong. Milestone 8 draws
/// the `ImageBrush`, so the page is now complete and legitimately silent, and
/// the assertion inverted.
///
/// What separates the two silences is not the warning list — it is whether the
/// page is the size the *markup* states and whether the picture is actually
/// there. So both are asserted, and the second one is asserted **in pixels**:
/// the original defect produced a 32 × 32 page that was entirely the PNG, and a
/// build that read the fixed page and drew nothing would produce a 612 × 792
/// page that is entirely white. Neither passes below.
#[test]
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
    // Nothing is owed on this page any more, which is milestone 8's own
    // criterion. Asserted as an empty list rather than as "no brush warning",
    // because a list that grew a new complaint would be a regression whatever
    // the complaint was.
    let report = document.archive().expect("a synthesised document reports");
    assert!(
        report.warnings().is_empty(),
        "the whole page reads: {:?}",
        report.warnings()
    );

    // And the picture is there. A page that read its markup and drew none of it
    // would be white, which is exactly what the four milestones before this one
    // produced for this file — so silence is only the right answer alongside
    // ink.
    let bitmap = page.render(&RenderOptions::default());
    assert_eq!((bitmap.width, bitmap.height), (612, 792));
    let ink = bitmap
        .data
        .chunks_exact(3)
        .filter(|px| *px != [0xFF, 0xFF, 0xFF])
        .count();
    assert!(
        ink > 1_000,
        "the page drew {ink} non-white pixels; the picture is 200 x 200 units \
         at 0.75 and the text is a line of eight glyphs"
    );
}

/// **Was the defect, pinned.** The same document without a raster was refused.
///
/// `wpf-shapes-only.xps` is a fixed page of three filled `<Path>` elements and
/// nothing else — no image part anywhere in the package. The comic path found
/// no entry it could page and refused the whole thing as
/// `ArchiveRefusal::NoImages`, whose documentation reads *"a valid archive with
/// no image entries"*, said about a document that has a page.
///
/// The refusal was not the complaint; the *name* was. It now opens as the
/// one-page fixed document it is.
#[test]
fn an_xps_without_a_raster_resource_is_not_refused_as_having_no_images() {
    let opened = Document::open(package("wpf-shapes-only.xps"));
    assert_ne!(
        opened.as_ref().err(),
        Some(&OpenError::UnsupportedArchive(ArchiveRefusal::NoImages)),
        "`NoImages` is a true sentence about a comic archive and a false one \
         about a fixed document with one page in it"
    );
    let document = opened.expect("a fixed document with one page in it");
    assert_eq!(document.page_count(), 1);
    assert_eq!(
        document.page(0).map(|page| page.size()),
        Some((612.0, 792.0)),
        "three filled paths on one 816 x 1056 fixed page"
    );
}

// ---- what happens now ---------------------------------------------------

/// Every package in the corpus opens as the document it is.
///
/// The sweep this replaces asserted the opposite and had to: it recorded that
/// the page count was the count of *raster parts*, so the three-page package
/// reported no pages at all and the one-page tiled-brush package reported one
/// because its two brushes share a single PNG. Five of the eight refused, three
/// opened as one-page comics, and not one was read as the document it is.
///
/// Now every one of them opens, the page counts are the `PageContent` counts,
/// every page is 612 × 792 pt because every fixed page in the corpus states
/// `Width="816" Height="1056"`, and the dialect each package reports is the one
/// its **namespace** says — which is the only discriminator there is, since the
/// content types are byte-identical across the two.
#[test]
fn every_package_in_the_corpus_opens_as_the_document_it_is() {
    // (package, pages, dialect)
    let expected: &[(&str, u32, Dialect)] = &[
        ("wpf-image-and-text.xps", 1, Dialect::Xps1),
        ("wpf-shapes-only.xps", 1, Dialect::Xps1),
        ("wpf-three-pages.xps", 3, Dialect::Xps1),
        ("wpf-gradients.xps", 1, Dialect::Xps1),
        ("wpf-tiled-brush.xps", 1, Dialect::Xps1),
        ("wpf-jpeg-image.xps", 1, Dialect::Xps1),
        ("xpsom-image-and-text.oxps", 1, Dialect::OpenXps),
        ("xpsom-gradients.oxps", 1, Dialect::OpenXps),
    ];
    assert_eq!(
        expected.len(),
        PACKAGES.len(),
        "the sweep covers the corpus"
    );
    for (name, pages, dialect) in expected {
        let document = Document::open(package(name))
            .unwrap_or_else(|e| panic!("{name} should open as a fixed document: {e:?}"));
        assert_eq!(document.page_count(), *pages, "{name}: pages");
        let report = document
            .archive()
            .unwrap_or_else(|| panic!("{name}: a synthesised document has a report"));
        assert_eq!(report.xps_dialect(), Some(*dialect), "{name}: dialect");
        for number in 0..*pages {
            let page = document
                .page(number)
                .unwrap_or_else(|| panic!("{name}: page {number}"));
            assert_eq!(
                page.size(),
                (612.0, 792.0),
                "{name}: page {number} states Width=\"816\" Height=\"1056\", \
                 which is 612 x 792 pt at 18.1's 1/96 inch"
            );
        }
        // And **no page is a placeholder**. Milestones 3 to 5 gave every page
        // `XpsPageDefect::NotDrawn`; milestone 6 draws the markup, so a page
        // that still carried a page-level warning would be one that failed to
        // read rather than one that failed to be drawn.
        let placeheld: Vec<&ArchiveWarning> = report
            .warnings()
            .iter()
            .filter(|w| matches!(w, ArchiveWarning::XpsPage { .. }))
            .collect();
        assert!(
            placeheld.is_empty(),
            "{name}: every page reads and draws, and these did not: {placeheld:?}"
        );
    }
}

/// The `.oxps` twin of a `.xps` is the same document, part names and all.
///
/// Milestone 1 measured that a dialect conversion keeps the resource GUIDs and
/// even the obfuscated font's 189 252 bytes, so the two `image-and-text`
/// packages differ in namespace and in nothing this milestone reads. If the
/// discrimination had keyed on anything but the namespace — the content type is
/// the obvious wrong answer — these two would not agree.
#[test]
fn the_two_dialects_of_one_document_open_the_same_way() {
    let one = Document::open(package("wpf-image-and-text.xps")).expect("XPS 1.0");
    let other = Document::open(package("xpsom-image-and-text.oxps")).expect("OpenXPS");
    assert_eq!(one.page_count(), other.page_count());
    assert_eq!(
        one.page(0).map(|p| p.size()),
        other.page(0).map(|p| p.size())
    );
    let (one, other) = (
        one.archive().expect("a report"),
        other.archive().expect("a report"),
    );
    assert_eq!(one.xps_dialect(), Some(Dialect::Xps1));
    assert_eq!(other.xps_dialect(), Some(Dialect::OpenXps));
    assert_eq!(
        one.pages()
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        other
            .pages()
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        "the two dialects name their fixed pages identically"
    );
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
/// *Amended, milestone 3.* **The media-type column is now checked**, through
/// OPC 7.2.3.5's ordered algorithm rather than by extension. Milestone 1 left
/// it out because the algorithm did not exist and wrote the column anyway, so
/// this milestone inherits fifty-two real answers to check against — including
/// the upper-case `Extension="ODTTF"` in both dialects, which 7.2.3.5 says
/// matches case-insensitively and which a byte comparison against `odttf` finds
/// nothing for. `(none)` is 7.2.3.5's third step and is a real answer: it is
/// what `[Content_Types].xml` resolves to, since it is an item and never a
/// part.
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
    let mut resolved = 0usize;
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
        let items: Vec<(String, &'static str, u64, u64)> = archive
            .entries()
            .iter()
            .map(|entry| {
                let method = match entry.method {
                    tinker_pdf_zip::Method::Stored => "stored",
                    tinker_pdf_zip::Method::Deflated => "deflate",
                    other => {
                        panic!("{name}: OPC 7.3.6 allows only stored and deflate, found {other:?}")
                    }
                };
                (
                    entry.name.clone(),
                    method,
                    entry.compressed_size,
                    entry.uncompressed_size,
                )
            })
            .collect();
        let mut opc = tinker_pdf::xps::opc::Package::open(
            archive,
            tinker_pdf_zip::limits::MAX_ZIP_NAME_LEN,
            tinker_pdf_xml::Limits::DEFAULT,
        );

        for (item, method, compressed, uncompressed) in items {
            let row = rows
                .next()
                .unwrap_or_else(|| panic!("{name}: inventory ran out"));
            let mut field = row.split('\t');
            assert_eq!(field.next(), Some(*name), "row {seen}: package");
            assert_eq!(field.next(), Some(item.as_str()), "row {seen}: item");

            // 7.2.3.5, and the whole reason it is an *ordered* algorithm rather
            // than a table of extensions.
            let media = match PartName::from_item(&item) {
                Some(part) => opc
                    .media_type(&part)
                    .unwrap_or_else(|e| panic!("{name}/{item}: {e}"))
                    .map(str::to_string),
                // Not a part name at all, which `[Content_Types].xml` is not.
                None => None,
            };
            if media.is_some() {
                resolved += 1;
            }
            assert_eq!(
                field.next(),
                Some(media.as_deref().unwrap_or("(none)")),
                "row {seen}: {name}/{item}: media type"
            );

            assert_eq!(field.next(), Some(method), "row {seen}: method");
            assert_eq!(
                field.next().and_then(|v| v.parse::<u64>().ok()),
                Some(compressed),
                "row {seen}: compressed size"
            );
            assert_eq!(
                field.next().and_then(|v| v.parse::<u64>().ok()),
                Some(uncompressed),
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
    // And the column is not all `(none)`, which is the shape a resolver that
    // silently answered nothing would also produce.
    assert_eq!(
        resolved,
        seen - PACKAGES.len(),
        "every item but each package's `[Content_Types].xml` resolves to a \
         media type"
    );
}

/// Every ZIP item name in the corpus survives OPC 7.3.5 and 7.3.4 in both
/// directions.
///
/// Fifty-two names, and the round trip is the point: item name to part name to
/// item name is the identity, and part name to item name to part name is too.
/// A mapping that percent-decoded `%41` to `A` would fail the first of those on
/// any name that carried one, which is why RFC 3987 §3.2 decodes **only** the
/// escapes that stand for characters outside US-ASCII.
///
/// `[Content_Types].xml` is asserted to fail the mapping rather than skipped:
/// OPC 7.3.7 chose the brackets *because* they violate 6.2.2.2's grammar, so a
/// reader that special-cased it by name would be papering over a validator that
/// does not work.
#[test]
fn every_item_name_in_the_inventory_round_trips() {
    let mut names = 0usize;
    for name in PACKAGES {
        let bytes = package(name);
        let archive = tinker_pdf_zip::Archive::open(&bytes, &tinker_pdf_zip::Limits::DEFAULT)
            .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        for entry in archive.entries() {
            let item = entry.name.as_str();
            if item == "[Content_Types].xml" {
                assert!(
                    PartName::from_item(item).is_none(),
                    "{name}: the content-types item is not a part name"
                );
                continue;
            }
            let part = PartName::from_item(item)
                .unwrap_or_else(|| panic!("{name}/{item} is not a part name"));
            assert_eq!(part.item_name(), item, "{name}: item -> part -> item");
            let again = PartName::from_item(&part.item_name())
                .unwrap_or_else(|| panic!("{name}/{item}: part -> item -> part"));
            assert_eq!(
                again.as_str(),
                part.as_str(),
                "{name}: part -> item -> part"
            );
            assert!(
                part.as_str().starts_with('/'),
                "{name}: a part name is absolute"
            );
            names += 1;
        }
    }
    assert!(names >= 40, "only {names} names round-tripped");
}

/// Relationship targets in the real corpus, resolved against the **source
/// part**.
///
/// Milestone 1 found both spellings in one corpus and this is where they meet:
/// WPF writes an *absolute* `Target="/FixedDocumentSequence.fdseq"` in the
/// package relationships part and a *relative*
/// `Target="../../../Resources/….png"` in a page's, while the XPS object model
/// writes the package one relative. A resolver that handled one form refuses
/// one of the two producers, and one that resolved a part relationship against
/// the *relationships* part rather than its source is one segment off — which
/// no absolute-only fixture can see.
#[test]
fn a_page_relationship_resolves_three_levels_up_against_its_source_part() {
    for name in ["wpf-image-and-text.xps", "xpsom-image-and-text.oxps"] {
        let bytes = package(name);
        let archive = tinker_pdf_zip::Archive::open(&bytes, &tinker_pdf_zip::Limits::DEFAULT)
            .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        let mut opc = tinker_pdf::xps::opc::Package::open(
            archive,
            tinker_pdf_zip::limits::MAX_ZIP_NAME_LEN,
            tinker_pdf_xml::Limits::DEFAULT,
        );
        let page = PartName::from_item("Documents/1/Pages/1.fpage").expect("a part name");
        let relationships = opc
            .relationships(Some(&page))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(relationships.len(), 2, "{name}: a PNG and an ODTTF");

        let resolved: Vec<String> = relationships
            .iter()
            .map(|r| {
                assert!(
                    r.target.starts_with("../../../"),
                    "{name}: {} is the relative form milestone 1 measured",
                    r.target
                );
                let part = page
                    .resolve(&r.target)
                    .unwrap_or_else(|| panic!("{name}: {} does not resolve", r.target));
                assert!(
                    opc.has(&part),
                    "{name}: {part} is not a part of the package"
                );
                part.as_str().to_string()
            })
            .collect();
        assert_eq!(
            resolved,
            [
                "/Resources/234b4a6b-25d1-4c77-bd16-cfcaa91dbca9.png",
                "/Resources/595c31af-dbe8-48a5-a032-c677a052f501.ODTTF",
            ],
            "{name}: three levels up from /Documents/1/Pages/"
        );

        // And the upper-case extension resolves case-insensitively, which is
        // 7.2.3.5's rule and the one a byte comparison gets wrong.
        let font = PartName::from_item("Resources/595c31af-dbe8-48a5-a032-c677a052f501.ODTTF")
            .expect("a part name");
        assert_eq!(
            opc.media_type(&font).expect("content types"),
            Some("application/vnd.ms-package.obfuscated-opentype"),
            "{name}: `<Default Extension=\"ODTTF\">` against `….ODTTF`"
        );
    }
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
