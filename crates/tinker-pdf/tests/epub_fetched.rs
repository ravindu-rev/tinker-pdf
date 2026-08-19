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

use epub_support::conservation::{conservation, spine_text};
use epub_support::{
    classify_doctype, css_properties, doctype_shape, entries, is_content_document, is_stylesheet,
    mimetype_verdict, named_references, numeric_references, read_at, todays_answer, Doctype,
    TodaysAnswer, RESERVED_META_INF,
};
use tinker_pdf::epub::SpineDefect;
use tinker_pdf::{ArchiveWarning, Document};
use tinker_pdf_xml::{
    Doctype as XmlDoctype, Error as XmlError, Limits as XmlLimits, Reader as XmlReader,
    Source as XmlSource, Warning as XmlWarning,
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

// ---- what a book nobody here commissioned is read as -----------------------

/// **Every fetched book paginates to its own spine, at the box `OpenOptions`
/// states.**
///
/// *Rewritten twice.* Until milestone 3 this was called
/// `every_fetched_book_is_mis_read_today` and asserted the defect: eighteen of
/// twenty opened as comics — `sample-internallinks.epub` as ten pages of store
/// badges, `sample-svg-in-spine.epub` as six, three of them table-of-contents
/// ornaments **one point wide** — and two were refused as `NoImages`.
/// Milestone 3 turned all twenty into `UnpaginatedBook`. Milestone 4 turns them
/// into books, and the numbers below are the spines those twenty package
/// documents state.
///
/// The committed corpus is our text through somebody else's tool; this is
/// somebody else's text through somebody else's tool, which is the only thing
/// that can catch a habit shared by our own inputs. Three of these twenty are
/// shapes the committed six do not have: a spine of ninety-four, a spine of
/// SVG content documents, and two books whose fonts are obfuscated.
///
/// **The page count and the page box are two claims** and both are asserted,
/// because a build that paginated correctly into a wrong box, and one that put
/// the right box on the wrong number of pages, would each satisfy the other's
/// test.
#[test]
fn every_fetched_book_paginates_to_its_own_spine() {
    let books = fetched!("the pagination sweep");
    let mut total = 0usize;
    for (name, bytes) in &books {
        let want = *FETCHED_SPINES
            .iter()
            .find(|(book, _)| book == name)
            .map(|(_, spine)| spine)
            .unwrap_or_else(|| panic!("{name} has no recorded spine length"));
        match todays_answer(bytes) {
            TodaysAnswer::Opens { pages, sizes, .. } => {
                println!("  {name:44} {pages} pages");
                assert_eq!(pages, want, "{name} did not paginate to its spine");
                assert!(
                    sizes.iter().all(|size| *size == (432.0, 648.0)),
                    "{name} has a page that is not the default box: {:?}",
                    sizes.iter().find(|size| **size != (432.0, 648.0))
                );
                total += pages as usize;
            }
            TodaysAnswer::Refused(why) => panic!("{name} is refused as {why:?}"),
            TodaysAnswer::Other(what) => panic!("{name} failed to open in a new way: {what}"),
        }
    }
    println!("  {total} pages over {} books", books.len());
    assert_eq!(books.len(), FETCHED_SPINES.len());
}

/// Every fetched book and the number of `<itemref>`s its spine holds.
///
/// Written down rather than recomputed, for `epub_ocf.rs`'s reason: a test that
/// derives the number it checks agrees with itself whatever the reader does.
const FETCHED_SPINES: &[(&str, u32)] = &[
    ("pg11-alice-images.epub", 15),
    ("pg11-epub2-images.epub", 14),
    ("pg1342-noimages.epub", 16),
    ("pg16328-beowulf.epub", 7),
    ("pg2701-images.epub", 12),
    ("pg84-images.epub", 32),
    ("sample-childrens-literature.epub", 3),
    ("sample-childrens-media-query.epub", 1),
    ("sample-epub30-spec.epub", 11),
    ("sample-georgia-cfi.epub", 2),
    ("sample-hefty-water.epub", 1),
    ("sample-internallinks.epub", 7),
    ("sample-linear-algebra.epub", 94),
    ("sample-quiz-bindings.epub", 1),
    ("sample-regime-anticancer-arabic.epub", 3),
    ("sample-svg-in-spine.epub", 6),
    ("sample-trees.epub", 3),
    ("sample-wasteland-otf-obf.epub", 1),
    ("sample-wasteland-woff-obf.epub", 1),
    ("sample-wasteland.epub", 1),
];

/// Not one page of any fetched book draws anything, and every one of them says
/// so.
///
/// The other half of the pair above. A build that paginated a book correctly
/// and drew a white page would pass every assertion in this file except this
/// one — and a white page reported as a success is the failure gap 17 spent
/// itself on. Every page carries a [`SpineDefect`], and the distribution is
/// asserted rather than the mere presence: `sample-svg-in-spine.epub`'s six
/// pages are `SvgContentDocument` and nobody else's are, so a build that
/// collapsed the two names would fail here.
#[test]
fn every_fetched_page_is_a_placeholder_that_says_why() {
    let books = fetched!("the placeholder sweep");
    let mut svg = 0usize;
    let mut not_laid_out = 0usize;
    for (name, bytes) in &books {
        let doc = Document::open(bytes.clone()).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        let report = doc.archive().expect("a book carries a report");
        let mut named = vec![false; doc.page_count() as usize];
        for warning in report.warnings() {
            let ArchiveWarning::SpinePage { page, defect } = warning else {
                continue;
            };
            named[*page as usize] = true;
            match defect {
                SpineDefect::SvgContentDocument => svg += 1,
                SpineDefect::NotLaidOut => not_laid_out += 1,
                other => panic!("{name} page {page}: {other:?}"),
            }
        }
        assert!(
            named.iter().all(|said| *said),
            "{name} has a page nothing said anything about"
        );
    }
    println!("  {not_laid_out} unlaid pages and {svg} SVG spine items");
    assert_eq!(svg, 6, "the six SVG spine items are one book's");
    assert_eq!(not_laid_out + svg, 231, "every page of every fetched book");
}

/// Text conservation over books nobody here commissioned.
///
/// The committed corpus's figures are written down in
/// `tests/epub/CONSERVATION.tsv`; these books cannot be committed, so what is
/// asserted is the half that needs no record and is true at **every**
/// milestone: **no page carries a character the book does not have there.**
///
/// The other half is the number this sweep prints, which is milestone 4's
/// honest score against somebody else's text: zero of 1.2 million characters.
/// Every milestone from 8 onward raises it, and the harness that measures it
/// exists now rather than then.
///
/// A source stream that came back **empty** would satisfy "nothing extra" while
/// measuring nothing at all, so the total is asserted to be large — which is
/// what says the harness read twenty real books rather than twenty containers
/// it could not open.
#[test]
fn no_fetched_page_carries_a_character_its_book_does_not_have() {
    let books = fetched!("text conservation");
    let mut source = 0usize;
    let mut conserved = 0usize;
    for (name, bytes) in &books {
        let doc = Document::open(bytes.clone()).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        let verdict = conservation(bytes, &doc);
        assert_eq!(
            verdict.extra, 0,
            "{name} carries {} characters its book does not have: {:?}",
            verdict.extra, verdict.divergences
        );
        assert_eq!(
            spine_text(bytes).items.len(),
            doc.page_count() as usize,
            "{name}: the harness and the engine disagree about the spine's length"
        );
        source += verdict.source;
        conserved += verdict.conserved;
    }
    println!(
        "  {conserved}/{source} characters conserved over {} books",
        books.len()
    );
    assert!(
        source > 1_000_000,
        "the harness read {source} characters out of twenty books, which is not twenty books"
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

/// **Milestone 2's mode over the corpus that cannot be committed**, which is
/// the only place the single-quoted external identifier exists.
///
/// The committed corpus supplies the double-quoted form and this one supplies
/// the single-quoted one — thirty content documents of it, on every content
/// document of both Project Gutenberg EPUB 2 books. Neither corpus shows both,
/// which is why milestone 2's own fixtures name both by hand and why this test
/// and its committed twin are both here.
///
/// Corroboration, again, and not the assertion: the rules are pinned by
/// fixtures inside `tinker-pdf-xml`. What this adds is the count.
#[test]
fn the_relaxed_mode_reads_the_fetched_books_the_shipped_parser_refuses() {
    let books = fetched!("the doctype mode over real books");
    let mut refused_today = 0usize;
    let mut unchanged = 0usize;
    let mut warned = 0usize;
    let mut still_refused: Vec<String> = Vec::new();
    let mut identifiers: Vec<(String, usize)> = Vec::new();
    let mut quotes: Vec<(&'static str, usize)> = Vec::new();

    for (name, bytes) in &books {
        for (part, text) in content_documents(bytes) {
            let shape = doctype_shape(&classify_doctype(&text));
            let Ok(source) = XmlSource::new(text.as_bytes()) else {
                // Not this test's subject: a part that does not decode has
                // nothing to say about a document type declaration.
                continue;
            };
            let mut strict = source.reader(&XmlLimits::DEFAULT);
            let strict_refusal = first_refusal(&mut strict);
            let mut relaxed = source.reader_with(&XmlLimits::DEFAULT, XmlDoctype::SkipExternalId);
            let relaxed_refusal = first_refusal(&mut relaxed);

            if strict_refusal != Some(XmlError::DoctypeUnsupported) {
                // Read today, or refused for a reason that is not a
                // declaration. Either way the mode must change nothing.
                unchanged += 1;
                assert_eq!(
                    strict_refusal, relaxed_refusal,
                    "{name} {part}: the mode changed a document with no declaration in it",
                );
                continue;
            }

            refused_today += 1;
            bump(&mut quotes, shape);
            match relaxed_refusal {
                None => {}
                Some(error) => still_refused.push(format!("{name} {part}: {error}")),
            }
            if relaxed
                .warnings()
                .contains(&XmlWarning::ExternalIdentifierNotAllowed)
            {
                warned += 1;
            }
            if let Some(public) = relaxed
                .external_identifier()
                .and_then(|identifier| identifier.public())
            {
                match identifiers.iter_mut().find(|(k, _)| k == public) {
                    Some((_, n)) => *n += 1,
                    None => identifiers.push((public.to_owned(), 1)),
                }
            }
        }
    }

    quotes.sort();
    identifiers.sort();
    println!("  refused today: {refused_today} {quotes:?}");
    println!("  read today and unchanged: {unchanged}");
    println!("  identifiers named: {identifiers:?}");
    println!("  warned about: {warned}");
    assert!(
        refused_today > 0,
        "no fetched content document is refused today, so the relaxation is untested"
    );
    assert!(
        unchanged > 0,
        "every fetched content document is refused today, so `unchanged` is untested"
    );
    assert!(
        still_refused.is_empty(),
        "the relaxed mode still refuses {} real content documents: {:?}",
        still_refused.len(),
        &still_refused[..still_refused.len().min(8)],
    );
    // The shape this corpus alone supplies, and milestone 2's criterion names.
    assert!(
        quotes
            .iter()
            .any(|(shape, n)| *shape == "public-single-quoted" && *n > 0),
        "the single-quoted external identifier is no longer in the fetched corpus",
    );
    assert_eq!(
        warned,
        identifiers.iter().map(|(_, n)| n).sum::<usize>(),
        "a public identifier was read without a warning beside it",
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
