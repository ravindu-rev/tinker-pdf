//! The cascade over a content document, and the box tree it produces (gap 31,
//! milestone 8).
//!
//! Milestone 6 built a CSS engine that matches against a trait, milestone 7 a
//! layout engine that takes plain structs; this is the file that owns the join,
//! and the two decisions it makes are the ones neither leaf could.
//!
//! # The user-agent stylesheet is a file, and it is committed
//!
//! [`UA_STYLESHEET`] is `src/epub/ua.css`, included at compile time and parsed
//! by milestone 6's parser exactly as a book's own sheet is. It is not a table
//! of Rust constants and not a `match` on element names, and that is the whole
//! point: **a UA sheet written in Rust is a second style system**, with its own
//! specificity rules, its own cascade order and no way for an author to beat
//! it. Written as CSS at `Origin::UserAgent` it loses to a book's own rules by
//! `css-cascade-5` §6.1's ordinary machinery, which is what a reading system
//! is required to do.
//!
//! Its absence is **visible and not merely worse**, and
//! `tests/epub_reading.rs` is what says so: with no UA sheet every
//! element computes `display: inline`, so a book has no block boxes at all —
//! every chapter is one paragraph, every heading is body text, and `<head>` is
//! set into the flow. Those are three independent consequences and the test
//! asserts all three, because a test for one of them is not a test.
//!
//! # Text nodes get the anonymous inline box's style, not the parent's
//!
//! CSS 2.2 §9.2.2.1 wraps a text node in an **anonymous inline box** that
//! inherits from its parent and has no margin, no padding, no border and no
//! background. Giving the text the parent's own computed style instead is the
//! shortcut that looks identical on a `<span>` and doubles every margin on a
//! `<p>`: the parent's `margin: 1em 0` would be applied by the `<p>`'s block
//! box and again by a text child that `tinker-pdf-layout` would then treat as
//! block-level. [`inline_box`] is that rule, and
//! `a_paragraph_does_not_pay_its_own_margin_twice` is what holds it.

use std::cell::RefCell;

use tinker_pdf_css::cascade::{cascade_from, ComputedStyle, Origin, StyleTree};
use tinker_pdf_css::font_face::FontFace;
use tinker_pdf_css::media::MediaContext;
use tinker_pdf_css::parser::Stylesheet;
use tinker_pdf_css::property::Display;
use tinker_pdf_css::{
    Budget as CssBudget, ImportResolver, Limits as CssLimits, Refusal as CssRefusal,
};
use tinker_pdf_layout::{BoxNode, CellSpan, Content};

use super::ocf::{resolve_reference, Ocf};
use super::xhtml::{Child, Dom, Node};
use super::Limits;

/// The user-agent stylesheet, as CSS, committed at `src/epub/ua.css`.
pub const UA_STYLESHEET: &str = include_str!("ua.css");

/// CSS 2.2 §4.3.2's reference pixel against a PDF point: 96 to 72.
///
/// Everything `tinker-pdf-css` computes is in CSS pixels — `absolute_px` turns
/// a `pt` into one and not the other way round — and everything a PDF page is
/// measured in is points. The factor lives here, once, at the boundary between
/// the two, because a build that converted in two places would eventually
/// convert in one and a half.
pub const PX_TO_PT: f64 = 72.0 / 96.0;

/// What one content document's stylesheets cost and could not do.
#[derive(Clone, Debug, Default)]
pub struct Census {
    /// Properties this build knows the name of and did not honour, per
    /// property, counted **by element reached** — `tinker-pdf-css`'s own
    /// counting, which is what makes the number a property of the book rather
    /// than of the stylesheet.
    pub unsupported: Vec<(&'static str, usize)>,
    /// Names no specification this build cites defines: a vendor extension, a
    /// custom property, or a typo.
    pub unknown: Vec<(String, usize)>,
    /// Declarations discarded by `css-syntax-3` §5.4.4.
    pub discarded_declarations: usize,
    /// Rules discarded by §5.4.2.
    pub discarded_rules: usize,
}

impl Census {
    /// Folds another census into this one, keeping the counts additive.
    pub fn absorb(&mut self, other: &Census) {
        for (property, count) in &other.unsupported {
            match self.unsupported.iter_mut().find(|(p, _)| p == property) {
                Some(slot) => slot.1 += count,
                None => self.unsupported.push((property, *count)),
            }
        }
        for (property, count) in &other.unknown {
            match self.unknown.iter_mut().find(|(p, _)| p == property) {
                Some(slot) => slot.1 += count,
                None => self.unknown.push((property.clone(), *count)),
            }
        }
        self.discarded_declarations += other.discarded_declarations;
        self.discarded_rules += other.discarded_rules;
    }

    /// The census, sorted by count and then by name, which is what a report
    /// prints and what a test compares.
    #[must_use]
    pub fn ranked(&self) -> Vec<(&'static str, usize)> {
        let mut out = self.unsupported.clone();
        out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        out
    }

    /// How many elements were affected by an unimplemented property, in total.
    #[must_use]
    pub fn affected(&self) -> usize {
        self.unsupported.iter().map(|(_, count)| count).sum()
    }
}

/// Where an `@import` in a book's stylesheet is resolved from.
///
/// Milestone 6 built [`ImportResolver`] and shipped `NoImports` beside it,
/// saying in as many words that *"a caller that has an OCF container
/// implements this"*. This is that caller. The `RefCell` is not a shortcut:
/// [`Ocf::read`] takes `&mut self` because inflating an entry spends the
/// archive's budget, and the trait takes `&self` because a resolver is shared
/// by a whole parse.
struct Container<'a, 'b> {
    ocf: RefCell<&'b mut Ocf<'a>>,
    limits: Limits,
}

impl ImportResolver for Container<'_, '_> {
    fn resolve(&self, href: &str, base: Option<&str>) -> Option<(String, Vec<u8>)> {
        // A sheet with no address of its own is a `<style>` element, and its
        // base is the document that holds it — which the caller put in `base`
        // for exactly this. With neither there is nothing to resolve against
        // and the import is dropped rather than guessed at.
        let base = base?;
        let path = resolve_reference(base, href, &self.limits).ok()?;
        let mut ocf = self.ocf.borrow_mut();
        let index = ocf.index_of(&path)?;
        let bytes = ocf.read(index).ok()?.to_vec();
        Some((path, bytes))
    }
}

/// Everything a content document is read *against*, which is the same for
/// every spine item in one book.
///
/// A struct rather than five parameters, and not only because
/// `clippy::too_many_arguments` says so: **these five are the book's, and the
/// two that are not — the path and the bytes — are the chapter's.** A caller
/// that built a fresh one per spine item would re-tokenize the user-agent
/// sheet thirteen times and could give two chapters of one book different
/// initial values.
pub struct Context<'a> {
    /// The parsed user-agent stylesheet, parsed once per book.
    pub ua: &'a [Stylesheet],
    /// The container's own ceilings, for resolving a `<link href>`.
    pub limits: &'a Limits,
    /// What the cascade may spend.
    pub css_limits: &'a CssLimits,
    /// What `@media` is evaluated against.
    pub media: &'a MediaContext,
    /// The root element's initial values, carrying the caller's base font
    /// size. See [`tinker_pdf_css::cascade::cascade_from`].
    pub initial: &'a ComputedStyle,
}

/// One content document, cascaded and turned into a box tree.
pub struct Reading {
    /// The element tree, kept because the anchors on the box tree index into
    /// it: a link's target, a destination's `id` and the outline all need to
    /// walk back from a positioned run to the element it came from.
    pub dom: Dom,
    /// The box tree, rooted at `<body>`.
    pub tree: BoxNode,
    /// One computed style per element of [`Reading::dom`].
    pub styles: StyleTree,
    /// What the cascade could not honour.
    pub census: Census,
    /// Every `@font-face` this document's author sheets declared, in source
    /// order, each carrying the address it must be resolved against (gap 31,
    /// milestone 9).
    ///
    /// Per **document** and not per book, because a spine item's sheets are
    /// its own — and folded into one set by the caller, because a PDF's font
    /// resources belong to the document rather than to a page.
    pub font_faces: Vec<FontFace>,
}

/// Reads one content document: markup, stylesheets, cascade, box tree.
///
/// `ua` is the parsed user-agent sheet, parsed once per book rather than once
/// per spine item — a thirteen-chapter book would otherwise tokenize the same
/// four kilobytes thirteen times, and the sheet cannot differ between them.
///
/// `initial` carries the caller's base font size as the root element's initial
/// value; see [`tinker_pdf_css::cascade::cascade_from`] for why that is not a
/// stylesheet rule.
///
/// # Errors
/// A [`CssRefusal`] from the cascade: a cap, or a tree the cascade will not
/// walk. A markup failure is **not** an error — it is a
/// [`super::xhtml::MarkupDefect`] on a partial tree, because a chapter that
/// stops half way has still said most of itself.
pub fn read_document(
    book: &mut Ocf<'_>,
    path: &str,
    bytes: &[u8],
    context: &Context<'_>,
    budget: &mut CssBudget,
) -> Result<Reading, CssRefusal> {
    let Context {
        ua,
        limits,
        css_limits,
        media,
        initial,
    } = context;
    let dom = match super::xhtml::read(bytes, &limits.xml) {
        Ok(dom) => dom,
        // An encoding this build does not decode, or a character XML §2.2
        // forbids. There is no tree at all, and the page that results says so
        // through its own defect rather than through this one.
        Err(_) => Dom {
            defects: vec![super::xhtml::MarkupDefect::Truncated],
            ..Dom::default()
        },
    };

    let author = author_sheets(book, path, &dom, limits, css_limits, budget, media);
    let mut sheets: Vec<(Origin, &Stylesheet)> =
        ua.iter().map(|sheet| (Origin::UserAgent, sheet)).collect();
    for sheet in &author {
        sheets.push((Origin::Author, sheet));
    }

    let styles = cascade_from(&sheets, &dom.nodes, css_limits, budget, initial)?;

    let mut census = Census {
        unsupported: styles.report.unsupported.clone(),
        unknown: styles.report.unknown.clone(),
        discarded_declarations: 0,
        discarded_rules: 0,
    };
    // A sheet's own parse report is counted **per sheet**, not per element: a
    // declaration §5.4.4 threw away never reached the cascade at all, so there
    // is no element for it to have affected. The two numbers are kept apart
    // for that reason rather than summed into one that means neither.
    for sheet in &author {
        census.discarded_declarations += sheet.report.discarded_declarations;
        census.discarded_rules += sheet.report.discarded_rules;
    }

    // A `<style>` element's sheet has no address of its own, so `parse` left
    // the base `None` even though the element is inside a document that does
    // have one. Filling it in here rather than in `tinker-pdf-css` is ruling 8:
    // the CSS crate knows what the sheet said and this one knows where the
    // sheet was.
    let mut font_faces: Vec<FontFace> = Vec::new();
    for sheet in &author {
        for face in &sheet.font_faces {
            let mut face = face.clone();
            if face.base.is_none() {
                face.base = Some(path.to_owned());
            }
            font_faces.push(face);
        }
    }

    let tree = box_tree(&dom, &styles);
    Ok(Reading {
        dom,
        tree,
        styles,
        census,
        font_faces,
    })
}

/// Every stylesheet a content document pulls in, in document order.
///
/// `<link rel="stylesheet">` and `<style>` in the order they appear, which is
/// `css-cascade-5` §6.1's sixth criterion — two sheets that set the same
/// property at the same specificity are decided by which came later, and a
/// build that read every `<link>` before every `<style>` would get that
/// backwards for calibre's books, which write both.
fn author_sheets(
    book: &mut Ocf<'_>,
    path: &str,
    dom: &Dom,
    limits: &Limits,
    css_limits: &CssLimits,
    budget: &mut CssBudget,
    media: &MediaContext,
) -> Vec<Stylesheet> {
    let mut out = Vec::new();
    for node in &dom.nodes {
        if !node.is_html() {
            continue;
        }
        match node.name.as_str() {
            "link" => {
                if !applies_as_stylesheet(node.attr("rel").unwrap_or_default()) {
                    continue;
                }
                let Some(href) = node.attr("href") else {
                    continue;
                };
                let Ok(target) = resolve_reference(path, href, limits) else {
                    continue;
                };
                let Some(index) = book.index_of(&target) else {
                    continue;
                };
                let Ok(bytes) = book.read(index).map(<[u8]>::to_vec) else {
                    continue;
                };
                let resolver = Container {
                    ocf: RefCell::new(book),
                    limits: *limits,
                };
                if let Ok(sheet) = tinker_pdf_css::parser::parse(
                    &bytes,
                    Some(&target),
                    &resolver,
                    media,
                    css_limits,
                    budget,
                ) {
                    out.push(sheet);
                }
            }
            "style" => {
                let mut source = String::new();
                for child in &node.children {
                    if let Child::Text(text) = child {
                        source.push_str(text);
                    }
                }
                if source.trim().is_empty() {
                    continue;
                }
                let resolver = Container {
                    ocf: RefCell::new(book),
                    limits: *limits,
                };
                // The **document's** path is the base, not `None`: a `<style>`
                // has no address of its own and HTML resolves a relative URL in
                // it against the document. Passing `None` would drop every
                // `@import` in an inline sheet.
                if let Ok(sheet) = tinker_pdf_css::parser::parse(
                    source.as_bytes(),
                    Some(path),
                    &resolver,
                    media,
                    css_limits,
                    budget,
                ) {
                    out.push(sheet);
                }
            }
            _ => {}
        }
    }
    out
}

/// Whether a `<link rel>` names a stylesheet this build applies.
///
/// HTML §4.2 makes `rel` a **token list**, and two of its tokens matter here.
/// `stylesheet` is what makes the link one at all; `alternate` is what makes it
/// one a reading system offers rather than applies, and applying it would set
/// a book in a theme its author marked as *not the default*. A build that
/// compared the whole attribute against `"stylesheet"` would drop
/// `rel="stylesheet next"`, and one that searched for the substring would apply
/// `rel="alternate stylesheet"`.
///
/// A function of its own because neither corpus contains an alternate sheet, so
/// the rule is unreachable from a real book and a defect injected into it
/// survived every test in the suite until this existed.
#[must_use]
pub fn applies_as_stylesheet(rel: &str) -> bool {
    let mut names = false;
    let mut alternate = false;
    for token in rel.split_whitespace() {
        names |= token.eq_ignore_ascii_case("stylesheet");
        alternate |= token.eq_ignore_ascii_case("alternate");
    }
    names && !alternate
}

/// CSS 2.2 §9.2.2.1's anonymous inline box: the parent's inherited values and
/// nothing else.
///
/// `text-decoration` is copied across although it is not an inherited
/// property, which is §16.3.1's rule rather than an exception invented here:
/// the decoration is drawn by the element that declared it **across its
/// in-flow descendants**, so an `<a>`'s underline reaches its text and a
/// `<div>`'s reaches every line in it. A build that dropped it would underline
/// nothing anywhere, since a decoration is only ever declared on an ancestor
/// of the text it marks.
#[must_use]
pub fn inline_box(parent: &ComputedStyle) -> ComputedStyle {
    let mut style = ComputedStyle::inherit_from(parent);
    style.text_decoration = parent.text_decoration;
    style
}

/// Turns the element tree and its computed styles into a box tree.
///
/// Public so that a test can build the same tree from a cascade it controls:
/// there is no way to open a book **without** the user-agent stylesheet, which
/// is the point of committing it, so the only way to assert what its absence
/// costs is to cascade twice and lay both out through this.
///
/// # Rooted at the document element, and not at `<body>`
///
/// Starting at `<body>` is the obvious choice and it is **wrong in a way no
/// output shows**: it removes `<head>` from the tree by position rather than by
/// `display`, so `head { display: none }` in the user-agent sheet becomes a
/// rule that changes nothing and every test of the sheet's absence gets the
/// right answer for the wrong reason. Milestone 8 wrote it that way first and
/// the test that found it is
/// `without_the_ua_stylesheet_a_book_has_no_block_structure_at_all`, which
/// asserts that the `<title>` and the `<style>` **do** reach the page once the
/// sheet is removed — an assertion that cannot fail if the subtree was never
/// in the tree.
///
/// A document with no element at all lays out as an empty block, which is a
/// page rather than a refusal.
#[must_use]
pub fn box_tree(dom: &Dom, styles: &StyleTree) -> BoxNode {
    let Some(root) = dom.root else {
        return BoxNode::element(ComputedStyle::initial(), Vec::new());
    };
    build(dom, styles, root)
}

fn build(dom: &Dom, styles: &StyleTree, at: usize) -> BoxNode {
    let style = styles
        .styles
        .get(at)
        .cloned()
        .unwrap_or_else(ComputedStyle::initial);
    let node: &Node = &dom.nodes[at];
    let anchor = u32::try_from(at).unwrap_or(u32::MAX);
    let mut children = Vec::with_capacity(node.children.len());
    for child in &node.children {
        match child {
            Child::Element(index) => children.push(build(dom, styles, *index)),
            Child::Text(text) => {
                children.push(BoxNode::text(inline_box(&style), text.clone()).with_anchor(anchor));
            }
        }
    }
    // An element with no children at all still has to be a `Children(vec![])`
    // rather than a `Text("")`: an empty `<p>` generates a block box with its
    // own margins, and one carrying an empty string would be an inline box
    // with none.
    BoxNode {
        style,
        content: Content::Children(children),
        anchor: Some(anchor),
        span: cell_span(
            node,
            &styles.styles.get(at).map_or(Display::Inline, |s| s.display),
        ),
    }
}

/// HTML's `colspan`, `rowspan` and `span`, CSS 2.2 §17.5.
///
/// **This is the one thing a table needs that no stylesheet can say.** There is
/// no CSS property behind any of the three, so the cascade cannot carry them
/// and `tinker_pdf_layout::style::consume`'s compile-time device — which is
/// about computed styles — has nothing to say about them. They arrive on
/// [`BoxNode::span`] instead, from here, which is the file that already knows
/// what an XHTML attribute is.
///
/// The clamps are HTML's own and each is a different number for a different
/// reason. `colspan` is 1 to 1 000 and a `colspan="0"` is *one* column, because
/// HTML 4's *"spans every column"* reading was dropped. `rowspan` is 0 to
/// 65 534 and **zero survives**: it is HTML's *"to the end of the row group"*,
/// which is the only one of the three where zero is a value rather than a
/// mistake, and clamping it here would turn a real book's `rowspan="0"` into a
/// one-row cell with the rest of the column shifted up.
fn cell_span(node: &Node, display: &Display) -> CellSpan {
    match display {
        Display::TableCell => CellSpan {
            columns: attribute_count(node.attr("colspan"), 1, 1_000, 1),
            rows: attribute_count(node.attr("rowspan"), 0, 65_534, 1),
        },
        // `<col span>` and `<colgroup span>`, which say how many columns the
        // box describes rather than how many a cell occupies.
        Display::TableColumn | Display::TableColumnGroup => CellSpan {
            columns: attribute_count(node.attr("span"), 1, 1_000, 1),
            rows: 1,
        },
        _ => CellSpan::ONE,
    }
}

/// HTML's *"rules for parsing non-negative integers"*, clamped.
///
/// A value with trailing rubbish — `"3 "`, `"2x"` — is HTML's leading-digits
/// rule and yields the digits; a value with no leading digit at all is the
/// default. A build that used `str::parse` alone would give `colspan="2 "` one
/// column, which is a table with a hole in it.
fn attribute_count(value: Option<&str>, low: u32, high: u32, default: u32) -> u32 {
    let Some(raw) = value else {
        return default;
    };
    let digits: String = raw
        .trim_start()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    match digits.parse::<u32>() {
        Ok(count) => count.clamp(low, high),
        Err(_) => default,
    }
}
