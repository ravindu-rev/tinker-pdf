//! hOCR, ALTO and PAGE XML read into FT-24's word-box model: the import
//! seam's first half (`docs/design/ocr.md`, *The import*).
//!
//! [`import`] reads a file an outside recogniser wrote and returns its pages
//! in file order, each as a [`WordPage`] in the file's own frame — origin at
//! the top-left corner of the page image, `x` to the right, `y` downwards, in
//! the file's unit — with what a layer needs beside the model, line for line,
//! and the warnings the read raised. The recogniser supplies bytes and decides
//! nothing (ruling 13): the import corrects no word, moves no box and
//! re-segments no line.
//!
//! # Which format
//!
//! Told from the file. A root element `alto`, in no namespace or in one of
//! ALTO's v2, v3 and v4 namespaces, is ALTO; a root element `PcGts` in a
//! namespace under [`PAGE_NAMESPACE_PREFIX`] is PAGE XML; both are read with
//! `tinker-pdf-xml`'s reader, which refuses a document type declaration with
//! an internal subset before it reads past it. Anything else is parsed as
//! HTML, hOCR's media type, with `tinker_pdf_xml::html::parse_bytes` — so an
//! hOCR file written as XHTML is read the same way — and is hOCR when an
//! element's class list holds `ocr_page`. Anything else again is
//! [`ImportError::NotRecognised`].
//!
//! # Bounds
//!
//! Ruling 1: a file is untrusted markup. The readers' own limits bound the
//! markup — its depth, its events, its names — every number read is checked
//! finite, and a page keeps at most [`MAX_IMPORTED_WORDS`] words. Ruling 10:
//! every leniency is an [`ImportWarning`] naming the file's element.

use std::collections::{BTreeMap, BTreeSet};

use tinker_pdf_content::word_boxes::upright;
use tinker_pdf_content::{
    LineDirection, Quad, WordBlock, WordBox, WordLine, WordPage, WordParagraph,
};
use tinker_pdf_xml::html::{self, NodeData};
use tinker_pdf_xml::{Doctype, Event, Limits};

/// The most words one imported page keeps.
///
/// | | |
/// | --- | --- |
/// | **This cap** | **100 000** |
///
/// A page of a broadsheet newspaper set in agate holds some tens of thousands
/// of words, so the cap stands well over any page a recogniser reads, and
/// under what one file can state: an ALTO page of empty `String` elements
/// costs two of `tinker-pdf-xml`'s events a word, and its token cap admits
/// half a million. Past the cap a page's words are dropped, in reading order,
/// and [`ImportWarning::TooManyWords`] says so once.
pub const MAX_IMPORTED_WORDS: usize = 100_000;

/// What every PAGE XML namespace begins with: the schema's dated versions
/// each add their date to it.
pub const PAGE_NAMESPACE_PREFIX: &str = "http://schema.primaresearch.org/PAGE/gts/pagecontent/";

/// ALTO's namespaces: a root `alto` in one of these, or in none, is ALTO.
pub const ALTO_NAMESPACES: [&str; 3] = [
    "http://www.loc.gov/standards/alto/ns-v2#",
    "http://www.loc.gov/standards/alto/ns-v3#",
    "http://www.loc.gov/standards/alto/ns-v4#",
];

/// How deep a walk over a parsed file goes: past the depth the readers admit,
/// so it never stops one, and a ceiling on recursion whatever they admit.
const MAX_WALK_DEPTH: usize = 1024;

/// Which of the three formats a file was read as.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ImportFormat {
    /// hOCR 1.2, read as HTML.
    Hocr,
    /// ALTO, in its v2, v3 or v4 namespace or none.
    Alto,
    /// PAGE XML, in any of its dated namespaces.
    PageXml,
}

/// The unit a file's numbers are in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Unit {
    /// Pixels of the page image: hOCR and PAGE XML always, and ALTO's
    /// `pixel`.
    Pixel,
    /// ALTO's `mm10`: tenths of a millimetre, 254 an inch.
    Mm10,
    /// ALTO's `inch1200`: 1 200 an inch.
    Inch1200,
}

impl Unit {
    /// How many of the unit make an inch, where its name says.
    #[must_use]
    pub fn per_inch(self) -> Option<f64> {
        match self {
            Unit::Pixel => None,
            Unit::Mm10 => Some(254.0),
            Unit::Inch1200 => Some(1200.0),
        }
    }
}

/// A rectangle in a file's frame: `y` runs down, so `top` is the smaller.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Extent {
    /// The left edge.
    pub left: f64,
    /// The top edge.
    pub top: f64,
    /// The right edge.
    pub right: f64,
    /// The bottom edge.
    pub bottom: f64,
}

impl Extent {
    /// The rectangle's width.
    #[must_use]
    pub fn width(&self) -> f64 {
        self.right - self.left
    }

    /// The rectangle's height.
    #[must_use]
    pub fn height(&self) -> f64 {
        self.bottom - self.top
    }

    fn union(self, other: Extent) -> Extent {
        Extent {
            left: self.left.min(other.left),
            top: self.top.min(other.top),
            right: self.right.max(other.right),
            bottom: self.bottom.max(other.bottom),
        }
    }

    fn quad(self) -> Quad {
        upright(self.left, self.top, self.right, self.bottom)
    }

    fn within(self, page: Extent) -> bool {
        self.left >= page.left
            && self.top >= page.top
            && self.right <= page.right
            && self.bottom <= page.bottom
    }
}

/// The direction a file states a line's paragraph runs in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
    /// Left to right.
    LeftToRight,
    /// Right to left.
    RightToLeft,
}

/// What a layer needs of a line that the model does not hold.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineSetting {
    /// The angle the file states the line's text is turned by,
    /// counter-clockwise as the page is seen, in degrees from 0 up to 360:
    /// hOCR's `textangle`, ALTO's `ROTATION` and PAGE XML's `orientation`,
    /// inherited from the nearest element that states one, and a quarter turn
    /// more for a line ALTO states `btt` or PAGE XML `bottom-to-top`, three
    /// quarters for one stated `ttb` or `top-to-bottom`.
    pub turn: f64,
    /// The paragraph direction the file states, if it states one: hOCR's
    /// `dir`, inherited as HTML inherits it, ALTO's `BASEDIRECTION` and PAGE
    /// XML's `readingDirection`.
    pub direction: Option<Direction>,
}

/// One page of an imported file.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportedPage {
    /// The page in FT-24's model, in the file's frame and unit: every box is
    /// the rectangle the file states, its numbers unchanged. `width` and
    /// `height` are the extent's far edges rounded up to whole numbers,
    /// `resolution` the horizontal resolution the file states — or the
    /// unit's own, for ALTO's `mm10` and `inch1200` — and 72 where it states
    /// none, and `index` its `ppageno` (hOCR) or `PHYSICAL_IMG_NR` less one
    /// (ALTO), or else its position in the file.
    pub page: WordPage,
    /// The page's extent in its frame: `ocr_page`'s `bbox`, or the origin to
    /// ALTO's `WIDTH` and `HEIGHT` or PAGE XML's `imageWidth` and
    /// `imageHeight`.
    pub extent: Extent,
    /// The unit of every number on the page.
    pub unit: Unit,
    /// The horizontal and vertical resolution the file states, in pixels an
    /// inch, where it states one.
    pub resolution: Option<(f64, f64)>,
    /// One setting a line of the model, in the model's order: block by block,
    /// paragraph by paragraph, line by line.
    pub lines: Vec<LineSetting>,
}

/// A file read: its format, its pages in file order, and the warnings the
/// read raised.
#[derive(Clone, Debug, PartialEq)]
pub struct Imported {
    /// The format the file was read as.
    pub format: ImportFormat,
    /// The pages, in file order.
    pub pages: Vec<ImportedPage>,
    /// Every leniency the read took.
    pub warnings: Vec<ImportWarning>,
}

/// Which element of a file a warning is about: its page's position in the
/// file, the `id` or `ID` it carries where it has one, and the line's
/// position on the page and the word's in its line, each in document order
/// and counted from 0.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Located {
    /// The page's position in the file.
    pub page: usize,
    /// The element's `id` (hOCR, PAGE XML) or `ID` (ALTO).
    pub id: Option<String>,
    /// The line's position among the page's lines.
    pub line: usize,
    /// The word's position among its line's words.
    pub word: usize,
}

/// A leniency the import took, naming the element it took it on (ruling 10).
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum ImportWarning {
    /// A word with no box, or a box that is not four finite numbers: skipped.
    WordWithoutBox(Located),
    /// A word whose box's far edge is before its near one: skipped.
    InvertedBox(Located),
    /// A word whose text is empty once read: skipped.
    EmptyWord(Located),
    /// A character that is no text — a control character (Unicode's `Cc`),
    /// U+FEFF or a noncharacter — taken out of a word.
    NotText {
        /// The word.
        at: Located,
        /// The character taken out.
        character: char,
    },
    /// A confidence that is not a number on its format's scale: the word is
    /// kept with none.
    ConfidenceOutOfRange {
        /// The word.
        at: Located,
        /// What the file states.
        stated: String,
    },
    /// A word whose box reaches outside its page: kept as the file states it.
    OutsidePage(Located),
    /// A reading order naming an element the file does not hold, or naming
    /// one a second time: the name skipped.
    OrderNamesNothing {
        /// The page whose order it is; `None` for ALTO's, which is the
        /// document's.
        page: Option<usize>,
        /// The name.
        name: String,
    },
    /// A page past [`MAX_IMPORTED_WORDS`]: the rest of its words dropped.
    TooManyWords {
        /// The page's position in the file.
        page: usize,
        /// The cap.
        limit: usize,
    },
}

/// Why a file was not imported.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImportError {
    /// The file is none of hOCR, ALTO and PAGE XML.
    NotRecognised,
    /// Markup the reader refuses, with the reader's own refusal: an ALTO or
    /// PAGE XML file that is not well-formed, an hOCR file past the HTML
    /// parser's caps, or a document type declaration with an internal subset
    /// in a file that holds no hOCR page.
    Markup(tinker_pdf_xml::Error),
    /// A page with no extent — an `ocr_page` with no `bbox`, an ALTO `Page`
    /// with no `WIDTH` or `HEIGHT`, a PAGE XML `Page` with no `imageWidth` or
    /// `imageHeight`, or one whose extent has no area — named by its position
    /// in the file: no layer can place a page it cannot measure.
    PageWithoutExtent {
        /// The page's position in the file.
        page: usize,
    },
    /// An ALTO `MeasurementUnit` other than `pixel`, `mm10` and `inch1200`.
    UnknownUnit(String),
}

impl core::fmt::Display for ImportError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ImportError::NotRecognised => f.write_str("not an hOCR, ALTO or PAGE XML file"),
            ImportError::Markup(error) => write!(f, "markup refused: {error}"),
            ImportError::PageWithoutExtent { page } => {
                write!(f, "page {page} states no extent")
            }
            ImportError::UnknownUnit(unit) => write!(f, "unknown ALTO unit {unit:?}"),
        }
    }
}

impl std::error::Error for ImportError {}

/// Reads an hOCR, ALTO or PAGE XML file into FT-24's model, a page per page
/// of the file, in file order (see the module documentation).
///
/// # Errors
/// [`ImportError`]: a file that is none of the three, markup the reader
/// refuses, a page with no extent, or an ALTO unit that is none of the
/// three.
pub fn import(bytes: &[u8]) -> Result<Imported, ImportError> {
    let (root, tree) = read_xml(bytes);
    if let Some(root) = &root {
        let format = match (root.local.as_str(), root.namespace.as_deref()) {
            ("alto", None) => Some(ImportFormat::Alto),
            ("alto", Some(namespace)) if ALTO_NAMESPACES.contains(&namespace) => {
                Some(ImportFormat::Alto)
            }
            ("PcGts", Some(namespace)) if namespace.starts_with(PAGE_NAMESPACE_PREFIX) => {
                Some(ImportFormat::PageXml)
            }
            _ => None,
        };
        if let Some(format) = format {
            let tree = tree.map_err(ImportError::Markup)?;
            let mut warnings = Vec::new();
            let drafts = match format {
                ImportFormat::Alto => read_alto(&tree, &mut warnings)?,
                _ => read_page_xml(&tree, &mut warnings)?,
            };
            return Ok(finished(format, drafts, warnings));
        }
    }
    let document = html::parse_bytes(bytes, &Limits::DEFAULT);
    let pages = hocr_pages(&document);
    if pages.is_empty() {
        // A file the XML reader refused before its root, for a declaration
        // it will not read past, is refused by that refusal: it may be an
        // ALTO or PAGE XML file, and it names no hOCR page.
        if let (None, Err(error)) = (&root, &tree) {
            if matches!(
                error,
                tinker_pdf_xml::Error::InternalSubset
                    | tinker_pdf_xml::Error::MalformedDoctype
                    | tinker_pdf_xml::Error::MisplacedDoctype
            ) {
                return Err(ImportError::Markup(*error));
            }
        }
        return Err(ImportError::NotRecognised);
    }
    if let Some(error) = document.stopped() {
        return Err(ImportError::Markup(error));
    }
    let drafts = read_hocr(&document, &pages)?;
    Ok(finished(ImportFormat::Hocr, drafts, Vec::new()))
}

// ---- what every format reads into ----------------------------------------------

/// A word's box as its file states it.
#[derive(Clone, Copy, Debug)]
enum Stated {
    /// No box, or one that is not four finite numbers.
    Absent,
    /// A box whose far edge is before its near one.
    Inverted,
    /// A box.
    Box(Extent),
}

impl Stated {
    fn extent(self) -> Option<Extent> {
        match self {
            Stated::Box(extent) => Some(extent),
            Stated::Absent | Stated::Inverted => None,
        }
    }
}

/// A confidence as its file states it, and the top of its format's scale.
#[derive(Clone, Debug)]
struct Confidence {
    stated: String,
    scale: f64,
}

/// A word as read, before it is checked.
#[derive(Clone, Debug)]
struct DraftWord {
    id: Option<String>,
    text: String,
    stated: Stated,
    confidence: Option<Confidence>,
    /// Its line's position on the page and its own in the line.
    line: usize,
    word: usize,
}

#[derive(Clone, Debug)]
struct DraftLine {
    stated: Option<Extent>,
    setting: LineSetting,
    /// A line the file states runs top to bottom.
    vertical: bool,
    words: Vec<DraftWord>,
}

#[derive(Clone, Debug)]
struct DraftParagraph {
    /// `None` for a paragraph the file does not state, which is its block's
    /// (FT-24.1's paragraph for a page that states none).
    stated: Option<Extent>,
    lines: Vec<DraftLine>,
}

#[derive(Clone, Debug)]
struct DraftBlock {
    stated: Option<Extent>,
    paragraphs: Vec<DraftParagraph>,
}

/// A page as read, its blocks in reading order.
#[derive(Clone, Debug)]
struct DraftPage {
    position: usize,
    index: u32,
    extent: Extent,
    unit: Unit,
    resolution: Option<(f64, f64)>,
    image: Option<String>,
    blocks: Vec<DraftBlock>,
}

fn finished(
    format: ImportFormat,
    drafts: Vec<DraftPage>,
    mut warnings: Vec<ImportWarning>,
) -> Imported {
    let pages = drafts
        .into_iter()
        .map(|draft| finished_page(draft, &mut warnings))
        .collect();
    Imported {
        format,
        pages,
        warnings,
    }
}

/// A draft page checked word by word and made the model's.
fn finished_page(draft: DraftPage, warnings: &mut Vec<ImportWarning>) -> ImportedPage {
    let mut kept = 0usize;
    let mut capped = false;
    let mut blocks = Vec::new();
    let mut settings = Vec::new();
    for block in draft.blocks {
        let mut paragraphs = Vec::new();
        let mut block_extent: Option<Extent> = None;
        for paragraph in block.paragraphs {
            let mut lines = Vec::new();
            let mut paragraph_extent: Option<Extent> = None;
            for line in paragraph.lines {
                let mut words = Vec::new();
                let mut words_extent: Option<Extent> = None;
                for word in line.words {
                    if kept == MAX_IMPORTED_WORDS {
                        if !capped {
                            warnings.push(ImportWarning::TooManyWords {
                                page: draft.position,
                                limit: MAX_IMPORTED_WORDS,
                            });
                            capped = true;
                        }
                        break;
                    }
                    if let Some((extent, word)) =
                        checked(word, draft.position, draft.extent, warnings)
                    {
                        kept += 1;
                        words_extent = Some(words_extent.map_or(extent, |e| e.union(extent)));
                        words.push(word);
                    }
                }
                let Some(words_extent) = words_extent else {
                    continue;
                };
                let extent = line.stated.unwrap_or(words_extent);
                paragraph_extent = Some(paragraph_extent.map_or(extent, |e| e.union(extent)));
                let direction = if line.vertical {
                    LineDirection::TopToBottom
                } else if line.setting.direction == Some(Direction::RightToLeft) {
                    LineDirection::RightToLeft
                } else {
                    LineDirection::LeftToRight
                };
                lines.push(WordLine {
                    quad: extent.quad(),
                    direction,
                    words,
                });
                settings.push(line.setting);
            }
            let Some(lines_extent) = paragraph_extent else {
                continue;
            };
            let extent = paragraph.stated.or(block.stated).unwrap_or(lines_extent);
            block_extent = Some(block_extent.map_or(lines_extent, |e| e.union(lines_extent)));
            paragraphs.push(WordParagraph {
                quad: extent.quad(),
                lines,
            });
        }
        let Some(paragraphs_extent) = block_extent else {
            continue;
        };
        let extent = block.stated.unwrap_or(paragraphs_extent);
        blocks.push(WordBlock {
            quad: extent.quad(),
            paragraphs,
        });
    }
    ImportedPage {
        page: WordPage {
            index: draft.index,
            width: whole(draft.extent.right),
            height: whole(draft.extent.bottom),
            resolution: draft.resolution.map_or(72.0, |(x, _)| x),
            image: draft.image,
            blocks,
        },
        extent: draft.extent,
        unit: draft.unit,
        resolution: draft.resolution,
        lines: settings,
    }
}

/// A far edge as a whole number of the model's, rounded up and held to `u32`.
fn whole(edge: f64) -> u32 {
    let up = edge.ceil();
    if up <= 0.0 {
        0
    } else if up >= f64::from(u32::MAX) {
        u32::MAX
    } else {
        // In range and whole by the two arms above.
        up as u32
    }
}

/// One word checked: its box, its text and its confidence, each defect a
/// warning naming it, and the word kept when it has a box and some text.
fn checked(
    word: DraftWord,
    page: usize,
    extent: Extent,
    warnings: &mut Vec<ImportWarning>,
) -> Option<(Extent, WordBox)> {
    let at = Located {
        page,
        id: word.id,
        line: word.line,
        word: word.word,
    };
    let rect = match word.stated {
        Stated::Absent => {
            warnings.push(ImportWarning::WordWithoutBox(at));
            return None;
        }
        Stated::Inverted => {
            warnings.push(ImportWarning::InvertedBox(at));
            return None;
        }
        Stated::Box(rect) => rect,
    };
    let text = cleaned(&word.text, &at, warnings);
    if text.is_empty() {
        warnings.push(ImportWarning::EmptyWord(at));
        return None;
    }
    let confidence = word
        .confidence
        .and_then(|confidence| on_the_models_scale(confidence, &at, warnings));
    if !rect.within(extent) {
        warnings.push(ImportWarning::OutsidePage(at));
    }
    Some((
        rect,
        WordBox {
            text,
            quad: rect.quad(),
            confidence,
        },
    ))
}

/// A word's text as the model holds it: ASCII white space off both ends and a
/// run of it inside one space, and every character that is no text taken out
/// with a warning naming it.
fn cleaned(raw: &str, at: &Located, warnings: &mut Vec<ImportWarning>) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut space = false;
    for c in raw.chars() {
        if matches!(c, '\t' | '\n' | '\u{C}' | '\r' | ' ') {
            space = !out.is_empty();
            continue;
        }
        if is_not_text(c) {
            warnings.push(ImportWarning::NotText {
                at: at.clone(),
                character: c,
            });
            continue;
        }
        if space {
            out.push(' ');
            space = false;
        }
        out.push(c);
    }
    out
}

/// A control character (Unicode's `Cc`: C0, DEL and C1), U+FEFF or a
/// noncharacter: what no `/ToUnicode` should ever state.
fn is_not_text(c: char) -> bool {
    let code = u32::from(c);
    c.is_control()
        || c == '\u{FEFF}'
        || (0xFDD0..=0xFDEF).contains(&code)
        || code & 0xFFFE == 0xFFFE
}

/// A confidence carried onto the model's scale, 0 to 100, from its format's,
/// or `None` with a warning for one that is not a number on that scale.
fn on_the_models_scale(
    confidence: Confidence,
    at: &Located,
    warnings: &mut Vec<ImportWarning>,
) -> Option<f32> {
    match number(&confidence.stated) {
        Some(value) if (0.0..=confidence.scale).contains(&value) => {
            Some((value / confidence.scale * 100.0) as f32)
        }
        _ => {
            warnings.push(ImportWarning::ConfidenceOutOfRange {
                at: at.clone(),
                stated: confidence.stated,
            });
            None
        }
    }
}

/// A finite decimal number, white space around it allowed.
fn number(text: &str) -> Option<f64> {
    text.trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
}

/// An angle in degrees, from 0 up to 360.
fn normalised(degrees: f64) -> f64 {
    if !degrees.is_finite() {
        return 0.0;
    }
    let turn = degrees % 360.0;
    if turn < 0.0 {
        turn + 360.0
    } else {
        turn
    }
}

/// A rectangle's four numbers as a box: absent where one is missing, inverted
/// where a far edge is before its near one.
fn stated_box(
    left: Option<f64>,
    top: Option<f64>,
    right: Option<f64>,
    bottom: Option<f64>,
) -> Stated {
    match (left, top, right, bottom) {
        (Some(left), Some(top), Some(right), Some(bottom)) => {
            if right < left || bottom < top {
                Stated::Inverted
            } else {
                Stated::Box(Extent {
                    left,
                    top,
                    right,
                    bottom,
                })
            }
        }
        _ => Stated::Absent,
    }
}

/// An extent a page can be placed by: four finite numbers with area.
fn page_extent(stated: Stated, position: usize) -> Result<Extent, ImportError> {
    match stated {
        Stated::Box(extent) if extent.width() > 0.0 && extent.height() > 0.0 => Ok(extent),
        _ => Err(ImportError::PageWithoutExtent { page: position }),
    }
}

/// Blocks in reading order: those `named` in the order named, each at its
/// first naming, then the rest in document order.
fn named_first(count: usize, named: &[usize]) -> Vec<usize> {
    let mut placed = vec![false; count];
    let mut out = Vec::with_capacity(count);
    for &at in named {
        if let Some(slot) = placed.get_mut(at) {
            if !*slot {
                *slot = true;
                out.push(at);
            }
        }
    }
    out.extend((0..count).filter(|at| !placed.get(*at).copied().unwrap_or(true)));
    out
}

/// Items in document order, each with its parent and the order it states:
/// the items sharing a parent that every one of them states an order for
/// take that order among the places they hold (hOCR's `order`).
fn by_stated_order<T>(items: Vec<(Option<usize>, Option<i64>, T)>) -> Vec<T> {
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (at, (parent, _, _)) in items.iter().enumerate() {
        if let Some(parent) = parent {
            groups.entry(*parent).or_default().push(at);
        }
    }
    let orders: Vec<Option<i64>> = items.iter().map(|(_, order, _)| *order).collect();
    let mut placement: Vec<usize> = (0..items.len()).collect();
    for members in groups.values() {
        if members.len() < 2
            || members
                .iter()
                .any(|&m| orders.get(m).copied().flatten().is_none())
        {
            continue;
        }
        let mut sorted = members.clone();
        sorted.sort_by_key(|&m| (orders.get(m).copied().flatten(), m));
        for (&slot, &member) in members.iter().zip(sorted.iter()) {
            if let Some(place) = placement.get_mut(slot) {
                *place = member;
            }
        }
    }
    let mut slots: Vec<Option<T>> = items.into_iter().map(|(_, _, item)| Some(item)).collect();
    placement
        .into_iter()
        .filter_map(|at| slots.get_mut(at).and_then(Option::take))
        .collect()
}

// ---- the XML the reader reads -------------------------------------------------

/// An element of a document `tinker-pdf-xml` read.
#[derive(Debug)]
struct XmlElement {
    local: String,
    /// Its attributes in no namespace, by local name.
    attributes: Vec<(String, String)>,
    parent: Option<usize>,
    children: Vec<XmlNode>,
    /// One past the last element of its subtree: the elements are in
    /// document order, so a subtree is the run from the element to here.
    end: usize,
}

#[derive(Debug)]
enum XmlNode {
    Element(usize),
    Text(String),
}

/// A document's elements in document order, the root first.
#[derive(Debug, Default)]
struct XmlTree {
    elements: Vec<XmlElement>,
}

/// The root element's name, as far as the reader got.
struct Root {
    local: String,
    namespace: Option<String>,
}

/// Reads `bytes` as XML: the root, if the reader reached it, and the tree or
/// the reader's refusal.
fn read_xml(bytes: &[u8]) -> (Option<Root>, Result<XmlTree, tinker_pdf_xml::Error>) {
    let source = match tinker_pdf_xml::Source::new(bytes) {
        Ok(source) => source,
        Err(error) => return (None, Err(error)),
    };
    let mut tree = XmlTree::default();
    let mut open: Vec<usize> = Vec::new();
    let mut root = None;
    for event in source.reader_with(&Limits::DEFAULT, Doctype::SkipExternalId) {
        let event = match event {
            Ok(event) => event,
            Err(error) => return (root, Err(error)),
        };
        match event {
            Event::Start(element) => {
                let at = tree.elements.len();
                let parent = open.last().copied();
                if parent.is_none() && root.is_none() {
                    root = Some(Root {
                        local: element.local().to_string(),
                        namespace: element.namespace().map(str::to_string),
                    });
                }
                tree.elements.push(XmlElement {
                    local: element.local().to_string(),
                    attributes: element
                        .attributes()
                        .iter()
                        .filter(|attribute| attribute.namespace().is_none())
                        .map(|attribute| {
                            (attribute.local().to_string(), attribute.value().to_string())
                        })
                        .collect(),
                    parent,
                    children: Vec::new(),
                    end: at + 1,
                });
                if let Some(parent) = parent.and_then(|p| tree.elements.get_mut(p)) {
                    parent.children.push(XmlNode::Element(at));
                }
                open.push(at);
            }
            Event::End(_) => {
                let count = tree.elements.len();
                if let Some(closed) = open.pop().and_then(|at| tree.elements.get_mut(at)) {
                    closed.end = count;
                }
            }
            Event::Text(text) | Event::Cdata(text) => {
                if let Some(parent) = open.last().and_then(|&p| tree.elements.get_mut(p)) {
                    parent.children.push(XmlNode::Text(text.into_owned()));
                }
            }
            Event::Comment(_) | Event::Instruction { .. } => {}
        }
    }
    (root, Ok(tree))
}

impl XmlTree {
    fn element(&self, at: usize) -> Option<&XmlElement> {
        self.elements.get(at)
    }

    fn local(&self, at: usize) -> &str {
        self.element(at).map_or("", |e| e.local.as_str())
    }

    fn attribute(&self, at: usize, name: &str) -> Option<&str> {
        self.element(at)?
            .attributes
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    fn number(&self, at: usize, name: &str) -> Option<f64> {
        number(self.attribute(at, name)?)
    }

    fn parent(&self, at: usize) -> Option<usize> {
        self.element(at)?.parent
    }

    /// The element children, in order.
    fn children(&self, at: usize) -> impl Iterator<Item = usize> + '_ {
        self.element(at)
            .into_iter()
            .flat_map(|e| e.children.iter())
            .filter_map(|child| match child {
                XmlNode::Element(at) => Some(*at),
                XmlNode::Text(_) => None,
            })
    }

    fn child(&self, at: usize, local: &str) -> Option<usize> {
        self.children(at).find(|&child| self.local(child) == local)
    }

    /// Every element of `at`'s subtree named `local`, `at` excluded, in
    /// document order.
    fn descendants(&self, at: usize, local: &str) -> Vec<usize> {
        let end = self.element(at).map_or(at, |e| e.end);
        (at + 1..end)
            .filter(|&next| self.local(next) == local)
            .collect()
    }

    /// The nearest of `at` and its ancestors named in `locals` that carries
    /// the attribute `name`, and its value.
    fn inherited(&self, at: usize, locals: &[&str], name: &str) -> Option<&str> {
        let mut walk = Some(at);
        let mut steps = 0;
        while let Some(element) = walk {
            if locals.contains(&self.local(element)) {
                if let Some(value) = self.attribute(element, name) {
                    return Some(value);
                }
            }
            steps += 1;
            if steps > MAX_WALK_DEPTH {
                return None;
            }
            walk = self.parent(element);
        }
        None
    }

    /// The text of `at`'s subtree, in document order.
    fn text(&self, at: usize) -> String {
        let mut out = String::new();
        self.text_into(at, &mut out, 0);
        out
    }

    fn text_into(&self, at: usize, out: &mut String, depth: usize) {
        if depth > MAX_WALK_DEPTH {
            return;
        }
        let Some(element) = self.element(at) else {
            return;
        };
        for child in &element.children {
            match child {
                XmlNode::Text(text) => out.push_str(text),
                XmlNode::Element(child) => self.text_into(*child, out, depth + 1),
            }
        }
    }

    /// Every element carrying the attribute `name`, by its value: the first
    /// where two carry one.
    fn ids(&self, name: &str) -> BTreeMap<String, usize> {
        let mut out = BTreeMap::new();
        for at in 0..self.elements.len() {
            if let Some(id) = self.attribute(at, name) {
                out.entry(id.to_string()).or_insert(at);
            }
        }
        out
    }
}

/// A reading order's members flattened: the blocks each name resolves to, in
/// the order named, with a warning for a name that resolves to nothing and for
/// a name given twice.
struct Naming<'a> {
    resolve: &'a dyn Fn(&str) -> Option<Vec<usize>>,
    seen: BTreeSet<String>,
    page: Option<usize>,
    warnings: &'a mut Vec<ImportWarning>,
}

impl Naming<'_> {
    fn name(&mut self, name: &str) -> Vec<usize> {
        if !self.seen.insert(name.to_string()) {
            self.warnings.push(ImportWarning::OrderNamesNothing {
                page: self.page,
                name: name.to_string(),
            });
            return Vec::new();
        }
        match (self.resolve)(name) {
            Some(blocks) => blocks,
            None => {
                self.warnings.push(ImportWarning::OrderNamesNothing {
                    page: self.page,
                    name: name.to_string(),
                });
                Vec::new()
            }
        }
    }
}

/// An unordered group's members, each the blocks it names, put in document
/// order among themselves: by the first block each names, those naming none
/// last as listed.
fn in_document_order(mut members: Vec<Vec<usize>>) -> Vec<usize> {
    members.sort_by_key(|blocks| blocks.first().copied().unwrap_or(usize::MAX));
    members.into_iter().flatten().collect()
}

// ---- ALTO ---------------------------------------------------------------------

/// The ALTO reader: the description's unit and image, the document's reading
/// order, and each `Page` of the layout.
fn read_alto(
    tree: &XmlTree,
    warnings: &mut Vec<ImportWarning>,
) -> Result<Vec<DraftPage>, ImportError> {
    let root = 0;
    let description = tree.child(root, "Description");
    let unit = match description.and_then(|d| tree.child(d, "MeasurementUnit")) {
        None => Unit::Pixel,
        Some(stated) => match tree.text(stated).trim() {
            "pixel" => Unit::Pixel,
            "mm10" => Unit::Mm10,
            "inch1200" => Unit::Inch1200,
            other => return Err(ImportError::UnknownUnit(other.to_string())),
        },
    };
    let image = description
        .and_then(|d| tree.child(d, "sourceImageInformation"))
        .and_then(|s| tree.child(s, "fileName"))
        .map(|f| tree.text(f).trim().to_string())
        .filter(|name| !name.is_empty());
    let ids = tree.ids("ID");

    // The text blocks an `ElementRef` or an `IDNEXT` reaches: the block
    // itself, a line's or a string's block, or a composed block's blocks;
    // anything else the file holds is there and holds no text.
    let resolve = |name: &str| -> Option<Vec<usize>> {
        let &at = ids.get(name)?;
        Some(match tree.local(at) {
            "TextBlock" => vec![at],
            "TextLine" | "String" | "SP" | "HYP" => {
                let mut walk = tree.parent(at);
                let mut steps = 0;
                while let Some(parent) = walk {
                    if tree.local(parent) == "TextBlock" || steps > MAX_WALK_DEPTH {
                        break;
                    }
                    steps += 1;
                    walk = tree.parent(parent);
                }
                walk.filter(|&p| tree.local(p) == "TextBlock")
                    .into_iter()
                    .collect()
            }
            _ => tree.descendants(at, "TextBlock"),
        })
    };

    // The document's reading order: the first group of `ReadingOrder`, depth
    // first (ALTO 4.3).
    let stated_order = tree
        .child(root, "ReadingOrder")
        .and_then(|order| {
            tree.children(order)
                .find(|&g| matches!(tree.local(g), "OrderedGroup" | "UnorderedGroup"))
        })
        .map(|group| {
            let mut naming = Naming {
                resolve: &resolve,
                seen: BTreeSet::new(),
                page: None,
                warnings: &mut *warnings,
            };
            alto_group(tree, group, &mut naming, 0)
        });

    let pages = tree
        .child(root, "Layout")
        .map(|layout| tree.descendants(layout, "Page"))
        .unwrap_or_default();
    let mut drafts = Vec::with_capacity(pages.len());
    for (position, &page) in pages.iter().enumerate() {
        let extent = page_extent(
            stated_box(
                Some(0.0),
                Some(0.0),
                tree.number(page, "WIDTH"),
                tree.number(page, "HEIGHT"),
            ),
            position,
        )?;
        let index = tree
            .attribute(page, "PHYSICAL_IMG_NR")
            .and_then(|n| n.trim().parse::<u32>().ok())
            .filter(|&n| n >= 1)
            .map_or_else(|| u32::try_from(position).unwrap_or(u32::MAX), |n| n - 1);
        let text_blocks = tree.descendants(page, "TextBlock");

        let mut line_number = 0usize;
        let mut blocks: Vec<DraftBlock> = Vec::with_capacity(text_blocks.len());
        for &block in &text_blocks {
            let mut lines = Vec::new();
            for line in tree
                .children(block)
                .filter(|&l| tree.local(l) == "TextLine")
            {
                lines.push(alto_line(tree, line, line_number));
                line_number += 1;
            }
            let stated = alto_box(tree, block).extent();
            blocks.push(DraftBlock {
                stated,
                paragraphs: vec![DraftParagraph {
                    stated: None,
                    lines,
                }],
            });
        }

        // Reading order: the document's stated one, else each `IDNEXT` chain
        // from a block nothing names, else document order.
        let position_of: BTreeMap<usize, usize> = text_blocks
            .iter()
            .enumerate()
            .map(|(i, &b)| (b, i))
            .collect();
        let named: Vec<usize> = if let Some(order) = &stated_order {
            order
                .iter()
                .filter_map(|block| position_of.get(block).copied())
                .collect()
        } else if text_blocks
            .iter()
            .any(|&b| tree.attribute(b, "IDNEXT").is_some())
        {
            alto_chains(
                tree,
                &text_blocks,
                &position_of,
                &resolve,
                position,
                warnings,
            )
        } else {
            Vec::new()
        };
        let order = named_first(blocks.len(), &named);
        let mut slots: Vec<Option<DraftBlock>> = blocks.into_iter().map(Some).collect();
        let blocks = order
            .into_iter()
            .filter_map(|at| slots.get_mut(at).and_then(Option::take))
            .collect();
        drafts.push(DraftPage {
            position,
            index,
            extent,
            unit,
            resolution: unit.per_inch().map(|r| (r, r)),
            image: image.clone(),
            blocks,
        });
    }
    Ok(drafts)
}

/// An ALTO group flattened, depth first: an ordered group's members as listed,
/// an unordered group's in document order among themselves.
fn alto_group(tree: &XmlTree, group: usize, naming: &mut Naming<'_>, depth: usize) -> Vec<usize> {
    if depth > MAX_WALK_DEPTH {
        return Vec::new();
    }
    let mut members = Vec::new();
    for member in tree.children(group).collect::<Vec<_>>() {
        match tree.local(member) {
            "ElementRef" => {
                if let Some(name) = tree.attribute(member, "REF") {
                    members.push(naming.name(name));
                }
            }
            "OrderedGroup" | "UnorderedGroup" => {
                members.push(alto_group(tree, member, naming, depth + 1));
            }
            _ => {}
        }
    }
    if tree.local(group) == "UnorderedGroup" {
        in_document_order(members)
    } else {
        members.into_iter().flatten().collect()
    }
}

/// Where a block's `IDNEXT` leads.
enum Next {
    /// The block states none, or names one on another page: its chain ends.
    End,
    /// A text block on this page.
    Block(usize),
    /// A name the file holds nothing under.
    Nothing(String),
}

/// The blocks each `IDNEXT` chain names, from each block nothing names in
/// document order, as positions among `blocks`.
fn alto_chains(
    tree: &XmlTree,
    blocks: &[usize],
    position_of: &BTreeMap<usize, usize>,
    resolve: &dyn Fn(&str) -> Option<Vec<usize>>,
    page: usize,
    warnings: &mut Vec<ImportWarning>,
) -> Vec<usize> {
    let next = |block: usize| -> Next {
        let Some(name) = tree.attribute(block, "IDNEXT") else {
            return Next::End;
        };
        match resolve(name) {
            None => Next::Nothing(name.to_string()),
            Some(reached) => match reached.first() {
                Some(&first) if position_of.contains_key(&first) => Next::Block(first),
                _ => Next::End,
            },
        }
    };
    let mut named_by_another = BTreeSet::new();
    for &block in blocks {
        if let Next::Block(reached) = next(block) {
            named_by_another.insert(reached);
        }
    }
    let mut placed = BTreeSet::new();
    let mut out = Vec::new();
    for &start in blocks {
        if named_by_another.contains(&start) {
            continue;
        }
        let mut at = start;
        while placed.insert(at) {
            if let Some(&position) = position_of.get(&at) {
                out.push(position);
            }
            match next(at) {
                Next::End => break,
                Next::Nothing(name) => {
                    warnings.push(ImportWarning::OrderNamesNothing {
                        page: Some(page),
                        name,
                    });
                    break;
                }
                Next::Block(reached) if placed.contains(&reached) => {
                    // A block a chain has placed already: named a second time.
                    let name = tree.attribute(at, "IDNEXT").unwrap_or_default();
                    warnings.push(ImportWarning::OrderNamesNothing {
                        page: Some(page),
                        name: name.to_string(),
                    });
                    break;
                }
                Next::Block(reached) => at = reached,
            }
        }
    }
    out
}

/// An ALTO element's `HPOS`, `VPOS`, `WIDTH` and `HEIGHT` as a box.
fn alto_box(tree: &XmlTree, at: usize) -> Stated {
    let (hpos, vpos) = (tree.number(at, "HPOS"), tree.number(at, "VPOS"));
    let (width, height) = (tree.number(at, "WIDTH"), tree.number(at, "HEIGHT"));
    match (hpos, vpos, width, height) {
        (Some(_), Some(_), Some(w), Some(h)) if w < 0.0 || h < 0.0 => Stated::Inverted,
        (Some(x), Some(y), Some(w), Some(h)) => {
            stated_box(Some(x), Some(y), Some(x + w), Some(y + h))
        }
        _ => Stated::Absent,
    }
}

/// An ALTO `TextLine`: its box, its turn and direction, and its words — a
/// `String` each, a `HYP` joined to the last.
fn alto_line(tree: &XmlTree, line: usize, ordinal: usize) -> DraftLine {
    let rotation = tree
        .inherited(line, &["TextBlock", "ComposedBlock", "Page"], "ROTATION")
        .and_then(number);
    let base = tree.inherited(line, &["TextLine", "TextBlock"], "BASEDIRECTION");
    let (quarters, direction, vertical) = match base.map(str::trim) {
        Some("rtl") => (0.0, Some(Direction::RightToLeft), false),
        Some("ltr") => (0.0, Some(Direction::LeftToRight), false),
        Some("btt") => (90.0, None, false),
        Some("ttb") => (270.0, None, true),
        _ => (0.0, None, false),
    };
    let mut words: Vec<DraftWord> = Vec::new();
    for element in tree.children(line).collect::<Vec<_>>() {
        match tree.local(element) {
            "String" => {
                let word = words.len();
                words.push(DraftWord {
                    id: tree.attribute(element, "ID").map(str::to_string),
                    text: tree
                        .attribute(element, "CONTENT")
                        .unwrap_or_default()
                        .to_string(),
                    stated: alto_box(tree, element),
                    confidence: tree.attribute(element, "WC").map(|wc| Confidence {
                        stated: wc.to_string(),
                        scale: 1.0,
                    }),
                    line: ordinal,
                    word,
                });
            }
            "HYP" => {
                let content = tree.attribute(element, "CONTENT").unwrap_or_default();
                let hpos = tree.number(element, "HPOS");
                let width = tree.number(element, "WIDTH");
                let vpos = tree.number(element, "VPOS");
                let height = tree.number(element, "HEIGHT");
                if let Some(last) = words.last_mut() {
                    // The hyphen ends the line's last word: its content is
                    // that word's last character, its box that word's end.
                    last.text.push_str(content);
                    if let Stated::Box(extent) = &mut last.stated {
                        if let (Some(x), Some(w)) = (hpos, width) {
                            if w >= 0.0 {
                                extent.left = extent.left.min(x);
                                extent.right = extent.right.max(x + w);
                            }
                        }
                        if let (Some(y), Some(h)) = (vpos, height) {
                            if h >= 0.0 {
                                extent.top = extent.top.min(y);
                                extent.bottom = extent.bottom.max(y + h);
                            }
                        }
                    }
                } else {
                    words.push(DraftWord {
                        id: tree.attribute(element, "ID").map(str::to_string),
                        text: content.to_string(),
                        stated: alto_box(tree, element),
                        confidence: None,
                        line: ordinal,
                        word: 0,
                    });
                }
            }
            _ => {}
        }
    }
    DraftLine {
        stated: alto_box(tree, line).extent(),
        setting: LineSetting {
            turn: normalised(rotation.unwrap_or(0.0) + quarters),
            direction,
        },
        vertical,
        words,
    }
}

// ---- PAGE XML -----------------------------------------------------------------

/// The PAGE XML reader: the one `Page` a `PcGts` holds.
fn read_page_xml(
    tree: &XmlTree,
    warnings: &mut Vec<ImportWarning>,
) -> Result<Vec<DraftPage>, ImportError> {
    let Some(page) = tree.child(0, "Page") else {
        return Ok(Vec::new());
    };
    let position = 0;
    let extent = page_extent(
        stated_box(
            Some(0.0),
            Some(0.0),
            tree.number(page, "imageWidth"),
            tree.number(page, "imageHeight"),
        ),
        position,
    )?;
    let per_inch = match tree.attribute(page, "imageResolutionUnit").map(str::trim) {
        None | Some("PPI") => Some(1.0),
        Some("PPCM") => Some(2.54),
        Some(_) => None,
    };
    let resolution = match (
        tree.number(page, "imageXResolution"),
        tree.number(page, "imageYResolution"),
        per_inch,
    ) {
        (Some(x), Some(y), Some(k)) if x > 0.0 && y > 0.0 => Some((x * k, y * k)),
        _ => None,
    };
    let image = tree
        .attribute(page, "imageFilename")
        .map(str::to_string)
        .filter(|name| !name.is_empty());

    let regions: Vec<usize> = tree
        .descendants(page, "TextRegion")
        .into_iter()
        .filter(|&r| tree.child(r, "TextLine").is_some())
        .collect();
    let position_of: BTreeMap<usize, usize> =
        regions.iter().enumerate().map(|(i, &r)| (r, i)).collect();
    let line_numbers: BTreeMap<usize, usize> = tree
        .descendants(page, "TextLine")
        .into_iter()
        .enumerate()
        .map(|(n, l)| (l, n))
        .collect();

    // The regions a `regionRef` reaches: a region of lines itself, or the
    // regions of lines inside a region that holds only regions; any other
    // element the page holds is there and holds no text.
    let ids = tree.ids("id");
    let resolve = |name: &str| -> Option<Vec<usize>> {
        let &at = ids.get(name)?;
        if position_of.contains_key(&at) {
            return Some(vec![at]);
        }
        Some(
            tree.descendants(at, "TextRegion")
                .into_iter()
                .filter(|r| position_of.contains_key(r))
                .collect(),
        )
    };
    let named: Vec<usize> = tree
        .child(page, "ReadingOrder")
        .and_then(|order| {
            tree.children(order)
                .find(|&g| matches!(tree.local(g), "OrderedGroup" | "UnorderedGroup"))
        })
        .map(|group| {
            let mut naming = Naming {
                resolve: &resolve,
                seen: BTreeSet::new(),
                page: Some(position),
                warnings: &mut *warnings,
            };
            page_group(tree, group, &mut naming, 0)
        })
        .unwrap_or_default()
        .into_iter()
        .filter_map(|region| position_of.get(&region).copied())
        .collect();

    let mut blocks = Vec::with_capacity(regions.len());
    for &region in &regions {
        let mut lines: Vec<(Option<i64>, usize)> = tree
            .children(region)
            .filter(|&l| tree.local(l) == "TextLine")
            .map(|l| {
                (
                    tree.attribute(l, "index")
                        .and_then(|i| i.trim().parse::<i64>().ok()),
                    l,
                )
            })
            .collect();
        if lines.iter().all(|(index, _)| index.is_some()) {
            lines.sort_by_key(|&(index, l)| (index, l));
        }
        let lines = lines
            .into_iter()
            .map(|(_, line)| page_line(tree, line, line_numbers.get(&line).copied().unwrap_or(0)))
            .collect();
        blocks.push(DraftBlock {
            stated: page_coords(tree, region).extent(),
            paragraphs: vec![DraftParagraph {
                stated: None,
                lines,
            }],
        });
    }
    let order = named_first(blocks.len(), &named);
    let mut slots: Vec<Option<DraftBlock>> = blocks.into_iter().map(Some).collect();
    let blocks = order
        .into_iter()
        .filter_map(|at| slots.get_mut(at).and_then(Option::take))
        .collect();
    Ok(vec![DraftPage {
        position,
        index: 0,
        extent,
        unit: Unit::Pixel,
        resolution,
        image,
        blocks,
    }])
}

/// A PAGE XML group flattened, depth first: its own `regionRef` first, then
/// an ordered group's members by `index`, an unordered group's in document
/// order among themselves.
fn page_group(tree: &XmlTree, group: usize, naming: &mut Naming<'_>, depth: usize) -> Vec<usize> {
    if depth > MAX_WALK_DEPTH {
        return Vec::new();
    }
    let mut out = Vec::new();
    if let Some(name) = tree.attribute(group, "regionRef") {
        out.extend(naming.name(name));
    }
    let mut members: Vec<(Option<i64>, usize)> = tree
        .children(group)
        .filter(|&m| {
            matches!(
                tree.local(m),
                "RegionRefIndexed"
                    | "OrderedGroupIndexed"
                    | "UnorderedGroupIndexed"
                    | "RegionRef"
                    | "OrderedGroup"
                    | "UnorderedGroup"
            )
        })
        .map(|m| {
            (
                tree.attribute(m, "index")
                    .and_then(|i| i.trim().parse::<i64>().ok()),
                m,
            )
        })
        .collect();
    let ordered = tree.local(group).starts_with("OrderedGroup");
    if ordered {
        members.sort_by_key(|&(index, m)| (index.is_none(), index, m));
    }
    let mut lists = Vec::with_capacity(members.len());
    for (_, member) in members {
        let list = match tree.local(member) {
            "RegionRefIndexed" | "RegionRef" => tree
                .attribute(member, "regionRef")
                .map(|name| naming.name(name))
                .unwrap_or_default(),
            _ => page_group(tree, member, naming, depth + 1),
        };
        lists.push(list);
    }
    if ordered {
        out.extend(lists.into_iter().flatten());
    } else {
        out.extend(in_document_order(lists));
    }
    out
}

/// A PAGE XML element's `Coords` as a box: the bounds of its points, given
/// as a `points` attribute (2013-07-15 and later) or as `Point` children
/// (before it).
fn page_coords(tree: &XmlTree, at: usize) -> Stated {
    let Some(coords) = tree.child(at, "Coords") else {
        return Stated::Absent;
    };
    let mut points: Vec<(f64, f64)> = Vec::new();
    if let Some(list) = tree.attribute(coords, "points") {
        for pair in list.split_ascii_whitespace() {
            let mut halves = pair.split(',');
            match (
                halves.next().and_then(number),
                halves.next().and_then(number),
                halves.next(),
            ) {
                (Some(x), Some(y), None) => points.push((x, y)),
                _ => return Stated::Absent,
            }
        }
    } else {
        for point in tree.children(coords).filter(|&p| tree.local(p) == "Point") {
            match (tree.number(point, "x"), tree.number(point, "y")) {
                (Some(x), Some(y)) => points.push((x, y)),
                _ => return Stated::Absent,
            }
        }
    }
    let Some(&(x, y)) = points.first() else {
        return Stated::Absent;
    };
    let mut extent = Extent {
        left: x,
        top: y,
        right: x,
        bottom: y,
    };
    for &(x, y) in &points {
        extent = extent.union(Extent {
            left: x,
            top: y,
            right: x,
            bottom: y,
        });
    }
    Stated::Box(extent)
}

/// The text and confidence of the `TextEquiv` with the lowest `index`, or the
/// first where none states one.
fn page_text(tree: &XmlTree, at: usize) -> Option<(String, Option<Confidence>)> {
    let equivalents: Vec<usize> = tree
        .children(at)
        .filter(|&t| tree.local(t) == "TextEquiv")
        .collect();
    let indexed = equivalents
        .iter()
        .filter_map(|&t| {
            tree.attribute(t, "index")
                .and_then(|i| i.trim().parse::<i64>().ok())
                .map(|i| (i, t))
        })
        .min();
    let chosen = indexed
        .map(|(_, t)| t)
        .or_else(|| equivalents.first().copied())?;
    let text = tree
        .child(chosen, "Unicode")
        .map(|u| tree.text(u))
        .unwrap_or_default();
    let confidence = tree.attribute(chosen, "conf").map(|conf| Confidence {
        stated: conf.to_string(),
        scale: 1.0,
    });
    Some((text, confidence))
}

/// A PAGE XML `TextLine`: its box, its turn and direction, and its words —
/// a `Word` each, or the line's own text over the line's box where it holds
/// none.
fn page_line(tree: &XmlTree, line: usize, ordinal: usize) -> DraftLine {
    let orientation = tree
        .inherited(line, &["TextRegion", "Page"], "orientation")
        .and_then(number);
    let reading = tree.inherited(
        line,
        &["TextLine", "TextRegion", "Page"],
        "readingDirection",
    );
    let (quarters, direction, vertical) = match reading.map(str::trim) {
        Some("right-to-left") => (0.0, Some(Direction::RightToLeft), false),
        Some("left-to-right") => (0.0, Some(Direction::LeftToRight), false),
        Some("bottom-to-top") => (90.0, None, false),
        Some("top-to-bottom") => (270.0, None, true),
        _ => (0.0, None, false),
    };
    let stated_line = page_coords(tree, line);
    let mut words = Vec::new();
    for (n, word) in tree
        .children(line)
        .filter(|&w| tree.local(w) == "Word")
        .enumerate()
    {
        let (text, confidence) = page_text(tree, word).unwrap_or_default();
        words.push(DraftWord {
            id: tree.attribute(word, "id").map(str::to_string),
            text,
            stated: page_coords(tree, word),
            confidence,
            line: ordinal,
            word: n,
        });
    }
    if words.is_empty() {
        if let Some((text, confidence)) = page_text(tree, line) {
            words.push(DraftWord {
                id: tree.attribute(line, "id").map(str::to_string),
                text,
                stated: stated_line,
                confidence,
                line: ordinal,
                word: 0,
            });
        }
    }
    DraftLine {
        stated: stated_line.extent(),
        setting: LineSetting {
            turn: normalised(orientation.unwrap_or(0.0) + quarters),
            direction,
        },
        vertical,
        words,
    }
}

// ---- hOCR ---------------------------------------------------------------------

/// The classes that make an element a block: `ocr_carea`, the engines'
/// `ocrx_block`, and `ocr_column`, `ocr_carea`'s obsolete name.
const BLOCK_CLASSES: [&str; 3] = ["ocr_carea", "ocrx_block", "ocr_column"];

/// The classes that make an element a line.
const LINE_CLASSES: [&str; 2] = ["ocr_line", "ocrx_line"];

/// The float classes that are a line where they hold words directly — no
/// line inside them.
const FLOAT_LINE_CLASSES: [&str; 4] = ["ocr_header", "ocr_footer", "ocr_caption", "ocr_textfloat"];

/// An element's class list.
fn classes(element: &html::Element) -> Vec<&str> {
    element
        .attribute("class")
        .map(|list| list.split_ascii_whitespace().collect())
        .unwrap_or_default()
}

fn has_class(element: &html::Element, wanted: &[&str]) -> bool {
    classes(element).iter().any(|c| wanted.contains(c))
}

/// An hOCR `title`'s properties, each its name and values (hOCR 1.2 2.4): a
/// property ends at a semicolon outside a double-quoted string, its first
/// word is its name and the rest its values, a quoted string one value.
fn properties(title: &str) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut in_quotes = false;
    let end_word = |word: &mut String, quoted: &mut bool, words: &mut Vec<String>| {
        if !word.is_empty() || *quoted {
            words.push(core::mem::take(word));
        }
        *quoted = false;
    };
    let end_property = |words: &mut Vec<String>, out: &mut Vec<(String, Vec<String>)>| {
        let mut taken = core::mem::take(words).into_iter();
        if let Some(name) = taken.next() {
            out.push((name, taken.collect()));
        }
    };
    for c in title.chars() {
        if in_quotes {
            if c == '"' {
                in_quotes = false;
            } else {
                word.push(c);
            }
            continue;
        }
        match c {
            '"' => {
                in_quotes = true;
                quoted = true;
            }
            ';' => {
                end_word(&mut word, &mut quoted, &mut words);
                end_property(&mut words, &mut out);
            }
            c if c.is_ascii_whitespace() => end_word(&mut word, &mut quoted, &mut words),
            c => word.push(c),
        }
    }
    end_word(&mut word, &mut quoted, &mut words);
    end_property(&mut words, &mut out);
    out
}

fn property<'p>(properties: &'p [(String, Vec<String>)], name: &str) -> Option<&'p [String]> {
    properties
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, values)| values.as_slice())
}

/// `bbox x0 y0 x1 y1` as a box (4.2): the integers its grammar states and the
/// decimals some files carry alike.
fn bbox(properties: &[(String, Vec<String>)]) -> Stated {
    match property(properties, "bbox") {
        Some([x0, y0, x1, y1]) => stated_box(number(x0), number(y0), number(x1), number(y1)),
        _ => Stated::Absent,
    }
}

/// `order n` (4.11).
fn stated_order(properties: &[(String, Vec<String>)]) -> Option<i64> {
    match property(properties, "order") {
        Some([n]) => n.trim().parse::<i64>().ok(),
        _ => None,
    }
}

/// Every `ocr_page` of a document that is not inside another, in document
/// order.
fn hocr_pages(document: &html::Document) -> Vec<usize> {
    let mut out = Vec::new();
    let mut stack = vec![(document.root(), 0usize)];
    while let Some((node, depth)) = stack.pop() {
        let Some(n) = document.node(node) else {
            continue;
        };
        if let Some(element) = n.element() {
            if has_class(element, &["ocr_page"]) {
                out.push(node);
                continue;
            }
        }
        if depth < MAX_WALK_DEPTH {
            for &child in n.children.iter().rev() {
                stack.push((child, depth + 1));
            }
        }
    }
    out
}

/// A word's or a line's text: its element's text content, with an
/// `alternatives` span contributing its first `ins` alone, the most probable
/// reading (5.7).
fn text_content(document: &html::Document, node: usize, out: &mut String, depth: usize) {
    if depth > MAX_WALK_DEPTH {
        return;
    }
    let Some(n) = document.node(node) else {
        return;
    };
    for &child in &n.children {
        let Some(c) = document.node(child) else {
            continue;
        };
        match &c.data {
            NodeData::Text(text) => out.push_str(text),
            NodeData::Element(element) => {
                if has_class(element, &["alternatives"]) {
                    let first = c.children.iter().copied().find(|&alt| {
                        document
                            .node(alt)
                            .and_then(html::Node::element)
                            .is_some_and(|e| e.name == "ins")
                    });
                    if let Some(first) = first {
                        text_content(document, first, out, depth + 1);
                    }
                } else {
                    text_content(document, child, out, depth + 1);
                }
            }
            _ => {}
        }
    }
}

/// Whether any element of `node`'s subtree carries one of `wanted`.
fn holds_class(document: &html::Document, node: usize, wanted: &[&str], depth: usize) -> bool {
    if depth > MAX_WALK_DEPTH {
        return false;
    }
    let Some(n) = document.node(node) else {
        return false;
    };
    n.children.iter().any(|&child| {
        document.node(child).is_some_and(|c| {
            c.element().is_some_and(|e| has_class(e, wanted))
                || holds_class(document, child, wanted, depth + 1)
        })
    })
}

/// A block as the walk found it: its parent and stated order for 4.11.
struct HocrBlock {
    parent: Option<usize>,
    order: Option<i64>,
    stated: Option<Extent>,
    paragraphs: Vec<HocrParagraph>,
    /// The paragraph loose lines go into until the block states another.
    loose: Option<usize>,
}

struct HocrParagraph {
    stated: Option<Extent>,
    lines: Vec<HocrLine>,
    /// The line loose words go into until the paragraph states another.
    loose: Option<usize>,
}

struct HocrLine {
    parent: Option<usize>,
    order: Option<i64>,
    line: DraftLine,
    number: usize,
}

/// What an element's ancestors leave it: where it is and what it inherits.
#[derive(Clone, Copy)]
struct Scope {
    block: Option<usize>,
    paragraph: Option<usize>,
    line: Option<usize>,
    turn: Option<f64>,
    direction: Option<Direction>,
}

/// One `ocr_page` walked: its blocks in document order.
struct HocrWalk<'d> {
    document: &'d html::Document,
    blocks: Vec<HocrBlock>,
    /// The block loose content goes into until the page states another.
    loose: Option<usize>,
    lines: usize,
}

impl HocrWalk<'_> {
    fn block_for(&mut self, scope: Scope) -> usize {
        if let Some(block) = scope.block {
            return block;
        }
        if let Some(block) = self.loose {
            return block;
        }
        self.blocks.push(HocrBlock {
            parent: None,
            order: None,
            stated: None,
            paragraphs: Vec::new(),
            loose: None,
        });
        let block = self.blocks.len() - 1;
        self.loose = Some(block);
        block
    }

    fn paragraph_for(&mut self, scope: Scope) -> (usize, usize) {
        let block = self.block_for(scope);
        if let Some(paragraph) = scope.paragraph {
            return (block, paragraph);
        }
        let Some(b) = self.blocks.get_mut(block) else {
            return (block, 0);
        };
        if let Some(paragraph) = b.loose {
            return (block, paragraph);
        }
        b.paragraphs.push(HocrParagraph {
            stated: None,
            lines: Vec::new(),
            loose: None,
        });
        let paragraph = b.paragraphs.len() - 1;
        b.loose = Some(paragraph);
        (block, paragraph)
    }

    fn paragraph(&mut self, block: usize, paragraph: usize) -> Option<&mut HocrParagraph> {
        self.blocks.get_mut(block)?.paragraphs.get_mut(paragraph)
    }

    fn line(&mut self, block: usize, paragraph: usize, line: usize) -> Option<&mut HocrLine> {
        self.paragraph(block, paragraph)?.lines.get_mut(line)
    }

    /// Opens a line in the paragraph the scope reaches: one the file states,
    /// or the loose line words outside any line go into.
    fn open_line(
        &mut self,
        scope: Scope,
        parent: Option<usize>,
        order: Option<i64>,
        stated: Option<Extent>,
        loose: bool,
    ) -> (usize, usize, usize) {
        let (block, paragraph) = self.paragraph_for(scope);
        let number = self.lines;
        self.lines += 1;
        let Some(p) = self.paragraph(block, paragraph) else {
            return (block, paragraph, 0);
        };
        p.lines.push(HocrLine {
            parent,
            order,
            line: DraftLine {
                stated,
                setting: LineSetting {
                    turn: normalised(scope.turn.unwrap_or(0.0)),
                    direction: scope.direction,
                },
                vertical: false,
                words: Vec::new(),
            },
            number,
        });
        let line = p.lines.len() - 1;
        p.loose = loose.then_some(line);
        (block, paragraph, line)
    }

    fn visit(&mut self, node: usize, scope: Scope, depth: usize) {
        if depth > MAX_WALK_DEPTH {
            return;
        }
        let document = self.document;
        let Some(n) = document.node(node) else {
            return;
        };
        for &child in &n.children {
            let Some(element) = document.node(child).and_then(html::Node::element) else {
                continue;
            };
            let props = element
                .attribute("title")
                .map(properties)
                .unwrap_or_default();
            let mut inner = scope;
            if let Some(angle) = property(&props, "textangle")
                .and_then(|values| values.first())
                .and_then(|value| number(value))
            {
                inner.turn = Some(angle);
            }
            if let Some(dir) = element.attribute("dir") {
                if dir.eq_ignore_ascii_case("ltr") {
                    inner.direction = Some(Direction::LeftToRight);
                } else if dir.eq_ignore_ascii_case("rtl") {
                    inner.direction = Some(Direction::RightToLeft);
                } else if dir.eq_ignore_ascii_case("auto") {
                    inner.direction = None;
                }
            }

            if scope.block.is_none() && has_class(element, &BLOCK_CLASSES) {
                self.blocks.push(HocrBlock {
                    parent: Some(node),
                    order: stated_order(&props),
                    stated: bbox(&props).extent(),
                    paragraphs: Vec::new(),
                    loose: None,
                });
                self.loose = None;
                let block = self.blocks.len() - 1;
                let inner = Scope {
                    block: Some(block),
                    paragraph: None,
                    line: None,
                    ..inner
                };
                self.visit(child, inner, depth + 1);
            } else if scope.paragraph.is_none()
                && scope.line.is_none()
                && has_class(element, &["ocr_par"])
            {
                let block = self.block_for(scope);
                let Some(b) = self.blocks.get_mut(block) else {
                    continue;
                };
                b.paragraphs.push(HocrParagraph {
                    stated: bbox(&props).extent(),
                    lines: Vec::new(),
                    loose: None,
                });
                b.loose = None;
                let paragraph = b.paragraphs.len() - 1;
                let inner = Scope {
                    block: Some(block),
                    paragraph: Some(paragraph),
                    line: None,
                    ..inner
                };
                self.visit(child, inner, depth + 1);
            } else if scope.line.is_none()
                && (has_class(element, &LINE_CLASSES)
                    || (has_class(element, &FLOAT_LINE_CLASSES)
                        && !holds_class(document, child, &LINE_CLASSES, 0)))
            {
                let stated = bbox(&props);
                let (block, paragraph, line) = self.open_line(
                    inner,
                    Some(node),
                    stated_order(&props),
                    stated.extent(),
                    false,
                );
                let words_scope = Scope {
                    block: Some(block),
                    paragraph: Some(paragraph),
                    line: Some(line),
                    ..inner
                };
                self.visit(child, words_scope, depth + 1);
                // A line holding text and no word is one word: its text over
                // its box.
                if let Some(l) = self.line(block, paragraph, line) {
                    if l.line.words.is_empty() {
                        let mut text = String::new();
                        text_content(document, child, &mut text, 0);
                        let number = l.number;
                        l.line.words.push(DraftWord {
                            id: element.attribute("id").map(str::to_string),
                            text,
                            stated,
                            confidence: None,
                            line: number,
                            word: 0,
                        });
                    }
                }
            } else if has_class(element, &["ocrx_word"]) {
                let (block, paragraph, line) = match scope.line {
                    Some(line) => {
                        let (block, paragraph) = self.paragraph_for(scope);
                        (block, paragraph, line)
                    }
                    None => {
                        let (block, paragraph) = self.paragraph_for(scope);
                        match self.paragraph(block, paragraph).and_then(|p| p.loose) {
                            Some(line) => (block, paragraph, line),
                            None => self.open_line(inner, None, None, None, true),
                        }
                    }
                };
                let mut text = String::new();
                text_content(document, child, &mut text, 0);
                let confidence = property(&props, "x_wconf")
                    .and_then(|values| values.first())
                    .map(|value| Confidence {
                        stated: value.clone(),
                        scale: 100.0,
                    });
                if let Some(l) = self.line(block, paragraph, line) {
                    let word = l.line.words.len();
                    let number = l.number;
                    l.line.words.push(DraftWord {
                        id: element.attribute("id").map(str::to_string),
                        text,
                        stated: bbox(&props),
                        confidence,
                        line: number,
                        word,
                    });
                }
            } else {
                self.visit(child, inner, depth + 1);
            }
        }
    }
}

/// The hOCR reader: each `ocr_page` of the document.
fn read_hocr(document: &html::Document, pages: &[usize]) -> Result<Vec<DraftPage>, ImportError> {
    let mut drafts = Vec::with_capacity(pages.len());
    for (position, &page) in pages.iter().enumerate() {
        let Some(element) = document.node(page).and_then(html::Node::element) else {
            continue;
        };
        let props = element
            .attribute("title")
            .map(properties)
            .unwrap_or_default();
        let extent = page_extent(bbox(&props), position)?;
        let index = match property(&props, "ppageno") {
            Some([n]) => n.trim().parse::<u32>().ok(),
            _ => None,
        }
        .unwrap_or_else(|| u32::try_from(position).unwrap_or(u32::MAX));
        let image = match property(&props, "image") {
            Some([name]) if !name.is_empty() => Some(name.clone()),
            _ => None,
        };
        let resolution = match property(&props, "scan_res") {
            Some([x, y]) => match (number(x), number(y)) {
                (Some(x), Some(y)) if x > 0.0 && y > 0.0 => Some((x, y)),
                _ => None,
            },
            _ => None,
        };
        let mut scope = Scope {
            block: None,
            paragraph: None,
            line: None,
            turn: property(&props, "textangle")
                .and_then(|values| values.first())
                .and_then(|value| number(value)),
            direction: None,
        };
        if let Some(dir) = element.attribute("dir") {
            if dir.eq_ignore_ascii_case("rtl") {
                scope.direction = Some(Direction::RightToLeft);
            } else if dir.eq_ignore_ascii_case("ltr") {
                scope.direction = Some(Direction::LeftToRight);
            }
        } else {
            scope.direction = inherited_dir(document, page);
        }
        let mut walk = HocrWalk {
            document,
            blocks: Vec::new(),
            loose: None,
            lines: 0,
        };
        walk.visit(page, scope, 0);

        let blocks = by_stated_order(
            walk.blocks
                .into_iter()
                .map(|block| (block.parent, block.order, block))
                .collect(),
        )
        .into_iter()
        .map(|block| DraftBlock {
            stated: block.stated,
            paragraphs: block
                .paragraphs
                .into_iter()
                .map(|paragraph| DraftParagraph {
                    stated: paragraph.stated,
                    lines: by_stated_order(
                        paragraph
                            .lines
                            .into_iter()
                            .map(|line| (line.parent, line.order, line.line))
                            .collect(),
                    ),
                })
                .collect(),
        })
        .collect();
        drafts.push(DraftPage {
            position,
            index,
            extent,
            unit: Unit::Pixel,
            resolution,
            image,
            blocks,
        });
    }
    Ok(drafts)
}

/// The direction a page inherits from the elements around it: the nearest
/// `dir` of `ltr` or `rtl` among its ancestors, as HTML inherits it.
fn inherited_dir(document: &html::Document, node: usize) -> Option<Direction> {
    let mut walk = document.node(node).and_then(|n| n.parent);
    let mut steps = 0;
    while let Some(at) = walk {
        let n = document.node(at)?;
        if let Some(dir) = n.element().and_then(|e| e.attribute("dir")) {
            if dir.eq_ignore_ascii_case("rtl") {
                return Some(Direction::RightToLeft);
            }
            if dir.eq_ignore_ascii_case("ltr") {
                return Some(Direction::LeftToRight);
            }
            if dir.eq_ignore_ascii_case("auto") {
                return None;
            }
        }
        steps += 1;
        if steps > MAX_WALK_DEPTH {
            return None;
        }
        walk = n.parent;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_title_reads_by_its_grammar() {
        let read = properties("image \"a b;c.png\"; bbox 0 0 10.5 20 ;x_wconf 93;  ppageno 7");
        assert_eq!(
            read,
            vec![
                ("image".to_string(), vec!["a b;c.png".to_string()]),
                (
                    "bbox".to_string(),
                    vec!["0".into(), "0".into(), "10.5".into(), "20".into()]
                ),
                ("x_wconf".to_string(), vec!["93".to_string()]),
                ("ppageno".to_string(), vec!["7".to_string()]),
            ]
        );
        assert!(properties("").is_empty());
        assert_eq!(properties("x \"\"")[0].1, vec![String::new()]);
    }

    #[test]
    fn text_is_cleaned_and_named() {
        let at = Located {
            page: 0,
            id: None,
            line: 0,
            word: 0,
        };
        let mut warnings = Vec::new();
        assert_eq!(
            cleaned(
                " \t a\u{1}b \n\r c\u{FEFF}\u{FDD0}\u{10FFFF}\u{A0} ",
                &at,
                &mut warnings
            ),
            "ab c\u{A0}"
        );
        let removed: Vec<char> = warnings
            .iter()
            .map(|w| match w {
                ImportWarning::NotText { character, .. } => *character,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(removed, ['\u{1}', '\u{FEFF}', '\u{FDD0}', '\u{10FFFF}']);
    }

    #[test]
    fn stated_order_reorders_only_whole_sibling_groups() {
        let items = vec![
            (Some(1), Some(2), "a"),
            (Some(1), Some(1), "b"),
            (Some(2), Some(9), "c"),
            (Some(2), None, "d"),
            (None, Some(0), "e"),
        ];
        assert_eq!(by_stated_order(items), ["b", "a", "c", "d", "e"]);
    }

    #[test]
    fn named_blocks_come_first_and_once() {
        assert_eq!(named_first(4, &[2, 0, 2, 9]), [2, 0, 1, 3]);
        assert_eq!(named_first(2, &[]), [0, 1]);
    }

    #[test]
    fn angles_run_from_zero_up_to_a_whole_turn() {
        assert_eq!(normalised(-90.0), 270.0);
        assert_eq!(normalised(450.0), 90.0);
        assert_eq!(normalised(f64::NAN), 0.0);
        assert_eq!(whole(2479.2), 2480);
        assert_eq!(whole(-3.0), 0);
        assert_eq!(whole(1e300), u32::MAX);
    }
}
