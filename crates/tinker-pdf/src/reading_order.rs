//! Reading order, named: the content stream's, the structure tree's, and one
//! this engine **infers** from geometry for a page that states none.
//!
//! [`crate::Page::text`] reports a page in the order its content stream drew
//! it — `TextDevice` sorts nothing, and the facade sorts only within a line
//! holding a right-to-left character (ruling 14). A tagged page states its
//! own order in its structure tree (14.8), which [`crate::Page::structured_text`]
//! joins. An untagged page states nothing, and a two-column page whose
//! producer wrote line by line across both columns reads interleaved.
//!
//! [`crate::Page::text_in`] names the three, and [`ReadingOrder::Inferred`] is
//! the one this module adds: columns found from the whitespace between lines,
//! blocks ordered top to bottom inside a column, a block that crosses a column
//! boundary placed before the columns under it. Design:
//! `docs/design/reading-order.md`.
//!
//! # The label is a type
//!
//! A wrong inference is not a crash and not a warning: it is a page read in an
//! order a person would not read it, silently. So the inferred answer is an
//! [`InferredOrder`] — never a [`TextPage`], never the default, and never
//! merged into what [`crate::Page::text`] returns — and it carries
//! [`InferredOrder::permutation`], the stream position of every character it
//! placed, so a caller who finds it wrong has the stream order one index
//! lookup away. Nothing that reads a page without asking — search, selection,
//! redaction, the structured view — can see an inference exist.
//!
//! A guess is never preferred to a statement: on a page whose document carries
//! a structure tree, [`crate::Page::text_in`] with [`ReadingOrder::Inferred`]
//! answers with the tree's order, labelled [`OrderedText::Stated`], and
//! [`crate::Page::inferred_order`] declines ([`DeclineReason::TreePresent`]).
//! [`InferenceOptions::hide_structure`] is the one way past that, and it
//! exists so the inference can be **measured** against the tree it hid.
//!
//! # Every threshold is a constant
//!
//! In ems of the page's median line size, so a page set at 7 pt and one at
//! 12 pt meet the same rule, and each is named once below. Nothing here needs
//! a transcendental, so the same page gives the same order on every target
//! (ruling 4).

use std::collections::BTreeMap;

use tinker_pdf_content::{Quad, TextChar, TextLine, TextPage, WritingMode};
use tinker_pdf_cos::CosDocument;

use crate::observe::Observed;
use crate::structure::StructuredText;
use crate::{Document, Page};

/// The narrowest vertical whitespace gap that can separate two columns, in
/// ems of the page's median line size.
///
/// **Four fifths of an em, not the design's one and a half**, and the
/// evidence is two producers' defaults. `css-multicol-1` §4.1 makes
/// `column-gap: normal` 1em, and the EPUB path lays it out so; LaTeX's
/// `\columnsep` is 10 pt in a 10 pt document. A justified column ends exactly
/// at its edge, so the whitespace between two such columns is the gap itself,
/// and at 1.5 em both read as one column, interleaved — the failure the
/// inference exists to undo (`a_two_column_book_reads_down_its_columns`). One
/// em exactly would be a threshold met to the last bit of a sum of advances,
/// so the rule sits a fifth below it. A word space is a quarter to a third of
/// an em, and a gap must also be free of text over [`COLUMN_FREE_SHARE`] of the
/// body's height, which no run of word spaces is.
pub const COLUMN_GAP_EMS: f64 = 0.8;

/// The share of the body's height a column gap must be free of text over.
///
/// A gap crossed by lines over more than the rest — `1 - 0.6` of the height —
/// is a page whose text runs across it, which is one column. A full-width
/// heading over two columns crosses the gap for a line or two and leaves it.
pub const COLUMN_FREE_SHARE: f64 = 0.6;

/// The narrowest a column of text may be, in ems, on either side of a gap.
///
/// A column of prose is set to a measure of a dozen ems and more; a column
/// of short labels beside their values is a few, and reading every label
/// before any value is the one order nobody wants. So a gap whose text on
/// either side is narrower than this separates nothing, and the near miss is
/// named. Not in the design, which did not meet the form; added with
/// `labels_beside_their_values_are_one_column` beside it.
pub const COLUMN_MIN_WIDTH_EMS: f64 = 8.0;

/// How far apart, in ems, the tops of two blocks may be and still be one row,
/// read in the order the content stream drew them.
///
/// Two cells of a table row, two labels on one baseline, and the halves of a
/// line a producer drew in two pieces all sit at one height, and the stream's
/// order between them is the only evidence there is about which comes first.
pub const ROW_TOLERANCE_EMS: f64 = 0.25;

/// The vertical gap, in line heights, past which two lines are two blocks.
///
/// `TextDevice`'s own rule (1.5), applied again after columns are found,
/// because a block of the content stream may hold lines of two columns.
pub const BLOCK_GAP_LINES: f64 = 1.5;

/// The ratio of line sizes past which two adjacent lines are two blocks: a
/// heading over its paragraph, a footnote under its body.
pub const BLOCK_SIZE_RATIO: f64 = 0.9;

/// Fewer horizontal lines than this and a page's columns are not looked for.
///
/// Two lines side by side are a column gap by the arithmetic and a label and
/// its value by any reading, and there is no third line to say which.
pub const MIN_COLUMN_LINES: usize = 3;

/// Which order a caller asks [`crate::Page::text_in`] for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ReadingOrder {
    /// The order the content stream drew the page in: [`crate::Page::text`],
    /// named. The default, as it always has been.
    #[default]
    Stream,
    /// The order the structure tree states (14.8): [`crate::Page::structured_text`].
    /// There is none for an untagged document.
    Stated,
    /// An order this engine inferred from the page's geometry, labelled as
    /// such: [`crate::Page::inferred_order`].
    Inferred,
}

/// How [`crate::Page::inferred_order`] reads the page.
///
/// `Default` is the inference a caller wants: on a page whose document is
/// tagged, it declines in favour of the tree.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InferenceOptions {
    /// Infer even where the document carries a structure tree, and read the
    /// page as an untagged reader would — `/Artifact` scopes (14.8.2.2)
    /// extracted as content rather than dropped.
    ///
    /// For **measuring** the inference against the tree it hid, which is the
    /// only third-party statement about reading order there is at scale. The
    /// answer carries [`InferenceWarning::TreePresent`], and its
    /// [`InferredOrder::permutation`] indexes the page read that way, which
    /// holds the artifacts' characters as well as [`crate::Page::text`]'s.
    pub hide_structure: bool,
}

/// A page's text in the order a caller asked for, labelled by which order it
/// is.
#[derive(Clone, Debug)]
pub enum OrderedText {
    /// The content stream's order — [`crate::Page::text`].
    Stream(TextPage),
    /// The structure tree's order — [`crate::Page::structured_text`].
    Stated(StructuredText),
    /// An order inferred from geometry — [`crate::Page::inferred_order`].
    Inferred(InferredOrder),
}

impl OrderedText {
    /// Which order this is: what the answer *is*, which for a request for
    /// [`ReadingOrder::Inferred`] on a tagged page is [`ReadingOrder::Stated`].
    #[must_use]
    pub fn order(&self) -> ReadingOrder {
        match self {
            OrderedText::Stream(_) => ReadingOrder::Stream,
            OrderedText::Stated(_) => ReadingOrder::Stated,
            OrderedText::Inferred(_) => ReadingOrder::Inferred,
        }
    }

    /// The text, one line per line (per run, for the stated order).
    #[must_use]
    pub fn plain_text(&self) -> String {
        match self {
            OrderedText::Stream(page) => page.plain_text(),
            OrderedText::Stated(text) => text.plain_text(),
            OrderedText::Inferred(order) => order.plain_text(),
        }
    }
}

/// What a block of an inferred order is taken to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Role {
    /// The page's body text.
    Body,
    /// Text repeated at the top of the page across pages.
    RunningHead,
    /// Text repeated at the foot of the page across pages.
    RunningFoot,
    /// A numeral alone at the head or foot of the page that counts across
    /// pages.
    PageNumber,
    /// A note set below the body it annotates.
    Footnote,
    /// A caption under a figure.
    Caption,
    /// Text the inference could not place: rotated or vertical lines, and the
    /// margin blocks of a page read with no other page to compare it with.
    Unplaced,
}

/// Why an inference declined to guess at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DeclineReason {
    /// The document carries a structure tree, which states the order; see
    /// [`InferenceOptions::hide_structure`].
    TreePresent,
    /// Most of the page's lines are vertical, which nothing here orders.
    VerticalWriting,
    /// Most of the page's lines run at an angle, which nothing here orders.
    RotatedText,
}

/// What an inference had to tolerate, or would not do (ruling 10).
///
/// A page this module inferred an order for and a page it was unsure of are
/// told apart here, by name; there is no probability, because no calibration
/// data exists to make one honest.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum InferenceWarning {
    /// A whitespace gap came within a factor of two of separating columns and
    /// did not: the page was read as one column. `gap` is its width, in
    /// points.
    ColumnsAmbiguous {
        /// The near miss's width, in points.
        gap: f64,
    },
    /// Fewer than [`MIN_COLUMN_LINES`] lines of body text, so no columns were
    /// looked for.
    PageTooSparse {
        /// How many there were.
        lines: usize,
    },
    /// Lines that run at an angle, placed last as [`Role::Unplaced`].
    RotatedText {
        /// How many.
        lines: usize,
    },
    /// Vertical lines, placed last as [`Role::Unplaced`].
    VerticalWriting {
        /// How many.
        lines: usize,
    },
    /// The page has no text at all.
    NoBodyText,
    /// The document carries a structure tree, and this inference was asked to
    /// hide it ([`InferenceOptions::hide_structure`]): the answer is a
    /// measurement, not a reading.
    TreePresent,
    /// The inference declined to guess; the blocks are the stream's, each
    /// [`Role::Unplaced`].
    Declined {
        /// Why.
        reason: DeclineReason,
    },
}

/// One block of an inferred order.
#[derive(Clone, Debug)]
pub struct InferredBlock {
    /// What the block is taken to be.
    pub role: Role,
    /// Which band of the page it is in, counted from the top: a block that
    /// crosses a column boundary — a heading over two columns, a paragraph set
    /// across the page between two sets of columns — ends one band and heads
    /// the next. 0 on a page with no such block.
    pub section: usize,
    /// The column it was assigned to within its band, counted in reading order
    /// from 0 — so on a right-to-left page column 0 is the rightmost. `None`
    /// for a block that crosses a column boundary and for one outside the
    /// body.
    pub column: Option<usize>,
    /// Its lines, in the order they are read. Each holds characters of the
    /// same [`TextPage`] the order was inferred over, unchanged; a line of the
    /// page that ran across a column boundary is here as one line per column.
    pub lines: Vec<TextLine>,
    /// The block's bounding quad.
    pub quad: Quad,
    /// Where the block's first character sits in
    /// [`InferredOrder::permutation`].
    pub start: usize,
}

/// A page's text in an order this engine **inferred** from geometry.
///
/// Not the file's order — see the module documentation for why that is a type
/// rather than a flag.
#[derive(Clone, Debug, Default)]
pub struct InferredOrder {
    /// The blocks, in inferred reading order.
    pub blocks: Vec<InferredBlock>,
    /// How many columns the body was read as; 1 when no gap separated any.
    pub columns: usize,
    /// For each character in inferred order, its position in the page's
    /// characters in stream order — blocks, then lines, then characters, of
    /// the page it was inferred over. A permutation of `0..n`, so the stream
    /// order is recovered by inverting it.
    pub permutation: Vec<usize>,
    /// What the inference had to tolerate.
    pub warnings: Vec<InferenceWarning>,
}

impl InferredOrder {
    /// The text in inferred order, one line per line.
    #[must_use]
    pub fn plain_text(&self) -> String {
        let mut out = String::new();
        for block in &self.blocks {
            for line in &block.lines {
                out.push_str(&line.text);
                out.push('\n');
            }
        }
        out
    }

    /// Why the inference declined, if it did.
    #[must_use]
    pub fn declined(&self) -> Option<DeclineReason> {
        self.warnings.iter().find_map(|w| match w {
            InferenceWarning::Declined { reason } => Some(*reason),
            _ => None,
        })
    }

    /// How many characters stand somewhere other than their stream position.
    ///
    /// Zero for a page the inference left exactly as the content stream drew
    /// it — which is every one-column page drawn top to bottom.
    #[must_use]
    pub fn moved(&self) -> usize {
        self.permutation
            .iter()
            .enumerate()
            .filter(|(at, from)| at != *from)
            .count()
    }

    /// Every character, in inferred order.
    #[must_use]
    pub fn chars(&self) -> Vec<&TextChar> {
        self.blocks
            .iter()
            .flat_map(|b| b.lines.iter())
            .flat_map(|l| l.chars.iter())
            .collect()
    }
}

impl Page {
    /// The page's text in the order `order` names, labelled by the order it
    /// is.
    ///
    /// [`ReadingOrder::Stream`] is [`Page::text`]; [`ReadingOrder::Stated`]
    /// is [`Page::structured_text`], `None` for an untagged document;
    /// [`ReadingOrder::Inferred`] is [`Page::inferred_order`] with its
    /// defaults — except on a page whose document carries a structure tree,
    /// where it is the tree's order, [`OrderedText::Stated`]: a guess is never
    /// preferred to a statement.
    #[must_use]
    pub fn text_in(&self, order: ReadingOrder) -> Option<OrderedText> {
        match order {
            ReadingOrder::Stream => Some(OrderedText::Stream(self.text())),
            ReadingOrder::Stated => self.structured_text().map(OrderedText::Stated),
            ReadingOrder::Inferred => match self.structured_text() {
                Some(stated) => Some(OrderedText::Stated(stated)),
                None => Some(OrderedText::Inferred(
                    self.inferred_order(&InferenceOptions::default()),
                )),
            },
        }
    }

    /// The page's text in an order inferred from its geometry.
    ///
    /// Opt-in and never the default; see the module documentation. On a page
    /// whose document carries a structure tree it declines
    /// ([`DeclineReason::TreePresent`]) unless
    /// [`InferenceOptions::hide_structure`] is set.
    #[must_use]
    pub fn inferred_order(&self, options: &InferenceOptions) -> InferredOrder {
        let tagged = has_structure_tree(&self.doc);
        if tagged && !options.hide_structure {
            let observed = Observed::read(self, false);
            return declined(&observed.text, DeclineReason::TreePresent, Vec::new());
        }
        let observed = Observed::read(self, options.hide_structure);
        let mut order = infer(&observed.text);
        if tagged {
            order.warnings.insert(0, InferenceWarning::TreePresent);
        }
        order
    }
}

impl Document {
    /// Page `index`'s text in an order inferred from its geometry —
    /// [`Page::inferred_order`], for a page of this document.
    ///
    /// `None` when there is no such page.
    #[must_use]
    pub fn inferred_order(&self, index: u32, options: &InferenceOptions) -> Option<InferredOrder> {
        Some(self.page(index)?.inferred_order(options))
    }
}

/// Whether the catalog names a structure tree root that is a dictionary —
/// the test [`crate::structure`]'s binding starts with, without the walk.
fn has_structure_tree(doc: &CosDocument) -> bool {
    doc.catalog().is_some_and(|catalog| {
        doc.resolve_key(&catalog, doc.intern(b"StructTreeRoot"))
            .as_dict()
            .is_some()
    })
}

// ---------------------------------------------------------------------------
// The inference
// ---------------------------------------------------------------------------

/// One line of the page, as the inference sees it.
struct Line<'a> {
    /// Which `TextDevice` block it came from.
    block: usize,
    /// Its characters' stream indices, in the line's own order.
    chars: Vec<usize>,
    source: &'a TextLine,
    bounds: (f64, f64, f64, f64),
}

/// A line, or the part of one that falls in one column.
#[derive(Clone)]
struct Piece {
    block: usize,
    chars: Vec<usize>,
    bounds: (f64, f64, f64, f64),
    size: f64,
    wmode: WritingMode,
    rtl: bool,
}

impl Piece {
    fn top(&self) -> f64 {
        self.bounds.3
    }

    fn first(&self) -> usize {
        self.chars.first().copied().unwrap_or(usize::MAX)
    }

    fn middle(&self) -> f64 {
        (self.bounds.1 + self.bounds.3) / 2.0
    }
}

/// Blocks, before they are written out.
struct Draft {
    role: Role,
    section: usize,
    column: Option<usize>,
    pieces: Vec<Piece>,
}

/// The order of `page`, as the stream gave it, every block
/// [`Role::Unplaced`] — what an inference that declines returns.
fn declined(
    page: &TextPage,
    reason: DeclineReason,
    mut warnings: Vec<InferenceWarning>,
) -> InferredOrder {
    let flat = flatten(page);
    let mut next = 0usize;
    let mut drafts = Vec::new();
    for (index, block) in page.blocks.iter().enumerate() {
        let pieces = block
            .lines
            .iter()
            .map(|line| {
                let chars: Vec<usize> = (next..next + line.chars.len()).collect();
                next += line.chars.len();
                piece_of(index, chars, line, &flat)
            })
            .collect();
        drafts.push(Draft {
            role: Role::Unplaced,
            section: 0,
            column: None,
            pieces,
        });
    }
    warnings.push(InferenceWarning::Declined { reason });
    write_out(drafts, &flat, 1, warnings)
}

/// Every character of `page`, in stream order.
fn flatten(page: &TextPage) -> Vec<&TextChar> {
    page.blocks
        .iter()
        .flat_map(|b| b.lines.iter())
        .flat_map(|l| l.chars.iter())
        .collect()
}

/// The enclosing rectangle of `chars`, as `(x0, y0, x1, y1)`; the line's own
/// when no character's quad is finite.
fn bounds_of(chars: &[usize], flat: &[&TextChar], fallback: &Quad) -> (f64, f64, f64, f64) {
    let mut out: Option<(f64, f64, f64, f64)> = None;
    for at in chars {
        let Some(c) = flat.get(*at) else { continue };
        if !c.quad.is_finite() {
            continue;
        }
        let (x0, y0, x1, y1) = c.quad.bounds();
        out = Some(match out {
            None => (x0, y0, x1, y1),
            Some((a, b, c, d)) => (a.min(x0), b.min(y0), c.max(x1), d.max(y1)),
        });
    }
    out.unwrap_or_else(|| {
        let b = fallback.bounds();
        if [b.0, b.1, b.2, b.3].iter().all(|v| v.is_finite()) {
            b
        } else {
            (0.0, 0.0, 0.0, 0.0)
        }
    })
}

fn piece_of(block: usize, chars: Vec<usize>, line: &TextLine, flat: &[&TextChar]) -> Piece {
    let bounds = bounds_of(&chars, flat, &line.quad);
    let size = chars
        .iter()
        .filter_map(|at| flat.get(*at))
        .map(|c| c.size)
        .filter(|s| s.is_finite())
        .fold(0.0f64, f64::max);
    Piece {
        block,
        chars,
        bounds,
        size,
        wmode: line.wmode,
        rtl: line.rtl,
    }
}

/// How a line runs: along the page's `x` axis, at an angle, or down.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Run {
    Across,
    Angled,
    Down,
}

fn run_of(line: &TextLine) -> Run {
    if line.wmode == WritingMode::Vertical {
        return Run::Down;
    }
    for c in &line.chars {
        let (dx, dy) = (c.quad.lr.0 - c.quad.ll.0, c.quad.lr.1 - c.quad.ll.1);
        if !(dx.is_finite() && dy.is_finite()) || (dx.abs() < 1e-9 && dy.abs() < 1e-9) {
            continue;
        }
        // Within about six degrees of the page's `x` axis, running forwards.
        return if dx > 0.0 && dy.abs() <= dx * 0.1 {
            Run::Across
        } else {
            Run::Angled
        };
    }
    Run::Across
}

/// The median of `values`, or `None` for none.
fn median(mut values: Vec<f64>) -> Option<f64> {
    values.retain(|v| v.is_finite() && *v > 0.0);
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    values.get(values.len() / 2).copied()
}

/// Infers an order for `page`.
fn infer(page: &TextPage) -> InferredOrder {
    let flat = flatten(page);
    let mut warnings = Vec::new();
    if flat.is_empty() {
        warnings.push(InferenceWarning::NoBodyText);
        return InferredOrder {
            columns: 1,
            warnings,
            ..InferredOrder::default()
        };
    }

    // The page's lines, with the stream index of every character.
    let mut lines: Vec<Line<'_>> = Vec::new();
    let mut next = 0usize;
    for (block, b) in page.blocks.iter().enumerate() {
        for source in &b.lines {
            let chars: Vec<usize> = (next..next + source.chars.len()).collect();
            next += source.chars.len();
            let bounds = bounds_of(&chars, &flat, &source.quad);
            lines.push(Line {
                block,
                chars,
                source,
                bounds,
            });
        }
    }

    let (mut across, mut angled, mut down) = (Vec::new(), Vec::new(), Vec::new());
    for (at, line) in lines.iter().enumerate() {
        match run_of(line.source) {
            Run::Across => across.push(at),
            Run::Angled => angled.push(at),
            Run::Down => down.push(at),
        }
    }
    let total = lines.len();
    if down.len() * 2 > total {
        return declined(page, DeclineReason::VerticalWriting, warnings);
    }
    if angled.len() * 2 > total {
        return declined(page, DeclineReason::RotatedText, warnings);
    }
    if !angled.is_empty() {
        warnings.push(InferenceWarning::RotatedText {
            lines: angled.len(),
        });
    }
    if !down.is_empty() {
        warnings.push(InferenceWarning::VerticalWriting { lines: down.len() });
    }

    let em = median(
        across
            .iter()
            .filter_map(|at| lines.get(*at))
            .map(|l| l.source.size)
            .collect(),
    )
    .unwrap_or(10.0);
    let rtl_page = {
        let rtl = across
            .iter()
            .filter_map(|at| lines.get(*at))
            .filter(|l| l.source.rtl)
            .count();
        rtl * 2 > across.len()
    };

    let body: Vec<&Line<'_>> = across.iter().filter_map(|at| lines.get(*at)).collect();

    // Columns.
    let columns = if body.len() < MIN_COLUMN_LINES {
        warnings.push(InferenceWarning::PageTooSparse { lines: body.len() });
        Columns::default()
    } else {
        find_columns(&body, &flat, em)
    };
    if let Some(gap) = columns.near_miss {
        warnings.push(InferenceWarning::ColumnsAmbiguous { gap });
    }
    let count = columns.cuts.len() + 1;

    // Each body line to its column, cut where it runs across a gap without
    // touching it, or held whole as a spanner where it does touch it.
    let mut placed: Vec<(Option<usize>, Piece)> = Vec::new();
    for line in &body {
        for (column, chars) in columns.split(line, &flat) {
            let column = column.map(|c| if rtl_page { count - 1 - c } else { c });
            placed.push((column, piece_of(line.block, chars, line.source, &flat)));
        }
    }

    let drafts = order_body(placed, count, em);
    let mut drafts: Vec<Draft> = drafts;

    // Lines nothing here orders, last and in stream order.
    let mut unplaced: Vec<Piece> = angled
        .iter()
        .chain(down.iter())
        .filter_map(|at| lines.get(*at))
        .map(|line| piece_of(line.block, line.chars.clone(), line.source, &flat))
        .collect();
    unplaced.sort_by_key(Piece::first);
    if !unplaced.is_empty() {
        let section = drafts.last().map_or(0, |d| d.section);
        drafts.push(Draft {
            role: Role::Unplaced,
            section,
            column: None,
            pieces: unplaced,
        });
    }

    write_out(drafts, &flat, count, warnings)
}

/// The body, ordered: section by section down the page — a section being what
/// lies between two lines that cross a column gap — each section's columns in
/// reading order, and each spanner before the section under it.
///
/// `O(n log n)` in the pieces: a piece's section is a binary search, and only
/// the `(section, column)` cells that hold something exist, because a hostile
/// page could otherwise ask for a grid of every spanner by every column.
fn order_body(placed: Vec<(Option<usize>, Piece)>, columns: usize, em: f64) -> Vec<Draft> {
    let tolerance = ROW_TOLERANCE_EMS * em;
    let (spanners, pieces): (Vec<_>, Vec<_>) =
        placed.into_iter().partition(|(column, _)| column.is_none());
    let mut spanners: Vec<Piece> = spanners.into_iter().map(|(_, p)| p).collect();
    order_rows(&mut spanners, tolerance);

    // A piece's section is how many spanners stand above it.
    let mut mids: Vec<f64> = spanners.iter().map(Piece::middle).collect();
    mids.sort_by(f64::total_cmp);
    let section_of = |piece: &Piece| -> usize {
        let mid = piece.middle();
        mids.len() - mids.partition_point(|m| *m <= mid)
    };

    // (section, column) -> the pieces in it.
    let mut cells: BTreeMap<(usize, usize), Vec<Piece>> = BTreeMap::new();
    for (column, piece) in pieces {
        let column = column.unwrap_or(0).min(columns.saturating_sub(1));
        cells
            .entry((section_of(&piece), column))
            .or_default()
            .push(piece);
    }

    // Spanners with no section between them are one run, so a full-width
    // paragraph of several lines is one block rather than one per line. A run
    // heads the band under it.
    let mut drafts = Vec::new();
    let mut spanners = spanners.into_iter();
    let mut emitted = 0usize;
    let mut band = 0usize;
    for ((section, column), cell) in cells {
        if emitted < section {
            let run: Vec<Piece> = spanners.by_ref().take(section - emitted).collect();
            emitted = section;
            if !run.is_empty() {
                band += 1;
                push_blocks(&mut drafts, run, Role::Body, band, None);
            }
        }
        for unit in units(cell, tolerance) {
            push_blocks(&mut drafts, unit, Role::Body, band, Some(column));
        }
    }
    let rest: Vec<Piece> = spanners.collect();
    if !rest.is_empty() {
        let band = if drafts.is_empty() { band } else { band + 1 };
        push_blocks(&mut drafts, rest, Role::Body, band, None);
    }
    // Bands are counted from 0 at the top of the page.
    if let Some(first) = drafts.first().map(|d| d.section) {
        for draft in &mut drafts {
            draft.section -= first;
        }
    }
    drafts
}

/// `pieces` grouped by the stream block they came from, each group ordered
/// down the page, and the groups ordered by their tops — the row tolerance
/// leaving the stream's order wherever two stand level.
fn units(pieces: Vec<Piece>, tolerance: f64) -> Vec<Vec<Piece>> {
    let mut groups: Vec<Vec<Piece>> = Vec::new();
    let mut by_block: BTreeMap<usize, usize> = BTreeMap::new();
    for piece in pieces {
        match by_block
            .get(&piece.block)
            .and_then(|at| groups.get_mut(*at))
        {
            Some(group) => group.push(piece),
            None => {
                by_block.insert(piece.block, groups.len());
                groups.push(vec![piece]);
            }
        }
    }
    for group in &mut groups {
        order_rows(group, tolerance);
    }
    let top = |g: &Vec<Piece>| g.iter().map(Piece::top).fold(f64::NEG_INFINITY, f64::max);
    let first = |g: &Vec<Piece>| g.iter().map(Piece::first).min().unwrap_or(usize::MAX);
    let mut keyed: Vec<(f64, usize, Vec<Piece>)> = groups
        .into_iter()
        .map(|g| (top(&g), first(&g), g))
        .collect();
    keyed.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    cluster_rows(&mut keyed, tolerance, |k| k.0, |k| k.1);
    keyed.into_iter().map(|(_, _, g)| g).collect()
}

/// Orders pieces down the page: by top, descending, with pieces whose tops
/// are within `tolerance` of the row's first kept in stream order.
fn order_rows(pieces: &mut [Piece], tolerance: f64) {
    pieces.sort_by(|a, b| b.top().total_cmp(&a.top()).then(a.first().cmp(&b.first())));
    cluster_rows(pieces, tolerance, Piece::top, Piece::first);
}

/// Within each run of `items` (already sorted by `top` descending) whose tops
/// lie within `tolerance` of the run's first, restores stream order by
/// `first`.
fn cluster_rows<T>(
    items: &mut [T],
    tolerance: f64,
    top: impl Fn(&T) -> f64,
    first: impl Fn(&T) -> usize,
) {
    let mut start = 0usize;
    while start < items.len() {
        let Some(head) = items.get(start).map(&top) else {
            break;
        };
        let mut end = start + 1;
        while items
            .get(end)
            .is_some_and(|item| head - top(item) <= tolerance)
        {
            end += 1;
        }
        if let Some(run) = items.get_mut(start..end) {
            run.sort_by_key(|item| first(item));
        }
        start = end;
    }
}

/// Cuts an ordered run of pieces into blocks at vertical gaps and size
/// changes, and appends them.
fn push_blocks(
    drafts: &mut Vec<Draft>,
    pieces: Vec<Piece>,
    role: Role,
    section: usize,
    column: Option<usize>,
) {
    let mut current: Vec<Piece> = Vec::new();
    for piece in pieces {
        let breaks = current.last().is_some_and(|prev| {
            let height = (prev.bounds.3 - prev.bounds.1).max(prev.size).max(1.0);
            let gap = (prev.bounds.1 - piece.bounds.3)
                .abs()
                .min((piece.bounds.1 - prev.bounds.3).abs());
            let (a, b) = (prev.size.max(1e-9), piece.size.max(1e-9));
            gap > height * BLOCK_GAP_LINES || a.min(b) / a.max(b) < BLOCK_SIZE_RATIO
        });
        if breaks {
            drafts.push(Draft {
                role,
                section,
                column,
                pieces: std::mem::take(&mut current),
            });
        }
        current.push(piece);
    }
    if !current.is_empty() {
        drafts.push(Draft {
            role,
            section,
            column,
            pieces: current,
        });
    }
}

/// The drafts as an [`InferredOrder`].
fn write_out(
    drafts: Vec<Draft>,
    flat: &[&TextChar],
    columns: usize,
    warnings: Vec<InferenceWarning>,
) -> InferredOrder {
    let mut permutation = Vec::with_capacity(flat.len());
    let mut blocks = Vec::with_capacity(drafts.len());
    for draft in drafts {
        if draft.pieces.is_empty() {
            continue;
        }
        let start = permutation.len();
        let mut lines = Vec::with_capacity(draft.pieces.len());
        for piece in &draft.pieces {
            let chars: Vec<TextChar> = piece
                .chars
                .iter()
                .filter_map(|at| flat.get(*at).map(|c| (*c).clone()))
                .collect();
            permutation.extend(piece.chars.iter().copied());
            let (x0, y0, x1, y1) = piece.bounds;
            lines.push(TextLine {
                text: chars.iter().map(|c| c.text.as_str()).collect(),
                chars,
                quad: rect_quad(x0, y0, x1, y1),
                wmode: piece.wmode,
                rtl: piece.rtl,
                size: piece.size,
            });
        }
        let (x0, y0, x1, y1) = draft.pieces.iter().fold(
            (
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ),
            |(a, b, c, d), p| {
                (
                    a.min(p.bounds.0),
                    b.min(p.bounds.1),
                    c.max(p.bounds.2),
                    d.max(p.bounds.3),
                )
            },
        );
        blocks.push(InferredBlock {
            role: draft.role,
            section: draft.section,
            column: draft.column,
            lines,
            quad: rect_quad(x0, y0, x1, y1),
            start,
        });
    }
    InferredOrder {
        blocks,
        columns,
        permutation,
        warnings,
    }
}

fn rect_quad(x0: f64, y0: f64, x1: f64, y1: f64) -> Quad {
    Quad {
        ul: (x0, y1),
        ur: (x1, y1),
        ll: (x0, y0),
        lr: (x1, y0),
    }
}

// ---------------------------------------------------------------------------
// Columns
// ---------------------------------------------------------------------------

/// The column gaps found on a page, left to right.
#[derive(Default)]
struct Columns {
    /// Where each gap cuts the page along `x`: the middle of the emptiest
    /// stretch of it. A character whose box holds a cut crosses the gap.
    cuts: Vec<f64>,
    /// The widest gap that came within a factor of two of the rule and missed.
    near_miss: Option<f64>,
}

impl Columns {
    /// `line` as one piece per column it has characters in, counted left to
    /// right, or as one spanner (`None`) when a character's box holds a cut —
    /// a heading set across the gap, as opposed to a line a producer drew
    /// across both columns at once, which touches neither.
    fn split(&self, line: &Line<'_>, flat: &[&TextChar]) -> Vec<(Option<usize>, Vec<usize>)> {
        if self.cuts.is_empty() {
            return vec![(Some(0), line.chars.clone())];
        }
        // `cuts` ascends, so each question is a binary search.
        let mut columns: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        let mut last: Option<usize> = None;
        for at in &line.chars {
            let extent = flat.get(*at).and_then(|c| {
                c.quad.is_finite().then(|| {
                    let (x0, _, x1, _) = c.quad.bounds();
                    (x0, x1)
                })
            });
            let Some((x0, x1)) = extent else {
                // A character with no position rides with the one before it.
                columns.entry(last.unwrap_or(0)).or_default().push(*at);
                continue;
            };
            let next = self.cuts.partition_point(|cut| *cut <= x0);
            if self.cuts.get(next).is_some_and(|cut| *cut < x1) {
                return vec![(None, line.chars.clone())];
            }
            let centre = (x0 + x1) / 2.0;
            let column = self.cuts.partition_point(|cut| *cut < centre);
            columns.entry(column).or_default().push(*at);
            last = Some(column);
        }
        columns
            .into_iter()
            .map(|(c, chars)| (Some(c), chars))
            .collect()
    }
}

/// Finds the vertical whitespace gaps that separate columns.
///
/// Each line is cut into fragments at internal gaps of at least
/// [`COLUMN_GAP_EMS`] — a line a producer drew across two columns is two
/// fragments — and the fragments' extents along `x` cut the body into
/// elementary intervals. An interval is *open* when the fragments over it
/// cover at most `1 - COLUMN_FREE_SHARE` of the body's height; a run of open
/// intervals at least [`COLUMN_GAP_EMS`] wide, with text at least
/// [`COLUMN_MIN_WIDTH_EMS`] wide on both sides, is a gap. Every gap is found
/// in one pass, so three columns are two gaps rather than a cut that
/// recurses.
fn find_columns(body: &[&Line<'_>], flat: &[&TextChar], em: f64) -> Columns {
    let gap_min = COLUMN_GAP_EMS * em;
    // (x0, x1, y0, y1)
    let mut fragments: Vec<(f64, f64, f64, f64)> = Vec::new();
    for line in body {
        let mut boxes: Vec<(f64, f64)> = line
            .chars
            .iter()
            .filter_map(|at| flat.get(*at))
            .filter(|c| c.quad.is_finite())
            .map(|c| {
                let (x0, _, x1, _) = c.quad.bounds();
                (x0, x1)
            })
            .collect();
        boxes.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (_, y0, _, y1) = line.bounds;
        let mut current: Option<(f64, f64)> = None;
        for (x0, x1) in boxes {
            current = match current {
                Some((a, b)) if x0 - b <= gap_min => Some((a, b.max(x1))),
                Some((a, b)) => {
                    fragments.push((a, b, y0, y1));
                    Some((x0, x1))
                }
                None => Some((x0, x1)),
            };
        }
        if let Some((a, b)) = current {
            fragments.push((a, b, y0, y1));
        }
    }
    fragments.retain(|f| f.0.is_finite() && f.1.is_finite() && f.1 >= f.0);
    if fragments.len() < MIN_COLUMN_LINES {
        return Columns::default();
    }
    let low = fragments.iter().map(|f| f.2).fold(f64::INFINITY, f64::min);
    let high = fragments
        .iter()
        .map(|f| f.3)
        .fold(f64::NEG_INFINITY, f64::max);
    let height = high - low;
    if !(height.is_finite() && height > 0.0) {
        return Columns::default();
    }

    let mut edges: Vec<f64> = fragments.iter().flat_map(|f| [f.0, f.1]).collect();
    edges.sort_by(f64::total_cmp);
    edges.dedup();
    if edges.len() < 2 {
        return Columns::default();
    }
    // Coverage of each elementary interval `edges[i]..edges[i + 1]`, as a sum
    // of the heights of the fragments over it, by a difference array.
    let index = |x: f64| edges.partition_point(|e| *e < x);
    let mut delta = vec![0.0f64; edges.len() + 1];
    for f in &fragments {
        let (from, to) = (index(f.0), index(f.1));
        if to > from {
            let h = (f.3 - f.2).max(0.0);
            if let Some(d) = delta.get_mut(from) {
                *d += h;
            }
            if let Some(d) = delta.get_mut(to) {
                *d -= h;
            }
        }
    }
    let intervals = edges.len() - 1;
    let mut coverage = Vec::with_capacity(intervals);
    let mut running = 0.0f64;
    for d in delta.iter().take(intervals) {
        running += d;
        coverage.push(running.max(0.0));
    }

    // Maximal runs of intervals at or under `limit`, as index ranges.
    let runs = |limit: f64| -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut start: Option<usize> = None;
        for (i, c) in coverage.iter().enumerate() {
            if *c <= limit {
                start.get_or_insert(i);
            } else if let Some(s) = start.take() {
                out.push((s, i));
            }
        }
        if let Some(s) = start {
            out.push((s, intervals));
        }
        out
    };
    let span =
        |(s, e): (usize, usize)| -> Option<(f64, f64)> { Some((*edges.get(s)?, *edges.get(e)?)) };
    // The text on each side of a run is at least the narrowest column wide,
    // measured over the fragments wholly on that side and no further than
    // the neighbouring gap.
    let extents = Extents::new(&fragments, COLUMN_MIN_WIDTH_EMS * em);

    let mut near_miss: Option<f64> = None;
    let mut miss = |width: f64| near_miss = Some(near_miss.map_or(width, |w: f64| w.max(width)));

    // A run that reaches the first or the last edge is a margin — the ragged
    // end of a column of lines of different lengths — and never a gap, which
    // has text on both sides.
    let inner = |(s, e): &(usize, usize)| *s > 0 && *e < intervals;
    let open_limit = (1.0 - COLUMN_FREE_SHARE) * height;
    let mut candidates: Vec<(usize, usize)> = Vec::new();
    for run in runs(open_limit).into_iter().filter(inner) {
        let Some((a, b)) = span(run) else { continue };
        let width = b - a;
        if width >= gap_min {
            candidates.push(run);
        } else if width * 2.0 >= gap_min && extents.left_of(a) && extents.right_of(b) {
            miss(width);
        }
    }
    // Wide enough, and crossed for too much of the page: free for half the
    // share the rule asks.
    let loose_limit = (1.0 - COLUMN_FREE_SHARE / 2.0) * height;
    for run in runs(loose_limit).into_iter().filter(inner) {
        let Some((a, b)) = span(run) else { continue };
        // Both lists ascend and a strict run lies inside a loose one, so the
        // first candidate starting in this run is the only one to ask about.
        let first = candidates.partition_point(|c| c.0 < run.0);
        let holds_candidate = candidates.get(first).is_some_and(|c| c.1 <= run.1);
        if !holds_candidate && b - a >= gap_min && extents.left_of(a) && extents.right_of(b) {
            miss(b - a);
        }
    }

    // Each candidate needs a column of text on both sides, between it and
    // its neighbours.
    let mut cuts = Vec::new();
    let bounds: Vec<(f64, f64)> = candidates.iter().filter_map(|c| span(*c)).collect();
    for (at, (&run, &(a, b))) in candidates.iter().zip(bounds.iter()).enumerate() {
        let before = at
            .checked_sub(1)
            .and_then(|p| bounds.get(p))
            .map_or(f64::NEG_INFINITY, |g| g.1);
        let after = bounds.get(at + 1).map_or(f64::INFINITY, |g| g.0);
        if !(extents.between(before, a) && extents.between(b, after)) {
            miss(b - a);
            continue;
        }
        // The cut: the middle of the widest of the run's least-covered
        // intervals, so a line that reaches a little into the gap is not
        // taken to cross it.
        let (s, e) = run;
        let least = coverage
            .get(s..e)
            .map_or(0.0, |c| c.iter().copied().fold(f64::INFINITY, f64::min));
        let mut best: Option<(f64, f64)> = None; // (width, middle)
        for i in s..e {
            let (Some(c), Some(lo), Some(hi)) = (coverage.get(i), edges.get(i), edges.get(i + 1))
            else {
                continue;
            };
            if *c <= least && best.is_none_or(|(w, _)| hi - lo > w) {
                best = Some((hi - lo, (lo + hi) / 2.0));
            }
        }
        if let Some((_, middle)) = best {
            cuts.push(middle);
        }
    }
    if !cuts.is_empty() {
        near_miss = None;
    }
    Columns { cuts, near_miss }
}

/// Whether the fragments wholly inside a stretch of `x` make a column: at
/// least two of them, spanning at least `min` from the leftmost start to the
/// rightmost end.
///
/// Every question [`find_columns`] asks is answered without walking every
/// fragment: what lies left of a point and right of one are prefix and
/// suffix sums over two sorted copies, and the stretches between gaps it asks
/// about are disjoint, so walking the fragments that start in each costs the
/// page once. A hostile page of a hundred thousand one-glyph lines is then a
/// sort and not a square.
struct Extents {
    /// `(x1, x0)`, by `x1`, beside the least `x0` up to each.
    by_end: Vec<(f64, f64)>,
    least_start: Vec<f64>,
    /// `(x0, x1)`, by `x0`, beside the greatest `x1` from each on.
    by_start: Vec<(f64, f64)>,
    greatest_end: Vec<f64>,
    min: f64,
}

impl Extents {
    fn new(fragments: &[(f64, f64, f64, f64)], min: f64) -> Extents {
        let mut by_end: Vec<(f64, f64)> = fragments.iter().map(|f| (f.1, f.0)).collect();
        by_end.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut least = f64::INFINITY;
        let least_start = by_end
            .iter()
            .map(|(_, x0)| {
                least = least.min(*x0);
                least
            })
            .collect();
        let mut by_start: Vec<(f64, f64)> = fragments.iter().map(|f| (f.0, f.1)).collect();
        by_start.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut greatest = f64::NEG_INFINITY;
        let mut greatest_end: Vec<f64> = by_start
            .iter()
            .rev()
            .map(|(_, x1)| {
                greatest = greatest.max(*x1);
                greatest
            })
            .collect();
        greatest_end.reverse();
        Extents {
            by_end,
            least_start,
            by_start,
            greatest_end,
            min,
        }
    }

    /// The fragments ending at or before `a`.
    fn left_of(&self, a: f64) -> bool {
        let n = self.by_end.partition_point(|(x1, _)| *x1 <= a);
        let (Some(last), Some(least)) = (
            n.checked_sub(1).and_then(|i| self.by_end.get(i)),
            n.checked_sub(1).and_then(|i| self.least_start.get(i)),
        ) else {
            return false;
        };
        n >= 2 && last.0 - least >= self.min
    }

    /// The fragments starting at or after `b`.
    fn right_of(&self, b: f64) -> bool {
        let from = self.by_start.partition_point(|(x0, _)| *x0 < b);
        let (Some(first), Some(greatest)) = (self.by_start.get(from), self.greatest_end.get(from))
        else {
            return false;
        };
        self.by_start.len() - from >= 2 && greatest - first.0 >= self.min
    }

    /// The fragments wholly between `lo` and `hi`.
    fn between(&self, lo: f64, hi: f64) -> bool {
        let from = self.by_start.partition_point(|(x0, _)| *x0 < lo);
        let (mut count, mut left, mut right) = (0usize, f64::INFINITY, f64::NEG_INFINITY);
        for (x0, x1) in self.by_start.get(from..).unwrap_or_default() {
            if *x0 > hi {
                break;
            }
            if *x1 <= hi {
                count += 1;
                left = left.min(*x0);
                right = right.max(*x1);
            }
        }
        count >= 2 && right - left >= self.min
    }
}
