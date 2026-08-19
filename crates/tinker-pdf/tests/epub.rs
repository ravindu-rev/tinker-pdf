//! Real EPUBs, and what this build does with one today (gap 31, milestone 1).
//!
//! Every book under `tests/epub/` was written by a real producer — pandoc or
//! calibre — over text authored in `tests/epub/source/`, which is
//! `fuzz/README.md`'s *"a tool's output on our input is ours to commit"*
//! reading and the same precedent gap 30's milestone 1 used for Windows' XPS
//! serialisers. `tests/epub/README.md` beside them says which producer wrote
//! which, and why the two corpora this milestone delivers had to be two.
//!
//! **This milestone comes before any EPUB code, on purpose.** Gap 29 closed
//! having never opened a `.cbz` a real archiver produced, recorded the debt in
//! three milestones, and had to write it into the gap's closing section. Gap 30
//! answered that by buying eight genuine packages first, and its Progress
//! sections record seven things real files did that ECMA-388 did not predict —
//! two of which would have produced a reader that refuses every OpenXPS file
//! Windows writes. This file is that device a third time.
//!
//! # What was pinned here, and what happened to it
//!
//! *Amended, gap 31 milestone 3.* When this file landed, an `.epub` opened and
//! what came back was its cover. Two tests recorded that exactly —
//! `today_an_epub_opens_as_a_comic_and_this_is_what_it_reports` and
//! `today_a_book_of_two_chapters_is_one_page_of_its_cover` — and both
//! **passed**, because a defect that is watched is not the same as one that is
//! described. Milestone 3 **deleted** them, in this commit, because a record of
//! old behaviour that does not break when the behaviour changes is not a
//! record.
//!
//! The three tests below them were `#[ignore]`d and failed under `--ignored`.
//! They are ordinary tests now. Each was written to accept **either** a correct
//! read **or** a refusal by a name that is true, which is gap 30's own
//! milestone-3 correction applied before it could bite: that plan's pinned test
//! used `.expect`, so the only way it could go green was for the book to open,
//! and its milestone 3 progress records that this *"forced the spine a
//! milestone early"*. What milestone 3 delivered is the second of the two — an
//! EPUB is told from a comic and refused as `UnpaginatedBook` — and none of the
//! three had to be rewritten to accept it, which is the whole value of having
//! written them that way.
//!
//! What they do **not** assert is which refusal, on purpose. `tests/epub_ocf.rs`
//! is where the name is pinned, one fixture per refusal; these three are about
//! the corpus.

mod epub_support;

use std::collections::BTreeMap;
use std::path::PathBuf;

use epub_support::{
    classify_doctype, css_properties, doctype_shape, entries, is_content_document, is_stylesheet,
    method_name, mimetype_verdict, named_references, numeric_references, ocf_zip, read, read_at,
    todays_answer, Doctype, OcfEntry, TodaysAnswer, OCF_MEDIA_TYPE, RESERVED_META_INF,
    XML_PREDEFINED,
};
use tinker_pdf::ArchiveRefusal;
use tinker_pdf_xml::{
    Doctype as XmlDoctype, Error as XmlError, Limits as XmlLimits, Reader as XmlReader,
    Source as XmlSource, Warning as XmlWarning,
};

// ---- the committed corpus ---------------------------------------------------

/// One book, and what this build makes of it today.
struct Book {
    file: &'static str,
    /// The program that wrote the `.epub`. Milestone 1's criterion is **two
    /// producers minimum**, and that number is in the plan for a reason gap 30
    /// closed owing: its corpus is one vendor's idea of the format, and this
    /// plan's equivalent trap is that six Project Gutenberg books are one
    /// `ebookmaker`'s.
    producer: &'static str,
    version: &'static str,
    /// EPUB 3 or EPUB 2, as the package document's `version` says.
    epub: &'static str,
    /// What the file demonstrates that no other file in the corpus does.
    demonstrates: &'static str,
}

/// Written out rather than globbed, so a file deleted from the directory fails
/// a test instead of shrinking a loop to nothing — gap 29's milestone-6 lesson
/// about a sweep that reports whatever it happens to find.
const BOOKS: &[Book] = &[
    Book {
        file: "pandoc-book-cover.epub",
        producer: "pandoc",
        version: "3.10.2",
        epub: "3.0",
        demonstrates: "EPUB 3 with a navigation document and an NCX both, and one \
                       picture that is the cover",
    },
    Book {
        file: "pandoc-book-nocover.epub",
        producer: "pandoc",
        version: "3.10.2",
        epub: "3.0",
        demonstrates: "the same book with no image entry anywhere",
    },
    Book {
        file: "pandoc-book-epub2.epub",
        producer: "pandoc",
        version: "3.10.2",
        epub: "2.0",
        demonstrates: "OPF 2.0, which this plan reads as a compatibility surface",
    },
    Book {
        file: "pandoc-plates.epub",
        producer: "pandoc",
        version: "3.10.2",
        epub: "3.0",
        demonstrates: "three pictures of three different sizes, stored and deflated, \
                       written in reverse physical order",
    },
    Book {
        file: "calibre-book-cover.epub",
        producer: "calibre",
        version: "9.13.0",
        epub: "3.0",
        demonstrates: "a second producer: content documents named .html, the package \
                       document at the archive root, and a META-INF directory entry",
    },
    Book {
        file: "calibre-book-nocover.epub",
        producer: "calibre",
        version: "9.13.0",
        epub: "2.0",
        demonstrates: "the second producer with no image entry, and an NCX with no nav",
    },
];

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/epub")
}

fn book(name: &str) -> Vec<u8> {
    let path = corpus_dir().join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// Four is the number milestone 1 owes; six is what it got, and **two
/// producers** is the part of the criterion that is not satisfiable by a bigger
/// pile from one of them.
#[test]
fn the_corpus_holds_four_books_from_two_producers() {
    assert!(
        BOOKS.len() >= 4,
        "milestone 1 owes at least four books, the list names {}",
        BOOKS.len()
    );
    let mut producers: Vec<&str> = BOOKS.iter().map(|b| b.producer).collect();
    producers.sort_unstable();
    producers.dedup();
    assert!(
        producers.len() >= 2,
        "milestone 1 owes at least two producers, the list names {producers:?}"
    );
    for entry in BOOKS {
        let bytes = book(entry.file);
        assert!(
            bytes.starts_with(b"PK\x03\x04"),
            "{} does not begin with a local file header",
            entry.file
        );
    }
    // Both EPUB versions, from both producers. A corpus of six EPUB 3 books
    // would satisfy the count and prove nothing about the compatibility surface
    // milestone 4 has to read.
    for version in ["3.0", "2.0"] {
        assert!(
            BOOKS.iter().any(|b| b.epub == version),
            "no book in the corpus is EPUB {version}"
        );
    }
}

/// Provenance, per file, checked rather than written down once.
///
/// Row 1 asks for *"per-file provenance"*, and a README that names five of six
/// books is exactly the failure mode a table maintained by hand has. Every
/// field of `BOOKS` has to appear beside the file it describes.
#[test]
fn the_readme_records_provenance_for_every_book() {
    let readme = std::fs::read_to_string(corpus_dir().join("README.md"))
        .expect("README.md is committed beside the books");
    for entry in BOOKS {
        assert!(
            readme.contains(entry.file),
            "README.md does not name {}",
            entry.file
        );
        let provenance = format!("{} {}", entry.producer, entry.version);
        assert!(
            readme.contains(&provenance),
            "README.md does not record `{provenance}` for {}",
            entry.file
        );
    }
    // And the reason each file is here, which is what stops a corpus growing by
    // accretion. Two books that demonstrate the same thing are one book and a
    // copy.
    let mut reasons: Vec<&str> = BOOKS.iter().map(|b| b.demonstrates).collect();
    let before = reasons.len();
    reasons.sort_unstable();
    reasons.dedup();
    assert_eq!(
        reasons.len(),
        before,
        "two books demonstrate the same thing"
    );
}

// ---- the route, measured rather than reasoned about -------------------------

/// The plan's sharpest structural finding: **`ArchiveRefusal::UnreadablePackage`
/// is unreachable for an EPUB.**
///
/// Gap 30 built that refusal for a package carrying OPC's own two items that
/// will not resolve, and argued at length for refusing rather than falling
/// through. An EPUB is OCF and carries neither item, so ECMA-388 E.3 fails at
/// **step 2's first check** and the comic fallthrough is exactly what E.3's own
/// text asks for. **Gap 30 is not wrong; EPUB is a different question it did
/// not ask.**
#[test]
fn no_committed_book_carries_either_of_opcs_two_items() {
    for entry in BOOKS {
        let bytes = book(entry.file);
        for item in entries(&bytes) {
            assert_ne!(
                item.name, "[Content_Types].xml",
                "{} carries a content-types item, so E.3 would reach step 3",
                entry.file
            );
            assert_ne!(
                item.name, "_rels/.rels",
                "{} carries a package relationships part",
                entry.file
            );
        }
    }
}

/// And the other half, which is the one milestone 3 discriminates on.
///
/// Asserted separately from the test above rather than folded into it: "no EPUB
/// looks like an OPC package" and "every EPUB carries the thing an EPUB is
/// recognised by" are two independent claims, and a single test that happened
/// to check the first would leave milestone 3's discriminator unmeasured.
#[test]
fn every_committed_book_carries_the_ocf_container() {
    for entry in BOOKS {
        let bytes = book(entry.file);
        assert!(
            entries(&bytes)
                .iter()
                .any(|e| e.name == "META-INF/container.xml"),
            "{} has no META-INF/container.xml",
            entry.file
        );
    }
}

// ---- OCF 3.3 section 4.3.2 --------------------------------------------------

/// The `mimetype` rule, in its four independent parts.
#[test]
fn every_committed_book_puts_an_uncompressed_mimetype_first() {
    for entry in BOOKS {
        let bytes = book(entry.file);
        let verdict = mimetype_verdict(&bytes);
        assert!(verdict.present, "{} has no mimetype entry", entry.file);
        assert!(
            verdict.first_by_offset,
            "{}'s mimetype is not the first file in the archive",
            entry.file
        );
        assert!(verdict.stored, "{}'s mimetype is compressed", entry.file);
        assert!(
            verdict.exact_bytes,
            "{}'s mimetype is not exactly the twenty bytes",
            entry.file
        );
        // No extra field, which §4.3.2 also requires — and which cannot be
        // checked here, because `tinker_pdf_zip::Entry` publishes the name, the
        // method, both sizes, the CRC, three flag bits, the header offset and
        // the directory index, and **not the extra field's length**. Recorded
        // as owed rather than quietly skipped: milestone 3's criterion needs
        // it, and closing it means either an accessor on `Entry` or a byte
        // check in the facade. Measured by hand on all six books here: zero.
    }
}

/// The trap milestone 3's exit criterion names, and the reason it needs a
/// fixture rather than a book.
///
/// §4.3.2 says `mimetype` is *"the first file in the ZIP archive"*, which is a
/// statement about physical position. The obvious implementation checks the
/// central directory's first row instead. **No real book can tell the two
/// apart** — all six here and all twenty fetched put it first in both — so the
/// distinction has to come from a container built to have it.
#[test]
fn no_real_book_distinguishes_physical_order_from_directory_order() {
    for entry in BOOKS {
        let verdict = mimetype_verdict(&book(entry.file));
        assert_eq!(
            verdict.first_by_offset, verdict.first_by_index,
            "{} now distinguishes the two checks; milestone 3 has a real witness",
            entry.file
        );
    }
}

/// And here is the container that does distinguish them, so the assertion above
/// is a measurement and not a tautology.
///
/// Two entries, `mimetype` written second and listed first. A check written
/// against `index == 0` says the book conforms; §4.3.2 says it does not.
#[test]
fn a_container_can_distinguish_physical_order_from_directory_order() {
    let files = [
        OcfEntry::deflated("META-INF/container.xml", b"<container/>"),
        OcfEntry::stored("mimetype", OCF_MEDIA_TYPE),
    ];
    // Physical order: container.xml, then mimetype. Directory order: the
    // reverse, so `mimetype` is row zero.
    let bytes = ocf_zip(&files, &[1, 0]);
    let verdict = mimetype_verdict(&bytes);
    assert!(verdict.present);
    assert!(
        verdict.first_by_index,
        "the fixture does not list mimetype first, so it tests nothing"
    );
    assert!(
        !verdict.first_by_offset,
        "the fixture does not write mimetype second, so it tests nothing"
    );
}

/// Each of §4.3.2's four requirements fails on its own, so a verdict that is
/// four booleans is four booleans rather than one dressed up.
#[test]
fn the_mimetype_verdict_fails_each_requirement_separately() {
    let container = OcfEntry::deflated("META-INF/container.xml", b"<container/>");

    let absent = ocf_zip(
        &[OcfEntry::deflated(
            "META-INF/container.xml",
            b"<container/>",
        )],
        &[0],
    );
    assert!(!mimetype_verdict(&absent).present);

    let second = ocf_zip(
        &[
            OcfEntry::deflated("META-INF/container.xml", b"<container/>"),
            OcfEntry::stored("mimetype", OCF_MEDIA_TYPE),
        ],
        &[0, 1],
    );
    let verdict = mimetype_verdict(&second);
    assert!(verdict.present && verdict.stored && verdict.exact_bytes);
    assert!(!verdict.first_by_offset, "a mimetype written second passed");

    let compressed = ocf_zip(
        &[
            OcfEntry::deflated("mimetype", OCF_MEDIA_TYPE),
            OcfEntry::deflated("META-INF/container.xml", b"<container/>"),
        ],
        &[0, 1],
    );
    let verdict = mimetype_verdict(&compressed);
    assert!(verdict.present && verdict.first_by_offset && verdict.exact_bytes);
    assert!(!verdict.stored, "a deflated mimetype passed");

    let wrong = ocf_zip(
        &[
            OcfEntry::stored("mimetype", b"application/epub+zip\n"),
            container,
        ],
        &[0, 1],
    );
    let verdict = mimetype_verdict(&wrong);
    assert!(verdict.present && verdict.first_by_offset && verdict.stored);
    assert!(
        !verdict.exact_bytes,
        "a mimetype with a trailing newline passed, and §4.3.2 says exactly the string"
    );
}

// ---- META-INF ---------------------------------------------------------------

/// OCF §4.2.6.3 lists **six** reserved names in `META-INF`, and the tone of the
/// section reads as though that is the whole of what goes there.
///
/// It is not, and the file that says so came out of the producer this milestone
/// commissioned rather than out of the corpus it downloaded: **every pandoc
/// book carries `META-INF/com.apple.ibooks.display-options.xml`**, and none of
/// the twenty fetched books carries anything unreserved at all. A milestone 3
/// that refused an unrecognised `META-INF` entry would refuse every book pandoc
/// writes and pass the entire fetched corpus, which is as good an argument for
/// two corpora as this milestone has.
#[test]
fn a_committed_book_puts_a_file_in_meta_inf_that_is_not_one_of_the_six() {
    let mut unreserved: BTreeMap<String, Vec<&'static str>> = BTreeMap::new();
    for entry in BOOKS {
        let bytes = book(entry.file);
        for item in entries(&bytes) {
            if !item.name.starts_with("META-INF/")
                || item.is_directory()
                || RESERVED_META_INF.contains(&item.name.as_str())
            {
                continue;
            }
            unreserved
                .entry(item.name.clone())
                .or_default()
                .push(entry.file);
        }
    }
    println!("  unreserved META-INF entries: {unreserved:?}");
    assert!(
        unreserved.contains_key("META-INF/com.apple.ibooks.display-options.xml"),
        "no book carries the vendor file this test exists for: {unreserved:?}"
    );
    assert_eq!(
        unreserved["META-INF/com.apple.ibooks.display-options.xml"].len(),
        4,
        "pandoc wrote it into a different number of books than the four it produced"
    );
}

/// One producer writes a **directory entry** for `META-INF/`, deflated, zero
/// bytes long; the other writes none.
///
/// A ZIP directory entry is a name that ends in `/` and holds nothing
/// (APPNOTE 4.4.17.1). Milestone 3 walks `META-INF` looking for reserved names
/// and will meet one, and a walk that treated every `META-INF/*` entry as a
/// file would meet an empty one first.
#[test]
fn one_producer_writes_a_meta_inf_directory_entry_and_the_other_does_not() {
    let mut with: Vec<&str> = Vec::new();
    let mut without: Vec<&str> = Vec::new();
    for entry in BOOKS {
        let bytes = book(entry.file);
        let has = entries(&bytes)
            .iter()
            .any(|e| e.is_directory() && e.name == "META-INF/");
        if has { &mut with } else { &mut without }.push(entry.producer);
    }
    with.sort_unstable();
    with.dedup();
    without.sort_unstable();
    without.dedup();
    assert_eq!(with, ["calibre"], "the producers that write one changed");
    assert_eq!(without, ["pandoc"], "the producers that write none changed");
}

// ---- the inventory ----------------------------------------------------------

/// `INVENTORY.tsv` recomputed through **this repository's own** ZIP reader.
///
/// The file is written by `inventory.ps1` through .NET's own central-directory
/// walk, and this recomputes name, method, header offset and both sizes through
/// `tinker-pdf-zip` and compares every row. So the inventory cannot drift from
/// the books, and two independent ZIP readers have to agree about all
/// seventy-two entries.
///
/// **The media-type column is deliberately not checked**, for gap 30's reason
/// in a different format: resolving one means following `container.xml` to the
/// package document and reading its manifest, which is milestone 3's and
/// milestone 4's work. The column is committed so that when they land they have
/// a table of real answers to check against — including the fact that one
/// producer's `application/xhtml+xml` items are named `.html`.
#[test]
fn inventory_matches_the_books() {
    let text = std::fs::read_to_string(corpus_dir().join("INVENTORY.tsv"))
        .expect("INVENTORY.tsv is committed beside the books");
    let mut lines = text.lines();
    assert_eq!(
        lines.next(),
        Some("book\tentry\tmedia_type\tmethod\theader_offset\tcompressed\tuncompressed"),
        "the inventory's header changed"
    );

    let mut expected: BTreeMap<&str, Vec<Vec<&str>>> = BTreeMap::new();
    for line in lines.filter(|l| !l.is_empty()) {
        let fields: Vec<&str> = line.split('\t').collect();
        assert_eq!(fields.len(), 7, "inventory row has the wrong shape: {line}");
        expected.entry(fields[0]).or_default().push(fields);
    }
    assert_eq!(
        expected.len(),
        BOOKS.len(),
        "the inventory names a different set of books than the corpus does"
    );

    let mut rows = 0usize;
    for entry in BOOKS {
        let bytes = book(entry.file);
        let ours = entries(&bytes);
        let theirs = expected
            .get(entry.file)
            .unwrap_or_else(|| panic!("{} is not in the inventory", entry.file));
        assert_eq!(
            ours.len(),
            theirs.len(),
            "{}: our reader found {} entries, the inventory names {}",
            entry.file,
            ours.len(),
            theirs.len()
        );
        for (mine, row) in ours.iter().zip(theirs) {
            assert_eq!(mine.name, row[1], "{}: entry name", entry.file);
            assert_eq!(
                method_name(mine.method),
                row[3],
                "{}: {} method",
                entry.file,
                mine.name
            );
            assert_eq!(
                mine.header_offset.to_string(),
                row[4],
                "{}: {} header offset",
                entry.file,
                mine.name
            );
            assert_eq!(
                mine.compressed_size.to_string(),
                row[5],
                "{}: {} compressed size",
                entry.file,
                mine.name
            );
            assert_eq!(
                mine.uncompressed_size.to_string(),
                row[6],
                "{}: {} uncompressed size",
                entry.file,
                mine.name
            );
            rows += 1;
        }
    }
    assert_eq!(
        rows, 72,
        "the inventory covers a different number of entries"
    );
    println!("  {rows} entries agreed on by two independent ZIP readers");
}

/// Both compression methods appear, in one archive, from one producer, in one
/// run.
///
/// pandoc deflates one of `pandoc-plates.epub`'s three PNGs and **stores** the
/// other two, because deflate does not help them. Gap 30 recorded the same
/// habit in Microsoft's two serialisers and called it *"neither producer is
/// consistent about it"*; it is not a Microsoft habit, it is what a producer
/// that measures does.
#[test]
fn one_book_holds_both_compression_methods() {
    let bytes = book("pandoc-plates.epub");
    let images: Vec<_> = entries(&bytes)
        .into_iter()
        .filter(|e| e.name.ends_with(".png"))
        .collect();
    assert_eq!(images.len(), 3);
    let stored = images
        .iter()
        .filter(|e| e.method == tinker_pdf_zip::Method::Stored)
        .count();
    assert_eq!(stored, 2, "the mix of methods in the plates book changed");
}

/// And the physical order of those three pictures is the **reverse** of the
/// order the book names them in.
///
/// Milestone 3 resolves a spine, not a directory listing, and this is the file
/// that says the two are different questions rather than the same one seen
/// twice.
#[test]
fn the_plates_are_written_in_reverse_of_the_order_the_book_names_them() {
    let bytes = book("pandoc-plates.epub");
    let mut images: Vec<_> = entries(&bytes)
        .into_iter()
        .filter(|e| e.name.ends_with(".png"))
        .collect();
    images.sort_by_key(|e| e.header_offset);
    let physical: Vec<&str> = images.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        physical,
        [
            "EPUB/media/file2.png",
            "EPUB/media/file1.png",
            "EPUB/media/file0.png"
        ],
        "the physical order of the plates changed"
    );
}

// ---- epubcheck --------------------------------------------------------------

/// Printed once per test that actually invoked epubcheck. CI greps for it.
const EPUBCHECK_RAN: &str = "epubcheck-oracle: RAN";

/// Printed once per test that could not. CI greps for it too, and fails.
const EPUBCHECK_SKIPPED: &str = "epubcheck-oracle: SKIPPED";

/// One row of `EPUBCHECK.tsv`.
#[derive(Debug, PartialEq, Eq)]
struct Verdict {
    fatal: u32,
    error: u32,
    warning: u32,
    usage: u32,
    /// `SEVERITY:CODE`, sorted and deduplicated.
    messages: Vec<String>,
}

fn recorded_verdicts() -> BTreeMap<String, Verdict> {
    let text = std::fs::read_to_string(corpus_dir().join("EPUBCHECK.tsv"))
        .expect("EPUBCHECK.tsv is committed beside the books");
    let mut lines = text.lines();
    assert_eq!(
        lines.next(),
        Some("book\tfatal\terror\twarning\tusage\tmessages"),
        "EPUBCHECK.tsv's header changed"
    );
    lines
        .filter(|l| !l.is_empty())
        .map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            assert!(f.len() >= 5, "epubcheck row has the wrong shape: {line}");
            let mut messages: Vec<String> = f
                .get(5)
                .unwrap_or(&"")
                .split_whitespace()
                .map(str::to_owned)
                .collect();
            messages.sort();
            messages.dedup();
            (
                f[0].to_owned(),
                Verdict {
                    fatal: f[1].parse().expect("fatal count"),
                    error: f[2].parse().expect("error count"),
                    warning: f[3].parse().expect("warning count"),
                    usage: f[4].parse().expect("usage count"),
                    messages,
                },
            )
        })
        .collect()
}

/// The recorded verdicts describe exactly the corpus, whether or not epubcheck
/// is installed.
///
/// Separate from the test that re-runs the tool, because the two can fail
/// independently: a book added without a verdict is a gap in the record even on
/// a machine with no Java, and that is the failure a gated test alone would
/// hide behind a `SKIPPED`.
#[test]
fn every_book_has_a_recorded_epubcheck_verdict() {
    let recorded = recorded_verdicts();
    assert_eq!(
        recorded.len(),
        BOOKS.len(),
        "EPUBCHECK.tsv names a different number of books than the corpus holds"
    );
    for entry in BOOKS {
        assert!(
            recorded.contains_key(entry.file),
            "{} has no recorded epubcheck verdict",
            entry.file
        );
    }
    // Five clean and one not. A corpus in which every producer's default output
    // is clean would be a corpus that had not met calibre's.
    let warned: Vec<&str> = recorded
        .iter()
        .filter(|(_, v)| v.warning > 0 || v.error > 0 || v.fatal > 0)
        .map(|(k, _)| k.as_str())
        .collect();
    assert_eq!(
        warned,
        ["calibre-book-cover.epub"],
        "the set of books epubcheck is unhappy with changed"
    );
}

/// Re-runs epubcheck and checks the recorded verdicts still hold.
///
/// Ruling 9's posture: epubcheck is a subprocess, never a dependency, nothing
/// is vendored, and its output is a transient comparison reference. What it
/// gives is exactly one thing and it is worth having — **when this engine and a
/// book disagree, epubcheck says whose fault it is.**
///
/// Set `TINKER_EPUBCHECK` to `epubcheck.jar` or to an `epubcheck` executable;
/// `TINKER_JAVA` names the JVM when `java` is not on `PATH`.
#[test]
fn the_recorded_epubcheck_verdicts_still_hold() {
    let Some(command) = epubcheck() else {
        println!("{EPUBCHECK_SKIPPED} -- TINKER_EPUBCHECK is unset or does not run");
        return;
    };
    println!("{EPUBCHECK_RAN} over {} committed books", BOOKS.len());
    let recorded = recorded_verdicts();
    for entry in BOOKS {
        let path = corpus_dir().join(entry.file);
        let mut run = std::process::Command::new(&command[0]);
        run.args(&command[1..]).arg(&path);
        let output = run
            .output()
            .unwrap_or_else(|e| panic!("epubcheck did not run over {}: {e}", entry.file));
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        let fresh = parse_epubcheck(&text);
        assert_eq!(
            fresh, recorded[entry.file],
            "{}: epubcheck now says something else",
            entry.file
        );
    }
}

/// How to invoke epubcheck, or `None`.
///
/// A version query rather than a `which`: a jar that will not start is not an
/// oracle, and this is the one call whose failure means *skip* rather than
/// *fail*.
fn epubcheck() -> Option<Vec<String>> {
    let named = std::env::var("TINKER_EPUBCHECK").ok()?;
    let command = if named.ends_with(".jar") {
        let java = std::env::var("TINKER_JAVA").unwrap_or_else(|_| "java".to_owned());
        vec![java, "-jar".to_owned(), named]
    } else {
        vec![named]
    };
    let ok = std::process::Command::new(&command[0])
        .args(&command[1..])
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    ok.then_some(command)
}

/// Counts `SEVERITY(CODE)` occurrences in epubcheck's own output.
///
/// The text form rather than `--json`, because parsing JSON here would mean
/// either a dependency this repository does not take or a parser nobody asked
/// for. The severities are the five epubcheck prints, and `USAGE` is counted
/// under its own name rather than folded into the informational bucket.
fn parse_epubcheck(text: &str) -> Verdict {
    let mut verdict = Verdict {
        fatal: 0,
        error: 0,
        warning: 0,
        usage: 0,
        messages: Vec::new(),
    };
    for line in text.lines() {
        for (severity, count) in [
            ("FATAL", &mut verdict.fatal),
            ("ERROR", &mut verdict.error),
            ("WARNING", &mut verdict.warning),
            ("USAGE", &mut verdict.usage),
        ] {
            let prefix = format!("{severity}(");
            if let Some(rest) = line.strip_prefix(&prefix) {
                if let Some(end) = rest.find(')') {
                    *count += 1;
                    verdict
                        .messages
                        .push(format!("{severity}:{}", &rest[..end]));
                }
            }
        }
    }
    verdict.messages.sort();
    verdict.messages.dedup();
    verdict
}

// ---- the doctype census -----------------------------------------------------

/// How many content documents carry a document type declaration, and in which
/// shape.
///
/// This is what decision 3 rests on. `tinker-pdf-xml` refuses `<!DOCTYPE`
/// before one byte after it is read — `Error::DoctypeUnsupported`, with four
/// committed bombs behind the refusal — so the census decides whether
/// milestone 2 is a nicety or a blocker.
///
/// **It is a blocker, and the two producers disagree about why.** pandoc writes
/// `<!DOCTYPE html>` on every EPUB 3 content document and the XHTML 1.1 public
/// identifier on every EPUB 2 one; calibre writes **no declaration at all**, in
/// either version. So a reader built on the parser as it stands reads every
/// calibre book and refuses every pandoc one — which is a sharper statement
/// than the plan's, and it is the corpus that supplies it rather than the
/// specification.
///
/// **And the quote character is the finding this corpus alone has.** The plan
/// measured the XHTML 1.1 identifier **single-quoted**, on Project Gutenberg's
/// EPUB 2 books, and milestone 2's exit criteria name that form. pandoc writes
/// the same identifier **double-quoted** — the settlement table's other row,
/// which 241 fetched content documents supply zero of. Neither corpus shows
/// both forms; together they do.
#[test]
fn the_committed_doctype_census() {
    let mut shapes: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut identifiers: BTreeMap<String, usize> = BTreeMap::new();
    let mut documents = 0usize;
    let mut producers_with_a_doctype: Vec<&str> = Vec::new();
    let mut producers_without: Vec<&str> = Vec::new();
    for entry in BOOKS {
        let bytes = book(entry.file);
        let mut per_book: BTreeMap<&'static str, usize> = BTreeMap::new();
        for (_, text) in content_documents(&bytes) {
            documents += 1;
            let doctype = classify_doctype(&text);
            *shapes.entry(doctype_shape(&doctype)).or_default() += 1;
            *per_book.entry(doctype_shape(&doctype)).or_default() += 1;
            if let Doctype::Public { id, .. } = &doctype {
                *identifiers.entry(id.clone()).or_default() += 1;
            }
        }
        println!("  {:38} {per_book:?}", entry.file);
        if per_book.keys().all(|s| *s == "none") {
            &mut producers_without
        } else {
            &mut producers_with_a_doctype
        }
        .push(entry.producer);
    }
    producers_with_a_doctype.sort_unstable();
    producers_with_a_doctype.dedup();
    producers_without.sort_unstable();
    producers_without.dedup();
    println!("  {documents} content documents: {shapes:?}");
    println!("  public identifiers: {identifiers:?}");
    println!("  producers that write a doctype: {producers_with_a_doctype:?}");
    println!("  producers that write none: {producers_without:?}");
    assert!(documents > 0, "the census read no content document at all");
    assert_eq!(
        producers_with_a_doctype,
        ["pandoc"],
        "the set of producers whose books milestone 2 is required for changed"
    );
    assert_eq!(
        producers_without,
        ["calibre"],
        "the set of producers whose books read without milestone 2 changed"
    );
    // The double-quoted public identifier: the settlement table's own row, and
    // the one no fetched book supplies.
    assert!(
        shapes.get("public-double-quoted").copied().unwrap_or(0) > 0,
        "the committed corpus no longer supplies a double-quoted external identifier, \
         which is the shape 241 fetched content documents have none of"
    );
    assert!(
        identifiers.contains_key("-//W3C//DTD XHTML 1.1//EN"),
        "the identifier EPUB 3.3 Appendix B leaves out is no longer in the corpus"
    );
    // The construct all four of gap 30's committed bombs live in. If a real
    // book ever carried one, milestone 2's decision to refuse it by name would
    // need re-arguing rather than inheriting.
    assert_eq!(
        shapes.get("internal-subset").copied().unwrap_or(0),
        0,
        "a committed book carries an internal DTD subset"
    );
}

/// **Milestone 2's mode, over the books milestone 1 committed**, in both
/// directions and against real bytes.
///
/// The census above says what shape each declaration has. This says what the
/// parser does with it: every content document the shipped parser refuses as a
/// declaration is read *whole* by the relaxed one, and every document it
/// already reads is unchanged, event for event. Both halves are needed — a mode
/// that read nothing and a mode that read everything each satisfy one of them.
///
/// **This sweep is corroboration and not the assertion.** Each rule the mode
/// rests on — the three shapes, both quote characters, a `>` inside a public
/// literal, the internal subset, the identifier set — is pinned by its own
/// fixture in `tinker-pdf-xml`, because a defect caught only by a broad
/// end-to-end fixture does not say which rule broke. What this adds is that the
/// fixtures describe the same construct two real producers write.
#[test]
fn the_relaxed_mode_reads_the_committed_books_the_shipped_parser_refuses() {
    let mut refused_today = 0usize;
    let mut unchanged = 0usize;
    let mut identifiers: BTreeMap<String, usize> = BTreeMap::new();
    let mut warned = 0usize;
    let mut per_producer: BTreeMap<&str, usize> = BTreeMap::new();

    for entry in BOOKS {
        let bytes = book(entry.file);
        for (name, text) in content_documents(&bytes) {
            let where_ = format!("{} {name}", entry.file);
            let shape = doctype_shape(&classify_doctype(&text));
            let source =
                XmlSource::new(text.as_bytes()).unwrap_or_else(|error| panic!("{where_}: {error}"));

            let mut strict = source.reader(&XmlLimits::DEFAULT);
            let strict_refusal = first_refusal(&mut strict);
            let mut relaxed = source.reader_with(&XmlLimits::DEFAULT, XmlDoctype::SkipExternalId);
            let relaxed_refusal = first_refusal(&mut relaxed);

            if shape == "none" {
                // calibre's books, which read today. The mode must not touch
                // them: same refusal (none), same warnings.
                unchanged += 1;
                assert_eq!(strict_refusal, None, "{where_} does not read today");
                assert_eq!(relaxed_refusal, None, "{where_} stopped reading");
                assert_eq!(strict.warnings(), relaxed.warnings(), "{where_}");
                assert_eq!(relaxed.external_identifier(), None, "{where_}");
                continue;
            }

            // pandoc's books, every one of which the shipped parser refuses.
            refused_today += 1;
            *per_producer.entry(entry.producer).or_default() += 1;
            assert_eq!(
                strict_refusal,
                Some(XmlError::DoctypeUnsupported),
                "{where_} is not refused by the shipped parser, so this row proves nothing",
            );
            assert_eq!(
                relaxed_refusal, None,
                "{where_} is still refused under the relaxed mode",
            );

            let identifier = relaxed.external_identifier();
            let says_not_allowed = relaxed
                .warnings()
                .contains(&XmlWarning::ExternalIdentifierNotAllowed);
            match shape {
                // `<!DOCTYPE html>`: no identifier, so nothing to warn about.
                "bare" => {
                    assert_eq!(identifier, None, "{where_}");
                    assert!(!says_not_allowed, "{where_} warned about nothing");
                }
                "public-double-quoted" | "public-single-quoted" => {
                    let identifier = identifier.expect("a PUBLIC form names one");
                    let public = identifier.public().expect("a PUBLIC form has one");
                    *identifiers.entry(public.to_owned()).or_default() += 1;
                    assert!(
                        says_not_allowed,
                        "{where_} names {public}, which Appendix B does not hold, silently",
                    );
                    warned += 1;
                }
                other => panic!("{where_}: the corpus grew a {other} declaration"),
            }
        }
    }

    println!("  refused today and read now: {refused_today} ({per_producer:?})");
    println!("  read today and unchanged:   {unchanged}");
    println!("  identifiers named:          {identifiers:?}");
    assert!(
        refused_today > 0,
        "no committed content document is refused today, so the relaxation is untested"
    );
    assert!(
        unchanged > 0,
        "every committed content document is refused today, so `unchanged` is untested"
    );
    assert_eq!(
        warned,
        identifiers.values().sum::<usize>(),
        "a public identifier was read without a warning beside it"
    );
    assert_eq!(
        identifiers.keys().collect::<Vec<_>>(),
        ["-//W3C//DTD XHTML 1.1//EN"],
        "the set of identifiers the committed corpus names changed",
    );
}

/// One reader run to its first refusal, or to the end.
fn first_refusal(reader: &mut XmlReader<'_>) -> Option<XmlError> {
    for event in reader.by_ref() {
        if let Err(error) = event {
            return Some(error);
        }
    }
    None
}

/// The two producers name their content documents differently, and the census
/// finds both.
///
/// calibre writes `index_split_000.html` and declares it `application/xhtml+xml`
/// in the manifest; pandoc writes `.xhtml`. A census keyed on `.xhtml` alone
/// would report **zero** content documents for every calibre book and would
/// still pass a doctype census, a reference census and a document count — which
/// is why this is asserted here rather than left to those three to notice.
#[test]
fn both_producers_content_documents_are_found_despite_different_extensions() {
    let calibre = book("calibre-book-cover.epub");
    let pandoc = book("pandoc-book-cover.epub");
    assert_eq!(content_documents(&calibre).len(), 6);
    assert_eq!(content_documents(&pandoc).len(), 5);
    let xhtml_only = |bytes: &[u8]| {
        entries(bytes)
            .iter()
            .filter(|e| e.name.ends_with(".xhtml"))
            .count()
    };
    // calibre writes `.xhtml` for the two files it generates itself — the
    // navigation document and the title page — and `.html` for the four it
    // split the input into. So the extension does not even agree with itself
    // inside one book, which is a sharper statement than "one producer differs
    // from the other".
    assert_eq!(xhtml_only(&calibre), 2);
    assert_eq!(xhtml_only(&pandoc), 5);
}

/// The scanner can read every shape the plan's settlement table names,
/// including the three no real book in either corpus supplies.
///
/// This is the test that makes the census's zeroes mean something. Twenty-six
/// real books produced exactly two external-identifier shapes between them —
/// the bare `<!DOCTYPE html>` and the single-quoted XHTML 1.1 public
/// identifier — so a census reporting zero double-quoted forms, zero `SYSTEM`
/// forms and zero internal subsets is indistinguishable from a scanner that
/// cannot see them, unless something proves it can.
#[test]
fn the_doctype_scanner_reads_every_shape_the_settlement_table_names() {
    assert_eq!(classify_doctype("<html/>"), Doctype::None);
    assert_eq!(classify_doctype("<!DOCTYPE html><html/>"), Doctype::Bare);
    assert_eq!(
        classify_doctype("<!doctype HTML>"),
        Doctype::Bare,
        "the declaration's keyword is case-insensitive and its name is not"
    );
    assert_eq!(
        classify_doctype(
            "<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Strict//EN\" \"xhtml1-strict.dtd\">"
        ),
        Doctype::Public {
            quote: '"',
            id: "-//W3C//DTD XHTML 1.0 Strict//EN".to_owned()
        }
    );
    assert_eq!(
        classify_doctype(
            "<!DOCTYPE html PUBLIC '-//W3C//DTD XHTML 1.1//EN' \
             'http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd'>"
        ),
        Doctype::Public {
            quote: '\'',
            id: "-//W3C//DTD XHTML 1.1//EN".to_owned()
        }
    );
    assert_eq!(
        classify_doctype("<!DOCTYPE html SYSTEM \"about:legacy-compat\">"),
        Doctype::System { quote: '"' }
    );
    // A name that merely begins with a keyword is not the keyword. The scan
    // accumulates whole words rather than searching for a substring, and this
    // is the difference: `PUBLICATION` is a root element name and the
    // declaration that follows it is a `SYSTEM` one.
    assert_eq!(
        classify_doctype("<!DOCTYPE PUBLICATION SYSTEM \"x.dtd\">"),
        Doctype::System { quote: '"' }
    );
    // And with no external identifier at all, where a prefix match would
    // invent one: a scanner reporting `Public` for `<!DOCTYPE publicity >`
    // hands milestone 2 an identifier that is not in the file.
    assert_eq!(classify_doctype("<!DOCTYPE publicity >"), Doctype::Bare);
    assert_eq!(classify_doctype("<!DOCTYPE systemic >"), Doctype::Bare);
    assert_eq!(
        classify_doctype("<!DOCTYPE html [ <!ENTITY a \"b\"> ]>"),
        Doctype::InternalSubset
    );
    assert_eq!(classify_doctype("<!DOCTYPE html"), Doctype::Unterminated);
}

/// A `>` inside a public literal does not end the declaration.
///
/// Milestone 2's exit criteria name this case explicitly, and it is asserted
/// here because it is the one that decides whether "skip to the matching `>`"
/// is a scan or a substring search. A scanner that stopped at the first `>`
/// would report a `Public` whose identifier is truncated, and would then hand
/// the rest of the declaration to the element grammar.
#[test]
fn the_doctype_scanner_is_not_ended_by_a_gt_inside_a_literal() {
    assert_eq!(
        classify_doctype("<!DOCTYPE html PUBLIC \"-//a>b//EN\" \"x.dtd\"><html/>"),
        Doctype::Public {
            quote: '"',
            id: "-//a>b//EN".to_owned()
        }
    );
    // And the internal subset is found even when a literal before it holds a
    // `[`, which is the mirror of the same mistake.
    assert_eq!(
        classify_doctype("<!DOCTYPE html PUBLIC \"-//a[b//EN\" \"x.dtd\" [ <!ENTITY e \"f\"> ]>"),
        Doctype::InternalSubset
    );
}

// ---- the named-character-reference census -----------------------------------

/// The census that settles the plan's one genuinely open question, over the
/// committed corpus.
///
/// XHTML's `&nbsp;`, `&mdash;` and `&hellip;` are declared by the DTD the
/// doctype mode discards, and are not among XML's five predefined entities. The
/// plan's working assumption was option 2 — a vendored table of the ~250 XHTML
/// 1.0 names, with each use warning. **The measurement refutes it**: zero uses
/// across all 270 content documents of both corpora.
///
/// A zero needs corroboration or it is worthless, and the plan named the
/// corroboration it expected — *"producers overwhelmingly write `&#160;`"*.
/// **That is not what these two do.** They write neither form: the em dash, the
/// ellipsis, the non-breaking hyphen and the Japanese line in `source/book.md`
/// all reach the content documents as **literal UTF-8**, and the numeric count
/// is zero too. (The fetched corpus writes the numeric form 65 times against
/// 83 240 literal non-ASCII characters, so the habit exists and is a rounding
/// error.) So the assertions below count non-ASCII characters as well, and the
/// zero means "these producers escape nothing" rather than "this text is ASCII"
/// — which is the difference between evidence and an artefact of the text this
/// milestone happened to write.
#[test]
fn the_committed_named_character_reference_census() {
    let mut named: BTreeMap<String, usize> = BTreeMap::new();
    let mut numeric = 0usize;
    let mut non_ascii = 0usize;
    let mut documents = 0usize;
    for entry in BOOKS {
        let bytes = book(entry.file);
        for (_, text) in content_documents(&bytes) {
            documents += 1;
            for (reference, count) in named_references(&text) {
                *named.entry(reference).or_default() += count;
            }
            numeric += numeric_references(&text);
            non_ascii += text.chars().filter(|c| !c.is_ascii()).count();
        }
    }
    println!(
        "  {documents} content documents: {} named references, {numeric} numeric, \
         {non_ascii} literal non-ASCII characters",
        named.len()
    );
    println!("  {named:?}");
    assert!(documents > 0, "the census read no content document at all");
    assert!(
        named.is_empty(),
        "a committed book now uses a named character reference: {named:?}"
    );
    assert_eq!(
        numeric, 0,
        "a committed book now uses a numeric character reference"
    );
    assert!(
        non_ascii > 0,
        "the corpus is pure ASCII, so its zero says nothing about the entity table"
    );
}

/// The scanner can find one, so the zero above is a measurement.
#[test]
fn the_named_reference_scanner_can_find_one() {
    let text = "<p>a&nbsp;b &mdash; c &hellip; d &amp; e &#160; f &#x2014; g & h</p>";
    let found = named_references(text);
    assert_eq!(
        found,
        vec![
            ("hellip".to_owned(), 1),
            ("mdash".to_owned(), 1),
            ("nbsp".to_owned(), 1)
        ],
        "the scanner missed a name, or counted one it should not"
    );
    for predefined in XML_PREDEFINED {
        assert!(
            !found.iter().any(|(n, _)| n == predefined),
            "the scanner counted XML's own {predefined}"
        );
    }
    // A name is an XML name, so it does not begin with a digit. `&123;` is not
    // a character reference in any spelling and counting it would inflate the
    // census with prices and dates.
    assert!(named_references("&123; &1a; &#38;").is_empty());
    // The two questions the plan's sentence "producers overwhelmingly write
    // `&#160;`" puts side by side, kept apart: a numeric reference is not a
    // named one and is not evidence about the entity table.
    assert_eq!(numeric_references(text), 2);
    assert_eq!(numeric_references("&#;&# ;&nbsp;&#xZZ;"), 0);
}

// ---- the CSS property census ------------------------------------------------

/// The census that replaces the plan's forty-one-name list with evidence.
#[test]
fn the_committed_css_property_census() {
    let mut union: BTreeMap<String, usize> = BTreeMap::new();
    let mut sheets = 0usize;
    for entry in BOOKS {
        let bytes = book(entry.file);
        let mut per_book: Vec<String> = Vec::new();
        for (_, text) in stylesheets(&bytes) {
            sheets += 1;
            for property in css_properties(&text) {
                if !per_book.contains(&property) {
                    per_book.push(property.clone());
                }
                *union.entry(property).or_default() += 1;
            }
        }
        println!("  {:38} {} properties", entry.file, per_book.len());
    }
    println!(
        "  {sheets} stylesheets, {} distinct properties",
        union.len()
    );
    println!("  {:?}", union.keys().collect::<Vec<_>>());
    assert!(sheets > 0, "the census read no stylesheet at all");
    // Every producer here writes a `page-break-*` property, which is the pair
    // milestone 7's fragmentation criterion says appear in every measured book
    // and is the reason CSS 2.2 §13.3 is in that row rather than a later one.
    assert!(
        union.contains_key("page-break-before") || union.contains_key("page-break-after"),
        "no committed stylesheet asks for a page break"
    );
}

/// The census does not read a pseudo-class as a property.
///
/// It did, on its first run: `@media screen { a:hover { … } }` puts the
/// selector `a:hover` at brace depth one, where it reads exactly like a
/// declaration until the `{` arrives, and `a` was reported as a CSS property of
/// the fetched corpus. A property list that a layout engine's `Known` enum is
/// built from cannot have selectors in it.
#[test]
fn the_css_census_does_not_read_a_pseudo_class_as_a_property() {
    let sheet = "
        /* a comment with a { brace and a ; semicolon */
        p { color: red; margin-top: 1em }
        @media screen and (min-width: 30em) {
            a:hover { text-decoration: underline; }
            p::first-line { font-variant: small-caps }
        }
        @font-face { src: url(x.otf); font-family: 'X' }
        :root { --brand: #333 }
    ";
    let found = css_properties(sheet);
    assert!(
        !found.iter().any(|p| p == "a" || p == "p"),
        "a selector reached the property list: {found:?}"
    );
    for property in [
        "color",
        "margin-top",
        "text-decoration",
        "font-variant",
        "src",
        "font-family",
        "--brand",
    ] {
        assert!(
            found.iter().any(|p| p == property),
            "{property} is missing from {found:?}"
        );
    }
    // The last declaration of a block has no semicolon, and a census that only
    // committed on `;` would lose one property per rule.
    assert!(found.iter().any(|p| p == "margin-top"));
}

// ---- pinned by milestone 1, resolved by milestone 3 -------------------------

/// Whether a refusal is a sentence that is **true of a book**.
///
/// `NoImages`'s own documentation reads *"a valid archive with no image
/// entries"*, which is a sentence about a comic archive; the rest of the list
/// is either about a damaged ZIP or about an OPC package, and none of the six
/// books is any of those. The catch-all answers `true` on purpose: when this
/// was written the honest name did not exist, `ArchiveRefusal` is
/// `#[non_exhaustive]`, and the point of the pinned tests was to accept
/// **whatever milestone 3 added** rather than to name it here and schedule the
/// work.
///
/// *Milestone 3 added four*, and the list below is unchanged: the catch-all
/// covers `UnpaginatedBook`, `UnreadableContainer`, `RootfileMissing` and
/// `EncryptedResources` without this function having to learn them. That is
/// what the shape bought, and it is why `Encrypted` is on the excluded list —
/// reusing that variant for a book with an `encryption.xml` would have been the
/// obvious shortcut and would have failed here.
fn refusal_is_true_of_a_book(why: ArchiveRefusal) -> bool {
    !matches!(
        why,
        ArchiveRefusal::NoImages
            | ArchiveRefusal::NotAZip
            | ArchiveRefusal::Damaged
            | ArchiveRefusal::Encrypted
            | ArchiveRefusal::MultiDisk
            | ArchiveRefusal::Zip64OutOfBounds
            | ArchiveRefusal::UnreadablePackage
    )
}

/// The predicate above is checked here, in a test that **runs**.
///
/// It was written because the three tests using it did not run: an
/// `#[ignore]`d test that would pass is not a pin, and nothing in a normal
/// `cargo test` run would say so. If `NoImages` had ever counted as a sentence
/// that is true of a book, all three would have gone green with nothing done at
/// all.
///
/// It is still here now that they run, and for a sharper reason: it is what
/// says the three below are satisfied by a **new** name rather than by
/// `ArchiveRefusal::Encrypted`, which is the variant a milestone 3 in a hurry
/// would have reused for a book carrying an `encryption.xml`.
#[test]
fn no_images_is_not_a_sentence_that_is_true_of_a_book() {
    assert!(!refusal_is_true_of_a_book(ArchiveRefusal::NoImages));
    assert!(!refusal_is_true_of_a_book(
        ArchiveRefusal::UnreadablePackage
    ));
    assert!(!refusal_is_true_of_a_book(ArchiveRefusal::Damaged));
    // And the catch-all answers `true`, which is what let the three tests below
    // accept a name milestone 1 had not seen invented. `TooLarge` stood for
    // that: "a bound was spent" is a sentence that can be true of a book. The
    // four milestone 3 actually added are asserted here too, so this predicate
    // cannot drift away from the refusals it is now being satisfied by.
    assert!(refusal_is_true_of_a_book(ArchiveRefusal::TooLarge));
    assert!(refusal_is_true_of_a_book(ArchiveRefusal::UnpaginatedBook));
    assert!(refusal_is_true_of_a_book(
        ArchiveRefusal::UnreadableContainer
    ));
    assert!(refusal_is_true_of_a_book(ArchiveRefusal::RootfileMissing));
    assert!(refusal_is_true_of_a_book(
        ArchiveRefusal::EncryptedResources
    ));
}

/// A book whose only picture is its cover is not a one-page book of that
/// picture.
///
/// **Failed until milestone 3**, which is where it went green. Either the book
/// reads — in which case its pages are not its pictures — or it is refused by a
/// name that is true of a book, which is what milestone 3 delivers on its own.
/// Both are accepted, so this row does not schedule milestone 4's pagination,
/// and it did not have to be rewritten when the second arm was the one that
/// came true.
#[test]
fn an_epub_whose_only_picture_is_its_cover_is_not_a_one_page_book_of_it() {
    let bytes = book("pandoc-book-cover.epub");
    let pictures = image_entries(&bytes);
    assert_eq!(
        pictures.len(),
        1,
        "the fixture no longer has exactly one picture"
    );
    match todays_answer(&bytes) {
        TodaysAnswer::Opens { pages, origins, .. } => {
            assert!(
                !(pages == 1 && origins == pictures),
                "the book is still one page of its cover"
            );
        }
        TodaysAnswer::Refused(why) => assert!(
            refusal_is_true_of_a_book(why),
            "refused as {why:?}, which is not a sentence about a book"
        ),
        TodaysAnswer::Other(what) => panic!("{what}"),
    }
}

/// A book with no picture at all is not refused as having no images.
///
/// **Failed until milestone 3**, and it is the other half of gap 30's pair
/// arriving verbatim: `ArchiveRefusal::NoImages` reads *"a valid archive with
/// no image entries"*, said about a book of four chapters. Independent of the
/// test above — one is a mis-read and one is a false refusal, and a build could
/// fix either without the other.
#[test]
fn an_epub_with_no_picture_at_all_is_not_refused_as_having_no_images() {
    for file in ["pandoc-book-nocover.epub", "calibre-book-nocover.epub"] {
        let bytes = book(file);
        assert!(
            image_entries(&bytes).is_empty(),
            "{file} no longer has zero pictures"
        );
        match todays_answer(&bytes) {
            TodaysAnswer::Opens { .. } => {}
            TodaysAnswer::Refused(why) => assert!(
                refusal_is_true_of_a_book(why),
                "{file} is refused as {why:?}, which is not a sentence about a book"
            ),
            TodaysAnswer::Other(what) => panic!("{file}: {what}"),
        }
    }
}

/// And the sweep: not one of the six is read as a bag of its pictures.
///
/// The third pinned test rather than a fold of the first two, because the two
/// above are about the two books that show the defect most sharply and this is
/// about the corpus. Gap 30's equivalent sweep found something its two examples
/// did not — that a three-page document with no raster refuses outright — and
/// the same shape is here: `pandoc-plates.epub` is a book of three chapters
/// that opens as three pages of three different sizes, and neither of the two
/// tests above would notice it.
#[test]
fn not_one_committed_book_is_read_as_the_book_it_is() {
    for entry in BOOKS {
        let bytes = book(entry.file);
        let pictures = image_entries(&bytes);
        match todays_answer(&bytes) {
            TodaysAnswer::Opens { origins, .. } => assert_ne!(
                origins, pictures,
                "{} is still paged by its pictures",
                entry.file
            ),
            TodaysAnswer::Refused(why) => assert!(
                refusal_is_true_of_a_book(why),
                "{} is refused as {why:?}, which is not a sentence about a book",
                entry.file
            ),
            TodaysAnswer::Other(what) => panic!("{}: {what}", entry.file),
        }
    }
}

// ---- helpers ----------------------------------------------------------------

/// Every entry whose **bytes** are a PNG or a JPEG, in the order the comic path
/// would page them.
///
/// By magic bytes and not by extension, because that is what `cbz.rs` does and
/// this has to predict its answer rather than a plausible one. The order is the
/// natural order over stored paths that `cbz::pages_from_archive` applies, which
/// for this corpus is the same as the directory order — asserted rather than
/// assumed by comparing against the origins the report hands back.
fn image_entries(bytes: &[u8]) -> Vec<String> {
    let mut named: Vec<(String, usize)> = entries(bytes)
        .iter()
        .enumerate()
        .map(|(i, e)| (e.name.clone(), i))
        .collect();
    named.sort_by(|a, b| a.0.cmp(&b.0));
    named
        .into_iter()
        .filter(|(_, index)| {
            read_at(bytes, *index).is_some_and(|data| {
                data.starts_with(b"\x89PNG\r\n\x1a\n") || data.starts_with(b"\xFF\xD8\xFF")
            })
        })
        .map(|(name, _)| name)
        .collect()
}

/// Every content document of a book, as text.
fn content_documents(bytes: &[u8]) -> Vec<(String, String)> {
    text_entries(bytes, is_content_document)
}

/// Every stylesheet of a book, as text.
fn stylesheets(bytes: &[u8]) -> Vec<(String, String)> {
    text_entries(bytes, is_stylesheet)
}

fn text_entries(bytes: &[u8], want: fn(&str) -> bool) -> Vec<(String, String)> {
    let chosen: Vec<(usize, String)> = entries(bytes)
        .iter()
        .enumerate()
        .filter(|(_, e)| want(&e.name))
        .map(|(i, e)| (i, e.name.clone()))
        .collect();
    chosen
        .into_iter()
        .filter_map(|(index, name)| {
            let raw = read_at(bytes, index)?;
            Some((name, String::from_utf8_lossy(&raw).into_owned()))
        })
        .collect()
}

/// The books say what they are: a package document that names its version.
///
/// Read as bytes rather than parsed, because nothing here parses XML yet — and
/// `BOOKS`'s `epub` column is a claim in a table until something checks it.
#[test]
fn every_book_declares_the_epub_version_the_table_claims() {
    for entry in BOOKS {
        let bytes = book(entry.file);
        let container = read(&bytes, "META-INF/container.xml")
            .unwrap_or_else(|| panic!("{} has no container", entry.file));
        let container = String::from_utf8_lossy(&container).into_owned();
        let start = container
            .find("full-path=")
            .unwrap_or_else(|| panic!("{}: no full-path", entry.file));
        let rest = &container[start + "full-path=".len() + 1..];
        let end = rest
            .find(['"', '\''])
            .unwrap_or_else(|| panic!("{}: unterminated full-path", entry.file));
        let opf_name = &rest[..end];
        let opf = read(&bytes, opf_name).unwrap_or_else(|| {
            panic!(
                "{}: container names {opf_name}, which is absent",
                entry.file
            )
        });
        let opf = String::from_utf8_lossy(&opf).into_owned();
        let claim = format!("version=\"{}\"", entry.epub);
        assert!(
            opf.contains(&claim),
            "{}: the package document does not say {claim}",
            entry.file
        );
    }
}
