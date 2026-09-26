//! Comic archives that real archivers wrote.
//!
//! Gap 29 closed having **never opened a `.cbz` a real archiver produced**.
//! Every fixture it had was built from APPNOTE 6.3.10's field layouts by
//! `cbz_support::zip`, three milestones recorded the debt as owed, and the
//! sixth had to write it into the gap's closing statement as a limitation of
//! the whole gap: *"The first real archive this meets may find something, and
//! nothing here would have."* [`docs/features/cbz.md`](../../../docs/features/cbz.md)
//! repeated it as an honest caveat and [`docs/ROADMAP.md`](../../../docs/ROADMAP.md)
//! carried it into tier 4.
//!
//! This file is that debt paid. The archives beside it in `cbz/` were written
//! by five independent ZIP implementations, none of them this one, over pages
//! authored here — and the pages are authored by `cbz_support`'s own writers,
//! which are the PNG and JPEG specifications transcribed, so the pictures are
//! ours without qualification and the archives are ours through somebody
//! else's tool. That is the shape `tests/epub/README.md` and `tests/xps/README.md`
//! already argue for, and ruling 13 admits: a third-party program may generate
//! an input, it may never say whether the output is right.
//!
//! # The cross-producer identity, which is what five writers buy
//!
//! One real archive proves the reader can open one real archive. Five prove
//! something stronger and it costs nothing to ask: **the same pages, packed by
//! five implementations that share no code, must come back as the same
//! pictures at the same sizes in the same order.** Five ZIP writers disagreeing
//! about what they wrote is a fact about the archives; five agreeing while this
//! reader is wrong requires the reader to be wrong in a way that is invariant
//! across stored entries, deflated entries, data descriptors and three
//! different central-directory layouts. That is not an oracle — nothing here
//! renders anything but this engine — it is a relation between reads, in the
//! shape `docs/verification.md`'s fourth corpus axis already uses.
//!
//! # Natural order is the assertion that matters
//!
//! The page names are `page1`, `page2`, `page3`, `page10`, `page11` and every
//! page is a different size, so the three candidate orderings give three
//! different sequences of dimensions. Lexicographic order — the one a reader
//! gets for free and the one that fails invisibly — reads 1, 10, 11, 2, 3 with
//! every page present and the comic unreadable.

mod cbz_support;

use std::path::{Path, PathBuf};
use tinker_pdf::{cbz, ArchiveWarning, ComicInfoDefect, Container, Document, Name, RenderOptions};
use tinker_pdf_zip::{Archive, Limits as ZipLimits, Method};

/// The pages, in the order a reader of the comic should meet them, with the
/// size each one is. Every size differs from every other, so a sequence of
/// sizes names an ordering.
const PAGES: &[(&str, u32, u32)] = &[
    ("page1.png", 60, 80),
    ("page2.png", 64, 88),
    ("page3.jpg", 48, 96),
    ("page10.png", 56, 72),
    ("page11.png", 72, 64),
];

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cbz")
}

/// Writes the pictures `make-corpus.ps1` packs.
///
/// `#[ignore]`d because it writes into the tree: it is how `source/` was
/// obtained, not something a test run should do. It lives here rather than in
/// the PowerShell script for the reason `docs/verification.md` gives for the
/// twelve fuzz seed corpora written the same way — the fixtures and the code
/// that makes them cannot drift when they are the same code. A PNG encoder
/// transcribed a second time into PowerShell would be a second implementation
/// of `cbz_support::rgb_png`, and the two would disagree eventually.
#[test]
#[ignore = "writes the source pictures into the tree; run it to regenerate them"]
fn write_the_source_pages() {
    let dir = corpus().join("source");
    std::fs::create_dir_all(&dir).expect("the corpus directory");
    for (name, width, height) in PAGES {
        let bytes = if name.ends_with(".jpg") {
            cbz_support::grey_jpeg(*width as u16, *height as u16)
        } else {
            cbz_support::rgb_png(
                *width,
                *height,
                &cbz_support::distinct_pixels(*width, *height),
            )
        };
        std::fs::write(dir.join(name), &bytes).expect("a source page");
        println!("{name}  {width} x {height}  {} bytes", bytes.len());
    }
}

/// The five archives that are ZIPs, in the order `INVENTORY.tsv` sorts them.
const ZIPS: &[&str] = &[
    "7z-deflate.cbz",
    "7z-store.cbz",
    "pwsh.cbz",
    "python.cbz",
    "winrar.cbz",
];

/// The containers that are not ZIPs and **are** read, with the sniff that
/// recognises each.
///
/// They join `ZIPS` in the cross-producer identity below rather than getting a
/// check of their own: what is worth asserting about a `.cbt` is not that it
/// opens, it is that it opens as *the same five pictures* a `.cbz` of the same
/// pages does.
/// The three `.cb7`s are one producer asked for three *shapes* rather than
/// three producers, and the difference is stated here because it is the row's
/// own caveat: `-m0=LZMA2` writes one folder holding one LZMA2 chunk, so the
/// folder walk and the chunk loop each ran exactly once for every committed
/// archive until `-ms=off` and `-m0=LZMA2:d8k:c8k` were added.
/// `the_two_cb7s_added_for_coverage_have_the_structure_they_are_named_for` in
/// `tinker-pdf-archive` asserts that they really hold those shapes.
const READ_CONTAINERS: &[(&str, Container)] = &[
    ("7z-tar.cbt", Container::Tar),
    ("7z-lzma2.cb7", Container::SevenZip),
    ("7z-nonsolid.cb7", Container::SevenZip),
    ("7z-dictreset.cb7", Container::SevenZip),
];

/// The containers that open but do **not** produce all five pages, and what
/// stops each.
///
/// A third list, and it exists because a two-way split stopped describing the
/// tree. `winrar-rar5.cbr` is not refused — it opens, walks its six records and
/// hands back four of the five pictures — and it is not in `READ_CONTAINERS`
/// either, because the fifth is a placeholder. Filing it under either would be
/// a claim this lane has not earned: "refused" is false, and "read" is the
/// sentence the exit criterion means.
const PARTLY_READ: &[(&str, Container, usize)] = &[("winrar-rar5.cbr", Container::Rar, 1)];

fn read(name: &str) -> Vec<u8> {
    let path = corpus().join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// One archive's rows of `INVENTORY.tsv`, computed by this crate's own reader.
fn inventory_of(name: &str) -> Vec<String> {
    let bytes = read(name);
    let archive = Archive::open(&bytes, &ZipLimits::DEFAULT)
        .unwrap_or_else(|e| panic!("{name} opens as an archive: {e:?}"));
    archive
        .entries()
        .iter()
        .map(|entry| {
            let method = match entry.method {
                Method::Stored => "stored".to_string(),
                Method::Deflated => "deflate".to_string(),
                Method::Other(code) => format!("method{code}"),
            };
            format!(
                "{name}\t{}\t{method}\t{}\t{}\t{:08x}",
                entry.name,
                entry.compressed_size,
                entry.uncompressed_size,
                entry.crc.unwrap_or(0)
            )
        })
        .collect()
}

/// **Two independent readers agree about every entry of every archive.**
///
/// `INVENTORY.tsv` is written by `inventory.ps1` through .NET's
/// `System.IO.Compression`, and recomputed here through `tinker-pdf-zip` on
/// every `cargo test`. It is the device `tests/xps/INVENTORY.tsv` already uses,
/// and it is admissible for the reason ruling 13 gives: the committed file is
/// the dated output of a tool that was run once, not a program adjudicating
/// this one. What the comparison is worth is that the two readers reach their
/// answers differently — .NET exposes no method code and its column is inferred
/// from whether the two lengths are equal, where this reader has the central
/// directory's own field.
#[test]
fn the_inventory_matches_the_archives() {
    let committed = std::fs::read_to_string(corpus().join("INVENTORY.tsv")).expect("INVENTORY.tsv");
    let mut lines = committed.lines();
    assert_eq!(
        lines.next(),
        Some("archive\tentry\tmethod\tcompressed\tuncompressed\tcrc32"),
        "the inventory's header"
    );
    let expected: Vec<&str> = lines.filter(|line| !line.trim().is_empty()).collect();
    let computed: Vec<String> = ZIPS.iter().flat_map(|name| inventory_of(name)).collect();
    assert_eq!(
        computed.len(),
        expected.len(),
        "the archives have {} entries and the inventory {} rows; regenerate it with \
         `pwsh -NoProfile -File crates/tinker-pdf/tests/cbz/inventory.ps1`",
        computed.len(),
        expected.len()
    );
    for (row, want) in computed.iter().zip(&expected) {
        assert_eq!(row, want, "an inventory row");
    }
}

/// **The debt this file exists to pay.** Five archives, five writers that share
/// no code, and the pages come back in the order a reader of the comic needs.
///
/// The stored order is deliberately wrong: `make-corpus.ps1` packs
/// `page1, page10, page11, page2, page3`, so an implementation that trusted the
/// archive's own order, or sorted lexicographically, would page the comic
/// 1, 10, 11, 2, 3 — every page present and nothing anywhere saying so.
#[test]
fn every_real_archive_opens_and_pages_in_natural_order() {
    let want: Vec<&str> = PAGES.iter().map(|(name, _, _)| *name).collect();
    for name in ZIPS {
        let document = Document::open(read(name)).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        let report = document
            .archive()
            .unwrap_or_else(|| panic!("{name} is a synthesised document"));
        let order: Vec<&str> = report.pages().iter().map(|p| p.name.as_str()).collect();
        assert_eq!(order, want, "{name}: page order");
        assert!(
            report.pages().iter().all(|page| page.defect.is_none()),
            "{name}: every page is its entry's own picture rather than a placeholder"
        );
        for (index, (page, width, height)) in PAGES.iter().enumerate() {
            let bitmap = document
                .page(index as u32)
                .unwrap_or_else(|| panic!("{name}: page {index}"))
                .render(&RenderOptions::default());
            assert_eq!(
                (bitmap.width, bitmap.height),
                (*width, *height),
                "{name}: {page} is one image pixel to one PDF point"
            );
        }
    }
}

/// **Every container this build reads, one reader, one comic.**
///
/// One real archive proves this reader opens one real archive. The relation
/// across several costs nothing more to ask and proves more: the archives share
/// no code, they disagree about which entries to store and which to deflate,
/// and two of them deflate entries to *more* bytes than they started with — and
/// every page still has to come back as the same picture at the same size.
///
/// **This is the exit criterion for every container decoder in
/// `docs/design/comic-archives.md`, and it needs no oracle.** The `.cbt`
/// beside the five ZIPs holds the *same five pages*, put there by a different
/// program before any decoder for it existed, so a decoder is right exactly
/// when the rasters it produces are byte-identical to the ones the
/// already-trusted ZIP path produces. A new container joins `READ_CONTAINERS`
/// and is held to the same sentence rather than getting a check of its own.
///
/// Nothing outside this repository renders anything here. Both sides of the
/// comparison are this engine reading two files, which is the shape
/// `docs/verification.md`'s fourth corpus axis already uses: a relation between
/// two reads needs no ground truth and can be asked of everything at once.
#[test]
fn five_zip_writers_produce_the_same_five_pictures() {
    let others: Vec<&str> = ZIPS[1..]
        .iter()
        .copied()
        .chain(READ_CONTAINERS.iter().map(|(name, _)| *name))
        .collect();
    let first = Document::open(read(ZIPS[0])).expect("the first archive opens");
    for index in 0..PAGES.len() as u32 {
        let reference = first
            .page(index)
            .expect("a page")
            .render(&RenderOptions::default());
        // Ink, so that agreement about this page means something. It is *not*
        // "some pixel differs from some other": `page3.jpg` is `grey_jpeg`'s
        // DC-only baseline JPEG and is one flat grey by construction, which is
        // exactly what makes it worth having — the pass-through path for a
        // JPEG is the one that copies bytes and never builds a raster, and a
        // picture with no detail still proves it ran. A page that drew nothing
        // is white, and that is the state this rules out.
        assert!(
            reference.data.iter().any(|&byte| byte != 0xFF),
            "page {index} of {} drew ink rather than nothing",
            ZIPS[0]
        );
        for name in &others {
            let document = Document::open(read(name)).unwrap_or_else(|e| panic!("{name}: {e:?}"));
            let bitmap = document
                .page(index)
                .unwrap_or_else(|| panic!("{name}: page {index}"))
                .render(&RenderOptions::default());
            assert_eq!(
                (bitmap.width, bitmap.height),
                (reference.width, reference.height),
                "{name}: page {index} is not the size {}'s is",
                ZIPS[0]
            );
            assert!(
                bitmap.data == reference.data,
                "{name}: page {index} is a different picture from {}'s",
                ZIPS[0]
            );
        }
    }
}

/// **A `.cbr` opens, pages what it stored, and names what it did not.**
///
/// The RAR fixture is the one archive in this corpus that does not produce
/// five pictures, and the shape of what it *does* produce is the whole of
/// ruling 2. `winrar-rar5.cbr` holds four PNGs WinRAR stored, one JPEG it
/// compressed with method 3, and a `QO` quick-open service record. So:
///
/// - the archive **opens**, rather than being refused for the one entry;
/// - it has **five pages**, in reading order, because the entry that could not
///   be decompressed keeps its page number;
/// - four of them are the ZIP's own pictures at the ZIP's own sizes;
/// - the fifth carries `PageDefect::RarEntryRefused` naming **method 3**,
///   which is a sentence a host can show and a user can act on.
///
/// This test is what stops the CBR row being quietly closed. The lane's exit
/// criterion is `five_zip_writers_produce_the_same_five_pictures`, and this
/// archive is deliberately not in it; if the decompressor lands, this test is
/// what has to be deleted, and deleting it is a visible act.
#[test]
fn the_rar_a_real_archiver_wrote_pages_what_it_stored_and_names_what_it_did_not() {
    for (name, what, expected_defects) in PARTLY_READ {
        let bytes = read(name);
        assert_eq!(cbz::container(&bytes), Some(*what), "{name}: the sniff");
        let document = Document::open(bytes).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        let report = document.archive().expect("a synthesised document");

        let want: Vec<&str> = PAGES.iter().map(|(page, _, _)| *page).collect();
        let order: Vec<&str> = report.pages().iter().map(|p| p.name.as_str()).collect();
        assert_eq!(order, want, "{name}: page order, placeholders included");

        let defects: Vec<&str> = report
            .pages()
            .iter()
            .filter(|p| p.defect.is_some())
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(
            defects.len(),
            *expected_defects,
            "{name}: pages that could not be built ({defects:?})"
        );
        assert_eq!(
            defects,
            ["page3.jpg"],
            "{name}: the entry WinRAR compressed is the one that is a placeholder"
        );

        // The defect names the method rather than saying "no". A host that
        // shows "compressed with method 3" tells a user to re-pack with -m0;
        // one that shows "could not read" tells them nothing.
        let page3 = report
            .pages()
            .iter()
            .find(|p| p.name == "page3.jpg")
            .expect("page3");
        assert_eq!(
            page3.defect,
            Some(cbz::PageDefect::RarEntryRefused(
                cbz::RarEntryError::Compressed { method: 3 }
            )),
            "{name}: the placeholder names its method"
        );

        // And the four that are stored are the same pictures the ZIPs give.
        let reference = Document::open(read(ZIPS[0])).expect("the reference archive");
        for (index, (page, width, height)) in PAGES.iter().enumerate() {
            if *page == "page3.jpg" {
                continue;
            }
            let bitmap = document
                .page(index as u32)
                .unwrap_or_else(|| panic!("{name}: page {index}"))
                .render(&RenderOptions::default());
            assert_eq!(
                (bitmap.width, bitmap.height),
                (*width, *height),
                "{name}: {page} is one image pixel to one PDF point"
            );
            let want = reference
                .page(index as u32)
                .unwrap_or_else(|| panic!("page {index} of the reference"))
                .render(&RenderOptions::default());
            assert_eq!(
                bitmap.data, want.data,
                "{name}: {page} is not the picture the ZIP gives"
            );
        }
    }
}

/// **The `.cbt` a real archiver wrote opens, and its pages are its entries'
/// own pictures.**
///
/// The order assertion is the one that matters and it is not free: 7-Zip packs
/// the five pages as `page1, page10, page11, page2, page3` — deliberately not
/// the reading order — so a reader that trusted the archive's own header order,
/// or sorted lexicographically, pages the comic 1, 10, 11, 2, 3 with every page
/// present and nothing anywhere saying so. tar has no central directory to
/// re-order things behind, which makes this the strictest form of that check in
/// the corpus.
#[test]
fn the_tar_a_real_archiver_wrote_pages_in_natural_order() {
    let want: Vec<&str> = PAGES.iter().map(|(name, _, _)| *name).collect();
    let document = Document::open(read("7z-tar.cbt")).expect("the .cbt opens");
    let report = document.archive().expect("a synthesised document");
    let order: Vec<&str> = report.pages().iter().map(|p| p.name.as_str()).collect();
    assert_eq!(order, want, "the .cbt's page order");
    assert!(
        report.pages().iter().all(|page| page.defect.is_none()),
        "every page is its entry's own picture rather than a placeholder"
    );
    assert!(
        report.warnings().is_empty(),
        "7-Zip's tar is not a damaged one: {:?}",
        report.warnings()
    );
    for (index, (page, width, height)) in PAGES.iter().enumerate() {
        let bitmap = document
            .page(index as u32)
            .unwrap_or_else(|| panic!("page {index}"))
            .render(&RenderOptions::default());
        assert_eq!(
            (bitmap.width, bitmap.height),
            (*width, *height),
            "{page} is one image pixel to one PDF point"
        );
    }
}

/// **The `.cb7` a real archiver wrote opens, and the archive's own CRC-32 is
/// what says its decompressor is right.**
///
/// This is the assertion tier 4's largest piece rests on. `7z-lzma2.cb7` was
/// written by 7-Zip with `-m0=LZMA2` before this engine had an LZMA decoder,
/// and it records a CRC-32 per file in its own header. So a wrong window, a
/// mis-set probability array or an ignored LZMA2 dictionary reset fails the
/// *format's* check inside `sevenz::Archive::read` and becomes a placeholder
/// page — which means "every page is its entry's own picture" below is a
/// statement about the decompressor and not only about the container.
///
/// It also exercises the part of the format that surprises: this archive's
/// **header is itself compressed**, with plain LZMA rather than the LZMA2 its
/// data uses, so listing these five names at all requires the other decoder.
///
/// # Three of them, because one shape is not the format
///
/// `-m0=LZMA2` is what a desktop archiver writes and it writes the *simplest*
/// thing the format allows: one folder, holding one LZMA2 chunk. So for as
/// long as it was the only `.cb7` here, `decode_folder`'s walk and
/// `decode_lzma2`'s chunk loop were each entered exactly once by every
/// committed archive, and the second iteration of either was reached by
/// nothing. The other two ask the same producer for the two shapes that make
/// those loops run:
///
/// - `7z-nonsolid.cb7` (`-ms=off`) is **five folders**, one per page, so the
///   walk runs past folder 0 five times over five different pack offsets;
/// - `7z-dictreset.cb7` (`-m0=LZMA2:d8k:c8k`) is one folder holding **three
///   LZMA2 chunks**, each opening with a dictionary reset — two of them
///   mid-stream, at output offsets that fall inside a page rather than between
///   two.
///
/// The structure is asserted where it can be, in
/// `the_two_cb7s_added_for_coverage_have_the_structure_they_are_named_for`,
/// because a flag is a request and not a result. What is asserted *here* is
/// the thing that matters: whatever shape the folders and chunks take, the
/// five pictures come back, adjudicated by the archive's own CRC-32.
#[test]
fn the_7z_a_real_archiver_wrote_pages_in_natural_order() {
    let want: Vec<&str> = PAGES.iter().map(|(name, _, _)| *name).collect();
    for name in READ_CONTAINERS
        .iter()
        .filter(|(_, kind)| *kind == Container::SevenZip)
        .map(|(name, _)| *name)
    {
        let document = Document::open(read(name)).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        let report = document.archive().expect("a synthesised document");
        let order: Vec<&str> = report.pages().iter().map(|p| p.name.as_str()).collect();
        assert_eq!(order, want, "{name}: page order");
        assert!(
            report.pages().iter().all(|page| page.defect.is_none()),
            "{name}: every page is its entry's own picture rather than a placeholder: {:?}",
            report.pages().iter().map(|p| p.defect).collect::<Vec<_>>()
        );
        for (index, (page, width, height)) in PAGES.iter().enumerate() {
            let bitmap = document
                .page(index as u32)
                .unwrap_or_else(|| panic!("{name}: page {index}"))
                .render(&RenderOptions::default());
            assert_eq!(
                (bitmap.width, bitmap.height),
                (*width, *height),
                "{name}: {page} is one image pixel to one PDF point"
            );
        }
    }
}

/// The five pages of the corpus, as `source/` holds them.
///
/// Read from the committed pictures rather than regenerated, so an archive
/// built here holds the same bytes the eight producers were handed.
fn source_pages() -> Vec<(String, Vec<u8>)> {
    PAGES
        .iter()
        .map(|(name, _, _)| {
            let path = corpus().join("source").join(name);
            let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            ((*name).to_owned(), bytes)
        })
        .collect()
}

/// A comic archive of the corpus' own pages plus one `ComicInfo.xml`.
///
/// Written by `cbz_support::zip` rather than by a producer, and that is the
/// honest shape rather than a shortcut: no committed archive carries a
/// `ComicInfo.xml`, because none of the four writers puts one there — an
/// archiver frames files and a comic *manager* is what writes the metadata.
/// So the pictures are the corpus' and the metadata entry is this
/// repository's, and the test below says which half proves what.
fn comic_with_metadata(comic_info: &[u8]) -> Vec<u8> {
    let pages = source_pages();
    let mut files: Vec<cbz_support::ZipFile> = pages
        .iter()
        .map(|(name, bytes)| cbz_support::ZipFile::stored(name, bytes))
        .collect();
    files.push(cbz_support::ZipFile::stored("ComicInfo.xml", comic_info));
    cbz_support::zip(&files, cbz_support::Damage::None)
}

/// **The metadata a comic carries reaches the document it describes.**
///
/// Gap 29 skipped `ComicInfo.xml` by name, and was right about two of its three
/// reasons: it is not a page, and warning about it would bury the warnings that
/// matter. The third — that nothing downstream had a use for a title — stopped
/// being true when the book path started writing `dc:title` into `/Info`, and
/// this is that asymmetry closed.
///
/// Read out of the trailer's own `/Info` dictionary rather than through the
/// report, because the report is this build's account of what it did and the
/// dictionary is what a reader will actually find.
#[test]
fn the_metadata_a_comic_carries_reaches_the_document() {
    let bytes = comic_with_metadata(
        b"<ComicInfo><Series>Nightwatch</Series><Number>12</Number>\
          <Writer>A. Writer</Writer></ComicInfo>",
    );
    let document = Document::open(bytes).expect("a comic with metadata opens");

    // The pages are untouched: metadata is a third thing an entry can be, and
    // adding it must not have made the archive one page longer or shorter.
    let report = document.archive().expect("a synthesised document");
    let order: Vec<&str> = report.pages().iter().map(|p| p.name.as_str()).collect();
    let want: Vec<&str> = PAGES.iter().map(|(name, _, _)| *name).collect();
    assert_eq!(
        order, want,
        "ComicInfo.xml is not a page and did not become one"
    );
    assert!(
        report.warnings().is_empty(),
        "a ComicInfo.xml this build reads is not a leniency: {:?}",
        report.warnings()
    );

    let info = report
        .comic_info()
        .expect("the report carries what it read");
    assert_eq!(info.series(), Some("Nightwatch"));
    assert_eq!(info.number(), Some("12"));
    assert_eq!(info.writer(), Some("A. Writer"));

    let cos = document.cos();
    let dict = cos
        .get(
            cos.trailer()
                .get_ref(Name::INFO)
                .expect("an /Info reference"),
        )
        .expect("the /Info resolves");
    let dict = dict.as_dict().expect("an /Info dictionary");
    let entry = |key: &[u8]| -> String {
        dict.get_string(cos.intern(key))
            .map(|s| String::from_utf8_lossy(&s.bytes).into_owned())
            .unwrap_or_default()
    };
    assert_eq!(entry(b"Title"), "Nightwatch #12");
    assert_eq!(entry(b"Author"), "A. Writer");
    assert_eq!(entry(b"Keywords"), "Nightwatch #12");
    assert_eq!(entry(b"Subject"), "", "no <Summary>, so no /Subject");
}

/// A `ComicInfo.xml` this build cannot read costs the archive nothing but its
/// title — and says so by name (rulings 2 and 10).
#[test]
fn unreadable_metadata_degrades_and_is_named() {
    let bytes = comic_with_metadata(b"<ComicInfo><Title>unclosed");
    let document = Document::open(bytes).expect("the pages open regardless");
    let report = document.archive().expect("a synthesised document");
    assert_eq!(
        report.pages().len(),
        PAGES.len(),
        "every page is still here"
    );
    assert_eq!(report.comic_info(), None);
    assert!(
        report
            .warnings()
            .contains(&ArchiveWarning::ComicInfo(ComicInfoDefect::Unreadable)),
        "the defect is named: {:?}",
        report.warnings()
    );
}

/// What five real archivers did that eight years of hand-built fixtures never
/// did — printed, so the census is a measurement rather than a memory.
#[test]
#[ignore = "prints the census README.md records"]
fn the_census_of_what_real_archivers_do() {
    for name in ZIPS {
        let bytes = read(name);
        let archive = Archive::open(&bytes, &ZipLimits::DEFAULT).expect("an archive");
        let stored = archive
            .entries()
            .iter()
            .filter(|e| e.method == Method::Stored)
            .count();
        let grew = archive
            .entries()
            .iter()
            .filter(|e| e.compressed_size > e.uncompressed_size)
            .count();
        let streamed = archive.entries().iter().filter(|e| e.streamed).count();
        println!(
            "{name}\troute={:?}\tentries={}\tstored={stored}\tgrew={grew}\tstreamed={streamed}\twarnings={:?}",
            archive.route(),
            archive.entries().len(),
            archive.warnings()
        );
    }
}
