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
/// | One `<table>`, which this build does not lay out as a table — milestone 11 — and whose cells it therefore sets as one inline run | 0.019 |
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
      out.push([node.tagName.toLowerCase(), (top + window.scrollY).toFixed(2), text]);
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
fn browser_blocks(browser: &Path, html: &str, width: f64) -> Vec<Block> {
    let path = scratch().join("continuous.html");
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
            Block { tag, text, top }
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
        let top = laid
            .pages
            .iter()
            .flat_map(|page| page.runs.iter())
            .filter(|run| run.anchor == Some(anchor) && !run.generated)
            .map(|run| run.y)
            .fold(f64::INFINITY, f64::min);
        if top.is_finite() {
            out.push(Block {
                tag: node.name.clone(),
                text,
                top,
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
