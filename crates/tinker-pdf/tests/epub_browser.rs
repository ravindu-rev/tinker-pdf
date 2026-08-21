//! The fifth oracle: a headless browser (gap 31, milestone 8).
//!
//! Ruling 9 names four external tools — mutool, pdftoppm, pdfium_test and
//! qpdf — and gap 31's plan proposes a fifth, in writing, for a reason it works
//! out rather than asserts:
//!
//! > **For XPS, agreeing with mutool was evidence. For EPUB, disagreeing with
//! > mutool is not evidence of a bug.**
//!
//! MuPDF reads EPUB and `mutool draw` takes `-W`, `-H` and `-S` for exactly
//! this layout — but MuPDF's EPUB engine is *itself* a partial CSS
//! implementation, so a disagreement names no culprit. A browser is the
//! reference implementation of CSS, and comparing a CSS implementation against
//! a partial one is comparing it against nothing. The amendment is written into
//! `docs/plans/99-consistency.md` beside ruling 9 rather than left here.
//!
//! # What this compares, and what it deliberately does not
//!
//! **Not pixels.** Two rasterisers, two hinting policies, two anti-aliasers,
//! and — since milestone 9 has not landed — two different fonts. Gap 18a
//! pre-argued the same point for a fixed-point wavelet against a float
//! reference.
//!
//! **Not page against page.** A browser lays a content document into one
//! continuous column, so there is no page 3 to compare against page 3. That is
//! why there are two comparisons here rather than one.
//!
//! ## 1. The continuous comparison
//!
//! One content document, rendered by the browser at the page's own measure with
//! no pagination, against this engine's layout of the same document at a page
//! tall enough to hold all of it. Both sides report **one entry per block box
//! that has direct text of its own**, in document order.
//!
//! Two things are asserted, and they are asserted separately because they fail
//! separately:
//!
//! - **The sequence of blocks is identical, exactly.** No tolerance, no
//!   threshold. A `display: none` honoured on the wrong element, a `display`
//!   value not honoured at all, an element the cascade lost, a `<head>` set
//!   into the flow: every one of them changes this list, and none of them
//!   changes it by a small amount.
//! - **Each block's offset from the first agrees to within a stated
//!   tolerance**, as a fraction of the browser's own column. One denominator
//!   for both sides, and it is the reference implementation's: dividing each
//!   side by *its own* span was the first thing tried and it cannot see the
//!   defect it exists for, because a fault that shortens every paragraph
//!   shortens the column in the same proportion and the normalised positions
//!   barely move. What is compared is therefore *the shape of the column* in
//!   the browser's units — which is where a dropped margin, a wrong
//!   `box-sizing` and an uncollapsed margin all live.
//!
//! ## One variable is held fixed, and saying which is the point
//!
//! The first version of this oracle compared the two sides as they came, and
//! **the tolerance it needed was larger than the defect it was for**. The
//! measurement is in the plan's progress note: with each side using its own
//! face, deleting `p { margin: 1em 0 }` from this engine — eleven paragraphs'
//! worth of space in the fixture — moved the normalised column by 0.033, and
//! the honest disagreement between the two builds was already 0.03. An oracle
//! whose noise floor is its own defect is the thing gap 31's risk table calls
//! *"thresholded into meaninglessness"*.
//!
//! The cause is not subtle: two faces have different advance widths, so a
//! paragraph takes one more or one fewer line on one side, and a line is worth
//! about as much as a margin. So **both sides are told to set the document in
//! Courier New**, whose advances are the 600/1000 of the Courier this build
//! measures with. That is the one variable neither build can share until
//! milestone 9 embeds a face, and holding it fixed is what leaves the
//! comparison about the box model, the cascade, margin collapsing, line
//! breaking and fragmentation — which is everything the comparison is for.
//!
//! It is a **rule on both sides and not a fudge factor on one**: the same
//! declaration is appended to the browser's stylesheet and to this engine's,
//! and [`the_continuous_comparison_notices_a_dropped_margin`] is what says the
//! result can still fail. With the face fixed, that same deleted margin moves
//! the column by an order of magnitude more than [`MAX_COLUMN_DEVIATION`].
//!
//! ## 2. The paginated comparison
//!
//! Chromium's `--print-to-pdf` paginates at a page size the caller chooses, so
//! the same document at 432 × 648 gives a **page count** and per-page text to
//! compare against. That checks fragmentation, which the continuous comparison
//! cannot — and it is read back through *this repository's own reader*, which
//! is a comparison no other oracle in this workspace can make.
//!
//! # Red when the browser is missing
//!
//! Gap 20's finding, applied for the fourth time: **a skipped oracle exits 0
//! and reads exactly like a pass.** Every test here prints [`RAN`] or
//! [`SKIPPED`], and `.github/workflows/ci.yml`'s `browser-oracle` job greps its
//! own output for the second and goes red.

mod epub_support;

use std::path::{Path, PathBuf};
use std::process::Command;

use tinker_pdf::epub::paint::BookMetrics;
use tinker_pdf::epub::read::{box_tree, PX_TO_PT, UA_STYLESHEET};
use tinker_pdf::epub::{xhtml, DEFAULT_FONT_SIZE, DEFAULT_PAGE, PAGE_MARGIN};
use tinker_pdf::Document;
use tinker_pdf_css::cascade::{cascade_from, ComputedStyle, Origin};
use tinker_pdf_css::media::MediaContext;
use tinker_pdf_css::parser::parse as css_parse;
use tinker_pdf_css::property::Display;
use tinker_pdf_css::{Budget as CssBudget, Limits as CssLimits, NoImports};
use tinker_pdf_layout::{layout, Limits as LayoutLimits, Options};

/// Printed once per test that actually ran the browser. CI greps it.
const RAN: &str = "browser-oracle: RAN";

/// Printed once per test that could not. CI greps for it too, and fails.
const SKIPPED: &str = "browser-oracle: SKIPPED";

/// How far the two columns may disagree, as a fraction of the column.
///
/// **Not a tuned number: an itemised one.** The measured disagreement on the
/// fixture is 0.036, and every part of it is known and printed by the table
/// this test dumps:
///
/// | | Of the column |
/// | --- | --- |
/// | A constant 13 pt: the browser reports a **line box's top** and this engine a **baseline**, and the two differ by the half-leading of whichever font size the first block is set at | 0.007 |
/// | One `<table>`. **Milestone 11 lays it out as a table and this number did not move**: 0.0360 before and 0.0360 after, at the same block, with the same 0.019 across the interval that holds it. What moved is the reason — this engine's table is now 35 pt *shorter* than the browser's rather than being a paragraph of inline text — and it is deliberately **not** claimed to be localised. [`the_browser_and_this_engine_lay_the_same_tables_out_the_same_way`] agrees to 0.0005 over sixteen cells with a `colspan`, a `rowspan`, a bare-`<tr>` table and a nested one, so what is left here is a variable that fixture holds fixed and this one does not: `vertical-align`, which HTML's own user-agent sheet puts on every cell, or the `<code>` at `font-size: 85%` inside this table's third row | 0.019 |
/// | One paragraph that broke a line differently, which is `<code>`'s `font-size: 85%` inside it against a face this build has one size of | 0.010 |
///
/// The cap is 0.05, which is that measurement with room for one more line, and
/// the **injected defect measures 0.105** — twice the cap and three times the
/// honest disagreement. That ratio is what makes this an oracle rather than a
/// threshold: see [`the_continuous_comparison_notices_a_dropped_margin`], which
/// fails if it ever stops holding.
const MAX_COLUMN_DEVIATION: f64 = 0.05;

/// The declaration both sides are given, so that neither is measuring its own
/// font. See the module header.
///
/// `!important` because it has to beat the book's own `html { font-family:
/// Georgia, serif }`, which a `*` selector loses to on specificity — and
/// because a comparison that silently did not apply would be a comparison of
/// two different documents.
const SAME_FACE: &str = concat!(
    "* { font-family: \"Courier New\", monospace !important; }",
    "html { font-size: 16px !important; }"
);

/// The browser, if there is one.
///
/// `TINKER_BROWSER` first, so a runner can name one this list has never heard
/// of; then the two Chromium-family browsers a Windows machine has by default
/// and the three names a Linux runner installs. **Nothing here searches for a
/// browser that is not Chromium-family**, because `--dump-dom`,
/// `--print-to-pdf` and `--headless=new` are Chromium's own switches and a
/// Gecko or WebKit build would need a different driver rather than a different
/// path.
fn browser() -> Option<PathBuf> {
    if let Some(named) = std::env::var_os("TINKER_BROWSER") {
        let path = PathBuf::from(named);
        return path.is_file().then_some(path);
    }
    let candidates = [
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        "/usr/bin/google-chrome",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
        "/usr/bin/microsoft-edge",
    ];
    candidates
        .iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
}

macro_rules! oracle {
    ($what:expr) => {
        match browser() {
            Some(path) => {
                println!("{RAN} {} ({})", $what, path.display());
                path
            }
            None => {
                println!("{SKIPPED} {} (no Chromium-family browser found)", $what);
                return;
            }
        }
    };
}

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join("tinker-pdf-browser-oracle");
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

/// The corpus book this oracle is run over, and the document inside it.
///
/// `ch001.xhtml` is the chapter milestone 1 wrote to have something of every
/// shape in it: two levels of heading, an ordered list, an unordered list, a
/// block quotation, a preformatted block, a table, inline emphasis, inline
/// code and two cross-references. A shorter document would agree with anything.
const BOOK: &str = "pandoc-book-cover.epub";
const DOCUMENT: &str = "EPUB/text/ch001.xhtml";
const STYLESHEET: &str = "EPUB/styles/stylesheet1.css";

fn corpus_book(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("epub")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

fn entry(book: &[u8], name: &str) -> String {
    let bytes = epub_support::read(book, name).unwrap_or_else(|| panic!("no {name} in the book"));
    String::from_utf8(bytes).expect("UTF-8")
}

// ---- the browser side ----------------------------------------------------------

/// One block box with text of its own, as one side of the comparison reports it.
#[derive(Clone, Debug, PartialEq)]
struct Block {
    /// The element's local name.
    tag: String,
    /// Its own text, whitespace collapsed — **its own** and not its subtree's,
    /// so that a `<p>` holding an `<em>` contributes the words outside the
    /// emphasis and the `<em>` contributes the words inside it. Comparing
    /// `textContent` instead would count every word once per ancestor and would
    /// agree with itself whatever the box tree did.
    text: String,
    /// How far down the column its first line sits.
    top: f64,
    /// How far across the measure its first line starts.
    ///
    /// **Milestone 12 added this and the reason is the whole of why a flex
    /// comparison is not the table comparison over a different document.**
    /// Every oracle in this file until now compared *y* offsets, because every
    /// specification it was about — margin collapsing, floats, §17 — puts its
    /// boxes one under another. `justify-content` puts them one *beside*
    /// another: a build that ignored the property entirely, or that read
    /// `space-between` as `flex-start`, moves nothing at all in `y` and would
    /// pass a column comparison exactly.
    left: f64,
}

/// The page a browser is asked to lay out, with the same UA sheet on it.
///
/// The order matters and is `css-cascade-5` §6.1's: this build's user-agent
/// rules go **first**, so the book's own sheet still beats them, exactly as it
/// does in the reader. What cannot be neutralised is the browser's *own*
/// user-agent sheet underneath, and that is the honest limit gap 31's oracle
/// section names in advance: the rules that matter here — `display`, the
/// margins, the heading sizes — are all restated by this build's sheet at a
/// specificity that wins, so what is left underneath are the properties
/// neither this build nor this comparison reads.
fn oracle_page(document: &str, author: &str, width_px: Option<f64>) -> String {
    let body = between(document, "<body", "</body>");
    // **The measure is set on `<html>` and never on `<body>`**, and the
    // difference is sixteen pixels that took a page to find. `body
    // { margin: 8px }` is the user-agent sheet's own rule and this engine pays
    // it: layout is given the page's content box and the body's margin comes
    // out of it. Setting `width` on `body` as well makes the body's *content*
    // box the full measure and its border box sixteen pixels wider — which the
    // browser then either overflows or, when printing, scales the whole page
    // down to fit.
    //
    // `width_px` is `None` for the printed comparison, where `@page` states the
    // box and a `width` rule would fight it.
    let measure = match width_px {
        Some(width) => format!("<style>html {{ width: {width}px; }}</style>"),
        None => String::new(),
    };
    format!(
        concat!(
            "<!doctype html><html><head><meta charset=\"utf-8\">",
            "<style>{ua}</style><style>{author}</style><style>{face}</style>",
            "{measure}",
            "</head><body>{body}{script}</body></html>"
        ),
        ua = UA_STYLESHEET,
        author = author,
        face = SAME_FACE,
        measure = measure,
        body = body,
        script = MEASURE
    )
}

/// The measurement, run in the page after layout and left in the DOM for
/// `--dump-dom` to hand back.
const MEASURE: &str = r#"<script>
(function () {
  var out = [];
  var own = function (node) {
    var text = "";
    for (var i = 0; i < node.childNodes.length; i++) {
      if (node.childNodes[i].nodeType === 3) text += node.childNodes[i].nodeValue;
    }
    return text.replace(/\s+/g, " ").trim();
  };
  var walk = function (node) {
    var style = getComputedStyle(node);
    if (style.display === "none") return;
    var text = own(node);
    if (text.length > 0 && (style.display === "block" || style.display === "list-item")) {
      var range = document.createRange();
      range.selectNodeContents(node);
      var rects = range.getClientRects();
      var top = rects.length > 0 ? rects[0].top : node.getBoundingClientRect().top;
      var left = rects.length > 0 ? rects[0].left : node.getBoundingClientRect().left;
      out.push([node.tagName.toLowerCase(), (top + window.scrollY).toFixed(2), text,
                (left + window.scrollX).toFixed(2)]);
    }
    for (var i = 0; i < node.children.length; i++) walk(node.children[i]);
  };
  walk(document.body);
  var pre = document.createElement("pre");
  pre.id = "tinker-oracle";
  pre.textContent = out.map(function (row) { return row.join("\u0001"); }).join("\u0002");
  document.body.appendChild(pre);
})();
</script>"#;

/// The author's sheet with its `@media screen` blocks restated for `print`.
///
/// **The second variable that has to be held fixed, and it is not a font.**
/// Milestone 6 decided that this build evaluates `@media` as `screen`, with the
/// argument in `tinker-pdf-css`'s module header: a reflowable book is being set
/// for a reading system's viewport and not for paper. A browser asked to
/// `--print-to-pdf` evaluates it as `print`, so the two are given **different
/// stylesheets** unless something says otherwise — and on this fixture that
/// difference is not academic: pandoc writes
/// `@media screen { .sourceCode { white-space: pre-wrap !important } }` on
/// every book, so the browser leaves a preformatted block unwrapped in print
/// where this build wraps it, and the chapter is a page shorter.
///
/// That was found by this oracle rather than reasoned about in advance: the
/// first run made two pages against this engine's three and the page-size
/// assertion above ruled out the obvious explanation.
///
/// The rewrite is textual and deliberately crude — find `@media screen`, take
/// its balanced block, restate it as `@media print` — because a full media
/// query rewriter in a test file would be a second implementation of the thing
/// under test.
fn as_screen(author: &str) -> String {
    let mut out = author.to_owned();
    let mut from = 0usize;
    while let Some(at) = out[from..].find("@media screen") {
        let start = from + at;
        let Some(open) = out[start..].find('{').map(|i| start + i) else {
            break;
        };
        let mut depth = 0usize;
        let mut close = None;
        for (offset, byte) in out[open..].bytes().enumerate() {
            match byte {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(open + offset);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(close) = close else {
            break;
        };
        let inner = out[open + 1..close].to_owned();
        let restated = format!("\n@media print {{{inner}}}\n");
        out.insert_str(close + 1, &restated);
        from = close + 1 + restated.len();
    }
    out
}

fn between<'a>(text: &'a str, open: &str, close: &str) -> &'a str {
    let Some(start) = text.find(open) else {
        return text;
    };
    let Some(after) = text[start..].find('>').map(|at| start + at + 1) else {
        return text;
    };
    let end = text.rfind(close).unwrap_or(text.len());
    &text[after..end.max(after)]
}

fn run(browser: &Path, arguments: &[String]) -> (bool, String) {
    let out = Command::new(browser)
        .args(arguments)
        .output()
        .expect("the browser runs");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

/// Lays a document out in the browser and reads the measurement back.
///
/// `slot` names the file the page is written to, and it is a parameter rather
/// than a constant because it has to be: `cargo test` runs the tests in this
/// file **in parallel**, and two comparisons sharing one path is one of them
/// measuring the other one's document. That is not a hypothetical — the float
/// comparison below read the corpus chapter on its first run, and the block
/// sequence assertion is what said so.
fn browser_blocks(browser: &Path, slot: &str, html: &str, width: f64) -> Vec<Block> {
    let path = scratch().join(slot);
    std::fs::write(&path, html).expect("the oracle page");
    let url = format!("file:///{}", path.display().to_string().replace('\\', "/"));
    let (ok, dom) = run(
        browser,
        &[
            "--headless=new".to_owned(),
            "--disable-gpu".to_owned(),
            "--no-sandbox".to_owned(),
            format!("--window-size={},8000", width.round() as i64),
            "--virtual-time-budget=8000".to_owned(),
            "--dump-dom".to_owned(),
            url,
        ],
    );
    assert!(ok, "the browser did not run: {dom}");
    let payload = between(&dom, "<pre id=\"tinker-oracle\">", "</pre>");
    assert!(
        !payload.trim().is_empty(),
        "the browser produced no measurement: {}",
        &dom[..dom.len().min(2_000)]
    );
    payload
        .split('\u{2}')
        .filter(|row| !row.trim().is_empty())
        .map(|row| {
            let mut parts = row.split('\u{1}');
            let tag = parts.next().unwrap_or_default().to_owned();
            let top: f64 = parts.next().unwrap_or("0").parse().unwrap_or(0.0);
            let text = unescape(parts.next().unwrap_or_default());
            let left: f64 = parts.next().unwrap_or("0").parse().unwrap_or(0.0);
            Block {
                tag,
                text,
                top,
                left,
            }
        })
        .collect()
}

/// `--dump-dom` hands back the DOM as markup, so the text inside the `<pre>`
/// carries XML's own escapes and has to lose them again.
fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

// ---- this engine's side ---------------------------------------------------------

/// The same document, laid into one column by this engine.
///
/// A page 100 000 pixels tall, which is one page for any chapter in either
/// corpus — the continuous comparison is against a column and not against
/// pages, so fragmentation is deliberately kept out of it. The paginated
/// comparison below is where fragmentation is checked.
fn engine_blocks(document: &str, author: &str, width_px: f64) -> Vec<Block> {
    let (dom, styles, laid) = engine_layout(document, author, width_px, 100_000.0);
    let mut out = Vec::new();
    for (index, node) in dom.nodes.iter().enumerate() {
        let style = &styles.styles[index];
        if !matches!(style.display, Display::Block | Display::ListItem) {
            continue;
        }
        // The element's **own** text, which is what its runs carry: a
        // descendant element's text is anchored to the descendant.
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
        let anchor = index as u32;
        let mine = || {
            laid.pages
                .iter()
                .flat_map(|page| page.runs.iter())
                .filter(|run| run.anchor == Some(anchor) && !run.generated)
        };
        let top = mine().map(|run| run.y).fold(f64::INFINITY, f64::min);
        // The **first** run's left edge and not the leftmost, which are two
        // different numbers the moment a block has more than one line: the
        // browser reports the first client rect, so taking a minimum here would
        // compare a justified block's narrowest line against its first.
        let left = mine()
            .min_by_key(|run| run.order)
            .map_or(f64::INFINITY, |run| run.x);
        if top.is_finite() {
            out.push(Block {
                tag: node.name.clone(),
                text,
                top,
                left,
            });
        }
    }
    out
}

/// The markup, the cascade and the layout, at a page box the caller states.
///
/// **The same code path for both comparisons**, which is the point: the
/// continuous one asks for a page tall enough to hold everything and the
/// paginated one asks for the real page box, and a build whose two answers came
/// from two functions could agree with the browser twice for different reasons.
fn engine_layout(
    document: &str,
    author: &str,
    width_px: f64,
    height_px: f64,
) -> (
    xhtml::Dom,
    tinker_pdf_css::cascade::StyleTree,
    tinker_pdf_layout::Layout,
) {
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
    let author = format!("{author}\n{SAME_FACE}\n");
    let sheet = css_parse(
        author.as_bytes(),
        None,
        &NoImports,
        &media,
        &limits,
        &mut budget,
    )
    .expect("the book's sheet");

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

// ---- the continuous comparison ---------------------------------------------------

/// **The block sequence agrees exactly and the column agrees to within a
/// stated fraction of itself.**
#[test]
fn the_browser_and_this_engine_lay_the_same_column_out_the_same_way() {
    let browser = oracle!("the continuous y-offset comparison");

    let book = corpus_book(BOOK);
    let document = entry(&book, DOCUMENT);
    let author = entry(&book, STYLESHEET);
    // The measure this engine sets at: the default page box less its margins,
    // in CSS pixels, which is the number `OpenOptions::page.0` reaches layout
    // as.
    let width_px = (DEFAULT_PAGE.0 - PAGE_MARGIN * 2.0) / PX_TO_PT;

    let theirs = browser_blocks(
        &browser,
        "continuous.html",
        &oracle_page(&document, &author, Some(width_px)),
        width_px,
    );
    let ours = engine_blocks(&document, &author, width_px);

    assert!(
        ours.len() >= 20,
        "the fixture produced {} block boxes, which is not a chapter",
        ours.len()
    );

    // 1. The sequence, exactly. Printed side by side on failure, because the
    // useful information is *where* the two lists part company.
    let theirs_text: Vec<(&str, &str)> = theirs
        .iter()
        .map(|block| (block.tag.as_str(), block.text.as_str()))
        .collect();
    let ours_text: Vec<(&str, &str)> = ours
        .iter()
        .map(|block| (block.tag.as_str(), block.text.as_str()))
        .collect();
    if theirs_text != ours_text {
        let at = theirs_text
            .iter()
            .zip(&ours_text)
            .position(|(a, b)| a != b)
            .unwrap_or(theirs_text.len().min(ours_text.len()));
        panic!(
            "the browser and this engine disagree about which blocks exist, \
             first at {at} of {}/{}:\n  browser: {:?}\n  ours:    {:?}",
            theirs_text.len(),
            ours_text.len(),
            theirs_text.get(at),
            ours_text.get(at)
        );
    }

    // 2. The positions: each block's offset from the first, as a fraction of
    // **the browser's** column.
    //
    // Dividing each side by its own span was the first thing tried and it is
    // the wrong measure: a defect that shortens every paragraph by the same
    // amount shortens the column by the same proportion, so the normalised
    // positions barely move and the oracle cannot see the one defect gap 31
    // names by name. Dividing both sides by one denominator — the reference
    // implementation's — keeps the number dimensionless and keeps a uniform
    // error visible, which is what the control below then measures.
    //
    // The offset is from the first block rather than from the top of the page
    // because the two sides report different things about that one line: the
    // browser gives a line box's top and this engine gives a baseline. That is
    // a constant per font size, and subtracting the first block removes the
    // constant without removing anything else.
    let offsets = |blocks: &[Block], span: f64| -> Vec<f64> {
        let first = blocks.first().map_or(0.0, |block| block.top);
        blocks
            .iter()
            .map(|block| (block.top - first) / span)
            .collect()
    };
    let span = (theirs.last().map_or(1.0, |block| block.top)
        - theirs.first().map_or(0.0, |block| block.top))
    .max(1.0);
    let theirs_at = offsets(&theirs, span);
    let ours_at = offsets(&ours, span);

    let mut worst = 0.0f64;
    let mut worst_at = 0usize;
    for (at, (a, b)) in theirs_at.iter().zip(&ours_at).enumerate() {
        let gap = (a - b).abs();
        if gap > worst {
            worst = gap;
            worst_at = at;
        }
    }
    println!(
        "  {} blocks, worst deviation {:.4} of the column at block {worst_at} ({:?})",
        ours.len(),
        worst,
        ours[worst_at].tag
    );
    if worst > MAX_COLUMN_DEVIATION || std::env::var_os("TINKER_BROWSER_TABLE").is_some() {
        // The whole column, both sides, because the useful thing about a
        // disagreement is which block it starts at and how much of it was
        // carried forward from the block before.
        for (at, block) in ours.iter().enumerate() {
            println!(
                "    {at:3} {:8} browser {:9.2} ours {:9.2} delta {:+.4}  {}",
                block.tag,
                theirs[at].top,
                block.top,
                theirs_at[at] - ours_at[at],
                &block.text[..block.text.len().min(48)]
            );
        }
    }
    assert!(
        worst <= MAX_COLUMN_DEVIATION,
        "the two columns differ by {worst:.4} at block {worst_at} ({:?}: {:?}), \
         which is past {MAX_COLUMN_DEVIATION}",
        ours[worst_at].tag,
        &ours[worst_at].text[..ours[worst_at].text.len().min(60)]
    );
}

/// And the comparison **can** fail, which is the other half of having one.
///
/// The same document with one margin removed from this engine's side, run
/// through the same comparison. A build whose oracle agreed with everything
/// would pass the test above with the margin rule deleted from `ua.css`, and
/// this is what says it would not: `p { margin: 1em 0 }` is eleven paragraphs'
/// worth of space in this chapter and taking it out moves the column by far
/// more than [`MAX_COLUMN_DEVIATION`].
#[test]
fn the_continuous_comparison_notices_a_dropped_margin() {
    let browser = oracle!("the dropped-margin control");

    let book = corpus_book(BOOK);
    let document = entry(&book, DOCUMENT);
    let author = entry(&book, STYLESHEET);
    let width_px = (DEFAULT_PAGE.0 - PAGE_MARGIN * 2.0) / PX_TO_PT;

    let theirs = browser_blocks(
        &browser,
        "dropped-margin.html",
        &oracle_page(&document, &author, Some(width_px)),
        width_px,
    );
    // The book's own sheet sets `p { margin: 1em 0 }`; overriding it to zero is
    // the defect, injected into the engine's side only.
    let injected = format!("{author}\np {{ margin-top: 0; margin-bottom: 0; }}\n");
    let ours = engine_blocks(&document, &injected, width_px);

    let span = (theirs.last().map_or(1.0, |block| block.top)
        - theirs.first().map_or(0.0, |block| block.top))
    .max(1.0);
    let offsets = |blocks: &[Block]| -> Vec<f64> {
        let first = blocks.first().map_or(0.0, |block| block.top);
        blocks
            .iter()
            .map(|block| (block.top - first) / span)
            .collect()
    };
    let worst = offsets(&theirs)
        .iter()
        .zip(offsets(&ours))
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f64, f64::max);
    println!("  worst deviation with the margin dropped: {worst:.4}");
    assert!(
        worst > MAX_COLUMN_DEVIATION,
        "dropping every paragraph margin moved the column by only {worst:.4}, \
         so the comparison would not have noticed"
    );
}

// ---- the paginated comparison ------------------------------------------------------

/// **The same document, paginated by both, at the same page size.**
///
/// `--print-to-pdf` is the only way to ask a browser for pages, and the PDF it
/// produces is read back through **this repository's own reader** — a
/// comparison no other oracle here can make, and the reason `Page::text()`
/// existing is load-bearing rather than decorative.
///
/// What is asserted is what fragmentation can honestly be held to across two
/// engines with two different faces: the **page count is within one**, and
/// **every page's text is a contiguous piece of the document's text, in
/// order**, on both sides. The second is the strong half — it says neither
/// build lost a paragraph at a page boundary or repeated one across it, which
/// is the fragmentation defect text conservation was built for and the one a
/// page count cannot see.
#[test]
fn the_browser_and_this_engine_fragment_the_same_document_into_the_same_pages() {
    let browser = oracle!("the --print-to-pdf fragmentation comparison");

    let book = corpus_book(BOOK);
    let document = entry(&book, DOCUMENT);
    let author = entry(&book, STYLESHEET);

    // **The browser's own page box, not this engine's**, and the reason is a
    // finding rather than a preference. Chromium's `--print-to-pdf` honours an
    // `@page { size: … }` for the *output* page and lays the document out at
    // its own default box anyway, then scales the result to fit: asked for
    // 432 x 648 points it wrote a 432 x 648 page whose body text is set at
    // **8.69 points** rather than 12, which is 576/792 — this page's height
    // over US Letter's. A page count compared across that scale is a
    // comparison of two different documents, and it was reading the printed
    // PDF's own `Tf` sizes back through this repository's reader that said so.
    //
    // So the comparison is made at the box the browser is not scaling: its
    // default. `@page` states only the margin, and this engine is laid out at
    // the same box the browser then reports.
    let page_rule = format!("@page {{ margin: {PAGE_MARGIN}pt; }}");
    let html = oracle_page(
        &document,
        &format!("{}\n{page_rule}\n", as_screen(&author)),
        None,
    );
    let source = scratch().join("paginated.html");
    std::fs::write(&source, html).expect("the oracle page");
    let out = scratch().join("paginated.pdf");
    let _ = std::fs::remove_file(&out);
    let (ok, log) = run(
        &browser,
        &[
            "--headless=new".to_owned(),
            "--disable-gpu".to_owned(),
            "--no-sandbox".to_owned(),
            "--no-pdf-header-footer".to_owned(),
            "--virtual-time-budget=8000".to_owned(),
            format!("--print-to-pdf={}", out.display()),
            format!(
                "file:///{}",
                source.display().to_string().replace('\\', "/")
            ),
        ],
    );
    assert!(ok, "the browser did not print: {log}");
    let printed = std::fs::read(&out).expect("the browser's PDF");
    let theirs = Document::open(printed).expect("this reader opens the browser's PDF");
    let (page_width, page_height) = theirs.page(0).expect("a printed page").size();

    // And the scale is gone, which is checkable rather than assumed: the body
    // is set at the size the stylesheet asked for. Without this the two page
    // counts below could agree for the wrong reason on a machine whose default
    // paper happened to be six by nine.
    let mut sizes: Vec<i64> = theirs
        .page(0)
        .expect("a page")
        .text()
        .blocks
        .iter()
        .flat_map(|block| block.lines.iter())
        .flat_map(|line| line.chars.iter())
        .map(|character| (character.size * 100.0).round() as i64)
        .collect();
    sizes.sort_unstable();
    sizes.dedup();
    println!("  the browser printed {page_width} x {page_height} points at sizes {sizes:?}");
    assert!(
        sizes.contains(&((DEFAULT_FONT_SIZE * 100.0).round() as i64)),
        "the browser scaled the page: nothing on it is set at {DEFAULT_FONT_SIZE} points, \
         only {sizes:?}"
    );

    // This engine's pagination of the same document, through the same cascade
    // and the same layout the browser was given — including the face and the
    // page box, which is what makes the two page counts comparable at all.
    let width_px = (page_width - PAGE_MARGIN * 2.0) / PX_TO_PT;
    let height_px = (page_height - PAGE_MARGIN * 2.0) / PX_TO_PT;
    let (_, _, mine) = engine_layout(
        &document,
        &format!("{author}\n{SAME_FACE}\n"),
        width_px,
        height_px,
    );

    println!(
        "  the browser paginates it into {} pages and this engine into {}",
        theirs.page_count(),
        mine.pages.len()
    );
    let difference = i64::from(theirs.page_count()) - mine.pages.len() as i64;
    assert!(
        difference.abs() <= 1,
        "the browser makes {} pages of this chapter and this engine {}",
        theirs.page_count(),
        mine.pages.len()
    );

    // And both sides' pages are a contiguous, ordered partition of the same
    // text. Whitespace goes, for `epub_support::conservation`'s reason: layout
    // reflows it by construction and it is the one thing that cannot be
    // conserved.
    let squeeze = |text: &str| -> String { text.chars().filter(|c| !c.is_whitespace()).collect() };
    let their_pages: Vec<String> = (0..theirs.page_count())
        .map(|at| squeeze(&theirs.page(at).expect("a page").text().plain_text()))
        .collect();
    let our_pages: Vec<String> = mine
        .pages
        .iter()
        .map(|page| {
            squeeze(
                &page
                    .runs
                    .iter()
                    .filter(|run| !run.generated)
                    .map(|run| run.text.clone())
                    .collect::<String>(),
            )
        })
        .collect();
    println!(
        "  browser pages carry {:?} characters and this engine's {:?}",
        their_pages.iter().map(String::len).collect::<Vec<_>>(),
        our_pages.iter().map(String::len).collect::<Vec<_>>()
    );

    let joined_theirs: String = their_pages.concat();
    let joined_ours: String = our_pages.concat();
    // The browser's own list markers are in its text layer and this build's are
    // artifacts, so the two strings are not equal — but each side's pages must
    // partition its own text, and the chapter's distinctive sentences must
    // appear exactly once on each side. A paragraph repeated across a page
    // break appears twice; one lost at a page break appears none.
    for phrase in [
        "Acontainerisafilethatisotherfiles",
        "Theorderofthings",
        "Whatgoeswrongquietly",
        "Whereitlives",
    ] {
        assert_eq!(
            joined_ours.matches(phrase).count(),
            1,
            "this engine's pages carry {phrase:?} {} times",
            joined_ours.matches(phrase).count()
        );
        assert_eq!(
            joined_theirs.matches(phrase).count(),
            1,
            "the browser's pages carry {phrase:?} {} times",
            joined_theirs.matches(phrase).count()
        );
    }
}

// ---- the float comparison (milestone 10) ---------------------------------------

/// How far the two columns of the float document may disagree.
///
/// **Itemised, like [`MAX_COLUMN_DEVIATION`], and against the same kind of
/// control.** The measured disagreement is **0.0154 of the column**, and it is
/// one thing rather than a budget:
///
/// | | Of the column |
/// | --- | --- |
/// | The constant [`MAX_COLUMN_DEVIATION`] names — the browser reports a **line box's top** and this engine a **baseline** — here at the 32-pixel `<h1>` the offsets are measured from, so 13.4 pixels of an 867-pixel column | 0.0150 |
/// | Everything else, over sixteen blocks and six figures | 0.0004 |
///
/// The second row is the one worth reading. Once the face and the line height
/// are the same on both sides, **every float in this document is where Chrome
/// put it to within half a pixel** — the deltas printed by
/// `TINKER_BROWSER_TABLE=1` are 0.0150, 0.0151, 0.0152 … all the way down, a
/// constant carried from the first block and never added to.
///
/// The cap is 0.05, three times the measurement, and the **injected defect
/// measures 0.2396** — fifteen times the honest disagreement and five times the
/// cap. That ratio is what makes this an oracle rather than a threshold: see
/// [`the_float_comparison_notices_a_float_that_was_not_floated`].
const MAX_FLOAT_DEVIATION: f64 = 0.05;

/// The float-heavy content document, written for this comparison.
///
/// **Every float has a stated `width`.** Shrink-to-fit is implemented and has
/// its own fixture one crate down, but a browser's preferred-width calculation
/// and this build's differ by a fraction of a character at every figure, and a
/// comparison that folded that into its tolerance would be measuring its own
/// font metrics rather than its float placement. What is compared here is
/// *placement*: the same figure of the same size, on the same side, in the same
/// column of text.
///
/// It holds the arrangements §9.5.1's rules distinguish: a left float with text
/// beside it, a right one, two narrow enough to sit side by side, two that are
/// not, and a `clear` after them all.
const FLOAT_DOCUMENT: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n",
    "<html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>Figures</title></head><body>\n",
    "<h1>The figures and the text around them</h1>\n",
    "<p>The first paragraph is set at the full measure and has no figure beside it at all, ",
    "which is what makes the second one worth measuring against it.</p>\n",
    "<div class=\"fl\">Figure one, on the left, with a caption long enough to take several ",
    "lines of its own.</div>\n",
    "<p>This paragraph runs down the right of the first figure and its lines are shortened ",
    "for as long as the figure lasts, and then they are not, which is the whole of the ",
    "interaction between a float and a line box.</p>\n",
    "<p>A second paragraph, which begins beside the figure if the first one did not finish ",
    "it and at the full measure otherwise.</p>\n",
    "<div class=\"fr\">Figure two, on the right, with a caption of its own.</div>\n",
    "<p>The text beside a right float is shortened from the other side, and a build that ",
    "shortened it from the left would put every line of this paragraph in the wrong place ",
    "while keeping every line count right.</p>\n",
    "<div class=\"fl narrow\">Three</div>\n",
    "<div class=\"fl narrow\">Four</div>\n",
    "<p>Figures three and four are narrow enough to sit side by side, and this paragraph ",
    "goes beside both of them.</p>\n",
    "<div class=\"fl wide\">Figure five is wide.</div>\n",
    "<div class=\"fl wide\">Figure six is wide too, and cannot sit beside five.</div>\n",
    "<p>Five and six cannot share a line, so six is under five and this paragraph is beside ",
    "whichever of them it reaches.</p>\n",
    "<p class=\"cl\">This paragraph clears both sides and starts below every figure above ",
    "it, whatever the paragraphs before it did.</p>\n",
    "<h2>After the clearance</h2>\n",
    "<p>A closing paragraph at the full measure, which is where the two columns have to ",
    "agree again if they agreed anywhere.</p>\n",
    "</body></html>\n"
);

/// The float document's own stylesheet, given to both sides unchanged.
///
/// **`line-height` is stated, and that is the second variable held fixed.**
/// [`SAME_FACE`] holds the font; this holds what `line-height: normal` means.
/// This build resolves it as 1.2 — the figure every specification example uses
/// — and a browser resolves it from the face's own metrics, which for Courier
/// New is 1.133. The difference is six per cent of *every* line in the
/// document, it accumulates down the column, and it is nothing whatever to do
/// with where a float went: measured without this rule the two columns
/// disagreed by 0.0449, of which the whole systematic part is that ratio.
/// Stating it leaves the comparison about §9.5.1, which is what it is for.
///
/// It is a rule **on both sides**, like the face — the same declaration reaches
/// the browser through [`oracle_page`] and this engine through the cascade —
/// and [`the_float_comparison_notices_a_float_that_was_not_floated`] is what
/// says the result can still fail.
const FLOAT_STYLESHEET: &str = concat!(
    "* { line-height: 1.2; }\n",
    ".fl { float: left; width: 180px; margin: 0 16px 8px 0; }\n",
    ".fr { float: right; width: 180px; margin: 0 0 8px 16px; }\n",
    ".narrow { width: 90px; }\n",
    ".wide { width: 260px; }\n",
    ".cl { clear: both; }\n",
    "p { margin: 8px 0; }\n"
);

/// **The float-heavy column, block by block, against the browser's.**
///
/// The same two assertions as the continuous comparison — the block sequence
/// exactly, then the offsets to within a stated fraction — over a document
/// whose every block is placed by §9.5.1 rather than by §9.4.1.
#[test]
fn the_browser_and_this_engine_place_the_same_floats_in_the_same_column() {
    let browser = oracle!("the float-heavy y-offset comparison");
    let width_px = (DEFAULT_PAGE.0 - PAGE_MARGIN * 2.0) / PX_TO_PT;

    let theirs = browser_blocks(
        &browser,
        "floats.html",
        &oracle_page(FLOAT_DOCUMENT, FLOAT_STYLESHEET, Some(width_px)),
        width_px,
    );
    let ours = engine_blocks(FLOAT_DOCUMENT, FLOAT_STYLESHEET, width_px);

    assert!(
        ours.len() >= 12,
        "the fixture produced {} block boxes",
        ours.len()
    );
    let theirs_text: Vec<(&str, &str)> = theirs
        .iter()
        .map(|block| (block.tag.as_str(), block.text.as_str()))
        .collect();
    let ours_text: Vec<(&str, &str)> = ours
        .iter()
        .map(|block| (block.tag.as_str(), block.text.as_str()))
        .collect();
    assert_eq!(
        theirs_text, ours_text,
        "the browser and this engine disagree about which blocks exist"
    );

    let worst = deviation(&theirs, &ours, true);
    println!("  worst float deviation {worst:.4} of the column");
    assert!(
        worst <= MAX_FLOAT_DEVIATION,
        "the two float columns differ by {worst:.4}, which is past {MAX_FLOAT_DEVIATION}"
    );
}

/// And the float comparison **can** fail: the same document with the floats
/// taken out of this engine's side.
///
/// `float: none` is not an arbitrary defect. It is what this build did until
/// milestone 10 — every figure in the flow, every paragraph pushed down past
/// it — and it is the exact failure the milestone exists to end. The number it
/// produces is what the tolerance above is judged against.
#[test]
fn the_float_comparison_notices_a_float_that_was_not_floated() {
    let browser = oracle!("the float control");
    let width_px = (DEFAULT_PAGE.0 - PAGE_MARGIN * 2.0) / PX_TO_PT;

    let theirs = browser_blocks(
        &browser,
        "float-control.html",
        &oracle_page(FLOAT_DOCUMENT, FLOAT_STYLESHEET, Some(width_px)),
        width_px,
    );
    let injected = format!("{FLOAT_STYLESHEET}\n.fl, .fr {{ float: none; }}\n");
    let ours = engine_blocks(FLOAT_DOCUMENT, &injected, width_px);

    let worst = deviation(&theirs, &ours, false);
    println!("  worst deviation with the floats unfloated: {worst:.4}");
    assert!(
        worst > MAX_FLOAT_DEVIATION,
        "taking the floats out moved the column by only {worst:.4}, \
         so the comparison would not have noticed"
    );
}

/// How far the table-heavy column may disagree with the browser's.
///
/// **The measured disagreement is 0.0005 of the column**, which is the closest
/// agreement any of the three comparisons in this file reaches and is what
/// happens when the variables neither build shares are stated rather than
/// tolerated: the face, `line-height`, `vertical-align` and the heading size
/// are all held, and what is left is §17.5.2's column widths, §17.5's row
/// heights, §17.2.1's generated boxes and §17.2's rendering order.
///
/// The cap is 0.02, and it is not three times the measurement — it is **one
/// line of this column**, which is 19.2 px of 1 199 and therefore 0.016. A
/// comparison whose cap were tighter than a line would fail the first time a
/// browser wrapped one cell one word differently, and one whose cap were looser
/// would admit a whole row. The **injected defect measures 0.1245** — six times
/// the cap and two hundred and forty-nine times the measurement. See
/// [`the_table_comparison_notices_a_cell_that_was_not_a_cell`].
const MAX_TABLE_DEVIATION: f64 = 0.02;

/// The table-heavy content document, written for this comparison.
///
/// **Every cell holds a `<p>`**, and that is not decoration. Both sides of this
/// oracle report `display: block` and `display: list-item` boxes and nothing
/// else — a `table-cell` is neither — so a document whose cells held bare text
/// would be compared on the blocks *around* the table and would agree with a
/// build that had no table model at all. The paragraph inside each cell is what
/// puts the cell's own content into the comparison: its `top` is where the
/// table put the cell, so a column of the wrong width, a row of the wrong
/// height, a `colspan` in the wrong place and a missing row group each move it.
///
/// It holds what §17 distinguishes: a `<caption>`, a `<thead>` and a `<tbody>`,
/// **a second table with no `<tbody>` at all** — which is what every table in
/// the committed corpus looks like and is §17.2.1's whole reason for existing —
/// a `colspan`, a `rowspan`, a `<tfoot>` written before the body it is drawn
/// under, a collapsing-border table beside a separated one, and a nested table
/// inside a cell.
const TABLE_DOCUMENT: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n",
    "<html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>Tables</title></head><body>\n",
    "<h1>The tables and the text around them</h1>\n",
    "<p>The first paragraph is set at the full measure and has no table beside it, ",
    "which is what makes the ones after it worth measuring against it.</p>\n",
    "<table class=\"fixed\">\n",
    "<caption><p>A caption, which §17.4 sets above the table it belongs to.</p></caption>\n",
    "<thead><tr><th><p>Heading one</p></th><th><p>Heading two</p></th>",
    "<th><p>Heading three</p></th></tr></thead>\n",
    "<tbody>\n",
    "<tr><td><p>A cell whose text is long enough to take more than one line at this ",
    "column width, which is what makes the row height depend on the column width.</p></td>",
    "<td><p>Short</p></td><td><p>Also short</p></td></tr>\n",
    "<tr><td colspan=\"2\"><p>A cell spanning two columns, which is wider than either of ",
    "them and therefore wraps at a different place.</p></td><td><p>Third</p></td></tr>\n",
    "<tr><td rowspan=\"2\"><p>A cell spanning two rows, so the row below it starts one ",
    "column to the right.</p></td><td><p>Second column</p></td><td><p>Third column</p></td></tr>\n",
    "<tr><td><p>Under the second</p></td><td><p>Under the third</p></td></tr>\n",
    "</tbody>\n",
    "</table>\n",
    "<p>A paragraph between the two tables, at the full measure again.</p>\n",
    "<table class=\"bare\">\n",
    "<tr><td><p>This table has no tbody in it at all, which is what every table in the ",
    "committed corpus looks like.</p></td><td><p>Second</p></td></tr>\n",
    "<tr><td><p>A second bare row</p></td><td><p>And its neighbour</p></td></tr>\n",
    "</table>\n",
    "<p>A paragraph before the collapsing table.</p>\n",
    "<table class=\"collapse\">\n",
    "<tfoot><tr><td><p>A footer written before the body</p></td><td><p>and drawn under it",
    "</p></td></tr></tfoot>\n",
    "<tbody><tr><td><p>The body of the collapsing table</p></td><td><p>with a second cell",
    "</p></td></tr></tbody>\n",
    "</table>\n",
    "<p>A paragraph before the nested table.</p>\n",
    "<table class=\"bare\">\n",
    "<tr><td><table class=\"bare inner\"><tr><td><p>Inner one</p></td><td><p>Inner two</p>",
    "</td></tr></table></td><td><p>Beside the nested table</p></td></tr>\n",
    "</table>\n",
    "<h2>After the tables</h2>\n",
    "<p>A closing paragraph at the full measure, which is where the two columns have to ",
    "agree again if they agreed anywhere.</p>\n",
    "</body></html>\n"
);

/// The table document's own stylesheet, given to both sides unchanged.
///
/// **`line-height` is stated for the float comparison's reason** — this build
/// resolves `normal` as 1.2 and a browser resolves it from Courier New's own
/// metrics as 1.133, which is six per cent of every line in the document and
/// nothing to do with §17.
///
/// Everything a browser's own user-agent sheet says about a table is restated
/// here at a specificity that wins, because this comparison is about §17.5.2
/// and §17.6 rather than about whose default `border-spacing` is which:
/// `table-layout`, `width`, `border-collapse`, `border-spacing` and the cell
/// padding are all stated, and the paragraphs inside the cells have their
/// margins taken off so that a cell's height is its text's.
const TABLE_STYLESHEET: &str = concat!(
    "* { line-height: 1.2; }\n",
    // **The third variable held fixed, and it is `vertical-align`.** HTML's own
    // user-agent sheet puts `vertical-align: middle` on a table and `inherit`
    // on a cell, so every browser centres a short cell in a tall row. This
    // build has no §17.5.4 at all -- `vertical-align` is `Unsupported` by name
    // and is the largest single gap the committed corpus measures, at
    // thirty-four elements -- so every cell here is set from its top. Stating
    // `top` on both sides leaves the comparison about §17.5.2's column widths
    // and §17.6's borders, which is what it is for. Without it this fixture
    // measures a gap that is already counted, and measures it at 0.1070 --
    // larger than the injected defect the oracle exists to catch, which is the
    // definition of an oracle whose noise floor is its own defect.
    "td, th { vertical-align: top; }\n",
    // And the fourth: the two sides report different things about a block's
    // first line -- the browser a line box's top and this engine a baseline --
    // and the difference is a constant per font size. `deviation` cancels the
    // constant by subtracting the first block, which works only while every
    // block shares one. A heading at 2em does not, and it is worth 0.0104 of
    // this column on its own.
    "h1, h2 { font-size: 1em; margin: 8px 0; }\n",
    "table { border-collapse: separate; border-spacing: 4px; margin: 8px 0; }\n",
    "td, th { padding: 2px; }\n",
    "td p, th p, caption p { margin: 0; }\n",
    "th { font-weight: normal; text-align: left; }\n",
    "p { margin: 8px 0; }\n",
    ".fixed { table-layout: fixed; width: 380px; }\n",
    ".bare { table-layout: fixed; width: 380px; }\n",
    ".inner { width: 180px; }\n",
    ".collapse { border-collapse: collapse; table-layout: fixed; width: 380px; }\n",
    ".collapse td { border: 1px solid #888888; }\n"
);

/// **The table-heavy column, block by block, against the browser's.**
///
/// The same two assertions as the continuous and float comparisons — the block
/// sequence exactly, then the offsets to within a stated fraction — over a
/// document whose every block below the first is placed by §17 rather than by
/// §9.4.1.
///
/// The sequence assertion is the sharper half here and it is worth saying why:
/// it is an *ordered* list, and §17.2's rendering order is not document order.
/// Both sides walk the document, so the `<tfoot>` written before its `<tbody>`
/// is reported before it on both — and the *offsets* are then what say it was
/// drawn under it. A build that emitted the footer where it was written would
/// pass the sequence and fail the offsets, which is exactly the split the two
/// assertions exist for.
#[test]
fn the_browser_and_this_engine_lay_the_same_tables_out_the_same_way() {
    let browser = oracle!("the table-heavy y-offset comparison");
    let width_px = (DEFAULT_PAGE.0 - PAGE_MARGIN * 2.0) / PX_TO_PT;

    let theirs = browser_blocks(
        &browser,
        "tables.html",
        &oracle_page(TABLE_DOCUMENT, TABLE_STYLESHEET, Some(width_px)),
        width_px,
    );
    let ours = engine_blocks(TABLE_DOCUMENT, TABLE_STYLESHEET, width_px);

    assert!(
        ours.len() >= 25,
        "the fixture produced {} block boxes, which is not a table-heavy \
         document",
        ours.len()
    );
    let theirs_text: Vec<(&str, &str)> = theirs
        .iter()
        .map(|block| (block.tag.as_str(), block.text.as_str()))
        .collect();
    let ours_text: Vec<(&str, &str)> = ours
        .iter()
        .map(|block| (block.tag.as_str(), block.text.as_str()))
        .collect();
    if theirs_text != ours_text {
        let at = theirs_text
            .iter()
            .zip(&ours_text)
            .position(|(a, b)| a != b)
            .unwrap_or(theirs_text.len().min(ours_text.len()));
        panic!(
            "the browser and this engine disagree about which blocks exist, \
             first at {at} of {}/{}:\n  browser: {:?}\n  ours:    {:?}",
            theirs_text.len(),
            ours_text.len(),
            theirs_text.get(at),
            ours_text.get(at)
        );
    }

    let worst = deviation(&theirs, &ours, true);
    println!("  worst table deviation {worst:.4} of the column");
    assert!(
        worst <= MAX_TABLE_DEVIATION,
        "the two table columns differ by {worst:.4}, which is past \
         {MAX_TABLE_DEVIATION}"
    );
}

/// And the table comparison **can** fail: the same document with the cells laid
/// out as blocks.
///
/// `display: block` on the cells is not an arbitrary defect. It is one of the
/// two things this build could have done before milestone 11 — the other being
/// to set them as inline text, which is what it actually did — and it is the
/// failure that produces a page holding every word in the right order, one
/// cell under another, with nothing anywhere saying a table was meant. The
/// number it produces is what the tolerance above is judged against.
#[test]
fn the_table_comparison_notices_a_cell_that_was_not_a_cell() {
    let browser = oracle!("the table control");
    let width_px = (DEFAULT_PAGE.0 - PAGE_MARGIN * 2.0) / PX_TO_PT;

    let theirs = browser_blocks(
        &browser,
        "table-control.html",
        &oracle_page(TABLE_DOCUMENT, TABLE_STYLESHEET, Some(width_px)),
        width_px,
    );
    let injected = format!("{TABLE_STYLESHEET}\ntd, th {{ display: block; }}\n");
    let ours = engine_blocks(TABLE_DOCUMENT, &injected, width_px);

    let worst = deviation(&theirs, &ours, false);
    println!("  worst deviation with the cells unceiled: {worst:.4}");
    assert!(
        worst > MAX_TABLE_DEVIATION,
        "laying the cells out as blocks moved the column by only {worst:.4}, \
         so the comparison would not have noticed"
    );
}

/// The worst disagreement between two columns, as a fraction of the browser's.
///
/// The same measure the continuous comparison makes, written once so that a
/// test and its control cannot drift apart: a control computed by a second copy
/// of this arithmetic could pass while the test it certifies measured something
/// else.
fn deviation(theirs: &[Block], ours: &[Block], table: bool) -> f64 {
    let span = (theirs.last().map_or(1.0, |block| block.top)
        - theirs.first().map_or(0.0, |block| block.top))
    .max(1.0);
    let offsets = |blocks: &[Block]| -> Vec<f64> {
        let first = blocks.first().map_or(0.0, |block| block.top);
        blocks
            .iter()
            .map(|block| (block.top - first) / span)
            .collect()
    };
    let theirs_at = offsets(theirs);
    let ours_at = offsets(ours);
    let mut worst = 0.0f64;
    for (at, (a, b)) in theirs_at.iter().zip(&ours_at).enumerate() {
        let gap = (a - b).abs();
        if gap > worst {
            worst = gap;
        }
        if table && std::env::var_os("TINKER_BROWSER_TABLE").is_some() {
            println!(
                "    {at:3} {:8} browser {:9.2} ours {:9.2} delta {:+.4}  {}",
                ours[at].tag,
                theirs[at].top,
                ours[at].top,
                a - b,
                &ours[at].text[..ours[at].text.len().min(40)]
            );
        }
    }
    worst
}

// ---- the flex comparison ---------------------------------------------------

/// How far the flex document's two columns may disagree, in each axis.
///
/// **Two numbers measured and one cap, and the two numbers are not alike:**
///
/// | | Of the measure (x) | Of the column (y) |
/// | --- | --- | --- |
/// | The measured disagreement over 46 blocks and eighteen containers | **0.0000** | **0.0004** |
/// | `display: block` on every container — what this build did until milestone 12 | 0.8000 | 0.2068 |
/// | `justify-content` read as its initial value, every container still flexed | 0.6333 | 0.0004 |
///
/// The x row of the first line is exact rather than rounded: every item in this
/// fixture is where Chrome put it to the two decimal places the browser
/// reports, which is what happens when §9.7's loop, §4.5's automatic minimum
/// and §8.2's six distributions are implemented rather than approximated. The
/// y row's 0.0004 is one twenty-fifth of a line, carried from the first
/// container and never added to.
///
/// The cap is 0.02 and it is **one line of this column**: `line-height: 1.2` at
/// 16 px is 19.2 px of the 984 px the browser's column spans, which is 0.0195.
/// A cap tighter than a line would fail the first time a browser wrapped one
/// item's text one word differently; a looser one would admit a whole flex
/// line. [`MAX_TABLE_DEVIATION`] is set by the same rule.
///
/// The third row is the one this file could not have measured before milestone
/// 12: it is a real defect that moves **nothing** down the column, and it is
/// why [`Block::left`] exists. See
/// [`the_flex_comparison_notices_a_container_that_was_not_flexed`] and
/// [`the_flex_comparison_notices_an_ignored_justify_content`], which fail if
/// either ratio stops holding.
const MAX_FLEX_DEVIATION: f64 = 0.02;

/// The flex-heavy content document, written for this comparison.
///
/// **Every item holds a `<p>`**, for [`TABLE_DOCUMENT`]'s reason: both sides of
/// this oracle report `display: block` and `display: list-item` boxes and
/// nothing else, and a flex *container* is neither — so a document whose items
/// held bare text would be compared on the blocks around the containers and
/// would agree with a build that had no flex model at all. The paragraph inside
/// each item is what puts the item's own position into the comparison.
///
/// **Every item is a `<div>` and none is a `<span>`.** `css-flexbox-1` §4
/// blockifies an item's `display`, and Chrome reports the blockified value from
/// `getComputedStyle` while this engine's cascade still reports `inline` — so a
/// `<span>` item would be in one side's block list and not the other's, and the
/// sequence assertion would fail for a reason that is not a layout fault.
///
/// It holds what §5 to §9 distinguish, one container each and **never all at
/// their initial values**: milestone 10's float section was built entirely of
/// left floats and three rules' right-hand halves were never tested, and the
/// flex equivalent is a fixture where every container is `row`, `nowrap` and
/// `flex-start`.
const FLEX_DOCUMENT: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n",
    "<html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>Flex</title></head><body>\n",
    "<h1>The flex containers and the text around them</h1>\n",
    "<p>The first paragraph is set at the full measure and has no container beside it, ",
    "which is what makes the ones after it worth measuring against it.</p>\n",
    // §9.7 growing: three items sharing the measure in the ratio 1:2:1.
    "<div class=\"row\"><div class=\"g1\"><p>Grow one</p></div>",
    "<div class=\"g2\"><p>Grow two</p></div><div class=\"g1\"><p>Grow three</p></div></div>\n",
    // §9.7 shrinking, scaled by the base size: 300 and 100 into 380.
    "<div class=\"row\"><div class=\"wide\"><p>Wide and shrinking</p></div>",
    "<div class=\"narrow\"><p>Narrow and shrinking</p></div></div>\n",
    // §9.2 step 3: `flex-basis` beating a stated `width`.
    "<div class=\"row\"><div class=\"basis\"><p>Basis fifty</p></div>",
    "<div class=\"fixed\"><p>Fixed one twenty</p></div></div>\n",
    // §8.2, each of the six values.
    "<div class=\"row start\"><div class=\"fixed\"><p>Start one</p></div>",
    "<div class=\"fixed\"><p>Start two</p></div></div>\n",
    "<div class=\"row end\"><div class=\"fixed\"><p>End one</p></div>",
    "<div class=\"fixed\"><p>End two</p></div></div>\n",
    "<div class=\"row centre\"><div class=\"fixed\"><p>Centre one</p></div>",
    "<div class=\"fixed\"><p>Centre two</p></div></div>\n",
    "<div class=\"row between\"><div class=\"fixed\"><p>Between one</p></div>",
    "<div class=\"fixed\"><p>Between two</p></div></div>\n",
    "<div class=\"row around\"><div class=\"fixed\"><p>Around one</p></div>",
    "<div class=\"fixed\"><p>Around two</p></div></div>\n",
    "<div class=\"row evenly\"><div class=\"fixed\"><p>Evenly one</p></div>",
    "<div class=\"fixed\"><p>Evenly two</p></div></div>\n",
    // §5.1's reversed row: the first item goes on the right.
    "<div class=\"row reverse\"><div class=\"fixed\"><p>Reverse first</p></div>",
    "<div class=\"fixed\"><p>Reverse second</p></div></div>\n",
    // §5.2's wrapping, and §5.2's reversed wrapping.
    "<div class=\"row wrap\"><div class=\"fixed\"><p>Wrap one</p></div>",
    "<div class=\"fixed\"><p>Wrap two</p></div><div class=\"fixed\"><p>Wrap three</p></div>",
    "<div class=\"fixed\"><p>Wrap four</p></div></div>\n",
    "<div class=\"row wrapback\"><div class=\"fixed\"><p>Back one</p></div>",
    "<div class=\"fixed\"><p>Back two</p></div><div class=\"fixed\"><p>Back three</p></div>",
    "<div class=\"fixed\"><p>Back four</p></div></div>\n",
    // §8.3 and §8.3's per-item override, against a tall neighbour.
    "<div class=\"row bottom\"><div class=\"fixed tall\"><p>Tall neighbour</p></div>",
    "<div class=\"fixed\"><p>Aligned to the end</p></div>",
    "<div class=\"fixed middle\"><p>Aligned to the centre</p></div></div>\n",
    // §8.4 over two lines in a container with a stated height.
    "<div class=\"row wrap deep\"><div class=\"fixed\"><p>Content one</p></div>",
    "<div class=\"fixed\"><p>Content two</p></div><div class=\"fixed\"><p>Content three</p></div>",
    "<div class=\"fixed\"><p>Content four</p></div></div>\n",
    // §5.4: the last item written is the first item drawn.
    "<div class=\"row\"><div class=\"fixed\"><p>Written first</p></div>",
    "<div class=\"fixed\"><p>Written second</p></div>",
    "<div class=\"fixed first\"><p>Written third</p></div></div>\n",
    // §5.1's column, which is the axis every fixture above holds fixed.
    "<div class=\"column\"><div><p>Column one</p></div><div><p>Column two</p></div>",
    "<div><p>Column three</p></div></div>\n",
    "<h2>After the containers</h2>\n",
    "<p>A closing paragraph at the full measure, which is where the two columns have to ",
    "agree again if they agreed anywhere.</p>\n",
    "</body></html>\n"
);

/// The flex document's own stylesheet, given to both sides unchanged.
///
/// The same four variables the table comparison holds fixed are held fixed
/// here, for the same reasons and with the same consequences if they are not:
/// the face ([`SAME_FACE`]), `line-height` — this build resolves `normal` as
/// 1.2 and Chrome resolves it from Courier New's metrics as 1.133, which is six
/// per cent of every line — the heading size, so `deviation`'s subtraction of
/// the first block cancels one constant rather than two, and the paragraph
/// margins inside the items, so an item's height is its text's.
///
/// **`min-width: 0` is deliberately *not* stated**, which is the one variable
/// this fixture leaves free on purpose. `css-flexbox-1` §4.5's automatic
/// minimum size is implemented here and is exactly the kind of clause an
/// implementation skips; stating `min-width: 0` on the items would neutralise
/// it on both sides and the shrinking container would then agree for the wrong
/// reason.
const FLEX_STYLESHEET: &str = concat!(
    "* { line-height: 1.2; }\n",
    "h1, h2 { font-size: 1em; margin: 8px 0; }\n",
    "p { margin: 8px 0; }\n",
    "div p { margin: 0; }\n",
    ".row { display: flex; margin: 8px 0; }\n",
    ".column { display: flex; flex-direction: column; margin: 8px 0; }\n",
    ".g1 { flex: 1 1 0px; }\n",
    ".g2 { flex: 2 1 0px; }\n",
    ".wide { flex: 0 1 300px; }\n",
    ".narrow { flex: 0 1 100px; }\n",
    ".basis { width: 150px; flex: 0 0 50px; }\n",
    ".fixed { flex: 0 0 80px; }\n",
    ".start { justify-content: flex-start; }\n",
    ".end { justify-content: flex-end; }\n",
    ".centre { justify-content: center; }\n",
    ".between { justify-content: space-between; }\n",
    ".around { justify-content: space-around; }\n",
    ".evenly { justify-content: space-evenly; }\n",
    ".reverse { flex-direction: row-reverse; }\n",
    ".wrap { flex-wrap: wrap; }\n",
    ".wrapback { flex-wrap: wrap-reverse; }\n",
    ".bottom { align-items: flex-end; }\n",
    ".tall { height: 60px; }\n",
    ".middle { align-self: center; }\n",
    ".deep { height: 120px; align-content: space-between; }\n",
    ".first { order: -1; }\n"
);

/// The worst disagreement between two flex layouts, in **both** axes.
///
/// Written once so that the comparison and its control cannot drift apart, for
/// [`deviation`]'s reason — and separate from `deviation` rather than a
/// parameter on it because the two denominators are different quantities: a
/// `y` offset is a fraction of the browser's own column and an `x` offset is a
/// fraction of the measure, which is the same on both sides by construction.
///
/// The first block's `top` is subtracted from both sides, cancelling the
/// baseline-against-line-box constant, and `left` is **not**: there is no such
/// constant in the inline axis, and subtracting the first block's `left` would
/// hide a container that put its whole line in the wrong place.
fn flex_deviation(theirs: &[Block], ours: &[Block], measure: f64) -> (f64, f64) {
    let span = (theirs.last().map_or(1.0, |block| block.top)
        - theirs.first().map_or(0.0, |block| block.top))
    .max(1.0);
    let their_first = theirs.first().map_or(0.0, |block| block.top);
    let our_first = ours.first().map_or(0.0, |block| block.top);
    let mut worst_y = 0.0f64;
    let mut worst_x = 0.0f64;
    for (at, (a, b)) in theirs.iter().zip(ours).enumerate() {
        let dy = ((a.top - their_first) - (b.top - our_first)).abs() / span;
        let dx = (a.left - b.left).abs() / measure;
        worst_y = worst_y.max(dy);
        worst_x = worst_x.max(dx);
        if std::env::var_os("TINKER_BROWSER_FLEX").is_some() {
            println!(
                "    {at:3} {:6} browser ({:8.2}, {:8.2}) ours ({:8.2}, {:8.2}) \
                 dx {dx:+.4} dy {dy:+.4}  {}",
                b.tag,
                a.left,
                a.top,
                b.left,
                b.top,
                &b.text[..b.text.len().min(32)]
            );
        }
    }
    (worst_x, worst_y)
}

/// **The flex-heavy document, block by block, against the browser's — in two
/// axes.**
///
/// The same two assertions as the other three comparisons, plus the one this
/// specification needs: the block sequence exactly, the *y* offsets to within a
/// stated fraction, and the *x* offsets to within a stated fraction of the
/// measure.
#[test]
fn the_browser_and_this_engine_lay_the_same_flex_containers_out_the_same_way() {
    let browser = oracle!("the flex-heavy two-axis comparison");
    let width_px = (DEFAULT_PAGE.0 - PAGE_MARGIN * 2.0) / PX_TO_PT;

    let theirs = browser_blocks(
        &browser,
        "flex.html",
        &oracle_page(FLEX_DOCUMENT, FLEX_STYLESHEET, Some(width_px)),
        width_px,
    );
    let ours = engine_blocks(FLEX_DOCUMENT, FLEX_STYLESHEET, width_px);

    assert!(
        ours.len() >= 35,
        "the fixture produced {} block boxes, which is not a flex-heavy document",
        ours.len()
    );
    let theirs_text: Vec<(&str, &str)> = theirs
        .iter()
        .map(|block| (block.tag.as_str(), block.text.as_str()))
        .collect();
    let ours_text: Vec<(&str, &str)> = ours
        .iter()
        .map(|block| (block.tag.as_str(), block.text.as_str()))
        .collect();
    if theirs_text != ours_text {
        let at = theirs_text
            .iter()
            .zip(&ours_text)
            .position(|(a, b)| a != b)
            .unwrap_or(theirs_text.len().min(ours_text.len()));
        panic!(
            "the browser and this engine disagree about which blocks exist, \
             first at {at} of {}/{}:\n  browser: {:?}\n  ours:    {:?}",
            theirs_text.len(),
            ours_text.len(),
            theirs_text.get(at),
            ours_text.get(at)
        );
    }

    let (worst_x, worst_y) = flex_deviation(&theirs, &ours, width_px);
    println!("  worst flex deviation: x {worst_x:.4}, y {worst_y:.4} of the column");
    assert!(
        worst_x <= MAX_FLEX_DEVIATION,
        "the two flex layouts differ across the measure by {worst_x:.4}, which \
         is past {MAX_FLEX_DEVIATION}"
    );
    assert!(
        worst_y <= MAX_FLEX_DEVIATION,
        "the two flex layouts differ down the column by {worst_y:.4}, which is \
         past {MAX_FLEX_DEVIATION}"
    );
}

/// And the flex comparison **can** fail: the same document with the containers
/// laid out as ordinary blocks.
///
/// `display: block` on every container is not an arbitrary defect. It is
/// exactly what this build did until milestone 12 — every item on a line of its
/// own, one under another, in a column that reads correctly and looks nothing
/// like the page the author wrote — and it is the failure the milestone exists
/// to end. The number it produces is what [`MAX_FLEX_DEVIATION`] is judged
/// against.
#[test]
fn the_flex_comparison_notices_a_container_that_was_not_flexed() {
    let browser = oracle!("the flex control");
    let width_px = (DEFAULT_PAGE.0 - PAGE_MARGIN * 2.0) / PX_TO_PT;

    let theirs = browser_blocks(
        &browser,
        "flex-control.html",
        &oracle_page(FLEX_DOCUMENT, FLEX_STYLESHEET, Some(width_px)),
        width_px,
    );
    let injected = format!("{FLEX_STYLESHEET}\n.row, .column {{ display: block; }}\n");
    let ours = engine_blocks(FLEX_DOCUMENT, &injected, width_px);

    let (worst_x, worst_y) = flex_deviation(&theirs, &ours, width_px);
    println!("  worst deviation with the containers unflexed: x {worst_x:.4}, y {worst_y:.4}");
    assert!(
        worst_y > MAX_FLEX_DEVIATION,
        "laying the containers out as blocks moved the column by only \
         {worst_y:.4}, so the comparison would not have noticed"
    );
    assert!(
        worst_x > MAX_FLEX_DEVIATION,
        "and it moved nothing across the measure ({worst_x:.4}), which is the \
         half a y-only oracle cannot see"
    );
}

/// And the second half of the same question: a build that laid the containers
/// out as flex containers and **ignored `justify-content`** moves nothing down
/// the column at all.
///
/// This is the control the other three comparisons in this file could not have:
/// [`Block::left`] exists for it. Every container in the fixture keeps its flex
/// layout and every `justify-content` declaration is replaced by the initial
/// value, and what fails is the *x* assertion alone.
#[test]
fn the_flex_comparison_notices_an_ignored_justify_content() {
    let browser = oracle!("the justify-content control");
    let width_px = (DEFAULT_PAGE.0 - PAGE_MARGIN * 2.0) / PX_TO_PT;

    let theirs = browser_blocks(
        &browser,
        "flex-justify.html",
        &oracle_page(FLEX_DOCUMENT, FLEX_STYLESHEET, Some(width_px)),
        width_px,
    );
    let injected = format!("{FLEX_STYLESHEET}\n.row, .column {{ justify-content: flex-start; }}\n");
    let ours = engine_blocks(FLEX_DOCUMENT, &injected, width_px);

    let (worst_x, worst_y) = flex_deviation(&theirs, &ours, width_px);
    println!("  worst deviation with justify-content ignored: x {worst_x:.4}, y {worst_y:.4}");
    assert!(
        worst_x > MAX_FLEX_DEVIATION,
        "ignoring justify-content moved the items across the measure by only \
         {worst_x:.4}"
    );
    assert!(
        worst_y <= MAX_FLEX_DEVIATION,
        "and it is invisible down the column ({worst_y:.4}), which is what says \
         a y-only oracle would have passed it"
    );
}
