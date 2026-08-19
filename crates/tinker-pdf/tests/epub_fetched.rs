//! The EPUB corpus that cannot be committed, and the censuses over it (gap 31,
//! milestone 1).
//!
//! `tests/epub/README.md` carries the licence table. Two excellent corpora
//! exist and neither may live in this repository: Project Gutenberg's books are
//! public-domain *text* under a **trademark** licence whose clause 1.E.1
//! requires the boilerplate to appear wherever a copy is "accessed, displayed,
//! performed, viewed, copied or distributed" and whose 1.E.4 forbids detaching
//! the terms; and `epub3-samples` is **CC-BY-SA 3.0**, which `deny.toml`'s
//! *"deliberately NO copyleft in this list — not even weak copyleft"* rule
//! bars. **The obvious source of committable EPUBs is barred by this
//! repository's own gate**, which is a better place to find that out than a
//! review.
//!
//! So the books are fetched instead, by `tests/epub/fetch-corpus.sh`, into a
//! directory outside the tree, and this file reads whatever is there.
//!
//! # Skipped, not silently passed
//!
//! Gap 20 found that a skipped oracle exits 0 and reads exactly like a pass.
//! Every test here prints [`RAN`] or [`SKIPPED`] and the CI job greps its own
//! output for the second and goes red. That matters more here than for any
//! oracle before it: the corpus is **not in the repository**, so a test over it
//! can fail to run for a second reason as well as the first, and both look like
//! a green tick.

mod epub_support;

use std::path::PathBuf;

use epub_support::{
    classify_doctype, css_properties, doctype_shape, entries, is_content_document, is_stylesheet,
    mimetype_verdict, named_references, numeric_references, read_at, todays_answer, Doctype,
    TodaysAnswer, RESERVED_META_INF,
};

/// Printed once per test that actually read the fetched corpus. CI greps it.
const RAN: &str = "epub-corpus: RAN";

/// Printed once per test that could not. CI greps for it too, and fails.
const SKIPPED: &str = "epub-corpus: SKIPPED";

/// The fetched corpus, or `None`.
///
/// A directory that exists **and holds at least one `.epub`**, rather than a
/// directory that exists: an interrupted fetch leaves an empty directory, and a
/// sweep over nothing passes.
fn corpus() -> Option<Vec<(String, Vec<u8>)>> {
    let dir = PathBuf::from(std::env::var_os("TINKER_EPUB_CORPUS")?);
    let mut books: Vec<(String, Vec<u8>)> = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("epub"))
        })
        .map(|p| {
            let name = p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let bytes = std::fs::read(&p).unwrap_or_else(|e| panic!("reading {name}: {e}"));
            (name, bytes)
        })
        .collect();
    if books.is_empty() {
        return None;
    }
    books.sort_by(|a, b| a.0.cmp(&b.0));
    Some(books)
}

macro_rules! fetched {
    ($what:expr) => {
        match corpus() {
            Some(books) => {
                println!("{} {} ({} books)", RAN, $what, books.len());
                books
            }
            None => {
                println!(
                    "{} {} -- TINKER_EPUB_CORPUS is unset or holds no .epub; run \
                     crates/tinker-pdf/tests/epub/fetch-corpus.sh",
                    SKIPPED, $what
                );
                return;
            }
        }
    };
}

/// Every content document of every fetched book, as text.
fn content_documents(bytes: &[u8]) -> Vec<(String, String)> {
    let names: Vec<(usize, String)> = entries(bytes)
        .iter()
        .enumerate()
        .filter(|(_, e)| is_content_document(&e.name))
        .map(|(i, e)| (i, e.name.clone()))
        .collect();
    names
        .into_iter()
        .filter_map(|(index, name)| {
            let raw = read_at(bytes, index)?;
            Some((name, String::from_utf8_lossy(&raw).into_owned()))
        })
        .collect()
}

/// Every stylesheet of every fetched book, as text.
fn stylesheets(bytes: &[u8]) -> Vec<(String, String)> {
    let names: Vec<(usize, String)> = entries(bytes)
        .iter()
        .enumerate()
        .filter(|(_, e)| is_stylesheet(&e.name))
        .map(|(i, e)| (i, e.name.clone()))
        .collect();
    names
        .into_iter()
        .filter_map(|(index, name)| {
            let raw = read_at(bytes, index)?;
            Some((name, String::from_utf8_lossy(&raw).into_owned()))
        })
        .collect()
}

// ---- what today does with a book that nobody here commissioned --------------

/// The measurement this milestone exists to make, against files this repository
/// did not produce.
///
/// The committed corpus is our text through somebody else's tool; this is
/// somebody else's text through somebody else's tool, which is the only thing
/// that can catch a habit shared by our own inputs.
#[test]
fn every_fetched_book_is_mis_read_today() {
    let books = fetched!("the mis-read sweep");
    let mut opened = 0usize;
    let mut refused = 0usize;
    for (name, bytes) in &books {
        match todays_answer(bytes) {
            TodaysAnswer::Opens {
                pages,
                sizes,
                warnings,
                parsed_parts,
                defects,
                ..
            } => {
                opened += 1;
                let shown: Vec<String> = sizes
                    .iter()
                    .take(6)
                    .map(|(w, h)| format!("{w}x{h}"))
                    .collect();
                println!(
                    "  opens  {name:44} pages={pages:<3} sizes={} warnings={warnings} \
                     parsed_parts={parsed_parts} page_defects={defects}",
                    shown.join(",")
                );
                // The silence is the finding, not the page count. Every one of
                // these is what a caller would have to look at to discover that
                // a book was lost, and every one of them says nothing.
                assert_eq!(warnings, 0, "{name} warns today, which would be news");
                assert_eq!(
                    parsed_parts, 0,
                    "{name} parsed markup today, which it cannot"
                );
                assert_eq!(defects, 0, "{name} reports a page defect today");
            }
            TodaysAnswer::Refused(why) => {
                refused += 1;
                println!("  refused {name:44} {why:?}");
                assert_eq!(
                    why,
                    tinker_pdf::ArchiveRefusal::NoImages,
                    "{name} is refused by a name other than the comic one"
                );
            }
            TodaysAnswer::Other(what) => panic!("{name} failed to open in a new way: {what}"),
        }
    }
    println!("  {opened} open as comics, {refused} refused as NoImages, 0 read as books");
    assert_eq!(
        opened + refused,
        books.len(),
        "the sweep did not account for every book"
    );
}

// ---- the route --------------------------------------------------------------

/// The plan's sharpest structural finding, re-measured on files it did not
/// choose: **`ArchiveRefusal::UnreadablePackage` is unreachable for an EPUB.**
///
/// Gap 30 built that refusal for a package carrying OPC's own two items that
/// will not resolve. An EPUB is OCF and carries neither, so ECMA-388 E.3 fails
/// at step 2's *first* check and the comic fallthrough is exactly what E.3's
/// own text asks for. **Gap 30 is not wrong; EPUB is a different question it
/// did not ask** — and this is the assertion that keeps that sentence honest as
/// the corpus grows.
#[test]
fn no_fetched_book_carries_either_of_opcs_two_items() {
    let books = fetched!("the OPC-items check");
    for (name, bytes) in &books {
        for entry in entries(bytes) {
            assert_ne!(
                entry.name, "[Content_Types].xml",
                "{name} carries a content-types item, so E.3 would reach step 3"
            );
            assert_ne!(
                entry.name, "_rels/.rels",
                "{name} carries a package relationships part"
            );
        }
    }
    println!(
        "  0 of {} carry [Content_Types].xml or _rels/.rels",
        books.len()
    );
}

/// And the other half: **every** book carries `META-INF/container.xml`, which is
/// what milestone 3 discriminates on.
#[test]
fn every_fetched_book_carries_the_ocf_container() {
    let books = fetched!("the container.xml check");
    for (name, bytes) in &books {
        assert!(
            entries(bytes)
                .iter()
                .any(|e| e.name == "META-INF/container.xml"),
            "{name} has no META-INF/container.xml, so milestone 3's discriminator would miss it"
        );
    }
    println!(
        "  {} of {} carry META-INF/container.xml",
        books.len(),
        books.len()
    );
}

/// §4.3.2's four requirements, counted separately.
#[test]
fn the_mimetype_rule_holds_across_the_fetched_corpus() {
    let books = fetched!("the mimetype census");
    let mut offset_and_index_agree = 0usize;
    for (name, bytes) in &books {
        let verdict = mimetype_verdict(bytes);
        assert!(verdict.present, "{name} has no mimetype entry");
        assert!(verdict.first_by_offset, "{name}'s mimetype is not first");
        assert!(verdict.stored, "{name}'s mimetype is compressed");
        assert!(verdict.exact_bytes, "{name}'s mimetype is not the string");
        if verdict.first_by_offset == verdict.first_by_index {
            offset_and_index_agree += 1;
        }
    }
    // The trap milestone 3's exit criterion names. Not one real book can tell
    // the right check from the wrong one, which is precisely why milestone 3
    // owes a fixture whose two orders disagree.
    println!(
        "  {offset_and_index_agree} of {} cannot distinguish header_offset==0 from index==0",
        books.len()
    );
    assert_eq!(
        offset_and_index_agree,
        books.len(),
        "a real book now distinguishes the two checks; milestone 3 has a witness"
    );
}

/// What producers actually put in `META-INF`, against §4.2.6.3's closed set.
#[test]
fn the_meta_inf_census() {
    let books = fetched!("the META-INF census");
    let mut unreserved: Vec<String> = Vec::new();
    let mut reserved_seen: Vec<String> = Vec::new();
    for (_, bytes) in &books {
        for entry in entries(bytes) {
            if !entry.name.starts_with("META-INF/") || entry.is_directory() {
                continue;
            }
            let bucket = if RESERVED_META_INF.contains(&entry.name.as_str()) {
                &mut reserved_seen
            } else {
                &mut unreserved
            };
            if !bucket.contains(&entry.name) {
                bucket.push(entry.name.clone());
            }
        }
    }
    reserved_seen.sort();
    unreserved.sort();
    println!("  reserved names seen: {reserved_seen:?}");
    println!("  names in META-INF that are NOT reserved: {unreserved:?}");
    assert!(
        !reserved_seen.is_empty(),
        "the census found no reserved name at all, so it looked at nothing"
    );
}

// ---- the three censuses milestone 2 is waiting on ---------------------------

/// The doctype census: how many content documents carry one, and in which
/// shape.
///
/// This is the measurement decision 3 rests on. `tinker-pdf-xml` refuses
/// `<!DOCTYPE` before one byte after it is read, so the answer decides whether
/// milestone 2 is a nicety or a blocker.
#[test]
fn the_doctype_census() {
    let books = fetched!("the doctype census");
    let mut shapes: Vec<(&'static str, usize)> = Vec::new();
    let mut identifiers: Vec<(String, usize)> = Vec::new();
    let mut documents = 0usize;
    for (name, bytes) in &books {
        let mut per_book: Vec<(&'static str, usize)> = Vec::new();
        for (_, text) in content_documents(bytes) {
            documents += 1;
            let doctype = classify_doctype(&text);
            bump(&mut shapes, doctype_shape(&doctype));
            bump(&mut per_book, doctype_shape(&doctype));
            if let Doctype::Public { id, .. } = &doctype {
                match identifiers.iter_mut().find(|(k, _)| k == id) {
                    Some((_, n)) => *n += 1,
                    None => identifiers.push((id.clone(), 1)),
                }
            }
        }
        per_book.sort();
        println!("  {name:44} {per_book:?}");
    }
    shapes.sort();
    identifiers.sort();
    println!("  {documents} content documents: {shapes:?}");
    println!("  public identifiers: {identifiers:?}");
    assert!(documents > 0, "the census read no content document at all");
    // Every one of gap 30's four committed bombs lives in an internal subset,
    // and milestone 2 refuses that construct by its own name. If a real book
    // ever carries one, that decision needs re-arguing rather than inheriting.
    let internal = shapes
        .iter()
        .find(|(s, _)| *s == "internal-subset")
        .map_or(0, |(_, n)| *n);
    assert_eq!(
        internal, 0,
        "a fetched book carries an internal DTD subset; milestone 2's refusal now costs a real book"
    );
}

/// The named-character-reference census, which settles the plan's one genuinely
/// open question.
///
/// XHTML's `&nbsp;` and friends are declared by the DTD the doctype mode
/// discards, and are not among XML's five. The plan's working assumption is a
/// vendored table of the ~250 XHTML 1.0 names with a per-use warning; the
/// alternative is refusing, and the third option is HTML's ~2 200. This is what
/// decides between them.
#[test]
fn the_named_character_reference_census() {
    let books = fetched!("the named-character-reference census");
    let mut totals: Vec<(String, usize)> = Vec::new();
    let mut documents = 0usize;
    let mut documents_using = 0usize;
    let mut books_using: Vec<String> = Vec::new();
    let mut numeric = 0usize;
    let mut non_ascii = 0usize;
    for (name, bytes) in &books {
        for (_, text) in content_documents(bytes) {
            documents += 1;
            let found = named_references(&text);
            if !found.is_empty() {
                documents_using += 1;
                if !books_using.contains(name) {
                    books_using.push(name.clone());
                }
            }
            for (reference, count) in found {
                match totals.iter_mut().find(|(k, _)| *k == reference) {
                    Some((_, n)) => *n += count,
                    None => totals.push((reference, count)),
                }
            }
            numeric += numeric_references(&text);
            non_ascii += text.chars().filter(|c| !c.is_ascii()).count();
        }
    }
    totals.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    println!(
        "  {documents_using} of {documents} content documents in {} of {} books use one",
        books_using.len(),
        books.len()
    );
    println!("  distinct names: {}", totals.len());
    println!("  {totals:?}");
    println!("  books that use one: {books_using:?}");
    // The corroboration, without which a zero is not evidence: how often the
    // same documents write the numeric form, and how often they write the
    // character itself. "Nobody needs the entity table" and "nobody in this
    // corpus writes a character above ASCII" recommend opposite things to
    // milestone 2.
    println!("  numeric references: {numeric}; literal non-ASCII characters: {non_ascii}");
    assert!(documents > 0, "the census read no content document at all");
    assert!(
        non_ascii > 0,
        "the fetched corpus is pure ASCII, so its zero says nothing about the table"
    );
}

/// The CSS property census, which replaces the plan's forty-one-name list with
/// evidence.
#[test]
fn the_css_property_census() {
    let books = fetched!("the CSS property census");
    let mut union: Vec<(String, usize)> = Vec::new();
    let mut sheets = 0usize;
    for (name, bytes) in &books {
        let mut per_book: Vec<String> = Vec::new();
        for (_, text) in stylesheets(bytes) {
            sheets += 1;
            for property in css_properties(&text) {
                if !per_book.contains(&property) {
                    per_book.push(property.clone());
                }
                match union.iter_mut().find(|(k, _)| *k == property) {
                    Some((_, n)) => *n += 1,
                    None => union.push((property, 1)),
                }
            }
        }
        println!("  {name:44} {} properties", per_book.len());
    }
    union.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    println!(
        "  {sheets} stylesheets, {} distinct properties",
        union.len()
    );
    println!("  {union:?}");
    assert!(sheets > 0, "the census read no stylesheet at all");
}

fn bump(counts: &mut Vec<(&'static str, usize)>, key: &'static str) {
    match counts.iter_mut().find(|(k, _)| *k == key) {
        Some((_, n)) => *n += 1,
        None => counts.push((key, 1)),
    }
}
