//! The structure tree an EPUB's pages are tagged into (ISO 32000-1 14.7,
//! 14.8).
//!
//! [`super::paint`] draws a laid-out page; this decides **what each thing it
//! draws is** in the document's logical structure. They are separate files
//! because they answer separate questions — where the ink goes and what the
//! ink means — and because the second is a statement about the *source*
//! document: every run carries the index of the XHTML element that wrote it,
//! so the tree built here is the book's own element tree and not a
//! description of the page.
//!
//! What each XHTML element says about itself reaches the tree as 14.9's
//! properties: `<img alt>` is a `/Figure`'s `/Alt`, and `xml:lang`/`lang` is
//! `/Lang`.
//!
//! Feature documentation: `docs/features/epub.md`, "Tagged output".

use std::collections::{BTreeMap, BTreeSet};

use tinker_pdf_cos::build::{DocumentBuilder, PageBuilder, TableAttributes, TableScope, Tag};
use tinker_pdf_cos::is_language_tag;
use tinker_pdf_layout::{Page as LayoutPage, ReplacedFragment, TextRun};

use super::paint::{artifact_or_run, draw_replaced, Effects, Fonts, Frame, OnPage};
use super::xhtml::Dom;

/// What one content document's pages are tagged from.
pub(crate) struct Tagging<'a> {
    /// The content document's element tree.
    pub dom: &'a Dom,
    /// **A base per content document.** Both an element index and a
    /// reading-order stamp restart at every spine item, so two chapters would
    /// otherwise name the same element and sort into each other.
    pub chapter: u64,
    /// The language the catalog's `/Lang` states for the whole document, when
    /// it states one.
    pub document_language: Option<&'a str>,
    /// Each picture this content document draws, with the position it reads
    /// at. See [`figure_orders`].
    pub figures: &'a Figures,
    /// Every `<a>` that holds a link annotation, by element. Each is a `/Link`
    /// (14.8.4.4.2) whose `/OBJR`s `PageBuilder::link_for` attaches by key.
    pub links: &'a BTreeSet<usize>,
    /// The content document's container path, which qualifies its `id`s: a
    /// structure element's `/ID` is unique in the **document** (14.7.2), and
    /// two chapters may both say `id="h1"`.
    pub path: &'a str,
    /// Every table cell carrying an `id`, by that id. See [`table_cells`].
    pub cells: &'a BTreeMap<String, usize>,
    /// The element names written as themselves and role-mapped. See
    /// [`register_roles`].
    pub roles: &'a BTreeSet<String>,
}

/// Every `<th>` and `<td>` of a content document carrying an `id`, by that
/// id, the first element winning where a document repeats one — which is
/// the set a cell's `headers` may name and the cells that are written with an
/// `/ID`.
pub(crate) fn table_cells(dom: &Dom) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for (at, node) in dom.nodes.iter().enumerate() {
        if !node.is_html() || !matches!(node.name.as_str(), "th" | "td") {
            continue;
        }
        if let Some(id) = node.id.as_deref().filter(|id| !id.is_empty()) {
            out.entry(id.to_string()).or_insert(at);
        }
    }
    out
}

/// Whether a link rectangle is one `PageBuilder::link_for` will write: finite,
/// and enclosing an area. An `<a>` whose every rectangle is refused holds no
/// annotation, and so is not a `/Link`.
pub(crate) fn is_link_rect(rect: (f64, f64, f64, f64)) -> bool {
    let (x0, y0, x1, y1) = rect;
    [x0, y0, x1, y1].iter().all(|v| v.is_finite()) && x0 != x1 && y0 != y1
}

/// Where each picture of a content document reads, both ways round.
#[derive(Default)]
pub(crate) struct Figures {
    /// By element.
    by_anchor: BTreeMap<u32, u64>,
    /// By position, ascending, for "is there a picture between these two
    /// runs".
    by_order: Vec<(u64, u32)>,
}

impl Figures {
    /// Whether a picture inside `element` reads strictly between `after` and
    /// `before` (both [`Tagging::order`] positions without the chapter's
    /// base).
    fn between(&self, dom: &Dom, element: usize, after: u64, before: u64) -> bool {
        let start = self.by_order.partition_point(|(order, _)| *order <= after);
        self.by_order[start..]
            .iter()
            .take_while(|(order, _)| *order < before)
            .any(|(_, anchor)| is_descendant(dom, *anchor as usize, element))
    }
}

/// Whether `node` is `ancestor` or below it. Parents precede their children,
/// so the walk up ends.
fn is_descendant(dom: &Dom, node: usize, ancestor: usize) -> bool {
    let mut at = Some(node);
    while let Some(index) = at {
        if index == ancestor {
            return true;
        }
        if index < ancestor {
            return false;
        }
        at = dom.nodes.get(index).and_then(|n| n.parent);
    }
    false
}

impl Tagging<'_> {
    /// The key of an element: what makes its halves on two pages one element,
    /// and what a link annotation names its `/Link` by.
    pub(crate) fn key(&self, element: usize) -> u64 {
        self.chapter.saturating_add(element as u64)
    }

    /// A run's reading position, **doubled**, so that a picture can be given
    /// the odd position between the run before it and the run after it (see
    /// [`figure_orders`]). Only the relative order of these numbers reaches
    /// the file, so the scale is free.
    fn order(&self, run: &TextRun) -> u64 {
        self.chapter
            .saturating_add((run.order as u64).saturating_mul(2))
    }

    /// The language declared on the content document's own `<body>` or
    /// `<html>` — the nearer one — which the elements directly under the
    /// body inherit and the tree has no node to carry.
    fn root_language(&self) -> Option<&str> {
        let body = self
            .dom
            .body()
            .and_then(|at| well_formed_language(self.dom, at));
        body.or_else(|| {
            self.dom
                .root
                .and_then(|at| well_formed_language(self.dom, at))
        })
    }

    /// The structure element `element` opens: its type, keyed so that its
    /// halves on two pages merge, and the 14.9 properties its markup states.
    ///
    /// `level` is its depth below `<body>`: an element at the top inherits the
    /// content document's own language, which `<body>` and `<html>` cannot
    /// carry into the tree because they are not in it.
    fn tag(&self, element: usize, level: usize, order: u64) -> Tag {
        let name = self
            .dom
            .nodes
            .get(element)
            .map_or("", |node| node.name.as_str());
        // An `<a>` holding an annotation is a `/Link`; one holding none — no
        // `href`, or one this build could not resolve — is what any other
        // inline element is.
        let kind = if self.links.contains(&element) {
            "Link"
        } else if self.roles.contains(name) {
            name
        } else {
            structure_type(name)
        };
        let mut tag = Tag::new(kind.as_bytes()).keyed(self.key(element), order);
        if let Some(language) = self.language(element, level) {
            tag = tag.lang(language);
        }
        self.table(tag, element)
    }

    /// A cell's identifier and a table's or cell's Table 349 attributes, from
    /// the HTML attributes that say the same things: `id`, `headers`, `scope`,
    /// `colspan`, `rowspan` and `summary`.
    fn table(&self, mut tag: Tag, element: usize) -> Tag {
        let Some(node) = self.dom.nodes.get(element).filter(|node| node.is_html()) else {
            return tag;
        };
        let mut attributes = TableAttributes::default();
        match node.name.as_str() {
            "table" => {
                attributes.summary = node.attr("summary").map(str::to_owned);
            }
            "th" | "td" => {
                if let Some(id) = node.id.as_deref() {
                    if self.cells.get(id) == Some(&element) {
                        tag = tag.id(self.qualified(id).as_bytes());
                    }
                }
                // HTML's `headers` is a list of ids separated by white space;
                // one naming no cell of this document is a reference into
                // nothing and is not written.
                attributes.headers = node
                    .attr("headers")
                    .unwrap_or_default()
                    .split_ascii_whitespace()
                    .filter(|id| self.cells.contains_key(*id))
                    .map(|id| self.qualified(id).into_bytes())
                    .collect();
                if node.name == "th" {
                    attributes.scope = node.attr("scope").and_then(scope);
                }
                attributes.row_span = node.attr("rowspan").and_then(span);
                attributes.col_span = node.attr("colspan").and_then(span);
            }
            _ => {}
        }
        if attributes.is_empty() {
            tag
        } else {
            tag.table(attributes)
        }
    }

    /// An `id` of this content document as a document-wide identifier.
    fn qualified(&self, id: &str) -> String {
        format!("{}#{id}", self.path)
    }
}

/// HTML's `scope` as Table 349's. `col` and `row` are the column and row;
/// `colgroup` and `rowgroup` head the rest of their group, and the nearest
/// thing Table 349 has to a group is the same direction, so they are the
/// same answers. `auto` — HTML's default, decided from the table's shape — is
/// left unstated rather than decided here.
fn scope(value: &str) -> Option<TableScope> {
    match value.trim().to_ascii_lowercase().as_str() {
        "col" | "colgroup" => Some(TableScope::Column),
        "row" | "rowgroup" => Some(TableScope::Row),
        _ => None,
    }
}

/// A `colspan` or `rowspan` worth stating: a whole number above one. HTML's
/// `rowspan="0"` (to the end of the group) has no Table 349 spelling and is
/// left unstated.
fn span(value: &str) -> Option<u32> {
    value.trim().parse::<u32>().ok().filter(|span| *span > 1)
}

impl Tagging<'_> {
    /// The `/Lang` an element is written with (14.9.2): what its own
    /// `xml:lang` or `lang` says, or — for an element at the top that says
    /// nothing — the content document's, when that differs from the
    /// document's. Anything else is inherited through the tree and the
    /// catalog, which is what 14.9.2's hierarchy is for, and writing it again
    /// on every element would say nothing more.
    ///
    /// A value that is not shaped like a language tag is not written; the
    /// content document is reported for it once by
    /// [`malformed_language_tags`].
    fn language(&self, element: usize, level: usize) -> Option<&str> {
        // A declaration that is not a tag says nothing a reader can use, so it
        // is treated as no declaration: the element inherits, exactly as one
        // that said nothing would. Found by `every_language_declaration_is_a_lang`,
        // whose first version of this function dropped the malformed tag and
        // with it the content document's own language.
        match well_formed_language(self.dom, element) {
            Some(language) => Some(language),
            None if level == 0 => self
                .root_language()
                .filter(|root| Some(*root) != self.document_language),
            None => None,
        }
    }
}

/// An element's own declaration of its language: `xml:lang` before `lang`,
/// as the selector engine's `:lang()` reads it (EPUB 3.3 §3.2 makes a content
/// document XML, so the XML attribute is the normative one).
fn declared_language(dom: &Dom, element: usize) -> Option<&str> {
    let node = dom.nodes.get(element)?;
    node.attr("xml:lang").or_else(|| node.attr("lang"))
}

/// The same, when it is shaped like a language tag.
fn well_formed_language(dom: &Dom, element: usize) -> Option<&str> {
    declared_language(dom, element).filter(|language| is_language_tag(language))
}

/// How many elements of a content document declare a language that is not
/// shaped like a language tag, and so are written with no `/Lang`.
pub(crate) fn malformed_language_tags(dom: &Dom) -> usize {
    (0..dom.nodes.len())
        .filter(|at| declared_language(dom, *at).is_some_and(|tag| !is_language_tag(tag)))
        .count()
}

/// The element chain a run sits under, outermost first.
///
/// From the run's own element up to — but not including — `<body>`, then
/// reversed. `<body>` is left out because [`DocumentBuilder`] already wraps
/// every page's roots in a `/Document`, and a `/Sect` per page under it that
/// meant "this chapter's body" would be a level that says nothing.
///
/// An element with no anchor gets an empty chain and is drawn untagged rather
/// than guessed at, which is the same refusal the reader makes: this build
/// does not invent structure (`docs/design/tagged-pdf.md`).
pub(crate) fn ancestry(dom: &Dom, anchor: Option<u32>) -> Vec<usize> {
    let Some(anchor) = anchor else {
        return Vec::new();
    };
    let mut at = anchor as usize;
    if at >= dom.nodes.len() {
        return Vec::new();
    }
    let body = dom.body();
    let mut chain = Vec::new();
    loop {
        if Some(at) == body {
            break;
        }
        chain.push(at);
        match dom.nodes[at].parent {
            // `parent` is always less than the node's own index, so this
            // terminates without a visited set.
            Some(parent) => at = parent,
            None => break,
        }
    }
    chain.reverse();
    chain
}

/// Draws a run of runs, opening one structure element per level they share.
///
/// **Grouped rather than one element per run.** Consecutive runs of one
/// paragraph share its whole chain, and opening a `/P` for each of them would
/// make a paragraph of three runs three paragraphs. The runs arrive in reading
/// order — `fragment::order` sorted them — so equal chains are adjacent and a
/// partition by the level's element is all the grouping there is to do.
#[allow(clippy::too_many_arguments)]
pub(crate) fn tag_runs(
    builder: &mut DocumentBuilder,
    page: &mut PageBuilder,
    frame: &Frame,
    fonts: &Fonts<'_>,
    tagging: &Tagging<'_>,
    runs: &[&TextRun],
    chains: &[Vec<usize>],
    level: usize,
    refused: &mut usize,
    effects: &OnPage<'_>,
) {
    // The element these runs are inside, and where the last thing drawn in it
    // here reads — for the picture that reads between two of its runs.
    let owner = level
        .checked_sub(1)
        .and_then(|up| chains.first()?.get(up))
        .copied();
    let mut last: Option<u64> = None;
    let mut at = 0usize;
    while at < runs.len() {
        // A run whose chain has run out belongs to the element opened around
        // it, so it is drawn here rather than descended into.
        if chains[at].len() <= level {
            let here = (runs[at].order as u64).saturating_mul(2);
            // **A picture between two runs of one element splits it.** The
            // picture was drawn before the text, in painting order, and
            // `finish` places its `/Figure` among this element's kids by
            // position — which needs the text before it and the text after it
            // to be two sequences, each reading where its run does. Found by
            // `every_img_alt_is_a_figure_alt`: without it, `<p>a <img/> b</p>`
            // read "a b" and then the picture.
            if let (Some(owner), Some(previous)) = (owner, last) {
                if tagging.figures.between(tagging.dom, owner, previous, here) {
                    page.continue_at(tagging.chapter.saturating_add(here));
                }
            }
            *refused += artifact_or_run(builder, page, runs[at], frame, fonts, effects);
            last = Some(here);
            at += 1;
            continue;
        }
        let element = chains[at][level];
        let mut end = at + 1;
        while end < runs.len() && chains[end].get(level) == Some(&element) {
            end += 1;
        }
        let (slice, tails) = (&runs[at..end], &chains[at..end]);
        // **The key is the element and the order is the reading position**,
        // and they are two numbers because they answer two questions. The key
        // has to be the same on every page this element appears on or its
        // halves never merge, so it is the element's own index. The order has
        // to ascend with the document or the halves merge into the wrong
        // place, so it is the reading-order stamp of the first run under it —
        // which for a float is where it was *met*, not where its box landed.
        let tag = tagging.tag(element, level, tagging.order(runs[at]));
        page.tagged_with(&tag, |page| {
            tag_runs(
                builder,
                page,
                frame,
                fonts,
                tagging,
                slice,
                tails,
                level + 1,
                refused,
                effects,
            );
        });
        last = Some((runs[end - 1].order as u64).saturating_mul(2));
        at = end;
    }
}

/// Draws one picture inside the elements it sits in, as a `/Figure` whose
/// `/Alt` is the `<img>`'s `alt` (14.9.3).
///
/// The elements around it are opened under the keys the text runs use, so
/// the picture joins the paragraph or the `<figure>` it was written in when
/// `finish` merges them, at the position [`figure_orders`] gave it.
///
/// **An empty `alt` is an artifact, not a figure.** HTML says an `<img>`
/// whose `alt` is the empty string represents nothing — it is decoration —
/// and 14.8.2.2 puts decoration outside the structure: so it is drawn inside
/// `/Artifact BMC … EMC`, the way a list marker is. An `<img>` with no `alt`
/// at all is a `/Figure` with no `/Alt`: the book did not say, and a
/// description invented here would be this engine speaking for the author.
///
/// `effects` is what the picture's elements apply to it — their opacity, the
/// clips above it, their transforms — and it is drawn inside them exactly as
/// an untagged picture is ([`super::paint`]), within the marked-content
/// sequence so that a `q`/`Q` pair and a `BDC`/`EMC` one nest.
pub(crate) fn draw_figure(
    page: &mut PageBuilder,
    tagging: &Tagging<'_>,
    fragment: &ReplacedFragment,
    frame: &Frame,
    name: &[u8],
    effects: &OnPage<'_>,
) {
    let paint = |page: &mut PageBuilder| {
        let opened = effects.open(page, fragment.anchor, false);
        draw_replaced(page, fragment, frame, name);
        Effects::close(page, opened);
    };
    let chain = ancestry(tagging.dom, fragment.anchor);
    let Some((&picture, around)) = chain.split_last() else {
        paint(page);
        return;
    };
    let alt = tagging
        .dom
        .nodes
        .get(picture)
        .and_then(|node| node.attr("alt"));
    if alt == Some("") {
        page.raw(b"/Artifact BMC");
        paint(page);
        page.raw(b"EMC");
        return;
    }
    let order = fragment
        .anchor
        .and_then(|anchor| tagging.figures.by_anchor.get(&anchor))
        .map_or(tagging.chapter, |order| {
            tagging.chapter.saturating_add(*order)
        });
    let mut figure = Tag::new(b"Figure").keyed(tagging.key(picture), order);
    if let Some(alt) = alt {
        figure = figure.alt(alt);
    }
    if let Some(language) = tagging.language(picture, around.len()) {
        figure = figure.lang(language);
    }
    open_around(
        page,
        tagging,
        around,
        0,
        order,
        &mut |page: &mut PageBuilder| {
            page.tagged_with(&figure, paint);
        },
    );
}

/// Opens `chain[level..]` around `draw`, outermost first, each under the key
/// and with the properties the text runs give it.
fn open_around(
    page: &mut PageBuilder,
    tagging: &Tagging<'_>,
    chain: &[usize],
    level: usize,
    order: u64,
    draw: &mut dyn FnMut(&mut PageBuilder),
) {
    let Some(&element) = chain.get(level) else {
        draw(page);
        return;
    };
    let tag = tagging.tag(element, level, order);
    page.tagged_with(&tag, |page| {
        open_around(page, tagging, chain, level + 1, order, draw);
    });
}

/// Where each picture of a content document reads, among its text: an odd
/// number between the doubled positions of the run before it and the run
/// after it ([`Tagging::order`]).
///
/// `tinker-pdf-layout` stamps every text run with its position in document
/// order and stamps a picture with none, so the position is recovered from
/// the element tree: a run precedes a picture when its element ends before
/// the picture's begins — which, with elements numbered in document order,
/// is a run whose element has a lower index and is not one of the picture's
/// ancestors. A run written directly in an ancestor, beside the picture —
/// `<p>see <img/> here</p>` — is placed by counting: the ancestor's text
/// before the child holding the picture has so many characters that are not
/// white space, and the ancestor's runs, taken in order, use them up. White
/// space is left out of the count because collapsing it is the one thing the
/// layout does to text that changes its length.
pub(crate) fn figure_orders(dom: &Dom, pages: &[LayoutPage]) -> Figures {
    let mut runs: Vec<&TextRun> = pages
        .iter()
        .flat_map(|page| page.runs.iter())
        .filter(|run| !run.generated && run.anchor.is_some())
        .collect();
    runs.sort_by_key(|run| run.order);

    let mut out = BTreeMap::new();
    for fragment in pages.iter().flat_map(|page| page.replaced.iter()) {
        let Some(anchor) = fragment.anchor else {
            continue;
        };
        let picture = anchor as usize;
        if picture >= dom.nodes.len() || out.contains_key(&anchor) {
            continue;
        }
        // Each ancestor, with how many non-white-space characters of its own
        // text come before the child on the path to the picture.
        let mut before: BTreeMap<usize, usize> = BTreeMap::new();
        let mut child = picture;
        while let Some(parent) = dom.nodes[child].parent {
            let mut count = 0usize;
            if let Some(node) = dom.nodes.get(parent) {
                for kid in &node.children {
                    match kid {
                        super::xhtml::Child::Element(at) if *at == child => break,
                        super::xhtml::Child::Text(text) => {
                            count += text.chars().filter(|c| !c.is_whitespace()).count();
                        }
                        super::xhtml::Child::Element(_) => {}
                    }
                }
            }
            before.insert(parent, count);
            child = parent;
        }

        let mut used: BTreeMap<usize, usize> = BTreeMap::new();
        let mut last = 0u64;
        for run in &runs {
            let Some(element) = run.anchor.map(|anchor| anchor as usize) else {
                continue;
            };
            let precedes = match before.get(&element) {
                Some(budget) => {
                    let total = used.entry(element).or_insert(0);
                    *total += run.text.chars().filter(|c| !c.is_whitespace()).count();
                    *total <= *budget
                }
                None => element < picture,
            };
            if precedes {
                last = last.max(run.order as u64);
            }
        }
        out.insert(anchor, last.saturating_mul(2).saturating_add(1));
    }
    let mut by_order: Vec<(u64, u32)> = out
        .iter()
        .map(|(anchor, order)| (*order, *anchor))
        .collect();
    by_order.sort_unstable();
    Figures {
        by_anchor: out,
        by_order,
    }
}

/// Registers a `/RoleMap` entry (14.7.3) for every element name of a content
/// document whose standard type is not its own spelling, and returns the
/// names it registered — the ones [`Tagging::tag`] then writes as
/// themselves.
///
/// **The XHTML name is kept and its meaning stated**, rather than the name
/// thrown away: `<em>` is written `/S /em` and `<strong>` `/S /strong`, both
/// mapped to `/Span`, so a reader that knows only the standard set reads two
/// spans and one that wants the book's own vocabulary has it. A name that is
/// its standard type's spelling (`p`, `table`, `h1`) is written as the
/// standard type, since mapping `/p` to `/P` would say nothing a reader does
/// not already know. A name `DocumentBuilder::map_role` refuses — one that is
/// itself a standard type, which an element outside the XHTML namespace can
/// be — is written as its standard type too.
pub(crate) fn register_roles(builder: &mut DocumentBuilder, dom: &Dom) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for node in &dom.nodes {
        if out.contains(&node.name) {
            continue;
        }
        let standard = structure_type(&node.name);
        if node.name.is_empty() || node.name.eq_ignore_ascii_case(standard) {
            continue;
        }
        if builder.map_role(node.name.as_bytes(), standard.as_bytes()) {
            out.insert(node.name.clone());
        }
    }
    out
}

/// ISO 32000 Table 333's standard structure type for an XHTML element — the
/// type its own name is role-mapped to where the two differ
/// ([`register_roles`]).
pub(crate) fn structure_type(name: &str) -> &'static str {
    match name {
        "p" => "P",
        "h1" => "H1",
        "h2" => "H2",
        "h3" => "H3",
        "h4" => "H4",
        "h5" => "H5",
        "h6" => "H6",
        "ul" | "ol" | "dl" => "L",
        "li" | "dt" | "dd" => "LI",
        "table" => "Table",
        "thead" => "THead",
        "tbody" => "TBody",
        "tfoot" => "TFoot",
        "tr" => "TR",
        "td" => "TD",
        "th" => "TH",
        "caption" | "figcaption" => "Caption",
        "blockquote" => "BlockQuote",
        "code" | "kbd" | "samp" | "var" | "pre" => "Code",
        // A subscript is text: `/Span`. It used to be `/Sub`, which is not
        // one of ISO 32000-1's types (`STANDARD_STRUCTURE_TYPES`) and was
        // written with no role map to say what it meant — a non-standard type
        // in a document claiming `/Marked true`.
        "sub" | "sup" => "Span",
        // MathML's element, outside the XHTML namespace: 14.8.4.5's formula.
        "math" => "Formula",
        // A picture is a `/Figure` (14.8.4.5) and carries the description;
        // `<figure>` is the grouping around it and its caption, and as a
        // `/Figure` of its own it would be a figure with no `/Alt` wrapped
        // round one that has it — which every PDF/UA reading reports.
        "img" => "Figure",
        "figure" => "Div",
        "section" | "article" | "nav" | "aside" | "header" | "footer" | "main" => "Sect",
        // `<a>` is not here: one holding a link annotation is a `/Link`, and
        // [`Tagging::tag`] decides that, because it depends on whether this
        // build resolved its `href` and not on its name. One holding none is
        // a `/Span` like any other inline element — a bare `/Link` would claim
        // an association to assistive technology that is not in the file
        // (14.8.4.4.2).
        //
        // §14.8.4.2's two inline defaults. Anything block-level this build
        // does not name is a `/Div` and anything else is a `/Span`, which is
        // what a reader does with an unknown tag anyway — and is honest,
        // because the alternative is inventing a type from a class attribute.
        "div" | "body" | "html" | "form" | "fieldset" => "Div",
        _ => "Span",
    }
}
