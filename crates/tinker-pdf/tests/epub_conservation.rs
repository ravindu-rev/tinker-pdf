//! The text-conservation harness, and the harness's own tests (gap 31,
//! milestone 4).
//!
//! `epub_support/conservation.rs` is the device; this is where it is proved and
//! where it is run. The plan schedules it **here**, against thirteen grey
//! pages, rather than with the first real layout:
//!
//! > *"Built at milestone 8 it would be a test written to pass; built at
//! > milestone 4 against thirteen grey pages it is a test written before there
//! > is anything to make it pass, and every milestone after inherits it as a
//! > constraint rather than acquiring it as a formality."*
//!
//! # An assertion that cannot fail is not an assertion
//!
//! Milestone 4's pages carry no text at all, so a corpus sweep asserting
//! *"conservation holds"* would be `0 == 0` on every book and would pass with
//! the whole harness deleted. Three legs answer that, and none of them is that
//! sweep:
//!
//! 1. **The comparator, against synthetic streams.** Each of the five failure
//!    classes gap 31 names is built by hand and asserted to be *detected*, by
//!    its own name and with its own size. These fail today if the comparator is
//!    wrong.
//! 2. **The paginated side, against a document that really has text.** A
//!    three-page PDF from this repository's own writer, whose words are known,
//!    run through the same [`conservation::paginated_text`] a book will use.
//!    That proves page order, run order and extraction end to end **now**,
//!    which is the half milestone 8 would otherwise be the first to exercise.
//! 3. **The source side, against markup with something to get wrong** — a
//!    `<head>`, a `<style>`, a `<script>`, entities, a CDATA section, and two
//!    elements whose text must not run together.
//!
//! The corpus sweep is the fourth leg and it asserts the two halves that are
//! true at *every* milestone — **nothing extra, and the recorded figure** —
//! with the figure written down in `tests/epub/CONSERVATION.tsv` so a milestone
//! that raises it has to say so in the same commit.

mod epub_support;

use epub_support::conservation::{
    self, compare, conservation, spine_text, visible_text, Divergence, SpineSource, SpineText,
};
use tinker_pdf::{Document, DocumentBuilder, OpenOptions};

// ---- leg 1: the comparator ---------------------------------------------------

fn source(text: &str) -> SpineText {
    SpineText {
        items: vec![SpineSource {
            path: "EPUB/ch1.xhtml".to_owned(),
            text: text.to_owned(),
        }],
    }
}

/// A book whose pages carry exactly its own characters conserves all of them.
///
/// Whitespace is where the pages are allowed to differ, and this is the
/// assertion that says so: the source is indented the way milestone 1's real
/// books are and the pages are set as lines, and the verdict is perfect.
#[test]
fn a_book_whose_pages_carry_its_own_characters_conserves_every_one() {
    let book = source("  Once upon a time,\n     there were three bears.\n");
    let verdict = compare(
        &book,
        &[
            "Once upon a time, there\n".to_owned(),
            "were three bears.\n".to_owned(),
        ],
    );
    assert!(verdict.holds(), "{verdict:?}");
    assert_eq!(verdict.conserved, verdict.source);
    assert_eq!(verdict.figure(), (verdict.source, verdict.source));
    assert!(verdict.divergences.is_empty());
}

/// **Missing text**: a float that pushed content off the page, or a
/// `display: none` honoured where it should not have been.
#[test]
fn text_a_page_lost_is_reported_as_missing_with_its_size() {
    let book = source("The first sentence. THE MIDDLE ONE IS GONE. The last sentence.");
    let verdict = compare(
        &book,
        &["The first sentence. The last sentence.".to_owned()],
    );
    assert!(!verdict.holds());
    assert_eq!(verdict.extra, 0, "nothing was gained");
    assert_eq!(verdict.missing, "THEMIDDLEONEISGONE.".chars().count());
    // The reported run names the lost material. Its *first* character is where
    // the two streams stopped agreeing rather than where the sentence began —
    // `THE` and `The` share a `T`, so a greedy walk consumes it — and the
    // harness reports what it measured rather than what a reader would have
    // guessed.
    assert!(
        matches!(
            verdict.divergences.first(),
            Some(Divergence::Missing { text, .. }) if text.contains("MIDDLEONEISGONE")
        ),
        "{:?}",
        verdict.divergences
    );
}

/// **Extra text**: a `display: none` *not* honoured, or a page carrying
/// something the book does not have there.
#[test]
fn text_a_page_gained_is_reported_as_extra_with_its_size() {
    let book = source("The first sentence. The last sentence.");
    let verdict = compare(
        &book,
        &["The first sentence. SOMETHING NOBODY WROTE. The last sentence.".to_owned()],
    );
    assert!(!verdict.holds());
    assert_eq!(verdict.missing, 0, "nothing was lost");
    assert_eq!(verdict.extra, "SOMETHINGNOBODYWROTE.".chars().count());
    assert!(matches!(
        verdict.divergences.first(),
        Some(Divergence::Extra { .. })
    ));
}

/// **Duplicated text**: a paragraph that appears at the bottom of one page and
/// again at the top of the next.
///
/// The failure with no pixel: it renders as a printing error and every page
/// count agrees. It is *extra* text, and the repeat resynchronises afterwards,
/// which is why the size is the paragraph's rather than the rest of the book's.
#[test]
fn a_paragraph_repeated_across_a_page_break_is_extra_text() {
    let paragraph = "The paragraph that straddles the boundary of two pages.";
    let book = source(&format!("Before it. {paragraph} After it."));
    let verdict = compare(
        &book,
        &[
            format!("Before it. {paragraph}"),
            format!("{paragraph} After it."),
        ],
    );
    assert!(!verdict.holds());
    assert_eq!(verdict.missing, 0);
    assert_eq!(
        verdict.extra,
        paragraph.chars().filter(|c| !c.is_whitespace()).count()
    );
}

/// **A whole chapter missing**: a spine item whose XHTML failed to parse and
/// was skipped.
///
/// Gap 29's page-renumbering hazard at book scale, and the one the plan calls
/// out as the reason this harness is scheduled before any CSS. The book reads
/// continuously across the hole and every page renders.
#[test]
fn a_spine_item_dropped_is_a_whole_chapter_missing() {
    let book = SpineText {
        items: vec![
            SpineSource {
                path: "a.xhtml".to_owned(),
                text: "Chapter one runs to its own end here.".to_owned(),
            },
            SpineSource {
                path: "b.xhtml".to_owned(),
                text: "Chapter two is the one that vanished entirely.".to_owned(),
            },
            SpineSource {
                path: "c.xhtml".to_owned(),
                text: "Chapter three carries on as though nothing happened.".to_owned(),
            },
        ],
    };
    let verdict = compare(
        &book,
        &[
            "Chapter one runs to its own end here.".to_owned(),
            "Chapter three carries on as though nothing happened.".to_owned(),
        ],
    );
    assert!(!verdict.holds());
    assert_eq!(verdict.extra, 0);
    assert_eq!(
        verdict.missing,
        "Chapter two is the one that vanished entirely."
            .chars()
            .filter(|c| !c.is_whitespace())
            .count()
    );
}

/// **Reordered text**: two chapters that swapped places.
///
/// Neither missing nor extra on its own, and every character present — which is
/// why the invariant is *in order* rather than *as a multiset*. A harness that
/// counted characters would call this book perfect.
#[test]
fn two_chapters_that_swapped_places_are_not_conserved() {
    let one = "Chapter one runs to its own end here.";
    let two = "Chapter two follows it and is entirely different.";
    let book = SpineText {
        items: vec![
            SpineSource {
                path: "a.xhtml".to_owned(),
                text: one.to_owned(),
            },
            SpineSource {
                path: "b.xhtml".to_owned(),
                text: two.to_owned(),
            },
        ],
    };
    let verdict = compare(&book, &[two.to_owned(), one.to_owned()]);
    assert!(!verdict.holds(), "a swapped book is not a conserved book");
    assert!(verdict.missing > 0 && verdict.extra > 0, "{verdict:?}");
}

/// A book whose pages are empty loses everything, and the harness says so.
///
/// **This is milestone 4's own state**, written down as an assertion rather
/// than left as an absence: the placeholder pages carry no characters, so every
/// character of every book is missing and none is extra.
#[test]
fn a_book_of_placeholders_conserves_nothing_and_gains_nothing() {
    let book = source("Every character of this is lost.");
    let verdict = compare(&book, &[String::new(), String::new()]);
    assert_eq!(verdict.conserved, 0);
    assert_eq!(verdict.extra, 0);
    assert_eq!(verdict.missing, verdict.source);
    assert_eq!(verdict.figure(), (0, verdict.source));
}

// ---- leg 2: the paginated side, against a document that has text -------------

/// The half of the harness milestone 4's own books cannot exercise.
///
/// A book at this milestone has no text, so [`conservation::paginated_text`] is
/// never asked to do anything by the sweep below. A PDF from this repository's
/// own writer is, and it proves the whole of the actual side: that pages are
/// walked **in page order**, that a page's runs come back in the order they
/// were drawn, and that extraction reaches the text at all. A build that walked
/// the pages backwards would pass every assertion in this file without it.
#[test]
fn the_paginated_side_reads_a_real_documents_pages_in_order() {
    let words = ["FIRSTPAGE", "SECONDPAGE", "THIRDPAGE"];
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    for word in words {
        builder.add_page(200.0, 100.0, |page| {
            page.text(b"F1", 12.0, 20.0, 40.0, word);
        });
    }
    let doc = Document::open(builder.finish()).expect("a PDF");
    assert_eq!(doc.page_count(), 3);

    let pages = conservation::paginated_text(&doc);
    assert_eq!(pages.len(), 3);
    for (page, word) in pages.iter().zip(words) {
        assert!(page.contains(word), "{page:?} does not carry {word}");
    }

    // And through the comparator, which is the assertion that the two sides
    // meet: a source stream of the same three words is conserved exactly.
    let book = SpineText {
        items: words
            .iter()
            .map(|word| SpineSource {
                path: format!("{word}.xhtml"),
                text: (*word).to_owned(),
            })
            .collect(),
    };
    let verdict = compare(&book, &pages);
    assert!(verdict.holds(), "{verdict:?}");
    assert_eq!(verdict.conserved, "FIRSTPAGESECONDPAGETHIRDPAGE".len());

    // The order half, made to fail: the same pages, read backwards, are not the
    // same book.
    let mut backwards = pages.clone();
    backwards.reverse();
    assert!(!compare(&book, &backwards).holds());
}

// ---- leg 3: the source side --------------------------------------------------

/// Markup removal, over the shapes a content document actually has.
#[test]
fn the_source_side_reads_the_text_and_not_the_markup() {
    let markup = concat!(
        r#"<?xml version="1.0" encoding="utf-8"?>"#,
        r#"<!DOCTYPE html PUBLIC '-//W3C//DTD XHTML 1.1//EN' 'http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd'>"#,
        r#"<html xmlns="http://www.w3.org/1999/xhtml">"#,
        r#"<head><title>Chrome, not text</title>"#,
        r#"<style type="text/css">p { color: red }</style></head>"#,
        r#"<body><!-- a comment --><p>First.</p><p>Second &amp; last &#8212; really.</p>"#,
        r#"<script>var gone = 1;</script>"#,
        // A second stylesheet, in the **body** rather than the head. The one
        // above is inside `<head>`, so the head rule already excludes it and
        // the style rule is never the reason — which the injection matrix
        // proved by deleting `style` from the skip list and watching every
        // test pass. HTML allows `<style>` in flow content and real books use
        // it, so this is not a hostile fixture; it is the ordinary case that
        // separates two rules a comment claimed were separate.
        r#"<style>blockquote { margin: 0 }</style>"#,
        r#"<p><![CDATA[Inside a section.]]></p></body></html>"#
    );
    let text = visible_text(markup);
    let squeezed: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    assert_eq!(
        squeezed,
        "First. Second & last \u{2014} really. Inside a section."
    );

    // Each exclusion on its own, because a build that dropped one of the three
    // would still pass a test that only looked at the joined string.
    assert!(!text.contains("Chrome"), "the head's title is not the book");
    assert!(
        !text.contains("color"),
        "a stylesheet in the head is not the book"
    );
    assert!(
        !text.contains("blockquote") && !text.contains("margin"),
        "and neither is one in the body, which is the only place the `style` \
         rule is what excludes it"
    );
    assert!(!text.contains("gone"), "a script is not the book");
    assert!(!text.contains("comment"), "a comment is not the book");
    assert!(
        !text.contains("xhtml11.dtd"),
        "a doctype's literals are not the book"
    );
}

/// **A tag boundary is a place two words may not run together.**
///
/// `<p>a</p><p>b</p>` is two words; a stripper that deleted the tags and
/// nothing else produces `ab`, which conserves every character and is a
/// different book. The conserved stream drops whitespace, so this is the one
/// place the source side has to *add* some.
#[test]
fn two_elements_text_does_not_run_together() {
    let text = visible_text("<p>one</p><p>two</p>");
    assert!(
        text.split_whitespace().collect::<Vec<_>>() == ["one", "two"],
        "{text:?}"
    );
}

/// The spine, the manifest and the container, read out of a real book by the
/// harness's own scanners rather than by the engine.
///
/// **The order is one of the claims being checked**, which is why this does not
/// call `epub::package::parse`. The two agree here — the pages the engine
/// reports and the paths the harness resolved are the same list, in the same
/// order — and that agreement is evidence precisely because the two arrived at
/// it separately.
#[test]
fn the_source_side_finds_the_same_spine_the_engine_did() {
    for name in [
        "calibre-book-cover.epub",
        "calibre-book-nocover.epub",
        "pandoc-book-cover.epub",
        "pandoc-book-epub2.epub",
        "pandoc-book-nocover.epub",
        "pandoc-plates.epub",
    ] {
        let bytes = corpus_book(name);
        let doc = Document::open(bytes.clone()).expect(name);
        let harness = spine_text(&bytes);
        // **Distinct consecutive** names, because since milestone 8 a chapter
        // takes as many pages as its text needs and `pages()` has one entry
        // per page. Collapsing runs of the same name is what turns a page list
        // back into a spine — and it is not a weakening: a chapter that
        // appeared twice in the spine at *different* positions still shows as
        // two entries, and a chapter dropped altogether still shows as none.
        let mut engine: Vec<&str> = Vec::new();
        for origin in doc.archive().expect("a report").pages() {
            if engine.last() != Some(&origin.name.as_str()) {
                engine.push(origin.name.as_str());
            }
        }
        let ours: Vec<&str> = harness.items.iter().map(|i| i.path.as_str()).collect();
        assert_eq!(ours, engine, "{name}");
        assert!(
            doc.page_count() as usize >= ours.len(),
            "{name}: a spine item contributes at least one page"
        );
        assert!(
            harness.items.iter().any(|i| !i.text.trim().is_empty()),
            "{name}: the harness read no text out of any chapter"
        );
    }
}

/// The source side drops a `hidden` subtree and keeps everything that only
/// looks like one.
///
/// **Both directions, because a scanner that dropped everything would pass
/// half of this.** `aria-hidden` is on three `<figcaption>`s in the committed
/// corpus and hides nothing; `data-hidden` and a `class="hidden"` are the two
/// spellings a hand-written book uses for something the UA sheet does not act
/// on either. A scan for the substring `hidden` would eat all four.
#[test]
fn the_source_side_drops_hidden_and_keeps_everything_else() {
    let markup = "<body>\
        <nav hidden=\"hidden\"><p>landmarks</p></nav>\
        <p aria-hidden=\"true\">aria</p>\
        <p data-hidden=\"true\">data</p>\
        <p class=\"hidden\">class</p>\
        <div hidden><span>bare</span></div>\
        <div hidden=\"\"><span>empty</span></div>\
        <p>kept</p></body>";
    let text = visible_text(markup);
    for gone in ["landmarks", "bare", "empty"] {
        assert!(!text.contains(gone), "{gone} should be hidden: {text:?}");
    }
    for kept in ["aria", "data", "class", "kept"] {
        assert!(text.contains(kept), "{kept} should be kept: {text:?}");
    }
}

/// A hidden element holding a **nested element of its own name** ends at the
/// right `</div>`.
///
/// The depth counter, on its own. No book in either corpus nests one, so a
/// fixture built from the corpus could not tell a counter from a flag — and a
/// flag would let every character after the inner `</div>` back into the
/// stream while the outer element is still open.
#[test]
fn a_hidden_subtree_ends_at_its_own_closing_tag() {
    let text = visible_text("<body><div hidden><div>inner</div>outer</div><p>after</p></body>");
    assert!(!text.contains("inner"), "{text:?}");
    assert!(!text.contains("outer"), "{text:?}");
    assert!(text.contains("after"), "{text:?}");
}

/// The source side decodes UTF-8 and does not transliterate it.
///
/// **This is the assertion milestone 4 owed and did not write**, and milestone
/// 8 is where its absence cost something: `visible_text` pushed one `char` per
/// **byte**, so an em dash became three characters and a kanji three more. No
/// test could see it while every page was empty — the corpus sweep compared a
/// mangled source against no pages at all and reported `0` conserved either
/// way — and the first book that laid out reported 2 454 characters of
/// spurious divergence.
#[test]
fn the_source_side_decodes_and_does_not_transliterate() {
    let text =
        visible_text("<p>an em\u{2014}dash, an ellipsis\u{2026} and \u{65e5}\u{672c}\u{8a9e}</p>");
    assert!(text.contains('\u{2014}'), "{text:?}");
    assert!(text.contains('\u{2026}'), "{text:?}");
    assert!(text.contains("\u{65e5}\u{672c}\u{8a9e}"), "{text:?}");
    assert!(
        !text.contains('\u{00e2}'),
        "UTF-8 read as Latin-1 leaves an a-circumflex behind: {text:?}"
    );
    // And the count is the count of characters, not of bytes: nine characters
    // of text plus the two spaces the tag boundaries add.
    assert_eq!(
        text.chars().filter(|c| !c.is_whitespace()).count(),
        "anem\u{2014}dash,anellipsis\u{2026}and\u{65e5}\u{672c}\u{8a9e}"
            .chars()
            .count()
    );
}

// ---- leg 4: the corpus, and the figure this milestone records ----------------

/// **Every committed book, against the figure written down beside it.**
///
/// Two halves, and only one of them is a number that moves:
///
/// - **nothing extra**, ever. No page of any book may carry a character the
///   book does not have there, and that is true at this milestone and at every
///   one after it.
/// - **the conserved figure**, recorded in `tests/epub/CONSERVATION.tsv`. At
///   milestone 4 it was `0` for every book, because every page was a
///   placeholder; at milestone 8 it is **every character of every book**. A
///   milestone that changes it has to update that file in the same commit,
///   which is what makes this a ratchet rather than a test written to pass —
///   and now that the figure is total, any regression at all moves it.
#[test]
fn every_committed_book_conserves_the_figure_the_record_states() {
    let recorded = record();
    let mut measured: Vec<String> = Vec::new();
    for name in COMMITTED {
        let bytes = corpus_book(name);
        let doc = Document::open(bytes.clone()).expect(name);
        let verdict = conservation(&bytes, &doc);
        let text = spine_text(&bytes);

        measured.push(format!(
            "{name}\t{}\t{}\t{}\t{}",
            text.items.len(),
            verdict.source,
            verdict.conserved,
            doc.page_count()
        ));

        // True at this milestone and at every one after it: no page carries a
        // character the book does not have there.
        assert_eq!(
            verdict.extra, 0,
            "{name} has {} characters on a page that the book does not have there: {:?}",
            verdict.extra, verdict.divergences
        );
        assert_eq!(
            verdict.missing,
            verdict.source - verdict.conserved,
            "{name}: every character is conserved or missing and nothing else"
        );
        println!(
            "  {name:26} {}/{} characters over {} pages",
            verdict.conserved,
            verdict.source,
            doc.page_count()
        );
    }

    // The record is the file rather than a list in this test, so a milestone
    // that lays text out has to re-measure rather than argue.
    assert_eq!(
        measured, recorded,
        "tests/epub/CONSERVATION.tsv is out of date"
    );
    assert_eq!(recorded.len(), 6, "the committed corpus is six books");
}

/// The committed corpus, which is the list of books rather than the record of
/// what they measured.
const COMMITTED: &[&str] = &[
    "calibre-book-cover.epub",
    "calibre-book-nocover.epub",
    "pandoc-book-cover.epub",
    "pandoc-book-epub2.epub",
    "pandoc-book-nocover.epub",
    "pandoc-plates.epub",
];

/// And the figure does not depend on the page box, which is the claim a
/// reflowable format has to make about its own text.
///
/// A book laid out at 300 × 500 is a different book by page count — or will be,
/// once milestone 7 fragments — and it is the **same** book by text. A build
/// that lost a character at one box and not at another would be a build whose
/// pagination decides its content.
#[test]
fn conservation_does_not_depend_on_the_page_box() {
    let bytes = corpus_book("pandoc-book-cover.epub");
    let wide =
        Document::open_with(bytes.clone(), &OpenOptions::at_page(300.0, 500.0)).expect("a book");
    let tall =
        Document::open_with(bytes.clone(), &OpenOptions::at_page(900.0, 1_200.0)).expect("a book");
    let at_wide = conservation(&bytes, &wide);
    let at_tall = conservation(&bytes, &tall);
    assert_eq!(at_wide.source, at_tall.source);
    assert_eq!(at_wide.figure(), at_tall.figure());
    assert_eq!(at_wide.extra, 0);
    assert_eq!(at_tall.extra, 0);
}

fn corpus_book(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("epub")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

fn record_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("epub")
        .join("CONSERVATION.tsv")
}

/// The recorded rows of `tests/epub/CONSERVATION.tsv`, header dropped: book,
/// spine items, conservable characters, conserved characters, pages.
fn record() -> Vec<String> {
    let text = std::fs::read_to_string(record_path()).expect("tests/epub/CONSERVATION.tsv");
    let mut lines = text.lines();
    assert_eq!(
        lines.next(),
        Some("book\tspine_items\tsource_characters\tconserved_characters\tpages"),
        "the record's header changed"
    );
    lines
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            assert_eq!(
                line.split('\t').count(),
                5,
                "a row of CONSERVATION.tsv has five cells"
            );
            line.to_owned()
        })
        .collect()
}
