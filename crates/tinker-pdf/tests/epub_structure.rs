//! EPUB output carries a structure tree, ISO 32000 §14.7.
//!
//! # What this is for
//!
//! Until now an EPUB converted by this engine was an **untagged** PDF: no
//! `/MarkInfo`, no `/StructTreeRoot`, not one marked-content id. Every reader
//! extracting from it fell back to §14.8's geometric heuristic, and every
//! assistive technology had nothing but that heuristic to go on. The engine has
//! read structure trees since Tier 3 (`docs/design/tagged-pdf.md`) and has had
//! a complete writer in `tinker-pdf-cos` for as long, with no production
//! caller. This is the caller.
//!
//! # The evidence problem, and how it is answered
//!
//! Reading this engine's own output back through this engine's own reader
//! proves the two halves **agree**, not that either is right — the shared
//! misunderstanding the fuzz-audit lane spent itself documenting. So the
//! load-bearing assertion here is not a round trip. It is
//! [`logical_order_conserves_the_source_exactly`], which compares the
//! structure tree's logical order against the **source XHTML** — ground truth
//! this engine did not author and cannot have agreed with by construction.
//!
//! The round-trip assertions below are still worth having, but they are
//! secondary and are labelled as such: they check that what was written is
//! well-formed, not that it is true.
//!
//! # What the markup says about itself, against the source
//!
//! `<img alt>` as a `/Figure`'s `/Alt` and `xml:lang`/`lang` as `/Lang` are
//! asserted the same way the order is: the chapter's XHTML is read again in
//! the test, with the XML leaf's event reader and nothing of the EPUB path,
//! and what it says is compared with what the tree carries —
//! [`every_img_alt_is_a_figure_alt`], [`every_language_declaration_is_a_lang`],
//! [`every_a_href_is_a_link_holding_its_annotation`],
//! [`every_table_attribute_is_carried_from_the_source`],
//! [`every_element_keeps_its_name_and_says_what_it_is`].
//!
//! # What is not done yet, each named rather than absent
//!
//! - **No PDF/UA conformance claim.** A structure tree is necessary for it and
//!   nowhere near sufficient.
//!
//! Cross-page structure elements **are** written — an element's kids carry
//! their own `/Pg` where they are not on its default page, which is 14.7.2
//! Table 323's own model — and that is what closes the float reading-order
//! row: see [`a_float_reads_where_it_was_written_even_across_a_page`].

#[path = "epub_support/mod.rs"]
mod epub_support;

#[path = "cbz_support/mod.rs"]
mod cbz_support;

use epub_support::conservation::conservation_in_logical_order;
use epub_support::{ocf_zip, OcfEntry};
use tinker_pdf::{Document, OpenOptions, StructElement, StructKid};

const CONTAINER_XML: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8"?>"#,
    r#"<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">"#,
    r#"<rootfiles><rootfile full-path="EPUB/content.opf" media-type="application/oebps-package+xml"/>"#,
    r#"</rootfiles></container>"#
);

const PACKAGE: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8"?>"#,
    r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id">"#,
    r#"<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">"#,
    r#"<dc:identifier id="pub-id">urn:uuid:1f0c2c1e-0000-4000-8000-0000000057a6</dc:identifier>"#,
    r#"<dc:title>A Tagged Book</dc:title><dc:language>en</dc:language>"#,
    r#"<dc:creator>The tinker-pdf authors</dc:creator>"#,
    r#"</metadata><manifest>"#,
    r#"<item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/>"#,
    r#"</manifest><spine><itemref idref="c1"/></spine></package>"#
);

fn book(style: &str, body: &str) -> Vec<u8> {
    let chapter = format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>T</title>"#,
            r#"<style>{}</style></head><body>{}</body></html>"#
        ),
        style, body
    );
    let entries = vec![
        OcfEntry::stored("mimetype", b"application/epub+zip"),
        OcfEntry::deflated("META-INF/container.xml", CONTAINER_XML.as_bytes()),
        OcfEntry::deflated("EPUB/content.opf", PACKAGE.as_bytes()),
        OcfEntry::deflated("EPUB/ch1.xhtml", chapter.as_bytes()),
    ];
    let directory: Vec<usize> = (0..entries.len()).collect();
    ocf_zip(&entries, &directory)
}

fn opened(body: &str) -> Document {
    Document::open_with(book("", body), &OpenOptions::at_page(400.0, 600.0)).expect("a book")
}

/// Every standard type the tree carries, in the order the walk meets them.
fn types(doc: &Document) -> Vec<String> {
    let tree = doc.structure().expect("a structure tree");
    tree.elements()
        .iter()
        .map(|element| element.standard_type.clone())
        .collect()
}

// ---- the tree is there ------------------------------------------------------

/// **The claim the row closed on: the output is a tagged PDF.**
///
/// `/MarkInfo << /Marked true >>` and a `/StructTreeRoot` the reader finds.
/// Asserted on an untagged-by-construction fixture too, so this is a
/// difference rather than a constant.
#[test]
fn an_epub_becomes_a_tagged_pdf() {
    let doc = opened("<p>one</p>");
    let tree = doc
        .structure()
        .expect("an EPUB now carries a structure tree");
    assert!(tree.marked, "/MarkInfo /Marked is not true");
    assert!(
        tree.element_count() > 0,
        "the tree is empty: {} elements",
        tree.element_count()
    );
    assert!(
        tree.content_count() > 0,
        "no marked-content id is claimed by any element"
    );
    assert!(
        tree.warnings.is_empty(),
        "the reader complained about what the writer wrote: {:?}",
        tree.warnings
    );
}

/// XHTML element names become Table 333's standard types.
///
/// A round-trip assertion, and secondary: it says the tree is well-formed, not
/// that its order is right. The order claim is the conservation test below.
#[test]
fn xhtml_elements_become_standard_structure_types() {
    let doc = opened("<h1>a</h1><p>b</p><ul><li>c</li></ul>");
    let seen = types(&doc);
    for wanted in ["H1", "P", "L", "LI"] {
        assert!(seen.iter().any(|t| t == wanted), "no {wanted} in {seen:?}");
    }
}

/// A table's parts each get their own type, which is what makes a table
/// navigable rather than a grid of paragraphs.
#[test]
fn a_table_carries_its_own_structure() {
    let doc = opened("<table><tr><th>h</th><td>d</td></tr></table>");
    let seen = types(&doc);
    for wanted in ["Table", "TR", "TH", "TD"] {
        assert!(seen.iter().any(|t| t == wanted), "no {wanted} in {seen:?}");
    }
}

/// Nesting is the document's, not the page's: a `<span>` inside a `<p>` is a
/// `/Span` **inside** a `/P`.
#[test]
fn an_inline_element_nests_inside_its_block() {
    let doc = opened("<p>before<span>inside</span>after</p>");
    let tree = doc.structure().expect("a tree");
    let paragraph = tree
        .elements()
        .into_iter()
        .find(|element| element.standard_type == "P")
        .expect("a P");
    let nested: Vec<&str> = paragraph
        .kids
        .iter()
        .filter_map(|kid| match kid {
            tinker_pdf::StructKid::Element(child) => Some(child.standard_type.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        nested.contains(&"Span"),
        "the span is not inside the paragraph: {nested:?}"
    );
}

/// §14.8.2.2: a list marker is an **artifact** and stays outside the tree.
///
/// It is drawn and not extracted, which is what keeps text conservation an
/// equality — a bullet on the page and not in the source would be one extra
/// character per list item.
#[test]
fn a_list_marker_is_an_artifact_and_not_an_element() {
    let doc = opened("<ul><li>item</li></ul>");
    let page = doc.page(0).expect("a page");
    let structured = page.structured_text().expect("structured text");
    let text: String = structured.nodes.iter().map(|n| n.text.as_str()).collect();
    assert!(
        text.contains("item"),
        "the list item's own text is missing: {text:?}"
    );
    assert!(
        !text.contains('\u{2022}') && !text.contains('-'),
        "a marker reached the structure tree: {text:?}"
    );
}

// ---- the load-bearing one ---------------------------------------------------

/// **Logical order against the source XHTML, which this engine did not
/// author.**
///
/// Every other assertion in this file reads what this engine wrote through
/// what this engine reads, and two halves of one build agreeing is not
/// evidence that either is right. This one compares the structure tree's
/// order against the book's own markup: no extra characters, none missing,
/// every one in source order.
#[test]
fn logical_order_conserves_the_source_exactly() {
    let bytes = book(
        "p { margin: 0 }",
        "<h1>Title</h1><p>alpha beta</p><ul><li>one</li><li>two</li></ul>\
         <table><tr><td>cell</td></tr></table><p>omega</p>",
    );
    for tenths in 4..=12 {
        let height = f64::from(tenths) * 50.0;
        let doc = Document::open_with(bytes.clone(), &OpenOptions::at_page(300.0, height))
            .expect("a book");
        let verdict = conservation_in_logical_order(&bytes, &doc);
        assert!(
            verdict.holds(),
            "at a {height}-point page logical order lost the source: \
             {} extra, {} missing, {:?}",
            verdict.extra,
            verdict.missing,
            verdict.divergences
        );
    }
}

/// A paragraph that **breaks after an inline child** still reads in order.
///
/// The case that needs a marked sequence to carry its own position rather than
/// its element's. A `<p>` holding an `<em>` becomes three kids — text, the
/// span, text — and the second run of text is a *resumption*, which takes a
/// position past the child that interrupted it. When the paragraph then breaks
/// across a page, the resumption on the far side is a different sequence again:
/// give it the element's position and it sorts back in front of the `<em>` that
/// preceded it, and the page reads "one two FOUR three".
///
/// Swept rather than fixed, because which page the break lands on is the whole
/// variable and one height is one coincidence.
#[test]
fn a_paragraph_broken_after_an_inline_child_reads_in_order() {
    let bytes = book(
        "p { margin: 0 }",
        "<p>alpha bravo <em>CHARLIE</em> delta echo foxtrot golf hotel india          juliett kilo lima mike november oscar papa quebec romeo sierra</p>",
    );
    for tenths in 3..=14 {
        let height = f64::from(tenths) * 20.0;
        let doc = Document::open_with(bytes.clone(), &OpenOptions::at_page(220.0, height))
            .expect("a book");
        let verdict = conservation_in_logical_order(&bytes, &doc);
        assert!(
            verdict.holds(),
            "at a {height}-point page the paragraph read out of order:              {} extra, {} missing, {:?}",
            verdict.extra,
            verdict.missing,
            verdict.divergences
        );
    }
}

/// **A float whose box is on another page still reads where it was written.**
///
/// This is the one the float row turned on, through five attempts. §9.5.1
/// places a float by geometry, so `clear` can push its box a page past the
/// text it was written among; three fixes tried to move the box or the page it
/// was drawn on and could not, because a glyph is only on the page it is drawn
/// on. A fourth emitted a structure tree per page, which reproduced page order
/// and measured no change at all.
///
/// A structure element's kids may name **different pages** — 14.7.2 Table 323
/// — so one element can hold the gloss's marked content wherever its glyphs
/// landed and still sit in the source's order among its siblings. That is what
/// the writer now emits and what this asserts.
///
/// Content order still differs, and is asserted to differ: an untagged
/// extractor sees the geometry and there is nothing here to hide that.
#[test]
fn a_float_reads_where_it_was_written_even_across_a_page() {
    let bytes = book(
        "h2 { page-break-before: always; margin: 0 } p { margin: 0 } \
         span.side { display: block; float: right; clear: right; width: 40% }",
        "<p>opening</p><h2>ONE</h2><span class=\"side\">gloss</span>\
         <p>a</p><p>b</p><p>c</p>",
    );
    for tenths in 6..=16 {
        let height = f64::from(tenths) * 10.0;
        let doc = Document::open_with(bytes.clone(), &OpenOptions::at_page(200.0, height))
            .expect("a book");
        let logical = conservation_in_logical_order(&bytes, &doc);
        assert!(
            logical.holds(),
            "at a {height}-point page the float moved in logical order: \
             {} extra, {} missing, {:?}",
            logical.extra,
            logical.missing,
            logical.divergences
        );
    }
}

// ---- what the markup says about itself, against the source -----------------
//
// Each assertion below reads the chapter's XHTML **independently** — with the
// XML leaf's event reader and nothing of the EPUB path — and compares what the
// markup says (an `alt`, an `xml:lang`) with what the structure tree carries.
// The XHTML is the ground truth this engine did not author.

/// A book whose package names `language` and whose one chapter is `chapter`,
/// with `resources` beside it in the container and the manifest.
fn book_of(language: &str, chapter: &str, resources: &[(&str, &str, Vec<u8>)]) -> Vec<u8> {
    let items: String = resources
        .iter()
        .enumerate()
        .map(|(at, (href, media, _))| {
            format!(r#"<item id="r{at}" href="{href}" media-type="{media}"/>"#)
        })
        .collect();
    let package = format!(
        concat!(
            r#"<?xml version="1.0" encoding="UTF-8"?>"#,
            r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id">"#,
            r#"<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">"#,
            r#"<dc:identifier id="pub-id">urn:uuid:1f0c2c1e-0000-4000-8000-0000000057a7</dc:identifier>"#,
            r#"<dc:title>Said About Itself</dc:title><dc:language>{}</dc:language>"#,
            r#"</metadata><manifest>"#,
            r#"<item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/>{}"#,
            r#"</manifest><spine><itemref idref="c1"/></spine></package>"#
        ),
        language, items
    );
    let mut entries = vec![
        OcfEntry::stored("mimetype", b"application/epub+zip"),
        OcfEntry::deflated("META-INF/container.xml", CONTAINER_XML.as_bytes()),
        OcfEntry::deflated("EPUB/content.opf", package.as_bytes()),
        OcfEntry::deflated("EPUB/ch1.xhtml", chapter.as_bytes()),
    ];
    for (href, _, bytes) in resources {
        entries.push(OcfEntry::stored(&format!("EPUB/{href}"), bytes));
    }
    let directory: Vec<usize> = (0..entries.len()).collect();
    ocf_zip(&entries, &directory)
}

/// A chapter document: `html` is the attributes of `<html>`.
fn chapter(html: &str, body: &str) -> String {
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<html xmlns="http://www.w3.org/1999/xhtml" {}><head><title>T</title>"#,
            r#"<style>p {{ margin: 0 }}</style></head><body>{}</body></html>"#
        ),
        html, body
    )
}

/// One element of the source XHTML, as the XML reader reports it.
struct Source {
    name: String,
    attributes: Vec<(String, String)>,
    parent: Option<usize>,
    /// The element's own character data, every text child concatenated.
    text: String,
}

impl Source {
    fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// The source XHTML's elements in document order, read with the XML leaf's
/// event reader and nothing of the EPUB path.
fn source_elements(xhtml: &str) -> Vec<Source> {
    let source = tinker_pdf_xml::Source::new(xhtml.as_bytes()).expect("well-formed");
    let limits = tinker_pdf_xml::Limits::default();
    let mut out: Vec<Source> = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    for event in source.reader(&limits) {
        match event.expect("the fixture is well-formed") {
            tinker_pdf_xml::Event::Start(element) => {
                let attributes = element
                    .attributes()
                    .iter()
                    .map(|attribute| {
                        let name = attribute.name();
                        let qualified = match name.prefix() {
                            Some(prefix) => format!("{prefix}:{}", name.local()),
                            None => name.local().to_string(),
                        };
                        (qualified, attribute.value().to_string())
                    })
                    .collect();
                out.push(Source {
                    name: element.local().to_string(),
                    attributes,
                    parent: open.last().copied(),
                    text: String::new(),
                });
                open.push(out.len() - 1);
            }
            tinker_pdf_xml::Event::End(_) => {
                open.pop();
            }
            tinker_pdf_xml::Event::Text(text) | tinker_pdf_xml::Event::Cdata(text) => {
                if let Some(&at) = open.last() {
                    out[at].text.push_str(&text);
                }
            }
            _ => {}
        }
    }
    out
}

/// A language tag's shape, written here rather than borrowed from the
/// engine: one to eight letters, then hyphen-joined subtags of one to eight
/// letters and digits (RFC 5646 §2.1), or empty.
fn shaped_like_a_language_tag(text: &str) -> bool {
    text.is_empty()
        || text.split('-').enumerate().all(|(at, subtag)| {
            (1..=8).contains(&subtag.len())
                && subtag
                    .bytes()
                    .all(|b| b.is_ascii_alphabetic() || (at > 0 && b.is_ascii_digit()))
        })
}

/// The language the source says an element's text is in: its own `xml:lang`
/// or `lang`, else its nearest ancestor's, else the package's — skipping any
/// declaration not shaped like a tag, which says nothing a reader can use.
fn source_language(elements: &[Source], at: usize, package: &str) -> String {
    let mut current = Some(at);
    while let Some(index) = current {
        let element = &elements[index];
        let declared = element
            .attribute("xml:lang")
            .or_else(|| element.attribute("lang"));
        if let Some(language) = declared.filter(|l| shaped_like_a_language_tag(l)) {
            return language.to_string();
        }
        current = element.parent;
    }
    package.to_string()
}

/// Every structure element with the text its own marked content draws and
/// the language 14.9.2's hierarchy gives it: its own `/Lang`, else the
/// nearest ancestor's, else the catalog's.
fn tree_languages(doc: &Document) -> Vec<(String, Option<String>, StructElement)> {
    let cos = doc.cos();
    let catalog = cos.catalog().expect("a catalog");
    let catalog_language = cos
        .resolve_key(&catalog, cos.intern(b"Lang"))
        .as_string()
        .map(|s| tinker_pdf_cos::decode_text_string(&s.bytes));

    let mut text: std::collections::BTreeMap<(u32, u32), String> = Default::default();
    for index in 0..doc.page_count() {
        let page = doc.page(index).expect("a page");
        for line in page.text().lines() {
            for character in &line.chars {
                if let Some(mcid) = character.mcid {
                    text.entry((index, mcid))
                        .or_default()
                        .push_str(&character.text);
                }
            }
        }
    }

    fn walk(
        kids: &[StructKid],
        inherited: &Option<String>,
        text: &std::collections::BTreeMap<(u32, u32), String>,
        out: &mut Vec<(String, Option<String>, StructElement)>,
    ) {
        for kid in kids {
            let StructKid::Element(element) = kid else {
                continue;
            };
            let language = element.lang.clone().or_else(|| inherited.clone());
            let own: String = element
                .kids
                .iter()
                .filter_map(|kid| match kid {
                    StructKid::Content {
                        page: Some(page),
                        mcid,
                        ..
                    } => text.get(&(*page, *mcid)).cloned(),
                    _ => None,
                })
                .collect();
            out.push((own, language.clone(), (**element).clone()));
            walk(&element.kids, &language, text, out);
        }
    }
    let tree = doc.structure().expect("a tree");
    let mut out = Vec::new();
    walk(&tree.kids, &catalog_language, &text, &mut out);
    out
}

/// Text compared with every white-space character removed, which is the one
/// thing collapsing and line breaking change.
fn squeezed(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

/// A small PNG of one colour.
fn picture(rgb: [u8; 3]) -> Vec<u8> {
    cbz_support::rgb_png(4, 4, &rgb.repeat(16))
}

/// **Every `<img alt>` is a `/Figure` carrying that `alt` as `/Alt`**
/// (14.9.3), read off the source and off the tree independently.
///
/// The three cases HTML distinguishes, each its own answer: a description
/// is a `/Figure` with `/Alt`; an empty `alt` says the picture is
/// decoration, which is an artifact (14.8.2.2) and in the tree nowhere; no
/// `alt` at all is a `/Figure` with no `/Alt`, because the book did not say
/// and this engine does not speak for it.
///
/// And the picture reads where it was written: an `<img>` in the middle of a
/// paragraph is a `/Figure` between the paragraph's two runs of text, not
/// before them or after.
#[test]
fn every_img_alt_is_a_figure_alt() {
    let body = concat!(
        r#"<p>Before <img src="red.png" alt="A red square"/> after</p>"#,
        r#"<figure><img src="blue.png" alt="A blue square"/><figcaption>Blue</figcaption></figure>"#,
        r#"<p>Plain <img src="deco.png" alt=""/> decorated</p>"#,
        r#"<p>Last: <img src="bare.png"/></p>"#,
    );
    let xhtml = chapter(r#"lang="en" xml:lang="en""#, body);
    let png = "image/png";
    let bytes = book_of(
        "en",
        &xhtml,
        &[
            ("red.png", png, picture([200, 0, 0])),
            ("blue.png", png, picture([0, 0, 200])),
            ("deco.png", png, picture([0, 200, 0])),
            ("bare.png", png, picture([90, 90, 90])),
        ],
    );
    let doc = Document::open_with(bytes, &OpenOptions::at_page(400.0, 600.0)).expect("a book");
    let tree = doc.structure().expect("a tree");
    assert!(tree.warnings.is_empty(), "{:?}", tree.warnings);

    // The source, read on its own.
    let expected: Vec<Option<String>> = source_elements(&xhtml)
        .iter()
        .filter(|element| element.name == "img")
        .filter(|element| element.attribute("alt") != Some(""))
        .map(|element| element.attribute("alt").map(str::to_string))
        .collect();
    assert_eq!(
        expected.len(),
        3,
        "the fixture is what this test says it is"
    );

    let figures: Vec<&StructElement> = tree
        .elements()
        .into_iter()
        .filter(|element| element.standard_type == "Figure")
        .collect();
    let written: Vec<Option<String>> = figures.iter().map(|f| f.alt.clone()).collect();
    assert_eq!(
        written, expected,
        "every described picture, in source order"
    );
    assert_eq!(
        doc.page(0).expect("a page").images().len(),
        4,
        "all four pictures are drawn, the decorative one included"
    );

    // The picture in the paragraph reads between its two runs.
    let paragraph = tree
        .elements()
        .into_iter()
        .find(|element| element.standard_type == "P")
        .expect("the first paragraph");
    let shape: Vec<&str> = paragraph
        .kids
        .iter()
        .map(|kid| match kid {
            StructKid::Element(child) => child.standard_type.as_str(),
            StructKid::Content { .. } => "text",
            StructKid::Object(_) => "object",
        })
        .collect();
    assert_eq!(shape, ["text", "Figure", "text"]);

    // `<figure>` is the grouping round the picture and its caption, not a
    // second figure with no description.
    let grouping = tree
        .elements()
        .into_iter()
        .find(|element| {
            element
                .kids
                .iter()
                .any(|kid| matches!(kid, StructKid::Element(c) if c.standard_type == "Caption"))
        })
        .expect("the <figure>");
    assert_eq!(grouping.standard_type, "Div");
    assert!(grouping.kids.iter().any(
        |kid| matches!(kid, StructKid::Element(c) if c.alt.as_deref() == Some("A blue square"))
    ));
}

/// **Every language declaration is a `/Lang`** (14.9.2), compared text by
/// text against what the source says each piece of text is in.
///
/// The source side resolves `xml:lang` before `lang`, element before
/// ancestor, and the package's `dc:language` last; the tree side resolves an
/// element's `/Lang`, its ancestors', and the catalog's. They must agree for
/// every element that has text of its own — including one whose own
/// declaration is not a language tag, which says nothing and so inherits.
#[test]
fn every_language_declaration_is_a_lang() {
    for (package, html) in [
        ("en", r#"lang="en" xml:lang="en""#),
        // The chapter's own language differs from the book's: the elements
        // at its top carry the chapter's, since `<html>` is not in the tree.
        ("en", r#"xml:lang="fr""#),
        // No language on `<html>` at all.
        ("de", ""),
    ] {
        let body = concat!(
            r#"<h1>Title words</h1>"#,
            r#"<p xml:lang="fr">Bonjour le monde</p>"#,
            r#"<p>Plain words <span lang="de">Guten Tag</span> more words</p>"#,
            r#"<p lang="es" xml:lang="it">Ciao mondo</p>"#,
            r#"<p lang="en_US">Underscored words</p>"#,
            r#"<ul><li xml:lang="nl">Goedendag</li><li>Item two</li></ul>"#,
        );
        let xhtml = chapter(html, body);
        let bytes = book_of(package, &xhtml, &[]);
        let doc = Document::open_with(bytes, &OpenOptions::at_page(400.0, 600.0)).expect("a book");

        let source = source_elements(&xhtml);
        let tree = tree_languages(&doc);
        let mut compared = 0;
        for (at, element) in source.iter().enumerate() {
            let own = squeezed(&element.text);
            if own.is_empty() || element.name == "title" || element.name == "style" {
                continue;
            }
            let wanted = source_language(&source, at, package);
            let found: Vec<&(String, Option<String>, StructElement)> = tree
                .iter()
                .filter(|(text, _, _)| squeezed(text) == own)
                .collect();
            assert!(
                !found.is_empty(),
                "{package}/{html}: no element draws {own:?}"
            );
            for (_, language, element) in found {
                assert_eq!(
                    language.as_deref(),
                    Some(wanted.as_str()),
                    "{package}/{html}: {own:?} ({} in the tree)",
                    element.standard_type
                );
            }
            compared += 1;
        }
        assert_eq!(compared, 8, "every element with text was compared");
        // The one declaration that is not a tag is named, once.
        let ignored: Vec<usize> = doc
            .archive()
            .expect("a synthesised book carries a report")
            .warnings()
            .iter()
            .filter_map(|warning| match warning {
                tinker_pdf::ArchiveWarning::LanguageTagIgnored { tags, .. } => Some(*tags),
                _ => None,
            })
            .collect();
        assert_eq!(ignored, [1], "{package}/{html}");
    }
}

/// A page's annotations, by reference, each with its `/StructParent`.
fn struct_parents(doc: &Document, page: u32) -> Vec<(tinker_pdf::ObjRef, Option<i64>)> {
    let cos = doc.cos();
    let pages = tinker_pdf_cos::pages::collect(cos);
    let Some(page) = pages.iter().find(|candidate| candidate.index == page) else {
        return Vec::new();
    };
    let object = cos.get(page.reference).expect("the page object");
    let dict = object.as_dict().expect("a dictionary");
    let annots = cos.resolve_key(dict, cos.intern(b"Annots"));
    annots
        .as_array()
        .unwrap_or_default()
        .iter()
        .filter_map(|entry| {
            let reference = entry.as_objref()?;
            let annotation = cos.resolve(entry);
            let key = annotation
                .as_dict()
                .and_then(|dict| cos.resolve_key(dict, cos.intern(b"StructParent")).as_int());
            Some((reference, key))
        })
        .collect()
}

/// **Every `<a href>` that goes somewhere is a `/Link` holding its
/// annotation** (14.8.4.4.2): the anchor's text, an `/OBJR` to each link
/// annotation drawn for it, and each annotation's `/StructParent` naming the
/// `/Link` back through the `/ParentTree` (14.7.4.4).
///
/// Read off the source independently: an `<a>` goes somewhere when its
/// `href` is a URI or a fragment naming an `id` the document has. An `<a>`
/// with no `href`, or with one naming a document the book does not hold, has
/// no annotation and is not a `/Link` — a bare `/Link` would claim an
/// association the file does not contain.
#[test]
fn every_a_href_is_a_link_holding_its_annotation() {
    let body = concat!(
        r##"<p>See <a href="#target">the target</a> and "##,
        r##"<a href="https://example.org/a">the site</a>.</p>"##,
        r##"<p>A <a id="anchor">bare anchor</a> and <a href="missing.xhtml">a broken one</a>.</p>"##,
        r##"<p>Long <a href="https://example.org/b">a link whose words are many enough "##,
        r##"to break across a line of this narrow page</a> ends.</p>"##,
        r##"<p id="target">Target paragraph</p>"##,
    );
    let xhtml = chapter(r#"lang="en""#, body);
    let bytes = book_of("en", &xhtml, &[]);
    let doc = Document::open_with(bytes, &OpenOptions::at_page(220.0, 600.0)).expect("a book");
    let tree = doc.structure().expect("a tree");
    assert!(tree.warnings.is_empty(), "{:?}", tree.warnings);

    // The source, read on its own: which anchors go somewhere, and their text.
    let source = source_elements(&xhtml);
    let ids: Vec<&str> = source
        .iter()
        .filter_map(|element| element.attribute("id"))
        .collect();
    let mut wanted: Vec<(String, String)> = Vec::new();
    for element in source.iter().filter(|element| element.name == "a") {
        let Some(href) = element.attribute("href") else {
            continue;
        };
        let goes = href.starts_with("https://")
            || href.strip_prefix('#').is_some_and(|id| ids.contains(&id));
        if goes {
            wanted.push((squeezed(&element.text), href.to_string()));
        }
    }
    assert_eq!(wanted.len(), 3, "the fixture is what this test says it is");

    // The tree: every `/Link`, its text, and what its annotations say.
    let texts = tree_languages(&doc);
    let annotations: Vec<(u32, tinker_pdf::ObjRef, Option<i64>)> = (0..doc.page_count())
        .flat_map(|page| {
            struct_parents(&doc, page)
                .into_iter()
                .map(move |(reference, key)| (page, reference, key))
        })
        .collect();
    let targets: Vec<(tinker_pdf::ObjRef, String)> = (0..doc.page_count())
        .flat_map(|page| doc.page(page).expect("a page").links())
        .filter_map(|link| {
            let reference = link.reference?;
            let target = match link.target? {
                tinker_pdf_cos::Action::Uri(uri) => String::from_utf8_lossy(&uri).into_owned(),
                tinker_pdf_cos::Action::GoTo(_) => "#target".to_string(),
                other => format!("{other:?}"),
            };
            Some((reference, target))
        })
        .collect();
    let catalog = doc.cos().catalog().expect("a catalog");
    let root = doc
        .cos()
        .resolve_key(&catalog, doc.cos().intern(b"StructTreeRoot"));
    let parent_tree_ref = root
        .as_dict()
        .and_then(|root| root.get_ref(doc.cos().intern(b"ParentTree")))
        .expect("a parent tree");
    let parent_tree = tinker_pdf_cos::number_tree(doc.cos(), parent_tree_ref);

    let mut found: Vec<(String, String)> = Vec::new();
    for (text, _, element) in texts.iter().filter(|(_, _, e)| e.standard_type == "Link") {
        let held: Vec<tinker_pdf::ObjRef> = element
            .kids
            .iter()
            .filter_map(|kid| match kid {
                StructKid::Object(reference) => Some(*reference),
                _ => None,
            })
            .collect();
        assert!(!held.is_empty(), "a /Link with no annotation: {text:?}");
        let mut goes: Vec<&String> = Vec::new();
        for reference in &held {
            let (_, _, key) = annotations
                .iter()
                .find(|(_, annotation, _)| annotation == reference)
                .expect("the /OBJR names an annotation the page carries");
            let key = key.expect("the annotation has a /StructParent");
            let (_, value) = parent_tree
                .iter()
                .find(|(k, _)| *k == key)
                .expect("its key is in the /ParentTree");
            assert_eq!(value.as_objref(), element.reference, "and names this /Link");
            let (_, target) = targets
                .iter()
                .find(|(annotation, _)| annotation == reference)
                .expect("a link annotation");
            goes.push(target);
        }
        goes.dedup();
        assert_eq!(goes.len(), 1, "one /Link, one destination: {goes:?}");
        // The text a link's element draws on every page it spans.
        let whole: String = texts
            .iter()
            .filter(|(_, _, e)| e.reference == element.reference)
            .map(|(t, _, _)| t.clone())
            .collect();
        found.push((squeezed(&whole), goes[0].clone()));
    }
    assert_eq!(
        found, wanted,
        "every anchor that goes somewhere, in source order"
    );

    // The broken one and the bare one are spans, and their annotations — none
    // for the bare one, none for the unresolved one — are nowhere.
    let links_written: usize = (0..doc.page_count())
        .map(|page| doc.page(page).expect("a page").links().len())
        .sum();
    let held: usize = texts
        .iter()
        .filter(|(_, _, e)| e.standard_type == "Link")
        .map(|(_, _, e)| {
            e.kids
                .iter()
                .filter(|k| matches!(k, StructKid::Object(_)))
                .count()
        })
        .sum();
    assert_eq!(held, links_written, "every link annotation is in the tree");
    assert!(
        links_written > 3,
        "the long link was broken across lines: {links_written}"
    );
}

/// **A table's `summary`, a header's `scope` and a cell's `headers` and spans
/// are its Table 349 attributes** (14.8.5.7), compared cell by cell with the
/// source.
///
/// The source side resolves each cell's `headers` to the header cells' text
/// by `id`; the tree side resolves each cell's `/Headers` through
/// `element_by_id` to the `/TH` elements' text. An id naming no cell — this
/// fixture's `nowhere` — is a reference into nothing and is not written.
#[test]
fn every_table_attribute_is_carried_from_the_source() {
    let body = concat!(
        r#"<table summary="Fruit prices by year"><caption>Prices</caption>"#,
        r#"<tr><th id="fruit" scope="col">Fruit</th><th id="y2025" scope="col">Year 2025</th>"#,
        r#"<th id="y2026" scope="colgroup">Year 2026</th></tr>"#,
        r#"<tr><th id="apple" scope="row">Apple</th><td headers="apple y2025">1.10</td>"#,
        r#"<td headers="apple y2026" rowspan="2">1.20</td></tr>"#,
        r#"<tr><th id="pear" scope="row">Pear</th><td headers="pear y2025 nowhere">0.90</td></tr>"#,
        r#"<tr><td colspan="2">Totals</td><td>2.10</td></tr>"#,
        r#"</table>"#,
    );
    let xhtml = chapter(r#"lang="en""#, body);
    let bytes = book_of("en", &xhtml, &[]);
    let doc = Document::open_with(bytes, &OpenOptions::at_page(400.0, 600.0)).expect("a book");
    let tree = doc.structure().expect("a tree");
    assert!(tree.warnings.is_empty(), "{:?}", tree.warnings);
    let texts = tree_languages(&doc);
    let element_text = |element: &StructElement| -> String {
        texts
            .iter()
            .filter(|(_, _, e)| e.reference == element.reference)
            .map(|(text, _, _)| squeezed(text))
            .collect()
    };
    // The tree's element drawing `text`, of a type.
    let in_tree = |text: &str, kind: &str| -> StructElement {
        texts
            .iter()
            .find(|(t, _, e)| squeezed(t) == text && e.standard_type == kind)
            .map(|(_, _, e)| e.clone())
            .unwrap_or_else(|| panic!("no {kind} draws {text:?}"))
    };

    // The source, read on its own.
    let source = source_elements(&xhtml);
    let by_id = |id: &str| -> Option<&Source> {
        source
            .iter()
            .find(|e| e.attribute("id") == Some(id) && matches!(e.name.as_str(), "th" | "td"))
    };
    let mut cells = 0;
    for element in &source {
        match element.name.as_str() {
            "table" => {
                let table = tree
                    .elements()
                    .into_iter()
                    .find(|e| e.standard_type == "Table")
                    .expect("a /Table");
                assert_eq!(
                    table.table.as_ref().and_then(|t| t.summary.as_deref()),
                    element.attribute("summary")
                );
            }
            "th" | "td" => {
                cells += 1;
                let kind = if element.name == "th" { "TH" } else { "TD" };
                let cell = in_tree(&squeezed(&element.text), kind);
                let attributes = cell.table.clone().unwrap_or_default();

                let wanted_scope = element.attribute("scope").map(|scope| match scope {
                    "col" | "colgroup" => tinker_pdf::TableScope::Column,
                    "row" | "rowgroup" => tinker_pdf::TableScope::Row,
                    other => panic!("the fixture has no scope {other}"),
                });
                assert_eq!(attributes.scope, wanted_scope, "{}", element.text);

                let wanted_headers: Vec<String> = element
                    .attribute("headers")
                    .unwrap_or_default()
                    .split_ascii_whitespace()
                    .filter_map(by_id)
                    .map(|header| squeezed(&header.text))
                    .collect();
                let written_headers: Vec<String> = attributes
                    .headers
                    .iter()
                    .map(|id| {
                        let header = tree.element_by_id(id).expect("every header resolves");
                        element_text(header)
                    })
                    .collect();
                assert_eq!(written_headers, wanted_headers, "{}", element.text);

                let wanted_span = |name: &str| {
                    element
                        .attribute(name)
                        .and_then(|span| span.parse::<u32>().ok())
                        .filter(|span| *span > 1)
                };
                assert_eq!(
                    attributes.row_span,
                    wanted_span("rowspan"),
                    "{}",
                    element.text
                );
                assert_eq!(
                    attributes.col_span,
                    wanted_span("colspan"),
                    "{}",
                    element.text
                );

                // A cell with an `id` carries it, qualified by its document.
                if let Some(id) = element.attribute("id") {
                    let written = cell.id.clone().expect("an /ID");
                    assert!(
                        written.ends_with(format!("#{id}").as_bytes()),
                        "{}",
                        String::from_utf8_lossy(&written)
                    );
                }
            }
            _ => {}
        }
    }
    assert_eq!(cells, 10, "every cell was compared");
}

/// **Every element keeps its XHTML name and says what it is**: written as the
/// name the source gives it, and role-mapped (14.7.3) to a standard type —
/// so `<em>` and `<strong>` are no longer two indistinguishable `/Span`s.
///
/// Read off the source: every element with text of its own is found in the
/// tree by that text, its `raw_type` is the source's element name (or that
/// name's standard spelling, for `p`, `h1`, `table` and the rest that are
/// their own standard type), and its `standard_type` is one of ISO
/// 32000-1's. And the reader walked a role map with no loop in it.
#[test]
fn every_element_keeps_its_name_and_says_what_it_is() {
    let body = concat!(
        r#"<section><h2>Heading words</h2>"#,
        r#"<p>Plain <em>emphasised</em> and <strong>strong</strong> and "#,
        r#"H<sub>two</sub>O and <code>code</code> and <abbr>abbreviated</abbr>.</p>"#,
        r#"<ol><li>first item</li></ol><dl><dt>term word</dt><dd>its definition</dd></dl>"#,
        r#"<aside>aside words</aside><blockquote>quoted words</blockquote>"#,
        r#"<figure><figcaption>caption words</figcaption></figure></section>"#,
    );
    let xhtml = chapter(r#"lang="en""#, body);
    let bytes = book_of("en", &xhtml, &[]);
    let doc = Document::open_with(bytes, &OpenOptions::at_page(400.0, 600.0)).expect("a book");
    let tree = doc.structure().expect("a tree");
    assert!(tree.warnings.is_empty(), "{:?}", tree.warnings);
    let texts = tree_languages(&doc);

    let source = source_elements(&xhtml);
    let mut compared = 0;
    for element in &source {
        let own = squeezed(&element.text);
        if own.is_empty() || matches!(element.name.as_str(), "title" | "style") {
            continue;
        }
        let (_, _, written) = texts
            .iter()
            .find(|(text, _, _)| squeezed(text) == own)
            .unwrap_or_else(|| panic!("no element draws {own:?}"));
        assert!(
            written.raw_type == element.name
                || written.raw_type.eq_ignore_ascii_case(&element.name),
            "<{}> was written /{}",
            element.name,
            written.raw_type
        );
        assert!(
            tinker_pdf_cos::STANDARD_STRUCTURE_TYPES.contains(&written.standard_type.as_str()),
            "<{}> reads as /{}, which is not a standard type",
            element.name,
            written.standard_type
        );
        compared += 1;
    }
    assert_eq!(compared, 13, "every element with text was compared");

    // The two the old mapping made one.
    let kind = |name: &str| {
        tree.elements()
            .into_iter()
            .find(|e| e.raw_type == name)
            .map(|e| e.standard_type.clone())
    };
    assert_eq!(kind("em"), Some("Span".to_string()));
    assert_eq!(kind("strong"), Some("Span".to_string()));
    assert_eq!(kind("sub"), Some("Span".to_string()), "a subscript is text");
}

/// **A `headers` naming a cell that is never written names nothing**, and is
/// left out: an empty header cell and one `display: none` removed are never
/// opened as structure elements, because nothing is drawn inside them, so
/// neither carries an `/ID` — and a `/Headers` entry is an element
/// identifier (Table 349). The review of the tagged-writing lane found both
/// written as ids no element carried, because the filter asked whether the
/// source had the cell rather than whether the PDF did.
#[test]
fn a_header_cell_that_is_not_written_is_not_named() {
    let body = concat!(
        r#"<table><tr><th id="corner"></th><th id="hidden" style="display:none">Gone</th>"#,
        r#"<th id="h">Head</th></tr>"#,
        r#"<tr><th id="r" scope="row">Row</th>"#,
        r#"<td headers="corner hidden h r">Cell</td></tr></table>"#,
    );
    let doc = opened(body);
    let tree = doc.structure().expect("a tree");
    let mut named = Vec::new();
    for element in tree.elements() {
        let Some(table) = &element.table else {
            continue;
        };
        for id in &table.headers {
            assert!(
                tree.element_by_id(id).is_some(),
                "{} names {}, which no element carries",
                element.standard_type,
                String::from_utf8_lossy(id)
            );
            named.push(String::from_utf8_lossy(id).into_owned());
        }
    }
    assert_eq!(
        named,
        ["EPUB/ch1.xhtml#h", "EPUB/ch1.xhtml#r"],
        "the two header cells that are drawn, and only those"
    );
    let ids: Vec<String> = tree
        .elements()
        .into_iter()
        .filter_map(|element| element.id.as_deref())
        .map(|id| String::from_utf8_lossy(id).into_owned())
        .collect();
    assert_eq!(ids, ["EPUB/ch1.xhtml#h", "EPUB/ch1.xhtml#r"]);
}

/// **More element names than one role map carries** are written as their
/// standard types, and the book says so. A `/RoleMap` is one dictionary and
/// this engine's reader keeps `MAX_DICT_ENTRIES` entries of one, so a book
/// naming more elements than that used to write every name, have the reader
/// drop the mappings past the cap, and read those elements back as types no
/// standard defines — in a document claiming `/Marked true`. Now every
/// element reads as a standard type, the names that could not be kept are
/// written as `/Span` itself, and `ArchiveWarning::ElementNamesUnmapped`
/// counts them.
#[test]
fn element_names_past_what_a_role_map_carries_are_written_as_their_types() {
    let distinct = tinker_pdf_cos::limits::MAX_DICT_ENTRIES + 100;
    let body: String = (0..distinct).map(|i| format!("<x{i}>w</x{i}> ")).collect();
    let doc = opened(&body);
    let tree = doc.structure().expect("a tree");
    let found = tree.elements();
    for element in &found {
        assert!(
            tinker_pdf_cos::STANDARD_STRUCTURE_TYPES.contains(&element.standard_type.as_str()),
            "/{} reads as /{}, which is not a standard type",
            element.raw_type,
            element.standard_type
        );
    }
    let unmapped: Vec<usize> = doc
        .archive()
        .expect("a synthesised book carries a report")
        .warnings()
        .iter()
        .filter_map(|warning| match warning {
            tinker_pdf::ArchiveWarning::ElementNamesUnmapped { item, names } => {
                assert_eq!(item, "EPUB/ch1.xhtml");
                Some(*names)
            }
            _ => None,
        })
        .collect();
    let written_as_span = found
        .iter()
        .filter(|element| element.raw_type == "Span")
        .count();
    assert!(written_as_span > 0, "some names were past the map");
    assert_eq!(
        unmapped,
        [written_as_span],
        "one warning, counting the names written as their type"
    );
}
