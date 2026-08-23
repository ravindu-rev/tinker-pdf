//! Laying a content document out, for the tests that check where it landed.
//!
//! This is the machinery `epub_browser.rs` used to hold, minus the browser.
//! Ruling 13 retired that oracle; what asks the questions now is
//! `epub_analytic.rs`, which computes the answer from the box model in the
//! test, and `epub_reftest.rs`, which lays out two documents that must agree.
//! Both need the same thing from this module: markup and a stylesheet in,
//! positioned fragments out, through the **same** path the reader uses.
//!
//! # Why the metrics are monospace everywhere below
//!
//! `BookMetrics::STANDARD` measures with the standard 14, and Courier's every
//! advance is exactly 600/1000 of the em — a number this repository does not
//! choose and cannot drift, because it is in the AFM the specification's own
//! Appendix D describes. So a document set in `monospace` has a **closed-form**
//! line breaker: a measure of `W` points at `font-size: S` fits
//! `floor(W / (0.6 * S))` characters, and the test can say where every line
//! must start without asking the engine anything.
//!
//! That is what makes an analytic suite possible at all here. A proportional
//! face would make every expected value a table of advances copied out of a
//! font, which is a fixture asserting itself.

use tinker_pdf::epub::paint::BookMetrics;
use tinker_pdf::epub::read::{box_tree, PX_TO_PT, UA_STYLESHEET};
use tinker_pdf::epub::{xhtml, DEFAULT_FONT_SIZE};
use tinker_pdf_css::cascade::{cascade_from, ComputedStyle, Origin, StyleTree};
use tinker_pdf_css::media::MediaContext;
use tinker_pdf_css::parser::parse as css_parse;
use tinker_pdf_css::property::Display;
use tinker_pdf_css::{Budget as CssBudget, Limits as CssLimits, NoImports};
use tinker_pdf_layout::{layout, Layout, Limits as LayoutLimits, Options};

/// Courier's advance, as Appendix D states it: 600 units of a 1000-unit em.
///
/// Every one of the standard 14's monospaced faces states this, and it is the
/// only advance in this build that a test may hard-code — a proportional face's
/// would be a table copied out of a font, which is a fixture agreeing with
/// itself.
pub const MONO_ADVANCE: f64 = 0.6;

/// The declaration an analytic or reftest document is given, so that the face
/// is the one whose advance is known.
///
/// `!important` because a book's own `html { font-family: … }` beats a `*`
/// selector on specificity, and a rule that silently did not apply would leave
/// every expected value in this suite measured against the wrong face.
pub const MONO: &str = concat!(
    "* { font-family: monospace !important; }",
    "html { font-size: 16px !important; }"
);

/// One block box that has text of its own, with where its first line landed.
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    /// The element's local name.
    pub tag: String,
    /// Its **own** text, whitespace collapsed — not its subtree's, so a `<p>`
    /// holding an `<em>` contributes the words outside the emphasis and the
    /// `<em>` contributes the words inside it.
    pub text: String,
    /// The baseline of its first line, from the top of the flow.
    pub top: f64,
    /// The left edge of its first line.
    pub left: f64,
    /// Which page that line is on, counting from zero.
    pub page: usize,
}

/// One positioned line of text, as the tests below read them.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    /// The page it is on, counting from zero.
    pub page: usize,
    /// The left edge.
    pub x: f64,
    /// The **baseline**, from the top of the page's content area.
    pub y: f64,
    /// The advance width the run was measured at.
    pub width: f64,
    /// The characters.
    pub text: String,
    /// `font-size`, in points.
    pub size: f64,
    /// Whether the line is content the source did not contain — a list marker.
    pub generated: bool,
}

/// The markup, the cascade and the layout, at a page box the caller states.
///
/// The same entry point for every test here: a build whose analytic answers and
/// reftest answers came from two functions could agree twice for two reasons.
#[must_use]
pub fn lay_out(
    document: &str,
    author: &str,
    width_px: f64,
    height_px: f64,
) -> (xhtml::Dom, StyleTree, Layout) {
    let dom = xhtml::read(document.as_bytes(), &tinker_pdf_xml::Limits::DEFAULT).expect("markup");
    let media = MediaContext::screen(width_px, height_px);
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
    .expect("the committed sheet");
    // `css-cascade-5` §6.1's order: this build's user-agent rules first, so the
    // document's own sheet still beats them, exactly as it does in the reader.
    let author = format!("{author}\n{MONO}\n");
    let sheet = css_parse(
        author.as_bytes(),
        None,
        &NoImports,
        &media,
        &limits,
        &mut budget,
    )
    .expect("the document's sheet");

    let mut initial = ComputedStyle::initial();
    initial.font_size = DEFAULT_FONT_SIZE / PX_TO_PT;
    let styles = cascade_from(
        &[(Origin::UserAgent, &ua), (Origin::Author, &sheet)],
        &dom.nodes,
        &limits,
        &mut budget,
        &initial,
    )
    .expect("a cascade");

    let laid = layout(
        &box_tree(&dom, &styles),
        &BookMetrics::STANDARD,
        &Options::new(width_px, height_px),
        &LayoutLimits::DEFAULT,
    )
    .expect("a layout");
    (dom, styles, laid)
}

/// A whole document laid into **one** page, so nothing is fragmented.
///
/// A hundred thousand points is one page for anything a test here writes, and
/// keeping fragmentation out of a column measurement is what lets the
/// fragmentation tests be about fragmentation.
#[must_use]
pub fn column(document: &str, author: &str, width_px: f64) -> Vec<Line> {
    lines(&lay_out(document, author, width_px, 100_000.0).2)
}

/// Every line of a layout, in page then reading order.
#[must_use]
pub fn lines(laid: &Layout) -> Vec<Line> {
    let mut out = Vec::new();
    for (number, page) in laid.pages.iter().enumerate() {
        let mut runs: Vec<&tinker_pdf_layout::TextRun> = page.runs.iter().collect();
        runs.sort_by_key(|run| run.order);
        for run in runs {
            out.push(Line {
                page: number,
                x: run.x,
                y: run.y,
                width: run.width,
                text: run.text.clone(),
                size: run.font_size,
                generated: run.generated,
            });
        }
    }
    out
}

/// The lines the source wrote, which is every line that is not a marker.
#[must_use]
pub fn written(laid: &Layout) -> Vec<Line> {
    lines(laid).into_iter().filter(|l| !l.generated).collect()
}

/// Every block box with text of its own, in document order.
///
/// The unit a reftest compares, because it is the one an author can reason
/// about: an element, its own words, and where its first line landed.
#[must_use]
pub fn blocks(document: &str, author: &str, width_px: f64, height_px: f64) -> Vec<Block> {
    let (dom, styles, laid) = lay_out(document, author, width_px, height_px);
    let mut out = Vec::new();
    for (index, node) in dom.nodes.iter().enumerate() {
        let style = &styles.styles[index];
        if !matches!(style.display, Display::Block | Display::ListItem) {
            continue;
        }
        let mut text = String::new();
        for child in &node.children {
            if let xhtml::Child::Text(chunk) = child {
                text.push_str(chunk);
            }
        }
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            continue;
        }
        let anchor = u32::try_from(index).unwrap_or(u32::MAX);
        let mine = || {
            laid.pages
                .iter()
                .enumerate()
                .flat_map(|(number, page)| page.runs.iter().map(move |run| (number, run)))
                .filter(|(_, run)| run.anchor == Some(anchor) && !run.generated)
        };
        // The **first** run in reading order, not the topmost: a block whose
        // second line starts further left would otherwise report a `left` that
        // belongs to a different line from its `top`.
        let Some((page, first)) = mine().min_by_key(|(_, run)| run.order) else {
            continue;
        };
        out.push(Block {
            tag: node.name.clone(),
            text,
            top: first.y,
            left: first.x,
            page,
        });
    }
    out
}

/// A whole XHTML document around `body`.
///
/// **The stylesheet is not in the head**, and that is not an omission: this
/// path takes the author sheet as an argument, exactly as the reader does when
/// it resolves a `<link>` to a stylesheet part of the book. A `<style>` element
/// written here would be quietly ignored, and every expected value in a test
/// that put its rules there would be measured against the user-agent sheet
/// alone — which is how the first draft of the analytic suite came to expect
/// `font-size: 20px` and measure 16.
#[must_use]
pub fn document(body: &str) -> String {
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><head>"#,
            r#"<title>t</title></head><body>{body}</body></html>"#
        ),
        body = body,
    )
}

/// How many characters of a monospaced face fit in a measure.
///
/// The closed form the analytic suite rests on, written from the box model
/// rather than asked of the line breaker: `css-text-3` §5 breaks at the last
/// opportunity that still fits, and with every advance equal that is a
/// division. Stated here so the one place it is computed is a place a reader
/// can check against the clause.
#[must_use]
pub fn fits(measure_pt: f64, size_pt: f64) -> usize {
    // A floating-point measure that is a whole number of advances must not lose
    // its last character to a representation error, so the division is nudged
    // by half a millionth of an advance before it is floored.
    let advance = MONO_ADVANCE * size_pt;
    ((measure_pt / advance) + 1e-6).floor().max(0.0) as usize
}
