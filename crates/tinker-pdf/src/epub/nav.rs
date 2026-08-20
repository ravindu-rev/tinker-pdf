//! The table of contents, from either of the two places a book keeps one (gap
//! 31, milestone 8).
//!
//! EPUB 3.3 §5.4 puts it in the **navigation document**: an XHTML content
//! document carrying `properties="nav"` in the manifest, whose `<nav
//! epub:type="toc">` holds an ordered list. EPUB 2 put it in an **NCX**, a
//! DAISY `application/x-dtbncx+xml` part named by `<spine toc="…">`. Both are
//! read here, and the reason both are is measurable rather than defensive:
//! **two of the six committed books have no navigation document at all** — one
//! calibre book has only an NCX, and one pandoc book ships a `nav.xhtml` its
//! manifest never marks as the nav — so a build that read only §5.4's shape
//! would give a third of this corpus no outline and would look correct doing
//! it.
//!
//! # What this module does not do
//!
//! It does not resolve an `href` to a page. A navigation entry names a
//! container-relative reference with an optional fragment, and turning that
//! into a page index needs the spine, every content document's element tree
//! and the pagination — none of which belongs to a file about parsing a list.
//! [`NavEntry::href`] comes out as written and [`super::synthesise`] resolves
//! it, the same way and through the same code path as an `<a href>` in the
//! body, so a cross-reference and a table-of-contents entry cannot disagree
//! about where a chapter is.

use tinker_pdf_xml::{Doctype, Event, Limits as XmlLimits, Source};

use super::xhtml::{Child, Dom};

/// The NCX namespace (`ncx` 2005-1, §1.3).
pub const NCX_NAMESPACE: &str = "http://www.daisy.org/z3986/2005/ncx/";

/// How deep an outline is read.
///
/// `DocumentBuilder::set_outline` refuses a tree deeper than
/// `tinker_pdf_cos::limits::MAX_NEST_DEPTH`, because that is where this
/// repository's **own reader** stops walking one — so a writer that produced a
/// deeper tree would produce a document it could not read back. Truncating
/// here rather than being refused there is the difference between a book with
/// a shallow table of contents and a book with none.
pub const MAX_NAV_DEPTH: usize = 8;

/// How many entries one level of an outline may hold.
///
/// The same argument one level along: `MAX_TREE_ENTRIES` is where the reader
/// stops walking a `/Next` chain, and a level past it would be written and not
/// read back.
pub const MAX_NAV_SIBLINGS: usize = 4_096;

/// One entry of a table of contents, before anything knows where it points.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavEntry {
    /// The text a reader shows.
    pub title: String,
    /// The reference as the book wrote it, fragment and all.
    ///
    /// `None` is a real shape and not a degraded one: §5.4.1 allows a `<span>`
    /// in place of an `<a>` for a heading that groups the entries beneath it,
    /// and 12.3.3 makes `/Dest` optional for exactly that case.
    pub href: Option<String>,
    /// Nested entries.
    pub children: Vec<NavEntry>,
}

/// The toc of a parsed navigation document (§5.4.1).
///
/// The `<nav>` carrying `epub:type="toc"` is the one §5.4.1.2 **requires**;
/// where a producer wrote none — pandoc's EPUB 2 output writes a bare `<div>`
/// with an `<ol>` in it — the first ordered list in the document is taken
/// instead, because a navigation document with a list in it and no `epub:type`
/// is still a navigation document and refusing it would lose a real book's
/// outline over an attribute.
#[must_use]
pub fn from_navigation_document(dom: &Dom) -> Vec<NavEntry> {
    let toc = dom
        .nodes
        .iter()
        .position(|node| {
            node.is_html()
                && node.name == "nav"
                && node
                    .attr("epub:type")
                    .is_some_and(|value| value.split_whitespace().any(|t| t == "toc"))
        })
        .or_else(|| {
            dom.nodes
                .iter()
                .position(|node| node.is_html() && node.name == "nav")
        });
    let list = match toc {
        Some(nav) => descendant_list(dom, nav),
        None => dom
            .nodes
            .iter()
            .position(|node| node.is_html() && node.name == "ol"),
    };
    match list {
        Some(list) => entries(dom, list, 0),
        None => Vec::new(),
    }
}

/// The first `<ol>` at or under an element, in document order.
fn descendant_list(dom: &Dom, at: usize) -> Option<usize> {
    (at..dom.nodes.len()).find(|index| dom.contains(at, *index) && dom.nodes[*index].name == "ol")
}

/// One `<ol>`'s `<li>` children, as entries.
fn entries(dom: &Dom, list: usize, depth: usize) -> Vec<NavEntry> {
    if depth >= MAX_NAV_DEPTH {
        return Vec::new();
    }
    let mut out = Vec::new();
    for child in &dom.nodes[list].children {
        let Child::Element(item) = child else {
            continue;
        };
        if dom.nodes[*item].name != "li" || out.len() >= MAX_NAV_SIBLINGS {
            continue;
        }
        let mut entry = NavEntry {
            title: String::new(),
            href: None,
            children: Vec::new(),
        };
        for inner in &dom.nodes[*item].children {
            let Child::Element(node) = inner else {
                continue;
            };
            match dom.nodes[*node].name.as_str() {
                // §5.4.1.2's two content models for an `<li>`: an `<a>` that
                // points somewhere, or a `<span>` that does not.
                "a" | "span" if entry.title.is_empty() => {
                    entry.title = text_of(dom, *node);
                    entry.href = dom.nodes[*node].attr("href").map(str::to_owned);
                }
                "ol" => entry.children = entries(dom, *node, depth + 1),
                _ => {}
            }
        }
        if entry.title.is_empty() && entry.href.is_none() && entry.children.is_empty() {
            continue;
        }
        out.push(entry);
    }
    out
}

/// Every character under an element, whitespace collapsed to single spaces.
///
/// An outline entry is a PDF text string and not a flow, so the collapsing is
/// the whole of `css-text-3` that applies to it — and it has to happen here
/// rather than being left to a viewer, because a title written across three
/// indented source lines would otherwise carry both newlines into the file.
fn text_of(dom: &Dom, at: usize) -> String {
    let mut raw = String::new();
    collect(dom, at, &mut raw);
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn collect(dom: &Dom, at: usize, out: &mut String) {
    for child in &dom.nodes[at].children {
        match child {
            Child::Text(text) => out.push_str(text),
            Child::Element(index) => collect(dom, *index, out),
        }
    }
}

/// An EPUB 2 NCX's `navMap`, as the same entries.
///
/// Read with the event reader directly rather than through
/// [`super::xhtml::read`], because an NCX is not a content document: it has no
/// stylesheets, no cascade and no box tree, and building an element tree for
/// it would be building a document model to read four element names out of.
#[must_use]
pub fn from_ncx(bytes: &[u8], limits: &XmlLimits) -> Vec<NavEntry> {
    let Ok(source) = Source::new(bytes) else {
        return Vec::new();
    };
    // An NCX carries the DTD-era doctype of `ncx-2005-1.dtd` — every one in
    // this corpus does — so the mode is the same relaxed one a content
    // document is read under and for the same reason.
    let reader = source.reader_with(limits, Doctype::SkipExternalId);

    // A stack of the `navPoint`s currently open, and the text buffer of
    // whichever `<text>` is open inside them.
    let mut stack: Vec<NavEntry> = Vec::new();
    let mut roots: Vec<NavEntry> = Vec::new();
    let mut in_map = false;
    let mut in_text = false;
    let mut text = String::new();

    for event in reader {
        let Ok(event) = event else {
            break;
        };
        match event {
            Event::Start(element) => match element.local() {
                "navMap" => in_map = true,
                "navPoint" if in_map && stack.len() < MAX_NAV_DEPTH => stack.push(NavEntry {
                    title: String::new(),
                    href: None,
                    children: Vec::new(),
                }),
                // **No `if !stack.is_empty()` here**, and its absence is the
                // injection matrix's finding. The `End` arm below has to test
                // the stack anyway — it needs a `navPoint` to put the title on
                // — so a guard here is the same rule enforced twice with only
                // one half reachable, and a defect that removed it survived
                // every test in the suite. A `<text>` outside a `navPoint`,
                // which is what `<docTitle>` holds, is now accumulated and
                // then dropped rather than not accumulated.
                "text" => {
                    in_text = true;
                    text.clear();
                }
                "content" if !stack.is_empty() => {
                    // `src` is required by the NCX DTD and is the reference a
                    // reading system follows; the `id` and `playOrder` beside
                    // it are not, because this build takes document order.
                    if let Some(src) = element.attribute(None, "src") {
                        if let Some(last) = stack.last_mut() {
                            if last.href.is_none() {
                                last.href = Some(src.to_owned());
                            }
                        }
                    }
                }
                _ => {}
            },
            Event::Text(chunk) | Event::Cdata(chunk) => {
                if in_text {
                    text.push_str(&chunk);
                }
            }
            Event::End(name) => match name.local() {
                "navMap" => in_map = false,
                "text" => {
                    if in_text {
                        if let Some(last) = stack.last_mut() {
                            if last.title.is_empty() {
                                last.title = text.split_whitespace().collect::<Vec<_>>().join(" ");
                            }
                        }
                        in_text = false;
                    }
                }
                "navPoint" => {
                    if let Some(done) = stack.pop() {
                        match stack.last_mut() {
                            Some(parent) if parent.children.len() < MAX_NAV_SIBLINGS => {
                                parent.children.push(done);
                            }
                            Some(_) => {}
                            None => {
                                if roots.len() < MAX_NAV_SIBLINGS {
                                    roots.push(done);
                                }
                            }
                        }
                    }
                }
                _ => {}
            },
            Event::Comment(_) | Event::Instruction { .. } => {}
        }
    }
    roots
}
