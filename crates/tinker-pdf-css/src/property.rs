//! Decision 5, in code: **a property is a variant here only when a consumer
//! exists, and every consumer matches exhaustively with no `_` arm.**
//!
//! # The problem this solves
//!
//! A partially-implemented CSS property does not fail. It lays the page out
//! slightly differently, and nobody can tell by looking. Gap 18a found a JPEG
//! 2000 precision shift that passed every boundary test because it produced a
//! plausible picture; gap 30 found a simple-font fallback that draws readable
//! text wrong only where a font's cmap and WinAnsi disagree. A property parsed
//! and then ignored is the same failure with no pixels at all.
//!
//! So [`Property`] is not a string-keyed map. Adding a variant to it without
//! adding an arm to [`apply`] and to [`Property::name`] **does not compile** —
//! `error[E0004]`, at the two `match`es below. That is gap 29's `const`-block
//! device one level up, and it is the strongest rung available: a test can be
//! forgotten and a `match` cannot.
//!
//! `tests/unimplemented_property_does_not_build.rs` injects exactly that defect
//! and asserts the **build** fails, in both directions — the pristine copy of
//! this crate is compiled first, so a harness that could not compile anything
//! would fail rather than pass.
//!
//! # `Unsupported` and `Unknown` are different facts
//!
//! [`Declaration::Unsupported`] is **this build's own gap**: a property from a
//! specification this crate cites, at a value it does not implement or with no
//! implementation at all. It is what an `As built` has to count.
//! [`Declaration::Unknown`] is a typo, a vendor extension or a custom property,
//! and is ordinary — milestone 1's census found `-webkit-column-count`,
//! `-epub-text-emphasis-style` and Antenna House's `-ah-margin-start` in real
//! books, and reporting those as gaps in this engine would drown the number
//! that matters.
//!
//! # The set is keyed by (property, value), not by property
//!
//! `float: inline-start` is **not** `float: left`. `position: sticky` is not
//! `position: relative`. `display: flex` is not `display: block`. Each property
//! below registers the exact set of values it honours, and a value outside it
//! is `Unsupported` **even though the property is supported** — because a build
//! that maps an unhandled value onto its nearest handled one is producing gap
//! 07's solid-black gradient in a stylesheet.

use crate::parser::ComponentValue;
use crate::tokenizer::Token;

// ---- values -----------------------------------------------------------------

/// An opaque RGBA colour, alpha as a byte.
///
/// Alpha is quantised to a byte rather than kept as a float, so two stylesheets
/// that say `rgba(0,0,0,.5)` and `rgba(0,0,0,50%)` compare equal and so that
/// nothing in the cascade depends on float equality. Ruling 4's determinism
/// question is answered the same way: a byte is a byte on every target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
    /// Alpha, 0 transparent to 255 opaque.
    pub a: u8,
}

impl Color {
    /// Opaque black, which is `color`'s initial value.
    pub const BLACK: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    /// `transparent`, which is `background-color`'s initial value.
    pub const TRANSPARENT: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
}

/// A length as the author wrote it, unit and all.
///
/// It is kept specified rather than computed at parse time because `em` is
/// relative to the element's **own** computed font size, which is not known
/// until the cascade has picked a winner for `font-size`. A parser that
/// resolved `em` eagerly would resolve it against the wrong number for every
/// element that sets both.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Len {
    /// An absolute length, already in CSS pixels.
    Px(f64),
    /// Relative to this element's computed `font-size`.
    Em(f64),
    /// Relative to the root element's computed `font-size`.
    Rem(f64),
    /// A percentage of something the layout decides.
    Percent(f64),
}

impl Len {
    /// Resolves everything but a percentage, which stays for layout.
    pub fn compute(self, font_size: f64, root_font_size: f64) -> LengthPercentage {
        match self {
            Len::Px(px) => LengthPercentage::Px(px),
            Len::Em(factor) => LengthPercentage::Px(factor * font_size),
            Len::Rem(factor) => LengthPercentage::Px(factor * root_font_size),
            Len::Percent(percent) => LengthPercentage::Percent(percent),
        }
    }
}

/// A computed length: absolute, or a percentage layout still owes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LengthPercentage {
    /// CSS pixels.
    Px(f64),
    /// Per cent of a containing-block dimension.
    Percent(f64),
}

impl LengthPercentage {
    /// Zero pixels.
    pub const ZERO: Self = Self::Px(0.0);
}

/// `width` and `height`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Size {
    /// `auto`
    Auto,
    /// A length or a percentage.
    Length(LengthPercentage),
}

/// A specified `width`/`height`, before `em` is resolved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SpecifiedSize {
    /// `auto`
    Auto,
    /// A length.
    Length(Len),
}

/// `min-width` and `min-height`, CSS 2.2 §10.4 and §10.7.
///
/// **`auto` is a value and not a synonym for zero**, and keeping the two apart
/// is the whole reason this is not a [`LengthPercentage`]. `css-sizing-3` §5.1
/// makes `auto` the initial value; it resolves to zero in a block formatting
/// context, which is CSS 2.2's own initial value read forward, and to
/// `css-flexbox-1` §4.5's **automatic minimum main size** on a flex item —
/// which is the item's min-content size and is nothing like zero. A build that
/// computed `auto` to zero here would let every flex item shrink below its
/// longest word, and the page would look like a page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MinSize {
    /// `auto`
    Auto,
    /// A length or a percentage.
    Length(LengthPercentage),
}

/// A specified `min-width`/`min-height`, before `em` is resolved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SpecifiedMinSize {
    /// `auto`
    Auto,
    /// A length.
    Length(Len),
}

/// `max-width` and `max-height`, CSS 2.2 §10.4 and §10.7.
///
/// `none` and not `auto`: §10.4's own grammar, and the two are different words
/// for a reason — there is no maximum at all, rather than one the layout works
/// out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MaxSize {
    /// `none`
    None,
    /// A length or a percentage.
    Length(LengthPercentage),
}

/// A specified `max-width`/`max-height`, before `em` is resolved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SpecifiedMaxSize {
    /// `none`
    None,
    /// A length.
    Length(Len),
}

/// A margin, which may be `auto` where padding may not.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MarginValue {
    /// `auto`, which is what centres a block.
    Auto,
    /// A length or a percentage.
    Length(LengthPercentage),
}

/// A specified margin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SpecifiedMargin {
    /// `auto`
    Auto,
    /// A length.
    Length(Len),
}

/// Which edge a side-valued property is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// Top.
    Top,
    /// Right.
    Right,
    /// Bottom.
    Bottom,
    /// Left.
    Left,
}

/// The four edges of a box, one value each.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sides<T> {
    /// Top.
    pub top: T,
    /// Right.
    pub right: T,
    /// Bottom.
    pub bottom: T,
    /// Left.
    pub left: T,
}

impl<T: Copy> Sides<T> {
    /// The same value on all four edges.
    pub fn all(value: T) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }

    /// Sets one edge. The `match` is exhaustive over [`Side`], so a fifth edge
    /// would not compile either.
    pub fn set(&mut self, side: Side, value: T) {
        match side {
            Side::Top => self.top = value,
            Side::Right => self.right = value,
            Side::Bottom => self.bottom = value,
            Side::Left => self.left = value,
        }
    }

    /// Reads one edge.
    pub fn get(&self, side: Side) -> T {
        match side {
            Side::Top => self.top,
            Side::Right => self.right,
            Side::Bottom => self.bottom,
            Side::Left => self.left,
        }
    }
}

/// `display`, at the sixteen values this build lays out.
///
/// `grid`, `contents`, `run-in` and the rest are `Unsupported` **by name and
/// by value**, which is the whole of device 2: mapping `display: grid` onto
/// `block` produces a page that looks entirely reasonable and is wrong.
///
/// # `flex` and `inline-flex` arrived together, and they had to
///
/// `css-flexbox-1` §3 defines the two as *"the same layout inside, a different
/// outside"*: both establish a flex formatting context and differ only in
/// whether the container itself is block-level or inline-level. A build with
/// one and not the other would have to map the missing one onto something,
/// and the only candidates are the value it is not and `block` — which is
/// device 2's own example of the wrong answer.
///
/// # The nine table values arrived together, and they had to
///
/// CSS 2.2 §17.2's box tree is not nine independent values: §17.2.1 *generates*
/// the missing ones, and a build that had `table-cell` and not `table-row`
/// could not perform the generation at all. Adding them one at a time would
/// mean a build in which a `<td>` is a cell of nothing.
///
/// `inline-table` is **not** here and its absence is the same decision read the
/// other way. It is an inline-level table, which is a table box in an inline
/// formatting context, and this build has no inline-level box that is not text;
/// mapping it onto [`Display::Table`] would put a table on a line of its own
/// and the page would look entirely reasonable. So it is a value of an
/// implemented property that this build does not take, which
/// [`Implemented::BadValue`] reports by name — the same answer `float:
/// inline-start` gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Display {
    /// `inline`
    Inline,
    /// `block`
    Block,
    /// `inline-block`
    InlineBlock,
    /// `list-item`
    ListItem,
    /// `none`
    None,
    /// `table`, CSS 2.2 §17.2.
    Table,
    /// `table-row-group`
    TableRowGroup,
    /// `table-header-group`
    TableHeaderGroup,
    /// `table-footer-group`
    TableFooterGroup,
    /// `table-row`
    TableRow,
    /// `table-cell`
    TableCell,
    /// `table-column`
    TableColumn,
    /// `table-column-group`
    TableColumnGroup,
    /// `table-caption`
    TableCaption,
    /// `flex`, `css-flexbox-1` §3. A block-level flex container.
    Flex,
    /// `inline-flex`, §3. The same formatting context, inline-level outside.
    InlineFlex,
}

impl Display {
    /// Whether this is one of §17.2's internal table values — everything a
    /// table box may contain, and nothing that may stand on its own.
    ///
    /// §17.2.1's generation rules are stated over exactly this set, which is
    /// why it is a predicate here rather than a `matches!` at each of its four
    /// callers.
    #[must_use]
    pub fn is_internal_table(self) -> bool {
        matches!(
            self,
            Display::TableRowGroup
                | Display::TableHeaderGroup
                | Display::TableFooterGroup
                | Display::TableRow
                | Display::TableCell
                | Display::TableColumn
                | Display::TableColumnGroup
                | Display::TableCaption
        )
    }

    /// Whether this is one of the three row-group values, which §17.2 treats
    /// alike everywhere but ordering.
    #[must_use]
    pub fn is_row_group(self) -> bool {
        matches!(
            self,
            Display::TableRowGroup | Display::TableHeaderGroup | Display::TableFooterGroup
        )
    }

    /// Whether this value establishes a **flex formatting context**,
    /// `css-flexbox-1` §3.
    ///
    /// The two values differ in what the container is *outside* — which is
    /// [`Display`]'s block-level question and is asked elsewhere — and are the
    /// same thing inside, which is what this predicate is for.
    #[must_use]
    pub fn is_flex_container(self) -> bool {
        matches!(self, Display::Flex | Display::InlineFlex)
    }
}

/// `flex-direction`, `css-flexbox-1` §5.1.
///
/// **Four values and not two.** The reverse pair is not a styling flourish: §5.1
/// makes `row-reverse` swap the *main-start* and *main-end* edges, so
/// `justify-content: flex-start` puts the first item on the **right**. A build
/// with `row` and `column` only would lay a reversed container out forwards and
/// the page would look entirely reasonable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlexDirection {
    /// `row`. The initial value: main axis is inline, main-start is the left.
    Row,
    /// `row-reverse`. Main axis is inline, main-start is the right.
    RowReverse,
    /// `column`. Main axis is block, main-start is the top.
    Column,
    /// `column-reverse`. Main axis is block, main-start is the bottom.
    ColumnReverse,
}

impl FlexDirection {
    /// Whether the main axis is the inline one, §5.1.
    #[must_use]
    pub fn is_row(self) -> bool {
        matches!(self, FlexDirection::Row | FlexDirection::RowReverse)
    }

    /// Whether main-start is the far end of the axis, §5.1.
    #[must_use]
    pub fn is_reversed(self) -> bool {
        matches!(
            self,
            FlexDirection::RowReverse | FlexDirection::ColumnReverse
        )
    }
}

/// `flex-wrap`, `css-flexbox-1` §5.2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlexWrap {
    /// `nowrap`. The initial value: one line, whatever it costs.
    NoWrap,
    /// `wrap`. Lines stack towards cross-end.
    Wrap,
    /// `wrap-reverse`. Lines stack towards cross-start, which is §5.2's own
    /// wording: the cross-start and cross-end directions are swapped.
    WrapReverse,
}

impl FlexWrap {
    /// Whether the container is multi-line, §9.3.
    #[must_use]
    pub fn wraps(self) -> bool {
        matches!(self, FlexWrap::Wrap | FlexWrap::WrapReverse)
    }
}

/// `justify-content`, `css-align-3` as `css-flexbox-1` §8.2 uses it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JustifyContent {
    /// `flex-start`. The initial value.
    FlexStart,
    /// `flex-end`
    FlexEnd,
    /// `center`
    Center,
    /// `space-between`
    SpaceBetween,
    /// `space-around`
    SpaceAround,
    /// `space-evenly`
    SpaceEvenly,
}

/// `align-items`, `css-flexbox-1` §8.3 — the container's default cross-axis
/// alignment for its items.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignItems {
    /// `flex-start`
    FlexStart,
    /// `flex-end`
    FlexEnd,
    /// `center`
    Center,
    /// `baseline`, §8.3: the items' first baselines are aligned.
    Baseline,
    /// `stretch`. The initial value.
    Stretch,
}

/// `align-self`, §8.3 — one item's own answer, or `auto` to take the
/// container's.
///
/// **A separate type from [`AlignItems`] and not the same one with a spare
/// variant.** `auto` is not an alignment: §8.3 makes it *"the value of the
/// parent's `align-items`"*, so a build that stored it as an alignment would
/// have to resolve it at every reader or silently pick one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignSelf {
    /// `auto`. The initial value: defer to the container's `align-items`.
    Auto,
    /// `flex-start`
    FlexStart,
    /// `flex-end`
    FlexEnd,
    /// `center`
    Center,
    /// `baseline`
    Baseline,
    /// `stretch`
    Stretch,
}

impl AlignSelf {
    /// §8.3's resolution: `auto` computes to the container's `align-items`.
    #[must_use]
    pub fn resolve(self, container: AlignItems) -> AlignItems {
        match self {
            AlignSelf::Auto => container,
            AlignSelf::FlexStart => AlignItems::FlexStart,
            AlignSelf::FlexEnd => AlignItems::FlexEnd,
            AlignSelf::Center => AlignItems::Center,
            AlignSelf::Baseline => AlignItems::Baseline,
            AlignSelf::Stretch => AlignItems::Stretch,
        }
    }
}

/// `align-content`, `css-flexbox-1` §8.4 — how the **lines** are distributed in
/// the cross axis.
///
/// **Not the same question as `align-items`**, and §8.4 says so in its first
/// sentence: this one *"has no effect on a single-line flex container"*. A
/// build that folded the two together would move every item in a `nowrap`
/// container the moment an author wrote `align-content: center`, which is a
/// declaration every browser ignores.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignContent {
    /// `flex-start`
    FlexStart,
    /// `flex-end`
    FlexEnd,
    /// `center`
    Center,
    /// `space-between`
    SpaceBetween,
    /// `space-around`
    SpaceAround,
    /// `stretch`. The initial value.
    Stretch,
}

/// `border-collapse`, CSS 2.2 §17.6.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BorderCollapse {
    /// `separate`, §17.6.1. The initial value.
    Separate,
    /// `collapse`, §17.6.2.
    Collapse,
}

/// `table-layout`, CSS 2.2 §17.5.2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableLayout {
    /// `auto`, §17.5.2.2. The initial value.
    Auto,
    /// `fixed`, §17.5.2.1.
    Fixed,
}

/// `border-spacing`, CSS 2.2 §17.6.1, computed to two lengths in CSS pixels.
///
/// Two numbers rather than one because the property takes `<length>
/// <length>?`, and the horizontal one spaces columns while the vertical one
/// spaces rows: a build that kept one would put the same gap in both
/// directions, which is right for every fixture written with one value and
/// wrong for every book that writes two.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BorderSpacing {
    /// Between columns, and between the table's left and right border and the
    /// cells beside them.
    pub horizontal: f64,
    /// Between rows, and at the top and bottom.
    pub vertical: f64,
}

impl BorderSpacing {
    /// `0 0`, which is the initial value.
    pub const ZERO: Self = Self {
        horizontal: 0.0,
        vertical: 0.0,
    };
}

/// `float`. `inline-start` and `inline-end` are **not** `left` and `right`:
/// they depend on the writing mode, which this build refuses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Float {
    /// `none`
    None,
    /// `left`
    Left,
    /// `right`
    Right,
}

/// `clear`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Clear {
    /// `none`
    None,
    /// `left`
    Left,
    /// `right`
    Right,
    /// `both`
    Both,
}

/// `box-sizing`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxSizing {
    /// `content-box`
    ContentBox,
    /// `border-box`
    BorderBox,
}

/// `position`, CSS 2.2 §9.3.1 and `css-position-3` §2.
///
/// **All five values, including the three this build does not place.** The
/// (property, value) key would allow `absolute` to be reported as a value of an
/// implemented property instead, and that is the wrong shape here for a reason
/// the rest of this file does not meet: `position` is the property whose
/// *other* longhands — `top`, `right`, `bottom`, `left` and `z-index` — mean
/// nothing without it. A build that refused the value would have to refuse the
/// five longhands with it and could then say nothing at all about the box; a
/// build that cascades the value can say, by name and per box,
/// `tinker_pdf_layout::Warning::PositionedAsStatic`. Which is the difference
/// between a gap counted at the stylesheet and a gap counted on the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Position {
    /// `static`. The initial value: the box is in the normal flow and the
    /// inset properties do not apply to it.
    Static,
    /// `relative`, §9.4.3: laid out in the normal flow, then offset.
    Relative,
    /// `absolute`, §9.6.1: out of flow, against the nearest positioned
    /// ancestor.
    Absolute,
    /// `fixed`, §9.6.1: out of flow, against the viewport.
    Fixed,
    /// `sticky`, `css-position-3` §3.4.
    Sticky,
}

/// `top`, `right`, `bottom` and `left`, CSS 2.2 §9.3.2.
///
/// `auto` is *"the position the box would have had"*, which is a different
/// fact from a zero offset and is why this is not a [`LengthPercentage`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Inset {
    /// `auto`
    Auto,
    /// A length or a percentage — of the containing block's **width** for
    /// `left`/`right` and of its **height** for `top`/`bottom`, §9.3.2.
    Length(LengthPercentage),
}

/// A specified inset, before `em` is resolved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SpecifiedInset {
    /// `auto`
    Auto,
    /// A length.
    Length(Len),
}

/// `z-index`, CSS 2.2 §9.9.1.
///
/// `auto` and an integer are not the same fact: §9.9.1 gives `auto` *"the same
/// stack level as the parent"* and **no new stacking context**, where `0` is
/// the same stack level *and* a new stacking context. Folding them would put
/// every `z-index: 0` box's descendants in the wrong order relative to their
/// uncles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZIndex {
    /// `auto`
    Auto,
    /// An integer, which may be negative.
    Layer(i32),
}

/// `column-count`, `css-multicol-1` §3.2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnCount {
    /// `auto`. The initial value: the count comes from `column-width`.
    Auto,
    /// A positive integer.
    Count(u16),
}

/// `column-width`, `css-multicol-1` §3.1, computed to CSS pixels.
///
/// **No percentage**, which is §3.1's own grammar: the property is
/// `auto | <length [0,∞]>`, and a percentage of a width that the column count
/// then decides is circular. A percentage here is therefore the author's
/// mistake rather than this build's gap, the same answer `border-spacing` gets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColumnWidth {
    /// `auto`
    Auto,
    /// A length in CSS pixels.
    Px(f64),
}

/// A specified `column-width`, before `em` is resolved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SpecifiedColumnWidth {
    /// `auto`
    Auto,
    /// A length.
    Length(Len),
}

/// `column-gap` and `row-gap`, `css-align-3` §8.1.
///
/// **`normal` is not zero and it is not one number either**, which is why it
/// survives to the consumer rather than being computed away here: §8.1 makes it
/// `1em` in a multi-column container and `0` everywhere else, so the value a
/// flex container reads and the value a multi-column container reads are
/// different numbers from the same declaration.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gap {
    /// `normal`
    Normal,
    /// A length or a percentage of the container's own content-box size in
    /// that axis.
    Length(LengthPercentage),
}

/// A specified gap, before `em` is resolved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SpecifiedGap {
    /// `normal`
    Normal,
    /// A length.
    Length(Len),
}

/// `column-span`, `css-multicol-1` §6.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnSpan {
    /// `none`. The initial value.
    None,
    /// `all`: the box spans every column of the container.
    All,
}

/// `column-fill`, `css-multicol-1` §4.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnFill {
    /// `balance`. The initial value.
    Balance,
    /// `auto`: fill each column in turn.
    Auto,
}

/// `vertical-align`, CSS 2.2 §10.8.1 and — for a table cell — §17.5.4.
///
/// **One property, two specifications, and they do not take the same values.**
/// §10.8.1 defines all ten for an inline-level box; §17.5.4 defines four of
/// them for a `table-cell` and says the rest *"are treated as `baseline`"*.
/// Both readings are kept here because the value is a fact about the
/// declaration and which of the two rules applies is a fact about the box,
/// which the cascade does not know and layout does.
///
/// A **percentage** is a percentage of the element's own `line-height` — not of
/// the parent's, and not of anything the containing block owns — so it survives
/// computation and is resolved where the used `line-height` is, which is
/// `tinker_pdf_layout::style::consume`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VerticalAlign {
    /// `baseline`. The initial value.
    Baseline,
    /// `sub`
    Sub,
    /// `super`
    Super,
    /// `top`: the top of the box is the top of the line box.
    Top,
    /// `middle`
    Middle,
    /// `bottom`
    Bottom,
    /// `text-top`: the top of the box is the top of the parent's content area.
    TextTop,
    /// `text-bottom`
    TextBottom,
    /// A length, or a percentage of this element's own `line-height`. Positive
    /// raises the box.
    Length(LengthPercentage),
}

/// A specified `vertical-align`, before `em` is resolved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SpecifiedVerticalAlign {
    /// `baseline`
    Baseline,
    /// `sub`
    Sub,
    /// `super`
    Super,
    /// `top`
    Top,
    /// `middle`
    Middle,
    /// `bottom`
    Bottom,
    /// `text-top`
    TextTop,
    /// `text-bottom`
    TextBottom,
    /// A length or a percentage.
    Length(Len),
}

/// `font-style`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontStyle {
    /// `normal`
    Normal,
    /// `italic`
    Italic,
    /// `oblique`
    Oblique,
}

/// `font-variant`, at the one value CSS 2.1 defines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontVariant {
    /// `normal`
    Normal,
    /// `small-caps`
    SmallCaps,
}

/// A specified `font-weight`, which may be relative to the parent's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecifiedWeight {
    /// A number, 1 to 1000.
    Absolute(u16),
    /// `bolder`
    Bolder,
    /// `lighter`
    Lighter,
}

/// `font-size`, as written.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SpecifiedFontSize {
    /// An absolute length, or an absolute keyword already resolved to one.
    Absolute(f64),
    /// Relative to the parent's computed size: `em`, `%` and `ex` all land
    /// here, because on `font-size` a percentage **is** an em.
    Relative(f64),
    /// `rem`, relative to the root's.
    Root(f64),
    /// `larger`
    Larger,
    /// `smaller`
    Smaller,
}

/// `line-height`.
///
/// A **number** is not a length and the difference is inherited: a number
/// inherits as the factor and is re-multiplied by each descendant's own font
/// size, where a length inherits already resolved. A build that computed a
/// number to pixels at the element that wrote it gets every nested font size
/// wrong, and the page still looks like a page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LineHeight {
    /// `normal`
    Normal,
    /// A factor of the element's own font size.
    Number(f64),
    /// An absolute length.
    Px(f64),
}

/// `letter-spacing` and `word-spacing`, computed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Spacing {
    /// `normal`
    Normal,
    /// An absolute length; may be negative.
    Px(f64),
}

/// `letter-spacing` and `word-spacing`, as written.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SpecifiedSpacing {
    /// `normal`
    Normal,
    /// A length, which may be an `em` — `letter-spacing: 0.1em` is what a book
    /// writes, and resolving it needs the element's own computed font size.
    Length(Len),
}

/// `text-align`, at CSS 2.1's four values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAlign {
    /// `left`
    Left,
    /// `right`
    Right,
    /// `center`
    Center,
    /// `justify`
    Justify,
}

/// `text-decoration`, as the line it draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextDecoration {
    /// `none`
    None,
    /// `underline`
    Underline,
    /// `overline`
    Overline,
    /// `line-through`
    LineThrough,
}

/// `white-space`, at `css-text-3` §3's five values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WhiteSpace {
    /// `normal`
    Normal,
    /// `pre`
    Pre,
    /// `nowrap`
    NoWrap,
    /// `pre-wrap`
    PreWrap,
    /// `pre-line`
    PreLine,
}

/// `list-style-type`, at the markers a book uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListStyleType {
    /// `disc`
    Disc,
    /// `circle`
    Circle,
    /// `square`
    Square,
    /// `decimal`
    Decimal,
    /// `lower-alpha` and `lower-latin`
    LowerAlpha,
    /// `upper-alpha` and `upper-latin`
    UpperAlpha,
    /// `lower-roman`
    LowerRoman,
    /// `upper-roman`
    UpperRoman,
    /// `none`
    None,
}

/// `visibility`, at the two values that are not `collapse`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visibility {
    /// `visible`
    Visible,
    /// `hidden` — laid out, and not painted. Not `display: none`, and a build
    /// that treated them alike would drop the box and move everything after it.
    Hidden,
}

/// `border-*-style`, at the strokes this engine can draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BorderStyle {
    /// `none`
    None,
    /// `hidden`, which is `none` except in a collapsing table.
    Hidden,
    /// `solid`
    Solid,
    /// `dashed`
    Dashed,
    /// `dotted`
    Dotted,
    /// `double`
    Double,
}

/// `page-break-before` and `page-break-after`, CSS 2.2 §13.3.1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageBreak {
    /// `auto`
    Auto,
    /// `always`
    Always,
    /// `avoid`
    Avoid,
    /// `left`
    Left,
    /// `right`
    Right,
}

/// `page-break-inside`, CSS 2.2 §13.3.1.
///
/// Two values and not three: `avoid-page` and the `break-inside` longhand's
/// `avoid-column` are about fragmentation contexts this build has none of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageBreakInside {
    /// `auto`
    Auto,
    /// `avoid`
    Avoid,
}

/// `overflow-wrap`, `css-text-3` §5.4.
///
/// **`break-word` and `anywhere` are two values and not one**, and the
/// difference is not what a first implementation guesses. Both allow a break
/// inside a word that would otherwise overflow; they differ in whether the
/// opportunity counts when a box's *min-content* size is computed — `anywhere`
/// counts and `break-word` does not. This build does not compute min-content
/// sizes at all, so the two behave alike here and the distinction is recorded
/// where it is made rather than collapsed into one variant, because collapsing
/// it is what makes a value silently become its neighbour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverflowWrap {
    /// `normal`
    Normal,
    /// `break-word`
    BreakWord,
    /// `anywhere`
    Anywhere,
}

/// `line-break`, `css-text-3` §5.1.
///
/// `auto` is a real value rather than a synonym for `Normal`: it means *"the
/// UA's own default"*, and a build that computed it away could not later change
/// its default without changing what an author wrote.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineBreakStrictness {
    /// `auto`
    Auto,
    /// `loose`
    Loose,
    /// `normal`
    Normal,
    /// `strict`
    Strict,
    /// `anywhere`
    Anywhere,
}

/// `word-break`, `css-text-3` §5.2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordBreak {
    /// `normal`
    Normal,
    /// `break-all`
    BreakAll,
    /// `keep-all`
    KeepAll,
}

/// One entry of a `font-family` list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FontFamily {
    /// A named face.
    Named(String),
    /// `serif`
    Serif,
    /// `sans-serif`
    SansSerif,
    /// `monospace`
    Monospace,
    /// `cursive`
    Cursive,
    /// `fantasy`
    Fantasy,
}

// ---- the enum decision 5 is about -------------------------------------------

/// A property this build implements, at a value it implements.
///
/// **Every variant here has a consumer.** Two of them, in fact — [`apply`] and
/// [`Property::name`] — and both `match` exhaustively with no `_` arm, so
/// adding a variant without adding both arms is `error[E0004]` rather than a
/// property that is parsed and ignored.
///
/// The four side-valued families carry a [`Side`] rather than getting four
/// variants each. That is not a weakening: [`Sides::set`] matches [`Side`]
/// exhaustively too, so the guard holds one level down, and the (property,
/// value) key is still per-longhand because `margin-top` and `margin-left` are
/// separate declarations by the time they reach here.
#[derive(Clone, Debug, PartialEq)]
pub enum Property {
    /// `color`
    Color(Color),
    /// `font-family`
    FontFamily(Vec<FontFamily>),
    /// `font-size`
    FontSize(SpecifiedFontSize),
    /// `font-style`
    FontStyle(FontStyle),
    /// `font-variant`
    FontVariant(FontVariant),
    /// `font-weight`
    FontWeight(SpecifiedWeight),
    /// `line-height`
    LineHeight(LineHeight),
    /// `letter-spacing`
    LetterSpacing(SpecifiedSpacing),
    /// `word-spacing`
    WordSpacing(SpecifiedSpacing),
    /// `text-align`
    TextAlign(TextAlign),
    /// `text-indent`
    TextIndent(Len),
    /// `text-decoration`
    TextDecoration(TextDecoration),
    /// `white-space`
    WhiteSpace(WhiteSpace),
    /// `list-style-type`
    ListStyleType(ListStyleType),
    /// `visibility`
    Visibility(Visibility),
    /// `display`
    Display(Display),
    /// `float`
    Float(Float),
    /// `clear`
    Clear(Clear),
    /// `box-sizing`
    BoxSizing(BoxSizing),
    /// `width`
    Width(SpecifiedSize),
    /// `height`
    Height(SpecifiedSize),
    /// `margin-*`
    Margin(Side, SpecifiedMargin),
    /// `padding-*`
    Padding(Side, Len),
    /// `border-*-width`
    BorderWidth(Side, Len),
    /// `border-*-style`
    BorderStyle(Side, BorderStyle),
    /// `border-*-color`
    BorderColor(Side, Color),
    /// `background-color`
    BackgroundColor(Color),
    /// `page-break-before`
    PageBreakBefore(PageBreak),
    /// `page-break-after`
    PageBreakAfter(PageBreak),
    /// `page-break-inside`
    PageBreakInside(PageBreakInside),
    /// `orphans`
    Orphans(u16),
    /// `widows`
    Widows(u16),
    /// `overflow-wrap`
    OverflowWrap(OverflowWrap),
    /// `line-break`
    LineBreak(LineBreakStrictness),
    /// `word-break`
    WordBreak(WordBreak),
    /// `border-collapse`
    BorderCollapse(BorderCollapse),
    /// `border-spacing`, horizontal then vertical, before `em` is resolved.
    BorderSpacing(Len, Len),
    /// `table-layout`
    TableLayout(TableLayout),
    /// `flex-direction`
    FlexDirection(FlexDirection),
    /// `flex-wrap`
    FlexWrap(FlexWrap),
    /// `flex-grow`, a non-negative `<number>`.
    FlexGrow(f64),
    /// `flex-shrink`, a non-negative `<number>`.
    FlexShrink(f64),
    /// `flex-basis`, before `em` is resolved.
    FlexBasis(SpecifiedSize),
    /// `justify-content`
    JustifyContent(JustifyContent),
    /// `align-items`
    AlignItems(AlignItems),
    /// `align-self`
    AlignSelf(AlignSelf),
    /// `align-content`
    AlignContent(AlignContent),
    /// `order`, `css-flexbox-1` §5.4, a signed `<integer>`.
    Order(i32),
    /// `min-width`, CSS 2.2 §10.4.
    MinWidth(SpecifiedMinSize),
    /// `max-width`, §10.4.
    MaxWidth(SpecifiedMaxSize),
    /// `min-height`, §10.7.
    MinHeight(SpecifiedMinSize),
    /// `max-height`, §10.7.
    MaxHeight(SpecifiedMaxSize),
    /// `vertical-align`, §10.8.1 and §17.5.4.
    VerticalAlign(SpecifiedVerticalAlign),
    /// `position`, §9.3.1.
    Position(Position),
    /// `top`, `right`, `bottom`, `left`, §9.3.2.
    Inset(Side, SpecifiedInset),
    /// `z-index`, §9.9.1.
    ZIndex(ZIndex),
    /// `column-count`, `css-multicol-1` §3.2.
    ColumnCount(ColumnCount),
    /// `column-width`, §3.1.
    ColumnWidth(SpecifiedColumnWidth),
    /// `column-gap`, `css-align-3` §8.1.
    ColumnGap(SpecifiedGap),
    /// `row-gap`, §8.1.
    RowGap(SpecifiedGap),
    /// `column-rule-width`, `css-multicol-1` §5.1.
    ColumnRuleWidth(Len),
    /// `column-rule-style`, §5.2.
    ColumnRuleStyle(BorderStyle),
    /// `column-rule-color`, §5.3.
    ColumnRuleColor(Color),
    /// `column-span`, §6.
    ColumnSpan(ColumnSpan),
    /// `column-fill`, §4.
    ColumnFill(ColumnFill),
    // <<< the compile-time proof injects a variant directly above this line >>>
}

impl Property {
    /// The property's name, for a warning or a census to carry.
    ///
    /// **The second exhaustive consumer.** It exists as well as [`apply`]
    /// because one `match` is one consequence: a variant added with an `apply`
    /// arm and no name would be applied and then anonymous everywhere it was
    /// reported. `tests/unimplemented_property_does_not_build.rs` injects both
    /// halves separately for exactly that reason.
    pub fn name(&self) -> &'static str {
        match self {
            Property::Color(_) => "color",
            Property::FontFamily(_) => "font-family",
            Property::FontSize(_) => "font-size",
            Property::FontStyle(_) => "font-style",
            Property::FontVariant(_) => "font-variant",
            Property::FontWeight(_) => "font-weight",
            Property::LineHeight(_) => "line-height",
            Property::LetterSpacing(_) => "letter-spacing",
            Property::WordSpacing(_) => "word-spacing",
            Property::TextAlign(_) => "text-align",
            Property::TextIndent(_) => "text-indent",
            Property::TextDecoration(_) => "text-decoration",
            Property::WhiteSpace(_) => "white-space",
            Property::ListStyleType(_) => "list-style-type",
            Property::Visibility(_) => "visibility",
            Property::Display(_) => "display",
            Property::Float(_) => "float",
            Property::Clear(_) => "clear",
            Property::BoxSizing(_) => "box-sizing",
            Property::Width(_) => "width",
            Property::Height(_) => "height",
            Property::Margin(side, _) => match side {
                Side::Top => "margin-top",
                Side::Right => "margin-right",
                Side::Bottom => "margin-bottom",
                Side::Left => "margin-left",
            },
            Property::Padding(side, _) => match side {
                Side::Top => "padding-top",
                Side::Right => "padding-right",
                Side::Bottom => "padding-bottom",
                Side::Left => "padding-left",
            },
            Property::BorderWidth(side, _) => match side {
                Side::Top => "border-top-width",
                Side::Right => "border-right-width",
                Side::Bottom => "border-bottom-width",
                Side::Left => "border-left-width",
            },
            Property::BorderStyle(side, _) => match side {
                Side::Top => "border-top-style",
                Side::Right => "border-right-style",
                Side::Bottom => "border-bottom-style",
                Side::Left => "border-left-style",
            },
            Property::BorderColor(side, _) => match side {
                Side::Top => "border-top-color",
                Side::Right => "border-right-color",
                Side::Bottom => "border-bottom-color",
                Side::Left => "border-left-color",
            },
            Property::BackgroundColor(_) => "background-color",
            Property::PageBreakBefore(_) => "page-break-before",
            Property::PageBreakAfter(_) => "page-break-after",
            Property::PageBreakInside(_) => "page-break-inside",
            Property::Orphans(_) => "orphans",
            Property::Widows(_) => "widows",
            Property::OverflowWrap(_) => "overflow-wrap",
            Property::LineBreak(_) => "line-break",
            Property::WordBreak(_) => "word-break",
            Property::BorderCollapse(_) => "border-collapse",
            Property::BorderSpacing(_, _) => "border-spacing",
            Property::TableLayout(_) => "table-layout",
            Property::FlexDirection(_) => "flex-direction",
            Property::FlexWrap(_) => "flex-wrap",
            Property::FlexGrow(_) => "flex-grow",
            Property::FlexShrink(_) => "flex-shrink",
            Property::FlexBasis(_) => "flex-basis",
            Property::JustifyContent(_) => "justify-content",
            Property::AlignItems(_) => "align-items",
            Property::AlignSelf(_) => "align-self",
            Property::AlignContent(_) => "align-content",
            Property::Order(_) => "order",
            Property::MinWidth(_) => "min-width",
            Property::MaxWidth(_) => "max-width",
            Property::MinHeight(_) => "min-height",
            Property::MaxHeight(_) => "max-height",
            Property::VerticalAlign(_) => "vertical-align",
            Property::Position(_) => "position",
            Property::Inset(side, _) => match side {
                Side::Top => "top",
                Side::Right => "right",
                Side::Bottom => "bottom",
                Side::Left => "left",
            },
            Property::ZIndex(_) => "z-index",
            Property::ColumnCount(_) => "column-count",
            Property::ColumnWidth(_) => "column-width",
            Property::ColumnGap(_) => "column-gap",
            Property::RowGap(_) => "row-gap",
            Property::ColumnRuleWidth(_) => "column-rule-width",
            Property::ColumnRuleStyle(_) => "column-rule-style",
            Property::ColumnRuleColor(_) => "column-rule-color",
            Property::ColumnSpan(_) => "column-span",
            Property::ColumnFill(_) => "column-fill",
            // <<< the compile-time proof's second arm goes here >>>
        }
    }

    /// Whether this property inherits, `css-cascade-5` §7.2.
    ///
    /// A third exhaustive consumer, and it is the one that decides what a
    /// child starts from. It is written as a `match` on `self` rather than as a
    /// table keyed by name so that it, too, fails to build for a new variant.
    pub fn inherited(&self) -> bool {
        match self {
            Property::Color(_)
            | Property::FontFamily(_)
            | Property::FontSize(_)
            | Property::FontStyle(_)
            | Property::FontVariant(_)
            | Property::FontWeight(_)
            | Property::LineHeight(_)
            | Property::LetterSpacing(_)
            | Property::WordSpacing(_)
            | Property::TextAlign(_)
            | Property::TextIndent(_)
            | Property::WhiteSpace(_)
            | Property::ListStyleType(_)
            | Property::Visibility(_)
            | Property::Orphans(_)
            | Property::Widows(_)
            | Property::OverflowWrap(_)
            | Property::LineBreak(_)
            | Property::WordBreak(_)
            // CSS 2.2 §17.6 and §17.6.1 both say *inherited: yes*, and the
            // reason is the box tree §17.2.1 builds rather than a convention:
            // the two properties are declared on the **table** and are read by
            // every cell in it, and an anonymous table box generated by the
            // fixup has no declaration of its own to read. A build that did not
            // inherit them would give an author-written `table { border-collapse:
            // collapse }` to the table and separate borders to every cell under
            // it, which draws a plausible table with two border models in it.
            | Property::BorderCollapse(_)
            | Property::BorderSpacing(_, _) => true,
            Property::TextDecoration(_)
            | Property::Display(_)
            | Property::Float(_)
            | Property::Clear(_)
            | Property::BoxSizing(_)
            | Property::Width(_)
            | Property::Height(_)
            | Property::Margin(_, _)
            | Property::Padding(_, _)
            | Property::BorderWidth(_, _)
            | Property::BorderStyle(_, _)
            | Property::BorderColor(_, _)
            | Property::BackgroundColor(_)
            | Property::PageBreakBefore(_)
            | Property::PageBreakAfter(_)
            // `page-break-inside` is the one row here that disagrees with the
            // specification the rest of this family is taken from, and the
            // disagreement is deliberate rather than a slip. CSS 2.2 §13.3.1's
            // own table says *inherited: yes*; `css-break-3` §4.1 defines
            // `break-inside` as **not** inherited and makes `page-break-inside`
            // a legacy alias of it, and gap 31's plan says in as many words
            // that it *"treats the `break-*` longhands as the modern spelling
            // of the same thing"*. Inheriting it would mean one
            // `page-break-inside: avoid` on `body` — which a real book writes
            // on a figure, and which cascades from wherever it is written —
            // silently forbidding every page break in the book, and a book that
            // cannot be broken is one enormous page rather than a visible
            // failure. `page-break-before` and `page-break-after` are already
            // not inherited two lines above, so this is also the answer that
            // keeps the family consistent.
            | Property::PageBreakInside(_)
            // §17.5.2's own table says *inherited: no*, and it is the one of
            // the three that is genuinely a property of one box: a table
            // nested in a `table-layout: fixed` table has its own algorithm.
            | Property::TableLayout(_)
            // `css-flexbox-1`'s own property tables say *inherited: no* for
            // every one of the ten, and the reason is the same one `display`
            // has: they describe a box's participation in **one** formatting
            // context. An inherited `flex-grow` would make every descendant of
            // a flexible item flexible in a context it is not in — and the
            // descendant is not a flex item at all, so nothing would ever read
            // it back out. `order` is the one worth naming twice: §5.4 makes it
            // *"a value for the box's ordinal group"*, and inheriting it would
            // reorder a paragraph's `<em>` against its siblings.
            | Property::FlexDirection(_)
            | Property::FlexWrap(_)
            | Property::FlexGrow(_)
            | Property::FlexShrink(_)
            | Property::FlexBasis(_)
            | Property::JustifyContent(_)
            | Property::AlignItems(_)
            | Property::AlignSelf(_)
            | Property::AlignContent(_)
            | Property::Order(_)
            // CSS 2.2 §10.4's and §10.7's own tables say *inherited: no* for
            // all four, and it is the answer that keeps them consistent with
            // `width` and `height` three families up: a maximum is a constraint
            // on **one** box's used size, and an inherited `max-width: 100%`
            // would re-clamp at every descendant against a containing block
            // that has already been clamped once.
            | Property::MinWidth(_)
            | Property::MaxWidth(_)
            | Property::MinHeight(_)
            | Property::MaxHeight(_)
            // §10.8.1's own table says *inherited: no*, and this is the row
            // most often got wrong -- `vertical-align` reads like a text
            // property and is not one. It aligns **this** box against its
            // parent's baseline, so inheriting it would apply the offset again
            // at every level and a `<sup>` inside a `<sup>` would climb off the
            // line. §17.5.4's table cell is the same answer for the same
            // reason, which is why calibre writes `vertical-align: inherit` on
            // every cell rather than relying on the cascade.
            | Property::VerticalAlign(_)
            // §9.3.1, §9.3.2 and §9.9.1: all three tables say *inherited: no*.
            // A box's position is a fact about that box's relationship to its
            // containing block, and an inherited `position: absolute` would
            // take every descendant out of the flow as well.
            | Property::Position(_)
            | Property::Inset(_, _)
            | Property::ZIndex(_)
            // `css-multicol-1`'s five and `css-align-3` §8.1's two: every one
            // of the seven property tables says *inherited: no*. They describe
            // a box's own **multi-column formatting context**, which is
            // `display`'s answer again -- a paragraph inside a two-column
            // container is not itself a two-column container.
            | Property::ColumnCount(_)
            | Property::ColumnWidth(_)
            | Property::ColumnGap(_)
            | Property::RowGap(_)
            | Property::ColumnRuleWidth(_)
            | Property::ColumnRuleStyle(_)
            | Property::ColumnRuleColor(_)
            | Property::ColumnSpan(_)
            | Property::ColumnFill(_) => false,
            // <<< the compile-time proof's third arm goes here >>>
        }
    }
}

/// One declaration, split three ways.
///
/// The names are gap 31's decision 5 and the split is not decoration:
/// `Unsupported` is **this build's own gap**, named, and is the number an
/// `As built` is judged on; `Unknown` is somebody else's vendor extension and
/// is ordinary.
#[derive(Clone, Debug, PartialEq)]
pub enum Declaration {
    /// A property this build implements, at a value it implements.
    Known(Property),
    /// A property this build knows the name of and does not implement, or one
    /// it implements at a value it does not.
    Unsupported {
        /// The name, from [`UNSUPPORTED_PROPERTIES`] or from the implemented
        /// set — a `&'static str` so a census cannot report a name the build
        /// does not know it has.
        property: &'static str,
        /// The value as written, so a warning can say which value it was.
        value: String,
    },
    /// One longhand set to one of §7.1's five defaulting keywords.
    ///
    /// One per longhand rather than one per declaration, so that `margin:
    /// inherit` cascades as four declarations exactly as `margin: 0` does --
    /// a `margin-top` written after it has to be able to beat one of them and
    /// not the other three.
    Defaulted {
        /// Which property.
        longhand: crate::longhand::Longhand,
        /// Which keyword.
        keyword: Defaulting,
    },
    /// `content`, on whatever selector carried it.
    ///
    /// Kept even when the selector has no pseudo-element. CSS 2.1 §12.2 makes
    /// `content` apply to `::before` and `::after` only, so `p { content: "x" }`
    /// is a declaration that legitimately does nothing -- which is not the same
    /// as one this build cannot read, and reporting it as a gap would be
    /// reporting a gap this build does not have.
    Content(ContentValue),
    /// A name no CSS specification this build cites defines: a typo, a vendor
    /// extension, or a custom property.
    Unknown {
        /// The name as written.
        property: String,
    },
}

/// What [`parse_declaration`] decided.
#[derive(Clone, Debug, PartialEq)]
pub enum Parsed {
    /// One or more longhands. A shorthand expands here rather than in the
    /// cascade, because each longhand cascades independently — a build that
    /// kept `margin` whole would let one `margin-top` lose to a `margin`
    /// shorthand it should have beaten.
    Known(Vec<Property>),
    /// Decision 5's own gap.
    Unsupported {
        /// The property's name.
        property: &'static str,
        /// The value as written.
        value: String,
    },
    /// §7.1's explicit defaulting, on one longhand or on every longhand a
    /// shorthand sets.
    ///
    /// The keyword travels with the *names* and not with a value, because it
    /// has none: what it resolves to is decided in the cascade, where the
    /// parent's computed style and the origins below this one are in hand.
    Defaulted {
        /// The longhands the keyword applies to, already expanded.
        longhands: Vec<crate::longhand::Longhand>,
        /// Which of the five.
        keyword: Defaulting,
    },
    /// `content`, which is cascaded on its own because it is not a
    /// [`Property`]. See [`ContentValue`].
    Content(ContentValue),
    /// Not a name this build cites.
    Unknown,
    /// A property this build **does** implement, whose value is not valid CSS
    /// at all. §5.4.4 discards it exactly as it discards a syntactically
    /// malformed declaration, and it is counted there rather than here: it is
    /// not a gap in this build.
    Invalid,
}

/// Properties this build knows the name of and does **not** implement.
///
/// Every name is from a specification this crate cites. Milestone 1's census
/// measured eighty-four distinct property names across the fetched corpus's
/// fifty-three stylesheets and forty-two across the committed corpus's eight,
/// and this list is what turns the difference between that and the implemented
/// set into a number instead of a shrug.
///
/// A name in neither list is [`Declaration::Unknown`], which is where
/// `-webkit-column-count`, `-epub-text-emphasis-style` and Antenna House's
/// `-ah-margin-start` land — all three measured in real books.
pub const UNSUPPORTED_PROPERTIES: &[&str] = &[
    "animation",
    "background-attachment",
    "background-image",
    "background-position",
    "background-repeat",
    "background-size",
    "border-image",
    "border-radius",
    "box-shadow",
    "break-after",
    "break-before",
    "break-inside",
    "caption-side",
    "clip",
    "clip-path",
    "color-scheme",
    "counter-increment",
    "counter-reset",
    "cursor",
    "direction",
    "empty-cells",
    "filter",
    "font",
    "font-display",
    "font-feature-settings",
    "font-kerning",
    "font-stretch",
    "font-variant-numeric",
    "grid",
    "grid-area",
    "grid-column",
    "grid-row",
    "grid-template",
    "grid-template-areas",
    "grid-template-columns",
    "grid-template-rows",
    "hyphens",
    "justify-items",
    "justify-self",
    "list-style",
    "list-style-image",
    "list-style-position",
    "mix-blend-mode",
    "opacity",
    "outline",
    "outline-color",
    "outline-offset",
    "outline-style",
    "outline-width",
    "overflow",
    "overflow-x",
    "overflow-y",
    "page",
    "quotes",
    "resize",
    "speak",
    "src",
    "tab-size",
    "text-emphasis",
    "text-emphasis-style",
    "text-overflow",
    "text-shadow",
    "text-transform",
    "transform",
    "transform-origin",
    "transition",
    "unicode-bidi",
    "unicode-range",
    "word-wrap",
    "writing-mode",
];

// ---- parsing ----------------------------------------------------------------

/// Turns one declaration into decision 5's three-way split.
pub fn parse_declaration(name: &str, values: &[ComponentValue]) -> Parsed {
    let significant: Vec<&ComponentValue> = values.iter().filter(|v| !v.is_whitespace()).collect();
    if significant.is_empty() {
        return Parsed::Invalid;
    }
    // Before the defaulting branch, because `content` is the one implemented
    // name with no `ComputedStyle` field and therefore nothing for §7.1's
    // keywords to read or write. [`parse_content`] refuses them by name.
    if name == "content" {
        return parse_content(values, &significant);
    }
    // `inherit`, `initial`, `unset`, `revert` and `revert-layer` are
    // `css-cascade-5` §7.1's explicit defaulting keywords, valid on **every**
    // property and on every shorthand. They are read here, before the value
    // grammars, because §7.1 makes them valid *instead of* a property's own
    // syntax rather than as part of it -- `float: inherit` is not a `float`
    // value and a build that asked the `float` grammar first would discard it.
    //
    // They are only a defaulting keyword when they are the **whole** value:
    // `margin: 0 inherit` is not §7.1 and is invalid, which is what the length
    // check is for.
    if significant.len() == 1 {
        if let Some(Token::Ident(word)) = significant[0].token() {
            let lower = word.to_ascii_lowercase();
            if let Some(keyword) = Defaulting::from_name(&lower) {
                if let Some(longhands) = defaultable(name) {
                    return Parsed::Defaulted { longhands, keyword };
                }
            }
        }
    }
    match implemented(name, values, &significant) {
        Some(Implemented::Known(properties)) => return Parsed::Known(properties),
        Some(Implemented::BadValue) => {
            return Parsed::Unsupported {
                property: implemented_name(name).unwrap_or("?"),
                value: serialize(values),
            }
        }
        Some(Implemented::Malformed) => return Parsed::Invalid,
        None => {}
    }
    if let Some(known) = UNSUPPORTED_PROPERTIES.iter().find(|p| **p == name) {
        return Parsed::Unsupported {
            property: known,
            value: serialize(values),
        };
    }
    Parsed::Unknown
}

/// The longhands a name defaults, whether it is one or a shorthand for several.
///
/// `None` for a name this build does not implement, which leaves the keyword to
/// fall through to [`UNSUPPORTED_PROPERTIES`] and be counted there -- `zoom:
/// inherit` is a gap in `zoom` and not a gap in defaulting.
fn defaultable(name: &str) -> Option<Vec<crate::longhand::Longhand>> {
    if let Some(one) = crate::longhand::Longhand::from_name(name) {
        return Some(vec![one]);
    }
    let (_, names) = DEFAULTABLE_SHORTHANDS.iter().find(|(n, _)| *n == name)?;
    Some(
        names
            .iter()
            .filter_map(|n| crate::longhand::Longhand::from_name(n))
            .collect(),
    )
}

/// `css-cascade-5` §7.1's five explicit defaulting keywords.
///
/// Five keywords and **five different answers**, which is the whole reason
/// they are one enum and not one boolean. The two pairs that look alike are
/// the ones that are not:
///
/// * `unset` is not a third thing beside `inherit` and `initial`: §7.1 defines
///   it as *whichever of the two* [`crate::longhand::Longhand::inherited`]
///   names. A build that mapped it onto either one alone is right on half the
///   properties.
/// * `revert` and `revert-layer` differ in **what they roll back past**.
///   `revert` drops a whole cascade origin -- an author `revert` is resolved as
///   though the author stylesheet had said nothing about this property on this
///   element -- and `revert-layer` drops one `@layer` inside the origin it is
///   already in. Confusing them gives a book that renders, with the wrong
///   value, and nothing anywhere to say so.
///
/// None of the five carries a value, which is why they are resolved against a
/// [`crate::longhand::Longhand`] rather than folded into [`Property`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Defaulting {
    /// `inherit`: the parent's computed value, whether or not the property
    /// inherits by default.
    Inherit,
    /// `initial`: the property's initial value, whether or not it inherits.
    Initial,
    /// `unset`: `inherit` for an inherited property, `initial` otherwise.
    Unset,
    /// `revert`: roll back to the previous cascade **origin**.
    Revert,
    /// `revert-layer`: roll back to the previous cascade **layer** within this
    /// origin, and to the previous origin when there is no earlier layer.
    RevertLayer,
}

impl Defaulting {
    /// The keyword as a stylesheet writes it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Defaulting::Inherit => "inherit",
            Defaulting::Initial => "initial",
            Defaulting::Unset => "unset",
            Defaulting::Revert => "revert",
            Defaulting::RevertLayer => "revert-layer",
        }
    }

    /// Reads §7.1's keyword, case-insensitively as CSS keywords are.
    #[must_use]
    pub fn from_name(word: &str) -> Option<Defaulting> {
        match word {
            "inherit" => Some(Defaulting::Inherit),
            "initial" => Some(Defaulting::Initial),
            "unset" => Some(Defaulting::Unset),
            "revert" => Some(Defaulting::Revert),
            "revert-layer" => Some(Defaulting::RevertLayer),
            _ => None,
        }
    }
}

/// The longhands each implemented shorthand sets, for defaulting only.
///
/// `margin: inherit` means all four margins inherit, so a defaulting keyword
/// on a shorthand has to expand exactly as a value on it does. This table is
/// **not** the expansion `implemented` performs -- that one parses values and
/// this one only needs names -- and the two are asserted to agree in
/// `every_shorthand_expands_the_same_way_for_a_value_and_a_keyword`, which
/// parses a real value through the ordinary path and compares the longhand set.
/// Two tables that must agree are worth one test; two tables that quietly
/// disagree are `border: inherit` leaving the border colour behind.
pub const DEFAULTABLE_SHORTHANDS: &[(&str, &[&str])] = &[
    ("background", &["background-color"]),
    (
        "border",
        &[
            "border-top-width",
            "border-right-width",
            "border-bottom-width",
            "border-left-width",
            "border-top-style",
            "border-right-style",
            "border-bottom-style",
            "border-left-style",
            "border-top-color",
            "border-right-color",
            "border-bottom-color",
            "border-left-color",
        ],
    ),
    (
        "border-bottom",
        &[
            "border-bottom-width",
            "border-bottom-style",
            "border-bottom-color",
        ],
    ),
    (
        "border-color",
        &[
            "border-top-color",
            "border-right-color",
            "border-bottom-color",
            "border-left-color",
        ],
    ),
    (
        "border-left",
        &[
            "border-left-width",
            "border-left-style",
            "border-left-color",
        ],
    ),
    (
        "border-right",
        &[
            "border-right-width",
            "border-right-style",
            "border-right-color",
        ],
    ),
    (
        "border-style",
        &[
            "border-top-style",
            "border-right-style",
            "border-bottom-style",
            "border-left-style",
        ],
    ),
    (
        "border-top",
        &["border-top-width", "border-top-style", "border-top-color"],
    ),
    (
        "border-width",
        &[
            "border-top-width",
            "border-right-width",
            "border-bottom-width",
            "border-left-width",
        ],
    ),
    (
        "column-rule",
        &[
            "column-rule-width",
            "column-rule-style",
            "column-rule-color",
        ],
    ),
    ("columns", &["column-width", "column-count"]),
    ("flex", &["flex-grow", "flex-shrink", "flex-basis"]),
    ("flex-flow", &["flex-direction", "flex-wrap"]),
    ("gap", &["row-gap", "column-gap"]),
    (
        "margin",
        &["margin-top", "margin-right", "margin-bottom", "margin-left"],
    ),
    (
        "padding",
        &[
            "padding-top",
            "padding-right",
            "padding-bottom",
            "padding-left",
        ],
    ),
];

/// `css-content-3` §2's `content`, to the part a paginated reader needs.
///
/// # It is not a [`Property`], and that is the design rather than an omission
///
/// Every other implemented property is a `Property` variant with a field in
/// `ComputedStyle`, because every other one is a thing a *box* has. `content`
/// is not: CSS 2.1 §12.2 applies it to `::before` and `::after` and to nothing
/// else, so a field on `ComputedStyle` would sit unread on every element in
/// every book, and `tinker-pdf-layout`'s `style::consume` would have to
/// destructure a field that means nothing to it.
///
/// So it is cascaded on its own, in `cascade::Matcher::pseudo_winners`, and
/// what comes out is a `String` rather than a value: `attr()` needs the
/// originating element, the cascade has it, and nothing downstream should have
/// to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContentValue {
    /// `normal` on a pseudo-element computes to `none` (§2.1), and `none`
    /// generates no box. One variant for both, because they differ only on
    /// elements that are not pseudo-elements, where neither does anything.
    None,
    /// A list to concatenate, in order.
    Items(Vec<ContentItem>),
}

/// One piece of a `content` value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContentItem {
    /// A quoted string, with its escapes already resolved.
    Text(String),
    /// `attr(name)`: the originating element's attribute, or the empty string
    /// when it has none -- which is §2.4's own fallback and not a guess.
    Attr(String),
}

/// What this build reads inside `content`, and what it refuses.
///
/// Refused **by name**, as `Unsupported { property: "content", value }`, so a
/// book that asks for one is counted rather than silently given an empty box:
///
/// * `<image>` / `url()` -- generated content that is a replaced element. The
///   box would need a size before the image is fetched, which is a different
///   layout question from the one this closes.
/// * `counter()` / `counters()` -- these need `counter-reset` and
///   `counter-increment`, a scoped counter tree, and §4's nesting rules. None
///   of the three is here, and a `counter()` resolved to nothing would number
///   every list item zero.
/// * `open-quote` / `close-quote` / `no-open-quote` / `no-close-quote` -- these
///   read the `quotes` property, which is still in [`UNSUPPORTED_PROPERTIES`]
///   and which pandoc writes. Guessing `"` would be wrong in every language
///   that does not use it.
///
/// The five §7.1 defaulting keywords are refused here too, and for a reason
/// worth stating: `content` has no `ComputedStyle` field, so there is nothing
/// for `inherit` to read and nothing for `copy_computed` to write. A build that
/// accepted them would have to invent a storage location for a value that has
/// none.
fn parse_content(values: &[ComponentValue], significant: &[&ComponentValue]) -> Parsed {
    let refuse = || Parsed::Unsupported {
        property: "content",
        value: serialize(values),
    };
    if significant.len() == 1 {
        if let Some(Token::Ident(word)) = significant[0].token() {
            let lower = word.to_ascii_lowercase();
            if lower == "none" || lower == "normal" {
                return Parsed::Content(ContentValue::None);
            }
            if Defaulting::from_name(&lower).is_some() {
                return refuse();
            }
        }
    }

    let mut items = Vec::new();
    for value in significant {
        match value {
            ComponentValue::Token(Token::Str(text)) => {
                items.push(ContentItem::Text(text.clone()));
            }
            ComponentValue::Function { name, arguments } if name.eq_ignore_ascii_case("attr") => {
                let inner: Vec<&ComponentValue> =
                    arguments.iter().filter(|v| !v.is_whitespace()).collect();
                match inner.as_slice() {
                    [ComponentValue::Token(Token::Ident(attribute))] => {
                        items.push(ContentItem::Attr(attribute.to_ascii_lowercase()));
                    }
                    // §2.4 allows a type and a fallback; neither is read here,
                    // and a build that ignored them would take `attr(x px, 1)`
                    // for `attr(x)` and put a number where a length belongs.
                    _ => return refuse(),
                }
            }
            // Every other component value is one of the three refusals above,
            // or something no specification this build cites defines.
            _ => return refuse(),
        }
    }
    if items.is_empty() {
        // `content:` with nothing after it is not §2's grammar at all, and
        // §5.4.4 discards it as it discards any malformed declaration.
        return Parsed::Invalid;
    }
    Parsed::Content(ContentValue::Items(items))
}

/// Reading a value that is supposed to be a length, three ways.
///
/// The split is what keeps the `Unsupported` census honest, and it is the
/// distinction a first implementation collapses. `width: 50vw` names a unit
/// `css-values-3` defines and this build refuses — **this build's gap**, and
/// the number an `As built` is judged on. `width: red` is not CSS at all —
/// **the author's typo**, discarded by §5.4.4 like any other malformed
/// declaration and not a gap in anything here. A build that reported both as
/// gaps would inflate the one figure the whole milestone is measured by, in
/// the flattering direction for the author and the damning one for this
/// engine.
enum LenOutcome {
    /// A length this build resolves.
    Ok(Len),
    /// A unit this build does not implement.
    Unsupported,
    /// Not a length at all.
    Invalid,
}

fn length_outcome(value: &ComponentValue) -> LenOutcome {
    match value {
        ComponentValue::Token(Token::Number { value, .. }) if *value == 0.0 => {
            LenOutcome::Ok(Len::Px(0.0))
        }
        ComponentValue::Token(Token::Percentage(percent)) => LenOutcome::Ok(Len::Percent(*percent)),
        ComponentValue::Token(Token::Dimension { value, unit }) => {
            match unit_to_len(*value, &unit.to_ascii_lowercase()) {
                Some(len) => LenOutcome::Ok(len),
                None => LenOutcome::Unsupported,
            }
        }
        _ => LenOutcome::Invalid,
    }
}

/// Reading a `<color>`, three ways, on the same principle.
///
/// A **name** outside this build's table is `Unsupported`: `rebeccapurple` is a
/// real colour and `currentColor` is a real keyword, and both are gaps here. A
/// colour **function** this build does not have — `lab()`, `color-mix()`,
/// `var()` — is `Unsupported` too. But `rgb(1;2;3)` is a function this build
/// does have, given something that is not CSS, and that is the author's.
enum ColourOutcome {
    Ok(Color),
    Unsupported,
    Invalid,
}

fn colour_outcome(value: &ComponentValue) -> ColourOutcome {
    match value {
        ComponentValue::Token(Token::Ident(_)) => match color(value) {
            Some(colour) => ColourOutcome::Ok(colour),
            None => ColourOutcome::Unsupported,
        },
        ComponentValue::Token(Token::Hash(_, _)) => match color(value) {
            Some(colour) => ColourOutcome::Ok(colour),
            None => ColourOutcome::Invalid,
        },
        ComponentValue::Function { name, .. } => {
            let lower = name.to_ascii_lowercase();
            if matches!(lower.as_str(), "rgb" | "rgba" | "hsl" | "hsla") {
                match color(value) {
                    Some(colour) => ColourOutcome::Ok(colour),
                    None => ColourOutcome::Invalid,
                }
            } else {
                ColourOutcome::Unsupported
            }
        }
        _ => ColourOutcome::Invalid,
    }
}

/// One colour value for a colour-valued property.
fn colour_property(
    value: Option<&ComponentValue>,
    single: bool,
    build: impl Fn(Color) -> Property,
) -> Implemented {
    let Some(value) = value else {
        return Implemented::Malformed;
    };
    if !single {
        return Implemented::Malformed;
    }
    match colour_outcome(value) {
        ColourOutcome::Ok(colour) => Implemented::Known(vec![build(colour)]),
        ColourOutcome::Unsupported => Implemented::BadValue,
        ColourOutcome::Invalid => Implemented::Malformed,
    }
}

/// One length value for a length-valued property, with an optional keyword in
/// front of it.
fn length_property(
    value: Option<&ComponentValue>,
    single: bool,
    keyword: impl Fn(&str) -> Option<Property>,
    build: impl Fn(Len) -> Property,
) -> Implemented {
    let Some(value) = value else {
        return Implemented::Malformed;
    };
    if !single {
        return Implemented::Malformed;
    }
    if let ComponentValue::Token(Token::Ident(word)) = value {
        return match keyword(&word.to_ascii_lowercase()) {
            Some(property) => Implemented::Known(vec![property]),
            None => Implemented::Malformed,
        };
    }
    match length_outcome(value) {
        LenOutcome::Ok(len) => Implemented::Known(vec![build(len)]),
        LenOutcome::Unsupported => Implemented::BadValue,
        LenOutcome::Invalid => Implemented::Malformed,
    }
}

/// One keyword **or** one `<length-percentage>`, where an identifier the
/// keyword closure declines is this build's gap rather than the author's.
///
/// The difference from [`length_property`] is one line and it is the whole
/// reason this exists: that helper reports an unrecognised identifier as
/// `Malformed`, which is right for `width: red` and wrong for
/// `min-width: min-content` — a value `css-sizing-3` defines and this build has
/// not implemented. Putting it in the author's column instead of this build's
/// would flatter the one figure the census is judged on. `flex-basis` met the
/// same problem one property family earlier and was written out longhand for
/// it; this is that reasoning as a helper, because five properties now need it.
fn keyword_or_length(
    value: Option<&ComponentValue>,
    single: bool,
    keyword: impl Fn(&str) -> Option<Property>,
    build: impl Fn(Len) -> Property,
) -> Implemented {
    let Some(value) = value else {
        return Implemented::Malformed;
    };
    if !single {
        return Implemented::Malformed;
    }
    if let ComponentValue::Token(Token::Ident(word)) = value {
        return match keyword(&word.to_ascii_lowercase()) {
            Some(property) => Implemented::Known(vec![property]),
            None => Implemented::BadValue,
        };
    }
    match length_outcome(value) {
        LenOutcome::Ok(len) => Implemented::Known(vec![build(len)]),
        LenOutcome::Unsupported => Implemented::BadValue,
        LenOutcome::Invalid => Implemented::Malformed,
    }
}

/// Refuses a negative length for a grammar that has none.
///
/// `Malformed` and not `BadValue`, on `border-width`'s precedent: a
/// `min-width: -1px` is not a value this build declined to implement, it is not
/// a value of the property, and §5.4.4 discards it exactly as it discards
/// `min-width: red`.
fn non_negative(outcome: Implemented) -> Implemented {
    let Implemented::Known(properties) = &outcome else {
        return outcome;
    };
    for property in properties {
        let len = match property {
            Property::MinWidth(SpecifiedMinSize::Length(len))
            | Property::MinHeight(SpecifiedMinSize::Length(len))
            | Property::MaxWidth(SpecifiedMaxSize::Length(len))
            | Property::MaxHeight(SpecifiedMaxSize::Length(len))
            | Property::ColumnGap(SpecifiedGap::Length(len))
            | Property::RowGap(SpecifiedGap::Length(len)) => *len,
            _ => continue,
        };
        let value = match len {
            Len::Px(value) | Len::Em(value) | Len::Rem(value) | Len::Percent(value) => value,
        };
        if value < 0.0 {
            return Implemented::Malformed;
        }
    }
    outcome
}

/// `column-count`, `css-multicol-1` §3.2: `auto | <integer [1,∞]>`.
fn column_count(value: Option<&ComponentValue>, single: bool) -> Implemented {
    match (single, value) {
        (true, Some(ComponentValue::Token(Token::Ident(word)))) => {
            if word.eq_ignore_ascii_case("auto") {
                Implemented::Known(vec![Property::ColumnCount(ColumnCount::Auto)])
            } else {
                Implemented::BadValue
            }
        }
        (
            true,
            Some(ComponentValue::Token(Token::Number {
                value,
                integer: true,
            })),
        ) if *value >= 1.0 && *value <= f64::from(u16::MAX) => {
            Implemented::Known(vec![Property::ColumnCount(ColumnCount::Count(
                *value as u16,
            ))])
        }
        _ => Implemented::Malformed,
    }
}

/// `column-width`, §3.1: `auto | <length [0,∞]>`, and **no percentage**.
fn column_width(value: Option<&ComponentValue>, single: bool) -> Implemented {
    match (single, value) {
        (true, Some(ComponentValue::Token(Token::Ident(word)))) => {
            if word.eq_ignore_ascii_case("auto") {
                Implemented::Known(vec![Property::ColumnWidth(SpecifiedColumnWidth::Auto)])
            } else {
                Implemented::BadValue
            }
        }
        (true, Some(value)) => match length_outcome(value) {
            LenOutcome::Ok(Len::Percent(_)) => Implemented::Malformed,
            LenOutcome::Ok(Len::Px(px)) if px < 0.0 => Implemented::Malformed,
            LenOutcome::Ok(len) => Implemented::Known(vec![Property::ColumnWidth(
                SpecifiedColumnWidth::Length(len),
            )]),
            LenOutcome::Unsupported => Implemented::BadValue,
            LenOutcome::Invalid => Implemented::Malformed,
        },
        _ => Implemented::Malformed,
    }
}

/// One `column-gap`/`row-gap` value, for the `gap` shorthand to read twice.
fn gap_value(value: Option<&ComponentValue>, single: bool) -> Implemented {
    non_negative(keyword_or_length(
        value,
        single,
        |word| (word == "normal").then_some(Property::RowGap(SpecifiedGap::Normal)),
        |len| Property::RowGap(SpecifiedGap::Length(len)),
    ))
}

/// `columns`, §3.3: `<'column-width'> || <'column-count'>`.
fn columns_shorthand(significant: &[&ComponentValue]) -> Implemented {
    if significant.is_empty() || significant.len() > 2 {
        return Implemented::Malformed;
    }
    let mut width: Option<SpecifiedColumnWidth> = None;
    let mut count: Option<ColumnCount> = None;
    let mut autos = 0usize;
    for value in significant {
        // A bare `auto` fits either slot and §3.3 says so; it is counted and
        // spent below, because `columns: auto` sets **both** longhands and
        // `columns: auto 3` sets the width.
        if let ComponentValue::Token(Token::Ident(word)) = value {
            if word.eq_ignore_ascii_case("auto") {
                autos += 1;
                continue;
            }
            return Implemented::BadValue;
        }
        if let ComponentValue::Token(Token::Number { integer: true, .. }) = value {
            if count.is_some() {
                return Implemented::Malformed;
            }
            match column_count(Some(value), true) {
                Implemented::Known(mut one) => match one.remove(0) {
                    Property::ColumnCount(value) => count = Some(value),
                    _ => return Implemented::Malformed,
                },
                other => return other,
            }
            continue;
        }
        if width.is_some() {
            return Implemented::Malformed;
        }
        match column_width(Some(value), true) {
            Implemented::Known(mut one) => match one.remove(0) {
                Property::ColumnWidth(value) => width = Some(value),
                _ => return Implemented::Malformed,
            },
            other => return other,
        }
    }
    if autos > 2 - usize::from(width.is_some()) - usize::from(count.is_some()) {
        return Implemented::Malformed;
    }
    Implemented::Known(vec![
        Property::ColumnWidth(width.unwrap_or(SpecifiedColumnWidth::Auto)),
        Property::ColumnCount(count.unwrap_or(ColumnCount::Auto)),
    ])
}

/// `column-rule`, §5.4: width, style and colour in any order, each optional,
/// and the absent ones reset to their initial values.
///
/// [`border_shorthand`]'s shape and its `currentColor` simplification, one
/// property family over: §5.3 gives `column-rule-color` the initial value
/// `currentColor`, this build has no such keyword, and an omitted colour
/// therefore takes the initial computed `color` rather than the element's.
fn column_rule_shorthand(values: &[ComponentValue]) -> Implemented {
    let significant: Vec<&ComponentValue> = values.iter().filter(|v| !v.is_whitespace()).collect();
    if significant.is_empty() || significant.len() > 3 {
        return Implemented::Malformed;
    }
    let mut width: Option<Len> = None;
    let mut style: Option<BorderStyle> = None;
    let mut paint: Option<Color> = None;
    for value in &significant {
        if style.is_none() {
            if let Some(found) = border_style(Some(value), true) {
                style = Some(found);
                continue;
            }
        }
        if width.is_none() {
            if let Some(found) = border_width(Some(value), true) {
                width = Some(found);
                continue;
            }
        }
        if paint.is_none() {
            if let Some(found) = color(value) {
                paint = Some(found);
                continue;
            }
        }
        return Implemented::BadValue;
    }
    Implemented::Known(vec![
        Property::ColumnRuleWidth(width.unwrap_or(Len::Px(3.0))),
        Property::ColumnRuleStyle(style.unwrap_or(BorderStyle::None)),
        Property::ColumnRuleColor(paint.unwrap_or(Color::BLACK)),
    ])
}

/// The outcome of trying to read a value for a property this build implements.
enum Implemented {
    /// One or more longhands.
    Known(Vec<Property>),
    /// A property this build implements at a value it does not — decision 5's
    /// second device, and the reason `float: inline-start` is not `float: left`.
    BadValue,
    /// Not valid CSS for this property at all: `color: ;`, `margin: red`.
    Malformed,
}

/// The `&'static str` name of an implemented property, so `Unsupported` can
/// carry one without allocating a name the build might not know it has.
fn implemented_name(name: &str) -> Option<&'static str> {
    IMPLEMENTED_NAMES.iter().copied().find(|n| *n == name)
}

/// Every property name this build implements, longhands and shorthands alike.
///
/// Kept beside [`UNSUPPORTED_PROPERTIES`] and asserted disjoint from it by
/// `no_property_is_both_implemented_and_unsupported`, because a name in both
/// would be reported as a gap this build does not have.
pub const IMPLEMENTED_NAMES: &[&str] = &[
    "align-content",
    "align-items",
    "align-self",
    "background",
    "background-color",
    "border",
    "border-bottom",
    "border-bottom-color",
    "border-bottom-style",
    "border-bottom-width",
    "border-collapse",
    "border-color",
    "border-left",
    "border-left-color",
    "border-left-style",
    "border-left-width",
    "border-right",
    "border-right-color",
    "border-right-style",
    "border-right-width",
    "border-spacing",
    "border-style",
    "border-top",
    "border-top-color",
    "border-top-style",
    "border-top-width",
    "border-width",
    "bottom",
    "box-sizing",
    "clear",
    "color",
    "column-count",
    "column-fill",
    "column-gap",
    "column-rule",
    "column-rule-color",
    "column-rule-style",
    "column-rule-width",
    "column-span",
    "column-width",
    "columns",
    "content",
    "display",
    "flex",
    "flex-basis",
    "flex-direction",
    "flex-flow",
    "flex-grow",
    "flex-shrink",
    "flex-wrap",
    "float",
    "font-family",
    "font-size",
    "font-style",
    "font-variant",
    "font-weight",
    "gap",
    "height",
    "justify-content",
    "left",
    "letter-spacing",
    "line-break",
    "line-height",
    "list-style-type",
    "margin",
    "margin-bottom",
    "margin-left",
    "margin-right",
    "margin-top",
    "max-height",
    "max-width",
    "min-height",
    "min-width",
    "order",
    "orphans",
    "overflow-wrap",
    "padding",
    "padding-bottom",
    "padding-left",
    "padding-right",
    "padding-top",
    "page-break-after",
    "page-break-before",
    "page-break-inside",
    "position",
    "right",
    "row-gap",
    "table-layout",
    "text-align",
    "text-decoration",
    "text-indent",
    "top",
    "vertical-align",
    "visibility",
    "white-space",
    "widows",
    "width",
    "word-break",
    "word-spacing",
    "z-index",
];

fn implemented(
    name: &str,
    values: &[ComponentValue],
    significant: &[&ComponentValue],
) -> Option<Implemented> {
    let one = significant.first().copied();
    let single = significant.len() == 1;
    Some(match name {
        "color" => colour_property(one, single, Property::Color),
        "background-color" => colour_property(one, single, Property::BackgroundColor),
        // The `background` shorthand at the one form a book writes: a colour
        // alone. Anything else names an image, a position or a repeat, none of
        // which this build has — and expanding the colour out of it and
        // dropping the rest would paint a background the author did not ask for.
        "background" => match (single, one.and_then(color)) {
            (true, Some(c)) => Implemented::Known(vec![Property::BackgroundColor(c)]),
            _ => Implemented::BadValue,
        },
        "display" => keyword(one, single, |word| {
            Some(Property::Display(match word {
                "inline" => Display::Inline,
                "block" => Display::Block,
                "inline-block" => Display::InlineBlock,
                "list-item" => Display::ListItem,
                "none" => Display::None,
                "table" => Display::Table,
                "table-row-group" => Display::TableRowGroup,
                "table-header-group" => Display::TableHeaderGroup,
                "table-footer-group" => Display::TableFooterGroup,
                "table-row" => Display::TableRow,
                "table-cell" => Display::TableCell,
                "table-column" => Display::TableColumn,
                "table-column-group" => Display::TableColumnGroup,
                "table-caption" => Display::TableCaption,
                "flex" => Display::Flex,
                "inline-flex" => Display::InlineFlex,
                _ => return None,
            }))
        }),
        "flex-direction" => keyword(one, single, |word| {
            Some(Property::FlexDirection(match word {
                "row" => FlexDirection::Row,
                "row-reverse" => FlexDirection::RowReverse,
                "column" => FlexDirection::Column,
                "column-reverse" => FlexDirection::ColumnReverse,
                _ => return None,
            }))
        }),
        "flex-wrap" => keyword(one, single, |word| {
            Some(Property::FlexWrap(match word {
                "nowrap" => FlexWrap::NoWrap,
                "wrap" => FlexWrap::Wrap,
                "wrap-reverse" => FlexWrap::WrapReverse,
                _ => return None,
            }))
        }),
        // `<'flex-direction'> || <'flex-wrap'>`, §5.3 — either, both, in either
        // order. **Both longhands are always emitted**, at their initial values
        // where the author wrote only one: §5.3's own note is that the
        // shorthand *"resets the omitted longhand to its initial value"*, and a
        // build that emitted only what was written would leave an earlier
        // `flex-wrap: wrap` standing under a later `flex-flow: column`.
        "flex-flow" => flex_flow(significant),
        "justify-content" => keyword(one, single, |word| {
            Some(Property::JustifyContent(match word {
                "flex-start" => JustifyContent::FlexStart,
                "flex-end" => JustifyContent::FlexEnd,
                "center" => JustifyContent::Center,
                "space-between" => JustifyContent::SpaceBetween,
                "space-around" => JustifyContent::SpaceAround,
                "space-evenly" => JustifyContent::SpaceEvenly,
                _ => return None,
            }))
        }),
        "align-items" => keyword(one, single, |word| {
            Some(Property::AlignItems(match word {
                "flex-start" => AlignItems::FlexStart,
                "flex-end" => AlignItems::FlexEnd,
                "center" => AlignItems::Center,
                "baseline" => AlignItems::Baseline,
                "stretch" => AlignItems::Stretch,
                _ => return None,
            }))
        }),
        "align-self" => keyword(one, single, |word| {
            Some(Property::AlignSelf(match word {
                "auto" => AlignSelf::Auto,
                "flex-start" => AlignSelf::FlexStart,
                "flex-end" => AlignSelf::FlexEnd,
                "center" => AlignSelf::Center,
                "baseline" => AlignSelf::Baseline,
                "stretch" => AlignSelf::Stretch,
                _ => return None,
            }))
        }),
        "align-content" => keyword(one, single, |word| {
            Some(Property::AlignContent(match word {
                "flex-start" => AlignContent::FlexStart,
                "flex-end" => AlignContent::FlexEnd,
                "center" => AlignContent::Center,
                "space-between" => AlignContent::SpaceBetween,
                "space-around" => AlignContent::SpaceAround,
                "stretch" => AlignContent::Stretch,
                _ => return None,
            }))
        }),
        "flex-grow" => flex_factor(one, single, Property::FlexGrow),
        "flex-shrink" => flex_factor(one, single, Property::FlexShrink),
        // §7.2.3's grammar is `content | <'width'>`, and `content` — size to the
        // item's own content — is a value this build does not have. It is
        // `BadValue` by name rather than mapped onto `auto`, which is device 2,
        // and that is why this is written out rather than going through
        // `length_property`: that helper reports an unknown keyword as
        // `Malformed`, which would put a real CSS value in the author's column
        // instead of in this build's.
        "flex-basis" => match (single, one) {
            (true, Some(ComponentValue::Token(Token::Ident(word)))) => {
                if word.eq_ignore_ascii_case("auto") {
                    Implemented::Known(vec![Property::FlexBasis(SpecifiedSize::Auto)])
                } else {
                    Implemented::BadValue
                }
            }
            (true, Some(value)) => match length_outcome(value) {
                LenOutcome::Ok(len) => {
                    Implemented::Known(vec![Property::FlexBasis(SpecifiedSize::Length(len))])
                }
                LenOutcome::Unsupported => Implemented::BadValue,
                LenOutcome::Invalid => Implemented::Malformed,
            },
            _ => Implemented::Malformed,
        },
        // §7.2's shorthand, and **the one place in this file where an omitted
        // component does not take the longhand's initial value.** §7.2 gives
        // the shorthand its own defaults: a bare `<number>` means `1 1 0%`,
        // where `flex-basis`'s initial value is `auto`. `flex: 1` and
        // `flex-grow: 1` are therefore different declarations, and the
        // difference is the whole of what a one-line `flex: 1` column does.
        "flex" => flex_shorthand(significant),
        // §5.4's `<integer>`, and it is **signed**: a negative `order` puts an
        // item before one that wrote none, which is what a book's "put the
        // figure first" rule is. `integer` above refuses zero and negatives,
        // because `orphans` and `widows` are line counts; this is not.
        "order" => match (single, one) {
            (
                true,
                Some(ComponentValue::Token(Token::Number {
                    value,
                    integer: true,
                })),
            ) => {
                if *value < f64::from(i32::MIN) || *value > f64::from(i32::MAX) {
                    Implemented::Malformed
                } else {
                    Implemented::Known(vec![Property::Order(*value as i32)])
                }
            }
            _ => Implemented::Malformed,
        },
        "border-collapse" => keyword(one, single, |word| {
            Some(Property::BorderCollapse(match word {
                "separate" => BorderCollapse::Separate,
                "collapse" => BorderCollapse::Collapse,
                _ => return None,
            }))
        }),
        "table-layout" => keyword(one, single, |word| {
            Some(Property::TableLayout(match word {
                "auto" => TableLayout::Auto,
                "fixed" => TableLayout::Fixed,
                _ => return None,
            }))
        }),
        // `<length> <length>?`, and **not** `<length-percentage>`: CSS 2.2
        // §17.6.1's grammar has no percentage in it, and a percentage of a
        // table whose width depends on its own spacing is circular. A
        // percentage here is therefore the author's mistake rather than this
        // build's gap, which is `Malformed` and not `BadValue`.
        "border-spacing" => match significant.len() {
            1 | 2 => {
                let mut lengths = Vec::with_capacity(2);
                for value in significant {
                    match length_outcome(value) {
                        LenOutcome::Ok(Len::Percent(_)) => return Some(Implemented::Malformed),
                        LenOutcome::Ok(len) => lengths.push(len),
                        LenOutcome::Unsupported => return Some(Implemented::BadValue),
                        LenOutcome::Invalid => return Some(Implemented::Malformed),
                    }
                }
                let horizontal = lengths[0];
                let vertical = *lengths.get(1).unwrap_or(&horizontal);
                Implemented::Known(vec![Property::BorderSpacing(horizontal, vertical)])
            }
            _ => Implemented::Malformed,
        },
        "float" => keyword(one, single, |word| {
            Some(Property::Float(match word {
                "none" => Float::None,
                "left" => Float::Left,
                "right" => Float::Right,
                _ => return None,
            }))
        }),
        "clear" => keyword(one, single, |word| {
            Some(Property::Clear(match word {
                "none" => Clear::None,
                "left" => Clear::Left,
                "right" => Clear::Right,
                "both" => Clear::Both,
                _ => return None,
            }))
        }),
        "box-sizing" => keyword(one, single, |word| {
            Some(Property::BoxSizing(match word {
                "content-box" => BoxSizing::ContentBox,
                "border-box" => BoxSizing::BorderBox,
                _ => return None,
            }))
        }),
        "visibility" => keyword(one, single, |word| {
            Some(Property::Visibility(match word {
                "visible" => Visibility::Visible,
                "hidden" => Visibility::Hidden,
                _ => return None,
            }))
        }),
        "font-style" => keyword(one, single, |word| {
            Some(Property::FontStyle(match word {
                "normal" => FontStyle::Normal,
                "italic" => FontStyle::Italic,
                "oblique" => FontStyle::Oblique,
                _ => return None,
            }))
        }),
        "font-variant" => keyword(one, single, |word| {
            Some(Property::FontVariant(match word {
                "normal" => FontVariant::Normal,
                "small-caps" => FontVariant::SmallCaps,
                _ => return None,
            }))
        }),
        "text-align" => keyword(one, single, |word| {
            Some(Property::TextAlign(match word {
                "left" | "start" => TextAlign::Left,
                "right" | "end" => TextAlign::Right,
                "center" => TextAlign::Center,
                "justify" => TextAlign::Justify,
                _ => return None,
            }))
        }),
        "text-decoration" => keyword(one, single, |word| {
            Some(Property::TextDecoration(match word {
                "none" => TextDecoration::None,
                "underline" => TextDecoration::Underline,
                "overline" => TextDecoration::Overline,
                "line-through" => TextDecoration::LineThrough,
                _ => return None,
            }))
        }),
        "white-space" => keyword(one, single, |word| {
            Some(Property::WhiteSpace(match word {
                "normal" => WhiteSpace::Normal,
                "pre" => WhiteSpace::Pre,
                "nowrap" => WhiteSpace::NoWrap,
                "pre-wrap" => WhiteSpace::PreWrap,
                "pre-line" => WhiteSpace::PreLine,
                _ => return None,
            }))
        }),
        "list-style-type" => keyword(one, single, |word| {
            Some(Property::ListStyleType(match word {
                "disc" => ListStyleType::Disc,
                "circle" => ListStyleType::Circle,
                "square" => ListStyleType::Square,
                "decimal" => ListStyleType::Decimal,
                "lower-alpha" | "lower-latin" => ListStyleType::LowerAlpha,
                "upper-alpha" | "upper-latin" => ListStyleType::UpperAlpha,
                "lower-roman" => ListStyleType::LowerRoman,
                "upper-roman" => ListStyleType::UpperRoman,
                "none" => ListStyleType::None,
                _ => return None,
            }))
        }),
        "page-break-before" | "page-break-after" => {
            let before = name == "page-break-before";
            keyword(one, single, move |word| {
                let value = match word {
                    "auto" => PageBreak::Auto,
                    "always" => PageBreak::Always,
                    "avoid" => PageBreak::Avoid,
                    "left" => PageBreak::Left,
                    "right" => PageBreak::Right,
                    _ => return None,
                };
                Some(if before {
                    Property::PageBreakBefore(value)
                } else {
                    Property::PageBreakAfter(value)
                })
            })
        }
        "page-break-inside" => keyword(one, single, |word| {
            Some(Property::PageBreakInside(match word {
                "auto" => PageBreakInside::Auto,
                "avoid" => PageBreakInside::Avoid,
                _ => return None,
            }))
        }),
        // CSS 2.2 §13.3.2. `<integer>`, and a value that is not one is the
        // author's error rather than this build's gap — `orphans: 2.5` and
        // `orphans: red` are both `Malformed`, which §5.4.4 discards, while
        // `orphans: inherit` is `Unsupported` by the CSS-wide-keyword rule
        // above. The three outcomes are the same three `length_outcome` draws,
        // one value type over.
        "orphans" | "widows" => {
            let orphans = name == "orphans";
            integer(one, single, move |count| {
                Some(if orphans {
                    Property::Orphans(count)
                } else {
                    Property::Widows(count)
                })
            })
        }
        "overflow-wrap" => keyword(one, single, |word| {
            Some(Property::OverflowWrap(match word {
                "normal" => OverflowWrap::Normal,
                "break-word" => OverflowWrap::BreakWord,
                "anywhere" => OverflowWrap::Anywhere,
                _ => return None,
            }))
        }),
        "line-break" => keyword(one, single, |word| {
            Some(Property::LineBreak(match word {
                "auto" => LineBreakStrictness::Auto,
                "loose" => LineBreakStrictness::Loose,
                "normal" => LineBreakStrictness::Normal,
                "strict" => LineBreakStrictness::Strict,
                "anywhere" => LineBreakStrictness::Anywhere,
                _ => return None,
            }))
        }),
        "word-break" => keyword(one, single, |word| {
            Some(Property::WordBreak(match word {
                "normal" => WordBreak::Normal,
                "break-all" => WordBreak::BreakAll,
                "keep-all" => WordBreak::KeepAll,
                _ => return None,
            }))
        }),
        "font-weight" => match one {
            Some(ComponentValue::Token(Token::Ident(word))) if single => {
                match word.to_ascii_lowercase().as_str() {
                    "normal" => Implemented::Known(vec![Property::FontWeight(
                        SpecifiedWeight::Absolute(400),
                    )]),
                    "bold" => Implemented::Known(vec![Property::FontWeight(
                        SpecifiedWeight::Absolute(700),
                    )]),
                    "bolder" => {
                        Implemented::Known(vec![Property::FontWeight(SpecifiedWeight::Bolder)])
                    }
                    "lighter" => {
                        Implemented::Known(vec![Property::FontWeight(SpecifiedWeight::Lighter)])
                    }
                    _ => Implemented::BadValue,
                }
            }
            Some(ComponentValue::Token(Token::Number { value, .. })) if single => {
                let rounded = *value as i64;
                if (1..=1000).contains(&rounded) {
                    Implemented::Known(vec![Property::FontWeight(SpecifiedWeight::Absolute(
                        rounded as u16,
                    ))])
                } else {
                    Implemented::BadValue
                }
            }
            _ => Implemented::Malformed,
        },
        "font-size" => match font_size(one, single) {
            Some(size) => Implemented::Known(vec![Property::FontSize(size)]),
            None if single && is_keyword_value(one) => Implemented::BadValue,
            None => Implemented::Malformed,
        },
        "line-height" => match one {
            // A unitless number is not a length here and is the value a book
            // should write: it inherits as the **factor**, not as the length it
            // resolved to on the element that wrote it.
            Some(ComponentValue::Token(Token::Number { value, .. })) if single => {
                Implemented::Known(vec![Property::LineHeight(LineHeight::Number(*value))])
            }
            // A percentage computes against the element's own font size and
            // then inherits as **that length**, which is why it is turned into
            // a factor here and not kept as a percentage.
            Some(ComponentValue::Token(Token::Percentage(value))) if single => Implemented::Known(
                vec![Property::LineHeight(LineHeight::Number(*value / 100.0))],
            ),
            _ => length_property(
                one,
                single,
                |word| (word == "normal").then_some(Property::LineHeight(LineHeight::Normal)),
                |len| {
                    Property::LineHeight(match len {
                        Len::Px(px) => LineHeight::Px(px),
                        Len::Em(factor) | Len::Rem(factor) => LineHeight::Number(factor),
                        Len::Percent(percent) => LineHeight::Number(percent / 100.0),
                    })
                },
            ),
        },
        "letter-spacing" | "word-spacing" => {
            let letter = name == "letter-spacing";
            let build = move |spacing: SpecifiedSpacing| {
                if letter {
                    Property::LetterSpacing(spacing)
                } else {
                    Property::WordSpacing(spacing)
                }
            };
            length_property(
                one,
                single,
                |word| (word == "normal").then(|| build(SpecifiedSpacing::Normal)),
                |len| build(SpecifiedSpacing::Length(len)),
            )
        }
        "text-indent" => length_property(one, single, |_| None, Property::TextIndent),
        "font-family" => match font_family(values) {
            Some(list) if !list.is_empty() => Implemented::Known(vec![Property::FontFamily(list)]),
            _ => Implemented::Malformed,
        },
        "width" | "height" => {
            let width = name == "width";
            let build = move |size: SpecifiedSize| {
                if width {
                    Property::Width(size)
                } else {
                    Property::Height(size)
                }
            };
            length_property(
                one,
                single,
                |word| (word == "auto").then(|| build(SpecifiedSize::Auto)),
                |len| build(SpecifiedSize::Length(len)),
            )
        }
        "margin-top" | "margin-right" | "margin-bottom" | "margin-left" => {
            let side = side_of(name);
            length_property(
                one,
                single,
                move |word| {
                    (word == "auto").then_some(Property::Margin(side, SpecifiedMargin::Auto))
                },
                move |len| Property::Margin(side, SpecifiedMargin::Length(len)),
            )
        }
        "padding-top" | "padding-right" | "padding-bottom" | "padding-left" => {
            let side = side_of(name);
            length_property(
                one,
                single,
                |_| None,
                move |len| Property::Padding(side, len),
            )
        }
        "border-top-width" | "border-right-width" | "border-bottom-width" | "border-left-width" => {
            let side = side_of(name);
            match border_width_outcome(one, single) {
                LenOutcome::Ok(len) => Implemented::Known(vec![Property::BorderWidth(side, len)]),
                LenOutcome::Unsupported => Implemented::BadValue,
                LenOutcome::Invalid => Implemented::Malformed,
            }
        }
        "border-top-style" | "border-right-style" | "border-bottom-style" | "border-left-style" => {
            let side = side_of(name);
            keyword(one, single, move |word| {
                border_style_named(word).map(|style| Property::BorderStyle(side, style))
            })
        }
        "border-top-color" | "border-right-color" | "border-bottom-color" | "border-left-color" => {
            let side = side_of(name);
            colour_property(one, single, move |c| Property::BorderColor(side, c))
        }
        // CSS 2.2 §10.4 and §10.7. A **negative** minimum or maximum is not a
        // value of the property at all — both grammars are
        // `<length-percentage [0,∞]>` — so it is `Malformed` and the author's,
        // where `min-content` is a real value this build has not implemented
        // and is `BadValue` and this build's.
        "min-width" | "min-height" => {
            let width = name == "min-width";
            let build = move |size: SpecifiedMinSize| {
                if width {
                    Property::MinWidth(size)
                } else {
                    Property::MinHeight(size)
                }
            };
            non_negative(keyword_or_length(
                one,
                single,
                |word| (word == "auto").then(|| build(SpecifiedMinSize::Auto)),
                |len| build(SpecifiedMinSize::Length(len)),
            ))
        }
        "max-width" | "max-height" => {
            let width = name == "max-width";
            let build = move |size: SpecifiedMaxSize| {
                if width {
                    Property::MaxWidth(size)
                } else {
                    Property::MaxHeight(size)
                }
            };
            non_negative(keyword_or_length(
                one,
                single,
                |word| (word == "none").then(|| build(SpecifiedMaxSize::None)),
                |len| build(SpecifiedMaxSize::Length(len)),
            ))
        }
        // §10.8.1's ten values. A length or a percentage may be negative —
        // `vertical-align: -0.4em` is how a book sets a chemical subscript —
        // so there is no `non_negative` here and its absence is the property's
        // grammar rather than an omission.
        "vertical-align" => keyword_or_length(
            one,
            single,
            |word| {
                Some(Property::VerticalAlign(match word {
                    "baseline" => SpecifiedVerticalAlign::Baseline,
                    "sub" => SpecifiedVerticalAlign::Sub,
                    "super" => SpecifiedVerticalAlign::Super,
                    "top" => SpecifiedVerticalAlign::Top,
                    "middle" => SpecifiedVerticalAlign::Middle,
                    "bottom" => SpecifiedVerticalAlign::Bottom,
                    "text-top" => SpecifiedVerticalAlign::TextTop,
                    "text-bottom" => SpecifiedVerticalAlign::TextBottom,
                    _ => return None,
                }))
            },
            |len| Property::VerticalAlign(SpecifiedVerticalAlign::Length(len)),
        ),
        "position" => keyword(one, single, |word| {
            Some(Property::Position(match word {
                "static" => Position::Static,
                "relative" => Position::Relative,
                "absolute" => Position::Absolute,
                "fixed" => Position::Fixed,
                "sticky" => Position::Sticky,
                _ => return None,
            }))
        }),
        "top" | "right" | "bottom" | "left" => {
            let side = match name {
                "top" => Side::Top,
                "right" => Side::Right,
                "bottom" => Side::Bottom,
                _ => Side::Left,
            };
            keyword_or_length(
                one,
                single,
                move |word| (word == "auto").then_some(Property::Inset(side, SpecifiedInset::Auto)),
                move |len| Property::Inset(side, SpecifiedInset::Length(len)),
            )
        }
        // §9.9.1's `<integer>`, signed for `order`'s reason: a negative
        // `z-index` puts a box behind its parent's background, which is what a
        // watermark is.
        "z-index" => match (single, one) {
            (true, Some(ComponentValue::Token(Token::Ident(word))))
                if word.eq_ignore_ascii_case("auto") =>
            {
                Implemented::Known(vec![Property::ZIndex(ZIndex::Auto)])
            }
            (
                true,
                Some(ComponentValue::Token(Token::Number {
                    value,
                    integer: true,
                })),
            ) if *value >= f64::from(i32::MIN) && *value <= f64::from(i32::MAX) => {
                Implemented::Known(vec![Property::ZIndex(ZIndex::Layer(*value as i32))])
            }
            _ => Implemented::Malformed,
        },
        "column-count" => column_count(one, single),
        "column-width" => column_width(one, single),
        // `css-multicol-1` §3.3: `<'column-width'> || <'column-count'>`, and
        // **both longhands are always emitted** for `flex-flow`'s reason —
        // §3.3 resets the omitted one to `auto`, so a build that emitted only
        // what was written would leave an earlier `column-count: 3` standing
        // under a later `columns: 20em`.
        "columns" => columns_shorthand(significant),
        "column-gap" | "row-gap" => {
            let column = name == "column-gap";
            let build = move |gap: SpecifiedGap| {
                if column {
                    Property::ColumnGap(gap)
                } else {
                    Property::RowGap(gap)
                }
            };
            non_negative(keyword_or_length(
                one,
                single,
                |word| (word == "normal").then(|| build(SpecifiedGap::Normal)),
                |len| build(SpecifiedGap::Length(len)),
            ))
        }
        // `css-align-3` §8.2: `<'row-gap'> <'column-gap'>?`, **row first**.
        // The order is the one to get wrong: it is the opposite of every
        // `<length> <length>?` in CSS 2.2, where the horizontal value leads,
        // and a build that read it the other way round would space a two-column
        // figure by its row gap and look entirely reasonable.
        "gap" => match significant.len() {
            1 | 2 => {
                let mut gaps: Vec<SpecifiedGap> = Vec::with_capacity(2);
                for value in significant {
                    match gap_value(Some(value), true) {
                        Implemented::Known(one) => match one.first() {
                            Some(Property::RowGap(gap)) => gaps.push(*gap),
                            _ => return Some(Implemented::Malformed),
                        },
                        other => return Some(other),
                    }
                }
                let row = gaps[0];
                let column = *gaps.get(1).unwrap_or(&row);
                Implemented::Known(vec![Property::RowGap(row), Property::ColumnGap(column)])
            }
            _ => Implemented::Malformed,
        },
        // §5.1 to §5.3, and each borrows the `border-*` longhand it is defined
        // in terms of: §5.1 is *"as for `border-width`"*, so `thin`, `medium`
        // and `thick` are its keywords too.
        "column-rule-width" => match border_width_outcome(one, single) {
            LenOutcome::Ok(len) => Implemented::Known(vec![Property::ColumnRuleWidth(len)]),
            LenOutcome::Unsupported => Implemented::BadValue,
            LenOutcome::Invalid => Implemented::Malformed,
        },
        "column-rule-style" => keyword(one, single, |word| {
            border_style_named(word).map(Property::ColumnRuleStyle)
        }),
        "column-rule-color" => colour_property(one, single, Property::ColumnRuleColor),
        "column-rule" => column_rule_shorthand(values),
        "column-span" => keyword(one, single, |word| {
            Some(Property::ColumnSpan(match word {
                "none" => ColumnSpan::None,
                "all" => ColumnSpan::All,
                _ => return None,
            }))
        }),
        "column-fill" => keyword(one, single, |word| {
            Some(Property::ColumnFill(match word {
                "balance" => ColumnFill::Balance,
                "auto" => ColumnFill::Auto,
                _ => return None,
            }))
        }),
        "margin" => expand_box(values, |side, value| {
            margin_value(Some(value), true).map(|m| Property::Margin(side, m))
        }),
        "padding" => expand_box(values, |side, value| {
            length(value).map(|len| Property::Padding(side, len))
        }),
        "border-width" => expand_box(values, |side, value| {
            border_width(Some(value), true).map(|len| Property::BorderWidth(side, len))
        }),
        "border-style" => expand_box(values, |side, value| {
            border_style(Some(value), true).map(|style| Property::BorderStyle(side, style))
        }),
        "border-color" => expand_box(values, |side, value| {
            color(value).map(|c| Property::BorderColor(side, c))
        }),
        "border" | "border-top" | "border-right" | "border-bottom" | "border-left" => {
            let sides: &[Side] = match name {
                "border-top" => &[Side::Top],
                "border-right" => &[Side::Right],
                "border-bottom" => &[Side::Bottom],
                "border-left" => &[Side::Left],
                _ => &[Side::Top, Side::Right, Side::Bottom, Side::Left],
            };
            border_shorthand(values, sides)
        }
        _ => return None,
    })
}

/// The side a longhand's name ends in.
fn side_of(name: &str) -> Side {
    if name.ends_with("-top") || name.contains("-top-") {
        Side::Top
    } else if name.ends_with("-right") || name.contains("-right-") {
        Side::Right
    } else if name.ends_with("-bottom") || name.contains("-bottom-") {
        Side::Bottom
    } else {
        Side::Left
    }
}

/// A keyword-only property: one identifier, mapped by a closure, and anything
/// else is `BadValue` rather than a guess.
fn keyword(
    value: Option<&ComponentValue>,
    single: bool,
    map: impl Fn(&str) -> Option<Property>,
) -> Implemented {
    match value {
        Some(ComponentValue::Token(Token::Ident(word))) if single => {
            match map(&word.to_ascii_lowercase()) {
                Some(property) => Implemented::Known(vec![property]),
                None => Implemented::BadValue,
            }
        }
        Some(_) if single => Implemented::BadValue,
        _ => Implemented::Malformed,
    }
}

/// One non-negative `<number>` for `flex-grow` or `flex-shrink`,
/// `css-flexbox-1` §7.1.
///
/// A **negative** factor is `Malformed` and not `BadValue`: §7.1's grammar is
/// `<number [0,∞]>`, so `flex-grow: -1` is not a value this build has declined
/// to implement — it is not a value of the property at all, and reporting it as
/// this build's gap would put a number in the census that belongs to the book.
fn flex_factor(
    value: Option<&ComponentValue>,
    single: bool,
    build: impl Fn(f64) -> Property,
) -> Implemented {
    match value {
        Some(ComponentValue::Token(Token::Number { value, .. })) if single => {
            if *value < 0.0 || !value.is_finite() {
                Implemented::Malformed
            } else {
                Implemented::Known(vec![build(*value)])
            }
        }
        _ => Implemented::Malformed,
    }
}

/// `flex-flow`, `css-flexbox-1` §5.3: `<'flex-direction'> || <'flex-wrap'>`.
fn flex_flow(significant: &[&ComponentValue]) -> Implemented {
    if significant.is_empty() || significant.len() > 2 {
        return Implemented::Malformed;
    }
    let mut direction = None;
    let mut wrap = None;
    for value in significant {
        let ComponentValue::Token(Token::Ident(word)) = value else {
            return Implemented::Malformed;
        };
        let word = word.to_ascii_lowercase();
        let as_direction = match word.as_str() {
            "row" => Some(FlexDirection::Row),
            "row-reverse" => Some(FlexDirection::RowReverse),
            "column" => Some(FlexDirection::Column),
            "column-reverse" => Some(FlexDirection::ColumnReverse),
            _ => None,
        };
        let as_wrap = match word.as_str() {
            "nowrap" => Some(FlexWrap::NoWrap),
            "wrap" => Some(FlexWrap::Wrap),
            "wrap-reverse" => Some(FlexWrap::WrapReverse),
            _ => None,
        };
        match (as_direction, as_wrap) {
            (Some(value), _) if direction.is_none() => direction = Some(value),
            (_, Some(value)) if wrap.is_none() => wrap = Some(value),
            // A keyword of the right shape given twice, or one neither
            // longhand takes. `flex-flow: row row` is the author's mistake;
            // `flex-flow: inline` is a value this build does not have.
            (None, None) => return Implemented::BadValue,
            _ => return Implemented::Malformed,
        }
    }
    Implemented::Known(vec![
        Property::FlexDirection(direction.unwrap_or(FlexDirection::Row)),
        Property::FlexWrap(wrap.unwrap_or(FlexWrap::NoWrap)),
    ])
}

/// `flex`, `css-flexbox-1` §7.2: `none | [ <'flex-grow'> <'flex-shrink'>? ||
/// <'flex-basis'> ]`.
///
/// The `||` is honoured rather than approximated by a left-to-right read: the
/// grammar genuinely permits `flex: 30px 1`, and a build that required the
/// numbers first would report a real declaration as malformed.
///
/// **The omitted `flex-basis` is `0%` and not `auto`.** §7.2: *"when omitted
/// from the `flex` shorthand, its specified value is `0%`"*, and this is the
/// one difference that decides what `flex: 1` does — with `auto` the item is
/// sized to its content and then grown, with `0%` the whole line is shared out
/// in proportion to the factors. Every three-column layout on the web depends
/// on the second.
fn flex_shorthand(significant: &[&ComponentValue]) -> Implemented {
    if significant.is_empty() || significant.len() > 3 {
        return Implemented::Malformed;
    }
    if significant.len() == 1 {
        if let ComponentValue::Token(Token::Ident(word)) = significant[0] {
            if word.eq_ignore_ascii_case("none") {
                return Implemented::Known(vec![
                    Property::FlexGrow(0.0),
                    Property::FlexShrink(0.0),
                    Property::FlexBasis(SpecifiedSize::Auto),
                ]);
            }
        }
    }
    let mut numbers: Vec<f64> = Vec::new();
    let mut basis: Option<SpecifiedSize> = None;
    let mut unsupported = false;
    for value in significant {
        match value {
            ComponentValue::Token(Token::Number { value, .. }) if *value != 0.0 => {
                if numbers.len() == 2 || !value.is_finite() || *value < 0.0 {
                    return Implemented::Malformed;
                }
                numbers.push(*value);
            }
            ComponentValue::Token(Token::Ident(word)) => {
                if basis.is_some() {
                    return Implemented::Malformed;
                }
                if word.eq_ignore_ascii_case("auto") {
                    basis = Some(SpecifiedSize::Auto);
                } else if word.eq_ignore_ascii_case("content") {
                    // §7.2.3's other keyword, which this build does not size to.
                    unsupported = true;
                    basis = Some(SpecifiedSize::Auto);
                } else {
                    return Implemented::BadValue;
                }
            }
            // A bare `0` is both a `<number>` and a `<length>`, and §7.2's
            // grammar reads it as whichever slot is still open: `flex: 0 0 0`
            // is grow, shrink and basis. Numbers first, because two of the
            // three slots are numbers.
            other => {
                let zero = matches!(
                    other,
                    ComponentValue::Token(Token::Number { value, .. }) if *value == 0.0
                );
                if zero && numbers.len() < 2 && basis.is_none() {
                    numbers.push(0.0);
                    continue;
                }
                if basis.is_some() {
                    return Implemented::Malformed;
                }
                match length_outcome(other) {
                    LenOutcome::Ok(len) => basis = Some(SpecifiedSize::Length(len)),
                    LenOutcome::Unsupported => {
                        unsupported = true;
                        basis = Some(SpecifiedSize::Auto);
                    }
                    LenOutcome::Invalid => return Implemented::Malformed,
                }
            }
        }
    }
    if unsupported {
        return Implemented::BadValue;
    }
    if numbers.is_empty() && basis.is_none() {
        return Implemented::Malformed;
    }
    let grow = numbers.first().copied().unwrap_or(1.0);
    let shrink = numbers.get(1).copied().unwrap_or(1.0);
    let basis = basis.unwrap_or(SpecifiedSize::Length(Len::Percent(0.0)));
    Implemented::Known(vec![
        Property::FlexGrow(grow),
        Property::FlexShrink(shrink),
        Property::FlexBasis(basis),
    ])
}

/// One non-negative `<integer>` for an integer-valued property.
///
/// A number with a fractional part is **not** an `<integer>` and is
/// `Malformed`, not `BadValue`: `css-values-3` §5.1 makes `2.5` invalid syntax
/// for an `<integer>`, which is the author's mistake and not a value type this
/// build has chosen not to implement. Zero is refused for the same reason — CSS
/// 2.2 §13.3.2's `orphans` and `widows` are counts of lines and there is no
/// such thing as a fragment of zero lines.
fn integer(
    value: Option<&ComponentValue>,
    single: bool,
    map: impl Fn(u16) -> Option<Property>,
) -> Implemented {
    match value {
        Some(ComponentValue::Token(Token::Number {
            value,
            integer: true,
        })) if single => {
            if *value < 1.0 || *value > f64::from(u16::MAX) {
                return Implemented::Malformed;
            }
            match map(*value as u16) {
                Some(property) => Implemented::Known(vec![property]),
                None => Implemented::Malformed,
            }
        }
        _ => Implemented::Malformed,
    }
}

/// Whether a value is the sort of thing a keyword property could plausibly
/// have been given — used to tell "a value this build does not implement" from
/// "not CSS at all".
fn is_keyword_value(value: Option<&ComponentValue>) -> bool {
    matches!(
        value,
        Some(ComponentValue::Token(Token::Ident(_)))
            | Some(ComponentValue::Function { .. })
            | Some(ComponentValue::Token(Token::Hash(_, _)))
            | Some(ComponentValue::Token(Token::Dimension { .. }))
            | Some(ComponentValue::Token(Token::Percentage(_)))
            | Some(ComponentValue::Token(Token::Number { .. }))
    )
}

/// CSS 2.1's absolute size keywords, at the scale every engine uses: `medium`
/// is 16px and each step is a factor of roughly 1.2, snapped to the integers
/// the specification's own table gives.
fn absolute_keyword(word: &str) -> Option<f64> {
    Some(match word {
        "xx-small" => 9.0,
        "x-small" => 10.0,
        "small" => 13.0,
        "medium" => 16.0,
        "large" => 18.0,
        "x-large" => 24.0,
        "xx-large" => 32.0,
        _ => return None,
    })
}

fn font_size(value: Option<&ComponentValue>, single: bool) -> Option<SpecifiedFontSize> {
    if !single {
        return None;
    }
    match value? {
        ComponentValue::Token(Token::Ident(word)) => {
            let lower = word.to_ascii_lowercase();
            if let Some(px) = absolute_keyword(&lower) {
                return Some(SpecifiedFontSize::Absolute(px));
            }
            match lower.as_str() {
                "larger" => Some(SpecifiedFontSize::Larger),
                "smaller" => Some(SpecifiedFontSize::Smaller),
                _ => None,
            }
        }
        // On `font-size` a percentage **is** an em: both are relative to the
        // parent's computed size, which is not true of any other property.
        ComponentValue::Token(Token::Percentage(percent)) => {
            Some(SpecifiedFontSize::Relative(percent / 100.0))
        }
        value => match length(value)? {
            Len::Px(px) => Some(SpecifiedFontSize::Absolute(px)),
            Len::Em(factor) => Some(SpecifiedFontSize::Relative(factor)),
            Len::Rem(factor) => Some(SpecifiedFontSize::Root(factor)),
            Len::Percent(percent) => Some(SpecifiedFontSize::Relative(percent / 100.0)),
        },
    }
}

fn margin_value(value: Option<&ComponentValue>, single: bool) -> Option<SpecifiedMargin> {
    if !single {
        return None;
    }
    match value? {
        ComponentValue::Token(Token::Ident(word)) if word.eq_ignore_ascii_case("auto") => {
            Some(SpecifiedMargin::Auto)
        }
        value => length(value).map(SpecifiedMargin::Length),
    }
}

/// CSS 2.1's three border-width keywords, at the values the specification's own
/// note suggests and every engine uses.
fn border_width(value: Option<&ComponentValue>, single: bool) -> Option<Len> {
    match border_width_outcome(value, single) {
        LenOutcome::Ok(len) => Some(len),
        _ => None,
    }
}

fn border_width_outcome(value: Option<&ComponentValue>, single: bool) -> LenOutcome {
    let Some(value) = value else {
        return LenOutcome::Invalid;
    };
    if !single {
        return LenOutcome::Invalid;
    }
    if let ComponentValue::Token(Token::Ident(word)) = value {
        return match word.to_ascii_lowercase().as_str() {
            "thin" => LenOutcome::Ok(Len::Px(1.0)),
            "medium" => LenOutcome::Ok(Len::Px(3.0)),
            "thick" => LenOutcome::Ok(Len::Px(5.0)),
            _ => LenOutcome::Invalid,
        };
    }
    match length_outcome(value) {
        // A negative border width is invalid, not zero — and a percentage is
        // not a border width at all. Both are the author's error rather than
        // this build's gap.
        LenOutcome::Ok(Len::Px(px)) if px < 0.0 => LenOutcome::Invalid,
        LenOutcome::Ok(Len::Percent(_)) => LenOutcome::Invalid,
        other => other,
    }
}

fn border_style_named(word: &str) -> Option<BorderStyle> {
    Some(match word {
        "none" => BorderStyle::None,
        "hidden" => BorderStyle::Hidden,
        "solid" => BorderStyle::Solid,
        "dashed" => BorderStyle::Dashed,
        "dotted" => BorderStyle::Dotted,
        "double" => BorderStyle::Double,
        _ => return None,
    })
}

fn border_style(value: Option<&ComponentValue>, single: bool) -> Option<BorderStyle> {
    if !single {
        return None;
    }
    match value? {
        ComponentValue::Token(Token::Ident(word)) => border_style_named(&word.to_ascii_lowercase()),
        _ => None,
    }
}

/// CSS 2.1 §8.3's one-to-four-value box expansion.
///
/// One value is all four; two are vertical then horizontal; three are top,
/// horizontal, bottom; four are clockwise from the top. Getting the *three*-
/// value case wrong is the classic: `margin: 1px 2px 3px` is not
/// `1px 2px 3px 2px` read as top-right-bottom-left-with-a-default.
fn expand_box(
    values: &[ComponentValue],
    map: impl Fn(Side, &ComponentValue) -> Option<Property>,
) -> Implemented {
    let significant: Vec<&ComponentValue> = values.iter().filter(|v| !v.is_whitespace()).collect();
    let order: [usize; 4] = match significant.len() {
        1 => [0, 0, 0, 0],
        2 => [0, 1, 0, 1],
        3 => [0, 1, 2, 1],
        4 => [0, 1, 2, 3],
        _ => return Implemented::Malformed,
    };
    let sides = [Side::Top, Side::Right, Side::Bottom, Side::Left];
    let mut out = Vec::with_capacity(4);
    for (side, source) in sides.iter().zip(order) {
        match map(*side, significant[source]) {
            Some(property) => out.push(property),
            None => return Implemented::BadValue,
        }
    }
    Implemented::Known(out)
}

/// `border`, `border-top` and its three siblings: width, style and colour in
/// any order, each optional, and **the ones that are absent are reset to their
/// initial values** — which is what makes `border: none` clear a border rather
/// than leaving its width behind.
fn border_shorthand(values: &[ComponentValue], sides: &[Side]) -> Implemented {
    let significant: Vec<&ComponentValue> = values.iter().filter(|v| !v.is_whitespace()).collect();
    if significant.is_empty() || significant.len() > 3 {
        return Implemented::Malformed;
    }
    let mut width: Option<Len> = None;
    let mut style: Option<BorderStyle> = None;
    let mut paint: Option<Color> = None;
    for value in &significant {
        if style.is_none() {
            if let Some(found) = border_style(Some(value), true) {
                style = Some(found);
                continue;
            }
        }
        if width.is_none() {
            if let Some(found) = border_width(Some(value), true) {
                width = Some(found);
                continue;
            }
        }
        if paint.is_none() {
            if let Some(found) = color(value) {
                paint = Some(found);
                continue;
            }
        }
        return Implemented::BadValue;
    }
    let mut out = Vec::with_capacity(sides.len() * 3);
    for side in sides {
        out.push(Property::BorderWidth(*side, width.unwrap_or(Len::Px(3.0))));
        out.push(Property::BorderStyle(
            *side,
            style.unwrap_or(BorderStyle::None),
        ));
        // `currentColor` is `border-color`'s initial value and this build has
        // no `currentColor` keyword, so an omitted colour takes the initial
        // computed `color`, black — recorded because it is a simplification
        // rather than the specification.
        out.push(Property::BorderColor(*side, paint.unwrap_or(Color::BLACK)));
    }
    Implemented::Known(out)
}

/// A `<length>`, or a `<percentage>` where one is allowed.
fn length(value: &ComponentValue) -> Option<Len> {
    match value {
        // A unitless zero is a length. A unitless anything else is not.
        ComponentValue::Token(Token::Number { value, .. }) if *value == 0.0 => Some(Len::Px(0.0)),
        ComponentValue::Token(Token::Percentage(percent)) => Some(Len::Percent(*percent)),
        ComponentValue::Token(Token::Dimension { value, unit }) => {
            unit_to_len(*value, &unit.to_ascii_lowercase())
        }
        _ => None,
    }
}

/// `css-values-3` §5's absolute units, §5.1's font-relative ones, and the two
/// this build refuses.
fn unit_to_len(value: f64, unit: &str) -> Option<Len> {
    Some(match unit {
        "px" => Len::Px(value),
        "pt" => Len::Px(value * 96.0 / 72.0),
        "pc" => Len::Px(value * 16.0),
        "in" => Len::Px(value * 96.0),
        "cm" => Len::Px(value * 96.0 / 2.54),
        "mm" => Len::Px(value * 96.0 / 25.4),
        "q" => Len::Px(value * 96.0 / 101.6),
        "em" => Len::Em(value),
        "rem" => Len::Rem(value),
        // §5.1.1: *"in the cases where it is impossible or impractical to
        // determine the x-height, a value of 0.5em must be assumed"*. This
        // crate has no font, by ruling 8, so that case is always. The same
        // paragraph gives `ch` a 0.5em fallback for horizontal writing.
        "ex" | "ch" => Len::Em(value * 0.5),
        // The viewport units are deliberately absent. A reflowable book's
        // viewport is the page box, and whether a fragmented page is a viewport
        // at all is a decision milestone 7 owns — so they are `Unsupported`
        // rather than silently resolved against something plausible.
        _ => return None,
    })
}

/// An absolute length in CSS pixels, for the media-query evaluator.
pub fn absolute_px(value: f64, unit: &str, font_size: f64, root_font_size: f64) -> Option<f64> {
    match unit_to_len(value, &unit.to_ascii_lowercase())?.compute(font_size, root_font_size) {
        LengthPercentage::Px(px) => Some(px),
        LengthPercentage::Percent(_) => None,
    }
}

/// A `font-family` list, comma-separated, with unquoted multi-word names
/// joined by spaces the way `css-fonts-4` §4.1 requires.
fn font_family(values: &[ComponentValue]) -> Option<Vec<FontFamily>> {
    let mut out = Vec::new();
    for group in values.split(|v| matches!(v, ComponentValue::Token(Token::Comma))) {
        let significant: Vec<&ComponentValue> =
            group.iter().filter(|v| !v.is_whitespace()).collect();
        if significant.is_empty() {
            return None;
        }
        if let [ComponentValue::Token(Token::Str(name))] = significant.as_slice() {
            out.push(FontFamily::Named(name.clone()));
            continue;
        }
        let mut words = Vec::new();
        for value in &significant {
            match value {
                ComponentValue::Token(Token::Ident(word)) => words.push(word.clone()),
                _ => return None,
            }
        }
        if words.len() == 1 {
            let generic = match words[0].to_ascii_lowercase().as_str() {
                "serif" => Some(FontFamily::Serif),
                "sans-serif" => Some(FontFamily::SansSerif),
                "monospace" => Some(FontFamily::Monospace),
                "cursive" => Some(FontFamily::Cursive),
                "fantasy" => Some(FontFamily::Fantasy),
                _ => None,
            };
            if let Some(generic) = generic {
                out.push(generic);
                continue;
            }
        }
        out.push(FontFamily::Named(words.join(" ")));
    }
    Some(out)
}

// ---- colour -----------------------------------------------------------------

/// The named colours this build knows, and no more.
///
/// It is CSS 2.1's sixteen plus `orange`, plus the greys and a handful of the
/// extended set — **not** `css-color-4`'s hundred and forty-eight. The reason
/// is the subject of this whole gap: a typo in a hex value produces a colour
/// that is slightly wrong and looks entirely plausible, and a name outside this
/// table is `Unsupported` and counted rather than guessed at. Adding a name is
/// cheap; getting one silently wrong is not.
const NAMED_COLOURS: &[(&str, u32)] = &[
    ("black", 0x00_00_00),
    ("silver", 0xc0_c0_c0),
    ("gray", 0x80_80_80),
    ("grey", 0x80_80_80),
    ("white", 0xff_ff_ff),
    ("maroon", 0x80_00_00),
    ("red", 0xff_00_00),
    ("purple", 0x80_00_80),
    ("fuchsia", 0xff_00_ff),
    ("magenta", 0xff_00_ff),
    ("green", 0x00_80_00),
    ("lime", 0x00_ff_00),
    ("olive", 0x80_80_00),
    ("yellow", 0xff_ff_00),
    ("navy", 0x00_00_80),
    ("blue", 0x00_00_ff),
    ("teal", 0x00_80_80),
    ("aqua", 0x00_ff_ff),
    ("cyan", 0x00_ff_ff),
    ("orange", 0xff_a5_00),
    ("darkgray", 0xa9_a9_a9),
    ("darkgrey", 0xa9_a9_a9),
    ("lightgray", 0xd3_d3_d3),
    ("lightgrey", 0xd3_d3_d3),
    ("dimgray", 0x69_69_69),
    ("dimgrey", 0x69_69_69),
    ("gainsboro", 0xdc_dc_dc),
    ("whitesmoke", 0xf5_f5_f5),
    ("darkblue", 0x00_00_8b),
    ("darkred", 0x8b_00_00),
    ("darkgreen", 0x00_64_00),
    ("lightblue", 0xad_d8_e6),
    ("pink", 0xff_c0_cb),
    ("brown", 0xa5_2a_2a),
    ("beige", 0xf5_f5_dc),
    ("ivory", 0xff_ff_f0),
    ("gold", 0xff_d7_00),
    ("indigo", 0x4b_00_82),
    ("violet", 0xee_82_ee),
    ("crimson", 0xdc_14_3c),
    ("tan", 0xd2_b4_8c),
    ("khaki", 0xf0_e6_8c),
    ("salmon", 0xfa_80_72),
    ("sienna", 0xa0_52_2d),
    ("steelblue", 0x46_82_b4),
    ("midnightblue", 0x19_19_70),
];

fn from_rgb(packed: u32) -> Color {
    Color {
        r: (packed >> 16) as u8,
        g: (packed >> 8) as u8,
        b: packed as u8,
        a: 255,
    }
}

/// A `<color>`: a name, a hex, `rgb()`/`rgba()` or `hsl()`/`hsla()`.
fn color(value: &ComponentValue) -> Option<Color> {
    match value {
        ComponentValue::Token(Token::Ident(word)) => {
            let lower = word.to_ascii_lowercase();
            if lower == "transparent" {
                return Some(Color::TRANSPARENT);
            }
            NAMED_COLOURS
                .iter()
                .find(|(name, _)| *name == lower)
                .map(|(_, packed)| from_rgb(*packed))
        }
        ComponentValue::Token(Token::Hash(digits, _)) => hex_colour(digits),
        ComponentValue::Function { name, arguments } => {
            let lower = name.to_ascii_lowercase();
            match lower.as_str() {
                "rgb" | "rgba" => rgb_function(arguments),
                "hsl" | "hsla" => hsl_function(arguments),
                _ => None,
            }
        }
        _ => None,
    }
}

fn hex_colour(digits: &str) -> Option<Color> {
    if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let nibble = |c: char| c.to_digit(16).unwrap_or(0) as u8;
    let chars: Vec<char> = digits.chars().collect();
    match chars.len() {
        3 | 4 => {
            let dup = |n: u8| n * 17;
            Some(Color {
                r: dup(nibble(chars[0])),
                g: dup(nibble(chars[1])),
                b: dup(nibble(chars[2])),
                a: if chars.len() == 4 {
                    dup(nibble(chars[3]))
                } else {
                    255
                },
            })
        }
        6 | 8 => {
            let byte = |i: usize| nibble(chars[i]) * 16 + nibble(chars[i + 1]);
            Some(Color {
                r: byte(0),
                g: byte(2),
                b: byte(4),
                a: if chars.len() == 8 { byte(6) } else { 255 },
            })
        }
        _ => None,
    }
}

/// The numeric arguments of a colour function, commas and slashes removed.
fn colour_arguments(arguments: &[ComponentValue]) -> Vec<&ComponentValue> {
    arguments
        .iter()
        .filter(|v| {
            !v.is_whitespace()
                && !matches!(
                    v,
                    ComponentValue::Token(Token::Comma) | ComponentValue::Token(Token::Delim('/'))
                )
        })
        .collect()
}

fn channel(value: &ComponentValue) -> Option<u8> {
    match value {
        ComponentValue::Token(Token::Number { value, .. }) => Some(clamp_byte(*value)),
        ComponentValue::Token(Token::Percentage(percent)) => {
            Some(clamp_byte(percent * 255.0 / 100.0))
        }
        _ => None,
    }
}

fn alpha(value: &ComponentValue) -> Option<u8> {
    match value {
        ComponentValue::Token(Token::Number { value, .. }) => Some(clamp_byte(value * 255.0)),
        ComponentValue::Token(Token::Percentage(percent)) => {
            Some(clamp_byte(percent * 255.0 / 100.0))
        }
        _ => None,
    }
}

fn clamp_byte(value: f64) -> u8 {
    // `round` is IEEE 754's correctly-rounded operation, so ruling 4 holds:
    // the same digits give the same byte on every target.
    let rounded = value.round();
    if rounded <= 0.0 {
        0
    } else if rounded >= 255.0 {
        255
    } else {
        rounded as u8
    }
}

fn rgb_function(arguments: &[ComponentValue]) -> Option<Color> {
    let values = colour_arguments(arguments);
    if values.len() != 3 && values.len() != 4 {
        return None;
    }
    Some(Color {
        r: channel(values[0])?,
        g: channel(values[1])?,
        b: channel(values[2])?,
        a: match values.get(3) {
            Some(value) => alpha(value)?,
            None => 255,
        },
    })
}

/// `hsl()` to RGB, `css-color-4` §7's own algorithm.
///
/// No transcendental anywhere in it — it is comparison, multiplication and
/// subtraction — so `cargo xtask libm` has nothing to object to even though
/// this crate is not on the pixel-path list.
fn hsl_function(arguments: &[ComponentValue]) -> Option<Color> {
    let values = colour_arguments(arguments);
    if values.len() != 3 && values.len() != 4 {
        return None;
    }
    let hue = match values[0] {
        ComponentValue::Token(Token::Number { value, .. }) => *value,
        ComponentValue::Token(Token::Dimension { value, unit }) => {
            match unit.to_ascii_lowercase().as_str() {
                "deg" => *value,
                "grad" => *value * 360.0 / 400.0,
                "rad" => *value * 180.0 / std::f64::consts::PI,
                "turn" => *value * 360.0,
                _ => return None,
            }
        }
        _ => return None,
    };
    let percent = |value: &ComponentValue| match value {
        ComponentValue::Token(Token::Percentage(p)) => Some((p / 100.0).clamp(0.0, 1.0)),
        _ => None,
    };
    let saturation = percent(values[1])?;
    let lightness = percent(values[2])?;
    let a = match values.get(3) {
        Some(value) => alpha(value)?,
        None => 255,
    };
    let hue = ((hue % 360.0) + 360.0) % 360.0;
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let sector = hue / 60.0;
    let second = chroma * (1.0 - (sector % 2.0 - 1.0).abs());
    let (r1, g1, b1) = match sector as u32 {
        0 => (chroma, second, 0.0),
        1 => (second, chroma, 0.0),
        2 => (0.0, chroma, second),
        3 => (0.0, second, chroma),
        4 => (second, 0.0, chroma),
        _ => (chroma, 0.0, second),
    };
    let m = lightness - chroma / 2.0;
    Some(Color {
        r: clamp_byte((r1 + m) * 255.0),
        g: clamp_byte((g1 + m) * 255.0),
        b: clamp_byte((b1 + m) * 255.0),
        a,
    })
}

// ---- serialisation, for a warning to carry ----------------------------------

/// A compact rendering of a value, so an `Unsupported` can say which value it
/// was rather than only which property.
pub fn serialize(values: &[ComponentValue]) -> String {
    let mut out = String::new();
    write_values(values, &mut out);
    out.trim().to_string()
}

fn write_values(values: &[ComponentValue], out: &mut String) {
    for value in values {
        match value {
            ComponentValue::Token(token) => write_token(token, out),
            ComponentValue::Function { name, arguments } => {
                out.push_str(name);
                out.push('(');
                write_values(arguments, out);
                out.push(')');
            }
            ComponentValue::Block { kind, values } => {
                let (open, close) = match kind {
                    crate::parser::BlockKind::Curly => ('{', '}'),
                    crate::parser::BlockKind::Paren => ('(', ')'),
                    crate::parser::BlockKind::Square => ('[', ']'),
                };
                out.push(open);
                write_values(values, out);
                out.push(close);
            }
        }
    }
}

fn write_token(token: &Token, out: &mut String) {
    match token {
        Token::Ident(name) | Token::Url(name) => out.push_str(name),
        Token::Function(name) => {
            out.push_str(name);
            out.push('(');
        }
        Token::AtKeyword(name) => {
            out.push('@');
            out.push_str(name);
        }
        Token::Hash(name, _) => {
            out.push('#');
            out.push_str(name);
        }
        Token::Str(text) => {
            out.push('"');
            out.push_str(text);
            out.push('"');
        }
        Token::BadString => out.push_str("<bad-string>"),
        Token::BadUrl => out.push_str("<bad-url>"),
        Token::Delim(c) => out.push(*c),
        Token::Number { value, .. } => out.push_str(&format_number(*value)),
        Token::Percentage(value) => {
            out.push_str(&format_number(*value));
            out.push('%');
        }
        Token::Dimension { value, unit } => {
            out.push_str(&format_number(*value));
            out.push_str(unit);
        }
        Token::Whitespace => out.push(' '),
        Token::Cdo => out.push_str("<!--"),
        Token::Cdc => out.push_str("-->"),
        Token::Colon => out.push(':'),
        Token::Semicolon => out.push(';'),
        Token::Comma => out.push(','),
        Token::OpenSquare => out.push('['),
        Token::CloseSquare => out.push(']'),
        Token::OpenParen => out.push('('),
        Token::CloseParen => out.push(')'),
        Token::OpenCurly => out.push('{'),
        Token::CloseCurly => out.push('}'),
    }
}

fn format_number(value: f64) -> String {
    if value == value.trunc() && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}
