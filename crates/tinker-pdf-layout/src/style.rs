//! The one place a computed style is read, and the compile-time device that
//! keeps it that way.
//!
//! # A property that cascades and is never laid out
//!
//! Gap 31's decision 5 makes a property a parser variant only when a consumer
//! exists, and `tinker-pdf-css` enforces it with three exhaustive `match`es:
//! adding a variant to `Property` without an arm in `cascade::apply` is
//! `error[E0004]`. That guard stops one milestone short of the page. A property
//! can be parsed, cascaded, written into `ComputedStyle` — and then read by
//! nobody, which produces exactly the failure the whole plan is organised
//! around: a book that is laid out slightly differently and looks entirely
//! fine.
//!
//! [`consume`] closes the second half. It destructures `ComputedStyle` with
//! **no `..`**, so a field added to that struct without a consumer here is
//! `error[E0027]: pattern does not mention field`. The proof is
//! `tests/uncascaded_field_does_not_build.rs`, which adds a field to the real
//! `ComputedStyle`, compiles this real module against it, and asserts the
//! **build** fails — the same shape as milestone 6's proof one crate down, and
//! possible for the same reason: `tinker-pdf-css` has an empty allow-list, so
//! it can be built by a bare `rustc` and this module linked against it.
//!
//! # Naming a field is not the same as honouring it
//!
//! `float` and `clear` were named here and not implemented until milestone 10,
//! and each **warned by name** rather than being quietly discarded or, worse,
//! mapped onto its nearest implemented neighbour. Laying a `float: left` out as
//! an ordinary block is gap 07's solid-black gradient in a stylesheet: the page
//! is plausible, the paragraph is in the wrong place, and nothing anywhere says
//! so. Both are now read by [`crate::floats`] and the two warnings are gone,
//! which is the only honest way for a warning of that kind to end.
//!
//! `font-variant` is still carried rather than honoured: it reaches the painter
//! as a value and no synthesis of small capitals happens anywhere.

use tinker_pdf_css::cascade::ComputedStyle;
use tinker_pdf_css::property::{
    AlignContent, AlignItems, AlignSelf, BorderCollapse, BorderSpacing, BorderStyle, BoxSizing,
    Clear, Color, ColumnCount, ColumnFill, ColumnSpan, ColumnWidth, Display, FlexDirection,
    FlexWrap, Float, FontFamily, FontStyle, FontVariant, Gap, Inset, JustifyContent,
    LengthPercentage, LineHeight, ListStyleType, MarginValue, MaxSize, MinSize, OverflowWrap,
    PageBreak, PageBreakInside, Position, Side, Sides, Size, Spacing, TableLayout, TextAlign,
    TextDecoration, VerticalAlign, Visibility, WhiteSpace, ZIndex,
};

use crate::metrics::FontRequest;
use crate::uax14::Tailoring;

/// Everything layout reads from one element's computed style.
///
/// It is a struct rather than a borrowed `&ComputedStyle` for one reason: the
/// borrow would let any code anywhere read any field, and then [`consume`]
/// would be a formality rather than the only door.
#[derive(Clone, Debug)]
pub struct Consumed {
    /// `display`, which decides what box is generated at all.
    pub display: Display,
    /// `visibility`. A hidden box is **laid out** and not painted, which is
    /// not `display: none` — a build that treated them alike would move
    /// everything after it.
    pub visible: bool,
    /// The font, for [`crate::metrics::Metrics`].
    pub families: Vec<FontFamily>,
    /// `font-size`, in points.
    pub font_size: f64,
    /// `font-style`.
    pub font_style: FontStyle,
    /// `font-weight`.
    pub font_weight: u16,
    /// `font-variant`, carried to the painter.
    pub font_variant: FontVariant,
    /// `color`.
    pub color: Color,
    /// `text-decoration`.
    pub text_decoration: TextDecoration,
    /// `line-height`, resolved against this element's own font size.
    pub line_height: f64,
    /// `letter-spacing`, in points.
    pub letter_spacing: f64,
    /// `word-spacing`, in points.
    pub word_spacing: f64,
    /// `text-align`.
    pub text_align: TextAlign,
    /// `text-indent`, still a percentage if that is what it was.
    pub text_indent: LengthPercentage,
    /// `white-space`.
    pub white_space: WhiteSpace,
    /// `list-style-type`, for a `display: list-item` marker.
    pub list_style_type: ListStyleType,
    /// `box-sizing`.
    pub box_sizing: BoxSizing,
    /// `width`.
    pub width: Size,
    /// `height`.
    pub height: Size,
    /// `margin-*`.
    pub margin: Sides<MarginValue>,
    /// `padding-*`.
    pub padding: Sides<LengthPercentage>,
    /// `border-*-width`, already zero where the style is `none` or `hidden`.
    pub border_width: Sides<f64>,
    /// `border-*-style`.
    pub border_style: Sides<BorderStyle>,
    /// `border-*-color`.
    pub border_color: Sides<Color>,
    /// `background-color`.
    pub background_color: Color,
    /// `page-break-before`, CSS 2.2 §13.3.1.
    pub page_break_before: PageBreak,
    /// `page-break-after`.
    pub page_break_after: PageBreak,
    /// `page-break-inside`.
    pub page_break_inside: PageBreakInside,
    /// `orphans`, §13.3.2.
    pub orphans: u16,
    /// `widows`.
    pub widows: u16,
    /// `overflow-wrap`, `css-text-3` §5.4.
    pub overflow_wrap: OverflowWrap,
    /// `line-break` and `word-break` together, §5.1 and §5.2.
    pub tailoring: Tailoring,
    /// `float`, CSS 2.2 §9.5.1. Read by [`crate::floats`].
    pub float: Float,
    /// `clear`, §9.5.2.
    pub clear: Clear,
    /// `border-collapse`, CSS 2.2 §17.6. Read by [`crate::table`].
    pub border_collapse: BorderCollapse,
    /// `border-spacing`, §17.6.1, **already zeroed under `collapse`**.
    ///
    /// §17.6.2's first sentence is *"in this model ... the `border-spacing`
    /// property is ignored"*, and doing it here rather than at each of the four
    /// readers is `border-width`'s precedent eight fields up: the used value is
    /// resolved once, at the one door, so no reader can forget. A build that
    /// left it to the readers spaces a collapsed table exactly like a separated
    /// one everywhere it forgot, which is a table that looks fine.
    pub border_spacing: BorderSpacing,
    /// `table-layout`, §17.5.2.
    pub table_layout: TableLayout,
    /// `flex-direction`, `css-flexbox-1` §5.1. Read by [`crate::flex`].
    pub flex_direction: FlexDirection,
    /// `flex-wrap`, §5.2.
    pub flex_wrap: FlexWrap,
    /// `flex-grow`, §7.1.
    pub flex_grow: f64,
    /// `flex-shrink`, §7.1.
    pub flex_shrink: f64,
    /// `flex-basis`, §7.2.3.
    pub flex_basis: Size,
    /// `justify-content`, §8.2.
    pub justify_content: JustifyContent,
    /// `align-items`, §8.3.
    pub align_items: AlignItems,
    /// `align-self`, §8.3, still `auto` where the author wrote nothing.
    ///
    /// **Unresolved here and not there.** §8.3 makes `auto` mean *"the value of
    /// the parent's `align-items`"*, and this struct is one element's style
    /// with no parent in it -- so resolving it would mean either reading a
    /// style that is not this one or picking a default, and the default is
    /// `stretch`, which is a visible answer rather than a missing one.
    /// [`crate::flex::self_alignment`] is where the two meet.
    pub align_self: AlignSelf,
    /// `align-content`, §8.4.
    pub align_content: AlignContent,
    /// `order`, §5.4.
    pub order: i32,
    /// `min-width`, CSS 2.2 §10.4. Read by [`crate::flow`] and by
    /// [`crate::flex`], which are the two places §10.4's clamp and
    /// `css-flexbox-1` §4.5's automatic minimum meet.
    pub min_width: MinSize,
    /// `max-width`, §10.4.
    pub max_width: MaxSize,
    /// `min-height`, §10.7.
    pub min_height: MinSize,
    /// `max-height`, §10.7.
    pub max_height: MaxSize,
    /// `vertical-align`, §10.8.1 for an inline box and §17.5.4 for a table
    /// cell, **with any percentage already resolved to points**.
    ///
    /// §10.8.1 makes a percentage a percentage of *"the `line-height` of the
    /// element itself"*, and the used `line-height` is resolved eleven fields
    /// up rather than in the cascade — because `normal` is a face's answer. So
    /// the resolution belongs here, at the one door, on `border-width`'s and
    /// `border-spacing`'s precedent: no reader can forget it, and no reader
    /// needs to know that the value ever was a percentage.
    pub vertical_align: VerticalAlign,
    /// `position`, §9.3.1. Read by [`crate::flow`], which lays every box out in
    /// the normal flow and says so by name.
    pub position: Position,
    /// `top`, `right`, `bottom` and `left`, §9.3.2.
    pub inset: Sides<Inset>,
    /// `z-index`, §9.9.1.
    pub z_index: ZIndex,
    /// `column-count`, `css-multicol-1` §3.2.
    pub column_count: ColumnCount,
    /// `column-width`, §3.1.
    pub column_width: ColumnWidth,
    /// `column-gap`, `css-align-3` §8.1, **unresolved**, for
    /// [`Consumed::align_self`]'s reason two fields up.
    ///
    /// §8.1 makes `normal` mean `1em` in a multi-column container and `0`
    /// everywhere else, and a percentage a percentage of the container's own
    /// content-box size in that axis — so both halves of the value need
    /// something this struct is not: the formatting context the box turns out
    /// to establish, and a measure. [`Consumed::gap_px`] is where the two meet.
    pub column_gap: Gap,
    /// `row-gap`, §8.1, unresolved for the same reason.
    pub row_gap: Gap,
    /// `column-rule-width`, `css-multicol-1` §5.1, already zero where the style
    /// is `none` or `hidden` — §5.1's own rule and `border-width`'s twenty
    /// fields up.
    pub column_rule_width: f64,
    /// `column-rule-style`, §5.2.
    pub column_rule_style: BorderStyle,
    /// `column-rule-color`, §5.3.
    pub column_rule_color: Color,
    /// `column-span`, §6.
    pub column_span: ColumnSpan,
    /// `column-fill`, §4.
    pub column_fill: ColumnFill,
}

/// Reads a computed style, exhaustively.
///
/// **Do not add a `..` to the pattern below.** It is the whole device: without
/// it, a `ComputedStyle` field with no consumer here is a compile error, and
/// with it a property can be parsed, cascaded and silently never used.
#[must_use]
pub fn consume(style: &ComputedStyle) -> Consumed {
    let ComputedStyle {
        color,
        font_family,
        font_size,
        font_style,
        font_variant,
        font_weight,
        line_height,
        letter_spacing,
        word_spacing,
        text_align,
        text_indent,
        white_space,
        list_style_type,
        visibility,
        text_decoration,
        display,
        float,
        clear,
        box_sizing,
        width,
        height,
        margin,
        padding,
        border_width,
        border_style,
        border_color,
        background_color,
        page_break_before,
        page_break_after,
        page_break_inside,
        orphans,
        widows,
        overflow_wrap,
        line_break,
        word_break,
        border_collapse,
        border_spacing,
        table_layout,
        flex_direction,
        flex_wrap,
        flex_grow,
        flex_shrink,
        flex_basis,
        justify_content,
        align_items,
        align_self,
        align_content,
        order,
        min_width,
        max_width,
        min_height,
        max_height,
        vertical_align,
        position,
        inset,
        z_index,
        column_count,
        column_width,
        column_gap,
        row_gap,
        column_rule_width,
        column_rule_style,
        column_rule_color,
        column_span,
        column_fill,
        // <<< the layout proof's binding goes here >>>
    } = style;

    // `line-height: normal` is not a number the cascade could have computed:
    // `css-inline-3` leaves it to the UA, and 1.2 is the figure every browser
    // and every specification example uses. A **number** is re-multiplied by
    // this element's own font size and a **length** is already resolved, which
    // is the distinction `tinker-pdf-css` keeps in `LineHeight` and the reason
    // it is resolved here rather than there.
    let line_height = match line_height {
        LineHeight::Normal => font_size * 1.2,
        LineHeight::Number(factor) => font_size * factor,
        LineHeight::Px(px) => *px,
    };
    let spacing = |value: &Spacing| match value {
        Spacing::Normal => 0.0,
        Spacing::Px(px) => *px,
    };
    // A border whose style is `none` or `hidden` has a used width of zero
    // whatever `border-width` says (CSS 2.2 §8.5.3), and resolving it here
    // rather than at every reader is what stops `border: 4px` with no style
    // from moving the whole page four points.
    let mut used_border = Sides::all(0.0);
    for side in [Side::Top, Side::Right, Side::Bottom, Side::Left] {
        let width = match border_style.get(side) {
            BorderStyle::None | BorderStyle::Hidden => 0.0,
            _ => border_width.get(side).max(0.0),
        };
        used_border.set(side, width);
    }
    // §10.8.1: *"percentages refer to the `line-height` of the element
    // itself"*, and `line_height` above is that number. The cascade could not
    // do it — `line-height: normal` is 1.2 of a font size no CSS crate has a
    // face for — so it is done here, once, and no reader of `vertical_align`
    // has to know the value ever was a percentage.
    let vertical_align = match vertical_align {
        VerticalAlign::Length(LengthPercentage::Percent(percent)) => {
            VerticalAlign::Length(LengthPercentage::Px(line_height * percent / 100.0))
        }
        other => *other,
    };

    Consumed {
        display: *display,
        visible: *visibility == Visibility::Visible,
        families: font_family.clone(),
        font_size: *font_size,
        font_style: *font_style,
        font_weight: *font_weight,
        font_variant: *font_variant,
        color: *color,
        text_decoration: *text_decoration,
        line_height,
        letter_spacing: spacing(letter_spacing),
        word_spacing: spacing(word_spacing),
        text_align: *text_align,
        text_indent: *text_indent,
        white_space: *white_space,
        list_style_type: *list_style_type,
        box_sizing: *box_sizing,
        width: *width,
        height: *height,
        margin: *margin,
        padding: *padding,
        border_width: used_border,
        border_style: *border_style,
        border_color: *border_color,
        background_color: *background_color,
        page_break_before: *page_break_before,
        page_break_after: *page_break_after,
        page_break_inside: *page_break_inside,
        // CSS 2.2 §13.3.2 makes both *"a positive integer"*; a zero would let
        // a fragment keep no lines at all, which is not what the property
        // means, and the parser already refuses it.
        orphans: (*orphans).max(1),
        widows: (*widows).max(1),
        overflow_wrap: *overflow_wrap,
        tailoring: Tailoring {
            strictness: *line_break,
            word_break: *word_break,
        },
        float: *float,
        clear: *clear,
        border_collapse: *border_collapse,
        border_spacing: match border_collapse {
            BorderCollapse::Separate => *border_spacing,
            BorderCollapse::Collapse => BorderSpacing::ZERO,
        },
        table_layout: *table_layout,
        flex_direction: *flex_direction,
        flex_wrap: *flex_wrap,
        flex_grow: *flex_grow,
        flex_shrink: *flex_shrink,
        flex_basis: *flex_basis,
        justify_content: *justify_content,
        align_items: *align_items,
        align_self: *align_self,
        align_content: *align_content,
        order: *order,
        min_width: *min_width,
        max_width: *max_width,
        min_height: *min_height,
        max_height: *max_height,
        vertical_align,
        position: *position,
        inset: *inset,
        z_index: *z_index,
        column_count: *column_count,
        column_width: *column_width,
        column_gap: *column_gap,
        row_gap: *row_gap,
        // §5.1 defines `column-rule-width` *"as for `border-width`"*, and
        // §5.2's `column-rule-style: none` forces the used width to zero
        // exactly as CSS 2.2 §8.5.3 does for a border. Resolved at the door for
        // the same reason: a rule declared `4px` with no style would otherwise
        // move a column boundary four points in every reader that forgot.
        column_rule_width: match column_rule_style {
            BorderStyle::None | BorderStyle::Hidden => 0.0,
            _ => column_rule_width.max(0.0),
        },
        column_rule_style: *column_rule_style,
        column_rule_color: *column_rule_color,
        column_span: *column_span,
        column_fill: *column_fill,
    }
}

impl Consumed {
    /// The font this element's text is measured and drawn with.
    #[must_use]
    pub fn font(&self) -> FontRequest<'_> {
        FontRequest {
            families: &self.families,
            weight: self.font_weight,
            style: self.font_style,
            size: self.font_size,
        }
    }

    /// Whether this element generates a block-level box.
    ///
    /// A `display: table` is block-level (CSS 2.2 §9.2.1) and an *internal*
    /// table box is **not**: it is neither block-level nor inline-level, it may
    /// only appear inside a table, and it is [`crate::table`]'s to place. The
    /// two halves are separate predicates for that reason — a build that folded
    /// the internal values in here would put a stray `<td>` on a line of its
    /// own as a block, which is a page that looks entirely reasonable.
    /// **`inline-flex` is here and it is not block-level**, which is the one
    /// disagreement with the specification in this predicate and is deliberate.
    /// `css-flexbox-1` §3 makes it inline-level, and this build has no
    /// inline-level box that is not text — so the two available answers are to
    /// set it as inline text, which throws the whole flex layout away, or to
    /// lay it out as a block-level flex container, which gets the *outside*
    /// wrong and the inside right. It takes the second and says so by name:
    /// [`crate::Warning::InlineFlexAsBlock`]. `inline-table` took the first
    /// answer one milestone earlier for the opposite reason — a table's
    /// contents are nothing like a line of text either way, so there was
    /// nothing to keep.
    #[must_use]
    pub fn is_block_level(&self) -> bool {
        matches!(
            self.display,
            Display::Block
                | Display::ListItem
                | Display::Table
                | Display::Flex
                | Display::InlineFlex
        )
    }

    /// Whether this element establishes a flex formatting context,
    /// `css-flexbox-1` §3.
    #[must_use]
    pub fn is_flex(&self) -> bool {
        self.display.is_flex_container()
    }

    /// Whether this element generates a table box, CSS 2.2 §17.2.
    #[must_use]
    pub fn is_table(&self) -> bool {
        self.display == Display::Table
    }

    /// Whether this element generates one of §17.2's internal table boxes —
    /// the ones that may only appear inside a table, and that §17.2.1's rule 9
    /// wraps in an anonymous one when they do not.
    #[must_use]
    pub fn is_internal_table(&self) -> bool {
        self.display.is_internal_table()
    }

    /// Whether this element generates no box at all.
    #[must_use]
    pub fn is_none(&self) -> bool {
        self.display == Display::None
    }

    /// Resolves one of the four margins against a containing-block width.
    ///
    /// A percentage margin is a percentage of the containing block's
    /// **width**, on all four sides — CSS 2.2 §8.3, and it is the rule that
    /// surprises: a `margin-top: 5%` on a wide short block is five per cent of
    /// its width, not of its height.
    #[must_use]
    pub fn margin_px(&self, side: tinker_pdf_css::property::Side, containing: f64) -> f64 {
        match self.margin.get(side) {
            MarginValue::Auto => 0.0,
            MarginValue::Length(LengthPercentage::Px(px)) => px,
            MarginValue::Length(LengthPercentage::Percent(percent)) => containing * percent / 100.0,
        }
    }

    /// Whether this box establishes a **multi-column formatting context**,
    /// `css-multicol-1` §3.
    ///
    /// §3's own condition, which is neither property alone: a container is
    /// multi-column when `column-count` or `column-width` is anything but
    /// `auto`. `column-gap`, `column-rule-*`, `column-span` and `column-fill`
    /// declared on a box that is not one of these do nothing, which is why they
    /// are not part of the test — a book that writes `column-gap: 2em` on every
    /// `<div>` has no multi-column container in it.
    #[must_use]
    pub fn is_multicol(&self) -> bool {
        self.column_count != ColumnCount::Auto || self.column_width != ColumnWidth::Auto
    }

    /// `css-align-3` §8.1's used gap in one axis, in points.
    ///
    /// `normal` is `1em` in a multi-column container and zero everywhere else,
    /// which is §8.1's own answer and the reason [`Consumed::column_gap`] is
    /// carried unresolved: the same declaration is two different numbers
    /// depending on what the box turned out to be.
    #[must_use]
    pub fn gap_px(&self, gap: Gap, containing: f64) -> f64 {
        match gap {
            Gap::Normal => {
                if self.is_multicol() {
                    self.font_size
                } else {
                    0.0
                }
            }
            Gap::Length(LengthPercentage::Px(px)) => px.max(0.0),
            Gap::Length(LengthPercentage::Percent(percent)) => {
                (containing * percent / 100.0).max(0.0)
            }
        }
    }

    /// Resolves one of the four paddings, which cannot be negative (§8.4).
    #[must_use]
    pub fn padding_px(&self, side: tinker_pdf_css::property::Side, containing: f64) -> f64 {
        match self.padding.get(side) {
            LengthPercentage::Px(px) => px.max(0.0),
            LengthPercentage::Percent(percent) => (containing * percent / 100.0).max(0.0),
        }
    }
}

/// CSS 2.2 §10.4's and §10.7's `min-width`/`min-height` as a used length.
///
/// `None` where the value is `auto`, which is not zero and is not the caller's
/// to guess: `css-sizing-3` §5.1 leaves `auto` to the formatting context, and
/// the two contexts in this crate answer differently — a block box's automatic
/// minimum is zero (CSS 2.2 §10.4's own initial value) and a flex item's is
/// `css-flexbox-1` §4.5's min-content size. A helper that returned zero would
/// let every flex item shrink below its longest word, and the page would look
/// like a page.
///
/// `None` also where the value is a percentage and `containing` is `None`,
/// which is §10.5's rule for a percentage against an indefinite size: it
/// behaves as `auto`.
#[must_use]
pub fn min_length(min: MinSize, containing: Option<f64>) -> Option<f64> {
    match min {
        MinSize::Auto => None,
        MinSize::Length(LengthPercentage::Px(px)) => Some(px.max(0.0)),
        MinSize::Length(LengthPercentage::Percent(percent)) => {
            containing.map(|size| (size * percent / 100.0).max(0.0))
        }
    }
}

/// The same for `max-width`/`max-height`, where `None` means `none`.
#[must_use]
pub fn max_length(max: MaxSize, containing: Option<f64>) -> Option<f64> {
    match max {
        MaxSize::None => None,
        MaxSize::Length(LengthPercentage::Px(px)) => Some(px.max(0.0)),
        MaxSize::Length(LengthPercentage::Percent(percent)) => {
            containing.map(|size| (size * percent / 100.0).max(0.0))
        }
    }
}

/// CSS 2.2 §10.4's two extra passes, as one function.
///
/// **The order is the algorithm and it is not a `clamp`.** §10.4 applies
/// `max-width` first and `min-width` second, so a `min-width` larger than the
/// `max-width` **wins** — which `f64::clamp` does not express and which
/// `min(max).max(min)` does. It is also the case `clamp` panics on, so writing
/// it this way is ruling 1 as well as §10.4.
#[must_use]
pub fn clamp_size(tentative: f64, min: Option<f64>, max: Option<f64>) -> f64 {
    let mut used = tentative;
    if let Some(max) = max {
        used = used.min(max);
    }
    if let Some(min) = min {
        used = used.max(min);
    }
    used
}
