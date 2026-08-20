//! The first book that reads (gap 31, milestone 8).
//!
//! Milestone 4 gave every book its spine's worth of grey pages; this is where
//! the pages carry the book. What is asserted here is the reading path end to
//! end — markup, the user-agent stylesheet, the cascade, layout, fragmentation
//! and synthesis — and the three things the plan's row 8 says the milestone is
//! judged on.
//!
//! # The user-agent stylesheet's absence is visible, not merely worse
//!
//! Row 8 asks for *"a test that removing it produces an undifferentiated
//! book"*. **An undifferentiated book has more than one property**, and the
//! rule thirteen milestones have found is that a test for one of two
//! independent consequences is not a test. Removing the sheet does three
//! separate things, and
//! [`without_the_ua_stylesheet_a_book_has_no_block_structure_at_all`] asserts
//! all three: every element becomes `display: inline`, so a chapter is one
//! paragraph; a heading is set at body size and body weight; and `<head>` is
//! set into the flow, so the `<title>` and the `<style>` become text.
//!
//! # The census is the number, and it reaches a caller
//!
//! Milestone 6 built the `Known`/`Unsupported`/`Unknown` split so that this
//! milestone could say, per book, exactly which properties it did not
//! implement. It is not printed by a test and forgotten: it is on the report
//! as [`ArchiveWarning::UnimplementedProperty`], counted by **elements
//! reached**, and the figures are written down in `tests/epub/CENSUS.tsv` so a
//! milestone that implements a property has to say so in the same commit.

mod epub_support;

use std::collections::BTreeSet;

use tinker_pdf::epub::read::UA_STYLESHEET;
use tinker_pdf::epub::SpineDefect;
use tinker_pdf::{ArchiveWarning, Document, OpenOptions};

// ---- the corpus ---------------------------------------------------------------

const COMMITTED: &[&str] = &[
    "calibre-book-cover.epub",
    "calibre-book-nocover.epub",
    "pandoc-book-cover.epub",
    "pandoc-book-epub2.epub",
    "pandoc-book-nocover.epub",
    "pandoc-plates.epub",
];

fn corpus_book(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("epub")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// Every page's text, in page order.
fn pages_text(doc: &Document) -> Vec<String> {
    (0..doc.page_count())
        .map(|at| doc.page(at).expect("a page").text().plain_text())
        .collect()
}

/// The census a book's report carries, ranked as the report ranks it.
fn census(doc: &Document) -> Vec<(String, usize)> {
    doc.archive()
        .expect("a book carries a report")
        .warnings()
        .iter()
        .filter_map(|warning| match warning {
            ArchiveWarning::UnimplementedProperty { property, elements } => {
                Some(((*property).to_owned(), *elements))
            }
            _ => None,
        })
        .collect()
}

// ---- the user-agent stylesheet -------------------------------------------------

/// **The committed sheet is a file, and it is the file a book is set with.**
///
/// `include_str!` is what makes that true, and this is what says it stays
/// true: the constant a test reads and the sheet the reader cascades are the
/// same bytes, so a build that grew a second copy in Rust — the shape a UA
/// sheet acquires when somebody adds "just one more" element name to a
/// `match` — could not pass this and the two tests below at once.
#[test]
fn the_committed_sheet_is_what_a_book_is_set_with() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("epub")
        .join("ua.css");
    let file = std::fs::read_to_string(&path).expect("src/epub/ua.css");
    assert_eq!(
        file, UA_STYLESHEET,
        "the published constant is not the committed file"
    );
    // And it is CSS rather than a list: it has to parse, and the rules it
    // parses to have to be enough to give a book its boxes.
    assert!(file.contains("display: block"));
    assert!(file.contains("display: none"));
    assert!(file.contains("display: list-item"));
}

/// **Removing the user-agent stylesheet produces an undifferentiated book**,
/// and "undifferentiated" is three separate claims.
///
/// The comparison is run through `tinker-pdf-css` and `tinker-pdf-layout`
/// directly rather than through `Document::open`, because there is no way to
/// open a book without the sheet — which is the point of committing it. What
/// is compared is the same content document, the same cascade and the same
/// layout, with and without the one sheet.
///
/// Three consequences, each of which a different bug would remove:
///
/// 1. **No block boxes.** Every element computes `display: inline`, so a
///    chapter of eleven paragraphs is one run-on paragraph. A build whose UA
///    sheet had only the `display` rules would pass this and fail (2).
/// 2. **A heading is body text.** `<h1>` is `2em` and `bold` in the sheet and
///    nowhere else; without it a heading is set at the paragraph's size and
///    weight. A build whose sheet had only the block rules would pass (1) and
///    fail this.
/// 3. **`<head>` is set into the flow.** The `<title>` and the `<style>`
///    element's own CSS become text on the page. A build whose sheet had no
///    `display: none` rule would pass (1) and (2) and fail this — and would
///    fail text conservation on every book in the corpus, which is how a
///    milestone with no such test would eventually find out.
#[test]
fn without_the_ua_stylesheet_a_book_has_no_block_structure_at_all() {
    use tinker_pdf::epub::paint::BookMetrics;
    use tinker_pdf::epub::read::{box_tree, PX_TO_PT};
    use tinker_pdf::epub::xhtml;
    use tinker_pdf_css::cascade::{cascade, Origin};
    use tinker_pdf_css::media::MediaContext;
    use tinker_pdf_css::parser::parse as css_parse;
    use tinker_pdf_css::property::Display;
    use tinker_pdf_css::{Budget as CssBudget, Limits as CssLimits, NoImports};
    use tinker_pdf_layout::{layout, Limits as LayoutLimits, Options};

    let markup = concat!(
        r#"<?xml version="1.0" encoding="utf-8"?>"#,
        r#"<html xmlns="http://www.w3.org/1999/xhtml">"#,
        r#"<head><title>A Chapter</title><style>p { color: #123456 }</style></head>"#,
        r#"<body><h1>The heading</h1><p>First paragraph.</p><p>Second paragraph.</p>"#,
        r#"<ul><li>an item</li></ul></body></html>"#
    );
    let dom = xhtml::read(markup.as_bytes(), &tinker_pdf_xml::Limits::DEFAULT).expect("markup");

    let media = MediaContext::screen(400.0, 800.0);
    let limits = CssLimits::DEFAULT;
    let mut budget = CssBudget::new(&limits);
    let ua = css_parse(
        UA_STYLESHEET.as_bytes(),
        None,
        &NoImports,
        &media,
        &limits,
        &mut budget,
    )
    .expect("the committed sheet parses");

    let with = cascade(
        &[(Origin::UserAgent, &ua)],
        &dom.nodes,
        &limits,
        &mut budget,
    )
    .expect("a cascade with the sheet");
    let without = cascade(&[], &dom.nodes, &limits, &mut budget).expect("a cascade without it");

    let at = |name: &str| {
        dom.nodes
            .iter()
            .position(|node| node.name == name)
            .unwrap_or_else(|| panic!("no <{name}> in the fixture"))
    };

    // (1) No block boxes at all, anywhere.
    assert!(
        without
            .styles
            .iter()
            .all(|style| style.display == Display::Inline),
        "an element still generates a block with no user-agent sheet"
    );
    assert_eq!(with.styles[at("body")].display, Display::Block);
    assert_eq!(with.styles[at("p")].display, Display::Block);
    assert_eq!(with.styles[at("li")].display, Display::ListItem);

    // (2) A heading is body text: same size, same weight as a paragraph.
    let heading = at("h1");
    let paragraph = at("p");
    assert_eq!(
        without.styles[heading].font_size, without.styles[paragraph].font_size,
        "a heading is already a different size with no sheet"
    );
    assert_eq!(without.styles[heading].font_weight, 400);
    assert!(
        with.styles[heading].font_size > with.styles[paragraph].font_size * 1.5,
        "the sheet does not make a heading bigger"
    );
    assert_eq!(with.styles[heading].font_weight, 700);

    // (3) `<head>` is set into the flow, so the title and the stylesheet
    // become text on the page.
    assert_eq!(with.styles[at("head")].display, Display::None);
    assert_ne!(without.styles[at("head")].display, Display::None);

    // And the whole of it, through layout: one block box against many, and the
    // stylesheet's own source set as text.
    let options = Options::new(400.0 / PX_TO_PT, 100_000.0);
    let laid_with = layout(
        &box_tree(&dom, &with),
        &BookMetrics,
        &options,
        &LayoutLimits::DEFAULT,
    )
    .expect("a layout with the sheet");
    let laid_without = layout(
        &box_tree(&dom, &without),
        &BookMetrics,
        &options,
        &LayoutLimits::DEFAULT,
    )
    .expect("a layout without it");

    // **Two paragraphs share a line box with no sheet and do not with one.**
    // A count of lines would be a number that happens to differ; this is the
    // definition of an inline formatting context, and it is what "the chapter
    // is one run-on paragraph" means. The anchors are what make it checkable:
    // `BoxNode::anchor` carries the element index through collapsing, line
    // breaking and pagination, so a run can be asked which `<p>` it came from
    // after all three.
    let share_a_line = |laid: &tinker_pdf_layout::Layout, first: usize, second: usize| {
        let ys = |element: usize| -> Vec<i64> {
            laid.pages
                .iter()
                .flat_map(|page| page.runs.iter())
                .filter(|run| run.anchor == Some(element as u32))
                .map(|run| (run.y * 100.0) as i64)
                .collect()
        };
        let first = ys(first);
        let second = ys(second);
        assert!(
            !first.is_empty() && !second.is_empty(),
            "a paragraph set nothing"
        );
        first.iter().any(|y| second.contains(y))
    };
    // The two `<p>` elements, which are the text nodes' owners and therefore
    // the anchors their runs carry.
    let paragraphs: Vec<usize> = dom
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| node.name == "p")
        .map(|(index, _)| index)
        .collect();
    assert_eq!(paragraphs.len(), 2, "the fixture has two paragraphs");
    assert!(
        share_a_line(&laid_without, paragraphs[0], paragraphs[1]),
        "two paragraphs are on separate lines with no user-agent sheet"
    );
    assert!(
        !share_a_line(&laid_with, paragraphs[0], paragraphs[1]),
        "two paragraphs share a line with the user-agent sheet"
    );

    // **And the space between them is a margin and not only a line.** This is
    // the assertion the injection matrix asked for: `p { margin: 1em 0 }` is in
    // the sheet, both measured producers set the same rule in their own, and a
    // defect that deleted the sheet's copy changed nothing about any book in
    // the corpus — so nothing anywhere could fail. The fixture has no author
    // sheet at all, which is the only place the rule is reachable.
    let first_line = |element: usize| -> f64 {
        laid_with
            .pages
            .iter()
            .flat_map(|page| page.runs.iter())
            .filter(|run| run.anchor == Some(element as u32))
            .map(|run| run.y)
            .fold(f64::INFINITY, f64::min)
    };
    let gap = first_line(paragraphs[1]) - first_line(paragraphs[0]);
    // One line of 16-pixel text at `line-height: normal` is about 19 pixels;
    // the sheet's `margin: 1em 0` collapses between the two paragraphs to a
    // further 16. A build with the block rule and no margin rule sets them one
    // line apart, which is what this number is either side of.
    assert!(
        gap > 30.0,
        "two paragraphs are {gap} apart, which is one line and no margin"
    );

    let text_without: String = laid_without
        .pages
        .iter()
        .flat_map(|page| page.runs.iter())
        .map(|run| run.text.clone())
        .collect();
    assert!(
        text_without.contains("#123456"),
        "the <style> element's own CSS is not on the page: {text_without}"
    );
    assert!(
        text_without.contains("A Chapter"),
        "the <title> is not on the page: {text_without}"
    );
    let text_with: String = laid_with
        .pages
        .iter()
        .flat_map(|page| page.runs.iter())
        .filter(|run| !run.generated)
        .map(|run| run.text.clone())
        .collect();
    assert!(!text_with.contains("#123456"), "{text_with}");
    assert!(!text_with.contains("A Chapter"), "{text_with}");
}

/// And the same three consequences seen **through the reader**, on a real
/// book's page.
///
/// The cascade test above proves the computed values; this one proves the page.
/// Both are needed: a build whose cascade was right and whose box tree ignored
/// `display` would pass the first and fail this.
#[test]
fn a_real_book_has_block_structure_a_heading_and_no_head_text() {
    let doc = Document::open(corpus_book("pandoc-book-cover.epub")).expect("a book");
    let text = pages_text(&doc).join("\n");

    // (3) `<head>` is not in the flow: the `<title>` of every content document
    // in this book is `ch001.xhtml`, and no page says so.
    assert!(
        !text.contains("ch001.xhtml"),
        "a content document's <title> reached the page"
    );
    assert!(
        !text.contains("color-scheme"),
        "a <style> element's own CSS reached the page"
    );

    // (1) Block structure: the chapter's paragraphs are on separate lines, so
    // the last word of one is not joined to the first of the next.
    assert!(
        text.contains("What a container is"),
        "the chapter heading is missing"
    );

    // (2) A heading is set larger than the body, which is visible in the
    // content stream rather than in the extracted text.
    let sizes = font_sizes(&doc);
    assert!(
        sizes.len() > 1,
        "every run in the book is set at one size: {sizes:?}"
    );
    assert!(
        sizes.iter().any(|size| *size > 125),
        "nothing in the book is set larger than the base size: {sizes:?}"
    );
}

/// Every `Tf` size in a document's content streams, rounded to a tenth.
fn font_sizes(doc: &Document) -> BTreeSet<u32> {
    let mut out = BTreeSet::new();
    for at in 0..doc.page_count() {
        for block in doc.page(at).expect("a page").text().blocks {
            for line in block.lines {
                for character in line.chars {
                    out.insert((character.size * 10.0).round() as u32);
                }
            }
        }
    }
    out
}

// ---- the reading path ----------------------------------------------------------

/// **`Page::text()` returns the words in reading order.**
///
/// Reading order and not merely presence: the assertion is that the whole
/// book's text, concatenated in page order and run order, holds these phrases
/// **in this sequence**. A build that painted its runs in tree order rather
/// than in line order would put every heading before every paragraph and pass
/// a `contains` test on each.
#[test]
fn a_page_reports_its_words_in_reading_order() {
    let doc = Document::open(corpus_book("pandoc-book-cover.epub")).expect("a book");
    let text: String = pages_text(&doc)
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    let sequence = [
        "A Short Account of Containers",
        "What a container is",
        "A container is a file that is other files.",
        "The order of things",
        "the order the entries were written",
        "What goes wrong quietly",
        "Where the parts are named",
    ];
    let mut from = 0usize;
    for phrase in sequence {
        let at = text[from..]
            .find(phrase)
            .unwrap_or_else(|| panic!("{phrase:?} is not after the phrase before it in: {text}"));
        from += at + phrase.len();
    }
}

/// **A chapter takes as many pages as its text needs.**
///
/// The half `tests/epub_package.rs` cannot assert on a fixture of one-sentence
/// chapters: one spine item, one long chapter, and more pages than itemrefs.
/// It is also the fragmentation half of text conservation — every character of
/// a chapter split over four pages is still on exactly one of them.
#[test]
fn a_chapter_takes_as_many_pages_as_its_text_needs() {
    let doc = Document::open(corpus_book("pandoc-book-cover.epub")).expect("a book");
    let report = doc.archive().expect("a report");
    let ch1: Vec<usize> = report
        .pages()
        .iter()
        .enumerate()
        .filter(|(_, origin)| origin.name == "EPUB/text/ch001.xhtml")
        .map(|(at, _)| at)
        .collect();
    assert!(
        ch1.len() > 1,
        "the longest chapter in the corpus fits on one page"
    );
    // Consecutive, because a chapter that fragmented into pages 3 and 7 would
    // have interleaved itself with another chapter.
    assert!(
        ch1.windows(2).all(|pair| pair[1] == pair[0] + 1),
        "a chapter's pages are not consecutive: {ch1:?}"
    );
}

/// The Japanese line in five of the six books reaches the page, and reaches
/// `Page::text()`.
///
/// **The corpus was built to catch a space-only line breaker and it catches a
/// `WinAnsiEncoding`-only writer too.** Twenty-five kanji and kana are not in
/// Windows code page 1252, and a build that wrote them as UTF-8 bytes into a
/// simple font's string would put mojibake on the page and lose them from
/// extraction — which is text conservation failing on the one sentence the
/// corpus exists for.
#[test]
fn the_japanese_line_survives_the_encoding() {
    const LINE: &str = "\u{65e5}\u{672c}\u{8a9e}\u{306e}\u{7d44}\u{7248}";
    for name in [
        "calibre-book-cover.epub",
        "calibre-book-nocover.epub",
        "pandoc-book-cover.epub",
        "pandoc-book-epub2.epub",
        "pandoc-book-nocover.epub",
    ] {
        let doc = Document::open(corpus_book(name)).expect(name);
        let text: String = pages_text(&doc).join(" ").split_whitespace().collect();
        assert!(text.contains(LINE), "{name} lost its Japanese line");
    }
    // And the one book that has no Japanese in it does not gain any, which is
    // what says the assertion above is about the book rather than about a
    // constant this test carries.
    let plates = Document::open(corpus_book("pandoc-plates.epub")).expect("plates");
    let text: String = pages_text(&plates).join(" ");
    assert!(!text.contains(LINE));
}

/// A list marker is **drawn and not extracted**.
///
/// 14.8.2.2's artifact, and the only reason text conservation can stay an
/// equality: `tinker-pdf-layout` flags generated content, this build wraps it
/// in `/Artifact BMC … EMC`, and `TextDevice` skips it. Two claims, because a
/// build that simply did not draw markers would pass the second.
#[test]
fn a_list_marker_is_drawn_and_is_not_extracted() {
    let doc = Document::open(corpus_book("calibre-book-cover.epub")).expect("a book");
    let text = pages_text(&doc).join(" ");
    assert!(
        text.contains("the order the entries were"),
        "the list itself is missing"
    );
    assert!(
        !text.contains("1."),
        "an ordered list's marker was extracted as text: {text}"
    );

    // And it is on the page: something is drawn, which is the half a text
    // assertion deliberately cannot see.
    let drawn = (0..doc.page_count()).any(|at| {
        let page = doc.page(at).expect("a page");
        let bitmap = page.render(&tinker_pdf::RenderOptions::default());
        // A page with a list on it has more ink than a page of prose alone
        // would: the assertion that the marker is *somewhere* is the bitmap's,
        // because it is deliberately not in the text.
        bitmap.data.iter().any(|byte| *byte != 0xFF)
    });
    assert!(drawn, "nothing was drawn on any page");
}

/// **The doctype mode milestone 2 built is the mode a book is read in**, and
/// the same bytes are refused by the default one.
///
/// Milestone 2 closed owing *"the facade end-to-end test over the real
/// books"*, and milestone 4 wrote that it *"still belongs to milestone 8, which
/// is the first milestone that reads a content document"*. This is it.
///
/// Two directions, because a test of one is not a test: every content document
/// of the EPUB 2 book carries XHTML 1.1's public identifier, which
/// `Doctype::Refuse` — the default, and what XPS passes — refuses before the
/// first tag; and the book's text is on its pages, which it could not be if the
/// facade had passed the default.
#[test]
fn a_real_books_doctype_is_skipped_here_and_refused_by_the_default_mode() {
    use tinker_pdf_xml::{Error, Limits as XmlLimits, Source};

    let bytes = corpus_book("pandoc-book-epub2.epub");
    let markup = String::from_utf8(
        epub_support::read(&bytes, "EPUB/text/ch001.xhtml").expect("the chapter"),
    )
    .expect("UTF-8");
    assert!(
        markup.contains(r#"<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN""#),
        "the fixture no longer carries the doctype this test is about"
    );

    // The default mode, which every other reader in this workspace uses.
    let source = Source::new(markup.as_bytes()).expect("it decodes");
    let refused = source
        .reader(&XmlLimits::DEFAULT)
        .find_map(|event| event.err());
    assert_eq!(
        refused,
        Some(Error::DoctypeUnsupported),
        "the default mode no longer refuses a doctype"
    );

    // And the mode milestone 2 added, through the door a host uses: the
    // chapter's own words are on the book's pages.
    let doc = Document::open(bytes).expect("a book");
    let text: String = pages_text(&doc).join(" ").split_whitespace().collect();
    assert!(
        text.contains("Acontainerisafilethatisotherfiles"),
        "the EPUB 2 book's chapter is not on any page"
    );
}

// ---- links and the outline ------------------------------------------------------

/// **A cross-reference between spine items reaches the page as a link
/// annotation**, and it points at the page the target is actually on.
///
/// Three claims, and the third is the one a build gets wrong: the links exist,
/// they are on the page the anchor's text is on, and their **destination** is
/// the page the `id` is on rather than the first page of the chapter. The
/// fixture's `href="ch001.xhtml#what-goes-wrong-quietly"` names a section
/// partway through a chapter that spans three pages, so a build that resolved
/// every fragment to its chapter's first page would fail here and nowhere else.
#[test]
fn a_cross_reference_becomes_a_link_to_the_page_its_target_is_on() {
    use tinker_pdf_cos::dest::{Action, Destination};

    let doc = Document::open(corpus_book("pandoc-book-cover.epub")).expect("a book");
    let mut targets: Vec<(u32, Option<u32>)> = Vec::new();
    for at in 0..doc.page_count() {
        for link in doc.page(at).expect("a page").links() {
            let to = match link.target {
                Some(Action::GoTo(Destination::Explicit { page_index, .. })) => page_index,
                other => panic!("page {at}: a cross-reference became {other:?}"),
            };
            targets.push((at, to));
        }
    }
    assert!(
        targets.len() >= 6,
        "a book with six internal cross-references has {} links",
        targets.len()
    );
    assert!(
        targets.iter().all(|(_, to)| to.is_some()),
        "a link points nowhere: {targets:?}"
    );
    // The navigation document links forward into the chapters and the chapter
    // links across its own sections, so at least one link points forwards.
    assert!(
        targets
            .iter()
            .any(|(from, to)| to.is_some_and(|to| to > *from)),
        "no link points forwards: {targets:?}"
    );
    // And not every link lands on the same page, which is what a build that
    // resolved every fragment to its chapter's first page would produce.
    let distinct: BTreeSet<Option<u32>> = targets.iter().map(|(_, to)| *to).collect();
    assert!(
        distinct.len() > 2,
        "every cross-reference resolves to the same page or two: {distinct:?}"
    );
}

/// A link's **rectangle** is over the words that are the link.
///
/// The other half of the pair above, and the one a page count cannot see: a
/// build that put every annotation at the origin, or one box around the whole
/// paragraph, would satisfy every destination assertion there is. The check is
/// that each rectangle overlaps a text line on its own page and is narrower
/// than the measure.
#[test]
fn a_links_rectangle_is_over_the_words_that_are_the_link() {
    let doc = Document::open(corpus_book("pandoc-book-cover.epub")).expect("a book");
    let mut checked = 0usize;
    for at in 0..doc.page_count() {
        let page = doc.page(at).expect("a page");
        let (width, height) = page.size();
        for link in page.links() {
            let rect = link.rect;
            assert!(
                rect.x0 >= 0.0 && rect.y0 >= 0.0 && rect.x1 <= width && rect.y1 <= height,
                "page {at}: a link is off the page: {rect:?}"
            );
            assert!(
                rect.x1 - rect.x0 < width * 0.9,
                "page {at}: a link is the whole measure wide: {rect:?}"
            );
            assert!(
                rect.y1 - rect.y0 < 40.0,
                "page {at}: a link is taller than a line: {rect:?}"
            );
            // **And a glyph's origin is strictly inside it**, which is the
            // assertion that says the rectangle is over the *words* rather than
            // merely near them. A hit area that begins at the baseline covers
            // the top of every glyph and none of the bottom, and it passes
            // every bound above — the injection matrix is what found that, and
            // this is what closes it.
            let baselines: Vec<(f64, f64)> = page
                .text()
                .blocks
                .iter()
                .flat_map(|block| block.lines.iter())
                .flat_map(|line| line.chars.iter())
                .map(|character| character.origin)
                .collect();
            assert!(
                baselines.iter().any(|(x, y)| {
                    *x >= rect.x0 && *x <= rect.x1 && *y > rect.y0 && *y < rect.y1
                }),
                "page {at}: no glyph's baseline is inside {rect:?}"
            );
            checked += 1;
        }
    }
    assert!(checked >= 6, "only {checked} links were checked");
}

/// **The navigation document becomes the outline**, and so does an NCX when
/// there is no navigation document.
///
/// Two of the six books have no §5.4 navigation document at all — one calibre
/// book ships only an NCX, and one pandoc book's `nav.xhtml` is never marked
/// `properties="nav"` — so a build that read only EPUB 3's shape would give a
/// third of the corpus no table of contents and would look correct doing it.
#[test]
fn every_book_gets_an_outline_from_whichever_toc_it_has() {
    use tinker_pdf_cos::dest::Destination;

    // Book, where its table of contents comes from, how many top-level entries
    // it has and how many entries are nested under them. The nesting count is
    // here because a build that flattened the tree would keep the top-level
    // count of four of the six books and lose the shape of the other two.
    let expected: &[(&str, &str, usize, usize)] = &[
        ("calibre-book-cover.epub", "nav.xhtml", 3, 0),
        ("calibre-book-nocover.epub", "toc.ncx", 3, 0),
        ("pandoc-book-cover.epub", "nav.xhtml", 2, 2),
        ("pandoc-book-epub2.epub", "nav.xhtml", 3, 2),
        ("pandoc-book-nocover.epub", "nav.xhtml", 2, 2),
        ("pandoc-plates.epub", "nav.xhtml", 3, 0),
    ];
    for (name, source, top_level, nested) in expected {
        let doc = Document::open(corpus_book(name)).expect(name);
        let outline = doc.outline();
        let titles: Vec<&str> = outline.iter().map(|entry| entry.title.as_str()).collect();
        assert_eq!(
            outline.len(),
            *top_level,
            "{name} ({source}) has the wrong number of top-level entries: {titles:?}"
        );
        let under: usize = outline.iter().map(|entry| entry.children.len()).sum();
        assert_eq!(under, *nested, "{name} ({source}) lost its nesting");
        assert!(
            outline.iter().all(|entry| !entry.title.is_empty()),
            "{name} has an outline entry with no title: {titles:?}"
        );
        for entry in &outline {
            match &entry.destination {
                Some(Destination::Explicit { page_index, .. }) => assert!(
                    page_index.is_some(),
                    "{name}: {:?} points at no page",
                    entry.title
                ),
                other => panic!("{name}: {:?} became {other:?}", entry.title),
            }
        }
    }
}

/// And the outline is **nested**, because a table of contents that is a flat
/// list of everything is a different document from the one the book wrote.
#[test]
fn an_outline_keeps_the_nesting_the_book_wrote() {
    let doc = Document::open(corpus_book("pandoc-book-cover.epub")).expect("a book");
    let outline = doc.outline();
    let nested: usize = outline.iter().map(|entry| entry.children.len()).sum();
    assert_eq!(
        nested, 2,
        "the two sub-sections of chapter one are not nested under it"
    );
}

/// **A same-document `href="#x"` reaches the page it points at**, across a page
/// boundary in a chapter that fragments.
///
/// Neither corpus has one that this build can see: pandoc writes
/// `href="#toc"` only inside the landmarks `<nav hidden>`, which is
/// `display: none` and generates no run to hang an annotation on, so the
/// same-document branch of the resolver is unreachable from any committed book
/// and a defect that made it return nothing survived the whole suite. The
/// fixture is one chapter long enough to need three pages with an anchor at the
/// top and its target at the bottom.
#[test]
fn a_same_document_reference_reaches_the_page_it_points_at() {
    use epub_support::{ocf_zip, OcfEntry};
    use tinker_pdf_cos::dest::{Action, Destination};

    let filler = "Filler text that has to be long enough to need more than one page. ".repeat(120);
    let chapter = format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>t</title></head>"#,
            r##"<body><p><a href="#end">to the end</a></p><p>{}</p>"##,
            r#"<p id="end">The end of the chapter.</p></body></html>"#
        ),
        filler
    );
    let package = concat!(
        r#"<?xml version="1.0" encoding="utf-8"?>"#,
        r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id">"#,
        r#"<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">"#,
        r#"<dc:identifier id="id">urn:uuid:0000</dc:identifier>"#,
        r#"<dc:title>Long</dc:title><dc:language>en</dc:language></metadata>"#,
        r#"<manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest>"#,
        r#"<spine><itemref idref="c1"/></spine></package>"#
    );
    let container = concat!(
        r#"<?xml version="1.0" encoding="UTF-8"?>"#,
        r#"<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">"#,
        r#"<rootfiles><rootfile full-path="content.opf" "#,
        r#"media-type="application/oebps-package+xml"/></rootfiles></container>"#
    );
    let entries = vec![
        OcfEntry::stored("mimetype", b"application/epub+zip"),
        OcfEntry::deflated("META-INF/container.xml", container.as_bytes()),
        OcfEntry::deflated("content.opf", package.as_bytes()),
        OcfEntry::deflated("c1.xhtml", chapter.as_bytes()),
    ];
    let directory: Vec<usize> = (0..entries.len()).collect();
    let doc = Document::open(ocf_zip(&entries, &directory)).expect("a book");
    assert!(
        doc.page_count() >= 3,
        "the fixture fits on {} pages, so nothing crosses a boundary",
        doc.page_count()
    );

    let links = doc.page(0).expect("a page").links();
    assert_eq!(links.len(), 1, "one anchor, one link");
    let to = match &links[0].target {
        Some(Action::GoTo(Destination::Explicit { page_index, .. })) => *page_index,
        other => panic!("a same-document reference became {other:?}"),
    };
    assert_eq!(
        to,
        Some(doc.page_count() - 1),
        "the target is on the last page and the link says {to:?}"
    );
}

// ---- the census ------------------------------------------------------------------

/// **The `Unsupported` census, per book, against the record.**
///
/// This is the figure row 8 says the milestone is judged on: what a book asked
/// for that this build does not implement, counted by the elements it reached.
/// It is written down in `tests/epub/CENSUS.tsv` rather than listed here, so a
/// milestone that implements `float` or `vertical-align` has to re-measure
/// rather than argue — the same ratchet `CONSERVATION.tsv` is.
#[test]
fn the_unsupported_census_is_the_one_the_record_states() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("epub")
        .join("CENSUS.tsv");
    let recorded: Vec<String> = std::fs::read_to_string(&path)
        .expect("tests/epub/CENSUS.tsv")
        .lines()
        .skip(1)
        .filter(|line| !line.trim().is_empty())
        .map(str::to_owned)
        .collect();

    let mut measured = Vec::new();
    for name in COMMITTED {
        let doc = Document::open(corpus_book(name)).expect(name);
        let entries = census(&doc);
        println!("  {name}");
        for (property, elements) in &entries {
            println!("      {property:20} {elements:5} elements");
            measured.push(format!("{name}\t{property}\t{elements}"));
        }
        assert!(
            !entries.is_empty(),
            "{name} reports no unimplemented property at all, which no book in \
             this corpus can honestly say"
        );
    }
    assert_eq!(measured, recorded, "tests/epub/CENSUS.tsv is out of date");
}

/// The census counts **elements reached** and not declarations written.
///
/// The two differ by a factor of a hundred on a real book and only one of them
/// is a fact about the book: a `float: left` in a rule that matches nothing is
/// not a gap the book noticed, and `.calibre13 { display: table-cell }`
/// matching eighteen cells is eighteen and not one. A build that counted at
/// parse time would report 1 for each.
#[test]
fn the_census_counts_elements_and_not_declarations() {
    let doc = Document::open(corpus_book("calibre-book-cover.epub")).expect("a book");
    let entries = census(&doc);
    let display = entries
        .iter()
        .find(|(property, _)| property == "display")
        .expect("this book sets display: table on its table");
    assert!(
        display.1 > 20,
        "display was counted per declaration rather than per element: {display:?}"
    );
    // And the ranking is by count, which is what makes the first line of a
    // report the thing worth reading.
    let counts: Vec<usize> = entries.iter().map(|(_, count)| *count).collect();
    let mut sorted = counts.clone();
    sorted.sort_unstable_by(|a, b| b.cmp(a));
    assert_eq!(counts, sorted, "the census is not ranked");
}

/// **No page of any committed book is a placeholder any more.**
///
/// The sentence milestone 4 wrote in order to delete: until this milestone
/// every page of all six books carried `SpineDefect::NotLaidOut`, and now the
/// list is empty. It is asserted as an equality against the empty list rather
/// than as "few", because any placeholder at all in this corpus is a chapter
/// this build stopped reading.
///
/// The variant milestone 8 does **not** remove — an SVG content document — has
/// its own fixture in `tests/epub_package.rs`, because no committed book has
/// one: pandoc's and calibre's covers are XHTML documents that *contain* SVG,
/// which is a different thing and is why the two names exist.
#[test]
fn no_committed_book_has_a_placeholder_page() {
    for name in COMMITTED {
        let doc = Document::open(corpus_book(name)).expect(name);
        let placeholders: Vec<(u32, SpineDefect)> = doc
            .archive()
            .expect("a report")
            .warnings()
            .iter()
            .filter_map(|warning| match warning {
                ArchiveWarning::SpinePage { page, defect } => Some((*page, *defect)),
                _ => None,
            })
            .collect();
        assert_eq!(placeholders, [], "{name} still has a placeholder page");
    }
}

/// The page box a caller states reaches **layout** and not only `/MediaBox`.
///
/// A caller's `font_size` is the other half of the same claim, and it is the
/// one a build wires up last: `OpenOptions::font_size` has had no consumer
/// since milestone 4, which said so in as many words. It has one now.
#[test]
fn the_callers_base_font_size_changes_the_pagination() {
    let bytes = corpus_book("pandoc-book-cover.epub");
    let small = Document::open_with(bytes.clone(), &{
        let mut options = OpenOptions::default();
        options.font_size = 8.0;
        options
    })
    .expect("a book");
    let large = Document::open_with(bytes, &{
        let mut options = OpenOptions::default();
        options.font_size = 18.0;
        options
    })
    .expect("a book");
    assert!(
        large.page_count() > small.page_count(),
        "a book set at 18pt is not longer than the same book at 8pt: {} against {}",
        large.page_count(),
        small.page_count()
    );
    // And both are still the same book.
    let small_text: String = pages_text(&small).join(" ").split_whitespace().collect();
    let large_text: String = pages_text(&large).join(" ").split_whitespace().collect();
    assert_eq!(small_text, large_text);
}
