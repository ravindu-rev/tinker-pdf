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
//! [`crate::Page::text_in`] names the three. [`ReadingOrder::Inferred`] is
//! the one this module adds, and until the inference lands it declines
//! ([`DeclineReason::NotImplemented`]) with the stream's blocks unmoved —
//! milestone 1 of `docs/design/reading-order.md`, which lands the types and
//! the measurement before the guess, so that "the inference improved on the
//! stream" is a number and not a hope.
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

use tinker_pdf_content::{Quad, TextChar, TextLine, TextPage, WritingMode};
use tinker_pdf_cos::CosDocument;

use crate::observe::Observed;
use crate::structure::StructuredText;
use crate::{Document, Page};

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
    /// The inference has not landed yet: milestone 1 of
    /// `docs/design/reading-order.md` names the answer before it guesses one.
    NotImplemented,
}

/// What an inference had to tolerate, or would not do (ruling 10).
///
/// A page this module inferred an order for and a page it was unsure of are
/// told apart here, by name; there is no probability, because no calibration
/// data exists to make one honest.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum InferenceWarning {
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

/// Infers an order for `page` — which, until the inference lands, is the
/// stream's, declined by name.
fn infer(page: &TextPage) -> InferredOrder {
    if page
        .blocks
        .iter()
        .all(|b| b.lines.iter().all(|l| l.chars.is_empty()))
    {
        return InferredOrder {
            columns: 1,
            warnings: vec![InferenceWarning::NoBodyText],
            ..InferredOrder::default()
        };
    }
    declined(page, DeclineReason::NotImplemented, Vec::new())
}

/// A line, or the part of one that falls in one column.
#[derive(Clone)]
struct Piece {
    chars: Vec<usize>,
    bounds: (f64, f64, f64, f64),
    size: f64,
    wmode: WritingMode,
    rtl: bool,
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
    for block in &page.blocks {
        let pieces = block
            .lines
            .iter()
            .map(|line| {
                let chars: Vec<usize> = (next..next + line.chars.len()).collect();
                next += line.chars.len();
                piece_of(chars, line, &flat)
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

fn piece_of(chars: Vec<usize>, line: &TextLine, flat: &[&TextChar]) -> Piece {
    let bounds = bounds_of(&chars, flat, &line.quad);
    let size = chars
        .iter()
        .filter_map(|at| flat.get(*at))
        .map(|c| c.size)
        .filter(|s| s.is_finite())
        .fold(0.0f64, f64::max);
    Piece {
        chars,
        bounds,
        size,
        wmode: line.wmode,
        rtl: line.rtl,
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
