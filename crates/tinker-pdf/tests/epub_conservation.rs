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
use epub_support::reading::{check, rep, word, Case, Expect, AR, HE, HEN, HEP};
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
        <div hidden = \"hidden\"><span>spaced</span></div>\
        <p>kept</p></body>";
    let text = visible_text(markup);
    for gone in ["landmarks", "bare", "empty", "spaced"] {
        assert!(!text.contains(gone), "{gone} should be hidden: {text:?}");
    }
    for kept in ["aria", "data", "class", "kept"] {
        assert!(text.contains(kept), "{kept} should be kept: {text:?}");
    }
}

/// **The word `hidden` inside an attribute value hides nothing** (gap 31,
/// milestone 13).
///
/// The fifth harness bug, as a fixture. `has_hidden_attribute` searched the
/// start tag as a string, so any value with ` hidden ` in it dropped the
/// element's whole subtree — and the tag that does it is real:
/// `sample-epub30-spec.epub` titles a section *"2.2.4.3 The hidden attribute"*,
/// and the harness reported the 1 757 characters underneath it as text the
/// engine had **invented**.
///
/// Both directions are asserted, because a parser that fixed this by requiring
/// `hidden` to be first, or last, or unquoted would pass a one-sided test.
#[test]
fn a_hidden_word_in_an_attribute_value_is_not_the_hidden_attribute() {
    let text = visible_text(
        "<body>\
         <section title=\"2.2.4.3 The hidden attribute\"><p>spec</p></section>\
         <p title=\"hidden\">exact</p>\
         <p alt=\"a hidden thing\">middle</p>\
         <p data-x=\"x hidden\">last</p>\
         <section title=\"The hidden attribute\" hidden><p>both</p></section>\
         </body>",
    );
    for kept in ["spec", "exact", "middle", "last"] {
        assert!(text.contains(kept), "{kept} should be kept: {text:?}");
    }
    // And a tag that carries **both** a value with the word in it and the real
    // attribute is still hidden: the fix must not overshoot into never seeing
    // one.
    assert!(!text.contains("both"), "{text:?}");
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
///
/// **Tier 4 broke "every character of every book", and the exception is worth
/// more than the rule was.** `kcc-fixed-layout.epub` conserves **0 of 6**: KCC
/// puts a `<div style="display:none;">.</div>` in every one of its six content
/// documents, and a full stop that is `display: none` is a source character
/// this build is *right* not to paint. So the invariant as milestone 8 stated
/// it — every character of every content document reaches a page — was never
/// quite the invariant; it was that plus an assumption that no producer writes
/// text it does not mean to show. The first real fixed-layout book in this
/// corpus writes six of them. The two assertions this test actually makes are
/// untouched: nothing extra is on a page, and every character is conserved or
/// missing and nothing else.
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
    assert_eq!(recorded.len(), 9, "the committed corpus is nine books");
}

/// The committed corpus, which is the list of books rather than the record of
/// what they measured.
const COMMITTED: &[&str] = &[
    "calibre-book-cover.epub",
    "calibre-book-nocover.epub",
    "calibre-embedded-font.epub",
    "kcc-fixed-layout.epub",
    "pandoc-book-cover.epub",
    "pandoc-book-epub2.epub",
    "pandoc-book-nocover.epub",
    "pandoc-embedded-font.epub",
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

/// **A letter in another case is the same letter.** `text-transform` and
/// synthesised small capitals draw the book's `The Swans` as `THE SWANS`,
/// and that is conservation, not a loss and a gain: the fetched
/// `sample-childrens-media-query.epub` read exactly that way once
/// text-transform landed. Only case is forgiven — `ß` drawn `SS` is two
/// characters for one and still diverges.
#[test]
fn a_letter_set_in_another_case_is_conserved_and_nothing_else_is() {
    let verdict = compare(
        &source("The Swans. Ho! pretty swans"),
        &["THE SWANS. Ho! pretty swans".to_owned()],
    );
    assert!(verdict.holds(), "{verdict:?}");
    assert_eq!(verdict.conserved, verdict.source);

    let verdict = compare(&source("Straße"), &["STRASSE".to_owned()]);
    assert!(
        !verdict.holds(),
        "a two-character mapping is not a case change: {verdict:?}"
    );

    let verdict = compare(&source("The Swans"), &["The Swins".to_owned()]);
    assert!(
        !verdict.holds(),
        "a different letter is not a case change: {verdict:?}"
    );
}

/// The same, end to end: a book whose heading is `text-transform: uppercase`
/// is drawn in capitals and conserves.
#[test]
fn a_book_with_an_uppercased_heading_conserves() {
    let bytes = epub_support::book::styled_book(
        "en",
        "h1 { text-transform: uppercase }",
        "<h1>The Swans</h1><p>Ho! pretty swans, do you know?</p>",
    );
    let doc = Document::open(bytes.clone()).expect("the book opens");
    let pages = conservation::paginated_text(&doc).concat();
    assert!(
        pages.contains("THE SWANS"),
        "the heading is drawn in capitals: {pages:?}"
    );
    let verdict = conservation(&bytes, &doc);
    assert!(verdict.holds(), "{verdict:?}");
}

/// The paragraph ROADMAP K-11's probe builds: `repeat` sentences, each a
/// filler, a bracketed `inside` and its full stop, and a tail — the shape
/// `sample-regime-anticancer-arabic.epub` holds at character 20 963.
fn bracketed_paragraph(inside: &str, repeat: usize) -> String {
    const FILLERS: [&str; 2] = [
        "إنّ الخلايا السرطانيّة تتكاثر بسرعة كبيرة، ويعتمد ذلك على عوامل عدّة",
        "تتألّف كلّ الموادّ الحيّة على كوكبنا الأرض من خلايا، باستثناء الفيروسات",
    ];
    const TAIL: &str = "وقد أصبحنا نعرف الآن أنّ بعض الموادّ تكبح الهستونات النازعة للأسيتيل، مخفّفة بذلك من نموّ الأورام";
    (0..repeat)
        .map(|i| format!("{} ({inside}). {TAIL}. ", FILLERS[i % 2]))
        .collect()
}

/// `text` with its Arabic marks taken out: the probe's mark-free twin.
fn unpointed(text: &str) -> String {
    text.chars()
        .filter(|c| !matches!(c, '\u{064B}'..='\u{065F}' | '\u{0670}'))
        .collect()
}

/// **A bracketed run and the full stop after it read as written in
/// right-to-left text** — ROADMAP K-11, closed 10 October 2026.
///
/// CI's `epub-corpus` job found the shape in
/// `sample-regime-anticancer-arabic.epub`, from character 20 963
/// (`Missing ").وقد…"`, `Extra ")وقد…"`). A synthetic probe of 240 books
/// failed 82, the same 82 with every mark stripped, and three causes, none of
/// them where a mark is drawn, stood behind them; each shape here failed at
/// 9297d2d, pointed and mark-free alike, in both builds:
///
/// - **The writer drew the punctuation backwards.** In a sans-serif book the
///   brackets, the full stop and the spaces are Helvetica's and the Arabic is
///   the overflow font's, so ` (` and `). ` are face segments of their own,
///   and a standard-14 segment with no right-to-left character in it was
///   drawn as typed even at a right-to-left level: the page itself showed
///   `عدة( النازعة` and read `.)وقد` — CI's `Missing ").وقد"`,
///   `Extra ")وقد"`. Such a segment is drawn in its level's order now on a
///   line that holds a right-to-left character (`paint::coded_order`,
///   `TextRun::bidi_line_rtl`).
/// - **The line came back in pieces.** Narrowed so that `(histone
///   deacetylase).` opens a line, the `(`, the Latin words and the Arabic
///   are three text objects, and the join of a line's pieces asked whether
///   either of **two** pieces held a right-to-left character; the `(` and
///   the Latin hold none, so the `(` was read as a line of its own. The
///   chain the pieces make is joined whole now where, read whole, the
///   bracket tie-break turns it (`text_order::rejoin_split_lines`). And justified
///   at `margin: 0 35%`, an Arabic line's word spaces were stretched past half
///   an em, which `TextDevice` cut it at, so the words read in drawn order,
///   backwards; a word space stretched by `Tw` carries the pen within
///   right-to-left text now (`tinker-pdf-content`'s `Stretch`).
/// - **The majority read the line the wrong way.** That narrowed line holds
///   eighteen Latin letters and seventeen Arabic ones, and ruling 14 read it
///   left to right, `.)histone deacetylase(`: a text a left-to-right
///   paragraph draws as the line stands. The pair is drawn facing outward,
///   which only a pair at a right-to-left level is; ruling 14's bracket
///   tie-break (amended 10 October 2026) reads it in the direction that pairs
///   it.
#[test]
fn a_bracketed_run_and_its_full_stop_in_right_to_left_text_read_as_written() {
    let read = |style: &str, open: &str, text: &str| {
        let bytes = epub_support::book::styled_book("ar", style, &format!("{open}{text}</p>"));
        let doc = Document::open(bytes.clone()).expect("the book opens");
        (
            conservation(&bytes, &doc),
            conservation::conservation_in_logical_order(&bytes, &doc),
        )
    };
    let rtl = "<p dir=\"rtl\">";
    for (style, open, inside, repeat) in [
        // The narrowed line that opens with the bracketed Latin words.
        ("p { margin: 0 25% }", rtl, "histone deacetylase", 6),
        // CI's shape: the punctuation in a face of its own.
        (
            "body { font-family: sans-serif } p { text-align: justify; margin: 0 15% }",
            rtl,
            "النازعة للأسيتيل",
            1,
        ),
        (
            "body { font-family: sans-serif } p { text-align: justify; margin: 0 15% }",
            rtl,
            "histone deacetylase",
            3,
        ),
        (
            "body { font-family: sans-serif } p { text-align: justify; margin: 0 15% }",
            "<p>",
            "انظر الفصل الثالث",
            6,
        ),
        // Justified narrow enough that word spaces stretch past half an em.
        ("p { text-align: justify; margin: 0 35% }", rtl, "HDAC", 3),
        ("p { text-align: justify; margin: 0 35% }", rtl, "2010", 6),
        (
            "p { text-align: justify; margin: 0 35% }",
            rtl,
            "النازعة للأسيتيل",
            6,
        ),
        // And the plain shapes, which must stay as they were.
        ("", rtl, "HDAC", 1),
        ("p { text-align: justify }", rtl, "2010", 3),
        ("", "<p>", "2010", 1),
        ("", "<p>", "النازعة للأسيتيل", 1),
    ] {
        let text = bracketed_paragraph(inside, repeat);
        for text in [text.clone(), unpointed(&text)] {
            let (content, logical) = read(style, open, &text);
            assert!(
                content.holds() && logical.holds(),
                "{open} {style:?}, ({inside}) x{repeat}: content {content:?}, logical {logical:?}"
            );
        }
    }
}

/// **A dir-less paragraph's line with Arabic at both ends and a Latin run in
/// brackets between reads as the other text UAX #9 draws alike** — a limit of
/// ruling 14 (`content-and-text.md`'s "Telling apart two texts UAX #9 draws
/// alike"), pinned so that a change to it is noticed.
///
/// With no `dir`, `… عوامل عدّة (HDAC). وقد …` is a left-to-right paragraph:
/// the pair is at its level, drawn facing inward, `(HDAC)`, with the Arabic
/// either side reversed. A right-to-left paragraph whose `/ToUnicode` names
/// the mirrored glyph draws `وقد … (HDAC) … عوامل عدّة` as exactly that line,
/// and nothing on the page says which drew it: both readings draw the line
/// (UAX #9 forwards, asserted here), and the bracket tie-break counts only a
/// pair facing outward. Both ends of the line are Arabic, so it reads as the
/// right-to-left paragraph: `(HDAC).` comes back `.)HDAC(`. K-11's probe has
/// 45 of these 120 dir-less books, every one with a Latin run in the
/// brackets, each pointed and not, in both builds.
#[test]
fn a_dirless_line_with_arabic_at_both_ends_and_latin_in_brackets_reads_as_its_drawn_alike_twin() {
    use tinker_pdf_shape::bidi::{logical_order, order_units, BaseDirection};
    let text = unpointed(&bracketed_paragraph("HDAC", 1));
    let bytes = epub_support::book::styled_book("ar", "", &format!("<p>{text}</p>"));
    let doc = Document::open(bytes.clone()).expect("the book opens");
    assert!(
        !conservation::conservation_in_logical_order(&bytes, &doc).holds(),
        "the dir-less line now reads as written: drop this limit from content-and-text.md"
    );
    let page = doc.page(0).expect("a page");
    let page_text = page.text();
    let line = page_text
        .lines()
        .into_iter()
        .find(|line| line.text.contains("HDAC"))
        .expect("the line holding the bracketed run");
    assert!(
        line.text.contains(".)HDAC("),
        "the line reads as the right-to-left twin: {:?}",
        line.text
    );
    // The line as drawn, left to right, and its two readings.
    let mut drawn: Vec<(f64, &str)> = line
        .chars
        .iter()
        .map(|c| (c.quad.bounds().0, c.text.as_str()))
        .collect();
    drawn.sort_by(|a, b| a.0.total_cmp(&b.0));
    let visual: Vec<&str> = drawn.iter().map(|(_, text)| *text).collect();
    let opens = visual.iter().position(|unit| *unit == "(");
    let closes = visual.iter().position(|unit| *unit == ")");
    assert!(
        opens < closes,
        "the pair is drawn facing inward: {visual:?}"
    );
    let read = |direction| -> (String, Vec<&str>) {
        let order = logical_order(&visual, direction);
        let read: Vec<&str> = order.iter().map(|at| visual[*at]).collect();
        let redrawn = order_units(&read, direction)
            .into_iter()
            .map(|at| read[at])
            .collect();
        (read.concat(), redrawn)
    };
    let (left_to_right, drawn_ltr) = read(BaseDirection::LeftToRight);
    let (right_to_left, drawn_rtl) = read(BaseDirection::RightToLeft);
    assert_eq!(
        drawn_ltr, visual,
        "a left-to-right paragraph draws the line"
    );
    assert_eq!(drawn_rtl, visual, "and so does a right-to-left one");
    assert!(left_to_right.contains("(HDAC)."), "{left_to_right:?}");
    assert_eq!(right_to_left, line.text, "ruling 14 reads the second");
}

/// **List markers and glosses in left-to-right text conserve** — ruling 14's
/// bracket tie-break as the review of the K-11 fix revised it.
///
/// A left-to-right paragraph's `1)` before a parenthesis, or a parenthesis
/// that opens at the end of a line and closes on the next, draws a closing
/// bracket left of an opening one, and a right-to-left paragraph draws the
/// same line for another text in which the two pair facing outward. As first
/// proposed, the tie-break read every such line from the right, and books
/// like these, which conserved before it, stopped conserving at their first
/// character: `<p>Step 1) read the word (…) aloud and 2) write it down, then
/// 3) check (…) again.</p>` in six of nine styles, Arabic and Hebrew, pointed
/// and not, in both builds, and three glosses wrapped at `margin: 0 25%` in a
/// `bundled-fonts` one. A reading from the right pairs no more of their
/// brackets than as typed, or pairs them with the spaces that stood outside
/// them inside the pair, and the tie-break now asks for more pairs, each set
/// against what it encloses.
#[test]
fn list_markers_and_glosses_in_left_to_right_text_conserve() {
    let words: [(&str, [&str; 4]); 2] = [
        ("ar", ["إنّ ال", "وقد أ", "عوامل عدّة", "الآن"]),
        ("he", ["בְּרֵא", "וַיֹּא", "שָׁלוֹם עוֹלָם", "בַּיִת"]),
    ];
    for (language, [first, second, gloss, word]) in words {
        let paragraphs = [
            format!(
                "Step 1) read the word ({first}) aloud and 2) write it down, then 3) check \
                 ({second}) again."
            ),
            format!(
                "Step 1) open the file ({word}) and 2) close it, then 3) read ({gloss}) once \
                 more and 4) stop here for now."
            ),
            format!(
                "Read peace ({second}) then war ({gloss}) and also ({word}) in this English \
                 sentence that keeps going on."
            ),
        ];
        for style in ["", "p { text-align: justify }", "p { margin: 0 25% }"] {
            for paragraph in &paragraphs {
                for text in [paragraph.clone(), unpointed_hebrew(&unpointed(paragraph))] {
                    let bytes =
                        epub_support::book::styled_book(language, style, &format!("<p>{text}</p>"));
                    let doc = Document::open(bytes.clone()).expect("the book opens");
                    let content = conservation(&bytes, &doc);
                    let logical = conservation::conservation_in_logical_order(&bytes, &doc);
                    assert!(
                        content.holds() && logical.holds(),
                        "{style:?}, {text:?}: content {content:?}, logical {logical:?}"
                    );
                }
            }
        }
    }
}

/// `text` with its Hebrew points taken out.
fn unpointed_hebrew(text: &str) -> String {
    text.chars()
        .filter(|c| !matches!(c, '\u{0591}'..='\u{05C7}'))
        .collect()
}

/// Whether `body`, set in `language` under `style`, conserves in content
/// and in logical order, pointed and with every mark taken out.
fn conserves_pointed_and_not(language: &str, style: &str, body: &str) {
    for body in [body.to_owned(), unpointed_hebrew(&unpointed(body))] {
        let bytes = epub_support::book::styled_book(language, style, &body);
        let doc = Document::open(bytes.clone()).expect("the book opens");
        let content = conservation(&bytes, &doc);
        let logical = conservation::conservation_in_logical_order(&bytes, &doc);
        assert!(
            content.holds() && logical.holds(),
            "{style:?}, {body}: content {content:?}, logical {logical:?}"
        );
    }
}

/// **Spacing carries a reader's pen only within right-to-left text** —
/// the review of the K-11 fix.
///
/// K-11's fix took the pen across a gap `Tw` or `Tc` made where a line closed
/// by `ET` met the next text object, so that a justified or letter-spaced
/// Arabic line was not cut into words read backwards. Each pointed book here
/// conserved at 9297d2d, which cut its line at the gap, and did not when the
/// pen was carried across it — the line so joined, each piece of it read
/// right alone, fell into ruling 14's limits, read by its two Hebrew ends or
/// by its majority as a right-to-left paragraph's. Their mark-free twins are
/// held to conserve too, which not all of them did at 9297d2d (the `-` line
/// under `word-spacing: 0.6em` did not):
///
/// - carried after a right-to-left word into a neutral, the `(` or `⟨` that
///   opens a Latin run, and into the Hebrew after `( ` in an English line;
/// - carried after a word of neutrals, the `-` of `שָׁלוֹם - (HDAC) - עוֹלָם`,
///   on a line that held the Latin;
/// - carried a little further than the spacing reaches, from one table cell
///   into the next, two Hebrew cells read as one word, `גדולספר`.
///
/// The pen is carried now only into a right-to-left glyph that starts where
/// the spacing put it, on a line holding no left-to-right letter or after a
/// right-to-left word (`tinker-pdf-content`'s `Stretch`). And the last,
/// a right-to-left paragraph letter-spaced by half an em, whose `) (` between
/// two Latin runs a reader read on its own (`bundled-fonts`): such a run of
/// neutrals is drawn as typed, as it was (`paint::neutrals_drawn_as_typed`).
#[test]
fn spacing_carries_the_pen_only_within_right_to_left_text() {
    let justified = "p, td { letter-spacing: 0.3em; text-align: justify; margin: 0 25% }";
    let spaced = "p { word-spacing: 0.6em; margin: 0 20% }";
    let cells = "p, td { letter-spacing: 0.5em }";
    for (language, style, body) in [
        ("he", spaced, "<p>שָׁלוֹם - (HDAC) - עוֹלָם מִלְחָמָה.</p>"),
        ("he", justified, "<p>שָׁלוֹם - (HDAC) - עוֹלָם מִלְחָמָה.</p>"),
        ("he", spaced, "<p>שָׁלוֹם עוֹלָם ⟨HDAC⟩. מִלְחָמָה סֵפֶר בַּיִת.</p>"),
        (
            "he",
            spaced,
            "<p>שָׁלוֹם עוֹלָם <b>(HDAC)</b>. מִלְחָמָה סֵפֶר בַּיִת.</p>",
        ),
        (
            "ar",
            justified,
            "<p>عوامل عدّة <b>(HDAC)</b>. وقد أصبحنا نعرف.</p>",
        ),
        (
            "ar",
            justified,
            "<p>عوامل عدّة <i>(histone deacetylase)</i>. وقد أصبحنا نعرف.</p>",
        ),
        (
            "en",
            justified,
            "<p>Note ( שָׁלוֹם ) spaced (x) and ( עוֹלָם מִלְחָמָה ) spaced again in the line.</p>",
        ),
    ] {
        conserves_pointed_and_not(language, style, body);
    }
    // Pointed only: these books' mark-free twins do not conserve in every
    // build, at 9297d2d either.
    let table = "<table style=\"border-spacing: 2px\"><tr><td>שָׁלוֹם</td><td>(HDAC)</td>\
                 <td>עוֹלָם.</td><td>מִלְחָמָה</td></tr><tr><td>סֵפֶר בַּיִת</td>\
                 <td>גָּדוֹל</td><td>(2010)</td><td>x</td></tr></table>";
    let mut pointed = vec![table];
    if cfg!(feature = "bundled-fonts") {
        pointed.push("<p dir=\"rtl\">שָׁלוֹם עוֹלָם (HDAC) (SIRT). מִלְחָמָה סֵפֶר בַּיִת.</p>");
    }
    for body in pointed {
        let bytes = epub_support::book::styled_book("he", cells, body);
        let doc = Document::open(bytes.clone()).expect("the book opens");
        let content = conservation(&bytes, &doc);
        let logical = conservation::conservation_in_logical_order(&bytes, &doc);
        assert!(
            content.holds() && logical.holds(),
            "{body}: content {content:?}, logical {logical:?}"
        );
    }
}

/// **A line whose pieces read right apart is read in them** — the review of
/// the K-11 fix.
///
/// K-11's fix joined the whole chain of pieces a line came to the reader
/// in, wherever any piece held a right-to-left character, so that a
/// right-to-left line opening with `(histone deacetylase).` kept its `(`
/// for the bracket tie-break to pair. The line after it, `deacetylase).
/// מִלְחָמָה סֵפֶר בַּיִת.`, is three pieces too, and read as two — `deacetylase`
/// and `). מִלְחָמָה …`, as 9297d2d joined them — each reads right; joined
/// whole it fell into the limit of a bracket a line break parted, its
/// Latin letters as many as its Hebrew ones, and read `.מִלְחָמָה סֵפֶר בַּיִת
/// .)deacetylase`. A chain is joined whole now only where the bracket
/// tie-break turns it read whole (`text_order::rejoin_split_lines`), and
/// otherwise as 9297d2d joined its pieces.
#[test]
fn a_line_whose_pieces_read_right_apart_is_read_in_them() {
    let style = "body { font-family: monospace } p { margin: 0 22% }";
    let text = "שָׁלוֹם עוֹלָם <i>(histone deacetylase)</i>. מִלְחָמָה סֵפֶר בַּיִת.";
    for body in [
        format!("<p dir=\"rtl\">{text}</p>"),
        format!("<div style=\"direction: rtl\"><p>{text}</p></div>"),
    ] {
        conserves_pointed_and_not("he", style, &body);
    }
}

/// **A neutral the device cut from its line carries no pen into the
/// right-to-left word after it** — the third review of the K-11 fix.
///
/// The text device starts a line afresh at every cut, so an opening mark
/// cut from the Latin text it belongs to — `«`, `"`, `#` or `?!` in a text
/// object of its own, a table cell's `[` — began a line that held no
/// left-to-right letter, and the pen was carried from it across the spacing
/// into the Hebrew or Arabic word after it. The line so joined read right
/// to left, its mark at the wrong end: `«שָׁלוֹם עוֹלָם»` letter-spaced by
/// `0.55em` as `שָׁלוֹם עוֹלָם«`, `<td>[שָׁלוֹם]</td>` as `שָׁלוֹם[`, and `<i>?!</i>
/// שָׁלוֹם עוֹלָם` at `word-spacing: 1em` as `שָׁלוֹם עוֹלָם !?`. Each book
/// conserved at 9297d2d (pointed; its mark-free twin did not, nor does it
/// now). After neutrals the pen goes on now only on a line that holds a
/// right-to-left letter of its own (`tinker-pdf-content`'s `Stretch`).
#[test]
fn a_neutral_cut_from_its_line_carries_no_pen_into_the_word_after_it() {
    let spaced = "p, li, td { letter-spacing: 0.55em; text-align: justify; margin: 0 20% }";
    let worded = "p, li { word-spacing: 1em; margin: 0 18% }";
    let table = "<table style=\"border-spacing: 0; letter-spacing: 0.6em\">\
                 <tr><td>שָׁלוֹם</td><td>עוֹלָם</td><td>מִלְחָמָה.</td><td>(HDAC)</td></tr>\
                 <tr><td>סֵפֶר בַּיִת</td><td>גָּדוֹל</td><td>(2010)</td><td>x</td></tr>\
                 <tr><td>«אוֹר»</td><td>יוֹם</td><td>[שָׁלוֹם]</td><td>עוֹלָם</td></tr>\
                 <tr><td>abc</td><td>מִלְחָמָה</td><td>def</td><td>סֵפֶר בַּיִת</td></tr></table>";
    for (language, style, body) in [
        ("he", spaced, "<p><i>«</i>שָׁלוֹם עוֹלָם<i>»</i></p>"),
        ("he", spaced, "<p><b>\"</b>שָׁלוֹם עוֹלָם<b>\"</b></p>"),
        ("he", spaced, "<p><i>#</i>שָׁלוֹם עוֹלָם</p>"),
        ("ar", spaced, "<p><i>«</i>عوامل عدّة<i>»</i></p>"),
        ("he", "", table),
        ("he", worded, "<p><i>?!</i> שָׁלוֹם עוֹלָם</p>"),
        ("ar", worded, "<p><i>(1)</i> عوامل عدّة</p>"),
    ] {
        let bytes = epub_support::book::styled_book(language, style, body);
        let doc = Document::open(bytes.clone()).expect("the book opens");
        let content = conservation(&bytes, &doc);
        let logical = conservation::conservation_in_logical_order(&bytes, &doc);
        assert!(
            content.holds() && logical.holds(),
            "{style:?}, {body}: content {content:?}, logical {logical:?}"
        );
    }
}

/// **A justified English paragraph quoting Hebrew in brackets reads as
/// written** — the third review of the K-11 fix.
///
/// `Words <b>שָׁלוֹם</b> <i>עוֹלָם</i> (<b>מִלְחָמָה</b>) and more English
/// text,`, five times over with the Hebrew words changing, under `text-align:
/// justify` and under `word-spacing: 0.3em`. A line that opens with a
/// bracketed Hebrew word the line above ended next to — `(הָאָרֶץ) and more
/// English text, Words …` — draws that pair facing outward, since N0 took
/// its level from the Hebrew word before it, on the line above; read from the
/// right the line pairs it, and the bracket tie-break turned the line,
/// fifty Latin letters to fifteen Hebrew ones, and read it backwards. Such a
/// pair is no evidence for the other direction (`tinker-pdf-shape`'s
/// `OpenedBefore`). In a default build each conserved in logical order at
/// 9297d2d, pointed and not; with `bundled-fonts`, whose wider Hebrew breaks
/// these lines elsewhere, the pointed paragraph set in monospace, justified,
/// at `margin: 0 20%` did.
#[test]
fn a_bracket_pair_the_line_above_resolved_turns_no_line() {
    let words = [
        "שָׁלוֹם",
        "עוֹלָם",
        "מִלְחָמָה",
        "סֵפֶר",
        "בַּיִת",
        "גָּדוֹל",
        "אוֹר",
        "יוֹם",
        "בְּרֵאשִׁית",
        "בָּרָא",
        "אֱלֹהִים",
        "הָאָרֶץ",
    ];
    let text: Vec<String> = (0..5)
        .map(|k| {
            format!(
                "Words <b>{}</b> <i>{}</i> (<b>{}</b>) and more English text,",
                words[3 * k % 12],
                words[(3 * k + 1) % 12],
                words[(3 * k + 2) % 12]
            )
        })
        .collect();
    let body = format!("<p>{}.</p>", text.join(" "));
    // Each style, and whether its unpointed twin conserved at 9297d2d too.
    let styles: &[(&str, bool)] = if cfg!(feature = "bundled-fonts") {
        &[(
            "body { font-family: monospace } p { text-align: justify; margin: 0 20% }",
            false,
        )]
    } else {
        &[
            ("p { text-align: justify }", true),
            ("p { word-spacing: 0.3em; margin: 0 20% }", true),
        ]
    };
    for &(style, twin) in styles {
        let bodies = if twin {
            vec![body.clone(), unpointed_hebrew(&body)]
        } else {
            vec![body.clone()]
        };
        for body in bodies {
            let bytes = epub_support::book::styled_book("en", style, &body);
            let doc = Document::open(bytes.clone()).expect("the book opens");
            let logical = conservation::conservation_in_logical_order(&bytes, &doc);
            assert!(logical.holds(), "{style:?}, {body}: {logical:?}");
        }
    }
}

/// **A cut ORNATE PARENTHESIS, or an ARABIC PERCENT SIGN, carries no pen**
/// (review of the third review's K-11 fix).
///
/// The text device carries the pen across spacing only into a glyph that
/// reads right to left by its class. The third review's table took in every
/// punctuation mark of the right-to-left blocks, the ORNATE PARENTHESES `﴿`
/// and `﴾` among them, which are `ON` like `(`: cut into a text object of
/// its own, a `﴿` began a line the pen went on from into the Arabic or
/// Hebrew word after it, and `﴿عوامل عدّة﴾` letter-spaced read
/// `﴾عوامل عدّة﴿`, `عوامل ﴿HDAC﴾ عدّة` at `word-spacing: 1em` `﴾HDAC﴿ عوامل`.
/// And the ARABIC PERCENT SIGN, `ET`, joined a justified line whole that then
/// read the sign at its far end, `٥٠ بيت ٫ كبير٪`. Each conserved at
/// 9297d2d, in both builds; the table holds `R`, `AL` and the ARABIC COMMA
/// now (`tinker-pdf-content`'s `reads_right_to_left`).
#[test]
fn an_ornate_parenthesis_or_an_arabic_percent_sign_carries_no_pen() {
    let spaced = "p, li, td { letter-spacing: 0.55em; text-align: justify; margin: 0 20% }";
    let worded = "p, li { word-spacing: 1em; margin: 0 18% }";
    let justified = "p { text-align: justify; margin: 0 41% }";
    for (language, style, body) in [
        (
            "ar",
            spaced,
            "<p><i>\u{FD3F}</i>عوامل عدّة<i>\u{FD3E}</i></p>",
        ),
        (
            "he",
            spaced,
            "<p><i>\u{FD3F}</i>שָׁלוֹם עוֹלָם<i>\u{FD3E}</i></p>",
        ),
        ("ar", worded, "<p>عوامل \u{FD3F}HDAC\u{FD3E} عدّة وقد.</p>"),
        (
            "ar",
            justified,
            "<p dir=\"rtl\">الطالب ﴿جميل﴾. جميل <b>٪</b>٥٠ بيت <i>٫</i> كبير ﴿نور﴾.</p>",
        ),
        (
            "ar",
            justified,
            "<p dir=\"rtl\">الطالب ﴿جميل﴾. جميل ٪٥٠ بيت ٫ كبير ﴿نور﴾.</p>",
        ),
    ] {
        let bytes = epub_support::book::styled_book(language, style, body);
        let doc = Document::open(bytes.clone()).expect("the book opens");
        let content = conservation(&bytes, &doc);
        let logical = conservation::conservation_in_logical_order(&bytes, &doc);
        assert!(
            content.holds() && logical.holds(),
            "{style:?}, {body}: content {content:?}, logical {logical:?}"
        );
    }
}

/// **A sign after a number that opens a wrapped Arabic line reads after
/// it** (review of the third review's K-11 fix).
///
/// `<p dir="rtl">جميل بيت كبير نور 5% يوم</p>` in sans-serif, broken
/// before the `5`. W2 makes the `5` an Arabic number for the Arabic letter on
/// the line above, so the `%` is a neutral at the paragraph's level, in one
/// piece with `يوم`; Helvetica draws it and the overflow font the Arabic, and
/// drawn in its level's order Helvetica's `% ` read back `%5 يوم`: a reader
/// of the line alone finds no Arabic letter before the `5`, and W5 makes
/// the `%` a European number's. Such a piece's neutrals are drawn as typed
/// now, as every one was at 9297d2d, which conserved each of these in both
/// builds (`paint::neutrals_drawn_as_typed`).
#[test]
fn a_sign_after_a_number_opening_a_wrapped_arabic_line_reads_as_typed() {
    let style = "body { font-family: sans-serif } p { margin: 0 42% }";
    for body in [
        "<p dir=\"rtl\">جميل بيت كبير نور 5% يوم</p>",
        "<p dir=\"rtl\">جميل بيت كبير نور 5‰ يوم</p>",
        "<p dir=\"rtl\">جميل بيت كبير نور 10–20% يوم</p>",
    ] {
        let bytes = epub_support::book::styled_book("ar", style, body);
        let doc = Document::open(bytes.clone()).expect("the book opens");
        let content = conservation(&bytes, &doc);
        let logical = conservation::conservation_in_logical_order(&bytes, &doc);
        assert!(
            content.holds() && logical.holds(),
            "{body}: content {content:?}, logical {logical:?}"
        );
    }
}

/// **A styled `?!` glued to the word after it, letter-spaced and justified,
/// reads as typed** (review of the third review's K-11 fix).
///
/// `كتاب <i>?!</i> مدرسة <b>?!</b>الطالب جميل.` justified at
/// `letter-spacing: 0.25em`: the bold `?!` is drawn against `الطالب` and a
/// stretched word space before it, so a reader reads the two together and,
/// by the comma tie-break, left to right — the `?!` in the order it was
/// drawn. Drawn in its level's order it read `!?`; as typed, as at 9297d2d,
/// which conserved it in logical order in both builds, it reads `?!`
/// (`paint::Gaps::left_to_right_ends`).
#[test]
fn a_styled_pair_glued_to_the_word_after_it_reads_as_typed_letter_spaced() {
    let style = "p { letter-spacing: 0.25em; text-align: justify; margin: 0 25% }";
    let body = "<p dir=\"rtl\">كتاب <i>?!</i> مدرسة <b>?!</b>الطالب جميل.</p>";
    let bytes = epub_support::book::styled_book("ar", style, body);
    let doc = Document::open(bytes.clone()).expect("the book opens");
    let logical = conservation::conservation_in_logical_order(&bytes, &doc);
    assert!(logical.holds(), "{body}: {logical:?}");
}

/// **A line of a paragraph with no `dir` that opens with a bracket pair
/// reads its full stop last** (review of the third review's K-11 fix).
///
/// `<p>جميل بيت كبير <b>(</b>نور<b>)</b> يوم.</p>`, broken before the
/// bracket: N0 takes the pair's level from the Arabic on the line above, so
/// the second line is drawn `موي )رون(.`, its stop at the right. The comma
/// tie-break reads that left to right; read from the right the line pairs
/// `(نور)`, and the bracket tie-break turned it and read the stop first,
/// `(نور). يوم` in logical order. A pair with a bracket just left of the
/// mark the comma tie-break took is one the line above resolved, and counts
/// for neither reading (`tinker-pdf-shape`'s `OpenedBefore`). And the line
/// `(١٬٢٣٤), الآن الموادّ تكبح عوامل.`, letter-spaced, in pieces the device
/// cut, was joined whole for that turn and read `.(١٬٢٣٤), …`. Each conserved
/// in logical order at 9297d2d — the number's book in content order too —
/// in the build it is listed for.
#[test]
fn a_dirless_line_opening_with_a_bracket_pair_reads_its_full_stop_last() {
    let narrow = "p { margin: 0 42% }";
    let books: &[(&str, &str)] = if cfg!(feature = "bundled-fonts") {
        &[
            ("ar", "<p>جميل بيت كبير <b>(</b>نور<b>)</b> يوم.</p>"),
            (
                "he",
                "<p>שלום עולם ספר בית יום אור יום <b>(</b>בית<b>)</b> יום.</p>",
            ),
        ]
    } else {
        &[
            ("ar", "<p>جميل بيت كبير <b>(</b>نور<b>)</b> يوم.</p>"),
            ("he", "<p>שלום עולם ספר <b>(</b>בית<b>)</b> יום.</p>"),
        ]
    };
    for (language, body) in books {
        let bytes = epub_support::book::styled_book(language, narrow, body);
        let doc = Document::open(bytes.clone()).expect("the book opens");
        let logical = conservation::conservation_in_logical_order(&bytes, &doc);
        assert!(logical.holds(), "{body}: {logical:?}");
    }
    let body = "<p>عوامل عدّة وقد (١٬٢٣٤), أصبحنا نعرف الآن الموادّ. عدّة وقد أصبحنا \
                (١٬٢٣٤), نعرف الآن الموادّ تكبح. وقد أصبحنا نعرف (١٬٢٣٤), الآن الموادّ \
                تكبح عوامل. </p>";
    let style = "p, li, td { letter-spacing: 0.5em }";
    let bytes = epub_support::book::styled_book("ar", style, body);
    let doc = Document::open(bytes.clone()).expect("the book opens");
    let content = conservation(&bytes, &doc);
    let logical = conservation::conservation_in_logical_order(&bytes, &doc);
    assert!(
        content.holds() && logical.holds(),
        "content {content:?}, logical {logical:?}"
    );
}

/// **A line of a paragraph with no `dir` that opens with a bracket pair
/// reads its closing quotation marks and its stops last** (K-11 review,
/// finding V1).
///
/// The test above's pair, which the line above resolved, was passed over
/// only where one of seven marks ended the line and the unit just left of it
/// held the bracket: `<p>جميل بيت كبير <b>(</b>نور<b>)</b> يوم?!</p>`,
/// broken before the bracket, draws `موي )رون(?!`, whose rightmost unit is
/// a stop but whose unit just left of it is another, so the pair counted,
/// the bracket tie-break turned the line and it read `!?(نور) يوم`; so did
/// `."`, whose rightmost unit is no stop, and `…`, which was none. The line
/// opens left of any closing quotation marks and then one stop or more, an
/// ellipsis among them (`tinker-pdf-shape`'s
/// `OpenedBefore::before_its_punctuation`). Each book conserved in logical
/// order at 9297d2d and did not at 56dfd93, in the build it is written for.
#[test]
fn a_dirless_line_opening_with_a_bracket_pair_reads_its_closing_quotes_and_stops_last() {
    let narrow = "p { margin: 0 42% }";
    // The Hebrew paragraph that breaks before its bracket at this width: a
    // longer one with `bundled-fonts`, as for the full stop above.
    let hebrew = if cfg!(feature = "bundled-fonts") {
        "שלום עולם ספר בית יום אור יום"
    } else {
        "שלום עולם ספר"
    };
    for end in ["?!", ".\"", "…"] {
        for (language, body) in [
            (
                "ar",
                format!("<p>جميل بيت كبير <b>(</b>نور<b>)</b> يوم{end}</p>"),
            ),
            (
                "he",
                format!("<p>{hebrew} <b>(</b>בית<b>)</b> יום{end}</p>"),
            ),
        ] {
            let bytes = epub_support::book::styled_book(language, narrow, &body);
            let doc = Document::open(bytes.clone()).expect("the book opens");
            let logical = conservation::conservation_in_logical_order(&bytes, &doc);
            assert!(logical.holds(), "{body}: {logical:?}");
        }
    }
}

// ---- the cases the reviews found -------------------------------------------
//
// Every book a finding of the K-11 and CD-19 reviews named is a row of the
// test of its mechanism, here and in `epub_fallback.rs`, read as
// `epub_support::reading` reads a book. A case that
// was fixed is held to its right reading (`Expect::Holds`); a case still open
// is pinned at what it reads today (`Expect::Pinned`) and says which ROADMAP
// row leaves it, so that it fails when the limit closes and the row moves.

/// **A case holds what it says and a pin holds exactly that** — the
/// harness the cases below rely on (`epub_support::reading::meets`).
///
/// `Holds` asks that the verdicts it names hold and lets the others be;
/// `Pinned` asks that exactly its verdicts hold, so that a gain fails it as a
/// loss does; `Neither` asks nothing. A verdict name it does not know is a
/// mistake in the table, not a reading, and panics. And a book drawn glyph
/// over glyph reads not apart, which no case below can show.
#[test]
fn a_case_holds_what_it_names_and_a_pin_exactly_that() {
    use epub_support::reading::{meets, Reading};
    let reading = Reading {
        cons: false,
        logical: true,
        whole: true,
        found: true,
        overprint: 0,
        text: String::new(),
    };
    assert_eq!(reading.holding(), "logical whole found apart");
    assert!(meets(Expect::Holds("logical found"), &reading).is_ok());
    assert!(meets(Expect::Holds(""), &reading).is_ok());
    assert_eq!(
        meets(Expect::Holds("cons logical"), &reading),
        Err("does not hold cons".to_owned())
    );
    assert!(meets(Expect::Pinned("found apart whole logical"), &reading).is_ok());
    assert_eq!(
        meets(Expect::Pinned("logical whole apart"), &reading),
        Err("pinned at [logical whole apart], reads [logical whole found apart]".to_owned())
    );
    assert!(meets(Expect::Pinned("cons logical whole found apart"), &reading).is_err());
    assert!(meets(Expect::Neither, &reading).is_ok());
    let over = Reading {
        overprint: 2,
        ..reading
    };
    assert_eq!(over.holding(), "logical whole found");
    assert!(meets(Expect::Holds("apart"), &over).is_err());
    assert!(std::panic::catch_unwind(|| meets(Expect::Holds("conserves"), &over)).is_err());
    // A real book drawn glyph over glyph, as its style asks: with a negative
    // letter spacing each letter starts before the one left of it ends (18
    // times here). Every case below reads apart, so this one is what shows
    // that the overprint count is read at all.
    check(&[Case::both(
        "negative letter spacing, each letter over the one before it (the overprint verdict's control)",
        "en",
        "p { letter-spacing: -0.4em }",
        "<p>abcdef ghijkl mnopqr.</p>",
        Expect::Pinned("cons logical whole found"),
    )]);
}

const LETTER_SPACED: &str =
    "p, li, td { letter-spacing: 0.55em; text-align: justify; margin: 0 20% }";
const WORD_SPACED: &str = "p, li { word-spacing: 1em; margin: 0 18% }";
const JUSTIFIED: &str = "p { text-align: justify; margin: 0 41% }";
const NARROW: &str = "p { margin: 0 42% }";

/// **A neutral the pen could carry into the word beside it reads where it
/// was typed** — the K-11 round-4 review's code F1 and hunt H5, fixed by
/// 56dfd93's table (`tinker-pdf-content`'s `reads_right_to_left`).
///
/// The text device carries the pen across spacing only into a glyph that
/// reads right to left by its class. The ORNATE PARENTHESES are `ON`, like
/// `(`; counted as right to left, a cut `﴿` carried the pen into the word
/// after it, in Arabic, Hebrew, an English sentence quoting Hebrew, at
/// `word-spacing: 1em` and in a table's cells. The ARABIC PERCENT SIGN is
/// `ET`; counted so, it joined a justified line whole, styled or not and
/// whatever letter is the overflow font's code 32. Guillemets, which round
/// 4's own rule covered, guard the shape. And `٪٥٠` opening a wrapped line of
/// a justified paragraph, styled or not, reads its sign at the line's far
/// end, `٥٠ يوم بيت كبير٪`, at every tree since 9297d2d: pinned (ROADMAP
/// CD-19, "Read so at every tree").
#[test]
fn a_neutral_the_pen_could_carry_into_a_word_reads_where_it_was_typed() {
    let percent = "<p dir=\"rtl\">الطالب ﴿جميل﴾. جميل <b>٪</b>٥٠ بيت <i>٫</i> كبير ﴿نور﴾.</p>";
    check(&[
        Case::both("ornate, ar, letter-spaced (code F1)", "ar", LETTER_SPACED, "<p><i>\u{FD3F}</i>عوامل عدّة<i>\u{FD3E}</i></p>", Expect::Holds("cons logical found apart")),
        Case::both("ornate, he, letter-spaced (code F1)", "he", LETTER_SPACED, "<p><i>\u{FD3F}</i>שָׁלוֹם עוֹלָם<i>\u{FD3E}</i></p>", Expect::Holds("cons logical found apart")),
        Case::both(
            "ornate, an English sentence quoting Hebrew (code F1)",
            "en",
            LETTER_SPACED,
            "<p>The verse \u{FD3F}שָׁלוֹם עוֹלָם\u{FD3E} and the words \u{FD3F}<i>מִלְחָמָה</i>\u{FD3E} are quoted in this English sentence here.</p>", Expect::Holds("cons logical found apart")),
        Case::both("ornate, ar, word-spaced (code F1)", "ar", WORD_SPACED, "<p>عوامل \u{FD3F}HDAC\u{FD3E} عدّة وقد.</p>", Expect::Holds("cons logical whole found apart")),
        Case::both(
            "ornate, ar, table cells (code F1)",
            "ar",
            LETTER_SPACED,
            "<table style=\"border-spacing: 0\"><tr><td>عوامل</td><td>\u{FD3F}عدّة\u{FD3E}</td><td>وقد</td></tr><tr><td>\u{FD3F}أصبحنا\u{FD3E}</td><td>نعرف</td><td>x</td></tr></table>", Expect::Holds("cons logical found apart")),
        Case::both("guillemets, ar, letter-spaced (guard)", "ar", LETTER_SPACED, "<p><i>«</i>عوامل عدّة<i>»</i></p>", Expect::Holds("cons logical found apart")),
        Case::both("guillemets, he, letter-spaced (guard)", "he", LETTER_SPACED, "<p><i>«</i>שָׁלוֹם עוֹלָם<i>»</i></p>", Expect::Holds("cons logical found apart")),
        Case::both("ARABIC PERCENT SIGN styled, justified (hunt H5)", "ar", JUSTIFIED, percent, Expect::Holds("cons logical found apart")),
        Case::both(
            "ARABIC PERCENT SIGN unstyled, justified (hunt H5)",
            "ar",
            JUSTIFIED,
            "<p dir=\"rtl\">الطالب ﴿جميل﴾. جميل ٪٥٠ بيت ٫ كبير ﴿نور﴾.</p>", Expect::Holds("cons logical found apart")),
        Case::both(
            "ARABIC PERCENT SIGN styled, justified, BEH at code 32 (hunt H5)",
            "ar",
            JUSTIFIED,
            format!("<p><b>ب</b></p>{percent}"), Expect::Holds("cons logical found apart")),
        Case::both(
            "ARABIC PERCENT SIGN styled, opening a justified line (as at every tree)",
            "ar",
            JUSTIFIED,
            "<p dir=\"rtl\">جميل بيت كبير نور <b>٪</b>٥٠ يوم بيت كبير</p>", Expect::Pinned("logical found apart")),
        Case::both(
            "ARABIC PERCENT SIGN unstyled, opening a justified line (as at every tree)",
            "ar",
            JUSTIFIED,
            "<p dir=\"rtl\">جميل بيت كبير نور ٪٥٠ يوم بيت كبير</p>", Expect::Pinned("found apart")),
    ]);
}

/// `<br/>`, then `{mark}` before a styled pair, opening a right-to-left line:
/// the price probe's `price/*/brb` book, `{lead}` the index of its first word.
fn price_book(words: &[&'static str], lead: usize, mark: &str, open: &str, close: &str) -> String {
    let s = |i: usize| word(words, i);
    format!(
        "<p dir=\"rtl\">{} {}<br/>{mark}<b>{open}</b>{}<b>{close}</b> {} {}</p>",
        s(lead),
        s(lead + 1),
        s(lead + 2),
        s(lead + 3),
        s(lead + 4)
    )
}

/// **A line that opens with a bracket pair reads its stops where they were
/// typed** — ruling 14's bracket tie-break and the pair the line above
/// resolved (`tinker-pdf-shape`'s `OpenedBefore`), and its price.
///
/// - The K-11 round-4 review's hunt H2 and code F2, fixed by 56dfd93: a
///   dir-less `(W) W.` line with the brackets styled apart, `[ ]` and `?`
///   too, and `(١٬٢٣٤),` opening a letter-spaced line, read their stop first.
///   The right-to-left paragraph guards the shape. The unstyled `(W) W.`,
///   read `نور) يوم(.`, and `(١٢٣),` are K-11's own dir-less limit, read so at
///   every tree since 9297d2d, pinned (ROADMAP CD-19, "Read so at every
///   tree").
/// - The fix's verification, V1, fixed by acf944b: the same line ending in
///   several stops or closing quotation marks.
/// - CR2 and F6 of the review of CD-19's fix: a right-to-left line that opens
///   with `?!`, `!!`, `".` or `'.` before a pair, after a `<br/>` or a soft
///   wrap, reads as the dir-less line does — ruling 14's price, which the
///   owner accepted on 12 October 2026; pinned, with the ellipsis the
///   owner's decision named. With `bundled-fonts` the soft-wrapped `".`
///   reads right.
/// - H6 and PRICE-RESIDUAL, a run of stops before a styled pair after a
///   `<br/>`: with no spacing, `…`, `?!` and `!!` pay the price, and a lone
///   `.`, the price's control, reads as at 9297d2d and 56dfd93 (it conserved
///   at 94e3bb5 alone), the four pinned; a run that mixes the stops, `.!` or
///   `!…`, ends no sentence (ruling 14) and reads as typed, held.
///   At `word-spacing: 0.6em` all six read right, and so do the `!?` and `?!`
///   books before styled brackets at `0.6em` and `1em` that lost their
///   structure tree's order where the overflow font at 0 Tw met the rule,
///   fixed by 935b077's read-back (held); the three `'.` books at `0.5em`
///   that hold it no more (Left (5)) are pinned.
/// - Confirm F3 of the review of CD-19's fix: two bracket pairs ending in
///   the ARABIC COMMA in pointed Hebrew, letter-spaced and justified, with
///   `bundled-fonts` (Left (4)), pinned; a default build reads them as at
///   every tree.
/// - H2-7d of the second review: `(abc ברא) אלהים؟`, ending in a stop outside
///   the rule's, which K-11's reader reads from the right since 56dfd93
///   (Left (4)), pinned.
#[test]
fn a_line_opening_with_a_bracket_pair_reads_its_stops_where_they_were_typed() {
    let hebrew = if cfg!(feature = "bundled-fonts") {
        "שלום עולם ספר בית יום אור יום"
    } else {
        "שלום עולם ספר"
    };
    let mut cases = vec![
        Case::both("dir-less (W) W. styled, ar (hunt H2)", "ar", NARROW, "<p>جميل بيت كبير <b>(</b>نور<b>)</b> يوم.</p>", Expect::Holds("logical found apart")),
        Case::both("dir-less (W) W. styled, he (hunt H2)", "he", NARROW, format!("<p>{hebrew} <b>(</b>בית<b>)</b> יום.</p>"), Expect::Holds("logical found apart")),
        Case::both("dir-less [W] W. styled, ar (hunt H2)", "ar", NARROW, "<p>جميل بيت كبير <b>[</b>نور<b>]</b> يوم.</p>", Expect::Holds("logical found apart")),
        Case::both("dir-less (W) W? styled, ar (hunt H2)", "ar", NARROW, "<p>جميل بيت كبير <b>(</b>نور<b>)</b> يوم?</p>", Expect::Holds("logical found apart")),
        Case::both("right-to-left (W) W. styled, ar (guard)", "ar", NARROW, "<p dir=\"rtl\">جميل بيت كبير <b>(</b>نور<b>)</b> يوم.</p>", Expect::Holds("cons logical whole found apart")),
        Case::both(
            "direction: rtl (W) W. styled, ar (guard)",
            "ar",
            NARROW,
            "<div style=\"direction: rtl\"><p>جميل بيت كبير <b>(</b>نور<b>)</b> يوم.</p></div>", Expect::Holds("cons logical whole found apart")),
        Case::both("dir-less (W) W. unstyled, ar (K-11's dir-less limit, as at every tree)", "ar", NARROW, "<p>جميل بيت كبير (نور) يوم.</p>", Expect::Pinned("found apart")),
        Case::both(
            "dir-less (١٬٢٣٤), opening a letter-spaced line (code F2)",
            "ar",
            "p, li, td { letter-spacing: 0.5em }",
            "<p>عوامل عدّة وقد (١٬٢٣٤), أصبحنا نعرف الآن الموادّ. عدّة وقد أصبحنا (١٬٢٣٤), نعرف الآن الموادّ تكبح. وقد أصبحنا نعرف (١٬٢٣٤), الآن الموادّ تكبح عوامل. </p>", Expect::Holds("cons logical found apart")),
        Case::both(
            "dir-less (١٢٣), opening a letter-spaced line (K-11's dir-less limit, as at every tree)",
            "ar",
            "p, li, td { letter-spacing: 0.5em }",
            "<p>عوامل عدّة وقد (١٢٣), أصبحنا نعرف الآن الموادّ. عدّة وقد أصبحنا (١٢٣), نعرف الآن الموادّ تكبح. وقد أصبحنا نعرف (١٢٣), الآن الموادّ تكبح عوامل. </p>", Expect::Pinned("found apart")),
    ];
    for end in ["...", "?!", "!!", "!?", ".\"", ".»", "…", ".”"] {
        cases.push(Case::both(
            "dir-less (W) W ending several marks, ar (V1)",
            "ar",
            NARROW,
            format!("<p>كتاب مدرسة الطالب <b>(</b>جميل<b>)</b> بيت{end}</p>"),
            Expect::Holds("logical found apart"),
        ));
        cases.push(Case::both(
            "dir-less (W) W ending several marks, he (V1)",
            "he",
            NARROW,
            format!("<p>{hebrew} <b>(</b>בית<b>)</b> יום{end}</p>"),
            Expect::Holds("logical found apart"),
        ));
    }
    let wide = "p { margin: 0 30% }";
    for (what, language, body) in [
        (
            "price ?! after a break, ar (CR2, F6)",
            "ar",
            "<p dir=\"rtl\">نور يوم<br/>?!(عوامل) وقد أصبحنا</p>",
        ),
        (
            "price !! after a break, ar (CR2, F6)",
            "ar",
            "<p dir=\"rtl\">يوم عوامل<br/>!!(وقد) أصبحنا نعرف</p>",
        ),
        (
            "price \". after a break, he (CR2, F6)",
            "he",
            "<p dir=\"rtl\">אלהים הארץ<br/>\".(שלום) עולם מלחמה</p>",
        ),
        (
            "price '. styled after a break, he (CR2)",
            "he",
            "<p dir=\"rtl\">עולם מלחמה<br/>'.<b>(</b>ספר<b>)</b> בית גדול</p>",
        ),
        (
            "price … after a break, ar (the ellipsis the owner named)",
            "ar",
            "<p dir=\"rtl\">كتاب مدرسة<br/>\u{2026}(الطالب) جميل بيت</p>",
        ),
    ] {
        cases.push(Case::both(
            what,
            language,
            wide,
            body,
            Expect::Pinned("found apart"),
        ));
    }
    cases.push(Case::each(
        "price \". after a soft wrap, he (CR2)",
        "he",
        NARROW,
        "<p dir=\"rtl\">אלהים הארץ שלום \".(עולם) מלחמה ספר. עולם מלחמה ספר \".(בית) גדול אור.</p>",
        Expect::Pinned("found apart"),
        Expect::Holds("cons logical whole found apart"),
    ));
    for (language, words) in [("ar", AR), ("he", HE)] {
        let s = |i: usize| word(&words, i);
        for mark in ["…", "?!", "!!", ".", ".!", "!…"] {
            let body = format!(
                "<p dir=\"rtl\">{} {}<br/>{mark}<b>(</b>{}<b>)</b> {} {}</p>",
                s(0),
                s(8),
                s(3),
                s(4),
                s(11)
            );
            // A run that mixes the stops ends no sentence, and reads as typed.
            let (what, expect) = if matches!(mark, ".!" | "!…") {
                (
                    "a run that mixes the stops opening a line after a break, no spacing (H6)",
                    Expect::Holds("cons logical whole found apart"),
                )
            } else {
                (
                    "a run of stops opening a line after a break, no spacing (H6, PRICE-RESIDUAL)",
                    Expect::Pinned("logical found apart"),
                )
            };
            cases.push(Case::both(what, language, "", body.clone(), expect));
            cases.push(Case::both(
                "a run of stops opening a line after a break, word-spaced (H6, PRICE-RESIDUAL)",
                language,
                "p { word-spacing: 0.6em }",
                body,
                Expect::Holds("cons logical whole found apart"),
            ));
        }
    }
    for (style, language, words, lead, mark, expect) in [
        (
            "p { word-spacing: 0.6em; margin: 0 20% }",
            "ar",
            AR,
            8,
            "!?",
            Expect::Holds("cons logical whole found apart"),
        ),
        (
            "p { word-spacing: 1em; margin: 0 20% }",
            "ar",
            AR,
            8,
            "!?",
            Expect::Holds("cons logical whole found apart"),
        ),
        (
            "p { word-spacing: 1em; margin: 0 20% }",
            "he",
            HE,
            6,
            "?!",
            Expect::Holds("cons logical whole found apart"),
        ),
        (
            "p { word-spacing: 0.5em; margin: 0 20% }",
            "ar",
            AR,
            13,
            "'.",
            Expect::Pinned("found apart"),
        ),
    ] {
        for (open, close) in [("(", ")"), ("[", "]"), ("{", "}")] {
            cases.push(Case::both(
                "price, before styled brackets under word spacing (PRICE-RESIDUAL)",
                language,
                style,
                price_book(&words, lead, mark, open, close),
                expect,
            ));
        }
    }
    let hep = |i: usize| word(&HEP, i);
    for (open, close) in [("(", ")"), ("[", "]")] {
        let two = rep(2, |i| {
            format!(
                "{} {} <b>{open}</b>{}<b>{close}</b> {open}{}{close} {}\u{60C}",
                hep(i + 6),
                hep(i + 7),
                hep(i + 8),
                hep(i + 9),
                hep(i + 10)
            )
        });
        cases.push(Case::each(
            "two pairs ending ، in pointed Hebrew, letter-spaced (confirm1 F3, Left (4))",
            "he",
            "p { letter-spacing: 0.5em; text-align: justify; margin: 0 25% }",
            format!("<p>{two}</p>"),
            Expect::Holds("found apart"),
            Expect::Pinned("found apart"),
        ));
    }
    for end in ["؟", "\u{6D4}", "\u{5C3}"] {
        cases.push(Case::both(
            "dir-less (abc W) W ending a stop outside the rule's, he (H2-7d)",
            "he",
            "p { word-spacing: 0.6em }",
            format!("<p>(abc ברא) אלהים{end}</p>"),
            Expect::Pinned("found apart"),
        ));
    }
    check(&cases);
}

/// **A left-to-right Hebrew paragraph that ends a number and a stop** —
/// ROADMAP CD-19, Left (1), pinned; H1 of the review of CD-19's fix.
///
/// `<p>בראשית. אור ٦٠.</p>` at `word-spacing: 0.5em`, in a default build: the
/// glyph CD-19's code-32 move put a word spacing right left a gap before the
/// final stop, which read alone; written at 0 Tw, the stop abuts the
/// right-to-left run, ruling 14's rejoin joins them, and the comma
/// tie-break, which asks for a strong right-to-left character at the line's
/// left end, does not take a line that opens with a number, so the stop is
/// read first. 56 books conserved at 56dfd93, 53 at 9297d2d too; a change to
/// the tie-break is the owner's to make. With `bundled-fonts` the stand-in
/// draws the Hebrew, and these books are found by search with no glyph drawn
/// over another, as at every tree since 94e3bb5 (held). Without the number
/// the line reads right, in a default build a gain over both trees since
/// e7ca679 (held); the paragraph alone, its own first letter at code 32,
/// reads as at every tree (a guard). And with them, H4(2) of the same review:
/// a right-to-left paragraph of the same words letter-spaced and justified,
/// whose rules' drawing a reader reads with the number run into the word
/// beside it, `٢٠١٠ .۴۵`, reads whole in both builds, the read-back searching
/// on to a drawing that reads its words (H-150, held). V2 of round
/// 6's hunt is the same tie-break at a closing bracket, which it refuses as
/// it does a number: `<p>שלום.<br/>בראשית (אלהים).</p>` letter-spaced, a
/// number at spacings the review did not name, and with `bundled-fonts`
/// Arabic, each conserving at 9297d2d and 56dfd93 and lost since e7ca679
/// wrote the overflow font at `0 Tw` (pinned).
#[test]
fn a_left_to_right_hebrew_paragraph_ending_a_number_and_a_stop() {
    let worded = "p { word-spacing: 0.5em }";
    let s = |i: usize| word(&HEN, i);
    let prose = rep(3, |i| {
        format!(
            "{} {} {}, {} {} {}.",
            s(i),
            s(i + 1),
            s(i + 2),
            s(i + 3),
            s(i + 4),
            s(i + 5)
        )
    });
    let mut cases = vec![
        Case::each(
            "ש, then a dir-less paragraph ending ٦٠.",
            "he",
            worded,
            "<p>ש</p><p>בראשית. אור ٦٠.</p>",
            Expect::Pinned("found apart"),
            Expect::Holds("found apart"),
        ),
        Case::each(
            "ש, then a dir=ltr paragraph ending ٦٠.",
            "he",
            worded,
            "<p>ש</p><p dir=\"ltr\">בראשית. אור ٦٠.</p>",
            Expect::Pinned("found apart"),
            Expect::Holds("found apart"),
        ),
        Case::each(
            "ש, then a dir-less paragraph ending 60.",
            "he",
            worded,
            "<p>ש</p><p>בראשית. אור 60.</p>",
            Expect::Pinned("found apart"),
            Expect::Holds("found apart"),
        ),
        Case::each(
            "ש, then a dir-less paragraph ending ٦٠:",
            "he",
            worded,
            "<p>ש</p><p>בראשית. אור ٦٠:</p>",
            Expect::Pinned("found apart"),
            Expect::Holds("found apart"),
        ),
        Case::both(
            "a dir-less paragraph ending ٦٠., its own first letter at code 32",
            "he",
            worded,
            "<p>בראשית. אור ٦٠.</p>",
            Expect::Holds("found apart"),
        ),
        Case::both(
            "ש, then a dir-less paragraph ending no number (control)",
            "he",
            worded,
            "<p>ש</p><p>בראשית. אור.</p>",
            Expect::Holds("cons logical whole found apart"),
        ),
        Case::each(
            "breadth A en/dot",
            "he",
            "p, td { word-spacing: 0.5em }",
            "<p>ש</p><p>בראשית ברא. אור גדול 60.</p>",
            Expect::Pinned("found apart"),
            Expect::Holds("found apart"),
        ),
    ];
    for attr in ["", " dir=\"ltr\""] {
        cases.push(Case::each(
            "breadth A an/dot",
            "he",
            "p { word-spacing: 0.5em; margin: 0 20% } td { word-spacing: 0.6em }",
            format!("<p>ש</p><p{attr}>בראשית ברא. אור גדול ٦٠.</p>"),
            Expect::Pinned("found apart"),
            Expect::Holds("found apart"),
        ));
        cases.push(Case::each(
            "ofscripts hen/prose/nat",
            "he",
            "p { word-spacing: 0.5em; margin: 0 20% }",
            format!("<p{attr}>{prose}</p>"),
            Expect::Pinned("found apart"),
            Expect::Holds("found apart"),
        ));
        cases.push(Case::each(
            "ofscripts hen/prose/nat, monospace, justified (conserved at 56dfd93 only)",
            "he",
            "body { font-family: monospace } p { word-spacing: 0.5em; text-align: justify; margin: 0 25% }",
            format!("<p{attr}>{prose}</p>"), Expect::Pinned("found apart"), Expect::Holds("cons logical found apart")));
    }
    cases.push(Case::each(
        "ofscripts hen/prose/out/rtl, letter-spaced and justified (H4(2))",
        "he",
        "p { letter-spacing: 0.1em; word-spacing: 0.5em; text-align: justify; margin: 0 25% }",
        format!("<p>ף</p><p dir=\"rtl\">{prose}</p>"),
        Expect::Holds("cons logical whole found apart"),
        Expect::Holds("cons logical whole found apart"),
    ));
    // V2 of round 6's lane V: the same class at a closing bracket and at
    // spacings Left (1) did not name, and with `bundled-fonts`.
    for (what, language, style, body) in [
        (
            "a dir-less line ending a bracketed word and a stop, letter-spaced (V2)",
            "he",
            "p { letter-spacing: 0.3em; word-spacing: -0.2em }",
            "<p>שלום.<br/>בראשית (אלהים).</p>",
        ),
        (
            "a dir-less line ending ٦٠., letter-spaced 0.15em, word-spaced 0.3em (V2)",
            "he",
            "p { letter-spacing: 0.15em; word-spacing: 0.3em }",
            "<p>שלום.<br/>בראשית ٦٠.</p>",
        ),
        (
            "a dir-less Arabic paragraph of styled words and (W)., letter-spaced (V2)",
            "ar",
            "p { letter-spacing: 0.15em; word-spacing: 0.3em }",
            "<p>كتاب <b>مدرسة</b> الطالب <i>جميل</i>, (بيت). جميل <b>بيت</b> كبير <i>نور</i>, (يوم). نور <b>يوم</b> عوامل <i>وقد</i>, (أصبحنا).</p>",
        ),
    ] {
        cases.push(Case::both(what, language, style, body, Expect::Pinned("found apart")));
    }
    check(&cases);
}

/// **A dir-less line K-11's reader reads from the right** — ROADMAP CD-19,
/// Left (3) and (4), pinned; H2-2 and H2-7 of the second review of CD-19's
/// fix.
///
/// Since 56dfd93 K-11's carry joins the right-to-left word after a
/// bracketed gloss to the line, and the line, mostly right to left, is read
/// from the right whatever the writer draws: a Latin gloss before a
/// bracketed Arabic word at `0.5em` and at `12px`, a table cell ending a dash
/// before a `(ספר) בית` cell, and in a default build a bold Greek word before
/// a bracketed Hebrew one in a spanned cell (Left (4)), pinned. A styled
/// Latin, Greek, Cyrillic or Armenian word before a gloss was fixed at
/// `12px` by 935b077's read-back, which centres the word space styled apart
/// in its stretch (held); before a bare gloss and a Latin tail at `2em`,
/// `TextDevice` mends the line the word space's stretch cut, since the joined
/// line's ends read left to right (Left (3)), held in both builds. And V3 of round 6's
/// hunt: two right-to-left cells of a table with no `dir`, touching, at
/// `word-spacing: 1em`, which K-11's reader joins into one line that holds
/// nothing left to right and whose left end is the second cell's `)`, so the
/// comma tie-break does not take it and the stop is read first: they
/// conserved at 9297d2d, and since 56dfd93, whose content stream is 9297d2d's
/// byte for byte, they read from the right (Left (4)), pinned.
#[test]
fn a_dirless_line_with_a_right_to_left_gloss() {
    let px12 = "p { word-spacing: 12px }";
    check(&[
        Case::both("a Latin gloss, styled, before (ספר) בית (H2-7a)", "he", px12, "<p>abc <b>def</b> (ספר) בית</p>", Expect::Holds("cons logical whole found apart")),
        Case::both("a Latin gloss before (كتاب) بيت (H2-7a)", "ar", "p { word-spacing: 0.5em }", "<p>abc def (كتاب) بيت</p>", Expect::Pinned("found apart")),
        Case::both("a Latin gloss before (كتاب) بيت at 12px (H2-7a)", "ar", px12, "<p>abc def (كتاب) بيت</p>", Expect::Pinned("found apart")),
        Case::both(
            "a cell ending a dash before a (ספר) בית cell (H2-7b)",
            "he",
            "td { word-spacing: 12px }",
            "<table><tr><td>abc —</td><td>(ספר) בית</td></tr></table>", Expect::Pinned("found apart")),
        Case::each(
            "a bold Greek word before a (ספר) בית cell, spanned (H2-7b)",
            "he",
            "td, th, caption, div { word-spacing: 12px } table { border-spacing: 2px }",
            "<table><tr><td colspan=\"2\">αλφα <b>βήτα</b> (ספר) בית</td></tr><tr><td>αλφα <b>βήτα</b></td><td>(ספר) בית</td></tr></table>", Expect::Pinned("whole found apart"), Expect::Holds("whole found apart")),
        Case::both("a bold Greek word before (ספר) בית (H2-2)", "he", px12, "<p>αλφα <b>βήτα</b> (ספר) בית</p>", Expect::Holds("cons logical whole found apart")),
        Case::both("a bold Cyrillic word before (ספר) בית (H2-2)", "he", px12, "<p>книга <b>дом</b> (ספר) בית</p>", Expect::Holds("cons logical whole found apart")),
        Case::both("a bold Greek word before (كتاب) بيت (H2-2)", "ar", px12, "<p>αλφα <b>βήτα</b> (كتاب) بيت</p>", Expect::Holds("cons logical whole found apart")),
        Case::both("a bold Greek word before (ספר) בית, dir=ltr (H2-2)", "he", px12, "<p dir=\"ltr\">αλφα <b>βήτα</b> (ספר) בית</p>", Expect::Holds("cons logical whole found apart")),
        Case::both("a bold Greek word before (ספר) בית at 2em (H2-2)", "he", "p { word-spacing: 2em }", "<p>αλφα <b>βήτα</b> (ספר) בית</p>", Expect::Holds("cons logical whole found apart")),
        Case::both("a bold Armenian word before (ספר) בית (H2-2)", "he", px12, "<p>գիրք <b>տուն</b> (ספר) בית</p>", Expect::Holds("cons logical whole found apart")),
        Case::both("a Greek word before (ספר) בית at 6px (guard)", "he", "p { word-spacing: 6px }", "<p>αλφα <b>βήτα</b> (ספר) בית</p>", Expect::Holds("cons logical whole found apart")),
        Case::both("an unstyled Greek gloss before (ספר) בית (guard)", "he", px12, "<p>αλφα βήτα (ספר) בית</p>", Expect::Holds("cons logical whole found apart")),
        Case::both(
            "a bold Greek word before a bare gloss and a Latin tail at 2em (Left (3))",
            "he",
            "p { word-spacing: 2em }",
            "<p>αλφα <b>βήτα</b> ספר בית xyz.</p>", Expect::Holds("cons logical whole found apart")),
        Case::both("a bold Armenian word before a bare gloss and a Latin tail (H2-2)", "he", px12, "<p>գիրք <b>տուն</b> ספר בית xyz.</p>", Expect::Holds("cons logical whole found apart")),
        // With `bundled-fonts` the Hebrew book conserves at every tree: a
        // guard there.
        Case::each(
            "two touching right-to-left cells, no dir (V3)",
            "he",
            "table { border-spacing: 0 } td, th { padding: 0 } td, th { word-spacing: 1em }",
            "<table><tr><td><b>שלום</b> <i>עולם</i>،</td><td>מלחמה <i>(</i>ספר<i>)</i>.</td></tr></table>",
            Expect::Pinned("logical found apart"),
            Expect::Holds("cons logical found apart"),
        ),
        Case::both(
            "two touching right-to-left cells, no dir, Arabic (V3)",
            "ar",
            "table { border-spacing: 0 } td, th { padding: 0 } td, th { word-spacing: 1em }",
            "<table><tr><td><b>كتاب</b> <i>مدرسة</i>،</td><td>الطالب <i>(</i>جميل<i>)</i>.</td></tr></table>",
            Expect::Pinned("logical found apart"),
        ),
    ]);
}
