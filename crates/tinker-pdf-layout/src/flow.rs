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
use std::sync::Arc;

use tinker_pdf_css::cascade::ComputedStyle;
use tinker_pdf_css::property::{
    AlignItems, BorderCollapse, BorderStyle, BoxSizing, Clear, Color, ColumnCount, ColumnFill,
    ColumnSpan, ColumnWidth, Direction, Display, Float, Hyphens, Inset, LengthPercentage,
    ListStylePosition, ListStyleType, MarginValue, OverflowWrap, PageBreak, PageBreakInside,
    Position, Side, Sides, Size, TableLayout, TextAlign, UnicodeBidi, VerticalAlign, ZIndex,
};

use crate::flex;
use crate::floats::{Ceilings, FloatContext, Placed};
use crate::limits::MAX_EMBEDDING_DEPTH;
use crate::metrics::{FirstStrong, FontRequest, Metrics, Neighbour, ShapingContext, CONTEXT_BYTES};
use crate::style::{consume, Consumed};
use crate::table::{self, CellWidths, Edge, Grid, Origin, Slot, TableBox};
use crate::text::{self, Collapser};
use crate::uax14;
use crate::{
    BoxNode, Budget, Content, Embedding, EmbeddingKind, Intrinsic, Limits, Options, Refusal,
    TextRun, Warning,
};

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
    /// The picture inside it, for a replaced box, CSS 2.2 §3.1.
    ///
    /// On the record rather than in [`Flow::items`] and that is not a filing
    /// decision: a record is the one thing in this module that is already
    /// carried through every context a box can end up in — a float, a band, an
    /// atomic inline, a column — and every one of those already moves a
    /// record's `x` when it moves the box. Putting the picture anywhere else
    /// would mean four more places to move it and four ways to forget.
    pub replaced: Option<ReplacedPaint>,
    /// CSS 2.2 §9.4.3's relative offset, carried to **paint**.
    ///
    /// The horizontal half of the offset is folded into `x`, because a record's
    /// `x` is already absolute; the vertical half cannot be, because a record's
    /// `y` is its items' and its items are the flow's. §9.4.3 says the offset
    /// *"does not affect the layout of any other box"*, and this field is that
    /// sentence: the flow keeps the box where it was and only the ink moves.
    pub dy: f64,
    /// The node's [`crate::BoxNode::anchor`], for [`crate::BoxFragment::anchor`].
    pub anchor: Option<u32>,
    /// What else the box paints — its corners' radii and its outline — boxed
    /// because almost no box has either and a record is in the frame of every
    /// recursion of [`Builder::block`].
    pub paint: Option<Box<crate::style::BoxPaint>>,
    /// The axes its overflow clip cuts, `css-overflow-3` §3.1 — **set only
    /// once its content is found to reach past its padding box**, by
    /// [`Builder::note_overflow`] or [`Builder::clip_tail`]. A box whose
    /// `overflow` clips and whose content fits keeps [`Clip::NONE`], and the
    /// page it is on carries no clip for it: a clip that removes nothing is
    /// not written.
    pub clip: Clip,
}

/// Which axes a box's overflow clips, `css-overflow-3` §3.1.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Clip {
    /// `overflow-x` is not `visible`.
    pub x: bool,
    /// `overflow-y` is not `visible`.
    pub y: bool,
}

impl Clip {
    /// No clip.
    pub const NONE: Clip = Clip { x: false, y: false };

    /// The axes a box clips, from its computed style.
    ///
    /// **`overflow` applies to block containers** (§3.1's *"Applies to"*), so
    /// a table box — whose content is a grid and not a flow — and a replaced
    /// box — whose content is a picture sized to it — clip nothing whatever
    /// they declare. A table **cell** is a block container and does clip.
    fn of(style: &Consumed, replaced: bool) -> Clip {
        if replaced || style.is_table() {
            return Clip::NONE;
        }
        Clip {
            x: style.overflow_x.clips(),
            y: style.overflow_y.clips(),
        }
    }

    /// Either axis.
    pub fn any(self) -> bool {
        self.x || self.y
    }
}

/// A replaced box's picture, as an inset from the box's own border-box corner.
///
/// **Insets and not absolute coordinates**, which is the whole reason this is
/// three numbers rather than a rectangle: [`translate`] moves a record by
/// adding to its `x`, and a picture stored at an absolute `x` would be left
/// behind by every flex placement, every band and every float in the crate. An
/// inset moves with the box for free.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ReplacedPaint {
    /// Border-box left edge to content-box left edge: `border-left` plus
    /// `padding-left`.
    pub left: f64,
    /// Border-box top edge to content-box top edge.
    pub top: f64,
    /// The used `width`, CSS 2.2 §10.3.2.
    pub width: f64,
    /// The used `height`, §10.6.2.
    pub height: f64,
    /// The node's [`crate::BoxNode::anchor`], carried unchanged.
    pub anchor: Option<u32>,
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
    /// The atomic inline boxes on it, §9.2.2, `x` already absolute and `dy`
    /// relative to the baseline.
    pub boxes: Vec<InlineBox>,
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

/// A set of boxes that sit **beside** one another and cannot be separated.
///
/// Three things in this crate are that shape and they arrived one milestone
/// apart each, so the type is named for what it is rather than for the first
/// of them:
///
/// - **One band of table rows**, CSS 2.2 §17. A band and not a row, and the
///   difference is `rowspan`: a page may break between two rows and may not
///   break across a cell that spans them, so the unit the fragmenter sees is
///   the maximal run of grid rows joined by a spanning cell. A table with no
///   `rowspan` in it has one band per row, which is where a real book's table
///   breaks.
/// - **One flex line**, `css-flexbox-1` §9. A row container's items sit beside
///   each other along the main axis; a column container's whole content is one
///   of these, because its lines sit beside each other too.
/// - **A whole multi-column container**, `css-multicol-1`. Its columns are the
///   same content read top to bottom and then left to right, which is the one
///   thing a flow whose `y` never goes backwards cannot express -- so the
///   container is one item and its `N` columns are inside it.
///
/// They are separate [`ItemKind`] variants over one payload rather than one
/// variant, because the fragmenter has a different sentence to say about each
/// when it is taller than a page.
///
/// Its items are in **band-local** coordinates and are not in [`Flow::items`],
/// for [`FloatRecord`]'s reason one milestone earlier: the page cutter walks a
/// single column whose `y` never goes backwards, and the cells of a row sit
/// beside each other rather than under each other. Keeping them here is what
/// lets a two-column row be one item.
#[derive(Clone, Debug)]
pub(crate) struct Abreast {
    /// The cells' items, at the band's own origin.
    pub items: Vec<Item>,
    /// The row and cell decorations, indexing [`Abreast::items`].
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
    Rows(Box<Abreast>),
    /// One flex line, whole, `css-flexbox-1` §9. Its items sit beside one
    /// another along the main axis, so the page cutter cannot order them and
    /// they are kept out of the column for [`Abreast`]'s reason.
    FlexLine(Box<Abreast>),
    /// One multi-column container, whole, `css-multicol-1`. Its columns are
    /// `N` slices of one flow placed side by side, which is the same shape
    /// again -- and being one item is what lets [`crate::fragment`] cut a
    /// container taller than a page at one height across every column of it.
    Columns(Box<Abreast>),
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
    /// Whether a box that does not fit the page it started on may be moved
    /// whole to the next one.
    ///
    /// True for a float, which is `css-break-3`'s rule. **False for an
    /// absolutely positioned box**, and that is the one difference between the
    /// two at pagination: pushing it is moving it, and where it is is the whole
    /// of what `position: absolute` said.
    pub pushable: bool,
    /// `z-index`, CSS 2.2 §9.9.1's painting order, with `auto` read as zero —
    /// §9.9.1 puts an `auto` positioned box in the same layer as a `z-index: 0`
    /// one, so the two are one number here rather than two.
    pub z: i32,
}

/// A whole book as one continuous column, before it is cut into pages.
#[derive(Clone, Debug, Default)]
pub(crate) struct Flow {
    pub items: Vec<Item>,
    pub blocks: Vec<BlockRecord>,
    /// The floats, in the order they were met — which is document order, and
    /// therefore the order their text has to be read back in.
    pub floats: Vec<FloatRecord>,
    /// The absolutely positioned boxes, §9.6, sorted by `z-index` and then by
    /// the order they were met — §9.9.1's painting order, and a **stable** sort
    /// for the reason `Page::runs` is stably sorted: two boxes in one layer are
    /// painted in document order.
    pub positioned: Vec<FloatRecord>,
    /// The `fixed` boxes, §9.6.1, which are drawn on **every** page.
    pub fixed: Vec<FloatRecord>,
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
    /// The page box, which is `position: fixed`'s containing block — CSS 2.2
    /// §9.6.1's *"fixed with respect to the page box"*.
    page: (f64, f64),
    /// Whether the very next box [`Builder::block`] lays out has **already**
    /// been taken out of flow.
    ///
    /// A third one-shot slot beside [`Builder::cell`] and [`Builder::flex_pass`]
    /// and for their reason: [`Builder::positioned_box`] lays the box out by
    /// asking `block` for it, and `block` is where the out-of-flow branch is,
    /// so without this the box is taken out of flow for ever. It is `take`n
    /// rather than read, so it applies to exactly one box — a `position:
    /// absolute` figure **inside** a `position: absolute` sidebar is still
    /// taken out of the sidebar's flow, which is §9.6's own rule.
    placed: bool,
    /// §9.6's containing blocks: the innermost positioned ancestor, or the
    /// initial containing block where there is none.
    ///
    /// A stack rather than one slot, because *nearest* is the whole of §9.6's
    /// rule: an absolutely positioned figure inside a relatively positioned
    /// chapter inside a relatively positioned book belongs to the chapter.
    positioned: Vec<crate::position::Containing>,
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
    /// What the flex driver has decided about the very next box
    /// [`Builder::block`] lays out, `css-flexbox-1` §9.
    ///
    /// A second one-shot slot beside [`Builder::cell`] rather than a field on
    /// it, because the two impose **different** things and a shared one would
    /// have to carry a flag saying which: a table cell's margins do not apply
    /// (§17.5.3) and a flex item's do, and a flex item is blockified (§4) and a
    /// cell is not. A build that merged them would zero a flex item's margins,
    /// which is invisible on every fixture written without one.
    flex_pass: Option<FlexPass>,
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
    /// The last bidi paragraph number handed out ([`TextRun::paragraph`]):
    /// one per inline formatting context's text up to a paragraph separator
    /// (a forced break of `Bidi_Class` `B`, [`separates_paragraphs`]), counted
    /// across the whole layout as `sequence` is, so a paragraph's lines are
    /// one number wherever they land.
    paragraphs: usize,
    /// An `inside` list marker waiting for the first line of its list item,
    /// CSS 2.2 §12.5.1 and `css-lists-3` §3.2.
    ///
    /// **It is the list item's first inline box**, so where it lands depends on
    /// what the item's content turns out to be, which is not known when the item
    /// is opened: an item that starts with text sets the marker at the head of
    /// that text's first line, and one that starts with a block sets it on an
    /// anonymous line of its own above the block — §9.2.1.1, the marker being
    /// inline content beside a block sibling. So it is armed by
    /// [`Builder::block`] and `take`n by whichever of those happens first, and
    /// a sub-flow — a float's, a column's — saves and clears it, because a float
    /// that leads a list item is not where its marker goes.
    inside_marker: Option<Piece>,
    /// The explicit bidi levels the inline boxes being gathered open, outermost
    /// first: what each [`Piece`] — and so each [`TextRun`] — carries for the
    /// painter's UAX #9. Bounded by UAX #9's own depth, past which X1 ignores
    /// an embedding anyway ([`MAX_EMBEDDING_DEPTH`]).
    embeddings: EmbeddingStack,
    /// The float contexts of the formatting contexts a scroll container
    /// interrupted, innermost last: CSS 2.2 §9.4.1 makes one a block
    /// formatting context of its own, so its children are placed against a
    /// fresh [`FloatContext`] and the outer one is put back when it closes.
    ///
    /// A stack on the builder rather than a local in [`Builder::block`], for
    /// [`Builder::fill_height`]'s reason: a `FloatContext` in `block`'s frame
    /// is twenty-four bytes at every level of the depth cap.
    outer_floats: Vec<FloatContext>,
    /// How many boxes that clip are open, **across** sub-flows — a float's, a
    /// cell's, an `inline-block`'s — which [`Builder::subflow`] does not swap.
    ///
    /// [`Builder::note_overflow`] reads a box's whole subtree to decide whether
    /// it overflowed, so a clipping box inside a clipping box reads its subtree
    /// twice, and a nest of them reads the innermost content once per level.
    /// That multiplication is the work a book chooses, so the reads are charged
    /// to [`Budget::spend_layout`] — **the nested ones only**: the outermost
    /// clipping box's read is linear in what it contains, the same order as
    /// laying the content out at all, and charging it would be counting the
    /// book's text a second time.
    clipping: usize,
    /// A refusal [`Builder::note_overflow`] met, kept until the next box asks
    /// for its budget or the walk ends.
    ///
    /// **Deferred and not returned**, which is a stack measurement: a fallible
    /// call holds a `Result` in the caller's frame, `note_overflow`'s caller
    /// is [`Builder::block`], and a fourth one there overflowed
    /// `a_tree_of_blocks_past_the_depth_cap_is_refused_by_name`'s stack. The
    /// bound is no weaker for it: every box spends from the budget before it
    /// is laid out, so the walk stops at the very next box, having done at
    /// most the one subtree the charge was for.
    deferred: Option<Refusal>,
    /// How far into its own bottom padding a clipped box's kept content
    /// reaches, which [`Builder::clip_tail`] sets and [`Builder::block`]
    /// takes off the bottom edge it emits next, so the border box still ends
    /// at the padding box's used bottom.
    overhang: f64,
}

/// What [`Builder::horizontal`] decided about one block box.
struct Horizontal {
    /// Border-box left edge, never left of the page.
    left: f64,
    /// The used `width`, as a content-box width.
    content_width: f64,
    /// The picture's used size, for a replaced box.
    replaced: Option<(f64, f64)>,
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

/// What the flex driver imposes on one item box. See [`Builder::flex_pass`].
///
/// Two variants and not one struct with optional fields, because the two are
/// asked at different times for different reasons and share nothing: the
/// measuring trials want the item's box **taken off** so the number that comes
/// back is its content's, and the real pass wants exact sizes put **on**.
#[derive(Clone, Copy, Debug)]
enum FlexPass {
    /// §9.2's content-size trials. The item's own `width`, margins, padding and
    /// borders are stripped, so [`Builder::measure_content`] returns the
    /// content extent rather than the extent plus whichever of those happen to
    /// be on the left.
    Measure,
    /// The real layout. `width` is a **border-box** size and `height` is a
    /// **content** one, which is the split [`Builder::block`] already has:
    /// `box_sizing` decides the first and the second is compared against the
    /// content height directly.
    Used {
        /// The used border-box width, or `None` to leave the item's own.
        width: Option<f64>,
        /// The used content height, or `None` to leave the item's own.
        height: Option<f64>,
    },
}

impl FlexPass {
    /// Applies the decision to a consumed style, `css-flexbox-1` §3 and §4.
    ///
    /// # §3's `float` rule holds **structurally**, and there is no assignment
    ///
    /// §3 says in as many words that `float` and `clear` *"do not create
    /// floating or clearance for flex items"*, and the first draft of this
    /// function zeroed both here. The injection matrix deleted the assignment
    /// and **nothing failed**, which is the finding rather than a gap in the
    /// fixtures: a float is placed by [`Builder::children`] when a block
    /// container walks its children, and a flex item never goes through that
    /// function at all -- the driver hands each item straight to
    /// [`Builder::sublayout`], which establishes a formatting context with an
    /// empty float set. `clear` is inert for the same reason: there is nothing
    /// in that set to clear.
    ///
    /// So the two assignments were a rule enforced in a place it could not be
    /// reached from, which is milestone 11's finding in the other direction and
    /// is exactly what hides the reachable half. They are gone;
    /// `a_float_declaration_on_a_flex_item_does_nothing` stays and now asserts
    /// the behaviour rather than the assignment.
    ///
    /// # Blockification, and the one half of it that is observable
    ///
    /// §4 blockifies every item's `display`, and in this build only the
    /// `inline-flex` arm changes anything: [`Builder::block`] reads `display`
    /// to ask whether the box is `none`, a `list-item`, a table or a flex
    /// container, and an `inline` or `inline-block` item answers all four the
    /// same way a `block` one does. The `inline-flex` arm is what makes an
    /// inline-flex **item** a block-level flex container rather than an atomic
    /// inline in a line of its own, and
    /// `an_inline_flex_item_is_blockified_and_does_not_warn` is its fixture.
    /// The other two arms are kept because they are what §4 says, and recorded
    /// here as unobservable so a later reader does not go looking for the test.
    fn apply(&self, style: &mut Consumed) {
        style.display = match style.display {
            Display::Inline | Display::InlineBlock => Display::Block,
            Display::InlineFlex => Display::Flex,
            other => other,
        };
        match self {
            FlexPass::Measure => {
                style.width = Size::Auto;
                style.height = Size::Auto;
                style.margin = Sides::all(MarginValue::Length(LengthPercentage::ZERO));
                style.padding = Sides::all(LengthPercentage::ZERO);
                style.border_width = Sides::all(0.0);
            }
            FlexPass::Used { width, height } => {
                if let Some(width) = width {
                    style.width = Size::Length(LengthPercentage::Px(*width));
                    style.box_sizing = BoxSizing::BorderBox;
                }
                if let Some(height) = height {
                    style.height = Size::Length(LengthPercentage::Px(*height));
                }
            }
        }
    }
}

/// CSS 2.2 §10.8.1 leaves `super` to the user agent — *"the exact offset is
/// not defined"* — and this is the number this build picked, as a fraction of
/// the **parent's** font size, which is the box §10.8 says the offset is
/// proper for the superscripts of.
const SUPER_RISE: f64 = 1.0 / 3.0;

/// The same for `sub`, and it is not the same number: a descender has less
/// room under a baseline than an ascender has over it.
const SUB_DROP: f64 = 1.0 / 5.0;

/// `middle` needs an x-height and [`crate::Metrics`] does not carry one.
///
/// A face's `sxHeight` is an OS/2 field this crate's trait never asked for,
/// and adding it would change every implementor for one value. Half the font
/// size is the standing approximation, and it is written down **once** so that
/// a milestone which adds the metric has one line to change rather than a
/// search to do.
const X_HEIGHT: f64 = 0.5;

/// One flex item's box, which the document may not contain.
///
/// `css-flexbox-1` §4: *"each contiguous sequence of child text runs is wrapped
/// in an anonymous block container flex item"*. A container written as
/// `<div class="row">text<span>more</span></div>` therefore has **two** items
/// and the first is not an element -- and a build that skipped the anonymous
/// one would drop the text out of the flow entirely, which text conservation
/// would catch and nothing else would.
enum ItemBox<'a> {
    /// A child element.
    Element(&'a BoxNode),
    /// §4's anonymous block container around a run of text.
    ///
    /// Boxed because a `BoxNode` is four hundred bytes of computed style and
    /// an anonymous item is the rare case: an unboxed variant would make every
    /// entry in a container's item vector that size, including the elements.
    Anonymous(Box<BoxNode>),
}

impl ItemBox<'_> {
    fn node(&self) -> &BoxNode {
        match self {
            ItemBox::Element(node) => node,
            ItemBox::Anonymous(node) => node,
        }
    }
}

/// What the driver worked out about one item before any of it was positioned.
struct FlexItem {
    /// §9's inputs, [`flex::resolve`]'s and [`flex::lines`]'s.
    sizes: flex::Item,
    /// `order`, §5.4.
    order: i32,
    /// `align-self`, §8.3, already resolved against the container's
    /// `align-items`.
    align: AlignItems,
    /// Whether the item's cross size property is `auto`, which is §9.4 step
    /// 11's condition for stretching it: an item with a stated height is
    /// **not** stretched however the container is aligned.
    cross_auto: bool,
    /// The cross-axis margin, border and padding.
    cross_extra: f64,
    /// The cross-axis margins alone, which a stretched item's border box has
    /// to give back.
    cross_margins: f64,
    /// The **cross-start** margin on its own.
    ///
    /// Four edge figures and not two, because §8 positions a *margin* box and
    /// a background paints a *border* box, and the two differ by exactly this
    /// on each axis. A build carrying only the totals paints every item that
    /// has an asymmetric margin in the wrong place, which is invisible on every
    /// fixture written with `margin: 0`.
    cross_lead: f64,
    /// The main-axis border and padding, which turns a used content size into
    /// the border-box size [`FlexPass::Used`] takes.
    main_inset: f64,
    /// The main-axis margins.
    main_margins: f64,
    /// The main-start margin on its own.
    main_lead: f64,
}

/// One run of text in an inline formatting context, after phase I.
struct Piece {
    text: String,
    style: Consumed,
    /// The box this piece **is**, where it is not text at all.
    ///
    /// CSS 2.2 §9.2.2's atomic inline-level box: `display: inline-block`. Its
    /// `text` is one U+FFFC OBJECT REPLACEMENT CHARACTER, which is what gives
    /// it a position in the string the line breaker works over and a width the
    /// measure can be asked for — and which never becomes a glyph, so text
    /// conservation never sees it.
    atomic: Option<Atomic>,
    /// The [`BoxNode::anchor`] of the node this piece's text came from.
    anchor: Option<u32>,
    /// Its position in document order. See [`Builder::sequence`].
    order: usize,
    /// Text the source does not contain: an `inside` list marker. Carried to
    /// [`TextRun::generated`], which is what keeps it out of text conservation
    /// and makes the painter mark it an artifact.
    generated: bool,
    /// The bidi levels its inline ancestors open, shared with every piece
    /// and run made under the same ones ([`EmbeddingStack::shared`]). See
    /// [`TextRun::embeddings`].
    embeddings: Arc<[Embedding]>,
}

/// [`Builder::embeddings`]: the levels the inline boxes being gathered open,
/// and the same stack as the pieces carry it.
///
/// **Shared, not copied.** Every piece, and every run cut from it, carries
/// the whole stack, up to [`MAX_EMBEDDING_DEPTH`] levels of twelve bytes —
/// and a copy each was a kilobyte and a half on every line of a paragraph
/// inside 125 nested isolating spans: a 400 KB paragraph of one-word lines
/// peaked at 855 MB against 259 MB without the spans (review of lane 8C).
/// A stack is made into an [`Arc`] once, when a piece first asks for it, and
/// every piece and run made under the same boxes holds that one. One is kept
/// per level, so closing a box goes back to the stack its parent's pieces
/// already share: a stack is made at most once for each box that opens a
/// level, and once for the context's own.
struct EmbeddingStack {
    open: Vec<Embedding>,
    /// `shared[i]` is `open[..i]` as a piece carries it, once one has asked:
    /// always one longer than `open`.
    shared: Vec<Option<Arc<[Embedding]>>>,
}

impl Default for EmbeddingStack {
    fn default() -> Self {
        Self {
            open: Vec::new(),
            shared: vec![None],
        }
    }
}

impl EmbeddingStack {
    fn len(&self) -> usize {
        self.open.len()
    }

    fn push(&mut self, embedding: Embedding) {
        self.open.push(embedding);
        self.shared.push(None);
    }

    fn pop(&mut self) {
        if self.open.pop().is_some() {
            self.shared.pop();
        }
    }

    /// The open levels as a piece carries them: the one already made for
    /// this level, or a new one kept for the next piece.
    fn shared(&mut self) -> Arc<[Embedding]> {
        let open = &self.open;
        match self.shared.get_mut(open.len()) {
            Some(slot) => Arc::clone(slot.get_or_insert_with(|| Arc::from(open.as_slice()))),
            // Unreachable while `push` and `pop` keep the two in step; a new
            // stack is still the right answer, only not a shared one.
            None => Arc::from(open.as_slice()),
        }
    }
}

/// An atomic inline-level box, CSS 2.2 §9.2.2.
///
/// **Its own formatting context, placed on a line.** An `inline-block` is laid
/// out once, at its own shrink-to-fit width, and then set on the line as a
/// single unit that cannot be broken — which is the whole of what "atomic"
/// means and the whole of the difference from what this build did before, which
/// was to pour its text into the line and lose its width, its height and its
/// vertical margins.
#[derive(Clone, Debug)]
pub(crate) struct Atomic {
    /// Its own flow, at `x = 0` and `y = 0` until the line places it.
    pub items: Vec<Item>,
    /// Its own block records, indexing those items.
    pub blocks: Vec<BlockRecord>,
    /// The **margin-box** width, which is what it takes on the line.
    pub width: f64,
    /// The margin-box height.
    pub height: f64,
    /// From the margin-box top to the baseline it sits on.
    ///
    /// §10.8.1: an inline-block's baseline is *"the baseline of its last line
    /// box"*, and its bottom margin edge where it has none — a box holding one
    /// picture and no text sits on the line rather than hanging from it.
    pub baseline: f64,
}

/// One atomic inline box, placed on a line.
#[derive(Clone, Debug)]
pub(crate) struct InlineBox {
    /// Its own flow, already moved to its `x` on the line.
    pub items: Vec<Item>,
    /// Its records, likewise.
    pub blocks: Vec<BlockRecord>,
    /// From the line's baseline to the box's **top**.
    pub dy: f64,
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
        page: (options.width, options.height),
        placed: false,
        // §10.1's initial containing block is the page box. Everything with no
        // positioned ancestor is placed against this, which is what makes a
        // `position: absolute` box in a book with no `position: relative`
        // anywhere in it land where the stylesheet meant.
        positioned: vec![crate::position::Containing {
            left: 0.0,
            top: 0.0,
            width: options.width,
            height: Some(options.height),
        }],
        cell: None,
        flex_pass: None,
        sequence: 0,
        paragraphs: 0,
        inside_marker: None,
        embeddings: EmbeddingStack::default(),
        outer_floats: Vec::new(),
        clipping: 0,
        deferred: None,
        overhang: 0.0,
    };
    builder.block(root, options.width, 0.0, 0, false, 0)?;
    if let Some(refusal) = builder.deferred.take() {
        return Err(refusal);
    }
    // The last pending margin is committed so the flow's height includes it,
    // which matters for a book whose last block has a bottom margin: without
    // it the final page is short by that margin and the page count can differ.
    builder.commit_margin();
    let mut warnings: Vec<(Warning, usize)> = builder.warnings.into_iter().collect();
    // Deterministic order, ruling 4: a `HashMap`'s iteration order is not, and
    // a warning list that changed between runs would change a report.
    warnings.sort_by(|a, b| format!("{:?}", a.0).cmp(&format!("{:?}", b.0)));
    let mut flow = builder.flow;
    // §9.9.1's painting order, and a **stable** sort: two positioned boxes in
    // one layer are painted in the order the document generated them, which is
    // the order this vector already holds. `z-index` decides the layer and
    // nothing else decides anything.
    flow.positioned.sort_by_key(|record| record.z);
    flow.fixed.sort_by_key(|record| record.z);
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
        if let Some(pass) = self.flex_pass.take() {
            pass.apply(&mut style);
        }
        let style = style;
        // §9.6: `absolute` and `fixed` are **out of flow**, so the box model
        // below is not this box's -- it never becomes part of the column at
        // all. The branch is here rather than lower for a float's own reason,
        // one screen up in `gather`: everything after this line writes into the
        // flow, and an out-of-flow box must not.
        if matches!(style.position, Position::Absolute | Position::Fixed)
            && !std::mem::take(&mut self.placed)
        {
            return self.positioned_box(node, &style, x, depth, avoid);
        }
        self.spend_box()?;
        let avoid = avoid || style.page_break_inside == PageBreakInside::Avoid;

        let margin_top = style.margin_px(Side::Top, containing);
        let margin_bottom = style.margin_px(Side::Bottom, containing);
        let padding = Sides {
            top: style.padding_px(Side::Top, containing),
            right: style.padding_px(Side::Right, containing),
            bottom: style.padding_px(Side::Bottom, containing),
            left: style.padding_px(Side::Left, containing),
        };
        let border = style.border_width;
        // `css-box-3` §4's `box-sizing` and the whole of §10.3's horizontal
        // half. See [`Builder::horizontal`].
        let extra = padding.left + padding.right + border.left + border.right;
        let Horizontal {
            left,
            content_width,
            replaced,
        } = self.horizontal(node, &style, containing, x, extra);
        let border_box_width = content_width + extra;
        // `css-overflow-3` §3.1. The axes it clips, and whether it is a scroll
        // container and so a formatting context of its own (CSS 2.2 §9.4.1).
        let clip = Clip::of(&style, replaced.is_some());
        let contained = clip.any() && style.is_scroll_container();

        let painted = style.background_color.a != 0
            || border.top > 0.0
            || border.right > 0.0
            || border.bottom > 0.0
            || border.left > 0.0
            || draws_beyond_its_border(&style);
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
            // Filled in below, once `content_x` and the used height exist.
            replaced: None,
            dy: 0.0,
            anchor: node.anchor,
            paint: style.paint.clone(),
            clip: Clip::NONE,
        };
        let block = self.flow.blocks.len();
        self.flow.blocks.push(record);

        // §9.5.2's clearance, which goes **between** the margins already
        // adjoining here and this box's own top margin — so it is introduced
        // before the top margin joins them, and introducing it is what stops
        // the two from collapsing through each other.
        self.clear(&style, margin_top, contained, left, border_box_width)?;

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
        // §9.6: a box with a `position` other than `static` is a containing
        // block for its absolutely positioned descendants. Its **padding box**
        // and not its content box, which §10.1 says in as many words and which
        // a build reading `content_x` here would get wrong by the padding. So
        // is a transformed box (`css-transforms-1` §2), whose descendants turn
        // with it.
        let anchors = style.position != Position::Static || style.transformed();
        if anchors {
            self.positioned.push(crate::position::Containing {
                left: left + border.left,
                top: self.cursor() + border.top,
                width: (border_box_width - border.left - border.right).max(0.0),
                height: None,
            });
        }
        let floats_before = self.flow.floats.len();
        let top_edge = border.top + padding.top;
        // **And so does a formatting context of its own**, with nothing
        // between the two at all: §8.3.1 says *"margins of elements that
        // establish new block formatting contexts ... do not collapse with
        // their in-flow children"*. An edge of no height is what opens the
        // box's border box here, so the first child's margin is committed
        // **inside** it rather than beside it.
        if top_edge > 0.0 || contained {
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
        self.open_clip(clip, contained);
        self.arm_marker(node, &style, ordinal);
        if let Some(size) = replaced {
            self.replaced_content(
                block,
                node,
                &style,
                (border.left + padding.left, top_edge),
                size,
            );
        } else if style.is_table() {
            // CSS 2.2 §17. Everything above this line -- the margins, the
            // border, the padding, `width`, `box-sizing`, the page-break
            // properties -- is the ordinary block box a table also is, and
            // reusing it is what stops a table from being a second, quietly
            // different, box model.
            self.table(node, &style, content_x, content_width, depth, avoid, block)?;
        } else if style.is_flex() {
            // `css-flexbox-1` §9, and the same sentence as the table above it:
            // a flex container is an ordinary block box on the outside. An
            // `inline-flex` arrives here only as an atomic inline's inside
            // ([`Builder::atomic_inline`]) or blockified — floated, positioned,
            // the root, a flex item — and in every one of those a flex
            // container is what it is.
            self.flex(node, &style, content_x, content_width, depth, avoid)?;
        } else if style.is_multicol() {
            // `css-multicol-1`, and the same sentence a third time: a
            // multi-column container is an ordinary block box on the outside,
            // and everything above this line is that box.
            self.columns(node, &style, content_x, content_width, depth, avoid)?;
        } else {
            self.children(
                node,
                None,
                &style,
                content_x,
                content_width,
                depth,
                avoid,
                block,
            )?;
        }
        self.disarm_marker();
        self.content_top = outer_top;
        if contained {
            self.leave_context(&style);
        }
        let content_height = self.y - before;

        // §10.6.3's height and §10.7's clamp. See [`Builder::fill_height`].
        self.fill_height(
            &style,
            content_height,
            block,
            floats_before,
            clip,
            padding.bottom,
        );

        let bottom_edge = border.bottom + padding.bottom - std::mem::take(&mut self.overhang);
        if bottom_edge > 0.0 {
            self.commit_margin();
            self.emit(bottom_edge, ItemKind::Edge, true);
        }
        self.open.pop();
        if anchors {
            self.positioned.pop();
        }
        if style.page_break_inside == PageBreakInside::Avoid {
            self.open_avoid.pop();
        }
        self.offset_relative(&style, block, floats_before, content_x, before, containing);
        if clip.any() {
            self.note_overflow(block, clip, floats_before, &border);
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
        // An `inside` one went into that line as its first inline box instead.
        if style.display == Display::ListItem
            && style.list_style_position == ListStylePosition::Outside
        {
            self.marker(node, &style, block, (content_x, content_width), ordinal);
        }
        Ok(())
    }

    /// A block box's used width and left edge: CSS 2.2 §10.3.3, §10.3.4 and
    /// §10.4, with `css-box-3`'s `box-sizing`, and the picture's size where
    /// the box is replaced.
    ///
    /// **A method and not sixty lines inside [`Builder::block`]**, and the
    /// reason is [`Builder::offset_relative`]'s: `block` recurses once per
    /// level of the document and its frame is what the depth cap is measured
    /// in stack against. These locals — two margins, a closure, a stated, a
    /// tentative and a used width, the picture — were `block`'s own until the
    /// overflow milestone needed room in that frame, and
    /// `a_tree_of_blocks_past_the_depth_cap_is_refused_by_name` overflowed
    /// until they moved here.
    #[inline(never)]
    fn horizontal(
        &mut self,
        node: &BoxNode,
        style: &Consumed,
        containing: f64,
        x: f64,
        extra: f64,
    ) -> Horizontal {
        let margin_left = style.margin_px(Side::Left, containing);
        let margin_right = style.margin_px(Side::Right, containing);
        // `css-box-3` §4: `content-box` measures `width` as the content, and
        // `border-box` measures it as content plus padding plus border. The
        // difference is invisible on a box with neither, which is why a fixture
        // for it must have both.
        //
        // **`css-ui-3` §5.1 puts `min-width` and `max-width` inside the same
        // sentence**: `border-box` measures *"the width and height ... and the
        // respective min/max properties"* from the border box, so the
        // conversion is one closure over all three rather than a special case
        // for `width`. A build that converted `width` and not `max-width` gets
        // every `box-sizing: border-box; max-width: 40em` figure wrong by the
        // padding and the page looks entirely reasonable.
        let to_content = |specified: f64| {
            match style.box_sizing {
                tinker_pdf_css::property::BoxSizing::ContentBox => specified,
                tinker_pdf_css::property::BoxSizing::BorderBox => specified - extra,
            }
            .max(0.0)
        };
        let auto_width = (containing - margin_left - margin_right - extra).max(0.0);
        let stated_width = match style.width {
            Size::Auto => None,
            Size::Length(length) => Some(to_content(resolve_length(length, containing))),
        };
        // CSS 2.2 §3.1's replaced element, sized here rather than by everything
        // below it. §10.3.4 says so in one sentence — *"the used value of
        // `width` is determined as for inline replaced elements"* — and
        // §10.6.2's title lists block-level replaced boxes beside inline ones,
        // so one call answers both axes for every `display` a picture can have.
        //
        // **Before `is_table`, `is_flex` and `is_multicol` below**, which is
        // `css-display-3` §2.2: *"replaced elements ... ignore the inner
        // display type"*. `img { display: flex }` is a block-level picture and
        // not an empty flex container, and a build that asked `is_flex` first
        // would produce the second.
        let replaced = replaced_box(node, style, containing);
        // §10.4: the tentative used width comes from §10.3, and then the whole
        // of §10.3 is *"applied again"* with `max-width` as the width, and
        // again with `min-width`. `style::clamp_size` is that order, which is
        // not `f64::clamp`: a `min-width` larger than the `max-width` wins.
        let tentative = stated_width.unwrap_or(auto_width);
        let content_width = match replaced {
            Some((width, _)) => width,
            None => crate::style::clamp_size(
                tentative,
                crate::style::min_length(style.min_width, Some(containing)).map(to_content),
                crate::style::max_length(style.max_width, Some(containing)).map(to_content),
            ),
        };
        // §10.3.3: with a used width that is not `auto`, two `auto` margins
        // centre the box and the leftover is otherwise put on the right. An
        // `auto` width that §10.4's clamp has narrowed reaches this too, and
        // that is §10.4's own instruction rather than an extra rule: the second
        // pass runs *"as if `width` were the clamped value"*, and by then it is
        // not `auto`.
        let both_auto =
            style.margin.left == MarginValue::Auto && style.margin.right == MarginValue::Auto;
        // §10.3.4 sends a block-level replaced box through §10.3.3's margin
        // rules with the width §10.3.2 gave it, which is never `auto` — so two
        // `auto` margins centre a picture exactly as they centre a `<div>` with
        // a stated width.
        let definite = replaced.is_some() || stated_width.is_some() || content_width < tentative;
        let left = if both_auto && definite {
            x + ((containing - (content_width + extra)) / 2.0).max(0.0)
        } else {
            x + margin_left
        };
        if content_width + extra > containing + 0.001 {
            self.warn(Warning::ContentOverflowedPage);
        }
        Horizontal {
            left: left.max(0.0),
            content_width,
            replaced,
        }
    }

    /// §10.6.3's `height`, §10.7's clamp, and the padding that makes the flow
    /// as tall as the box said it was.
    ///
    /// **A method and not fifteen lines inside [`Builder::block`]**, and the
    /// reason is [`Builder::offset_relative`]'s below, word for word: `block`
    /// recurses once per level of the document and its frame is what the depth
    /// cap is measured in stack against. The four locals this holds went back
    /// into `block` for one edit and
    /// `a_tree_of_blocks_past_the_depth_cap_is_refused_by_name` overflowed the
    /// stack again.
    ///
    /// A specified height is honoured by padding the flow out to it; a content
    /// taller than the height overflows, which CSS 2.2 §10.6.3's `overflow:
    /// visible` initial value asks for.
    ///
    /// §10.7 then clamps that tentative height, and the two halves of the clamp
    /// are not equally implementable here. `min-height` is padding and is
    /// exactly what `height` already does. `max-height` can only make a box
    /// **shorter**, and this module has already emitted the items its content
    /// came to — the flow is one column whose `y` never goes backwards, so
    /// there is no negative edge to emit. So it clamps the padding, which is
    /// the whole of its effect on a box whose content fits, and says
    /// `MaxHeightAsAuto` by name on the box whose content does not. A build
    /// that stayed silent would draw a `max-height: 4em` figure at whatever
    /// height its caption came to and nothing anywhere would say so.
    ///
    /// The percentages resolve against `None` for §10.5's reason: this box's
    /// containing block has an `auto` height at this point in the pass, so a
    /// percentage `min-height` or `max-height` behaves as `auto` and `none`.
    ///
    /// **A box that clips its block axis is the exception, and makes both
    /// halves implementable.** `css-overflow-3` §3.1 clips its content to its
    /// padding box, so the content past the used height is not drawn and the
    /// box that follows is placed after the used height and not after the
    /// content: [`Builder::clip_tail`] takes it back out of the flow, and a
    /// `max-height: 4em; overflow: hidden` box is exactly four ems tall.
    fn fill_height(
        &mut self,
        style: &Consumed,
        content_height: f64,
        block: usize,
        floats_before: usize,
        clip: Clip,
        padding_bottom: f64,
    ) {
        let stated_height = definite_height(style);
        let min_height = crate::style::min_length(style.min_height, None);
        let max_height = crate::style::max_length(style.max_height, None);
        let wanted = crate::style::clamp_size(
            stated_height.unwrap_or(content_height),
            min_height,
            max_height,
        );
        if wanted > content_height {
            self.commit_margin();
            self.emit(wanted - content_height, ItemKind::Edge, true);
        } else if clip.y && content_height > wanted + EPSILON {
            let cut = self.y - content_height + wanted;
            self.clip_tail(block, floats_before, (cut, cut + padding_bottom), clip);
        } else if max_height.is_some_and(|max| content_height > max + EPSILON) {
            self.warn(Warning::MaxHeightAsAuto);
        }
    }

    /// A box that clips, opened: a scroll container's fresh float context
    /// (see [`Builder::outer_floats`]) and the count of open clipping boxes
    /// (see [`Builder::clipping`]).
    ///
    /// Never inlined, for [`Builder::disarm_marker`]'s reason: the context it
    /// moves is a value in its own frame rather than in `block`'s.
    #[inline(never)]
    fn open_clip(&mut self, clip: Clip, contained: bool) {
        if contained {
            let outer = std::mem::take(&mut self.floats);
            self.outer_floats.push(outer);
        }
        if clip.any() {
            self.clipping += 1;
        }
    }

    /// A scroll container's own block formatting context, closed: CSS 2.2
    /// §9.4.1's three consequences that are not the clip.
    ///
    /// 1. The last child's bottom margin is committed **inside** the box, which
    ///    is §8.3.1's *"do not collapse with their in-flow children"* at the
    ///    bottom as the zero-height edge [`Builder::block`] emits is at the
    ///    top.
    /// 2. §10.6.7: with an `auto` height, *"if the element has any floating
    ///    descendants whose bottom margin edge is below the element's bottom
    ///    content edge, then the height is increased to include those
    ///    edges"* — which is what `overflow: hidden` round a floated picture is
    ///    written for, and the reason a book writes it.
    /// 3. The float context the box interrupted is put back, so the floats it
    ///    placed are nobody else's to flow round.
    #[inline(never)]
    fn leave_context(&mut self, style: &Consumed) {
        self.commit_margin();
        let inner = std::mem::replace(
            &mut self.floats,
            self.outer_floats.pop().unwrap_or_default(),
        );
        if definite_height(style).is_some() {
            return;
        }
        if let Some(bottom) = inner.clearance_bottom(Clear::Both) {
            if bottom > self.y + EPSILON {
                self.emit(bottom - self.y, ItemKind::Edge, true);
            }
        }
    }

    /// CSS 2.2 §9.5: *"The border box of ... an element in the normal flow that
    /// establishes a new block formatting context (such as an element with
    /// `overflow` other than `visible`) must not overlap the margin box of any
    /// floats in the same block formatting context"*, and *"if necessary,
    /// implementations should clear the said element by placing it below any
    /// preceding floats"*.
    ///
    /// **The *should*, and not the *may* that follows it.** §9.5 also permits
    /// placing the box beside the floats, narrowed, *"if there is sufficient
    /// space"*, and leaves *sufficient* undefined; a browser narrows. This
    /// build clears, which is the sentence's own first answer: the box keeps
    /// the width its containing block gives it and starts below the floats it
    /// would have overlapped. A float that only begins below the box's top
    /// edge is not looked for, since this box's height is not known yet.
    #[inline(never)]
    fn clear_beside_floats(
        &mut self,
        left: f64,
        width: f64,
        margin_top: f64,
    ) -> Result<(), Refusal> {
        if self.floats.is_empty() {
            return Ok(());
        }
        self.budget.spend_layout(self.floats.len())?;
        let top = self.cursor() + margin_top;
        let (lo, hi) = self.floats.band(top, top + 1.0, left, left + width);
        if lo <= left + EPSILON && hi >= left + width - EPSILON {
            return Ok(());
        }
        let Some(bottom) = self.floats.clearance_bottom(Clear::Both) else {
            return Ok(());
        };
        let clearance = bottom - top;
        if clearance <= 0.0 {
            return Ok(());
        }
        let inside = self.inside_open();
        self.commit_margin();
        self.emit(clearance, ItemKind::Edge, inside);
        Ok(())
    }

    /// **The content past a clipping box's used height, taken back out of the
    /// flow**, `css-overflow-3` §3.1.
    ///
    /// The flow is one column whose `y` never goes backwards, so a box shorter
    /// than its content cannot be drawn with the content running on under the
    /// next box: everything is placed in order. What a block-axis clip makes
    /// possible is to take out what the clip would hide. `edges` is the
    /// content box's bottom edge and the padding box's: every item that begins
    /// at or below the second leaves the column, the one that straddles it is
    /// kept and shortened to end there (its ink is cut at the same edge by the
    /// page's clip, not here), and the cursor goes back to wherever the kept
    /// content ends — the content edge at the least. What the kept content
    /// reaches into the bottom padding is [`Builder::overhang`], taken off the
    /// bottom edge, so the box's bottom padding, border and margin, and the
    /// next box, follow the used height whatever was kept.
    ///
    /// **Out of the column, and not out of the book.** The items that left are
    /// kept as an out-of-flow record of no height at the padding edge, with
    /// every run in them **laid out and not painted** — CSS 2.2 §11.2's
    /// `visibility: hidden`, which is what a clip that hides all of a run makes
    /// it — so the text is still the layout's, in its reading order, and text
    /// conservation stays an equality rather than learning an exception. The
    /// floats this box's content placed below the padding edge are hidden the
    /// same way where they stand, and the margins still adjoining, which
    /// belonged to the last child, are dropped: a margin is not content.
    ///
    /// Its work is charged by [`Builder::note_overflow`], which runs over the
    /// same subtree once the box is closed and is the one fallible call of the
    /// two: `block`'s frame holds one `Result` for both.
    #[inline(never)]
    fn clip_tail(&mut self, block: usize, floats_before: usize, edges: (f64, f64), clip: Clip) {
        let (cut, line) = edges;
        let Some(first) = self.flow.blocks[block].first else {
            return;
        };
        // The box's own first item is always kept: it is its top edge, or
        // the zero-height one a scroll container opens with, or its first
        // line, and a box with no first item has no border box to draw.
        let from = (first + 1).min(self.flow.items.len());
        let keep = from
            + self.flow.items[from..]
                .iter()
                .position(|item| item.y >= line - EPSILON)
                .unwrap_or(self.flow.items.len() - from);
        let mut tail = self.flow.items.split_off(keep);
        if !tail.is_empty() {
            hide(&mut tail, &mut [], Some(line));
            // The current flow's own list of records beside the column, so a
            // sub-flow — a float's, a cell's, a measuring trial's — carries
            // its hidden tail with it and translates it where it goes.
            self.flow.floats.push(FloatRecord {
                items: tail,
                blocks: Vec::new(),
                top: line,
                bottom: line,
                pushable: false,
                z: 0,
            });
        }
        let mut end = cut;
        if let Some(last) = self.flow.items.last_mut() {
            if last.y + last.height > line {
                last.height = (line - last.y).max(0.0);
            }
            end = last.y + last.height;
        }
        for record in &mut self.flow.blocks[block..] {
            match record.first {
                Some(head) if head >= keep => {
                    record.first = None;
                    record.last = 0;
                }
                _ => record.last = record.last.min(keep),
            }
        }
        for open in &self.open {
            let record = &mut self.flow.blocks[*open];
            record.last = record.last.min(keep);
        }
        for float in &mut self.flow.floats[floats_before..] {
            if float.top >= line - EPSILON {
                hide(&mut float.items, &mut float.blocks, Some(line));
                float.top = line;
                float.bottom = line;
            }
        }
        self.pending = Pending::default();
        self.ceiling_box = self.ceiling_box.min(line);
        self.ceiling_line = self.ceiling_line.min(line);
        self.y = end.min(line);
        if self.y < cut {
            self.emit(cut - self.y, ItemKind::Edge, true);
        }
        self.overhang = (self.y - cut).max(0.0);
        self.flow.blocks[block].clip = clip;
    }

    /// Whether a clipping box's content reached past its padding box, and so
    /// whether its clip removes anything at all.
    ///
    /// Horizontally: every run, every atomic inline, every descendant box and
    /// every float its content placed, against the padding box's two sides —
    /// a word longer than the measure, a table wider than its container, a
    /// list marker hung outside it. Vertically: the floats, which §10.6.7
    /// contains only under an `auto` height; the in-flow content past a
    /// stated one is [`Builder::clip_tail`]'s, which marks the box itself.
    ///
    /// **Extents, not ink**, which is `css-overflow-3` §2.2's scrollable
    /// overflow rather than §2.1's ink overflow: an italic's overhang past
    /// the last advance is not measured, so a box whose text fits exactly
    /// writes no clip and the overhang is drawn.
    ///
    /// It also closes the count [`Builder::open_clip`] opened, and charges the
    /// subtree's size where the box is nested in another clipping box —
    /// for this read and for [`Builder::clip_tail`]'s, which covered the same
    /// items.
    #[inline(never)]
    fn note_overflow(
        &mut self,
        block: usize,
        clip: Clip,
        floats_before: usize,
        border: &Sides<f64>,
    ) {
        self.clipping = self.clipping.saturating_sub(1);
        let record = &self.flow.blocks[block];
        let first = record.first.unwrap_or(self.flow.items.len());
        let last = record.last.min(self.flow.items.len());
        if self.clipping > 0 {
            let spent = self.budget.spend_layout(
                last.saturating_sub(first)
                    + (self.flow.blocks.len() - block)
                    + (self.flow.floats.len() - floats_before),
            );
            if let Err(refusal) = spent {
                self.deferred.get_or_insert(refusal);
                return;
            }
        }
        let record = &self.flow.blocks[block];
        if record.clip.any() || first >= last {
            return;
        }
        let left = record.x + border.left - EPSILON;
        let right = record.x + record.width - border.right + EPSILON;
        let tail = &self.flow.items[last - 1];
        let bottom = tail.y + tail.height - border.bottom + EPSILON;
        let mut reach = Reach::default();
        reach.items(&self.flow.items[first..last]);
        for descendant in &self.flow.blocks[block + 1..] {
            if descendant.first.is_some() {
                reach.span(descendant.x, descendant.x + descendant.width);
            }
        }
        let mut below = false;
        for float in &self.flow.floats[floats_before..] {
            reach.items(&float.items);
            for record in &float.blocks {
                reach.span(record.x, record.x + record.width);
            }
            below |= float.bottom > bottom;
        }
        let across = reach.lo < left || reach.hi > right;
        if (clip.x && across) || (clip.y && below) {
            self.flow.blocks[block].clip = clip;
        }
    }

    /// [`Budget::spend_box`], after any refusal [`Builder::note_overflow`]
    /// deferred.
    fn spend_box(&mut self) -> Result<(), Refusal> {
        if let Some(refusal) = self.deferred.take() {
            return Err(refusal);
        }
        self.budget.spend_box()
    }

    /// A replaced box's one flow item and the picture recorded against it.
    ///
    /// **A method and not fifteen lines inside [`Builder::block`]**, and the
    /// reason is [`Builder::offset_relative`]'s below, word for word: `block`
    /// recurses once per level of the document and its frame is what the depth
    /// cap is measured in stack against. Written inline, this took
    /// `a_tree_of_blocks_past_the_depth_cap_is_refused_by_name` from a named
    /// refusal to a stack overflow a second time. The fixture found it again;
    /// this is where the frame went.
    ///
    /// A replaced box has no children to lay out — CSS 2.2 §3.1 puts its
    /// content *"outside the scope of the CSS formatting model"* — so the flow
    /// gets one item of the used height and the picture is recorded against the
    /// box that item belongs to. An `ItemKind::Edge` rather than an item kind
    /// of its own, which is what makes a picture page-breakable,
    /// float-placeable and band-placeable for free: an edge is what a border
    /// and a padding already are, and nothing in [`crate::fragment`] has to
    /// learn a new shape.
    ///
    /// **`visibility` gates the picture and not the box.** CSS 2.2 §11.2 makes
    /// `visibility: hidden` a box that is laid out and not painted, which is
    /// exactly what `painted` does for a background, and a picture is ink like
    /// any other.
    fn replaced_content(
        &mut self,
        block: usize,
        node: &BoxNode,
        style: &Consumed,
        inset: (f64, f64),
        size: (f64, f64),
    ) {
        if size.1 > 0.0 {
            self.commit_margin();
            self.emit(size.1, ItemKind::Edge, true);
        }
        self.flow.blocks[block].replaced = style.visible.then_some(ReplacedPaint {
            left: inset.0,
            top: inset.1,
            width: size.0,
            height: size.1,
            anchor: node.anchor,
        });
    }

    /// CSS 2.2 §9.4.3's offset, applied once the box is closed.
    ///
    /// **A method and not five lines inside [`Builder::block`]**, which is not
    /// a style choice: `block` recurses once per level of the document and its
    /// frame is what the depth cap is measured in stack against. Five more
    /// locals in it took `a_tree_of_blocks_past_the_depth_cap_is_refused_by_name`
    /// from a named refusal to a stack overflow — the cap counted the same
    /// depth and the frames no longer fitted. The fixture found it; this is
    /// where the frame went.
    #[allow(clippy::too_many_arguments)]
    fn offset_relative(
        &mut self,
        style: &Consumed,
        block: usize,
        floats_before: usize,
        content_x: f64,
        before: f64,
        containing: f64,
    ) {
        // §9.4.3, and `css-position-3` §3.4 for the second value: `sticky` is
        // offset by how far its nearest scrollport has scrolled, and a
        // paginated document has no scrollport, so §3.4's own answer is that it
        // *"is the same as `relative`"*. Not a degradation -- the value of a
        // parameter this medium does not have.
        //
        // **The offset is applied to the ink and not to the flow.** §9.4.3 says
        // it *"does not affect the layout of any other box"*, and here that is
        // load-bearing rather than merely true: this module is one column whose
        // `y` never goes backwards, and a box moved up by `top: -10px` would
        // put an item above the one before it.
        if !matches!(style.position, Position::Relative | Position::Sticky) {
            return;
        }
        {
            let against = crate::position::Containing {
                left: content_x,
                top: before,
                width: containing,
                height: None,
            };
            let (dx, dy) = crate::position::relative_offset(&style.inset, &against);
            if dx != 0.0 || dy != 0.0 {
                let range = self.flow.blocks[block]
                    .first
                    .map(|first| (first, self.flow.blocks[block].last));
                if let Some((first, last)) = range {
                    shift(&mut self.flow.items[first..last], dx, dy);
                }
                // Every record from this box's own onwards is this box or a
                // descendant of it: records are pushed in tree order and this
                // box's siblings do not exist yet.
                for record in &mut self.flow.blocks[block..] {
                    record.x += dx;
                    record.dy += dy;
                }
                // A float inside a relatively positioned box moves with it, and
                // the floats from this box's onwards are exactly the ones it
                // placed -- the same tree-order argument as the records.
                for float in &mut self.flow.floats[floats_before..] {
                    shift(&mut float.items, dx, dy);
                    for record in &mut float.blocks {
                        record.x += dx;
                        record.dy += dy;
                    }
                }
            }
        }
    }

    /// A block container's children: block-level ones recursed into, runs of
    /// inline-level ones wrapped in an anonymous block box.
    ///
    /// `run`, where it is given, stands in for `node`'s own children: a run of
    /// a multi-column container's children between two spanners, laid out in
    /// the container's box **without a copy of the container** — see
    /// [`Builder::column_run`].
    #[allow(clippy::too_many_arguments)]
    fn children(
        &mut self,
        node: &BoxNode,
        run: Option<&[BoxNode]>,
        style: &Consumed,
        content_x: f64,
        content_width: f64,
        depth: usize,
        avoid: bool,
        block: usize,
    ) -> Result<(), Refusal> {
        let content = match run {
            Some(run) => Written::Children(run),
            None => match &node.content {
                Content::Replaced(_) => Written::Replaced,
                Content::Text(source) => Written::Text(source),
                Content::Children(written) => Written::Children(written),
            },
        };
        match content {
            // Unreachable: [`Builder::block`] sizes a replaced box and emits
            // its one item before the dispatch that calls this, for
            // `css-display-3` §2.2's reason. An arm rather than a `_`, so that
            // a fourth kind of content cannot be added without this file
            // deciding what a block container does with it.
            Written::Replaced => Ok(()),
            Written::Text(source) => {
                self.text_block(node, source, style, block, content_x, content_width)
            }
            Written::Children(written) => {
                // §9.2.1.1: an inline box holding a block-level box is split
                // round it, which is this container's child list with that
                // inline box's children standing in its place.
                let children = self.split_inlines(written, depth, false)?;
                let children = children.as_slice();
                let styles: Vec<Consumed> = children.iter().map(|c| consume(&c.style)).collect();
                let any_block = styles.iter().any(|s| !s.is_none() && s.is_block_level());
                if !any_block {
                    // CSS 2.2 §9.4.2: an inline formatting context.
                    let mut pieces = Vec::new();
                    self.lead_with_marker(&mut pieces);
                    let mut collapser = Collapser::new();
                    for child in children.iter().copied() {
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
                for (index, (&child, child_style)) in children.iter().zip(&styles).enumerate() {
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
                        self.misparented(
                            node,
                            &children[index..end],
                            content_width,
                            content_x,
                            depth,
                            avoid,
                        )?;
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
                        // An `inside` marker with no inline content before the
                        // block: its own anonymous line, §9.2.1.1.
                        self.marker_line(style, block, content_x, content_width)?;
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

    /// A block whose content is one text node: one inline formatting context
    /// of one piece.
    ///
    /// **A function of its own for the stack's sake**, as
    /// [`Builder::misparented`] is: [`Builder::children`] is in the frame of
    /// every level of the block recursion, and an unoptimised build gives every
    /// local of every arm its own slot whichever arm runs — a [`Piece`] holds a
    /// whole [`Consumed`]. Kept here, the bytes are spent only by the arm that
    /// needs them, and `a_tree_of_blocks_past_the_depth_cap_is_refused_by_name`
    /// keeps its margin under the depth cap as the computed style grows.
    #[inline(never)]
    fn text_block(
        &mut self,
        node: &BoxNode,
        source: &str,
        style: &Consumed,
        block: usize,
        content_x: f64,
        content_width: f64,
    ) -> Result<(), Refusal> {
        let mut pieces = Vec::new();
        let mut collapser = Collapser::new();
        self.lead_with_marker(&mut pieces);
        let text = collapser.push_transformed(source, style.white_space, style.text_transform);
        pieces.push(Piece {
            text,
            style: style.clone(),
            anchor: node.anchor,
            order: self.order(),
            atomic: None,
            generated: false,
            embeddings: Arc::default(),
        });
        self.lines(&pieces, style, block, content_x, content_width)
    }

    /// CSS 2.2 §9.2.1.1: *"when an inline box contains an in-flow block-level
    /// box, the inline box (and its inline ancestors within the same line box)
    /// are broken around the block-level box"*. A child list in which an
    /// inline box holds such a box — at any depth through inline boxes — is
    /// returned with that inline box replaced by its own children, recursively,
    /// so the caller's run of inline content ends at the block and a new one
    /// begins after it: the content before and after are anonymous block boxes
    /// of their own, and the block is a block between them. The text keeps its
    /// own computed style and anchor, which are what an inline box gives its
    /// content; what is lost is the inline box's own box — a background,
    /// border or horizontal margin on the `<span>` — which this build does not
    /// draw on an inline box in any case.
    ///
    /// An inline box with no such descendant is left whole, and the search
    /// that found so is linear in it and is not charged: the gather that sets
    /// it next walks the same nodes. **Inside a box that splits**, every node a
    /// search visits is charged to the layout work, because there the same
    /// nested inline boxes are searched once per level that splits.
    #[inline(never)]
    fn split_inlines<'n>(
        &mut self,
        children: &'n [BoxNode],
        depth: usize,
        charged: bool,
    ) -> Result<Vec<&'n BoxNode>, Refusal> {
        let mut out = Vec::with_capacity(children.len());
        for child in children {
            if self.holds_block(child, depth + 1, charged)? {
                let Content::Children(inner) = &child.content else {
                    out.push(child);
                    continue;
                };
                out.extend(self.split_inlines(inner, depth + 1, true)?);
            } else {
                out.push(child);
            }
        }
        Ok(out)
    }

    /// Whether `node` is a non-atomic inline box with an in-flow block-level
    /// box inside it, reached through inline boxes only — an inline-block, a
    /// picture, a float and an absolutely positioned box are each a boundary.
    fn holds_block(
        &mut self,
        node: &BoxNode,
        depth: usize,
        charged: bool,
    ) -> Result<bool, Refusal> {
        if depth > self.limits.max_depth {
            return Err(Refusal::TooDeep { depth });
        }
        let style = &node.style;
        if style.display != Display::Inline || style.float != Float::None {
            return Ok(false);
        }
        let Content::Children(children) = &node.content else {
            return Ok(false);
        };
        if charged {
            self.budget.spend_layout(children.len())?;
        }
        for child in children {
            let inner = &child.style;
            let in_flow = inner.float == Float::None
                && !matches!(inner.position, Position::Absolute | Position::Fixed);
            let block_level = matches!(
                inner.display,
                Display::Block | Display::ListItem | Display::Table | Display::Flex
            );
            if (in_flow && block_level) || self.holds_block(child, depth + 1, charged)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// §17.2.1 rule 9's anonymous table round a run of misparented internal
    /// table boxes, laid out as the block it is. A function of its own for
    /// [`Builder::text_block`]'s reason: the wrapper is a whole [`BoxNode`].
    #[inline(never)]
    #[allow(clippy::too_many_arguments)]
    fn misparented(
        &mut self,
        node: &BoxNode,
        run: &[&BoxNode],
        content_width: f64,
        content_x: f64,
        depth: usize,
        avoid: bool,
    ) -> Result<(), Refusal> {
        let wrapper = anonymous_table(&node.style, run);
        self.block(&wrapper, content_width, content_x, depth + 1, avoid, 0)
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
        self.lead_with_marker(&mut pieces);
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
        // exist yet when this runs. See the refusal table in
        // `docs/features/epub.md`.
        if style.float != Float::None {
            return self.float_box(node, &style, content_width, content_x, depth, false);
        }
        // §9.6: `absolute` and `fixed` are out of flow wherever they are
        // written, so one inside a line is taken out of it the way a float is
        // and laid out as the positioned box it is, against its containing
        // block. Where an inset pair leaves it at its static position, that is
        // the context's top left — the float's limit, for the float's reason
        // — and it is named.
        if matches!(style.position, Position::Absolute | Position::Fixed) {
            let auto = |side| style.inset.get(side) == Inset::Auto;
            if (auto(Side::Top) && auto(Side::Bottom)) || (auto(Side::Left) && auto(Side::Right)) {
                self.warn(Warning::PositionedInLine);
            }
            return self.positioned_box(node, &style, content_x, depth, false);
        }
        self.budget.spend_box()?;
        // §9.2.2's own list: *"inline-level boxes that are not inline boxes
        // (such as replaced inline-level elements, inline-block elements and
        // inline-table elements) are called atomic inline-level boxes"*. A
        // picture is the first of the three and takes the same path as the
        // second — one box on the line, placed rather than set, with nothing
        // inside it a line breaker may split.
        if matches!(style.display, Display::InlineBlock | Display::InlineFlex)
            || matches!(node.content, Content::Replaced(_))
        {
            // Here rather than beside the block builder for the reason the
            // warning that used to stand here gave: an `inline-block` is not
            // block-level, so it arrives in an inline formatting context — and
            // this is where that context can give it a place on a line instead
            // of pouring its text into one.
            return self.atomic_inline(node, &style, out, depth, content_width);
        }
        match &node.content {
            // Unreachable: the line above takes every replaced element, whether
            // its `display` is `inline`, `inline-block` or anything else the
            // block dispatch did not claim first.
            Content::Replaced(_) => {}
            Content::Text(source) => {
                let text =
                    collapser.push_transformed(source, style.white_space, style.text_transform);
                if !text.is_empty() {
                    out.push(Piece {
                        text,
                        style,
                        anchor: node.anchor,
                        order: self.order(),
                        atomic: None,
                        generated: false,
                        embeddings: self.embeddings.shared(),
                    });
                }
            }
            Content::Children(children) => {
                // `css-writing-modes-3` §2.2: an inline box whose
                // `unicode-bidi` is not `normal` opens a level round its
                // content, which every piece inside it carries. A block
                // container's `embed` and `isolate` do nothing, and its
                // `plaintext` is its paragraphs' (see [`Builder::line`]).
                let opened = (!style.is_block_level())
                    .then(|| embedding_of(&style, node.anchor))
                    .flatten()
                    .filter(|_| self.embeddings.len() < MAX_EMBEDDING_DEPTH);
                if let Some(embedding) = opened {
                    self.embeddings.push(embedding);
                }
                let mut gathered = Ok(());
                // An in-flow block among the children was split out before
                // this (§9.2.1.1, [`Builder::split_inlines`]), and a float or a
                // positioned box is taken out of the line by its own `gather`.
                for child in children {
                    gathered =
                        self.gather(child, out, collapser, depth + 1, content_x, content_width);
                    if gathered.is_err() {
                        break;
                    }
                }
                if opened.is_some() {
                    self.embeddings.pop();
                }
                gathered?;
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
    ///
    /// A scroll container is cleared a second time, past the floats its border
    /// box would overlap: see [`Builder::clear_beside_floats`]. The two are
    /// one call from [`Builder::block`] for that function's frame.
    fn clear(
        &mut self,
        style: &Consumed,
        margin_top: f64,
        contained: bool,
        left: f64,
        width: f64,
    ) -> Result<(), Refusal> {
        if contained {
            self.clear_beside_floats(left, width, margin_top)?;
        }
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
        let outer_width = self.float_width(node, style, containing, depth, avoid, false)?;
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
            // `css-break-3`'s rule: a float that would fit a page of its own
            // belongs whole on the next one.
            pushable: true,
            z: 0,
        });
        self.flow.floats.append(&mut nested);
        Ok(())
    }

    /// One absolutely positioned or fixed box, CSS 2.2 §9.6.
    ///
    /// Out of flow, which this crate already has a shape for: a
    /// [`FloatRecord`] is items that are **not** in the column, carried with
    /// the `y` they were placed at. An out-of-flow positioned box is the same
    /// thing with a different placement rule and one difference at pagination,
    /// which is why the record grew a flag rather than a twin.
    ///
    /// **`fixed` is not a degradation here.** §9.6.1: *"in the case of paged
    /// media, fixed boxes are repeated on every page, and are fixed with
    /// respect to the page box"*. That is the specification's own paged answer,
    /// it is what a stylesheet asking for a running header meant, and it is
    /// what this does.
    fn positioned_box(
        &mut self,
        node: &BoxNode,
        style: &Consumed,
        x: f64,
        depth: usize,
        avoid: bool,
    ) -> Result<(), Refusal> {
        if depth > self.limits.max_depth {
            return Err(Refusal::TooDeep { depth });
        }
        self.budget.spend_box()?;
        // §9.6.1 again: `fixed`'s containing block is the page box and
        // `absolute`'s is the nearest positioned ancestor, which is what the
        // stack's last entry is. The initial containing block sits at the
        // bottom of that stack, so `absolute` with no positioned ancestor
        // anywhere falls out of the same expression rather than a special case.
        let against = if style.position == Position::Fixed {
            crate::position::Containing {
                left: 0.0,
                top: 0.0,
                width: self.page.0,
                height: Some(self.page.1),
            }
        } else {
            *self
                .positioned
                .last()
                .expect("the initial containing block is never popped")
        };

        // §10.3.7: with **both** `left` and `right` stated and `width: auto`
        // the box fills between them; otherwise the width is shrink-to-fit,
        // which is a float's own rule and therefore a float's own function.
        let left_inset =
            crate::position::inset_px(style.inset.get(Side::Left), Some(against.width));
        let right_inset =
            crate::position::inset_px(style.inset.get(Side::Right), Some(against.width));
        let outer_width = match (style.width, left_inset, right_inset) {
            (Size::Auto, Some(left), Some(right)) => (against.width - left - right).max(0.0),
            // Its two trial layouts go through `block` as well, so the
            // one-shot has to be re-armed for each of them: that is what the
            // last argument is, and it is a parameter rather than a field the
            // trials read because a float's trials must **not** be armed.
            _ => self.float_width(node, style, against.width, depth, avoid, true)?,
        };

        self.placed = true;
        let Sublayout {
            mut items,
            mut blocks,
            floats: nested,
            height,
        } = self.sublayout(node, outer_width, depth, avoid)?;

        // §10.3.7's third case and §10.6.4's: with neither inset of a pair
        // stated the box stays at its **static position** — where it would have
        // been had it been `static`. That is the case nearly every real
        // stylesheet takes and the one an implementation leaves out.
        let left = crate::position::used_left(&style.inset, &against, x, outer_width);
        let top = crate::position::used_top(&style.inset, &against, self.cursor(), height);
        translate(&mut items, &mut blocks, left, top);
        let mut nested = nested;
        for record in &mut nested {
            translate(&mut record.items, &mut record.blocks, left, top);
            record.top += top;
            record.bottom += top;
        }
        let z = match style.z_index {
            ZIndex::Auto => 0,
            ZIndex::Layer(layer) => layer,
        };
        let record = FloatRecord {
            items,
            blocks,
            top,
            bottom: top + height,
            pushable: false,
            z,
        };
        if style.position == Position::Fixed {
            self.flow.fixed.push(record);
        } else {
            self.flow.positioned.push(record);
        }
        // A float **inside** an out-of-flow box belongs to that box's own
        // formatting context and is drawn with it, so it joins the same list
        // rather than the column's.
        for mut inner in nested {
            inner.pushable = false;
            inner.z = z;
            if style.position == Position::Fixed {
                self.flow.fixed.push(inner);
            } else {
                self.flow.positioned.push(inner);
            }
        }
        Ok(())
    }

    /// One `display: inline-block` box, CSS 2.2 §9.2.2.
    ///
    /// **Laid out once, in a formatting context of its own, and then set on the
    /// line as one thing.** §10.3.9 gives it a float's width — shrink-to-fit
    /// where `width` is `auto` — which is why it is a float's own function; and
    /// §9.4.2 makes its inside a block formatting context, which is what
    /// `sublayout` already produces.
    ///
    /// Its piece's text is one **U+FFFC OBJECT REPLACEMENT CHARACTER**. That is
    /// not a placeholder for the box's text: it is the box's position in the
    /// string the line breaker works over, so the breaker can put a line end
    /// beside it and the measure can be asked what it costs. It never becomes a
    /// glyph — [`Builder::line`] gives an atomic span an [`InlineBox`] and no
    /// run at all — so text conservation never sees it, and the box's own runs
    /// carry their own reading-order stamps.
    fn atomic_inline(
        &mut self,
        node: &BoxNode,
        style: &Consumed,
        out: &mut Vec<Piece>,
        depth: usize,
        containing: f64,
    ) -> Result<(), Refusal> {
        let width = self.float_width(node, style, containing, depth, false, false)?;
        let sub = self.sublayout(node, width, depth, false)?;
        // §10.8.1: *"the baseline of the last line box in the normal flow"*,
        // and the bottom margin edge where there is none. The **last** and not
        // the first, which is the difference between a two-line inline-block
        // sitting on the line and hanging from it. An `inline-flex` is the
        // other way round: `css-flexbox-1` §8.5 gives a flex container its
        // items' **first** baseline set.
        let baseline = if style.display == Display::InlineFlex {
            first_baseline(&sub)
        } else {
            last_baseline(&sub)
        }
        .unwrap_or(sub.height);
        // A float inside an inline-block belongs to the inline-block's own
        // formatting context — §9.4.2 makes it one — so it is folded into the
        // box rather than escaping to the paragraph's.
        let mut items = sub.items;
        let mut blocks = sub.blocks;
        for float in sub.floats {
            let base = items.len();
            for mut record in float.blocks {
                if let Some(first) = record.first {
                    record.first = Some(first + base);
                    record.last += base;
                }
                blocks.push(record);
            }
            items.extend(float.items);
        }
        out.push(Piece {
            text: "\u{FFFC}".to_string(),
            style: style.clone(),
            anchor: node.anchor,
            order: self.order(),
            generated: false,
            embeddings: self.embeddings.shared(),
            atomic: Some(Atomic {
                items,
                blocks,
                width,
                height: sub.height,
                baseline,
            }),
        });
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
        out_of_flow: bool,
    ) -> Result<f64, Refusal> {
        let margins =
            style.margin_px(Side::Left, containing) + style.margin_px(Side::Right, containing);
        let extra = style.padding_px(Side::Left, containing)
            + style.padding_px(Side::Right, containing)
            + style.border_width.left
            + style.border_width.right;
        // §10.3.6 for a floating replaced box and §10.3.5's sentence for an
        // absolutely positioned one say the same thing in the same words:
        // *"the used value of `width` is determined as for inline replaced
        // elements"*. So a picture never reaches the shrink-to-fit trials
        // below, which would measure it as nothing — they count the rightmost
        // edge any **text** reached and a picture has none.
        if let Some((width, _)) = replaced_box(node, style, containing) {
            return Ok(width + extra + margins);
        }
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
        self.placed = out_of_flow;
        let preferred = self.measure_content(node, MAX_MEASURE, depth, avoid)? + right;
        self.placed = out_of_flow;
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
    /// stated `width`; see the refusal table in `docs/features/epub.md`.
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
                        // An atomic box's own text is inside it, so a trial
                        // that did not look would measure a paragraph holding
                        // an `inline-block` as the width of the words beside it.
                        for placed in &line.boxes {
                            extend(&placed.items, width);
                        }
                    }
                    ItemKind::Rows(band) | ItemKind::FlexLine(band) | ItemKind::Columns(band) => {
                        extend(&band.items, width);
                    }
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
        self.subflow(node, None, None, measure, depth, avoid)
    }

    /// The same, for a box whose **own** box model has already been paid.
    ///
    /// `inside` lays out only the node's children at the stated width, which is
    /// what a multi-column container needs: [`Builder::block`] has already
    /// applied its margins, border, padding and width, and laying the box out
    /// again would pay for every one of them twice. `run`, with `inside`, is
    /// the children to lay out in place of the node's own — see
    /// [`Builder::children`].
    fn subflow(
        &mut self,
        node: &BoxNode,
        inside: Option<&Consumed>,
        run: Option<&[BoxNode]>,
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
        let inside_marker = self.inside_marker.take();
        // A sub-flow is a formatting context of its own and its paragraphs
        // are their own: an inline-block, a float or a positioned box inside
        // an isolating span opens no level for the runs inside it, which UAX
        // #9 reads as one neutral of the line outside.
        let embeddings = std::mem::take(&mut self.embeddings);

        let result = match inside {
            None => self.block(node, measure, 0.0, depth, avoid, 0),
            Some(style) => {
                // A record for the container itself, so the sub-flow's records
                // are numbered from zero exactly as `block` numbers them and
                // the line filler has an index to patch. **Not painted**: this
                // box's background and border belong to the flow that called
                // this one, and a second copy would draw them once per column.
                let mut record = decorate(node, 0.0, measure);
                record.painted = false;
                self.flow.blocks.push(record);
                self.open.push(0);
                self.children(node, run, style, 0.0, measure, depth, avoid, 0)
            }
        };
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
        self.inside_marker = inside_marker;
        self.embeddings = embeddings;
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

        // The column boxes: §17.5.2's widths. Their backgrounds are §17.5.1's
        // second and third layers, painted per cell by [`Builder::band`]; their
        // borders are §17.6.2.1's to resolve in the collapsing model, and
        // §17.6.1 says to ignore them in the separated one. A background
        // **image** on one is not painted, and is the one thing named.
        let mut declared: Vec<Option<f64>> = vec![None; grid.columns];
        let collapsing = style.border_collapse == BorderCollapse::Collapse;
        for (at, width) in declared.iter_mut().enumerate() {
            let Some(column) = tree.columns.get(at) else {
                break;
            };
            for described in [column.node, column.group].into_iter().flatten() {
                if consume(&described.style)
                    .paint
                    .is_some_and(|paint| paint.image.is_some())
                {
                    self.warn(Warning::ColumnBoxNotPainted);
                }
            }
            let Some(described) = column.node.or(column.group) else {
                continue;
            };
            let consumed = consume(&described.style);
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
            // and cannot therefore make the table wider; see the refusal
            // table in `docs/features/epub.md`.
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

        // **§17.5.3's `vertical-align`, which is where the property does most
        // of its work in a real book.** Four values apply to a cell -- `top`,
        // `middle`, `bottom` and `baseline` -- and §17.5.3 says the other four
        // *"behave as `baseline`"*, which is why the match below has no arm for
        // them rather than an arm that guesses.
        //
        // `baseline` is the initial value and it is **not** the same as `top`:
        // it puts every cell's first line on one row baseline, so a row holding
        // a large heading and small body text has them sitting on a line rather
        // than both starting at the row's top edge. It is also the one value
        // that changes how tall the row has to be, which is why it is settled
        // here, before the heights, and the other three below them.
        let alignment: Vec<VerticalAlign> = grid
            .slots
            .iter()
            .map(|slot| {
                let cell = &tree.groups[slot.group].rows[slot.row].cells[slot.cell];
                consume(&cell.content.node().style).vertical_align
            })
            .collect();
        let cell_baseline: Vec<f64> = laid
            .iter()
            .map(|sub| sub.as_ref().and_then(first_baseline).unwrap_or(0.0))
            .collect();
        // How far each cell's content is pushed down inside its own box.
        let mut lead = vec![0.0f64; grid.slots.len()];
        let mut row_baseline = vec![0.0f64; grid.rows];
        for (at, slot) in grid.slots.iter().enumerate() {
            // A spanning cell has no single row to share a baseline with, so
            // §17.5.3's alignment is taken over its whole box below instead.
            if slot.rows == 1 && matches!(alignment[at], VerticalAlign::Top) {
                continue;
            }
            if slot.rows == 1
                && !matches!(alignment[at], VerticalAlign::Middle | VerticalAlign::Bottom)
            {
                row_baseline[slot.top] = row_baseline[slot.top].max(cell_baseline[at]);
            }
        }
        for (at, slot) in grid.slots.iter().enumerate() {
            if slot.rows == 1
                && !matches!(
                    alignment[at],
                    VerticalAlign::Top | VerticalAlign::Middle | VerticalAlign::Bottom
                )
            {
                lead[at] = (row_baseline[slot.top] - cell_baseline[at]).max(0.0);
            }
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
                // The lead is part of the height: a cell pushed down to reach
                // its row's baseline needs the room it was pushed into, and a
                // build that added the two the other way round draws the last
                // line of the tallest-baselined cell over the row below.
                let height = laid[at].as_ref().map_or(0.0, |sub| sub.height) + lead[at];
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
        // And now the three values that are measured against the cell's box
        // rather than against its row's baseline, which needs the heights.
        for (at, slot) in grid.slots.iter().enumerate() {
            let alignment = alignment[at];
            if !matches!(
                alignment,
                VerticalAlign::Top | VerticalAlign::Middle | VerticalAlign::Bottom
            ) {
                continue;
            }
            let box_height = heights[slot.top..slot.top + slot.rows].iter().sum::<f64>()
                + (slot.rows.saturating_sub(1)) as f64 * vspacing;
            let content = laid[at].as_ref().map_or(0.0, |sub| sub.height);
            let free = (box_height - content).max(0.0);
            lead[at] = match alignment {
                VerticalAlign::Bottom => free,
                VerticalAlign::Middle => free / 2.0,
                _ => 0.0,
            };
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
                &lead,
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

    /// A multi-column container's content, `css-multicol-1`.
    ///
    /// **One flow item, `N` columns inside it.** A column of a multi-column
    /// container reads to its bottom and then jumps back to the top of the next
    /// one, which is the one thing a flow whose `y` never goes backwards cannot
    /// say — so the container is an [`Abreast`], the same answer a table band
    /// and a flex line already are, and [`crate::fragment`] cuts it across
    /// pages at one height over every column of it.
    ///
    /// The steps are §3's and §4's, and each has an answer of its own:
    ///
    /// 1. §3.4's pseudo-algorithm turns `column-count`, `column-width` and the
    ///    gap into a used count and a used width — [`column_geometry`];
    /// 2. the content is laid out **once**, at one column's width, in a flow of
    ///    its own;
    /// 3. §4's `column-fill: balance` finds the shortest height that still fits
    ///    the content in that many columns — [`balance`] over [`fill_columns`];
    /// 4. the flow is sliced at that height and the slices are placed side by
    ///    side;
    /// 5. §5's rule is drawn down the middle of each gap.
    ///
    /// The content is laid out once and sliced, rather than laid out per
    /// column: a column is not a narrower rendering of the content, it is the
    /// **same** rendering cut in a different place, and a build that re-laid
    /// each column would break the same paragraph twice and lose the join.
    fn columns(
        &mut self,
        node: &BoxNode,
        style: &Consumed,
        content_x: f64,
        content_width: f64,
        depth: usize,
        avoid: bool,
    ) -> Result<(), Refusal> {
        // §5.1's gap is `css-align-3` §8.1's, and `normal` is one em **because
        // this box turned out to be multi-column** — which is why the value is
        // carried unresolved as far as here.
        let gap = style.gap_px(style.column_gap, content_width);
        let (count, width) = column_geometry(style, content_width, gap);
        // §6's `column-span: all`: a spanning box *"interrupts"* the columns,
        // is laid out across the container's whole width, and the columns
        // resume beneath it — so a container with spanning children is
        // several column sets, one per run of the children between them, each
        // balanced on its own, with the spanners as ordinary blocks between.
        // A spanner is an in-flow block-level **child** here; one deeper in
        // the tree is laid out in its column and counted.
        let Content::Children(children) = &node.content else {
            return self.column_set(
                node,
                None,
                style,
                content_x,
                depth,
                avoid,
                (count, width, gap),
            );
        };
        self.note_deep_spanners(children, depth)?;
        let spans = |child: &BoxNode| {
            let inner = &child.style;
            inner.column_span == ColumnSpan::All
                && inner.display != Display::None
                && inner.float == Float::None
                && !matches!(inner.position, Position::Absolute | Position::Fixed)
        };
        if !children.iter().any(spans) {
            return self.column_set(
                node,
                None,
                style,
                content_x,
                depth,
                avoid,
                (count, width, gap),
            );
        }
        let mut from = 0;
        for (at, child) in children.iter().enumerate() {
            if !spans(child) {
                continue;
            }
            self.column_run(
                node,
                &children[from..at],
                style,
                content_x,
                depth,
                avoid,
                (count, width, gap),
            )?;
            self.commit_margin();
            self.block(child, content_width, content_x, depth + 1, avoid, 0)?;
            self.commit_margin();
            from = at + 1;
        }
        self.column_run(
            node,
            &children[from..],
            style,
            content_x,
            depth,
            avoid,
            (count, width, gap),
        )
    }

    /// One run of a multi-column container's children between two spanners,
    /// as a column set of its own: the container's box with only these
    /// children laid out in it, so [`Builder::column_set`] lays them out and
    /// balances them as it does a whole container's. A run of nothing is no
    /// set.
    ///
    /// **The run is borrowed, not copied.** A copy of the container holding a
    /// copy of the run was the first way of saying this, and it is a copy of
    /// the whole subtree, alive while that subtree is laid out — so nested
    /// multi-column containers each with a spanner held one copy per level at
    /// once, a few hundred kilobytes of markup becoming gigabytes.
    #[allow(clippy::too_many_arguments)]
    fn column_run(
        &mut self,
        node: &BoxNode,
        run: &[BoxNode],
        style: &Consumed,
        content_x: f64,
        depth: usize,
        avoid: bool,
        geometry: (usize, f64, f64),
    ) -> Result<(), Refusal> {
        if run.iter().all(|child| child.style.display == Display::None) {
            return Ok(());
        }
        self.column_set(node, Some(run), style, content_x, depth, avoid, geometry)
    }

    /// `column-span: all` below a multi-column container's own children,
    /// counted per box and laid out in its column: a spanner nested in a
    /// child is §6's too, and splitting the container round a box inside one
    /// of its children would split that child, which this build does not.
    /// Nested multi-column containers are their own question and are not
    /// entered. Every node visited is charged to the layout work.
    fn note_deep_spanners(&mut self, children: &[BoxNode], depth: usize) -> Result<(), Refusal> {
        if depth > self.limits.max_depth {
            return Err(Refusal::TooDeep { depth });
        }
        for child in children {
            let Content::Children(inner) = &child.content else {
                continue;
            };
            if child.style.display == Display::None || consume(&child.style).is_multicol() {
                continue;
            }
            self.budget.spend_layout(inner.len())?;
            for grandchild in inner {
                if grandchild.style.column_span == ColumnSpan::All
                    && grandchild.style.display != Display::None
                {
                    self.warn(Warning::ColumnSpanAsNone);
                }
            }
            self.note_deep_spanners(inner, depth + 1)?;
        }
        Ok(())
    }

    /// One column set: `node`'s children — or `run`, a run of them — laid out
    /// once at one column's width, balanced, sliced and placed side by side.
    /// See [`Builder::columns`].
    #[allow(clippy::too_many_arguments)]
    fn column_set(
        &mut self,
        node: &BoxNode,
        run: Option<&[BoxNode]>,
        style: &Consumed,
        content_x: f64,
        depth: usize,
        avoid: bool,
        (count, width, gap): (usize, f64, f64),
    ) -> Result<(), Refusal> {
        let sub = self.subflow(node, Some(style), run, width, depth, avoid)?;
        if sub.items.is_empty() {
            return Ok(());
        }
        let Sublayout {
            items: inner,
            blocks: records,
            floats: inner_floats,
            height: inner_height,
        } = sub;

        // §4: `balance` is the shortest height that still fits; `auto` fills
        // each column in turn, and a container with no stated height has no
        // bottom for the first column to reach — so all of it is the first
        // column, which is exactly what asking for the content's own height
        // produces rather than a special case.
        //
        // **And both are capped at the page.** [`crate::fragment`]'s slicer
        // cuts a band at one height across every column of it, which is right
        // for a table row and wrong for a column set: it puts the top of
        // column one and the top of column two on one page and the bottoms of
        // both on the next, so the book reads *across* the columns instead of
        // down them. The geometry it draws is right — that is what a
        // multi-column container looks like over two pages — and the reading
        // order is not, and a page's runs are sorted by a stamp that cannot
        // repair it.
        //
        // So the slicer is never handed a band it has to cut. A container
        // taller than a page becomes **several bands** stacked down the flow,
        // one column set each, and each is short enough to be moved whole to
        // the next page rather than divided. That is also what `css-break-3`
        // §5 makes of a multi-column container across a fragmentainer
        // boundary, so the cure and the specification are one sentence.
        let ceiling = self.page.1.max(EPSILON);
        let target = match style.column_fill {
            ColumnFill::Balance => balance(&inner, count),
            ColumnFill::Auto => inner_height,
        }
        .min(ceiling);
        let starts = fill_columns(&inner, target);

        // Every column's half-open range of items and its own top, in document
        // order — which is the order the sets are emitted in and therefore the
        // order the book reads in.
        let mut ranges: Vec<(usize, usize, f64)> = Vec::with_capacity(starts.len());
        for (column, &from) in starts.iter().enumerate() {
            let to = starts.get(column + 1).copied().unwrap_or(inner.len());
            ranges.push((from, to, inner[from].y));
        }
        // A float inside a column stays inside it: its formatting context is
        // the container's own flow, so it belongs to the column its top fell
        // in and there is nothing for the page cutter to carry forward. The
        // same sentence a cell's float already carries. Decided once, here,
        // because each column belongs to exactly one set below.
        let mut per_column: Vec<Vec<FloatRecord>> = (0..ranges.len()).map(|_| Vec::new()).collect();
        for float in inner_floats {
            let column = ranges
                .iter()
                .rposition(|(_, _, top)| float.top + EPSILON >= *top)
                .unwrap_or(0);
            if let Some(slot) = per_column.get_mut(column) {
                slot.push(float);
            }
        }

        self.commit_margin();
        for (set, chunk) in ranges.chunks(count).enumerate() {
            let mut band = Abreast {
                items: Vec::new(),
                blocks: Vec::new(),
            };
            let mut height = 0.0f64;
            for &(_, to, top) in chunk {
                let tail = &inner[to - 1];
                height = height.max(tail.y + tail.height - top);
            }
            // The rules first, so a column's own backgrounds are drawn over
            // them rather than under. Their spacers are `Edge` items: nothing
            // to read, divisible by the page cutter, and the anchor a
            // `BlockRecord` needs for its height — the same device a table
            // row's spacer is. One fewer than the set has columns, because a
            // rule goes **between** two of them.
            let ruled = style.column_rule_width > 0.0 && style.column_rule_color.a != 0;
            let rules = if ruled {
                chunk.len().saturating_sub(1)
            } else {
                0
            };
            for rule in 0..rules {
                let spacer = band.items.len();
                band.items.push(Item {
                    y: 0.0,
                    height,
                    kind: ItemKind::Edge,
                });
                // §5.1: *"the column rule is drawn in the middle of the gap"*,
                // so it is centred in the gap and not laid against either
                // column.
                let gap_left = content_x + (rule + 1) as f64 * width + rule as f64 * gap;
                band.blocks.push(BlockRecord {
                    x: gap_left + (gap - style.column_rule_width) / 2.0,
                    width: style.column_rule_width,
                    first: Some(spacer),
                    last: spacer + 1,
                    background: style.column_rule_color,
                    border_width: Sides::all(0.0),
                    border_style: Sides::all(BorderStyle::None),
                    border_color: Sides::all(Color::TRANSPARENT),
                    painted: true,
                    replaced: None,
                    dy: 0.0,
                    // The rule belongs to the container: an `opacity` on it
                    // fades its rules with its text.
                    anchor: node.anchor,
                    paint: None,
                    clip: Clip::NONE,
                });
            }
            for (at, &(from, to, top)) in chunk.iter().enumerate() {
                let base = band.items.len();
                let dx = content_x + at as f64 * (width + gap);
                let mut slice: Vec<Item> = inner[from..to].to_vec();
                let mut copies: Vec<BlockRecord> = Vec::new();
                // A box that spans a column boundary is two fragments, which is
                // the page cutter's own rule met one level down: its record is
                // clipped to the slice and copied into each column it reaches.
                for record in &records {
                    let Some(first) = record.first else {
                        continue;
                    };
                    let lo = first.max(from);
                    let hi = record.last.min(to);
                    if lo >= hi {
                        continue;
                    }
                    let mut copy = record.clone();
                    copy.first = Some(lo - from + base);
                    copy.last = hi - from + base;
                    copies.push(copy);
                }
                translate(&mut slice, &mut copies, dx, -top);
                band.items.append(&mut slice);
                band.blocks.append(&mut copies);

                let column = set * count + at;
                for mut float in std::mem::take(&mut per_column[column]) {
                    translate(&mut float.items, &mut float.blocks, dx, -top);
                    let float_base = band.items.len();
                    for mut record in float.blocks {
                        if let Some(first) = record.first {
                            record.first = Some(first + float_base);
                            record.last += float_base;
                        }
                        band.blocks.push(record);
                    }
                    band.items.extend(float.items);
                }
            }
            self.emit(height, ItemKind::Columns(Box::new(band)), true);
        }
        Ok(())
    }

    /// A `display: flex` box's content, `css-flexbox-1` §9.
    ///
    /// §9's own numbered order, and every step is separable:
    ///
    /// 1. §4 generates the items, wrapping each run of child text in an
    ///    anonymous block container;
    /// 2. §9.2 sizes each item along the main axis — `flex-basis`, then the
    ///    main size property, then the content — and clamps it by §4.5's
    ///    automatic minimum;
    /// 3. §5.4 puts the items into order-modified document order;
    /// 4. §9.3 collects them into flex lines;
    /// 5. §9.7 resolves each line's flexible lengths;
    /// 6. every item is laid out at its used main size, **in document order**,
    ///    so the reading-order stamps ascend with the source;
    /// 7. §9.4 sizes the lines in the cross axis and stretches the items that
    ///    asked for it;
    /// 8. §8.2 and §8.3 position the items, §8.4 the lines, and the items are
    ///    emitted in **order-modified** order.
    ///
    /// Steps 6 and 8 being different orders is §5.4's own note — `order` *"does
    /// not affect ordering in non-visual media"* — and is the third time this
    /// crate has met the same shape: a float, a `<tfoot>`, and now this.
    fn flex(
        &mut self,
        node: &BoxNode,
        style: &Consumed,
        content_x: f64,
        content_width: f64,
        depth: usize,
        avoid: bool,
    ) -> Result<(), Refusal> {
        let row = style.flex_direction.is_row();
        let wrap = style.flex_wrap;
        let boxes = flex_boxes(node);
        if boxes.is_empty() {
            return Ok(());
        }
        // The layout total, charged before the loop on what the items have
        // undertaken to cost: three trials and up to two layouts each, which is
        // `MAX_LINE_BREAK_WORK`'s posture and the reason a container with four
        // million items is a refusal rather than a memory graph.
        self.budget.spend_layout(boxes.len().saturating_mul(5))?;
        for item in &boxes {
            if matches!(item, ItemBox::Anonymous(_)) {
                self.budget.spend_box()?;
            }
        }

        // A column container's cross axis is the inline one, so its cross size
        // is the measure and is always definite; a row container's is the block
        // one, and is definite only where the container states a height.
        let stated_height = match style.height {
            Size::Length(length) => Some(resolve_length(length, content_width).max(0.0)),
            Size::Auto => None,
        };
        let (container_main_definite, container_cross_definite) = if row {
            (Some(content_width), stated_height)
        } else {
            (stated_height, Some(content_width))
        };

        // `css-align-3` §8.1's two gaps, sorted onto the two axes — and the
        // mapping is the **axis** and not the direction: `column-gap` is always
        // the inline-axis gap, so it separates the *items* of a row container
        // and the *lines* of a column one, and `row-gap` is the other way. A
        // build that read `column-gap` as "the gap between flex items" gets
        // every row container right and every column container wrong.
        //
        // A percentage is of the container's own content box in that axis, and
        // its block size is `auto` until its content is laid out — so a
        // percentage `row-gap` here resolves against nothing and is zero, which
        // is §8.1's answer for an indefinite size rather than a guess.
        let inline_gap = style.gap_px(style.column_gap, content_width);
        let block_gap = style.gap_px(style.row_gap, stated_height.unwrap_or(0.0));
        let (main_gap, cross_gap) = if row {
            (inline_gap, block_gap)
        } else {
            (block_gap, inline_gap)
        };

        // ---- steps 1 and 2: the items, sized along the main axis ------------
        let mut items: Vec<FlexItem> = Vec::with_capacity(boxes.len());
        let mut cross_inner: Vec<f64> = Vec::with_capacity(boxes.len());
        for item in &boxes {
            let inner = item.node();
            let consumed = consume(&inner.style);
            // CSS 2.2 §8.3: a percentage margin or padding is a percentage of
            // the containing block's **width**, on all four sides. §9 changes
            // nothing about that, so a `margin-top: 10%` on an item in a column
            // container is still ten per cent of the container's width.
            let (main_lead, main_trail, cross_lead, cross_trail) = if row {
                (Side::Left, Side::Right, Side::Top, Side::Bottom)
            } else {
                (Side::Top, Side::Bottom, Side::Left, Side::Right)
            };
            let margin = |side| consumed.margin_px(side, content_width);
            let inset =
                |side| consumed.padding_px(side, content_width) + consumed.border_width.get(side);
            let margin_main = margin(main_lead) + margin(main_trail);
            let margin_cross = margin(cross_lead) + margin(cross_trail);
            let inset_main = inset(main_lead) + inset(main_trail);
            let inset_cross = inset(cross_lead) + inset(cross_trail);

            let (main_property, cross_property) = if row {
                (consumed.width, consumed.height)
            } else {
                (consumed.height, consumed.width)
            };
            // A percentage of an indefinite main size behaves as `auto`, which
            // is CSS 2.2 §10.5's rule for a height against an `auto` containing
            // block and `css-flexbox-1` §9.2's for a percentage `flex-basis`.
            let definite_main = |size: Size| -> Option<f64> {
                match size {
                    Size::Length(LengthPercentage::Px(px)) => Some(px),
                    Size::Length(LengthPercentage::Percent(percent)) => {
                        container_main_definite.map(|main| main * percent / 100.0)
                    }
                    Size::Auto => None,
                }
            };
            // `box-sizing: border-box` measures `width` — and `flex-basis`,
            // which §7.2.3 sizes *"as for `width`"* — from the border box, and
            // every size in §9 is a content one. `css-ui-3` §5.1 puts the
            // min/max properties in the same sentence as `width`, so the two
            // conversions below are the same closure at two insets rather than
            // a special case for the size property.
            let to_content = |value: f64| match consumed.box_sizing {
                BoxSizing::ContentBox => value.max(0.0),
                BoxSizing::BorderBox => (value - inset_main).max(0.0),
            };
            // The two main-axis sizing properties, which are **not** the same
            // pair in the two directions: `min-width` is a main minimum in a
            // row container and a cross one in a column container. A build that
            // read `min_width` on the main axis of both honours half the books
            // and silently ignores the other half.
            let (min_main_size, max_main_size) = if row {
                (consumed.min_width, consumed.max_width)
            } else {
                (consumed.min_height, consumed.max_height)
            };
            let stated_min_main =
                crate::style::min_length(min_main_size, container_main_definite).map(to_content);
            let stated_max_main =
                crate::style::max_length(max_main_size, container_main_definite).map(to_content);
            let specified_main = definite_main(main_property).map(to_content);
            // §9.2 step 3: `flex-basis` first, and the main size property only
            // where it is `auto`. The two are read in that order rather than
            // the other because that is the whole point of the property: an
            // item with `width: 200px; flex-basis: 0` is flexed from zero.
            let basis = match consumed.flex_basis {
                Size::Auto => specified_main,
                other => definite_main(other).map(to_content),
            };

            let available_cross = (content_width - margin_cross - inset_cross).max(0.0);
            let (min_main, max_main, item_cross) = if row {
                self.flex_pass = Some(FlexPass::Measure);
                let min = self.measure_content(inner, 0.0, depth + 1, avoid)?;
                self.flex_pass = Some(FlexPass::Measure);
                let max = self.measure_content(inner, MAX_MEASURE, depth + 1, avoid)?;
                // A row container's cross size is a height, which is not known
                // until the item has been laid out at its used main size. Zero
                // is a placeholder the layout below replaces.
                (min, max, 0.0)
            } else {
                // A column container's main size is a **height**, so the two
                // content sizes §9.2 asks for are the same number: a box's
                // height at a stated width is not a range. What is a range is
                // the cross axis, and that is what the two trials measure here.
                self.flex_pass = Some(FlexPass::Measure);
                let min_cross = self.measure_content(inner, 0.0, depth + 1, avoid)?;
                self.flex_pass = Some(FlexPass::Measure);
                let max_cross = self.measure_content(inner, MAX_MEASURE, depth + 1, avoid)?;
                // `css-sizing-3`'s fit-content: the max-content size, floored
                // by the min-content size and capped by what there is room for.
                let fit = max_cross.min(available_cross.max(min_cross));
                let stretched = consumed.align_self.resolve(style.align_items)
                    == AlignItems::Stretch
                    && matches!(cross_property, Size::Auto)
                    && !wrap.wraps();
                // §9.4 step 11's stretch and `css-sizing-3`'s fit-content are
                // both *"clamped by the used min and max cross sizes"*, and
                // **the clamp is not written here**, which is a finding and not
                // an omission. Every flex item is laid out again through
                // [`Builder::block`], which applies §10.4 to the width it is
                // given; a column container's cross size is always definite, so
                // the line's own cross extent is the container's either way;
                // and the two content measurements above go through the same
                // block path and come back clamped. A clamp added here was
                // reverted when its counted injection fired **zero** — there is
                // no fixture that can tell the two builds apart, which is the
                // definition of code that is not doing anything.
                let cross = if stretched { available_cross } else { fit };
                let height = self.trial_height(inner, cross, depth + 1, avoid)?;
                (height, height, cross)
            };
            let base = basis.unwrap_or(max_main);
            // §4.5 applies to `min-width: auto` and to nothing else, so a
            // stated minimum **replaces** the automatic one rather than losing
            // to it. Where the value is `auto`: §4.5's content-based minimum,
            // *"further clamped by"* the item's own specified size where it has
            // one — without which a `flex: 0 0 40px` item holding one long word
            // could not be made narrower than the word, which is not what the
            // declaration says.
            let min = match stated_min_main {
                Some(stated) => stated,
                // §4.5 again: the content-based minimum is for an item *"that
                // is not a scroll container"*; *"for scroll containers the
                // automatic minimum size is zero, as usual"*. So a `pre {
                // overflow: auto }` in a row shrinks below its longest line
                // and clips it, which is what the declaration is for.
                None if consumed.is_scroll_container() => 0.0,
                None => match specified_main {
                    Some(specified) => min_main.min(specified),
                    None => min_main,
                },
            };
            // §9.2 step 4: the hypothetical main size is the base size clamped
            // by the used minimum **and maximum**, which is what makes `flex: 1`
            // on three items of different content lengths still wrap where they
            // must — and what stops a `max-width` item claiming a line's worth
            // of space at §9.3 step 5 and then shrinking away from it.
            let hypothetical = crate::style::clamp_size(base, Some(min), stated_max_main);

            items.push(FlexItem {
                sizes: flex::Item {
                    grow: consumed.flex_grow,
                    shrink: consumed.flex_shrink,
                    base,
                    hypothetical,
                    min,
                    max: stated_max_main.unwrap_or(f64::INFINITY),
                    extra: margin_main + inset_main,
                },
                order: consumed.order,
                align: flex::self_alignment(consumed.align_self, style.align_items),
                cross_auto: matches!(cross_property, Size::Auto),
                cross_extra: margin_cross + inset_cross,
                cross_margins: margin_cross,
                cross_lead: margin(cross_lead),
                main_inset: inset_main,
                main_margins: margin_main,
                main_lead: margin(main_lead),
            });
            cross_inner.push(item_cross);
        }

        // ---- steps 3, 4 and 5: order, lines, flexible lengths ---------------
        let orders: Vec<i32> = items.iter().map(|item| item.order).collect();
        let placement = flex::ordered(&orders);
        let sizes: Vec<flex::Item> = placement.iter().map(|at| items[*at].sizes).collect();
        // An indefinite main size has no free space in it: §9.7 against a
        // container sized to its own content distributes nothing, which is what
        // using the sum of the hypothetical sizes as the measure produces.
        let total_hypothetical: f64 = sizes.iter().map(flex::Item::outer_hypothetical).sum();
        let available_main = container_main_definite.unwrap_or(total_hypothetical);
        let ranges = flex::lines(&sizes, available_main, wrap, main_gap);
        let mut used_main = vec![0.0f64; sizes.len()];
        for &(from, to) in &ranges {
            // §9.7 distributes what is left **after** the gaps: they are not
            // free space, and an item that grew into one would close it.
            let room = available_main - gaps_between(to - from, main_gap);
            for (offset, size) in flex::resolve(&sizes[from..to], room)
                .into_iter()
                .enumerate()
            {
                used_main[from + offset] = size;
            }
        }
        // Position in `placement` for each item, so the two loops below can
        // walk the same items in their two different orders.
        let mut slot = vec![0usize; items.len()];
        for (position, at) in placement.iter().enumerate() {
            slot[*at] = position;
        }

        // ---- step 6: laid out in document order -----------------------------
        let mut laid: Vec<Option<Sublayout>> = (0..items.len()).map(|_| None).collect();
        let mut outer_cross = vec![0.0f64; items.len()];
        let mut baseline = vec![0.0f64; items.len()];
        for at in 0..items.len() {
            let main = used_main[slot[at]];
            let item = &items[at];
            let pass = if row {
                FlexPass::Used {
                    width: Some(main + item.main_inset),
                    height: None,
                }
            } else {
                FlexPass::Used {
                    width: Some(cross_inner[at] + item.cross_extra - item.cross_margins),
                    height: Some(main),
                }
            };
            self.flex_pass = Some(pass);
            let sub = self.sublayout(boxes[at].node(), content_width, depth + 1, avoid)?;
            outer_cross[at] = if row {
                sub.height
            } else {
                cross_inner[at] + item.cross_extra
            };
            baseline[at] = first_baseline(&sub).unwrap_or(outer_cross[at]);
            laid[at] = Some(sub);
        }

        // ---- step 7: the lines' cross sizes, and §9.4 step 11's stretch -----
        let single = ranges.len() == 1;
        let mut line_cross: Vec<f64> = Vec::with_capacity(ranges.len());
        let mut line_baseline: Vec<f64> = Vec::with_capacity(ranges.len());
        for &(from, to) in &ranges {
            let mut ascent = 0.0f64;
            let mut descent = 0.0f64;
            let mut plain = 0.0f64;
            for &at in &placement[from..to] {
                if items[at].align == AlignItems::Baseline {
                    ascent = ascent.max(baseline[at]);
                    descent = descent.max(outer_cross[at] - baseline[at]);
                } else {
                    plain = plain.max(outer_cross[at]);
                }
            }
            let content = plain.max(ascent + descent);
            // §9.4 step 8's own exception: a **single-line** container with a
            // definite cross size gives its line that size, whatever the items
            // came to. A build without it makes `align-items: center` in a
            // `height: 300px` container centre nothing, because the line is
            // exactly as tall as its tallest item.
            let cross = match (single, container_cross_definite) {
                (true, Some(definite)) => definite,
                _ => content,
            };
            line_cross.push(cross);
            line_baseline.push(ascent);
        }

        // §8.4: the lines in the cross axis. `free` is zero unless the
        // container's cross size is definite, which is §8.4's *"has no effect
        // on a single-line flex container"* arriving as arithmetic rather than
        // as a special case.
        let lines_total: f64 =
            line_cross.iter().sum::<f64>() + gaps_between(ranges.len(), cross_gap);
        let container_cross = container_cross_definite.unwrap_or(lines_total);
        let (lead, gap, extra) = flex::align_content(
            style.align_content,
            container_cross - lines_total,
            ranges.len(),
        );
        for cross in &mut line_cross {
            *cross += extra;
        }

        // §9.4 step 11: an item whose cross size property is `auto` and whose
        // resolved alignment is `stretch` takes its line's cross size. It is a
        // **size** change and therefore a second layout, which is why it is
        // here and not folded into the positions below.
        for (line, &(from, to)) in ranges.iter().enumerate() {
            for (position, &at) in placement.iter().enumerate().take(to).skip(from) {
                let item = &items[at];
                if item.align != AlignItems::Stretch || !item.cross_auto {
                    continue;
                }
                let wanted = line_cross[line];
                if wanted <= outer_cross[at] + EPSILON {
                    continue;
                }
                let inner = (wanted - item.cross_extra).max(0.0);
                let pass = if row {
                    FlexPass::Used {
                        width: Some(used_main[position] + item.main_inset),
                        height: Some(inner),
                    }
                } else {
                    FlexPass::Used {
                        width: Some(inner + item.cross_extra - item.cross_margins),
                        height: Some(used_main[position]),
                    }
                };
                self.flex_pass = Some(pass);
                let sub = self.sublayout(boxes[at].node(), content_width, depth + 1, avoid)?;
                baseline[at] = first_baseline(&sub).unwrap_or(wanted);
                laid[at] = Some(sub);
                outer_cross[at] = wanted;
            }
        }

        // ---- step 8: positions ---------------------------------------------
        let mut main_at = vec![0.0f64; items.len()];
        let mut cross_at = vec![0.0f64; items.len()];
        let mut line_top = vec![0.0f64; ranges.len()];
        let mut logical = lead;
        for (line, &(from, to)) in ranges.iter().enumerate() {
            let used: f64 = (from..to)
                .map(|position| used_main[position] + sizes[position].extra)
                .sum::<f64>()
                + gaps_between(to - from, main_gap);
            // §9 has no `overflow` in it: a line whose items do not fit is
            // drawn where they were put. Saying so is the difference between a
            // known gap and a figure that quietly runs off the page.
            if used > available_main + EPSILON {
                self.warn(Warning::ContentOverflowedPage);
            }
            let (offset, between) =
                flex::justify(style.justify_content, available_main - used, to - from);
            let mut running = offset;
            for position in from..to {
                let at = placement[position];
                let outer = used_main[position] + sizes[position].extra;
                main_at[at] =
                    flex::main_position(style.flex_direction, running, outer, available_main);
                running += outer + between + main_gap;
                let inside = flex::align(
                    items[at].align,
                    line_cross[line],
                    outer_cross[at],
                    baseline[at],
                    line_baseline[line],
                );
                cross_at[at] =
                    flex::cross_position(wrap, inside, outer_cross[at], line_cross[line]);
            }
            line_top[line] = flex::cross_position(wrap, logical, line_cross[line], container_cross);
            logical += line_cross[line] + gap + cross_gap;
        }

        if row {
            self.emit_flex_rows(
                &items,
                &ranges,
                &placement,
                &mut laid,
                &line_cross,
                &line_top,
                &main_at,
                &cross_at,
                &outer_cross,
                content_x,
                avoid,
            );
        } else {
            self.emit_flex_column(
                &items, &placement, &mut laid, &main_at, &cross_at, &used_main, &sizes, content_x,
            );
        }
        Ok(())
    }

    /// One trial layout's height, with the item's own box stripped — §9.2's
    /// content size along a **block** axis, which [`Builder::measure_content`]
    /// cannot give because it answers about widths.
    fn trial_height(
        &mut self,
        node: &BoxNode,
        measure: f64,
        depth: usize,
        avoid: bool,
    ) -> Result<f64, Refusal> {
        // A trial's warnings are not the book's, for `measure_content`'s
        // reason: a paragraph set at a measure of nothing reports a line that
        // overflowed on every word.
        let warnings = std::mem::take(&mut self.warnings);
        self.flex_pass = Some(FlexPass::Measure);
        let trial = self.sublayout(node, measure, depth, avoid);
        self.warnings = warnings;
        Ok(trial?.height)
    }

    /// A row container's lines, emitted one flow item each.
    ///
    /// **In physical top-to-bottom order and not in line order**, because
    /// `flex-wrap: wrap-reverse` stacks the lines the other way and the page
    /// cutter walks a column whose `y` never goes backwards.
    #[allow(clippy::too_many_arguments)]
    fn emit_flex_rows(
        &mut self,
        items: &[FlexItem],
        ranges: &[(usize, usize)],
        placement: &[usize],
        laid: &mut [Option<Sublayout>],
        line_cross: &[f64],
        line_top: &[f64],
        main_at: &[f64],
        cross_at: &[f64],
        outer_cross: &[f64],
        content_x: f64,
        avoid: bool,
    ) {
        let mut order: Vec<usize> = (0..ranges.len()).collect();
        order.sort_by(|a, b| line_top[*a].total_cmp(&line_top[*b]));
        self.commit_margin();
        let separator = MarginBreak {
            forced: false,
            allowed_by_a: true,
            allowed_by_b: !avoid,
        };
        let mut y = 0.0f64;
        for line in order {
            let (from, to) = ranges[line];
            // The space above this line is a `Margin` item and not an `Edge`,
            // so §13.3.3's case (1) applies to it: a container of several lines
            // may be broken between two of them, which is what `css-break-3` §5
            // says of a row flex container and is where a real page break in
            // one goes.
            let above = (line_top[line] - y).max(0.0);
            self.emit(above, ItemKind::Margin(separator.clone()), true);
            y = line_top[line] + line_cross[line];
            let mut band = Abreast {
                items: Vec::new(),
                blocks: Vec::new(),
            };
            for &at in &placement[from..to] {
                place_flex_item(
                    &mut band,
                    laid[at].take(),
                    content_x + main_at[at],
                    cross_at[at],
                    cross_at[at] + items[at].cross_lead,
                    (outer_cross[at] - items[at].cross_margins).max(0.0),
                );
            }
            self.emit(line_cross[line], ItemKind::FlexLine(Box::new(band)), true);
        }
    }

    /// A column container's whole content, as one flow item.
    ///
    /// One and not one per line: a column container's lines sit **beside** each
    /// other, so there is no position between two of them the page cutter could
    /// order — which is [`Abreast`]'s own reason, met from the other direction.
    #[allow(clippy::too_many_arguments)]
    fn emit_flex_column(
        &mut self,
        items: &[FlexItem],
        placement: &[usize],
        laid: &mut [Option<Sublayout>],
        main_at: &[f64],
        cross_at: &[f64],
        used_main: &[f64],
        sizes: &[flex::Item],
        content_x: f64,
    ) {
        let mut band = Abreast {
            items: Vec::new(),
            blocks: Vec::new(),
        };
        let mut height = 0.0f64;
        for (position, &at) in placement.iter().enumerate() {
            let outer_main = used_main[position] + sizes[position].extra;
            height = height.max(main_at[at] + outer_main);
            place_flex_item(
                &mut band,
                laid[at].take(),
                content_x + cross_at[at],
                main_at[at],
                main_at[at] + items[at].main_lead,
                (outer_main - items[at].main_margins).max(0.0),
            );
        }
        self.commit_margin();
        self.emit(height, ItemKind::FlexLine(Box::new(band)), true);
    }

    /// One band of rows, as one flow item's worth of content.
    #[allow(clippy::too_many_arguments)]
    fn band(
        &mut self,
        tree: &TableBox<'_>,
        grid: &Grid,
        rows_of: &[(usize, usize)],
        laid: &mut [Option<Sublayout>],
        lead: &[f64],
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
    ) -> Result<Abreast, Refusal> {
        let mut items: Vec<Item> = Vec::new();
        let mut blocks: Vec<BlockRecord> = Vec::new();
        let band_top = tops[from];

        // CSS 2.2 §17.5.1's second and third layers, under everything else in
        // the band: a column group's and a column's background *"covers
        // exactly the full area of all cells that originate in"* it, so each
        // is painted once per such cell, over that cell's box.
        //
        // **Only where nothing above it hides it.** A cell, its rows or its
        // row group with an opaque background covers the cell's whole area,
        // and painting the column under it would leave the column's colour in
        // the anti-aliased seam where the two rectangles' edges meet — a
        // hairline round every cell that the same table without the column
        // does not have. Where the layers above are transparent the column
        // shows, as §17.5.1 says; where the row group's is translucent, its
        // record was painted once round the whole group before this band
        // began, so it is painted over the column again in the cell's area,
        // which is what keeps the order.
        let opaque = |node: Option<&BoxNode>| {
            node.is_some_and(|node| {
                let style = consume(&node.style);
                style.visible && style.background_color.a == u8::MAX
            })
        };
        for grid_row in from..to {
            for slot in grid.slots.iter().filter(|slot| slot.top == grid_row) {
                let rows_covered = (slot.top..slot.top + slot.rows).all(|row| {
                    rows_of
                        .get(row)
                        .is_some_and(|(group, row)| opaque(tree.groups[*group].rows[*row].node))
                });
                let cell = &tree.groups[slot.group].rows[slot.row].cells[slot.cell];
                if opaque(Some(cell.content.node()))
                    || rows_covered
                    || opaque(tree.groups[slot.group].node)
                {
                    continue;
                }
                let column = tree.columns.get(slot.left);
                let layers = [
                    column.and_then(|column| column.group),
                    column.and_then(|column| column.node),
                ];
                let area = (
                    lefts[slot.left],
                    columns[slot.left..slot.left + slot.columns]
                        .iter()
                        .sum::<f64>()
                        + slot.columns.saturating_sub(1) as f64 * hspacing,
                    tops[slot.top] - band_top,
                    heights[slot.top..slot.top + slot.rows].iter().sum::<f64>()
                        + slot.rows.saturating_sub(1) as f64 * vspacing,
                );
                let mut painted_any = false;
                for node in layers.into_iter().flatten() {
                    painted_any |= self.background_layer(node, area, &mut items, &mut blocks)?;
                }
                if painted_any {
                    if let Some(group) = tree.groups[slot.group].node {
                        self.background_layer(group, area, &mut items, &mut blocks)?;
                    }
                }
            }
        }

        // The row boxes next, so a cell's background covers a row's rather
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
                // §17.5.3: the content moves inside the cell and the cell's
                // own box does not. The spacer below is the box, which is why
                // it is pushed at `cell_top` and this at `cell_top + lead`.
                translate(&mut inner, &mut records, cell_x, cell_top + lead[at]);
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
                    translate(
                        &mut float.items,
                        &mut float.blocks,
                        cell_x,
                        cell_top + lead[at],
                    );
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
        Ok(Abreast { items, blocks })
    }

    /// One of §17.5.1's layers over one cell's area — `(x, width, top,
    /// height)`, the top relative to the band — as a spacer and a record that
    /// paints the box's background colour and nothing else: a column's or a
    /// row group's border is §17.6.2.1's (collapsing) or ignored (§17.6.1,
    /// separated). Whether anything was painted.
    fn background_layer(
        &mut self,
        node: &BoxNode,
        (x, width, top, height): (f64, f64, f64, f64),
        items: &mut Vec<Item>,
        blocks: &mut Vec<BlockRecord>,
    ) -> Result<bool, Refusal> {
        let style = consume(&node.style);
        if !style.visible || style.background_color.a == 0 {
            return Ok(false);
        }
        self.budget.spend_box()?;
        let mut record = decorate(node, x, width);
        record.border_width = Sides::all(0.0);
        record.paint = None;
        record.painted = true;
        let spacer = items.len();
        items.push(Item {
            y: top,
            height,
            kind: ItemKind::Edge,
        });
        record.first = Some(spacer);
        record.last = spacer + 1;
        blocks.push(record);
        Ok(true)
    }

    /// A block record for a box this module lays out itself — a row or a row
    /// group, neither of which goes through [`Builder::block`].
    fn record(&mut self, node: &BoxNode, x: f64, width: f64) -> usize {
        let record = decorate(node, x, width);
        let index = self.flow.blocks.len();
        self.flow.blocks.push(record);
        index
    }

    /// A marker no line took — an item with no inline content anywhere in it —
    /// is not carried into the next item's first line.
    ///
    /// **A method that is never inlined, for the sake of one assignment.** The
    /// slot holds a [`Piece`], and a [`Consumed`] inside that is hundreds of
    /// bytes: assigning to it in [`Builder::block`] put the old value's drop in
    /// `block`'s frame, and `a_tree_of_blocks_past_the_depth_cap_is_refused_by_name`
    /// overflowed its stack the first time this was written that way.
    #[inline(never)]
    fn disarm_marker(&mut self) {
        self.inside_marker = None;
    }

    /// An `inside` list marker, armed for the item's first line. A method and
    /// not lines in [`Builder::block`], for [`Builder::fill_height`]'s reason,
    /// and never inlined for [`Builder::disarm_marker`]'s.
    #[inline(never)]
    fn arm_marker(&mut self, node: &BoxNode, style: &Consumed, ordinal: usize) {
        if style.display != Display::ListItem
            || style.list_style_position != ListStylePosition::Inside
        {
            return;
        }
        let mut text = marker_of(node, style, ordinal);
        if text.is_empty() {
            return;
        }
        // `css-counter-styles-3` §6's suffix ends in a space, which an
        // `outside` marker replaces with its own gap and an `inside` one sets.
        text.push(' ');
        let order = self.order();
        self.inside_marker = Some(Piece {
            text,
            style: style.clone(),
            // The item's own, so that whatever the painter applies to the item
            // — its `opacity` — reaches its marker. Generated text stays out of
            // the structure tree and out of conservation by `generated`, not by
            // having no anchor.
            anchor: node.anchor,
            order,
            atomic: None,
            generated: true,
            embeddings: Arc::default(),
        });
    }

    /// The armed `inside` marker, at the head of an inline formatting context.
    fn lead_with_marker(&mut self, pieces: &mut Vec<Piece>) {
        if let Some(marker) = self.inside_marker.take() {
            pieces.insert(0, marker);
        }
    }

    /// The armed `inside` marker on an anonymous line of its own, where the
    /// item's first content is a block.
    fn marker_line(
        &mut self,
        style: &Consumed,
        block: usize,
        content_x: f64,
        content_width: f64,
    ) -> Result<(), Refusal> {
        let Some(marker) = self.inside_marker.take() else {
            return Ok(());
        };
        self.lines(&[marker], style, block, content_x, content_width)
    }

    /// A `list-item`'s marker, on the first line of its own box.
    #[inline(never)]
    fn marker(
        &mut self,
        node: &BoxNode,
        style: &Consumed,
        block: usize,
        (content_x, content_width): (f64, f64),
        ordinal: usize,
    ) {
        let text = marker_of(node, style, ordinal);
        if text.is_empty() {
            return;
        }
        let Some(first) = self.flow.blocks[block].first else {
            return;
        };
        let font = style.font();
        let width = self.advance_of(&text, &font);
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
                        // On the item's start side: the left in a
                        // left-to-right item and the right in a right-to-left
                        // one (`css-lists-3` §3.1's marker box stands outside
                        // the principal box on its inline-start side).
                        x: if style.direction == Direction::Rtl {
                            content_x + content_width + style.font_size * 0.5
                        } else {
                            content_x - width - style.font_size * 0.5
                        },
                        y: 0.0,
                        width,
                        text,
                        font_size: style.font_size,
                        families: style.families.clone(),
                        weight: style.font_weight,
                        style: style.font_style,
                        variant: style.font_variant,
                        kerning: style.font_kerning,
                        features: style.font_features.clone(),
                        paragraph_rtl: Some(style.direction == Direction::Rtl),
                        paragraph: 0,
                        embeddings: Arc::default(),
                        bidi_level: None,
                        hyphenated: false,
                        color: style.color,
                        decoration: style.text_decoration,
                        painted: style.visible,
                        letter_spacing: 0.0,
                        word_spacing: 0.0,
                        generated: true,
                        // The item's, for the painter; see `arm_marker`.
                        anchor: node.anchor,
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
        let mut opportunities = uax14::opportunities(&content, container.tailoring);
        // `css-text-3` §5.4: under `hyphens: none` a soft hyphen is no place
        // to break a word, though UAX #14 makes it one (class `BA`).
        opportunities.retain(|opportunity| {
            soft_hyphen_before(&content, opportunity.at).is_none()
                || hyphen_shown(&content, &spans, pieces, opportunity.at)
        });

        let indent = match container.text_indent {
            LengthPercentage::Px(px) => px,
            LengthPercentage::Percent(percent) => content_width * percent / 100.0,
        };

        let mut start = 0usize;
        let mut first_line = true;
        let mut lines_here = 0usize;
        let mut cursor = 0usize;
        let first_item = self.flow.items.len();
        // A paragraph separator ends a bidi paragraph as well as a line, so a
        // `plaintext` container's direction is asked again of the text after
        // each one. **Only a separator**: `css-writing-modes-3` §2.4 bounds a
        // bidi paragraph by a block boundary or a *"bidi type B"* forced
        // break, and three of UAX #14's seven forced breaks are not one
        // ([`separates_paragraphs`]). A U+2028 LINE SEPARATOR ends the line
        // and not the paragraph, and a paragraph started after one asked its
        // direction of everything to the next separator — so a block of
        // line separators was asked `O(n^2)` characters, and each line after
        // one could take a direction its paragraph did not have (review of
        // lane 8C).
        let mut paragraph = None;
        let mut paragraph_starts = true;
        let mut paragraph_number = 0usize;
        while start < content.len() {
            if paragraph_starts {
                paragraph = self.paragraph_direction(container, &content, &spans, pieces, start);
                self.paragraphs += 1;
                paragraph_number = self.paragraphs;
            }
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
            // `css-text-3` §8.1: the indent is a margin on the line box's
            // **start** edge, which is the right in a right-to-left
            // paragraph — the side `line` aligns `start` to, by the same
            // test.
            let rtl = paragraph.unwrap_or(container.direction == Direction::Rtl);
            // §9.5's other half: the measure is what the floats beside this
            // line have left of it, and where nothing is left the line goes
            // under them. Both are decided **before** the line is filled,
            // because the width is what decides where it breaks.
            let (line_x, available) = self.beside(
                container,
                content_x,
                content_width,
                (indent_here, rtl),
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
            // §5.4: a line that breaks at a soft hyphen ends in a hyphen. The
            // end of the text is not a break at one, and nor is a break
            // inside a word (`overflow-wrap`) that lands after a soft hyphen
            // `hyphens: none` holds: under `none` it is never a hyphen.
            let hyphenated =
                !hard && end < content.len() && hyphen_shown(&content, &spans, pieces, end);
            self.line(
                &content,
                &spans,
                pieces,
                container,
                block,
                line_x,
                available,
                (trim_start, trim_end),
                (justify, (paragraph, paragraph_number), hyphenated),
                lines_here,
            );
            lines_here += 1;
            first_line = false;
            paragraph_starts = hard
                && content
                    .get(..end)
                    .and_then(|before| before.chars().next_back())
                    .is_some_and(separates_paragraphs);
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

    /// The base direction of the paragraph that starts at `from`: the block
    /// container's `direction`, or, under `unicode-bidi: plaintext`, what P2
    /// and P3 find in the paragraph's own text (`css-writing-modes-3` §2.2).
    ///
    /// The text is asked a box at a time, and what an inline box isolates is
    /// skipped rather than asked: P2 does not look inside an isolate. A
    /// separator inside one — any of the seven ([`separates_paragraphs`]) —
    /// still ends the paragraph, since P1 splits the text before any isolate
    /// is opened. The separator is what keeps the scan to this paragraph
    /// rather than the rest of the container, so a container of a thousand
    /// preserved newlines is not scanned a thousand times over; and since
    /// [`Builder::lines`] starts a paragraph only after a separator, a
    /// thousand line separators are one paragraph, scanned once (review of
    /// lane 8C). `None` is a provider with no UAX #9
    /// ([`Metrics::first_strong`]).
    fn paragraph_direction(
        &self,
        container: &Consumed,
        content: &str,
        spans: &[(usize, usize, usize)],
        pieces: &[Piece],
        from: usize,
    ) -> Option<bool> {
        if container.unicode_bidi != UnicodeBidi::Plaintext {
            return Some(container.direction == Direction::Rtl);
        }
        let first = spans.partition_point(|&(_, end, _)| end <= from);
        for &(start, end, index) in spans.get(first..).unwrap_or_default() {
            let Some(slice) = content.get(start.max(from)..end) else {
                continue;
            };
            let isolated = pieces.get(index).is_some_and(|piece| {
                piece
                    .embeddings
                    .iter()
                    .any(|e| e.kind != EmbeddingKind::Embed)
            });
            if isolated {
                if slice.contains(separates_paragraphs) {
                    return Some(false);
                }
                continue;
            }
            match self.metrics.first_strong(slice)? {
                FirstStrong::Left | FirstStrong::Separator => return Some(false),
                FirstStrong::Right => return Some(true),
                FirstStrong::Neither => {}
            }
        }
        Some(false)
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
    ///
    /// `indent` is the line's `text-indent` and whether the line reads right
    /// to left, which decides the side it is taken from (`css-text-3` §8.1:
    /// the line box's start edge).
    #[allow(clippy::too_many_arguments)]
    fn beside(
        &mut self,
        container: &Consumed,
        content_x: f64,
        content_width: f64,
        (indent, rtl): (f64, bool),
        content: &str,
        spans: &[(usize, usize, usize)],
        pieces: &[Piece],
        opportunities: &[uax14::Opportunity],
        start: usize,
    ) -> Result<(f64, f64), Refusal> {
        let full = (content_width - indent).max(0.0);
        // The indent's place is the start edge: a right-to-left line keeps
        // its left edge and gives the indent up from its right.
        let shift = if rtl { 0.0 } else { indent };
        if self.floats.is_empty() {
            return Ok((content_x + shift, full));
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
        let word = self.measure(content, spans, pieces, start, first, start)
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
            band_left + shift,
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
            width += self.measure(content, spans, pieces, cursor, opportunity.at, start);
            cursor = opportunity.at;
            let trailing = self.trailing(content, spans, pieces, start, opportunity.at);
            // A break at a soft hyphen sets a hyphen at the line's end, and
            // the line has to have room for it (§5.4).
            let hyphen = if hard || opportunity.at >= content.len() {
                0.0
            } else {
                self.hyphen_width(content, spans, pieces, opportunity.at)
            };
            if width - trailing + hyphen <= available {
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
            width += self.measure(content, spans, pieces, at, next, start);
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

    /// The width of the hyphen a line breaking at byte `at` would end in: the
    /// advance of a hyphen in the style of the soft hyphen just before it,
    /// with its `letter-spacing`, or nothing where there is no soft hyphen
    /// there (§5.4).
    fn hyphen_width(
        &self,
        content: &str,
        spans: &[(usize, usize, usize)],
        pieces: &[Piece],
        at: usize,
    ) -> f64 {
        let Some(style) = soft_hyphen_before(content, at)
            .and_then(|shy| piece_at(spans, shy))
            .map(|piece| &pieces[piece].style)
        else {
            return 0.0;
        };
        let mut buffer = [0u8; 4];
        self.advance_of(HYPHEN.encode_utf8(&mut buffer), &style.font()) + style.letter_spacing
    }

    /// The advance of one byte range, spanning as many pieces as it must, on
    /// a line that starts at `line_start`.
    ///
    /// Each piece's slice is measured **in its context** ([`context_of`]):
    /// the text either side of it on the line, which a shaper joins across
    /// and kerns against. Where the line ends is not known yet while it is
    /// being filled, so the text after a slice is taken as far as the
    /// neighbour goes; the line's own runs are measured again with both ends
    /// known when the line is set ([`Builder::line`]).
    #[allow(clippy::too_many_arguments)]
    fn measure(
        &self,
        content: &str,
        spans: &[(usize, usize, usize)],
        pieces: &[Piece],
        from: usize,
        to: usize,
        line_start: usize,
    ) -> f64 {
        let mut total = 0.0;
        for (at, (start, end, index)) in spans.iter().enumerate() {
            let lo = (*start).max(from);
            let hi = (*end).min(to);
            if lo >= hi {
                continue;
            }
            // An atomic box costs its own width and no glyph at all: the
            // U+FFFC in the string is its position, not its ink. A build that
            // measured the character would give a two-inch figure the width of
            // one replacement glyph and overflow every line holding one.
            if let Some(atomic) = &pieces[*index].atomic {
                total += atomic.width;
                continue;
            }
            let style = &pieces[*index].style;
            let slice = &content[lo..hi];
            let context = context_of(
                content,
                spans,
                pieces,
                at,
                lo..hi,
                line_start..content.len(),
            );
            total += self.advance_in(slice, &style.font(), &context);
            total += style.letter_spacing * visible_chars(slice) as f64;
            total += style.word_spacing * slice.chars().filter(|c| *c == ' ').count() as f64;
        }
        total
    }

    /// The advance of one slice in one style, through **one** path.
    ///
    /// The single place this crate decides whether a run belongs to the shaper
    /// or to `Metrics::measure`. `metrics.rs`'s [`Shaper`] documents why that
    /// has to be one place: a ligature is narrower than its components and a
    /// joined Arabic word is narrower still, so a run measured one way and
    /// drawn the other breaks in the wrong place — the exact failure the
    /// `Metrics` trait's own documentation warns about, now with a second path
    /// that really does disagree.
    ///
    /// Direction is asked of the text rather than carried in, because a
    /// paragraph's levels are not resolved in this crate; a run that is
    /// entirely right-to-left is shaped as such and everything else is not.
    /// That is a **coarse** answer and it is deliberate — `flow.rs` breaks
    /// lines over logical text and never reorders, so what it needs from
    /// direction is the run's *width*, which the two agree on.
    fn advance_of(&self, text: &str, font: &FontRequest<'_>) -> f64 {
        self.advance_in(text, font, &ShapingContext::NONE)
    }

    /// [`Builder::advance_of`], with the text either side of the slice on its
    /// line: what the shaper joins across and kerns against
    /// ([`crate::metrics::Shaper::shape_in`]). A provider with no shaper has
    /// no use for it and measures the slice alone, as it always has.
    ///
    /// A soft hyphen measures nothing: it is invisible where no line breaks at
    /// it (`css-text-3` §5.4), and where one does the hyphen it becomes is
    /// added by whoever set the break ([`Builder::hyphen_width`]).
    fn advance_in(&self, text: &str, font: &FontRequest<'_>, context: &ShapingContext<'_>) -> f64 {
        let visible;
        let text = if text.contains(SOFT_HYPHEN) {
            visible = text.replace(SOFT_HYPHEN, "");
            visible.as_str()
        } else {
            text
        };
        match self.metrics.shaper() {
            Some(shaper) => shaper.shape_in(text, font, false, context).advance,
            None => self.metrics.measure(text, font),
        }
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
        self.measure(content, spans, pieces, from + trimmed.len(), to, from)
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
        (start, end): (usize, usize),
        (justify, (paragraph, paragraph_number), hyphenated): (bool, (Option<bool>, usize), bool),
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
        // §10.8.1's alignment, kept beside each run rather than applied as it
        // is met: **two of its values are defined by the line box** and the
        // line box does not exist until every other value has had its say. So
        // the shift a run knows now goes into `TextRun::y`, which [`LineBox`]
        // documents as relative to the baseline, and the two that do not are
        // decided below.
        let mut aligned: Vec<(VerticalAlign, f64, f64)> = Vec::new();
        // §9.2.2's atomic boxes, gathered beside the runs and given their `x`
        // in the same pass that gives the runs theirs.
        let mut boxes: Vec<(usize, InlineBox)> = Vec::new();
        let mut width = 0.0;
        for (span_at, (span_start, span_end, index)) in spans.iter().enumerate() {
            let lo = (*span_start).max(start);
            let hi = (*span_end).min(end);
            if lo >= hi {
                continue;
            }
            let style = &pieces[*index].style;
            let font = style.font();
            // §9.2.2: an atomic box's extent either side of the baseline is
            // the box's own, not a font's — which is why the two `over`/`under`
            // are read from it and every one of §10.8.1's eight values then
            // works on it unchanged.
            let atomic = pieces[*index].atomic.as_ref();
            let vertical = self.metrics.vertical(&font);
            let leading = (style.line_height - vertical.height()) / 2.0;
            // The inline box's own extent either side of **its** baseline,
            // which is the content area plus §10.8.1's half-leading and is
            // what every one of the eight values is stated against.
            let (over, under) = match atomic {
                Some(atomic) => (atomic.baseline, atomic.height - atomic.baseline),
                None => (vertical.ascent + leading, vertical.descent + leading),
            };
            // §10.8.1's `text-top` and `text-bottom` are stated against the
            // **content area**, which is the font's box and not the inline
            // box: the half-leading is part of the line's height and not part
            // of the letters. An atomic box has no such distinction — its
            // margin box is all there is — so for it the two pairs are one.
            let (face_over, face_under) = match atomic {
                Some(_) => (over, under),
                None => (vertical.ascent, vertical.descent),
            };
            // Positive is **down**, because that is the direction this module's
            // `y` runs. §10.8.1 states its lengths the other way up -- a
            // positive `vertical-align` length *raises* the box -- so the one
            // place the sign is flipped is the arm that reads the length.
            let shift = match style.vertical_align {
                VerticalAlign::Baseline => 0.0,
                VerticalAlign::Sub => SUB_DROP * container.font_size,
                VerticalAlign::Super => -SUPER_RISE * container.font_size,
                // *"the top of the box with the top of the parent's content
                // area"*. The parent here is the block container, because this
                // build flattens an inline subtree into spans rather than
                // nesting boxes -- so the parent's content area is the strut's,
                // which is exactly what §10.8.1's strut is.
                VerticalAlign::TextTop => face_over - strut.ascent,
                VerticalAlign::TextBottom => strut.descent - face_under,
                // *"the vertical midpoint of the box with the baseline of the
                // parent box plus half the x-height of the parent"*.
                VerticalAlign::Middle => {
                    -(X_HEIGHT * container.font_size) / 2.0 - (face_under - face_over) / 2.0
                }
                VerticalAlign::Length(LengthPercentage::Px(px)) => -px,
                // A percentage is of this element's own `line-height` and
                // `style::consume` has already resolved it to one, so this arm
                // is unreachable rather than unhandled.
                VerticalAlign::Length(LengthPercentage::Percent(_)) => 0.0,
                // Decided below, against a line box that does not exist yet.
                VerticalAlign::Top | VerticalAlign::Bottom => 0.0,
            };
            if !matches!(
                style.vertical_align,
                VerticalAlign::Top | VerticalAlign::Bottom
            ) {
                above = above.max(over - shift);
                below = below.max(under + shift);
            }
            aligned.push((style.vertical_align, over, under));
            // An atomic box is placed, not set: no run, no glyph, and a width
            // that is the box's. Its `x` is filled in by the alignment pass
            // below, in the same order the runs are.
            if let Some(atomic) = atomic {
                boxes.push((
                    runs.len(),
                    InlineBox {
                        items: atomic.items.clone(),
                        blocks: atomic.blocks.clone(),
                        dy: shift - atomic.baseline,
                    },
                ));
                runs.push(TextRun {
                    x: 0.0,
                    y: shift,
                    width: atomic.width,
                    text: String::new(),
                    font_size: style.font_size,
                    families: style.families.clone(),
                    weight: style.font_weight,
                    style: style.font_style,
                    variant: style.font_variant,
                    kerning: style.font_kerning,
                    features: style.font_features.clone(),
                    paragraph_rtl: paragraph,
                    paragraph: paragraph_number,
                    embeddings: pieces[*index].embeddings.clone(),
                    bidi_level: None,
                    hyphenated: false,
                    color: style.color,
                    decoration: style.text_decoration,
                    painted: false,
                    letter_spacing: 0.0,
                    word_spacing: 0.0,
                    generated: true,
                    anchor: pieces[*index].anchor,
                    order: pieces[*index].order,
                });
                width += atomic.width;
                continue;
            }
            let text = content[lo..hi].to_string();
            // The run in the context it is drawn in: its neighbours on this
            // line, both ends of which are known now.
            let context = context_of(content, spans, pieces, span_at, lo..hi, start..end);
            // The run that ends a hyphenated line is measured with the hyphen
            // set after it, in its context, as the painter draws it.
            let ends_hyphenated = hyphenated && hi == end && text.ends_with(SOFT_HYPHEN);
            let measured = if ends_hyphenated {
                let mut drawn = text.clone();
                drawn.push(HYPHEN);
                self.advance_in(&drawn, &font, &context) + style.letter_spacing
            } else {
                self.advance_in(&text, &font, &context)
            };
            let advance = measured
                + style.letter_spacing * visible_chars(&text) as f64
                + style.word_spacing * text.chars().filter(|c| *c == ' ').count() as f64;
            runs.push(TextRun {
                x: 0.0,
                y: shift,
                width: advance,
                text,
                font_size: style.font_size,
                families: style.families.clone(),
                weight: style.font_weight,
                style: style.font_style,
                variant: style.font_variant,
                kerning: style.font_kerning,
                features: style.font_features.clone(),
                paragraph_rtl: paragraph,
                paragraph: paragraph_number,
                embeddings: pieces[*index].embeddings.clone(),
                bidi_level: None,
                hyphenated: ends_hyphenated,
                color: style.color,
                decoration: style.text_decoration,
                painted: style.visible,
                letter_spacing: style.letter_spacing,
                word_spacing: style.word_spacing,
                generated: pieces[*index].generated,
                anchor: pieces[*index].anchor,
                order: pieces[*index].order,
            });
            width += advance;
        }

        // §10.8.1's `top` and `bottom` are aligned to the **line box**, which
        // is why they could not be decided above. A box taller than the line it
        // is aligned to grows it -- downward for `top`, upward for `bottom` --
        // and one pass over each is where §10.8.1 stops being an algorithm and
        // starts being a description. Growing in the other direction is what
        // keeps the aligned edge where it was put.
        for (align, over, under) in &aligned {
            match align {
                VerticalAlign::Top => below = below.max(over + under - above),
                VerticalAlign::Bottom => above = above.max(over + under - below),
                _ => {}
            }
        }
        for (run, (align, over, under)) in runs.iter_mut().zip(&aligned) {
            match align {
                VerticalAlign::Top => run.y = over - above,
                VerticalAlign::Bottom => run.y = below - under,
                _ => {}
            }
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
        // `css-text-3` §7.1: `start` and `end` are the block container's
        // inline-start and -end sides, and a justified paragraph's last line
        // is `text-align-last: auto`, which is `start` (§7.2).
        // A provider that cannot say what P2 finds leaves a `plaintext`
        // paragraph aligned by `direction`, as [`Metrics::first_strong`] says.
        let rtl = paragraph.unwrap_or(container.direction == Direction::Rtl);
        let start_side = if rtl { slack } else { 0.0 };
        let mut offset = match container.text_align {
            TextAlign::Left => 0.0,
            TextAlign::Right => slack,
            TextAlign::Center => slack / 2.0,
            TextAlign::Start => start_side,
            TextAlign::End => slack - start_side,
            TextAlign::Justify => {
                if justify && spaces > 0 {
                    extra_per_space = slack / spaces as f64;
                    0.0
                } else {
                    start_side
                }
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

        // The placeholder runs carry the `x` the alignment pass computed, so
        // the boxes take it from them and the placeholders go. One pass and not
        // two, which is what keeps a box and the text beside it from ever
        // disagreeing about where the line starts.
        let mut boxes: Vec<InlineBox> = boxes
            .into_iter()
            .map(|(at, mut placed)| {
                translate(&mut placed.items, &mut placed.blocks, runs[at].x, 0.0);
                placed
            })
            .collect();
        boxes.shrink_to_fit();
        runs.retain(|run| !(run.generated && run.text.is_empty()));

        let height = above + below;
        let line = LineBox {
            baseline: above,
            runs,
            boxes,
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

/// CSS 2.2 §10.3.2's last case: the used `width` of a replaced box that has no
/// intrinsic width and no ratio to derive one from, in CSS pixels.
const REPLACED_DEFAULT_WIDTH: f64 = 300.0;

/// §10.6.2's last case: *"the height of the largest rectangle that has a 2:1
/// ratio, has a height not greater than 150px, and has a width not greater than
/// the device width"*. The 150 is the cap; the 2:1 is applied against the used
/// width beside it.
const REPLACED_DEFAULT_HEIGHT: f64 = 150.0;

/// A replaced box's used size, with every specified value resolved the way
/// [`Builder::block`] resolves it for every other box.
///
/// `None` for anything that is not a replaced element, which is what lets the
/// three callers — the block builder, the float width and the absolutely
/// positioned box — ask the same question without first asking what kind of
/// node they are holding.
fn replaced_box(node: &BoxNode, style: &Consumed, containing: f64) -> Option<(f64, f64)> {
    let Content::Replaced(intrinsic) = &node.content else {
        return None;
    };
    let extra = style.padding_px(Side::Left, containing)
        + style.padding_px(Side::Right, containing)
        + style.border_width.left
        + style.border_width.right;
    let to_content = |specified: f64| {
        match style.box_sizing {
            BoxSizing::ContentBox => specified,
            BoxSizing::BorderBox => specified - extra,
        }
        .max(0.0)
    };
    Some(replaced_size(
        *intrinsic,
        match style.width {
            Size::Auto => None,
            Size::Length(length) => Some(to_content(resolve_length(length, containing))),
        },
        match style.height {
            // §10.5: a percentage height against a containing block whose own
            // height is `auto` *"is treated as `auto`"*, and at this point in
            // the pass every ancestor's height is still being accumulated. The
            // same reading [`Builder::block`]'s `stated_height` takes, so a
            // picture and a `<div>` agree about what a percentage height is.
            Size::Length(LengthPercentage::Px(px)) => Some(px.max(0.0)),
            Size::Length(LengthPercentage::Percent(_)) | Size::Auto => None,
        },
        (
            crate::style::min_length(style.min_width, Some(containing)).map(to_content),
            crate::style::min_length(style.min_height, None),
        ),
        (
            crate::style::max_length(style.max_width, Some(containing)).map(to_content),
            crate::style::max_length(style.max_height, None),
        ),
    ))
}

/// CSS 2.2 §10.3.2 and §10.6.2: a replaced box's used `width` and `height`,
/// with §10.4's and §10.7's constraints applied.
///
/// # Why this is one function and not two
///
/// The two sections are mutually recursive and the specification writes them
/// that way: §10.3.2's second case is *"the used value of `width` is (used
/// height) × (intrinsic ratio)"* and §10.6.2's second is *"(used width) ÷
/// (intrinsic ratio)"*. Only one of the two can be the one that recurses, and
/// which one it is depends on which of `width` and `height` the author stated.
/// A build with a `used_width` and a `used_height` that each called the other
/// either loops or silently picks a winner; this takes the pair at once and the
/// cases are the specification's own, in its order.
///
/// # And why §10.4's table is here rather than [`crate::style::clamp_size`]
///
/// §10.4 has two algorithms. For every other box it is *"apply the rules again
/// with `max-width` as the width, then again with `min-width`"*, which
/// `clamp_size` is. For a replaced box **with an intrinsic ratio and both
/// `width` and `height` auto** it is instead a table of eleven constraint
/// violations, and the difference is the whole point of the table: clamping the
/// width alone would leave the height at its intrinsic value and **stretch the
/// picture**. `img { max-width: 100% }` — which is on almost every reflowable
/// book's stylesheet — is exactly that case, so the table is the common path
/// and not the exotic one.
///
/// Every argument is already resolved into content-box CSS pixels; `None` is
/// `auto` for `width`/`height` and for `min-*`, and `none` for `max-*`.
fn replaced_size(
    intrinsic: Intrinsic,
    width: Option<f64>,
    height: Option<f64>,
    min: (Option<f64>, Option<f64>),
    max: (Option<f64>, Option<f64>),
) -> (f64, f64) {
    // The four constraints as numbers rather than as options, which is what
    // lets §10.4's table be written in the specification's own words: every one
    // of its rows reads `max(…, min-height)` or `min(…, max-width)`, and those
    // expressions are only total once an absent minimum is zero and an absent
    // maximum is infinite.
    let min_width = min.0.unwrap_or(0.0).max(0.0);
    let min_height = min.1.unwrap_or(0.0).max(0.0);
    let max_width = max.0.unwrap_or(f64::INFINITY).max(0.0);
    let max_height = max.1.unwrap_or(f64::INFINITY).max(0.0);

    // ---- §10.3.2 and §10.6.2, before any constraint ------------------------
    let (tentative_width, tentative_height) = match (width, height) {
        (Some(w), Some(h)) => (w, h),
        // `width` stated, `height` auto: §10.6.2's case 2, then 3, then 4.
        (Some(w), None) => (w, height_from(w, intrinsic)),
        // `height` stated, `width` auto: §10.3.2's *"`width` has a computed
        // value of `auto`, `height` has some other computed value, and the
        // element does have an intrinsic ratio"*, then case 4, then case 5.
        (None, Some(h)) => {
            let w = match (intrinsic.ratio, intrinsic.width) {
                (Some(ratio), _) => h * ratio,
                (None, Some(w)) => w,
                (None, None) => REPLACED_DEFAULT_WIDTH,
            };
            (w, h)
        }
        (None, None) => {
            let w = match (intrinsic.width, intrinsic.height, intrinsic.ratio) {
                // §10.3.2 case 1.
                (Some(w), _, _) => w,
                // §10.3.2 case 2's first half: no intrinsic width, but an
                // intrinsic height and a ratio.
                (None, Some(h), Some(ratio)) => h * ratio,
                // §10.3.2 calls this one *"undefined in CSS 2.2"*.
                // `css-images-3` §5.3.2's default sizing algorithm defines it:
                // the largest rectangle with the ratio that fits the default
                // object size, which §5.3.1 fixes at 300 by 150.
                (None, None, Some(ratio)) => {
                    REPLACED_DEFAULT_WIDTH.min(REPLACED_DEFAULT_HEIGHT * ratio)
                }
                // §10.3.2's last case: *"none of the conditions above are met,
                // then the used value of `width` becomes 300px"*. An intrinsic
                // height with no ratio lands here and not in case 2, which
                // needs both.
                (None, _, None) => REPLACED_DEFAULT_WIDTH,
            };
            let h = match intrinsic.height {
                // §10.6.2 case 1.
                Some(h) => h,
                None => height_from(w, intrinsic),
            };
            (w, h)
        }
    };

    // ---- §10.4 and §10.7 ---------------------------------------------------
    //
    // The table governs only the case it is stated for. Everywhere else §10.4's
    // ordinary instruction applies — *"the rules are applied again, but this
    // time using the computed value of `max-width` as the computed value for
    // `width`"* — and applying §10.3's rules again with a width that is no
    // longer `auto` is exactly what recomputing the height from the clamped
    // width does.
    let governed = width.is_none()
        && height.is_none()
        && intrinsic.ratio.is_some()
        && tentative_width > 0.0
        && tentative_height > 0.0;
    if !governed {
        let used_width = crate::style::clamp_size(tentative_width, min.0, max.0).max(0.0);
        // **Only a width the clamp actually moved re-derives the height**, and
        // that condition is not a shortcut: §10.6.2's case 1 prefers an
        // intrinsic *height* to a height derived through the ratio, so a box
        // whose width nothing touched must keep the height §10.6.2 already gave
        // it rather than have `height_from` answer a second time. Recomputing
        // unconditionally is also what made the `(Some(w), None)` arm above
        // unreadable — a counted injection that broke it caught **nothing**,
        // because every value it produced was thrown away here.
        let used_height = if height.is_some() || used_width == tentative_width {
            tentative_height
        } else {
            height_from(used_width, intrinsic)
        };
        return (
            used_width,
            crate::style::clamp_size(used_height, min.1, max.1).max(0.0),
        );
    }

    let (w, h) = (tentative_width, tentative_height);
    let (over_w, under_w) = (w > max_width, w < min_width);
    let (over_h, under_h) = (h > max_height, h < min_height);
    // The two-violation rows first: each of them is also a single-violation row
    // and would be answered wrongly by it.
    let (used_width, used_height) = match (over_w, under_w, over_h, under_h) {
        (true, _, true, _) if max_width / w <= max_height / h => {
            (max_width, min_height.max(max_width * h / w))
        }
        (true, _, true, _) => (min_width.max(max_height * w / h), max_height),
        (_, true, _, true) if min_width / w <= min_height / h => {
            (max_width.min(min_height * w / h), min_height)
        }
        (_, true, _, true) => (min_width, max_height.min(min_width * h / w)),
        // §10.4's *(w < min-width) and (h > max-height)* row and its
        // *(w > max-width) and (h < min-height)* twin are **not written
        // here**, and that is a proof rather than an omission: each is an
        // identity of the single-violation row that answers it, for every
        // input that could reach it.
        //
        // Reaching the first means `under_w && over_h`, with `over_w` and
        // `under_h` both false — every earlier arm needs one of those two. So
        // the *w < min-width* row below answers it, and its height reads
        // `min(min-width × h ÷ w, max-height)`: `min-width ÷ w > 1` makes the
        // left term greater than `h`, and `h > max-height` makes it greater
        // than the right, so the minimum **is** `max-height` and the pair is
        // `(min-width, max-height)` — the row. The twin is the same argument
        // with every inequality turned round, against the *w > max-width* row.
        //
        // Measured, 16 September 2026: transcribing both rows and then
        // deleting them again failed **0** tests in `tinker-pdf-layout` and
        // **0** in `tinker-pdf`, and the proof above is why no fixture could
        // raise either number. A row that cannot change an answer is not a
        // guard, so it is gone and the answers it gave are asserted by
        // `a_minimum_on_one_axis_and_a_maximum_on_the_other_are_both_honoured`.
        (true, ..) => (max_width, min_height.max(max_width * h / w)),
        (_, true, ..) => (min_width, max_height.min(min_width * h / w)),
        (_, _, true, _) => (min_width.max(max_height * w / h), max_height),
        (.., true) => (max_width.min(min_height * w / h), min_height),
        _ => (w, h),
    };
    (used_width.max(0.0), used_height.max(0.0))
}

/// §10.6.2's cases 2, 3 and 4: a replaced box's `height` when `height` is
/// `auto` and the used `width` is settled.
fn height_from(width: f64, intrinsic: Intrinsic) -> f64 {
    match (intrinsic.ratio, intrinsic.height) {
        (Some(ratio), _) if ratio > 0.0 => width / ratio,
        (_, Some(height)) => height,
        _ => (width / 2.0).min(REPLACED_DEFAULT_HEIGHT),
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
/// What a block container holds, as [`Builder::children`] lays it out: the
/// node's own [`Content`], or a run of its children in their place.
#[derive(Clone, Copy)]
enum Written<'n> {
    Replaced,
    Text(&'n str),
    Children(&'n [BoxNode]),
}

fn decorate(node: &BoxNode, x: f64, width: f64) -> BlockRecord {
    let style = consume(&node.style);
    let painted = style.background_color.a != 0
        || style.border_width.top > 0.0
        || style.border_width.right > 0.0
        || style.border_width.bottom > 0.0
        || style.border_width.left > 0.0
        || draws_beyond_its_border(&style);
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
        replaced: None,
        dy: 0.0,
        anchor: node.anchor,
        paint: style.paint.clone(),
        clip: Clip::NONE,
    }
}

/// Content a block-axis clip hid, kept as `visibility: hidden` content is: laid
/// out, carrying its reading-order stamps, and painting nothing — no run, no
/// background, no border, no picture. See [`Builder::clip_tail`].
///
/// `at` moves the outermost items to one height and gives them none, so a
/// hidden tail of any length occupies the single point where it was cut and
/// can never be what makes a page; the items inside a band or an atomic inline
/// keep their own coordinates, which nothing reads once nothing paints.
fn hide(items: &mut [Item], blocks: &mut [BlockRecord], at: Option<f64>) {
    for record in blocks.iter_mut() {
        record.painted = false;
        record.replaced = None;
        record.clip = Clip::NONE;
    }
    for item in items {
        if let Some(y) = at {
            item.y = y;
            item.height = 0.0;
        }
        match &mut item.kind {
            ItemKind::Line(line) => {
                for run in &mut line.runs {
                    run.painted = false;
                }
                for placed in &mut line.boxes {
                    hide(&mut placed.items, &mut placed.blocks, None);
                }
            }
            ItemKind::Rows(band) | ItemKind::FlexLine(band) | ItemKind::Columns(band) => {
                let Abreast { items, blocks } = &mut **band;
                hide(items, blocks, None);
            }
            ItemKind::Margin(_) | ItemKind::Edge => {}
        }
    }
}

/// A `height` that is a length: §10.5 makes a percentage of an `auto`-height
/// containing block `auto`, and at this point in the pass every containing
/// block's height is `auto`.
fn definite_height(style: &Consumed) -> Option<f64> {
    match style.height {
        Size::Length(LengthPercentage::Px(px)) => Some(px.max(0.0)),
        Size::Length(LengthPercentage::Percent(_)) | Size::Auto => None,
    }
}

/// The horizontal extent some content reached, for [`Builder::note_overflow`].
struct Reach {
    lo: f64,
    hi: f64,
}

impl Default for Reach {
    fn default() -> Self {
        Reach {
            lo: f64::INFINITY,
            hi: f64::NEG_INFINITY,
        }
    }
}

impl Reach {
    fn span(&mut self, from: f64, to: f64) {
        self.lo = self.lo.min(from);
        self.hi = self.hi.max(to);
    }

    /// Every run and box in some items, into bands and atomic inlines — the
    /// same walk `measure_content` makes, on both sides.
    fn items(&mut self, items: &[Item]) {
        for item in items {
            match &item.kind {
                ItemKind::Line(line) => {
                    for run in &line.runs {
                        self.span(run.x, run.x + run.width);
                    }
                    for placed in &line.boxes {
                        self.items(&placed.items);
                        for record in &placed.blocks {
                            self.span(record.x, record.x + record.width);
                        }
                    }
                }
                ItemKind::Rows(band) | ItemKind::FlexLine(band) | ItemKind::Columns(band) => {
                    self.items(&band.items);
                    for record in &band.blocks {
                        self.span(record.x, record.x + record.width);
                    }
                }
                ItemKind::Margin(_) | ItemKind::Edge => {}
            }
        }
    }
}

/// Whether a box draws an outline, a background image or a shadow, any of
/// which makes it painted with no background colour or border at all — or is
/// transformed, whose fragment the painter needs as the reference box its
/// content turns about.
fn draws_beyond_its_border(style: &Consumed) -> bool {
    style.paint.as_ref().is_some_and(|paint| {
        paint.transformed
            || paint.outline.is_some()
            || paint.image.is_some()
            || !paint.shadows.is_empty()
    })
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
/// `docs/features/epub.md`'s refusal table.
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
/// `css-flexbox-1` §4: a flex container's items.
///
/// Every in-flow child is one, and **each contiguous run of child text is
/// wrapped in an anonymous block container** — except a run that is all white
/// space, which §4 says *"is not rendered"*. A build that skipped the wrapping
/// would drop a container's bare text out of the flow entirely, and a build
/// that skipped the exception would make a flex item out of the newline between
/// two `<div>`s, which every producer writes.
fn flex_boxes(container: &BoxNode) -> Vec<ItemBox<'_>> {
    let mut out: Vec<ItemBox<'_>> = Vec::new();
    match &container.content {
        // Unreachable: `css-display-3` §2.2 makes a replaced element's inner
        // display type ignored, so [`Builder::block`] never dispatches one to
        // the flex driver. A picture has no flex items either way.
        Content::Replaced(_) => {}
        Content::Text(text) => {
            if !text.trim().is_empty() {
                out.push(ItemBox::Anonymous(Box::new(anonymous_flex_item(
                    &container.style,
                    vec![BoxNode::text(container.style.clone(), text.clone())],
                ))));
            }
        }
        Content::Children(children) => {
            let mut run: Vec<BoxNode> = Vec::new();
            let mut any = false;
            for child in children {
                if consume(&child.style).is_none() {
                    continue;
                }
                if let Content::Text(text) = &child.content {
                    run.push(child.clone());
                    any = any || !text.trim().is_empty();
                    continue;
                }
                if any {
                    out.push(ItemBox::Anonymous(Box::new(anonymous_flex_item(
                        &container.style,
                        std::mem::take(&mut run),
                    ))));
                }
                run.clear();
                any = false;
                out.push(ItemBox::Element(child));
            }
            if any {
                out.push(ItemBox::Anonymous(Box::new(anonymous_flex_item(
                    &container.style,
                    run,
                ))));
            }
        }
    }
    out
}

/// `css-multicol-1` §3.4's pseudo-algorithm: the used column count and width.
///
/// §3.4 is four cases over two properties and it is written out rather than
/// folded, because the two-stated case is **not** the minimum of the two
/// answers taken separately: `column-count: 3; column-width: 10em` in a box
/// with room for five ten-em columns is three columns of a third of the box
/// each, not three of ten em.
///
/// The `(auto, auto)` case cannot be reached — [`Consumed::is_multicol`] is
/// exactly its negation — and is answered as one column rather than left to a
/// panic, which is ruling 1.
fn column_geometry(style: &Consumed, available: f64, gap: f64) -> (usize, f64) {
    let stated = match style.column_count {
        ColumnCount::Auto => None,
        ColumnCount::Count(count) => Some(usize::from(count).max(1)),
    };
    let wanted = match style.column_width {
        ColumnWidth::Auto => None,
        // A zero or negative `column-width` would divide by zero below. §3.1
        // makes the value non-negative and a zero one meaningless, so it is
        // read as `auto` rather than obeyed.
        ColumnWidth::Px(px) if px > 0.0 => Some(px),
        ColumnWidth::Px(_) => None,
    };
    // §3.4's *"floor((available + gap) / (width + gap))"*, at least one.
    let fits = |width: f64| -> usize {
        let step = width + gap;
        if step <= 0.0 {
            return 1;
        }
        let count = ((available + gap) / step).floor();
        if (1.0..1_000.0).contains(&count) {
            count as usize
        } else if count >= 1_000.0 {
            // A container a thousand columns wide is a stylesheet accident and
            // a work bomb. Ruling 2: it is capped rather than refused.
            1_000
        } else {
            1
        }
    };
    let count = match (stated, wanted) {
        (Some(count), None) => count,
        (None, Some(width)) => fits(width),
        (Some(count), Some(width)) => count.min(fits(width)),
        (None, None) => 1,
    };
    let count = count.max(1);
    let width = ((available - (count - 1) as f64 * gap) / count as f64).max(0.0);
    (count, width)
}

/// Fills columns of a stated height, greedily, and returns the first item index
/// of each.
///
/// A column ends **before** the first item that would take it past the height,
/// which is `css-break-3`'s class-2 break — between line boxes — with a box's
/// own padding edge allowed as well, exactly as [`crate::fragment`]'s last tier
/// allows it.
///
/// A column always takes at least one item, however tall it is. Without that
/// clause a box taller than the target starts a new column for ever, which is
/// §9.3's own `"if the very first uncollected item wouldn't fit, collect just
/// it"` met in a different specification.
fn fill_columns(items: &[Item], height: f64) -> Vec<usize> {
    let mut starts = vec![0usize];
    let Some(first) = items.first() else {
        return starts;
    };
    let mut top = first.y;
    for (at, item) in items.iter().enumerate().skip(1) {
        if item.y + item.height > top + height + EPSILON && at > *starts.last().unwrap_or(&0) {
            starts.push(at);
            top = item.y;
        }
    }
    starts
}

/// `css-multicol-1` §4's `balance`: the shortest height that still fits the
/// content in `count` columns.
///
/// **A search and not a division.** `total / count` is the answer only when the
/// content can be cut anywhere, and it cannot: the cuts are between items, so
/// the even share usually needs one more column than there is. The predicate
/// — *does this height fit in `count` columns* — is monotone in the height, so
/// a bisection finds the boundary.
///
/// Sixty-four halvings, which takes an interval of any real page height below
/// the last bit of an `f64`. The number is stated rather than tuned: the value
/// returned is only ever fed back through [`fill_columns`], whose comparisons
/// carry `EPSILON`, so the last bits cannot reach the page.
///
/// Ruling 4: `floor` and halving are exact in IEEE-754 and identical on every
/// target, which is why the balance is arithmetic rather than a transcendental.
fn balance(items: &[Item], count: usize) -> f64 {
    let (Some(first), Some(last)) = (items.first(), items.last()) else {
        return 0.0;
    };
    let total = last.y + last.height - first.y;
    if count <= 1 || total <= 0.0 {
        return total.max(0.0);
    }
    let mut lo = 0.0f64;
    let mut hi = total;
    for _ in 0..64 {
        let mid = lo + (hi - lo) / 2.0;
        if fill_columns(items, mid).len() <= count {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    // **The bisection's answer is a hair too small and the hair is visible.**
    // `fill_columns` keeps a line whose bottom lands on the boundary to within
    // `EPSILON`, so the smallest height it *accepts* is a hair under the height
    // it then comes to -- and at that height the first column drops its last
    // line while the second still takes it. Seven lines into two columns split
    // three and four instead of four and three, which is a page that looks
    // deliberate.
    //
    // So the height is settled by asking the fill what it actually came to and
    // filling again at that. The second answer is a fixed point of the first,
    // and the loop is bounded rather than trusted.
    let mut height = hi;
    for _ in 0..4 {
        let actual = tallest(items, &fill_columns(items, height));
        if actual <= height + EPSILON {
            break;
        }
        height = actual;
    }
    height
}

/// The tallest of the columns a fill produced.
fn tallest(items: &[Item], starts: &[usize]) -> f64 {
    let mut height = 0.0f64;
    for (column, &from) in starts.iter().enumerate() {
        let to = starts.get(column + 1).copied().unwrap_or(items.len());
        if from >= to {
            continue;
        }
        let bottom = items[to - 1].y + items[to - 1].height;
        height = height.max(bottom - items[from].y);
    }
    height
}

/// `css-align-3` §8.1: what `count` things cost in gaps.
///
/// A gap goes **between** two things and not after the last, so `n` of them
/// cost `n - 1` gaps and one of them costs none. Written once, because the
/// off-by-one is the whole of what this property gets wrong: five places need
/// the number and four of them would look right with `count * gap` on any
/// fixture where the container was wide enough to hide it.
fn gaps_between(count: usize, gap: f64) -> f64 {
    count.saturating_sub(1) as f64 * gap
}

/// §4's anonymous block container around a run of text.
fn anonymous_flex_item(parent: &ComputedStyle, run: Vec<BoxNode>) -> BoxNode {
    let mut style = ComputedStyle::inherit_from(parent);
    style.display = Display::Block;
    BoxNode {
        style,
        content: Content::Children(run),
        anchor: None,
        span: crate::CellSpan::ONE,
        marker: None,
    }
}

/// The distance from a sub-flow's top to its **first** baseline, `css-align-3`
/// §9.
///
/// `None` when there is no line box in it at all — an item holding one empty
/// box — which §8.3 answers by synthesising a baseline from the box's cross-end
/// edge. That is the caller's fallback and not this function's, because the
/// box's size is the caller's to know.
/// The distance from a sub-flow's top to its **last** baseline.
///
/// CSS 2.2 §10.8.1's rule for an `inline-block`, and the last rather than the
/// first is the whole of it: a two-line inline-block aligned on its first
/// baseline **hangs** from the line it is on, with its second line below the
/// paragraph's, and every book that puts a two-line caption inline looks
/// broken in a way nothing names.
fn last_baseline(sub: &Sublayout) -> Option<f64> {
    let mut found = None;
    for item in &sub.items {
        match &item.kind {
            ItemKind::Line(line) => found = Some(item.y + line.baseline),
            ItemKind::Rows(band) | ItemKind::FlexLine(band) | ItemKind::Columns(band) => {
                for inner in &band.items {
                    if let ItemKind::Line(line) = &inner.kind {
                        found = Some(item.y + inner.y + line.baseline);
                    }
                }
            }
            ItemKind::Margin(_) | ItemKind::Edge => {}
        }
    }
    found
}

fn first_baseline(sub: &Sublayout) -> Option<f64> {
    for item in &sub.items {
        match &item.kind {
            ItemKind::Line(line) => return Some(item.y + line.baseline),
            ItemKind::Rows(band) | ItemKind::FlexLine(band) | ItemKind::Columns(band) => {
                for inner in &band.items {
                    if let ItemKind::Line(line) = &inner.kind {
                        return Some(item.y + inner.y + line.baseline);
                    }
                }
            }
            ItemKind::Margin(_) | ItemKind::Edge => {}
        }
    }
    None
}

/// One flex item's sub-flow, moved into a line at the position §8 gave it.
///
/// The spacer is the same device the table band uses and is here for the same
/// reason: an item's background and border are its **box's**, which is the size
/// §9 gave it and not the extent its text happened to reach. A stretched item
/// holding one word would otherwise be painted one line tall inside a box three
/// lines tall.
///
/// `box_top` is the **border** box's top and `x`/`top` move the *margin* box,
/// which are two different edges and differ by the item's cross-start margin. A
/// build that used one for both paints every item that has a margin on it in
/// the wrong place.
fn place_flex_item(
    band: &mut Abreast,
    sub: Option<Sublayout>,
    x: f64,
    top: f64,
    box_top: f64,
    box_height: f64,
) {
    let Some(Sublayout {
        items: mut inner,
        blocks: mut records,
        floats,
        height: _,
    }) = sub
    else {
        return;
    };
    translate(&mut inner, &mut records, x, top);
    let spacer = band.items.len();
    band.items.push(Item {
        y: box_top,
        height: box_height,
        kind: ItemKind::Edge,
    });
    let base = band.items.len();
    for (index, mut record) in records.into_iter().enumerate() {
        if index == 0 {
            record.first = Some(spacer);
            record.last = spacer + 1;
        } else if let Some(first) = record.first {
            record.first = Some(first + base);
            record.last += base;
        }
        band.blocks.push(record);
    }
    band.items.append(&mut inner);
    // A float inside a flex item stays inside the line: §3 makes a flex item
    // establish a formatting context of its own, so nothing it contains can
    // reach past the item.
    for mut float in floats {
        translate(&mut float.items, &mut float.blocks, x, top);
        let float_base = band.items.len();
        for mut record in float.blocks {
            if let Some(first) = record.first {
                record.first = Some(first + float_base);
                record.last += float_base;
            }
            band.blocks.push(record);
        }
        band.items.extend(float.items);
    }
}

fn anonymous_table(parent: &ComputedStyle, run: &[&BoxNode]) -> BoxNode {
    let mut style = ComputedStyle::inherit_from(parent);
    style.display = Display::Table;
    BoxNode {
        style,
        content: Content::Children(run.iter().map(|node| (*node).clone()).collect()),
        anchor: None,
        span: crate::CellSpan::ONE,
        marker: None,
    }
}

/// Moves a finished sub-flow to where its float was placed.
///
/// A run's `y` is not touched because a run has not got one yet: it is written
/// at pagination out of its line box's position, so moving the item moves the
/// text with it.
/// CSS 2.2 §9.4.3's offset, applied to the **ink** and not to the flow.
///
/// [`translate`]'s twin, and the difference is the whole of §9.4.3: that one
/// moves an item, this one moves what an item draws. A relatively positioned
/// box keeps its place in the column — so the page cutter still sees a `y` that
/// never goes backwards — and every run and every decoration inside it is drawn
/// somewhere else.
///
/// A run's `y` is already §10.8.1's shift from its line's baseline, so the two
/// offsets add: a `vertical-align: super` inside a `position: relative` span is
/// raised twice, by two different rules, and that is right.
fn shift(items: &mut [Item], dx: f64, dy: f64) {
    for item in items {
        match &mut item.kind {
            ItemKind::Line(line) => {
                for run in &mut line.runs {
                    run.x += dx;
                    run.y += dy;
                }
                for placed in &mut line.boxes {
                    translate(&mut placed.items, &mut placed.blocks, dx, 0.0);
                    placed.dy += dy;
                }
            }
            ItemKind::Rows(band) | ItemKind::FlexLine(band) | ItemKind::Columns(band) => {
                shift(&mut band.items, dx, dy);
                for record in &mut band.blocks {
                    record.x += dx;
                    record.dy += dy;
                }
            }
            ItemKind::Margin(_) | ItemKind::Edge => {}
        }
    }
}

fn translate(items: &mut [Item], blocks: &mut [BlockRecord], dx: f64, dy: f64) {
    for item in items {
        item.y += dy;
        match &mut item.kind {
            ItemKind::Line(line) => {
                for run in &mut line.runs {
                    run.x += dx;
                }
                for placed in &mut line.boxes {
                    translate(&mut placed.items, &mut placed.blocks, dx, 0.0);
                }
            }
            // A band's items are already relative to the band, so only the
            // horizontal half of the move reaches inside it. A build that
            // passed `dy` down as well would move a table inside a float twice.
            ItemKind::Rows(band) | ItemKind::FlexLine(band) | ItemKind::Columns(band) => {
                translate(&mut band.items, &mut band.blocks, dx, 0.0);
            }
            ItemKind::Margin(_) | ItemKind::Edge => {}
        }
    }
    for block in blocks {
        block.x += dx;
    }
}

/// U+00AD SOFT HYPHEN: a place a word may break, invisible unless it does
/// (`css-text-3` §5.4).
pub(crate) const SOFT_HYPHEN: char = '\u{AD}';

/// What a line that breaks at a soft hyphen ends in: U+002D, the hyphen every
/// face has — the standard 14 have no U+2010 — and the one a reader joining
/// hyphenated words already looks for.
pub(crate) const HYPHEN: char = '-';

/// Whether `c` is a paragraph separator, `Bidi_Class` `B`: what ends a bidi
/// paragraph inside a block (`css-writing-modes-3` §2.4) and stops UAX #9's
/// P2.
///
/// The seven characters of `DerivedBidiClass.txt`'s `B`, written out because
/// this crate has no `Bidi_Class` table ([`Metrics::first_strong`] says why)
/// and the class is closed and small. Four of them are also UAX #14 forced
/// breaks — LF, CR, NEL and U+2029 — and the other three forced breaks are
/// not separators: U+000B is `S`, U+000C and U+2028 LINE SEPARATOR `WS`.
/// U+001C to U+001E are separators that are not forced breaks; one ends the
/// scan for a paragraph's direction, as P2 says, but no line is cut there,
/// so the text after it stays in the layout's paragraph until the next
/// forced break that is a separator, where P1 would start one. They are C0
/// controls XML 1.0 does not admit, so only markup read as HTML holds one.
fn separates_paragraphs(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{1C}'..='\u{1E}' | '\u{85}' | '\u{2029}')
}

/// Where the soft hyphen just before byte `at` of `content` is, if one is.
fn soft_hyphen_before(content: &str, at: usize) -> Option<usize> {
    content
        .get(..at)
        .filter(|before| before.ends_with(SOFT_HYPHEN))
        .map(|_| at - SOFT_HYPHEN.len_utf8())
}

/// Whether a line that ends at byte `at` ends in a hyphen: it ends just
/// after a soft hyphen whose element's `hyphens` is not `none`
/// (`css-text-3` §5.4).
///
/// The one test both halves ask — which soft hyphens are break
/// opportunities, and which line ends show a hyphen — so that a break that
/// reaches a soft hyphen another way, `overflow-wrap`'s break inside a word,
/// cannot show one `none` forbids (review of lane 8C).
fn hyphen_shown(
    content: &str,
    spans: &[(usize, usize, usize)],
    pieces: &[Piece],
    at: usize,
) -> bool {
    soft_hyphen_before(content, at).is_some_and(|shy| {
        piece_at(spans, shy).is_none_or(|p| {
            pieces
                .get(p)
                .is_none_or(|piece| piece.style.hyphens != Hyphens::None)
        })
    })
}

/// How many characters of `text` are seen: all but its soft hyphens, which
/// take no `letter-spacing` either.
fn visible_chars(text: &str) -> usize {
    text.chars().filter(|c| *c != SOFT_HYPHEN).count()
}

/// The level an inline box opens round its content, from its `unicode-bidi`
/// and `direction` (`css-writing-modes-3` §2.4.2's table).
fn embedding_of(style: &Consumed, anchor: Option<u32>) -> Option<Embedding> {
    let kind = match style.unicode_bidi {
        UnicodeBidi::Normal => return None,
        UnicodeBidi::Embed => EmbeddingKind::Embed,
        UnicodeBidi::Isolate => EmbeddingKind::Isolate,
        UnicodeBidi::Plaintext => EmbeddingKind::FirstStrong,
    };
    Some(Embedding {
        kind,
        rtl: style.direction == Direction::Rtl,
        anchor,
    })
}

/// The text either side of `slice` — part of span `at` — on a line that
/// covers `line`, as a shaper sees it ([`ShapingContext`]).
///
/// The rule is the painter's, because a run measured in one context and drawn
/// in another is the two-paths failure [`crate::metrics::Shaper`] warns
/// about: a context is a **painted** neighbour of **text** on **the same
/// line**. An atomic box, generated content (a marker, `::before`) and text
/// that is laid out and not drawn (`visibility: hidden`) are no one's
/// context and take none, and a line's edges stop it. Inside one span the
/// rest of the span is the context — a slice between two break
/// opportunities is part of a run the painter shapes whole. Empty spans are
/// passed over, since they draw nothing to stand between two runs.
///
/// Whether a neighbour that qualifies is in the same **face** — the other
/// half of the painter's rule — is the provider's to decide, which is why each
/// side carries its own [`FontRequest`].
///
/// A neighbour is handed over as its near [`CONTEXT_BYTES`] and no more —
/// the last bytes of what comes before, the first of what comes after — so
/// that what a provider does with it costs the same on a line of any length
/// (review of lane 8C).
fn context_of<'p>(
    content: &'p str,
    spans: &[(usize, usize, usize)],
    pieces: &'p [Piece],
    at: usize,
    slice: core::ops::Range<usize>,
    line: core::ops::Range<usize>,
) -> ShapingContext<'p> {
    let text_of = |index: usize| {
        pieces
            .get(index)
            .filter(|piece| piece.atomic.is_none() && !piece.generated && piece.style.visible)
    };
    let Some(&(start, end, own)) = spans.get(at) else {
        return ShapingContext::NONE;
    };
    let Some(piece) = text_of(own) else {
        return ShapingContext::NONE;
    };
    let neighbour = |from: usize, to: usize, piece: &'p Piece| {
        (from < to)
            .then(|| content.get(from..to))
            .flatten()
            .map(|text| Neighbour {
                text,
                font: piece.style.font(),
            })
    };
    // Only the near end of either, [`CONTEXT_BYTES`] of it cut back to a
    // character boundary: before a slice the neighbour is the line so far,
    // and the slice is measured at every break opportunity.
    let before_of = |from: usize, to: usize, piece: &'p Piece| {
        let mut from = from.max(to.saturating_sub(CONTEXT_BYTES));
        while from < to && !content.is_char_boundary(from) {
            from += 1;
        }
        neighbour(from, to, piece)
    };
    let after_of = |from: usize, to: usize, piece: &'p Piece| {
        let mut to = to.min(from.saturating_add(CONTEXT_BYTES));
        while to > from && !content.is_char_boundary(to) {
            to -= 1;
        }
        neighbour(from, to, piece)
    };
    let before = if slice.start > start {
        before_of(start.max(line.start), slice.start, piece)
    } else {
        spans[..at]
            .iter()
            .rev()
            .find(|(s, e, _)| s < e)
            .and_then(|&(s, e, index)| before_of(s.max(line.start), e, text_of(index)?))
    };
    let after = if slice.end < end {
        after_of(slice.end, end.min(line.end), piece)
    } else {
        spans
            .get(at + 1..)
            .unwrap_or(&[])
            .iter()
            .find(|(s, e, _)| s < e)
            .and_then(|&(s, e, index)| after_of(s, e.min(line.end), text_of(index)?))
    };
    ShapingContext { before, after }
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

/// A list marker's text, CSS 2.2 §12.5, for a caller with no counters.
///
/// `css-counter-styles-3` §6's predefined styles, from the one place this
/// workspace formats them, [`tinker_pdf_css::counter::marker_text`], so a
/// marker this crate counts and one a cascade counted cannot be drawn two ways.
#[must_use]
pub fn marker_text(kind: ListStyleType, ordinal: usize) -> String {
    tinker_pdf_css::counter::marker_text(kind, i64::try_from(ordinal).unwrap_or(i64::MAX))
}

/// A list item's marker text: the caller's, where it counted one, and this
/// crate's own sibling count where it did not. See [`BoxNode::marker`].
fn marker_of(node: &BoxNode, style: &Consumed, ordinal: usize) -> String {
    match &node.marker {
        Some(text) => text.clone(),
        None => marker_text(style.list_style_type, ordinal + 1),
    }
}
