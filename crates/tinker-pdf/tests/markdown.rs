//! Markdown onto the HTML path (tier 5's formats row): `Document::open_markdown`
//! and `DocumentBuilder::from_markdown`.
//!
//! The reader itself is held to CommonMark 0.31.2's 652 published examples by
//! `commonmark_spec.rs`, which needs the fetched `spec.txt` and skips without
//! it. This file is what runs on every `cargo test`: a handful of reader
//! answers worked out here from the specification's rules, and the claims the
//! document path makes on top of the reader — that a Markdown document is laid
//! out **exactly** as the XHTML it translates to, that what is not CommonMark
//! on the way is named, and that the nesting cap fires.

mod render_support;

use std::sync::Arc;

use render_support::{curvy_font, ink};
use tinker_pdf::markdown::{to_html, MAX_MARKDOWN_NESTING};
use tinker_pdf::standalone::TranslationDefect;
use tinker_pdf::{
    ArchiveWarning, Bitmap, Document, DocumentBuilder, FromHtml, OpenError, OpenOptions, PageBox,
    RenderOptions, SimpleFontProvider,
};

/// `document` with a face to draw its text in.
///
/// A document that embeds no face draws **none** of its text without one — it
/// extracts perfectly and renders `UnreadableFont` and a blank page — so two
/// pages compared without it are two blank pages, equal whatever was laid out
/// on them. The face is `render_support`'s synthetic one, attached after
/// pagination: the line breaks are the ones `open` made from the built-in
/// metrics, on both sides of a comparison, and only the glyphs are its.
fn drawn(document: Document) -> Document {
    document.with_fonts(Arc::new(SimpleFontProvider::new(curvy_font())))
}

/// Pixels that are not white: what says a comparison compared something.
const LEAST_INK: usize = 200;

fn render(document: &Document, page: u32) -> Bitmap {
    document
        .page(page)
        .expect("a page")
        .render(&RenderOptions::default())
}

fn warnings(document: &Document) -> Vec<ArchiveWarning> {
    document
        .archive()
        .expect("a synthesised document has a report")
        .warnings()
        .to_vec()
}

fn text(document: &Document) -> String {
    (0..document.page_count())
        .map(|at| document.page(at).expect("a page").text().plain_text())
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

const NOTE: &str = "# Release notes\n\
\n\
The *third* release, with **two** fixes and `one` change.\n\
\n\
- a tight list\n\
- of two items\n\
\n\
1. ordered\n\
2. and numbered\n\
\n\
> quoted, with a [link][home].\n\
\n\
```rust\n\
fn main() {}\n\
```\n\
\n\
[home]: https://example.org/home \"Home\"\n";

// ---- the reader's answers, worked out from the rules -------------------------

/// **A tight list sets its items without paragraphs and a loose one with
/// them** (§5.3): the blank line between the items is the whole difference.
#[test]
fn a_blank_line_between_items_is_what_makes_a_list_loose() {
    assert_eq!(
        to_html("- one\n- two\n"),
        "<ul>\n<li>one</li>\n<li>two</li>\n</ul>\n"
    );
    assert_eq!(
        to_html("- one\n\n- two\n"),
        "<ul>\n<li>\n<p>one</p>\n</li>\n<li>\n<p>two</p>\n</li>\n</ul>\n"
    );
}

/// **A reference definition is not text and the reference finds it in any
/// case** (§4.7, §6.3): the label matches case-insensitively and collapses
/// white space.
#[test]
fn a_reference_link_finds_its_definition_whatever_its_case() {
    assert_eq!(
        to_html("See [The  Home][the\nHOME].\n\n[THE home]: /h \"T\"\n"),
        "<p>See <a href=\"/h\" title=\"T\">The  Home</a>.</p>\n"
    );
    // The second label is the one looked up, not the text.
    assert_eq!(
        to_html("See [The Home][HOME].\n\n[the home]: /h\n"),
        "<p>See [The Home][HOME].</p>\n"
    );
}

/// **The rule of three** (§6.2, rules 9 and 10): `*a**b*` cannot close the
/// first run with the second, because the run that can both open and close is
/// two long and the sum of the lengths is a multiple of three.
#[test]
fn emphasis_follows_the_rule_of_three() {
    assert_eq!(to_html("*a**b*\n"), "<p><em>a**b</em></p>\n");
    assert_eq!(
        to_html("**strong *and em* inside**\n"),
        "<p><strong>strong <em>and em</em> inside</strong></p>\n"
    );
}

/// **No links in links** (§6.3), and the second time too: once a link has
/// closed, the `[` still open around it are dead, and a later `[` that wraps
/// a later link is just as dead. The reader keeps a watermark of the brackets
/// it has already deactivated so a thousand links after a thousand `[` are not
/// a million steps, and this is the shape that watermark has to come down for
/// — a bracket pushed after the stack has shrunk below it.
#[test]
fn a_link_inside_a_link_is_not_one_even_after_an_earlier_one() {
    assert_eq!(
        to_html("[x [y](u)] [p [q](v)](w)\n"),
        "<p>[x <a href=\"u\">y</a>] [p <a href=\"v\">q</a>](w)</p>\n"
    );
}

/// **A code span strips one space from each end only when both are there and
/// the span is not all spaces** (§6.1), and nothing inside it is markup.
#[test]
fn a_code_span_is_literal_and_strips_one_space_each_side() {
    assert_eq!(
        to_html("`` *x* `` and `  `\n"),
        "<p><code>*x*</code> and <code>  </code></p>\n"
    );
}

/// **A tab is four columns to the block structure and itself in content**
/// (§2.2): a tab after `>` is the optional space and three columns of an
/// indented code block's four, so this is a paragraph, not code.
#[test]
fn a_tab_counts_as_columns_for_structure() {
    assert_eq!(
        to_html(">\tquoted\n"),
        "<blockquote>\n<p>quoted</p>\n</blockquote>\n"
    );
    assert_eq!(
        to_html("    \tcode\n"),
        "<pre><code>\tcode\n</code></pre>\n"
    );
}

/// **A named reference resolves from XHTML 1.0's sets, a numeric one from its
/// number, and a name in neither stays literal** — which is where this reader
/// parts from CommonMark's full list, by name.
#[test]
fn entity_references_resolve_from_the_xhtml_sets() {
    assert_eq!(
        to_html("&copy; &#x263A; &#0; &HilbertSpace;\n"),
        "<p>\u{a9} \u{263a} \u{fffd} &amp;HilbertSpace;</p>\n"
    );
}

// ---- the document path -------------------------------------------------------

/// **A Markdown document opens, every word of it on its pages, its first
/// heading as its title, and nothing tolerated.**
#[test]
fn a_markdown_document_opens_with_its_words_and_its_title() {
    let document =
        Document::open_markdown(NOTE.as_bytes().to_vec(), &OpenOptions::default()).expect("opens");
    let words = text(&document);
    for expected in [
        "Release notes",
        "third release",
        "a tight list",
        "and numbered",
        "quoted, with a link",
        "fn main() {}",
    ] {
        assert!(words.contains(expected), "{expected:?} is not in {words:?}");
    }
    assert!(!words.contains("[home]"), "the definition reached the page");
    assert_eq!(document.metadata().title.as_deref(), Some("Release notes"));
    assert!(warnings(&document).is_empty(), "{:?}", warnings(&document));
}

/// **The title is the first heading's text as the page shows it**: a link's
/// text and an image's description, and none of the brackets the link pass
/// took back out of the heading. The heading's inlines are a tree whose arena
/// still holds the `[` and `![` text nodes the bracket pass unlinked when it
/// made the link, so reading the arena in push order titled `# [foo](bar) baz`
/// "[foo baz".
#[test]
fn the_title_is_the_first_headings_text_with_its_links_and_images_resolved() {
    for (source, title) in [
        ("# [foo](bar) baz\n", "foo baz"),
        ("# ![alt](x.png) t\n", "alt t"),
        ("# [x][r]\n\n[r]: /u\n", "x"),
        ("# *a* [**b** `c`](u) d\n", "a b c d"),
        // Brackets that made no link are the heading's text.
        (
            "# [not a link] and ![nor this\n",
            "[not a link] and ![nor this",
        ),
    ] {
        let document = Document::open_markdown(source.as_bytes().to_vec(), &OpenOptions::default())
            .expect("opens");
        assert_eq!(
            document.metadata().title.as_deref(),
            Some(title),
            "{source:?}"
        );
    }
}

/// **A hard line break is a line break on the page** (CommonMark 0.31.2
/// §6.7): a line ending after two spaces or a backslash is a `<br />`, which
/// HTML's rendering makes a newline (§15.3.4), so `first` and `second` are
/// read back as two lines; and a soft line break (§6.8) is a space, so the
/// same words written without either are one. Before `<br>` set a break,
/// all three read back as one line.
#[test]
fn a_hard_line_break_is_a_line_break_on_the_page() {
    let lines = |source: &str| -> Vec<String> {
        let document = Document::open_markdown(source.as_bytes().to_vec(), &OpenOptions::default())
            .expect("opens");
        let text = document.page(0).expect("a page").text();
        text.lines().iter().map(|line| line.text.clone()).collect()
    };
    assert_eq!(lines("first  \nsecond\n"), ["first", "second"]);
    assert_eq!(lines("first\\\nsecond\n"), ["first", "second"]);
    assert_eq!(lines("first\nsecond\n"), ["first second"]);
}

/// **A Markdown document is the XHTML it translates to, pixel for pixel**:
/// `from_markdown` against `from_html` handed the same reader's HTML in an
/// XHTML document. One cascade, one layout, one painter — the translation is
/// the only step Markdown adds.
#[test]
fn a_markdown_document_is_the_xhtml_it_translates_to() {
    let page = PageBox::new(400.0, 500.0);
    let sheet = "h1 { color: #036 } code { background-color: #eee }";
    let (from_markdown, report) =
        DocumentBuilder::from_markdown(NOTE, sheet, page).expect("lays out");
    let xhtml = format!(
        "<html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>Release notes</title></head>\
         <body>\n{}</body></html>",
        to_html(NOTE)
    );
    let (from_html, _) = DocumentBuilder::from_html(xhtml, sheet, page).expect("lays out");
    let a = drawn(Document::open(from_markdown.finish()).expect("opens"));
    let b = drawn(Document::open(from_html.finish()).expect("opens"));
    assert_eq!(a.page_count(), b.page_count());
    assert_eq!(a.page_count() as usize, report.pages());
    for at in 0..a.page_count() {
        let (left, right) = (render(&a, at), render(&b, at));
        assert!(ink(&left) >= LEAST_INK, "page {at} drew no text to compare");
        assert!(left.data == right.data, "page {at} differs");
    }
}

/// **Raw HTML is set as the text it is, and counted**: a block and two inline
/// tags are three, and the tags are on the page as characters rather than
/// lost — a `<div>` that is not well-formed XML would otherwise have stopped
/// the reader and taken the rest of the document with it.
#[test]
fn raw_html_is_set_as_text_and_counted() {
    let source = "<div class=\"x\">\nunclosed\n\nand <span>inline</span> tags\n";
    let document = Document::open_markdown(source.as_bytes().to_vec(), &OpenOptions::default())
        .expect("opens");
    assert_eq!(
        warnings(&document),
        [ArchiveWarning::Translation {
            item: String::new(),
            defect: TranslationDefect::RawHtmlAsText,
            count: 3
        }]
    );
    let words = text(&document);
    assert!(words.contains("<div class=\"x\"> unclosed"), "{words:?}");
    assert!(words.contains("and <span>inline</span> tags"), "{words:?}");
}

/// **Bytes that are not UTF-8 are read as U+FFFD and counted.**
#[test]
fn bytes_that_are_not_utf_8_are_counted() {
    let document =
        Document::open_markdown(b"caf\xE9 and na\xEFve\n".to_vec(), &OpenOptions::default())
            .expect("opens");
    // The replacement character is then a character the standard 14 have no
    // glyph for, which is the second warning and a true one.
    assert_eq!(
        warnings(&document),
        [
            ArchiveWarning::Translation {
                item: String::new(),
                defect: TranslationDefect::NotUtf8,
                count: 2
            },
            ArchiveWarning::UncoveredCharacters { characters: 2 }
        ]
    );
    assert_eq!(
        Document::open_markdown(Vec::new(), &OpenOptions::default()).err(),
        Some(OpenError::Empty)
    );
}

/// **`MAX_MARKDOWN_REFERENCE_BYTES` fires**: a definition whose destination is
/// two thousand bytes, used two hundred times, copies its destination while the
/// budget lasts — the larger of the cap and the document's length — and the
/// uses after that read as the text they are written as, counted.
#[test]
fn reference_links_past_the_copy_budget_read_as_text_and_are_counted() {
    use tinker_pdf::markdown::MAX_MARKDOWN_REFERENCE_BYTES;

    let destination = format!("/{}", "d".repeat(1_999));
    let source = format!("[a]: {destination}\n\n{}\n", "[a] ".repeat(200));
    assert!(
        source.len() < MAX_MARKDOWN_REFERENCE_BYTES,
        "the cap is the budget"
    );
    let resolved = MAX_MARKDOWN_REFERENCE_BYTES / destination.len();
    let html = to_html(&source);
    assert_eq!(html.matches("<a href=").count(), resolved);
    assert_eq!(html.matches("[a]").count(), 200 - resolved);

    let (_, defects) = tinker_pdf::markdown::to_xhtml(&source);
    assert_eq!(
        defects,
        [(TranslationDefect::ReferenceBudgetSpent, 200 - resolved)]
    );
}

/// **`MAX_MARKDOWN_NESTING` fires**: block quotes nested past it are read as
/// the text they then are, and counted, and the document still opens.
#[test]
fn a_container_past_the_nesting_cap_is_read_as_text_and_counted() {
    let deep = format!("{} deep\n", ">".repeat(MAX_MARKDOWN_NESTING + 5));
    let html = to_html(&deep);
    assert_eq!(
        html.matches("<blockquote>").count(),
        MAX_MARKDOWN_NESTING - 1,
        "the document is one container and the quotes are the rest"
    );
    let document =
        Document::open_markdown(deep.into_bytes(), &OpenOptions::default()).expect("opens");
    assert!(
        warnings(&document).iter().any(|w| matches!(
            w,
            ArchiveWarning::Translation {
                defect: TranslationDefect::NestingTooDeep,
                ..
            }
        )),
        "{:?}",
        warnings(&document)
    );
    assert!(text(&document).contains("deep"));
}

/// **Inline nesting is held under the depth the XML reader takes, so nothing
/// after it is lost.** CommonMark nests emphasis as deep as its delimiters
/// go, and three hundred `<em>` around one word is three hundred elements: the
/// XML reader stops at 256, and every block after the nest went with it, with
/// `Markup(Truncated)` and no translation defect naming why. The document path
/// now sets an inline that would nest past the bound without its element — its
/// text kept — and counts it as `NestingTooDeep`; `to_html`, held to the
/// specification, still nests all three hundred. The second case is the same
/// nest inside containers at `MAX_MARKDOWN_NESTING`, the deepest blocks can go.
#[test]
fn inline_nesting_past_what_the_reader_takes_keeps_the_rest_of_the_document() {
    let nest = format!("{}x{}", "*a ".repeat(300), " a*".repeat(300));
    let quotes = ">".repeat(MAX_MARKDOWN_NESTING - 1);
    // Lists nested to the cap, each a list and an item, the nest in the last.
    let levels = (MAX_MARKDOWN_NESTING - 1) / 2;
    let lists: String = (0..levels)
        .map(|i| format!("{}- x\n", "  ".repeat(i)))
        .collect();
    for source in [
        format!("{nest}\n\nafter the nest\n"),
        format!("{quotes} {nest}\n\nafter the nest\n"),
        format!("{lists}{}- {nest}\n\nafter the nest\n", "  ".repeat(levels)),
    ] {
        assert_eq!(to_html(&source).matches("<em>").count(), 300);
        let document =
            Document::open_markdown(source.into_bytes(), &OpenOptions::default()).expect("opens");
        let words = text(&document);
        assert!(words.contains("after the nest"), "{words:?}");
        assert!(words.contains("a a x a a"), "the nest's own text is kept");
        let found = warnings(&document);
        assert!(
            found.iter().any(|w| matches!(
                w,
                ArchiveWarning::Translation {
                    defect: TranslationDefect::NestingTooDeep,
                    ..
                }
            )),
            "{found:?}"
        );
        // Ninety-nine quotes' margins leave lines too narrow for a word, which
        // the layout says (`LineOverflowed`) and is not a stop. A stop is the
        // reader truncating or the chapter becoming a placeholder.
        assert!(
            !found.iter().any(|w| matches!(
                w,
                ArchiveWarning::Markup { .. } | ArchiveWarning::SpinePage { .. }
            )),
            "the reader or the layout stopped: {found:?}"
        );
    }
    // The bound is on depth and not on count: three hundred emphases side by
    // side, and as many quotes each holding one, keep every element.
    for source in ["*a* ".repeat(300), "> *a*\n\n".repeat(300)] {
        let (xhtml, defects) = tinker_pdf::markdown::to_xhtml(&source);
        assert_eq!(xhtml.matches("<em>").count(), 300);
        assert_eq!(defects, []);
    }
}
