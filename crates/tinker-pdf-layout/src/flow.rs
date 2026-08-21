//! The box model, block and inline formatting contexts, and margin collapsing.
//!
//! CSS 2.2 §9.4.1 and §9.4.2, `css-box-3`'s box model with `box-sizing`, and
//! §8.3.1's collapsing margins. What comes out is a **linear flow**: one
//! continuous column of items, each with a `y` and a height, which
//! [`crate::fragment`] then cuts into pages.
//!
//! # Margin collapsing has three cases and they are three rules
//!
//! Gap 31's plan calls it *"the rule a first implementation omits and whose
//! omission moves every block on every page"*, and there is a sharper version
//! of that: it is also the rule whose **partial** implementation is most
//! plausible. §8.3.1's three cases are
//!
//! 1. **adjacent siblings** — one box's bottom margin and the next one's top
//!    margin are adjoining;
//! 2. **a parent and its first child** — with no border, padding or clearance
//!    between them, the parent's top margin and the child's are adjoining, and
//!    the same at the bottom when the parent's height is `auto`;
//! 3. **a box that collapses through itself** — an empty box with no border,
//!    no padding and no height has its own top and bottom margins adjoining,
//!    so it collapses *into* the margins on either side of it.
//!
//! Case 1 is the one every implementation has. Cases 2 and 3 are where they
//! quietly differ, and each of the three is asserted on its own in the tests
//! rather than through one fixture that happens to exercise all three — a
//! fixture that exercises three rules and passes tells you nothing about which
//! of the three ran.
//!
//! All three fall out of **one** accumulator. [`Pending`] holds the margins
//! that are adjoining at the current position and is not committed to the flow
//! until something that is not a margin arrives — a border, a padding, a line
//! box. A parent that has none of those between itself and its first child
//! never commits, so their margins meet in the accumulator; a box that has
//! nothing at all inside it never commits either, so its own two margins meet
//! there. Writing them as three special cases is how an implementation ends up
//! with two of them.
//!
//! # A float is laid out here and placed somewhere else
//!
//! A floated box is taken out of the flow: it does not advance the cursor, its
//! content is laid out in a formatting context of its own at `x = 0`, and the
//! whole of it is then moved to wherever [`crate::floats`] says §9.5.1 puts it.
//! Three things follow, and each is a thing this module does that it would not
//! otherwise do:
//!
//! - the float's items are **not** in [`Flow::items`], because the page cutter
//!   walks that vector expecting a `y` that never goes backwards and a float's
//!   is above the lines that flow around it;
//! - a line box's measure is asked of the floats before it is filled, and a
//!   line with no room beside them is shifted below them — §9.5's second
//!   sentence, and the one an implementation leaves out;
//! - every run carries a document-order stamp, because the order the boxes were
//!   made in stopped being the order the words were written in.
//!
//! # The collapsed value is not a maximum
//!
//! §8.3.1: *"the maximum of the positive adjoining margins, plus the minimum of
//! the negative ones"*. A build that took `max()` over signed values gets every
//! ordinary book right and every negative margin wrong, and a negative margin
//! is what a book uses to pull a drop cap up.

use std::collections::HashMap;

use tinker_pdf_css::cascade::ComputedStyle;
use tinker_pdf_css::property::{
    BorderCollapse, BorderStyle, BoxSizing, Clear, Color, Display, Float, LengthPercentage,
    ListStyleType, MarginValue, OverflowWrap, PageBreak, PageBreakInside, Side, Sides, Size,
    TableLayout, TextAlign,
};

use crate::floats::{Ceilings, FloatContext, Placed};
use crate::metrics::Metrics;
use crate::style::{consume, Consumed};
use crate::table::{self, CellWidths, Edge, Grid, Origin, Slot, TableBox};
use crate::text::{self, Collapser};
use crate::uax14;
use crate::{BoxNode, Budget, Content, Limits, Options, Refusal, TextRun, Warning};

/// Slack for the comparisons a float's geometry needs, in points.
///
/// [`crate::fragment`]'s figure and [`crate::floats`]'s, for the same reason:
/// a word that fits the measure to within a thousandth of a point fits, and
/// the alternative is a rounding error sending a line under a figure it was
/// beside.
const EPSILON: f64 = 1e-6;

/// The measure a max-content trial is run at, in points.
///
/// Wide enough that no line in a book reaches it — a hundred thousand points is
/// thirty-five feet of Courier — and small enough that the arithmetic stays
/// exact in an `f64`, which `f64::MAX` would not: a width of `f64::MAX` less a
/// margin is still `f64::MAX`, and every shrink-to-fit answer would be
/// infinite.
const MAX_MEASURE: f64 = 100_000.0;

/// One block box's decorations and where they sit.
#[derive(Clone, Debug)]
pub(crate) struct BlockRecord {
    /// Border-box left edge.
    pub x: f64,
    /// Border-box width.
    pub width: f64,
    /// The first flow item inside this box's border box, if it has any.
    pub first: Option<usize>,
    /// One past the last.
    pub last: usize,
    /// `background-color`.
    pub background: Color,
    /// `border-*-width`.
    pub border_width: Sides<f64>,
    /// `border-*-style`.
    pub border_style: Sides<BorderStyle>,
    /// `border-*-color`.
    pub border_color: Sides<Color>,
    /// Whether anything about it would be painted at all.
    pub painted: bool,
}

/// CSS 2.2 §13.3.3's first kind of break position: the margin between two
/// block-level boxes.
#[derive(Clone, Debug, Default)]
pub(crate) struct MarginBreak {
    /// Rule A: at least one of the `page-break-before`/`page-break-after`
    /// values meeting here is `always`, `left` or `right`.
    pub forced: bool,
    /// Rule A: none of them is `avoid`, or one of them forces.
    pub allowed_by_a: bool,
    /// Rule B: no common ancestor has `page-break-inside: avoid`, or one of
    /// them forces.
    pub allowed_by_b: bool,
}

/// One line box, and everything rules C and D need to decide whether a page
/// may break before it.
#[derive(Clone, Debug)]
pub(crate) struct LineBox {
    /// Distance from the line box's top to the baseline.
    pub baseline: f64,
    /// The runs on it, `x` already absolute and `y` relative to the baseline.
    pub runs: Vec<TextRun>,
    /// Which line of its block container this is, counting from zero.
    pub index_in_block: usize,
    /// How many lines that block container has in total. Patched once the
    /// block is finished, because the answer does not exist until then.
    pub lines_in_block: usize,
    /// `orphans`, CSS 2.2 §13.3.2.
    pub orphans: u16,
    /// `widows`.
    pub widows: u16,
    /// Rule D: this line's block container, or one of its ancestors, has
    /// `page-break-inside: avoid`.
    pub avoid_inside: bool,
}

/// One band of table rows that cannot be separated, CSS 2.2 §17.
///
/// **A band and not a row**, and the difference is `rowspan`. A page may break
/// between two rows and may not break across a cell that spans them, so the
/// unit the fragmenter sees is the maximal run of grid rows joined by a
/// spanning cell. A table with no `rowspan` in it has one band per row, which
/// is where a real book's table breaks.
///
/// Its items are in **band-local** coordinates and are not in [`Flow::items`],
/// for [`FloatRecord`]'s reason one milestone earlier: the page cutter walks a
/// single column whose `y` never goes backwards, and the cells of a row sit
/// beside each other rather than under each other. Keeping them here is what
/// lets a two-column row be one item.
#[derive(Clone, Debug)]
pub(crate) struct RowBand {
    /// The cells' items, at the band's own origin.
    pub items: Vec<Item>,
    /// The row and cell decorations, indexing [`RowBand::items`].
    pub blocks: Vec<BlockRecord>,
}

/// What a flow item is.
#[derive(Clone, Debug)]
pub(crate) enum ItemKind {
    /// A collapsed margin. Breaking here is §13.3.3's case (1).
    Margin(MarginBreak),
    /// A border or a padding edge. Nothing may break inside one.
    Edge,
    /// A line box. Breaking before one is §13.3.3's case (2).
    Line(Box<LineBox>),
    /// One band of table rows, whole. §13.3.3 gives no break position inside
    /// one, which is why it is one item and not its cells' items spliced into
    /// the column.
    Rows(Box<RowBand>),
}

/// One piece of the continuous column.
#[derive(Clone, Debug)]
pub(crate) struct Item {
    /// Distance from the top of the flow.
    pub y: f64,
    /// How tall it is.
    pub height: f64,
    /// What it is.
    pub kind: ItemKind,
}

/// One float, laid out in its own formatting context and placed.
///
/// Its items are **not** in [`Flow::items`], and that is the whole reason this
/// type exists: [`crate::fragment`] cuts pages by walking a single column whose
/// `y` never goes backwards, and a float's content sits beside that column
/// rather than in it. Keeping the two apart is what lets a float be placed
/// above the line boxes that flow around it without the page cutter ever seeing
/// a `y` it cannot order.
#[derive(Clone, Debug)]
pub(crate) struct FloatRecord {
    /// The float's own flow, in the same coordinates as [`Flow::items`].
    pub items: Vec<Item>,
    /// Its own block records, indexing [`FloatRecord::items`].
    pub blocks: Vec<BlockRecord>,
    /// Margin-box top, which is where the float's first page is decided.
    pub top: f64,
    /// Margin-box bottom.
    pub bottom: f64,
}

/// A whole book as one continuous column, before it is cut into pages.
#[derive(Clone, Debug, Default)]
pub(crate) struct Flow {
    pub items: Vec<Item>,
    pub blocks: Vec<BlockRecord>,
    /// The floats, in the order they were met — which is document order, and
    /// therefore the order their text has to be read back in.
    pub floats: Vec<FloatRecord>,
    pub warnings: Vec<(Warning, usize)>,
}

/// The margins that are adjoining at the current position.
///
/// §8.3.1's whole algorithm, as one object. Nothing here is a special case for
/// a parent, a sibling or an empty box; the three cases are what happens when
/// this is not committed between them.
#[derive(Clone, Debug, Default)]
struct Pending {
    /// A margin position exists here at all, which is true between two block
    /// boxes even when both margins are zero — §13.3.3 breaks *in the vertical
    /// margin*, and a zero margin is still a margin.
    exists: bool,
    /// The largest positive margin adjoining here.
    positive: f64,
    /// The most negative one.
    negative: f64,
    /// Every `page-break-before`/`page-break-after` of a box meeting here.
    breaks: Vec<PageBreak>,
    /// The `page-break-inside: avoid` boxes that are ancestors of **every**
    /// element meeting here, as an intersection built one contribution at a
    /// time.
    ///
    /// Rule B says *"a **common** ancestor of all the elements"*, and a flag
    /// would answer a different question: an ordinary paragraph adjoining the
    /// first child of a `page-break-inside: avoid` figure has no common
    /// ancestor that avoids anything, and a build that ORed the two would
    /// refuse a break at the one margin that is the natural place for one.
    /// `None` means nothing has contributed yet, which is not the same as an
    /// empty intersection.
    avoid_common: Option<Vec<usize>>,
}

impl Pending {
    fn add(&mut self, margin: f64) {
        self.exists = true;
        if margin >= 0.0 {
            self.positive = self.positive.max(margin);
        } else {
            self.negative = self.negative.min(margin);
        }
    }

    /// Narrows the common-ancestor set by one contributing box.
    fn meet(&mut self, open_avoid: &[usize]) {
        self.avoid_common = Some(match self.avoid_common.take() {
            None => open_avoid.to_vec(),
            Some(previous) => previous
                .into_iter()
                .filter(|block| open_avoid.contains(block))
                .collect(),
        });
    }

    /// Rule B's question: is there a common ancestor that avoids breaking?
    fn avoided_inside(&self) -> bool {
        self.avoid_common
            .as_ref()
            .is_some_and(|blocks| !blocks.is_empty())
    }

    /// §8.3.1: the maximum of the positive margins plus the minimum of the
    /// negative ones. **Not** the maximum of the signed values.
    fn value(&self) -> f64 {
        self.positive + self.negative
    }
}

/// Builds the flow.
struct Builder<'a, M: Metrics> {
    metrics: &'a M,
    limits: &'a Limits,
    budget: &'a mut Budget,
    flow: Flow,
    warnings: HashMap<Warning, usize>,
    y: f64,
    pending: Pending,
    /// Blocks whose border box is open, innermost last.
    open: Vec<usize>,
    /// Of those, the ones with `page-break-inside: avoid`, which is rule B's
    /// candidate set at the moment a margin is contributed.
    open_avoid: Vec<usize>,
    /// The floats of the formatting context being built, CSS 2.2 §9.5.
    floats: FloatContext,
    /// §9.5.1's rule 5: the lowest border-box top any earlier box has had.
    ceiling_box: f64,
    /// Rule 6: the lowest top any earlier line box has had. Two fields rather
    /// than one running maximum, because they are two rules with two fixtures.
    ceiling_line: f64,
    /// Rule 4: the content top of the block container being filled.
    content_top: f64,
    /// What the table driver has decided about the very next box
    /// [`Builder::block`] lays out, CSS 2.2 §17.5.3 and §17.6.2.
    ///
    /// A cell's used width is its **column's**, whatever the cell's own
    /// `width` says — that is what a column is — and under a collapsing border
    /// model its used borders are the resolved ones, which are not on any
    /// element at all. Neither can be expressed as a computed style, so neither
    /// can arrive through [`consume`].
    ///
    /// It is `take`n rather than read, so it applies to **exactly one box**. A
    /// copy would reach the cell's children, and a cell holding a nested table
    /// would give that table the outer cell's column width and the outer cell's
    /// collapsed borders — a table inside a table that is silently the wrong
    /// size, which is this plan's own definition of the failure worth
    /// preventing.
    cell: Option<CellPass>,
    /// Document order, stamped on every run as it is made.
    ///
    /// **Reading order stops being emission order the moment a float exists.**
    /// A float's content is laid out when the float is met and drawn where the
    /// float was placed, which can be a page later than the text that follows
    /// it in the source; without a stamp, the only order available to a reader
    /// of the output is the order the boxes happened to be produced in, and
    /// text conservation — an *ordered* comparison — would fail on a book that
    /// lost nothing at all.
    sequence: usize,
}

/// What laying a subtree out in its own formatting context came to.
///
/// A struct rather than the four-tuple it was, because a float's own flow, its
/// own decorations, the floats **inside** it and its height are four different
/// things and a caller that took the third for the second would compile.
struct Sublayout {
    /// The subtree's own flow, at `x = 0` and `y = 0`.
    items: Vec<Item>,
    /// Its own block records, indexing those items.
    blocks: Vec<BlockRecord>,
    /// Any floats it placed in its own formatting context.
    floats: Vec<FloatRecord>,
    /// How tall the whole of it came to, margins included.
    height: f64,
}

/// What the table driver imposes on one cell box. See [`Builder::cell`].
#[derive(Clone, Debug)]
struct CellPass {
    /// The border-box width the cell must take.
    ///
    /// `None` means *ignore the cell's own `width` and take the measure* —
    /// which is what §17.5.2.2's first pass needs, because a cell with `width:
    /// 4em` has a maximum content width decided by its text and not by its
    /// declaration. A build that measured the first pass with the declaration
    /// in place would find every such cell's maximum equal to its minimum and
    /// its column would never grow.
    width: Option<f64>,
    /// The collapsed borders, §17.6.2, already halved. `None` in the separated
    /// model, where a cell's own borders are its own.
    borders: Option<Collapsed>,
}

/// One box's four resolved borders, §17.6.2.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Collapsed {
    width: Sides<f64>,
    style: Sides<BorderStyle>,
    color: Sides<Color>,
}

impl CellPass {
    /// Applies the decision to a consumed style, CSS 2.2 §17.5.3: a cell's
    /// margins do not apply, and its width is its column's.
    fn apply(&self, style: &mut Consumed) {
        style.margin = Sides::all(MarginValue::Length(LengthPercentage::ZERO));
        match self.width {
            Some(width) => {
                style.width = Size::Length(LengthPercentage::Px(width));
                style.box_sizing = BoxSizing::BorderBox;
            }
            None => style.width = Size::Auto,
        }
        if let Some(borders) = self.borders {
            style.border_width = borders.width;
            style.border_style = borders.style;
            style.border_color = borders.color;
        }
    }
}

/// One run of text in an inline formatting context, after phase I.
struct Piece {
    text: String,
    style: Consumed,
    /// The [`BoxNode::anchor`] of the node this piece's text came from.
    anchor: Option<u32>,
    /// Its position in document order. See [`Builder::sequence`].
    order: usize,
}

/// Lays a tree out into one continuous column.
pub(crate) fn build<M: Metrics>(
    root: &BoxNode,
    metrics: &M,
    options: &Options,
    limits: &Limits,
    budget: &mut Budget,
) -> Result<Flow, Refusal> {
    let mut builder = Builder {
        metrics,
        limits,
        budget,
        flow: Flow::default(),
        warnings: HashMap::new(),
        y: 0.0,
        pending: Pending::default(),
        open: Vec::new(),
        open_avoid: Vec::new(),
        floats: FloatContext::default(),
        // Nothing is earlier than the first box, and a ceiling of zero would
        // be a claim about the top of the page rather than the absence of one:
        // a book whose first block has a negative top margin starts above the
        // page and a float in it belongs there too.
        ceiling_box: f64::NEG_INFINITY,
        ceiling_line: f64::NEG_INFINITY,
        content_top: 0.0,
        cell: None,
        sequence: 0,
    };
    builder.block(root, options.width, 0.0, 0, false, 0)?;
    // The last pending margin is committed so the flow's height includes it,
    // which matters for a book whose last block has a bottom margin: without
    // it the final page is short by that margin and the page count can differ.
    builder.commit_margin();
    let mut warnings: Vec<(Warning, usize)> = builder.warnings.into_iter().collect();
    // Deterministic order, ruling 4: a `HashMap`'s iteration order is not, and
    // a warning list that changed between runs would change a report.
    warnings.sort_by(|a, b| format!("{:?}", a.0).cmp(&format!("{:?}", b.0)));
    let mut flow = builder.flow;
    flow.warnings = warnings;
    Ok(flow)
}

impl<M: Metrics> Builder<'_, M> {
    fn warn(&mut self, warning: Warning) {
        *self.warnings.entry(warning).or_insert(0) += 1;
    }

    /// Pushes an item, advances the flow, and records it against every open
    /// block.
    fn emit(&mut self, height: f64, kind: ItemKind, inside_open: bool) -> usize {
        let index = self.flow.items.len();
        self.flow.items.push(Item {
            y: self.y,
            height,
            kind,
        });
        self.y += height;
        if inside_open {
            for block in &self.open {
                let record = &mut self.flow.blocks[*block];
                if record.first.is_none() {
                    record.first = Some(index);
                }
                record.last = index + 1;
            }
        } else {
            // A margin that has not entered any of the open boxes yet — a
            // block's own top margin collapsing with its parent's — belongs to
            // neither border box, which is what a margin is.
            for block in &self.open {
                let record = &mut self.flow.blocks[*block];
                if record.first.is_some() {
                    record.last = index + 1;
                }
            }
        }
        index
    }

    /// Commits whatever margins are adjoining, if any.
    fn commit_margin(&mut self) {
        if !self.pending.exists {
            return;
        }
        let pending = std::mem::take(&mut self.pending);
        let forced = pending
            .breaks
            .iter()
            .any(|b| matches!(b, PageBreak::Always | PageBreak::Left | PageBreak::Right));
        let avoided = pending.breaks.contains(&PageBreak::Avoid);
        let avoid_inside = pending.avoided_inside();
        // Rule A: allowed when at least one value forces, or when all of them
        // are `auto`. Written as the specification writes it rather than as
        // "no avoid", because the two differ exactly when a forced break and an
        // avoid meet — which is the case a book with `page-break-before: always`
        // on a chapter inside a `page-break-after: avoid` heading produces.
        let allowed_by_a = forced || !avoided;
        // Rule B bites only where every value is `auto`.
        let allowed_by_b = forced || avoided || !avoid_inside;
        let height = pending.value();
        let inside = self
            .open
            .last()
            .is_some_and(|block| self.flow.blocks[*block].first.is_some());
        self.emit(
            height,
            ItemKind::Margin(MarginBreak {
                forced,
                allowed_by_a,
                allowed_by_b,
            }),
            inside,
        );
    }

    /// One block-level box.
    #[allow(clippy::too_many_arguments)]
    fn block(
        &mut self,
        node: &BoxNode,
        containing: f64,
        x: f64,
        depth: usize,
        avoid: bool,
        ordinal: usize,
    ) -> Result<(), Refusal> {
        if depth > self.limits.max_depth {
            return Err(Refusal::TooDeep { depth });
        }
        let mut style = consume(&node.style);
        if style.is_none() {
            return Ok(());
        }
        // Exactly one box, and the one the table driver just decided about.
        // See [`Builder::cell`].
        if let Some(pass) = self.cell.take() {
            pass.apply(&mut style);
        }
        let style = style;
        self.budget.spend_box()?;
        let avoid = avoid || style.page_break_inside == PageBreakInside::Avoid;

        let margin_top = style.margin_px(Side::Top, containing);
        let margin_bottom = style.margin_px(Side::Bottom, containing);
        let margin_left = style.margin_px(Side::Left, containing);
        let margin_right = style.margin_px(Side::Right, containing);
        let padding = Sides {
            top: style.padding_px(Side::Top, containing),
            right: style.padding_px(Side::Right, containing),
            bottom: style.padding_px(Side::Bottom, containing),
            left: style.padding_px(Side::Left, containing),
        };
        let border = style.border_width;

        // `css-box-3` §4: `content-box` measures `width` as the content, and
        // `border-box` measures it as content plus padding plus border. The
        // difference is invisible on a box with neither, which is why a fixture
        // for it must have both.
        let extra = padding.left + padding.right + border.left + border.right;
        let (content_width, mut left) = match style.width {
            Size::Auto => {
                let available = containing - margin_left - margin_right - extra;
                (available.max(0.0), x + margin_left)
            }
            Size::Length(length) => {
                let specified = match length {
                    LengthPercentage::Px(px) => px,
                    LengthPercentage::Percent(percent) => containing * percent / 100.0,
                };
                let content = match style.box_sizing {
                    tinker_pdf_css::property::BoxSizing::ContentBox => specified,
                    tinker_pdf_css::property::BoxSizing::BorderBox => specified - extra,
                }
                .max(0.0);
                // §10.3.3: with a specified width, two `auto` margins centre
                // the box and the leftover is otherwise put on the right.
                let outer = content + extra;
                let both_auto = style.margin.left == MarginValue::Auto
                    && style.margin.right == MarginValue::Auto;
                let left = if both_auto {
                    x + ((containing - outer) / 2.0).max(0.0)
                } else {
                    x + margin_left
                };
                (content, left)
            }
        };
        if content_width + extra > containing + 0.001 {
            self.warn(Warning::ContentOverflowedPage);
        }
        left = left.max(0.0);
        let border_box_width = content_width + extra;

        let painted = style.background_color.a != 0
            || border.top > 0.0
            || border.right > 0.0
            || border.bottom > 0.0
            || border.left > 0.0;
        let record = BlockRecord {
            x: left,
            width: border_box_width,
            first: None,
            last: 0,
            background: style.background_color,
            border_width: border,
            border_style: style.border_style,
            border_color: style.border_color,
            painted: painted && style.visible,
        };
        let block = self.flow.blocks.len();
        self.flow.blocks.push(record);

        // §9.5.2's clearance, which goes **between** the margins already
        // adjoining here and this box's own top margin — so it is introduced
        // before the top margin joins them, and introducing it is what stops
        // the two from collapsing through each other.
        self.clear(&style, margin_top)?;

        // The top margin joins whatever is adjoining, and the box's
        // `page-break-before` joins the break position that margin is. The
        // avoid set is taken **before** this box is opened, because an element
        // is not its own ancestor.
        self.pending.breaks.push(style.page_break_before);
        self.pending.meet(&self.open_avoid.clone());
        self.pending.add(margin_top);
        // §9.5.1's rule 5 counts this box from here on. The border-box top is
        // where the margins standing at this position have taken it, which is
        // not `self.y` — they have not been committed yet and will not be
        // until something that is not a margin arrives.
        self.ceiling_box = self.ceiling_box.max(self.y + self.pending.value());

        self.open.push(block);
        if style.page_break_inside == PageBreakInside::Avoid {
            self.open_avoid.push(block);
        }
        let top_edge = border.top + padding.top;
        if top_edge > 0.0 {
            // A border or a padding between the parent and its first child is
            // exactly what stops case 2 from happening, so the margin is
            // committed here and the two do not meet.
            self.commit_margin();
            self.emit(top_edge, ItemKind::Edge, true);
        }

        let content_x = left + border.left + padding.left;
        let before = self.y;
        // Rule 4's containing block for any float among the children. It is
        // `before` plus the margins standing here rather than `before` itself,
        // for the reason above: an uncommitted margin has not moved `self.y`
        // yet and it will.
        let outer_top = std::mem::replace(&mut self.content_top, before + self.pending.value());
        if style.is_table() {
            // CSS 2.2 §17. Everything above this line -- the margins, the
            // border, the padding, `width`, `box-sizing`, the page-break
            // properties -- is the ordinary block box a table also is, and
            // reusing it is what stops a table from being a second, quietly
            // different, box model.
            self.table(node, &style, content_x, content_width, depth, avoid, block)?;
        } else {
            self.children(node, &style, content_x, content_width, depth, avoid, block)?;
        }
        self.content_top = outer_top;
        let content_height = self.y - before;

        // A specified height is honoured by padding the flow out to it; a
        // content taller than the height overflows, which CSS 2.2 §10.6.3's
        // `overflow: visible` initial value asks for.
        if let Size::Length(length) = style.height {
            let wanted = match length {
                LengthPercentage::Px(px) => px,
                // §10.5: a percentage height against an `auto` containing
                // block behaves as `auto`, which is why this is not resolved
                // against the page.
                LengthPercentage::Percent(_) => content_height,
            };
            if wanted > content_height {
                self.commit_margin();
                self.emit(wanted - content_height, ItemKind::Edge, true);
            }
        }

        let bottom_edge = border.bottom + padding.bottom;
        if bottom_edge > 0.0 {
            self.commit_margin();
            self.emit(bottom_edge, ItemKind::Edge, true);
        }
        self.open.pop();
        if style.page_break_inside == PageBreakInside::Avoid {
            self.open_avoid.pop();
        }

        // The bottom margin joins the next adjoining position. When the box had
        // no border, no padding, no content and no height, its top margin is
        // still sitting in the same accumulator — which is case 3, collapsing
        // through, with no code of its own.
        self.pending.breaks.push(style.page_break_after);
        self.pending.meet(&self.open_avoid.clone());
        self.pending.add(margin_bottom);

        // The marker of a `list-item` is generated content and goes on the
        // box's first line, which is why it is placed after the children.
        if style.display == Display::ListItem {
            self.marker(&style, block, content_x, ordinal);
        }
        Ok(())
    }

    /// A block container's children: block-level ones recursed into, runs of
    /// inline-level ones wrapped in an anonymous block box.
    #[allow(clippy::too_many_arguments)]
    fn children(
        &mut self,
        node: &BoxNode,
        style: &Consumed,
        content_x: f64,
        content_width: f64,
        depth: usize,
        avoid: bool,
        block: usize,
    ) -> Result<(), Refusal> {
        match &node.content {
            Content::Text(source) => {
                let mut pieces = Vec::new();
                let mut collapser = Collapser::new();
                let text = collapser.push(source, style.white_space);
                pieces.push(Piece {
                    text,
                    style: style.clone(),
                    anchor: node.anchor,
                    order: self.order(),
                });
                self.lines(&pieces, style, block, content_x, content_width)
            }
            Content::Children(children) => {
                let styles: Vec<Consumed> = children.iter().map(|c| consume(&c.style)).collect();
                let any_block = styles.iter().any(|s| !s.is_none() && s.is_block_level());
                if !any_block {
                    // CSS 2.2 §9.4.2: an inline formatting context.
                    let mut pieces = Vec::new();
                    let mut collapser = Collapser::new();
                    for child in children {
                        self.gather(
                            child,
                            &mut pieces,
                            &mut collapser,
                            depth + 1,
                            content_x,
                            content_width,
                        )?;
                    }
                    return self.lines(&pieces, style, block, content_x, content_width);
                }
                // §9.2.1.1: block-level and inline-level siblings, so the runs
                // of inline content are wrapped in anonymous block boxes. The
                // anonymous box inherits nothing of its own — it is not an
                // element — so its line boxes take the container's own
                // `text-align`, `text-indent` and strut.
                let mut run: Vec<&BoxNode> = Vec::new();
                // Counted as the children are walked rather than recomputed per
                // item: `ordinal` used to scan the whole child list for each
                // list item, which is `O(children^2)` for one long list.
                let mut ordinal = 0usize;
                // §17.2.1 rule 9's run, once it has been wrapped.
                let mut wrapped_until = 0usize;
                for (index, (child, child_style)) in children.iter().zip(&styles).enumerate() {
                    if index < wrapped_until {
                        continue;
                    }
                    if child_style.is_none() {
                        continue;
                    }
                    if child_style.is_internal_table() {
                        // §17.2.1 rule 9, **the ninth generation step**: an
                        // internal table box whose parent is not a table is
                        // wrapped, with its consecutive internal siblings, in
                        // an anonymous table. Without it a stray `<tr>` is
                        // neither block-level nor inline-level and generates
                        // nothing at all -- which loses a row of a book rather
                        // than misplacing it.
                        if !run.is_empty() {
                            self.anonymous(&run, style, block, content_x, content_width, depth)?;
                            run.clear();
                        }
                        let end = table::misparented_run(children, index);
                        self.budget.spend_box()?;
                        let wrapper = anonymous_table(&node.style, &children[index..end]);
                        self.block(&wrapper, content_width, content_x, depth + 1, avoid, 0)?;
                        wrapped_until = end;
                        continue;
                    }
                    if child_style.float != Float::None {
                        // The inline content standing before a float is set
                        // first, and that is not §9.2.1.1's doing — a float is
                        // not block-level and does not on its own make an
                        // anonymous box. It is document order's: the float's
                        // text is stamped where it is laid out, and laying it
                        // out before the words that precede it in the source
                        // would put those words after it in every reading of
                        // the output.
                        if !run.is_empty() {
                            self.anonymous(&run, style, block, content_x, content_width, depth)?;
                            run.clear();
                        }
                        self.float_box(
                            child,
                            child_style,
                            content_width,
                            content_x,
                            depth + 1,
                            avoid,
                        )?;
                        continue;
                    }
                    if child_style.is_block_level() {
                        if !run.is_empty() {
                            self.anonymous(&run, style, block, content_x, content_width, depth)?;
                            run.clear();
                        }
                        let here = ordinal;
                        if child_style.display == Display::ListItem {
                            ordinal += 1;
                        }
                        self.block(child, content_width, content_x, depth + 1, avoid, here)?;
                    } else {
                        run.push(child);
                    }
                }
                if !run.is_empty() {
                    self.anonymous(&run, style, block, content_x, content_width, depth)?;
                }
                Ok(())
            }
        }
    }

    /// One anonymous block box holding a run of inline-level siblings.
    #[allow(clippy::too_many_arguments)]
    fn anonymous(
        &mut self,
        run: &[&BoxNode],
        style: &Consumed,
        block: usize,
        content_x: f64,
        content_width: f64,
        depth: usize,
    ) -> Result<(), Refusal> {
        self.budget.spend_box()?;
        let mut pieces = Vec::new();
        let mut collapser = Collapser::new();
        for child in run {
            self.gather(
                child,
                &mut pieces,
                &mut collapser,
                depth + 1,
                content_x,
                content_width,
            )?;
        }
        self.lines(&pieces, style, block, content_x, content_width)
    }

    /// Collects one inline-level subtree's text, phase I applied **across**
    /// the whole context.
    #[allow(clippy::too_many_arguments)]
    fn gather(
        &mut self,
        node: &BoxNode,
        out: &mut Vec<Piece>,
        collapser: &mut Collapser,
        depth: usize,
        content_x: f64,
        content_width: f64,
    ) -> Result<(), Refusal> {
        if depth > self.limits.max_depth {
            return Err(Refusal::TooDeep { depth });
        }
        let style = consume(&node.style);
        if style.is_none() {
            return Ok(());
        }
        // §9.7: a float is block-level whatever `display` said, so a floated
        // element inside a paragraph is taken out here rather than made into a
        // piece — and the paragraph is **not** split around it, which is the
        // difference between this path and the block one. What it costs is
        // that the float's static position is the top of the inline formatting
        // context rather than the line it was written on: the lines do not
        // exist yet when this runs. See the crate's `Still owed`.
        if style.float != Float::None {
            return self.float_box(node, &style, content_width, content_x, depth, false);
        }
        self.budget.spend_box()?;
        if style.display == Display::InlineBlock {
            // Here rather than beside the block builder, which is where it was
            // until milestone 10 and where it could not fire: an
            // `inline-block` is not block-level, so it arrives in an inline
            // formatting context and is set as text.
            self.warn(Warning::InlineBlockAsInline);
        }
        match &node.content {
            Content::Text(source) => {
                let text = collapser.push(source, style.white_space);
                if !text.is_empty() {
                    out.push(Piece {
                        text,
                        style,
                        anchor: node.anchor,
                        order: self.order(),
                    });
                }
            }
            Content::Children(children) => {
                for child in children {
                    let child_style = consume(&child.style);
                    // A float is not the §9.2.1.1 case: it is taken out of the
                    // inline flow rather than splitting the inline box that
                    // holds it, so warning about it would name the wrong rule.
                    if child_style.float == Float::None && child_style.is_block_level() {
                        self.warn(Warning::BlockInInline);
                    }
                    self.gather(child, out, collapser, depth + 1, content_x, content_width)?;
                }
            }
        }
        Ok(())
    }

    /// The next document-order stamp. See [`Builder::sequence`].
    fn order(&mut self) -> usize {
        self.sequence += 1;
        self.sequence
    }

    /// Where content would start if it arrived now: `self.y` plus whatever
    /// margins are standing at this position and have not been committed.
    fn cursor(&self) -> f64 {
        self.y + self.pending.value()
    }

    /// Whether the innermost open box has already begun, which decides whether
    /// an item emitted here is inside its border box.
    fn inside_open(&self) -> bool {
        self.open
            .last()
            .is_some_and(|block| self.flow.blocks[*block].first.is_some())
    }

    /// CSS 2.2 §9.5.2: clearance, introduced above a box's own top margin.
    ///
    /// **Clearance is not a margin and not a border.** It is a third thing,
    /// and §8.3.1 gives it the property that makes it worth being a third
    /// thing: a box with clearance does not collapse its top margin with the
    /// margins above it, so the cleared box moves down and stays down. Adding
    /// the distance to the margin instead would let the next box's margin
    /// collapse it away again.
    fn clear(&mut self, style: &Consumed, margin_top: f64) -> Result<(), Refusal> {
        if style.clear == Clear::None {
            return Ok(());
        }
        self.budget.spend_layout(self.floats.len())?;
        let Some(bottom) = self.floats.clearance_bottom(style.clear) else {
            // Nothing on those sides is floated, so there is nothing to clear
            // and nothing to say: `clear` on a book's every `<hr>` is not a
            // fidelity gap and a warning about it would drown the ones that
            // are.
            return Ok(());
        };
        let clearance = bottom - (self.cursor() + margin_top);
        if clearance <= 0.0 {
            return Ok(());
        }
        let inside = self.inside_open();
        self.commit_margin();
        self.emit(clearance, ItemKind::Edge, inside);
        Ok(())
    }

    /// One floated box: §9.5.1's placement, and its content in a formatting
    /// context of its own.
    ///
    /// The float's own flow is built first and placed second, because §9.5.1's
    /// rules 2, 3 and 7 all need the height of the box being placed and a
    /// float's height is whatever its content came to.
    fn float_box(
        &mut self,
        node: &BoxNode,
        style: &Consumed,
        containing: f64,
        cb_left: f64,
        depth: usize,
        avoid: bool,
    ) -> Result<(), Refusal> {
        if depth > self.limits.max_depth {
            return Err(Refusal::TooDeep { depth });
        }
        let outer_width = self.float_width(node, style, containing, depth, avoid)?;
        let Sublayout {
            mut items,
            mut blocks,
            floats: mut nested,
            height,
        } = self.sublayout(node, outer_width, depth, avoid)?;

        // A float with `clear` clears before it is placed: §9.5.2's *"the top
        // margin edge is moved below"* is about the box, and a float is a box.
        let mut hint = self.cursor();
        if style.clear != Clear::None {
            self.budget.spend_layout(self.floats.len())?;
            if let Some(bottom) = self.floats.clearance_bottom(style.clear) {
                hint = hint.max(bottom);
            }
        }
        let ceilings = Ceilings {
            containing_top: self.content_top,
            earlier_box_top: self.ceiling_box,
            earlier_line_top: self.ceiling_line,
        };
        let (left, top) = self.floats.place(
            style.float,
            outer_width,
            height,
            hint,
            &ceilings,
            cb_left,
            cb_left + containing,
            self.budget,
        )?;

        translate(&mut items, &mut blocks, left, top);
        for record in &mut nested {
            translate(&mut record.items, &mut record.blocks, left, top);
            record.top += top;
            record.bottom += top;
        }
        self.floats.push(Placed {
            side: style.float,
            left,
            right: left + outer_width,
            top,
            bottom: top + height,
        });
        // Rule 5 counts a float among the boxes an element earlier in the
        // document generated, and rule 6 does not count it among the line
        // boxes: a float holds line boxes but is not on one.
        self.ceiling_box = self.ceiling_box.max(top);
        self.flow.floats.push(FloatRecord {
            items,
            blocks,
            top,
            bottom: top + height,
        });
        self.flow.floats.append(&mut nested);
        Ok(())
    }

    /// §10.3.5: a float's used width, shrink-to-fit where `width` is `auto`.
    ///
    /// *"min(max(preferred minimum width, available width), preferred width)"*,
    /// and the two preferred widths are measured by laying the float's content
    /// out twice — once at a measure nothing can reach, which puts every
    /// paragraph on one line, and once at a measure nothing fits in, which puts
    /// every unbreakable word on one. Measuring them from the same line breaker
    /// that will set the float is what stops the shrink-to-fit width and the
    /// actual set text disagreeing about where a word ends.
    ///
    /// It costs two extra layouts of the float's subtree, charged to the same
    /// budget as everything else. A float with a stated `width` pays neither.
    fn float_width(
        &mut self,
        node: &BoxNode,
        style: &Consumed,
        containing: f64,
        depth: usize,
        avoid: bool,
    ) -> Result<f64, Refusal> {
        let margins =
            style.margin_px(Side::Left, containing) + style.margin_px(Side::Right, containing);
        let extra = style.padding_px(Side::Left, containing)
            + style.padding_px(Side::Right, containing)
            + style.border_width.left
            + style.border_width.right;
        if let Size::Length(length) = style.width {
            let specified = match length {
                LengthPercentage::Px(px) => px,
                LengthPercentage::Percent(percent) => containing * percent / 100.0,
            };
            let content = match style.box_sizing {
                tinker_pdf_css::property::BoxSizing::ContentBox => specified,
                tinker_pdf_css::property::BoxSizing::BorderBox => specified - extra,
            }
            .max(0.0);
            return Ok(content + extra + margins);
        }
        // A trial measures how far right its text reached, which counts the
        // insets on the **left** of it and none of the ones on the right.
        let right = style.padding_px(Side::Right, containing)
            + style.border_width.right
            + style.margin_px(Side::Right, containing);
        let preferred = self.measure_content(node, MAX_MEASURE, depth, avoid)? + right;
        let minimum = self.measure_content(node, 0.0, depth, avoid)? + right;
        Ok(containing.max(minimum).min(preferred).max(minimum))
    }

    /// The outer width one trial layout came to: the rightmost edge any text
    /// in it reached.
    ///
    /// Text and not boxes. A block inside the float with an `auto` width fills
    /// whatever measure the trial was run at, so counting block records would
    /// make the preferred width of every float the width of the trial — which
    /// is the shrink-to-fit bug that produces a page-wide float holding one
    /// word. What it costs is a float whose only wide thing is a block with a
    /// stated `width`; see the crate's `Still owed`.
    fn measure_content(
        &mut self,
        node: &BoxNode,
        measure: f64,
        depth: usize,
        avoid: bool,
    ) -> Result<f64, Refusal> {
        // A trial's warnings are not the book's. Setting a paragraph at a
        // measure of zero reports a line that overflowed on every word, and
        // reporting that to the caller would be reporting an answer this
        // function threw away.
        let warnings = std::mem::take(&mut self.warnings);
        let trial = self.sublayout(node, measure, depth, avoid);
        self.warnings = warnings;
        let trial = trial?;
        let (items, floats) = (trial.items, trial.floats);
        let mut width = 0.0f64;
        // A band's text is inside it, so a trial that did not look would
        // measure a cell holding a nested table as empty and give its column no
        // width at all.
        fn extend(items: &[Item], width: &mut f64) {
            for item in items {
                match &item.kind {
                    ItemKind::Line(line) => {
                        for run in &line.runs {
                            *width = width.max(run.x + run.width);
                        }
                    }
                    ItemKind::Rows(band) => extend(&band.items, width),
                    ItemKind::Margin(_) | ItemKind::Edge => {}
                }
            }
        }
        let mut extents = |items: &[Item]| extend(items, &mut width);
        extents(&items);
        for float in &floats {
            extents(&float.items);
        }
        Ok(width)
    }

    /// Lays a subtree out in a formatting context of its own, at `x = 0`.
    ///
    /// Everything positional is swapped out and put back: the items, the block
    /// records, the cursor, the adjoining margins, the open boxes and — the one
    /// that would be a silent fault rather than a crash — **the float context**,
    /// because a float establishes a new block formatting context and floats
    /// outside it do not reach inside it.
    fn sublayout(
        &mut self,
        node: &BoxNode,
        measure: f64,
        depth: usize,
        avoid: bool,
    ) -> Result<Sublayout, Refusal> {
        let items = std::mem::take(&mut self.flow.items);
        let blocks = std::mem::take(&mut self.flow.blocks);
        let floats = std::mem::take(&mut self.flow.floats);
        let context = std::mem::take(&mut self.floats);
        let y = std::mem::replace(&mut self.y, 0.0);
        let pending = std::mem::take(&mut self.pending);
        let open = std::mem::take(&mut self.open);
        let open_avoid = std::mem::take(&mut self.open_avoid);
        let ceiling_box = std::mem::replace(&mut self.ceiling_box, f64::NEG_INFINITY);
        let ceiling_line = std::mem::replace(&mut self.ceiling_line, f64::NEG_INFINITY);
        let content_top = std::mem::replace(&mut self.content_top, 0.0);

        let result = self.block(node, measure, 0.0, depth, avoid, 0);
        if result.is_ok() {
            // The same reason `build` does it: without this the float's bottom
            // margin is not part of its height, and a float whose height is
            // short by a margin lets the line beside it start too high.
            self.commit_margin();
        }

        let inner = std::mem::replace(&mut self.flow.items, items);
        let inner_blocks = std::mem::replace(&mut self.flow.blocks, blocks);
        let inner_floats = std::mem::replace(&mut self.flow.floats, floats);
        let height = std::mem::replace(&mut self.y, y);
        self.floats = context;
        self.pending = pending;
        self.open = open;
        self.open_avoid = open_avoid;
        self.ceiling_box = ceiling_box;
        self.ceiling_line = ceiling_line;
        self.content_top = content_top;
        result?;
        Ok(Sublayout {
            items: inner,
            blocks: inner_blocks,
            floats: inner_floats,
            height,
        })
    }

    /// A `display: table` box's content, CSS 2.2 §17.
    ///
    /// The order below is the specification's and every step of it is
    /// separable:
    ///
    /// 1. §17.2.1 generates the boxes the document left out — [`table::generate`];
    /// 2. §17.4's captions are laid out above the table;
    /// 3. §17.5 places every cell in the grid, `colspan` and `rowspan` included;
    /// 4. §17.6.2 resolves the collapsing borders, which must happen **before**
    ///    any measuring because a collapsed border changes how much room a cell
    ///    has for its text;
    /// 5. §17.5.2.2's **first pass** measures a minimum and a maximum content
    ///    width per cell, or §17.5.2.1 skips it because a fixed layout does not
    ///    depend on the contents;
    /// 6. §17.5.2.2's **second pass** distributes the table's width over the
    ///    columns;
    /// 7. every cell is laid out at its column's width, **in document order**,
    ///    so the reading-order stamps ascend with the source;
    /// 8. the rows are emitted in **visual order** — header, bodies, footer —
    ///    which is not document order once a book writes `<tfoot>` first.
    ///
    /// Steps 7 and 8 being different orders is milestone 10's finding in two
    /// dimensions, and [`crate::TextRun::order`] is what makes it survivable.
    #[allow(clippy::too_many_arguments)]
    fn table(
        &mut self,
        node: &BoxNode,
        style: &Consumed,
        content_x: f64,
        content_width: f64,
        depth: usize,
        avoid: bool,
        block: usize,
    ) -> Result<(), Refusal> {
        let tree = table::generate(node);
        for step in table::Step::ALL {
            for _ in 0..tree.generated.count(step) {
                self.budget.spend_box()?;
            }
        }
        // §17.4. HTML requires `<caption>` to be a table's first element child,
        // so for every conforming book this order is also document order --
        // which is what keeps the stamps ascending. `caption-side` is
        // `Unsupported` by name, so a caption asked for at the bottom is a
        // reported gap rather than a caption drawn in the wrong place.
        for caption in &tree.captions {
            self.block(caption, content_width, content_x, depth + 1, avoid, 0)?;
        }
        // **The first of the three places the layout total is charged.** A
        // `colspan` is a number in the file and the slots it occupies are the
        // work, so the charge is made before the slots are marked rather than
        // as they are marked -- `MAX_LINE_BREAK_WORK`'s posture, and the reason
        // a `colspan="4000000000"` costs a refusal and not a gigabyte.
        let grid = {
            let budget = &mut *self.budget;
            Grid::place(&tree, |slots| budget.spend_layout(slots))?
        };
        if grid.clamped > 0 {
            self.warn(Warning::RowspanPastTheRowGroup);
        }
        if grid.columns == 0 || grid.slots.is_empty() {
            return Ok(());
        }
        // **The second.** The grid is rows by columns and neither factor bounds
        // the other: five cells of `colspan="1000"` are five boxes and five
        // thousand slots.
        self.budget
            .spend_layout(grid.rows.saturating_mul(grid.columns))?;
        let rows_of = tree.visual_rows();
        let mut occupancy = vec![vec![None; grid.columns]; grid.rows];
        for (at, slot) in grid.slots.iter().enumerate() {
            for row in occupancy.iter_mut().skip(slot.top).take(slot.rows) {
                for column in row.iter_mut().skip(slot.left).take(slot.columns) {
                    *column = Some(at);
                }
            }
        }

        // The column boxes: §17.5.2's widths, and §17.5.1's two rendering
        // layers this build does not paint.
        let mut declared: Vec<Option<f64>> = vec![None; grid.columns];
        let collapsing = style.border_collapse == BorderCollapse::Collapse;
        for (at, width) in declared.iter_mut().enumerate() {
            let Some(column) = tree.columns.get(at) else {
                break;
            };
            let Some(described) = column.node.or(column.group) else {
                continue;
            };
            let consumed = consume(&described.style);
            let bordered = !collapsing
                && (consumed.border_width.top > 0.0
                    || consumed.border_width.right > 0.0
                    || consumed.border_width.bottom > 0.0
                    || consumed.border_width.left > 0.0);
            if consumed.background_color.a != 0 || bordered {
                self.warn(Warning::ColumnBoxNotPainted);
            }
            if let Size::Length(length) = consumed.width {
                *width = Some(resolve_length(length, content_width).max(0.0));
            }
        }

        // §17.6.2, before anything is measured.
        let borders: Vec<Option<Collapsed>> = if collapsing {
            collapsed_borders(node, &tree, &grid, &occupancy, &rows_of)
                .into_iter()
                .map(Some)
                .collect()
        } else {
            vec![None; grid.slots.len()]
        };

        // Document order over the slots, which is *not* the order they were
        // placed in: `Grid::place` walks the row groups in visual order.
        let mut document: Vec<usize> = (0..grid.slots.len()).collect();
        document.sort_by_key(|at| {
            let slot = &grid.slots[*at];
            (slot.group, slot.row, slot.cell)
        });

        let hspacing = style.border_spacing.horizontal;
        let vspacing = style.border_spacing.vertical;
        let spacing_total = hspacing * (grid.columns as f64 + 1.0);
        let available = (content_width - spacing_total).max(0.0);
        // §17.5.2.1's own first sentence: *"a value of `auto` means use the
        // automatic table layout algorithm"*. A build that ran the fixed
        // algorithm whenever `table-layout: fixed` was declared divides the
        // containing block evenly among the columns and draws a table nobody
        // asked for.
        let fixed_layout =
            style.table_layout == TableLayout::Fixed && !matches!(style.width, Size::Auto);

        let mut specified: Vec<Option<f64>> = vec![None; grid.slots.len()];
        let mut cells: Vec<CellWidths> = Vec::with_capacity(grid.slots.len());
        for &at in &document {
            let slot = grid.slots[at];
            let cell = &tree.groups[slot.group].rows[slot.row].cells[slot.cell];
            let inner = cell.content.node();
            let consumed = consume(&inner.style);
            specified[at] = match consumed.width {
                Size::Auto => None,
                Size::Length(length) => Some(resolve_length(length, content_width).max(0.0)),
            };
            let (min, max) = if fixed_layout {
                (0.0, 0.0)
            } else {
                // §17.5.2.2's **first pass**. The two trials are the same two
                // `float_width` runs for shrink-to-fit and for the same reason:
                // measuring from the breaker that will set the text is what
                // stops the width and the set text disagreeing about where a
                // word ends.
                let inset = borders[at].map_or(consumed.border_width, |c| c.width);
                let right = consumed.padding_px(Side::Right, content_width) + inset.right;
                let left = consumed.padding_px(Side::Left, content_width) + inset.left;
                self.cell = Some(CellPass {
                    width: None,
                    borders: borders[at],
                });
                let max = self.measure_content(inner, MAX_MEASURE, depth + 1, avoid)? + right;
                self.cell = Some(CellPass {
                    width: None,
                    borders: borders[at],
                });
                let min = self.measure_content(inner, 0.0, depth + 1, avoid)? + right;
                let empty = left + right;
                (min.max(empty), max.max(min).max(empty))
            };
            cells.push(CellWidths {
                left: slot.left,
                columns: slot.columns,
                min,
                max,
                specified: specified[at],
            });
        }

        // **The third.** §17.5.2.2 spreads every spanning cell over every
        // column it touches, which is the product a nested table multiplies:
        // the inner table's whole distribution runs once inside each of the
        // outer cell's three layouts.
        let mut spread = grid.columns;
        for slot in &grid.slots {
            spread = spread.saturating_add(slot.columns);
        }
        self.budget.spend_layout(spread)?;

        let columns: Vec<f64> = if fixed_layout {
            let first: Vec<CellWidths> = grid
                .slots
                .iter()
                .enumerate()
                .filter(|(_, slot)| slot.top == 0)
                .map(|(at, slot)| CellWidths {
                    left: slot.left,
                    columns: slot.columns,
                    min: 0.0,
                    max: 0.0,
                    specified: specified[at],
                })
                .collect();
            table::fixed(grid.columns, &declared, &first, available)
        } else {
            let constraints = table::constraints(grid.columns, &cells, &declared);
            // §17.5.2.2's second pass. CAPMIN is zero here because the captions
            // were laid out at the *containing block's* width a few lines up
            // and cannot therefore make the table wider; see the crate's `Still
            // owed`.
            let used = match style.width {
                Size::Auto => table::automatic_width(&constraints, available, 0.0),
                Size::Length(_) => available,
            };
            table::distribute(&constraints, used)
        };

        let table_width = columns.iter().sum::<f64>() + spacing_total;
        // The record was made at the containing block's width, because `block`
        // cannot know a table's used width until §17.5.2 has run over its
        // contents. This is the one place a block record is corrected after the
        // fact, and the alternative -- a table box painted the full width of
        // the page with its cells in the left half of it -- is a background
        // that is visibly wrong and a border that is silently in the wrong
        // place.
        self.flow.blocks[block].width -= content_width - table_width;
        if table_width > content_width + EPSILON {
            self.warn(Warning::ContentOverflowedPage);
        }

        let mut lefts = Vec::with_capacity(grid.columns);
        let mut x = content_x + hspacing;
        for width in &columns {
            lefts.push(x);
            x += width + hspacing;
        }
        let span_width = |slot: &Slot| -> f64 {
            columns[slot.left..slot.left + slot.columns]
                .iter()
                .sum::<f64>()
                + (slot.columns.saturating_sub(1)) as f64 * hspacing
        };

        // Step 7: every cell, at its column's width, in document order.
        let mut laid: Vec<Option<Sublayout>> = (0..grid.slots.len()).map(|_| None).collect();
        for &at in &document {
            let slot = grid.slots[at];
            let cell = &tree.groups[slot.group].rows[slot.row].cells[slot.cell];
            let width = span_width(&slot);
            self.cell = Some(CellPass {
                width: Some(width),
                borders: borders[at],
            });
            laid[at] = Some(self.sublayout(cell.content.node(), width, depth + 1, avoid)?);
        }

        // §17.5.3's row heights: the rows a cell does not span first, then the
        // ones it does. The order is the same as the width algorithm's and for
        // the same reason -- a spanning cell met first would put its whole
        // height into its top row.
        let mut heights = vec![0.0f64; grid.rows];
        for (grid_row, (group, row)) in rows_of.iter().enumerate() {
            if let Some(row_node) = tree.groups[*group].rows[*row].node {
                if let Size::Length(length) = consume(&row_node.style).height {
                    heights[grid_row] = resolve_length(length, content_width).max(0.0);
                }
            }
        }
        for (at, slot) in grid.slots.iter().enumerate() {
            if slot.rows == 1 {
                let height = laid[at].as_ref().map_or(0.0, |sub| sub.height);
                heights[slot.top] = heights[slot.top].max(height);
            }
        }
        for (at, slot) in grid.slots.iter().enumerate() {
            if slot.rows <= 1 {
                continue;
            }
            let have: f64 = heights[slot.top..slot.top + slot.rows].iter().sum::<f64>()
                + (slot.rows - 1) as f64 * vspacing;
            let want = laid[at].as_ref().map_or(0.0, |sub| sub.height);
            if want > have {
                let extra = (want - have) / slot.rows as f64;
                for height in &mut heights[slot.top..slot.top + slot.rows] {
                    *height += extra;
                }
            }
        }
        let mut tops = Vec::with_capacity(grid.rows + 1);
        let mut y = 0.0;
        for height in &heights {
            tops.push(y);
            y += height + vspacing;
        }
        tops.push(y);

        // **A band, not a row.** A page may break between two rows and may not
        // break across a cell that spans them, so the unit the fragmenter sees
        // is the maximal run of rows a `rowspan` joins. With no `rowspan` in
        // the table every band is one row, which is where a book's table
        // breaks.
        let mut joined = vec![false; grid.rows];
        for slot in &grid.slots {
            for row in joined
                .iter_mut()
                .take(slot.top + slot.rows)
                .skip(slot.top + 1)
            {
                *row = true;
            }
        }
        let mut bands: Vec<(usize, usize)> = Vec::new();
        let mut start = 0usize;
        for (row, joined) in joined.iter().enumerate().skip(1) {
            if !joined {
                bands.push((start, row));
                start = row;
            }
        }
        bands.push((start, grid.rows));

        // Step 8: emit, in visual order. The margins standing at this position
        // are committed first -- CSS 2.2 §8.3.1 does not collapse a table's
        // margins through it, and the first thing emitted here is not a margin.
        self.commit_margin();
        let spacing_break = MarginBreak {
            forced: false,
            allowed_by_a: true,
            allowed_by_b: !avoid,
        };
        let mut open_group: Option<usize> = None;
        for &(from, to) in &bands {
            let group_index = rows_of[from].0;
            if open_group != Some(group_index) {
                if open_group.is_some() {
                    self.open.pop();
                    open_group = None;
                }
                if let Some(group_node) = tree.groups[group_index].node {
                    self.budget.spend_box()?;
                    let record = self.record(group_node, content_x, table_width);
                    self.open.push(record);
                    open_group = Some(group_index);
                }
            }
            // §17.6.1's vertical spacing, above every band including the
            // first. It is a `Margin` item and not an `Edge`, so it is
            // §13.3.3's case (1) -- the break position a table between two
            // pages needs.
            self.emit(vspacing, ItemKind::Margin(spacing_break.clone()), true);
            let band = self.band(
                &tree,
                &grid,
                &rows_of,
                &mut laid,
                &heights,
                &tops,
                &lefts,
                &columns,
                from,
                to,
                content_x,
                table_width,
                hspacing,
                vspacing,
            )?;
            let height: f64 = heights[from..to].iter().sum::<f64>()
                + (to - from).saturating_sub(1) as f64 * vspacing;
            self.emit(height, ItemKind::Rows(Box::new(band)), true);
        }
        if open_group.is_some() {
            self.open.pop();
        }
        // And below the last one, which is what makes the table's own content
        // height include §17.6.1's last spacing.
        self.emit(vspacing, ItemKind::Margin(spacing_break), true);
        Ok(())
    }

    /// One band of rows, as one flow item's worth of content.
    #[allow(clippy::too_many_arguments)]
    fn band(
        &mut self,
        tree: &TableBox<'_>,
        grid: &Grid,
        rows_of: &[(usize, usize)],
        laid: &mut [Option<Sublayout>],
        heights: &[f64],
        tops: &[f64],
        lefts: &[f64],
        columns: &[f64],
        from: usize,
        to: usize,
        content_x: f64,
        table_width: f64,
        hspacing: f64,
        vspacing: f64,
    ) -> Result<RowBand, Refusal> {
        let mut items: Vec<Item> = Vec::new();
        let mut blocks: Vec<BlockRecord> = Vec::new();
        let band_top = tops[from];

        // The row boxes first, so a cell's background covers a row's rather
        // than the other way round -- CSS 2.2 §17.5.1's layer order, and the
        // reason these records come before the cells' in this vector.
        for grid_row in from..to {
            let (group, row) = rows_of[grid_row];
            let Some(row_node) = tree.groups[group].rows[row].node else {
                continue;
            };
            self.budget.spend_box()?;
            let mut record = decorate(
                row_node,
                content_x + hspacing,
                (table_width - 2.0 * hspacing).max(0.0),
            );
            if !record.painted {
                continue;
            }
            // A spacer with the row's exact geometry, so the record's fragment
            // is the row's border box and not the extent of whatever text
            // happened to be in it. A row holding one short cell and one tall
            // one would otherwise be painted the height of the tall one in one
            // build and the short one in another, and both look like a row.
            let spacer = items.len();
            items.push(Item {
                y: tops[grid_row] - band_top,
                height: heights[grid_row],
                kind: ItemKind::Edge,
            });
            record.first = Some(spacer);
            record.last = spacer + 1;
            blocks.push(record);
        }

        for grid_row in from..to {
            for (at, slot) in grid.slots.iter().enumerate() {
                if slot.top != grid_row {
                    continue;
                }
                let Some(sub) = laid[at].take() else {
                    continue;
                };
                let cell_x = lefts[slot.left];
                let cell_top = tops[slot.top] - band_top;
                let cell_width = columns[slot.left..slot.left + slot.columns]
                    .iter()
                    .sum::<f64>()
                    + slot.columns.saturating_sub(1) as f64 * hspacing;
                let cell_height = heights[slot.top..slot.top + slot.rows].iter().sum::<f64>()
                    + slot.rows.saturating_sub(1) as f64 * vspacing;
                let Sublayout {
                    items: mut inner,
                    blocks: mut records,
                    floats,
                    height: _,
                } = sub;
                translate(&mut inner, &mut records, cell_x, cell_top);
                // §17.5.3: a cell's box is its row's height, whatever its
                // content came to. The spacer is what says so; without it a
                // one-line cell in a five-line row is painted one line tall,
                // which is a table with ragged backgrounds.
                let spacer = items.len();
                items.push(Item {
                    y: cell_top,
                    height: cell_height,
                    kind: ItemKind::Edge,
                });
                let base = items.len();
                for (index, mut record) in records.into_iter().enumerate() {
                    if index == 0 {
                        record.x = cell_x;
                        record.width = cell_width;
                        record.first = Some(spacer);
                        record.last = spacer + 1;
                    } else if let Some(first) = record.first {
                        record.first = Some(first + base);
                        record.last += base;
                    }
                    blocks.push(record);
                }
                items.append(&mut inner);
                // A float inside a cell stays inside the band: its own
                // formatting context is the cell's, so it cannot reach past the
                // row it is in and there is nothing for the page cutter to
                // carry forward.
                for mut float in floats {
                    translate(&mut float.items, &mut float.blocks, cell_x, cell_top);
                    let float_base = items.len();
                    for mut record in float.blocks {
                        if let Some(first) = record.first {
                            record.first = Some(first + float_base);
                            record.last += float_base;
                        }
                        blocks.push(record);
                    }
                    items.extend(float.items);
                }
            }
        }
        Ok(RowBand { items, blocks })
    }

    /// A block record for a box this module lays out itself — a row or a row
    /// group, neither of which goes through [`Builder::block`].
    fn record(&mut self, node: &BoxNode, x: f64, width: f64) -> usize {
        let record = decorate(node, x, width);
        let index = self.flow.blocks.len();
        self.flow.blocks.push(record);
        index
    }

    /// A `list-item`'s marker, on the first line of its own box.
    fn marker(&mut self, style: &Consumed, block: usize, content_x: f64, ordinal: usize) {
        let text = marker_text(style.list_style_type, ordinal + 1);
        if text.is_empty() {
            return;
        }
        let Some(first) = self.flow.blocks[block].first else {
            return;
        };
        let font = style.font();
        let width = self.metrics.measure(&text, &font);
        for index in first..self.flow.blocks[block].last {
            if let ItemKind::Line(line) = &mut self.flow.items[index].kind {
                // The marker reads before the first word of its own item, and
                // the stamp says so: sorting a page's runs into document order
                // is a **stable** sort, so a marker sharing the first run's
                // number stays in front of it.
                let order = line.runs.first().map_or(0, |run| run.order);
                line.runs.insert(
                    0,
                    TextRun {
                        // Outside the content box, half an em clear of it,
                        // which is `list-style-position: outside`'s initial
                        // value.
                        x: content_x - width - style.font_size * 0.5,
                        y: 0.0,
                        width,
                        text,
                        font_size: style.font_size,
                        families: style.families.clone(),
                        weight: style.font_weight,
                        style: style.font_style,
                        variant: style.font_variant,
                        color: style.color,
                        decoration: style.text_decoration,
                        painted: style.visible,
                        letter_spacing: 0.0,
                        word_spacing: 0.0,
                        generated: true,
                        anchor: None,
                        order,
                    },
                );
                return;
            }
        }
    }

    /// The inline formatting context: pieces in, line boxes out.
    fn lines(
        &mut self,
        pieces: &[Piece],
        container: &Consumed,
        block: usize,
        content_x: f64,
        content_width: f64,
    ) -> Result<(), Refusal> {
        if pieces.is_empty() {
            return Ok(());
        }
        // One string for the whole context, because UAX #14 is about text and
        // not about elements: a break opportunity between `<em>a</em>` and
        // `<em>b</em>` is decided by the characters either side of it, and a
        // breaker run per element would never see the pair.
        let mut content = String::new();
        let mut spans: Vec<(usize, usize, usize)> = Vec::new();
        for (index, piece) in pieces.iter().enumerate() {
            let start = content.len();
            content.push_str(&piece.text);
            spans.push((start, content.len(), index));
        }
        if content.is_empty() {
            return Ok(());
        }
        self.budget.spend_breaks(content.chars().count())?;
        let opportunities = uax14::opportunities(&content, container.tailoring);

        let indent = match container.text_indent {
            LengthPercentage::Px(px) => px,
            LengthPercentage::Percent(percent) => content_width * percent / 100.0,
        };

        let mut start = 0usize;
        let mut first_line = true;
        let mut lines_here = 0usize;
        let mut cursor = 0usize;
        let first_item = self.flow.items.len();
        while start < content.len() {
            // `cursor` is where the previous line stopped looking, and it is
            // not an optimisation. Restarting the scan at zero for every line
            // makes filling a paragraph `O(lines x opportunities)`, which for a
            // page one point wide is `O(characters^2)` -- and a paragraph is
            // exactly the input a hostile book has an unlimited supply of.
            // `5adf502`'s finding, in the loop rather than in the recursion.
            while cursor < opportunities.len() && opportunities[cursor].at <= start {
                cursor += 1;
            }
            let indent_here = if first_line { indent } else { 0.0 };
            // §9.5's other half: the measure is what the floats beside this
            // line have left of it, and where nothing is left the line goes
            // under them. Both are decided **before** the line is filled,
            // because the width is what decides where it breaks.
            let (line_x, available) = self.beside(
                container,
                content_x,
                content_width,
                indent_here,
                &content,
                &spans,
                pieces,
                &opportunities[cursor..],
                start,
            )?;
            let (end, hard) = self.fit(
                &content,
                &spans,
                pieces,
                &opportunities[cursor..],
                start,
                available,
            );
            let (trim_start, trim_end) = self.trim(&content, &spans, pieces, start, end);
            // §6: the last line of a paragraph is not justified, and a
            // preserved newline ends a paragraph exactly as the end of the
            // text does. The test is on `end` rather than on the trimmed end,
            // because a last line with a trailing space would otherwise be
            // stretched to fill the measure and nothing about it would look
            // wrong until somebody counted.
            let justify =
                container.text_align == TextAlign::Justify && !hard && end < content.len();
            self.line(
                &content, &spans, pieces, container, block, line_x, available, trim_start,
                trim_end, justify, lines_here,
            );
            lines_here += 1;
            first_line = false;
            start = end;
        }
        // `lines_in_block` cannot be known when a line is made, so it is
        // patched here. Rule C is about *"the number of line boxes between the
        // break and the end of the box"*, which is a fact about the finished
        // block and not about the line.
        for index in first_item..self.flow.items.len() {
            if let ItemKind::Line(line) = &mut self.flow.items[index].kind {
                line.lines_in_block = lines_here;
            }
        }
        Ok(())
    }

    /// Where the next line box starts and how wide it is, given the floats.
    ///
    /// CSS 2.2 §9.5: *"line boxes are shortened to make room for the float"* —
    /// and §9.5's other sentence, the one an implementation leaves out:
    /// *"if a shortened line box is too small to contain any content, then it
    /// is shifted downward until either it fits or there are no more floats
    /// present."* A build with only the first sentence sets one word per line
    /// down the side of a wide figure and never recovers.
    ///
    /// **The height it asks the band about is the strut's**, not the line's.
    /// The line's own height is not known until it has been filled and it
    /// cannot be filled until its width is known, so something has to be
    /// assumed; the container's own `line-height` is the assumption every
    /// line in a book of uniform text meets exactly, and the case it gets
    /// wrong — one oversized inline in the last line beside a float — is worth
    /// less than the circularity it avoids.
    #[allow(clippy::too_many_arguments)]
    fn beside(
        &mut self,
        container: &Consumed,
        content_x: f64,
        content_width: f64,
        indent: f64,
        content: &str,
        spans: &[(usize, usize, usize)],
        pieces: &[Piece],
        opportunities: &[uax14::Opportunity],
        start: usize,
    ) -> Result<(f64, f64), Refusal> {
        let full = (content_width - indent).max(0.0);
        if self.floats.is_empty() {
            return Ok((content_x + indent, full));
        }
        let left = content_x;
        let right = content_x + content_width;
        let height = container.line_height.max(0.0);
        let top = self.cursor();
        // What has to fit for the line to be worth setting here: the first
        // unbreakable run of it. A word longer than the whole measure would
        // never fit anywhere, so it is not a reason to go looking below a
        // float — that line overflows wherever it is put.
        let first = opportunities.first().map_or(content.len(), |o| o.at);
        let word = self.measure(content, spans, pieces, start, first)
            - self.trailing(content, spans, pieces, start, first);
        let mut chosen = top;
        let (band_left, band_right) = loop {
            // Two scans for the band and one for the step below it, each over
            // every float in this context.
            //
            // **One charge and one band.** This loop used to leave its band
            // behind and a second call recomputed it afterwards, at the same
            // height, over the same list, for the same answer — and the charge
            // for that second scan was one no book could ever reach, because
            // this one fires first. The injection campaign is what said so.
            self.budget.spend_layout(3 * self.floats.len())?;
            let band = self.floats.band(chosen, chosen + height, left, right);
            if word > full + EPSILON || band.1 - band.0 - indent >= word - EPSILON {
                break band;
            }
            match self.floats.next_bottom(chosen) {
                Some(next) => chosen = next,
                None => break band,
            }
        };
        if chosen > top {
            // The line goes below the float, and the space it left is part of
            // this block: a background painted behind a paragraph is painted
            // behind the gap beside the figure too.
            self.commit_margin();
            self.emit(chosen - top, ItemKind::Edge, true);
        }
        Ok((
            band_left + indent,
            (band_right - band_left - indent).max(0.0),
        ))
    }

    /// Where the next line ends: the last break opportunity that fits, or the
    /// first one if none does.
    ///
    /// The second half of the answer is whether the line ended at a **hard**
    /// break — a preserved newline — because §6 does not justify the last line
    /// of a paragraph and a paragraph ends at a hard break as well as at the
    /// end of the text.
    ///
    /// The order of the two tests below is the whole function. UAX #14's LB3
    /// makes the end of the text a mandatory break, so a build that asked *is
    /// this mandatory?* before *does this fit?* takes the end of the text on
    /// the first iteration and sets every paragraph as one line — which is a
    /// book with no line breaking at all, and every English fixture that
    /// happens to be shorter than a line still passes.
    fn fit(
        &mut self,
        content: &str,
        spans: &[(usize, usize, usize)],
        pieces: &[Piece],
        opportunities: &[uax14::Opportunity],
        start: usize,
        available: f64,
    ) -> (usize, bool) {
        let mut cursor = start;
        let mut width = 0.0;
        let mut best: Option<usize> = None;
        for opportunity in opportunities.iter() {
            let hard = opportunity.mandatory && opportunity.at < content.len();
            if !self.wrappable(spans, pieces, opportunity.at) && !hard {
                continue;
            }
            width += self.measure(content, spans, pieces, cursor, opportunity.at);
            cursor = opportunity.at;
            let trailing = self.trailing(content, spans, pieces, start, opportunity.at);
            if width - trailing <= available {
                if hard {
                    return (opportunity.at, true);
                }
                best = Some(opportunity.at);
                continue;
            }
            if let Some(at) = best {
                return (at, false);
            }
            // Nothing fits and this is the first opportunity: the word is
            // longer than the line. `css-text-3` §5.4 is what decides between
            // setting it anyway and breaking inside it.
            if matches!(
                self.overflow_wrap(spans, pieces, start),
                OverflowWrap::BreakWord | OverflowWrap::Anywhere
            ) {
                if let Some(at) =
                    self.break_inside(content, spans, pieces, start, opportunity.at, available)
                {
                    return (at, false);
                }
            }
            self.warn(Warning::LineOverflowed);
            return (opportunity.at, hard);
        }
        (best.unwrap_or(content.len()), false)
    }

    /// `overflow-wrap`'s last resort: the largest prefix of one unbreakable
    /// word that fits, at least one character.
    fn break_inside(
        &mut self,
        content: &str,
        spans: &[(usize, usize, usize)],
        pieces: &[Piece],
        start: usize,
        limit: usize,
        available: f64,
    ) -> Option<usize> {
        let mut width = 0.0;
        let mut last = None;
        for (offset, ch) in content[start..limit].char_indices() {
            let at = start + offset;
            let next = at + ch.len_utf8();
            width += self.measure(content, spans, pieces, at, next);
            if width > available && last.is_some() {
                return last;
            }
            last = Some(next);
        }
        None
    }

    /// Whether the piece that a boundary falls after allows wrapping.
    ///
    /// `white-space: nowrap` is per element, so a context that mixes a
    /// `nowrap` span with ordinary text has opportunities in one and not in the
    /// other — which is why this is a lookup per boundary rather than a flag on
    /// the whole context.
    fn wrappable(&self, spans: &[(usize, usize, usize)], pieces: &[Piece], at: usize) -> bool {
        let Some(piece) = piece_at(spans, at.saturating_sub(1)) else {
            return true;
        };
        text::wraps(pieces[piece].style.white_space)
    }

    fn overflow_wrap(
        &self,
        spans: &[(usize, usize, usize)],
        pieces: &[Piece],
        at: usize,
    ) -> OverflowWrap {
        piece_at(spans, at).map_or(OverflowWrap::Normal, |p| pieces[p].style.overflow_wrap)
    }

    /// The advance of one byte range, spanning as many pieces as it must.
    fn measure(
        &self,
        content: &str,
        spans: &[(usize, usize, usize)],
        pieces: &[Piece],
        from: usize,
        to: usize,
    ) -> f64 {
        let mut total = 0.0;
        for (start, end, index) in spans {
            let lo = (*start).max(from);
            let hi = (*end).min(to);
            if lo >= hi {
                continue;
            }
            let style = &pieces[*index].style;
            let slice = &content[lo..hi];
            total += self.metrics.measure(slice, &style.font());
            total += style.letter_spacing * slice.chars().count() as f64;
            total += style.word_spacing * slice.chars().filter(|c| *c == ' ').count() as f64;
        }
        total
    }

    /// The advance of the collapsible spaces at the end of a range, which
    /// §4.1.2 hangs rather than sets.
    fn trailing(
        &self,
        content: &str,
        spans: &[(usize, usize, usize)],
        pieces: &[Piece],
        from: usize,
        to: usize,
    ) -> f64 {
        let slice = &content[from..to];
        let trimmed = slice.trim_end_matches([' ', '\n']);
        self.measure(content, spans, pieces, from + trimmed.len(), to)
    }

    /// Phase II, §4.1.2: the two ends of one line.
    fn trim(
        &self,
        content: &str,
        spans: &[(usize, usize, usize)],
        pieces: &[Piece],
        start: usize,
        end: usize,
    ) -> (usize, usize) {
        let collapsing =
            piece_at(spans, start).is_none_or(|p| text::collapses(pieces[p].style.white_space));
        let slice = &content[start..end];
        // A trailing segment break is a break, not a character to set: it is
        // the reason this line ended.
        let without_break = slice.strip_suffix('\n').unwrap_or(slice);
        if !collapsing {
            return (start, start + without_break.len());
        }
        let (offset, length) =
            text::trim_line(without_break, tinker_pdf_css::property::WhiteSpace::Normal);
        (start + offset, start + length)
    }

    /// Emits one line box.
    #[allow(clippy::too_many_arguments)]
    fn line(
        &mut self,
        content: &str,
        spans: &[(usize, usize, usize)],
        pieces: &[Piece],
        container: &Consumed,
        block: usize,
        x: f64,
        available: f64,
        start: usize,
        end: usize,
        justify: bool,
        index_in_block: usize,
    ) {
        // CSS 2.2 §10.8.1's strut: every line box carries the block
        // container's own font and `line-height`, whether or not any text on it
        // uses them. Without it an empty line has no height and a line of small
        // text in a large paragraph is too short.
        let strut = self.metrics.vertical(&container.font());
        let strut_leading = (container.line_height - strut.height()) / 2.0;
        let mut above = strut.ascent + strut_leading;
        let mut below = strut.descent + strut_leading;

        let mut runs: Vec<TextRun> = Vec::new();
        let mut width = 0.0;
        for (span_start, span_end, index) in spans {
            let lo = (*span_start).max(start);
            let hi = (*span_end).min(end);
            if lo >= hi {
                continue;
            }
            let style = &pieces[*index].style;
            let font = style.font();
            let vertical = self.metrics.vertical(&font);
            let leading = (style.line_height - vertical.height()) / 2.0;
            above = above.max(vertical.ascent + leading);
            below = below.max(vertical.descent + leading);
            let text = content[lo..hi].to_string();
            let advance = self.metrics.measure(&text, &font)
                + style.letter_spacing * text.chars().count() as f64
                + style.word_spacing * text.chars().filter(|c| *c == ' ').count() as f64;
            runs.push(TextRun {
                x: 0.0,
                y: 0.0,
                width: advance,
                text,
                font_size: style.font_size,
                families: style.families.clone(),
                weight: style.font_weight,
                style: style.font_style,
                variant: style.font_variant,
                color: style.color,
                decoration: style.text_decoration,
                painted: style.visible,
                letter_spacing: style.letter_spacing,
                word_spacing: style.word_spacing,
                generated: false,
                anchor: pieces[*index].anchor,
                order: pieces[*index].order,
            });
            width += advance;
        }

        // §6's alignment. Justification distributes the slack over the spaces
        // rather than over the characters, which is what a text engine does and
        // what `Tw` in a content stream can express.
        let slack = (available - width).max(0.0);
        let spaces: usize = runs
            .iter()
            .map(|run| run.text.chars().filter(|c| *c == ' ').count())
            .sum();
        let mut extra_per_space = 0.0;
        let mut offset = match container.text_align {
            TextAlign::Left => 0.0,
            TextAlign::Right => slack,
            TextAlign::Center => slack / 2.0,
            TextAlign::Justify => {
                if justify && spaces > 0 {
                    extra_per_space = slack / spaces as f64;
                }
                0.0
            }
        };
        offset += x;
        for run in &mut runs {
            run.x = offset;
            let count = run.text.chars().filter(|c| *c == ' ').count() as f64;
            run.width += extra_per_space * count;
            run.word_spacing += extra_per_space;
            offset += run.width;
        }

        let height = above + below;
        let line = LineBox {
            baseline: above,
            runs,
            index_in_block,
            lines_in_block: 0,
            orphans: container.orphans,
            widows: container.widows,
            // Rule D is *"the `page-break-inside` property is `auto`"*, and the
            // property this build does not inherit — see `Property::inherited`
            // and the argument beside it — so the ancestor chain has to be
            // consulted here rather than left to the cascade. `open_avoid`
            // holds exactly the enclosing boxes that avoid, which is the
            // question rule D asks.
            avoid_inside: !self.open_avoid.is_empty(),
        };
        // A line box is content, so every margin adjoining above it is
        // committed here. That is what stops a parent's top margin collapsing
        // with a child's when there is text between them.
        self.commit_margin();
        let _ = block;
        // §9.5.1's rule 6, and the only place it is recorded: a float may not
        // rise above a line box that already holds earlier content.
        self.ceiling_line = self.ceiling_line.max(self.y);
        self.emit(height, ItemKind::Line(Box::new(line)), true);
    }
}

/// A length or a percentage against a containing width.
fn resolve_length(length: LengthPercentage, containing: f64) -> f64 {
    match length {
        LengthPercentage::Px(px) => px,
        LengthPercentage::Percent(percent) => containing * percent / 100.0,
    }
}

/// One box's decorations, as a record with no items in it yet.
fn decorate(node: &BoxNode, x: f64, width: f64) -> BlockRecord {
    let style = consume(&node.style);
    let painted = style.background_color.a != 0
        || style.border_width.top > 0.0
        || style.border_width.right > 0.0
        || style.border_width.bottom > 0.0
        || style.border_width.left > 0.0;
    BlockRecord {
        x,
        width,
        first: None,
        last: 0,
        background: style.background_color,
        border_width: style.border_width,
        border_style: style.border_style,
        border_color: style.border_color,
        painted: painted && style.visible,
    }
}

/// One box's **specified** border on one side, for §17.6.2.1.
///
/// Specified and not used, which is [`Edge::width`]'s whole note: §8.5.3 makes
/// a `hidden` border's used width zero, and §17.6.2.1's first rule is that a
/// `hidden` border beats every other. A build that collapsed used widths would
/// find `hidden` at zero, lose on width, and draw the border the author hid.
fn specified_edge(node: &BoxNode, side: Side, origin: Origin) -> Edge {
    Edge {
        style: node.style.border_style.get(side),
        width: node.style.border_width.get(side).max(0.0),
        color: node.style.border_color.get(side),
        origin,
    }
}

/// The collapsed border of every cell, CSS 2.2 §17.6.2.
///
/// Each of the four sides of each cell is a grid line, and every box that
/// touches that line brings a border to it: the two cells on either side, their
/// rows, their row groups, the columns, the column groups and the table.
/// [`table::collapse`] then applies §17.6.2.1's five rules to the set.
///
/// **Half the resolved width at an inner line and the whole of it at an outer
/// one.** §17.6.2 centres a collapsed border on the grid line, which would put
/// half the table's outermost border outside the table box; this build keeps
/// that half inside. The ink is the same width either way and the table is half
/// a border narrower than a browser's, which is the divergence and is named in
/// the crate's `Still owed`.
fn collapsed_borders(
    table: &BoxNode,
    tree: &TableBox<'_>,
    grid: &Grid,
    occupancy: &[Vec<Option<usize>>],
    rows_of: &[(usize, usize)],
) -> Vec<Collapsed> {
    let row_node = |grid_row: usize| -> Option<&BoxNode> {
        rows_of
            .get(grid_row)
            .and_then(|(group, row)| tree.groups[*group].rows[*row].node)
    };
    let group_node = |grid_row: usize| -> Option<&BoxNode> {
        rows_of
            .get(grid_row)
            .and_then(|(group, _)| tree.groups[*group].node)
    };
    let group_of = |grid_row: usize| rows_of.get(grid_row).map(|(group, _)| *group);
    let column_nodes = |column: usize| -> (Option<&BoxNode>, Option<&BoxNode>) {
        match tree.columns.get(column) {
            Some(box_) => (box_.node, box_.group),
            None => (None, None),
        }
    };
    let mut out = Vec::with_capacity(grid.slots.len());
    for slot in &grid.slots {
        let mut edges: [Vec<Edge>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
        let cell = |at: usize| -> &BoxNode {
            let slot = &grid.slots[at];
            tree.groups[slot.group].rows[slot.row].cells[slot.cell]
                .content
                .node()
        };
        let me = tree.groups[slot.group].rows[slot.row].cells[slot.cell]
            .content
            .node();
        let last_row = slot.top + slot.rows;
        let last_column = slot.left + slot.columns;

        // Top.
        edges[0].push(specified_edge(me, Side::Top, Origin::Cell));
        if let Some(node) = row_node(slot.top) {
            edges[0].push(specified_edge(node, Side::Top, Origin::Row));
        }
        if slot.top == 0 {
            edges[0].push(specified_edge(table, Side::Top, Origin::Table));
            for column in slot.left..last_column {
                let (col, group) = column_nodes(column);
                if let Some(node) = col {
                    edges[0].push(specified_edge(node, Side::Top, Origin::Column));
                }
                if let Some(node) = group {
                    edges[0].push(specified_edge(node, Side::Top, Origin::ColumnGroup));
                }
            }
        } else {
            for above in occupancy[slot.top - 1]
                .iter()
                .take(last_column)
                .skip(slot.left)
                .flatten()
            {
                edges[0].push(specified_edge(cell(*above), Side::Bottom, Origin::Cell));
            }
            if let Some(node) = row_node(slot.top - 1) {
                edges[0].push(specified_edge(node, Side::Bottom, Origin::Row));
            }
        }
        if group_of(slot.top) != slot.top.checked_sub(1).and_then(group_of) {
            if let Some(node) = group_node(slot.top) {
                edges[0].push(specified_edge(node, Side::Top, Origin::RowGroup));
            }
            if let Some(node) = slot.top.checked_sub(1).and_then(group_node) {
                edges[0].push(specified_edge(node, Side::Bottom, Origin::RowGroup));
            }
        }

        // Bottom.
        edges[2].push(specified_edge(me, Side::Bottom, Origin::Cell));
        if let Some(node) = row_node(last_row - 1) {
            edges[2].push(specified_edge(node, Side::Bottom, Origin::Row));
        }
        if last_row >= grid.rows {
            edges[2].push(specified_edge(table, Side::Bottom, Origin::Table));
            for column in slot.left..last_column {
                let (col, group) = column_nodes(column);
                if let Some(node) = col {
                    edges[2].push(specified_edge(node, Side::Bottom, Origin::Column));
                }
                if let Some(node) = group {
                    edges[2].push(specified_edge(node, Side::Bottom, Origin::ColumnGroup));
                }
            }
        } else {
            for below in occupancy[last_row]
                .iter()
                .take(last_column)
                .skip(slot.left)
                .flatten()
            {
                edges[2].push(specified_edge(cell(*below), Side::Top, Origin::Cell));
            }
            if let Some(node) = row_node(last_row) {
                edges[2].push(specified_edge(node, Side::Top, Origin::Row));
            }
        }
        if group_of(last_row.saturating_sub(1)) != group_of(last_row) {
            if let Some(node) = group_node(last_row - 1) {
                edges[2].push(specified_edge(node, Side::Bottom, Origin::RowGroup));
            }
            if let Some(node) = group_node(last_row) {
                edges[2].push(specified_edge(node, Side::Top, Origin::RowGroup));
            }
        }

        // Left.
        edges[3].push(specified_edge(me, Side::Left, Origin::Cell));
        let (col, colgroup) = column_nodes(slot.left);
        if let Some(node) = col {
            edges[3].push(specified_edge(node, Side::Left, Origin::Column));
        }
        if let Some(node) = colgroup {
            edges[3].push(specified_edge(node, Side::Left, Origin::ColumnGroup));
        }
        if slot.left == 0 {
            edges[3].push(specified_edge(table, Side::Left, Origin::Table));
            for row in slot.top..last_row {
                if let Some(node) = row_node(row) {
                    edges[3].push(specified_edge(node, Side::Left, Origin::Row));
                }
                if let Some(node) = group_node(row) {
                    edges[3].push(specified_edge(node, Side::Left, Origin::RowGroup));
                }
            }
        } else {
            for row in occupancy.iter().take(last_row).skip(slot.top) {
                if let Some(left) = row[slot.left - 1] {
                    edges[3].push(specified_edge(cell(left), Side::Right, Origin::Cell));
                }
            }
            let (col, colgroup) = column_nodes(slot.left - 1);
            if let Some(node) = col {
                edges[3].push(specified_edge(node, Side::Right, Origin::Column));
            }
            if let Some(node) = colgroup {
                edges[3].push(specified_edge(node, Side::Right, Origin::ColumnGroup));
            }
        }

        // Right.
        edges[1].push(specified_edge(me, Side::Right, Origin::Cell));
        let (col, colgroup) = column_nodes(last_column - 1);
        if let Some(node) = col {
            edges[1].push(specified_edge(node, Side::Right, Origin::Column));
        }
        if let Some(node) = colgroup {
            edges[1].push(specified_edge(node, Side::Right, Origin::ColumnGroup));
        }
        if last_column >= grid.columns {
            edges[1].push(specified_edge(table, Side::Right, Origin::Table));
            for row in slot.top..last_row {
                if let Some(node) = row_node(row) {
                    edges[1].push(specified_edge(node, Side::Right, Origin::Row));
                }
                if let Some(node) = group_node(row) {
                    edges[1].push(specified_edge(node, Side::Right, Origin::RowGroup));
                }
            }
        } else {
            for row in occupancy.iter().take(last_row).skip(slot.top) {
                if let Some(right) = row[last_column] {
                    edges[1].push(specified_edge(cell(right), Side::Left, Origin::Cell));
                }
            }
            let (col, colgroup) = column_nodes(last_column);
            if let Some(node) = col {
                edges[1].push(specified_edge(node, Side::Left, Origin::Column));
            }
            if let Some(node) = colgroup {
                edges[1].push(specified_edge(node, Side::Left, Origin::ColumnGroup));
            }
        }

        let outer = [
            slot.top == 0,
            last_column >= grid.columns,
            last_row >= grid.rows,
            slot.left == 0,
        ];
        let mut width = Sides::all(0.0);
        let mut style = Sides::all(BorderStyle::None);
        let mut color = Sides::all(Color::BLACK);
        for (index, side) in [Side::Top, Side::Right, Side::Bottom, Side::Left]
            .into_iter()
            .enumerate()
        {
            let won = table::collapse(&edges[index]);
            let used = won.used_width();
            width.set(side, if outer[index] { used } else { used / 2.0 });
            style.set(side, won.style);
            color.set(side, won.color);
        }
        out.push(Collapsed {
            width,
            style,
            color,
        });
    }
    out
}

/// §17.2.1 rule 9's anonymous table, around a run of misparented boxes.
fn anonymous_table(parent: &ComputedStyle, run: &[BoxNode]) -> BoxNode {
    let mut style = ComputedStyle::inherit_from(parent);
    style.display = Display::Table;
    BoxNode {
        style,
        content: Content::Children(run.to_vec()),
        anchor: None,
        span: crate::CellSpan::ONE,
    }
}

/// Moves a finished sub-flow to where its float was placed.
///
/// A run's `y` is not touched because a run has not got one yet: it is written
/// at pagination out of its line box's position, so moving the item moves the
/// text with it.
fn translate(items: &mut [Item], blocks: &mut [BlockRecord], dx: f64, dy: f64) {
    for item in items {
        item.y += dy;
        match &mut item.kind {
            ItemKind::Line(line) => {
                for run in &mut line.runs {
                    run.x += dx;
                }
            }
            // A band's items are already relative to the band, so only the
            // horizontal half of the move reaches inside it. A build that
            // passed `dy` down as well would move a table inside a float twice.
            ItemKind::Rows(band) => translate(&mut band.items, &mut band.blocks, dx, 0.0),
            ItemKind::Margin(_) | ItemKind::Edge => {}
        }
    }
    for block in blocks {
        block.x += dx;
    }
}

/// Which piece a byte offset belongs to.
///
/// A binary search rather than a scan, and for the same reason the line
/// filler carries a cursor: this is called once per break opportunity, so a
/// linear scan makes a paragraph of a thousand `<em>`s cost
/// `O(pieces x characters)`. The spans are built in document order and are
/// disjoint, so the search is sound by construction.
fn piece_at(spans: &[(usize, usize, usize)], at: usize) -> Option<usize> {
    let found = spans.binary_search_by(|(start, end, _)| {
        if at < *start {
            std::cmp::Ordering::Greater
        } else if at >= *end {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Equal
        }
    });
    found.ok().map(|index| spans[index].2)
}

/// A list marker's text, CSS 2.2 §12.5.
///
/// The three alphabetic and two Roman forms are computed rather than tabled,
/// because a table stops at whatever length its author thought of and a book
/// with more list items than that gets a marker that is silently wrong.
#[must_use]
pub fn marker_text(kind: ListStyleType, ordinal: usize) -> String {
    match kind {
        ListStyleType::None => String::new(),
        ListStyleType::Disc => "\u{2022}".to_string(),
        ListStyleType::Circle => "\u{25e6}".to_string(),
        ListStyleType::Square => "\u{25aa}".to_string(),
        ListStyleType::Decimal => format!("{ordinal}."),
        ListStyleType::LowerAlpha => format!("{}.", alphabetic(ordinal, b'a')),
        ListStyleType::UpperAlpha => format!("{}.", alphabetic(ordinal, b'A')),
        ListStyleType::LowerRoman => format!("{}.", roman(ordinal).to_lowercase()),
        ListStyleType::UpperRoman => format!("{}.", roman(ordinal)),
    }
}

/// Bijective base 26: 1 is `a`, 26 is `z`, 27 is `aa`.
///
/// **Not** ordinary base 26, which is the mistake: `z` is 26 and the next is
/// `aa`, not `ba`, and there is no digit for zero.
fn alphabetic(ordinal: usize, first: u8) -> String {
    if ordinal == 0 {
        return String::new();
    }
    let mut out = Vec::new();
    let mut n = ordinal;
    while n > 0 {
        let digit = (n - 1) % 26;
        out.push(first + digit as u8);
        n = (n - 1) / 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// Roman numerals, subtractive, up to 3 999; above that the number itself,
/// because there is no agreed spelling and a wrong one is worse than a digit.
fn roman(ordinal: usize) -> String {
    const TABLE: [(usize, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    if ordinal == 0 || ordinal > 3_999 {
        return ordinal.to_string();
    }
    let mut out = String::new();
    let mut n = ordinal;
    for (value, sign) in TABLE {
        while n >= value {
            out.push_str(sign);
            n -= value;
        }
    }
    out
}
