//! One variant per CSS property **name**, which is not one per [`Property`].
//!
//! `css-cascade-5` §7.1's five defaulting keywords are values that name no
//! value: `color: inherit` says *take the parent's computed colour*, and the
//! only thing the declaration carries is which property it is about. So the
//! cascade needs a handle on a property that is independent of any value, and
//! [`Property`] is not one -- it always holds a specified value, and six of its
//! variants stand for four names each, carrying a [`Side`].
//!
//! Hence eighty-three unit variants, one per name this build implements as a
//! longhand. The sixteen shorthands are not here, because a shorthand is not a
//! property: `crate::property::DEFAULTABLE_SHORTHANDS` expands each into the
//! longhands it sets, which is what `margin: inherit` means.
//!
//! # This file is generated, and it is checked in to be read
//!
//! Eighty-three variants across four consumers is not hand-written code, and a
//! macro would put it somewhere nobody can grep. So it is generated from
//! `property.rs` itself -- the variants, the names out of [`Property::name`],
//! the inheritance out of [`Property::inherited`] -- and written here as
//! ordinary source. Read it like any other file.
//!
//! # What holds it to `Property`
//!
//! [`Property::longhand`] is an exhaustive `match`, so a property cannot be
//! added without naming the [`Longhand`] it sets -- and a `Longhand` cannot be
//! added without [`Longhand::name`], [`Longhand::inherited`] and
//! `cascade::copy_computed` growing an arm, because all three are exhaustive
//! too.
//!
//! [`Longhand::ALL`] is the one thing `rustc` cannot check, because it is a
//! list and not a `match`. `every_implemented_name_is_defaultable` is what
//! checks it, against [`crate::property::IMPLEMENTED_NAMES`]: a name this
//! build parses and that nothing here can default is a hole, and that test
//! reports it by name.

use crate::property::{Property, Side};

/// One CSS property name, without a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Longhand {
    /// `color`
    Color,
    /// `font-family`
    FontFamily,
    /// `font-size`
    FontSize,
    /// `font-style`
    FontStyle,
    /// `font-variant`
    FontVariant,
    /// `font-weight`
    FontWeight,
    /// `line-height`
    LineHeight,
    /// `letter-spacing`
    LetterSpacing,
    /// `word-spacing`
    WordSpacing,
    /// `text-align`
    TextAlign,
    /// `text-indent`
    TextIndent,
    /// `text-decoration`
    TextDecoration,
    /// `white-space`
    WhiteSpace,
    /// `list-style-type`
    ListStyleType,
    /// `visibility`
    Visibility,
    /// `display`
    Display,
    /// `float`
    Float,
    /// `clear`
    Clear,
    /// `box-sizing`
    BoxSizing,
    /// `width`
    Width,
    /// `height`
    Height,
    /// `margin-top`
    MarginTop,
    /// `margin-right`
    MarginRight,
    /// `margin-bottom`
    MarginBottom,
    /// `margin-left`
    MarginLeft,
    /// `padding-top`
    PaddingTop,
    /// `padding-right`
    PaddingRight,
    /// `padding-bottom`
    PaddingBottom,
    /// `padding-left`
    PaddingLeft,
    /// `border-top-width`
    BorderWidthTop,
    /// `border-right-width`
    BorderWidthRight,
    /// `border-bottom-width`
    BorderWidthBottom,
    /// `border-left-width`
    BorderWidthLeft,
    /// `border-top-style`
    BorderStyleTop,
    /// `border-right-style`
    BorderStyleRight,
    /// `border-bottom-style`
    BorderStyleBottom,
    /// `border-left-style`
    BorderStyleLeft,
    /// `border-top-color`
    BorderColorTop,
    /// `border-right-color`
    BorderColorRight,
    /// `border-bottom-color`
    BorderColorBottom,
    /// `border-left-color`
    BorderColorLeft,
    /// `background-color`
    BackgroundColor,
    /// `page-break-before`
    PageBreakBefore,
    /// `page-break-after`
    PageBreakAfter,
    /// `page-break-inside`
    PageBreakInside,
    /// `orphans`
    Orphans,
    /// `widows`
    Widows,
    /// `overflow-wrap`
    OverflowWrap,
    /// `line-break`
    LineBreak,
    /// `word-break`
    WordBreak,
    /// `border-collapse`
    BorderCollapse,
    /// `border-spacing`
    BorderSpacing,
    /// `table-layout`
    TableLayout,
    /// `flex-direction`
    FlexDirection,
    /// `flex-wrap`
    FlexWrap,
    /// `flex-grow`
    FlexGrow,
    /// `flex-shrink`
    FlexShrink,
    /// `flex-basis`
    FlexBasis,
    /// `justify-content`
    JustifyContent,
    /// `align-items`
    AlignItems,
    /// `align-self`
    AlignSelf,
    /// `align-content`
    AlignContent,
    /// `order`
    Order,
    /// `min-width`
    MinWidth,
    /// `max-width`
    MaxWidth,
    /// `min-height`
    MinHeight,
    /// `max-height`
    MaxHeight,
    /// `vertical-align`
    VerticalAlign,
    /// `position`
    Position,
    /// `top`
    InsetTop,
    /// `right`
    InsetRight,
    /// `bottom`
    InsetBottom,
    /// `left`
    InsetLeft,
    /// `z-index`
    ZIndex,
    /// `column-count`
    ColumnCount,
    /// `column-width`
    ColumnWidth,
    /// `column-gap`
    ColumnGap,
    /// `row-gap`
    RowGap,
    /// `column-rule-width`
    ColumnRuleWidth,
    /// `column-rule-style`
    ColumnRuleStyle,
    /// `column-rule-color`
    ColumnRuleColor,
    /// `column-span`
    ColumnSpan,
    /// `column-fill`
    ColumnFill,
    // <<< the compile-time proof injects a longhand directly above this line >>>
}

impl Longhand {
    /// Every longhand this build implements.
    ///
    /// A list, and therefore the one thing here `rustc` does not check; see
    /// this module's header for the test that does.
    pub const ALL: &'static [Longhand] = &[
        Longhand::Color,
        Longhand::FontFamily,
        Longhand::FontSize,
        Longhand::FontStyle,
        Longhand::FontVariant,
        Longhand::FontWeight,
        Longhand::LineHeight,
        Longhand::LetterSpacing,
        Longhand::WordSpacing,
        Longhand::TextAlign,
        Longhand::TextIndent,
        Longhand::TextDecoration,
        Longhand::WhiteSpace,
        Longhand::ListStyleType,
        Longhand::Visibility,
        Longhand::Display,
        Longhand::Float,
        Longhand::Clear,
        Longhand::BoxSizing,
        Longhand::Width,
        Longhand::Height,
        Longhand::MarginTop,
        Longhand::MarginRight,
        Longhand::MarginBottom,
        Longhand::MarginLeft,
        Longhand::PaddingTop,
        Longhand::PaddingRight,
        Longhand::PaddingBottom,
        Longhand::PaddingLeft,
        Longhand::BorderWidthTop,
        Longhand::BorderWidthRight,
        Longhand::BorderWidthBottom,
        Longhand::BorderWidthLeft,
        Longhand::BorderStyleTop,
        Longhand::BorderStyleRight,
        Longhand::BorderStyleBottom,
        Longhand::BorderStyleLeft,
        Longhand::BorderColorTop,
        Longhand::BorderColorRight,
        Longhand::BorderColorBottom,
        Longhand::BorderColorLeft,
        Longhand::BackgroundColor,
        Longhand::PageBreakBefore,
        Longhand::PageBreakAfter,
        Longhand::PageBreakInside,
        Longhand::Orphans,
        Longhand::Widows,
        Longhand::OverflowWrap,
        Longhand::LineBreak,
        Longhand::WordBreak,
        Longhand::BorderCollapse,
        Longhand::BorderSpacing,
        Longhand::TableLayout,
        Longhand::FlexDirection,
        Longhand::FlexWrap,
        Longhand::FlexGrow,
        Longhand::FlexShrink,
        Longhand::FlexBasis,
        Longhand::JustifyContent,
        Longhand::AlignItems,
        Longhand::AlignSelf,
        Longhand::AlignContent,
        Longhand::Order,
        Longhand::MinWidth,
        Longhand::MaxWidth,
        Longhand::MinHeight,
        Longhand::MaxHeight,
        Longhand::VerticalAlign,
        Longhand::Position,
        Longhand::InsetTop,
        Longhand::InsetRight,
        Longhand::InsetBottom,
        Longhand::InsetLeft,
        Longhand::ZIndex,
        Longhand::ColumnCount,
        Longhand::ColumnWidth,
        Longhand::ColumnGap,
        Longhand::RowGap,
        Longhand::ColumnRuleWidth,
        Longhand::ColumnRuleStyle,
        Longhand::ColumnRuleColor,
        Longhand::ColumnSpan,
        Longhand::ColumnFill,
    ];

    /// The name a stylesheet writes.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Longhand::Color => "color",
            Longhand::FontFamily => "font-family",
            Longhand::FontSize => "font-size",
            Longhand::FontStyle => "font-style",
            Longhand::FontVariant => "font-variant",
            Longhand::FontWeight => "font-weight",
            Longhand::LineHeight => "line-height",
            Longhand::LetterSpacing => "letter-spacing",
            Longhand::WordSpacing => "word-spacing",
            Longhand::TextAlign => "text-align",
            Longhand::TextIndent => "text-indent",
            Longhand::TextDecoration => "text-decoration",
            Longhand::WhiteSpace => "white-space",
            Longhand::ListStyleType => "list-style-type",
            Longhand::Visibility => "visibility",
            Longhand::Display => "display",
            Longhand::Float => "float",
            Longhand::Clear => "clear",
            Longhand::BoxSizing => "box-sizing",
            Longhand::Width => "width",
            Longhand::Height => "height",
            Longhand::MarginTop => "margin-top",
            Longhand::MarginRight => "margin-right",
            Longhand::MarginBottom => "margin-bottom",
            Longhand::MarginLeft => "margin-left",
            Longhand::PaddingTop => "padding-top",
            Longhand::PaddingRight => "padding-right",
            Longhand::PaddingBottom => "padding-bottom",
            Longhand::PaddingLeft => "padding-left",
            Longhand::BorderWidthTop => "border-top-width",
            Longhand::BorderWidthRight => "border-right-width",
            Longhand::BorderWidthBottom => "border-bottom-width",
            Longhand::BorderWidthLeft => "border-left-width",
            Longhand::BorderStyleTop => "border-top-style",
            Longhand::BorderStyleRight => "border-right-style",
            Longhand::BorderStyleBottom => "border-bottom-style",
            Longhand::BorderStyleLeft => "border-left-style",
            Longhand::BorderColorTop => "border-top-color",
            Longhand::BorderColorRight => "border-right-color",
            Longhand::BorderColorBottom => "border-bottom-color",
            Longhand::BorderColorLeft => "border-left-color",
            Longhand::BackgroundColor => "background-color",
            Longhand::PageBreakBefore => "page-break-before",
            Longhand::PageBreakAfter => "page-break-after",
            Longhand::PageBreakInside => "page-break-inside",
            Longhand::Orphans => "orphans",
            Longhand::Widows => "widows",
            Longhand::OverflowWrap => "overflow-wrap",
            Longhand::LineBreak => "line-break",
            Longhand::WordBreak => "word-break",
            Longhand::BorderCollapse => "border-collapse",
            Longhand::BorderSpacing => "border-spacing",
            Longhand::TableLayout => "table-layout",
            Longhand::FlexDirection => "flex-direction",
            Longhand::FlexWrap => "flex-wrap",
            Longhand::FlexGrow => "flex-grow",
            Longhand::FlexShrink => "flex-shrink",
            Longhand::FlexBasis => "flex-basis",
            Longhand::JustifyContent => "justify-content",
            Longhand::AlignItems => "align-items",
            Longhand::AlignSelf => "align-self",
            Longhand::AlignContent => "align-content",
            Longhand::Order => "order",
            Longhand::MinWidth => "min-width",
            Longhand::MaxWidth => "max-width",
            Longhand::MinHeight => "min-height",
            Longhand::MaxHeight => "max-height",
            Longhand::VerticalAlign => "vertical-align",
            Longhand::Position => "position",
            Longhand::InsetTop => "top",
            Longhand::InsetRight => "right",
            Longhand::InsetBottom => "bottom",
            Longhand::InsetLeft => "left",
            Longhand::ZIndex => "z-index",
            Longhand::ColumnCount => "column-count",
            Longhand::ColumnWidth => "column-width",
            Longhand::ColumnGap => "column-gap",
            Longhand::RowGap => "row-gap",
            Longhand::ColumnRuleWidth => "column-rule-width",
            Longhand::ColumnRuleStyle => "column-rule-style",
            Longhand::ColumnRuleColor => "column-rule-color",
            Longhand::ColumnSpan => "column-span",
            Longhand::ColumnFill => "column-fill",
            // <<< the compile-time proof's sixth arm goes here >>>
        }
    }

    /// Whether §7.2 makes this property inherit, which is what `unset`
    /// needs in order to choose between `inherit` and `initial`.
    ///
    /// The same answer [`Property::inherited`] gives, from the same source
    /// -- this file is generated from that `match` -- and stated per
    /// **name** rather than per value, because `unset` has no value to ask.
    #[must_use]
    pub fn inherited(self) -> bool {
        match self {
            Longhand::Color
            | Longhand::FontFamily
            | Longhand::FontSize
            | Longhand::FontStyle
            | Longhand::FontVariant
            | Longhand::FontWeight
            | Longhand::LineHeight
            | Longhand::LetterSpacing
            | Longhand::WordSpacing
            | Longhand::TextAlign
            | Longhand::TextIndent
            | Longhand::WhiteSpace
            | Longhand::ListStyleType
            | Longhand::Visibility
            | Longhand::Orphans
            | Longhand::Widows
            | Longhand::OverflowWrap
            | Longhand::LineBreak
            | Longhand::WordBreak
            | Longhand::BorderCollapse
            | Longhand::BorderSpacing => true,
            Longhand::TextDecoration
            | Longhand::Display
            | Longhand::Float
            | Longhand::Clear
            | Longhand::BoxSizing
            | Longhand::Width
            | Longhand::Height
            | Longhand::MarginTop
            | Longhand::MarginRight
            | Longhand::MarginBottom
            | Longhand::MarginLeft
            | Longhand::PaddingTop
            | Longhand::PaddingRight
            | Longhand::PaddingBottom
            | Longhand::PaddingLeft
            | Longhand::BorderWidthTop
            | Longhand::BorderWidthRight
            | Longhand::BorderWidthBottom
            | Longhand::BorderWidthLeft
            | Longhand::BorderStyleTop
            | Longhand::BorderStyleRight
            | Longhand::BorderStyleBottom
            | Longhand::BorderStyleLeft
            | Longhand::BorderColorTop
            | Longhand::BorderColorRight
            | Longhand::BorderColorBottom
            | Longhand::BorderColorLeft
            | Longhand::BackgroundColor
            | Longhand::PageBreakBefore
            | Longhand::PageBreakAfter
            | Longhand::PageBreakInside
            | Longhand::TableLayout
            | Longhand::FlexDirection
            | Longhand::FlexWrap
            | Longhand::FlexGrow
            | Longhand::FlexShrink
            | Longhand::FlexBasis
            | Longhand::JustifyContent
            | Longhand::AlignItems
            | Longhand::AlignSelf
            | Longhand::AlignContent
            | Longhand::Order
            | Longhand::MinWidth
            | Longhand::MaxWidth
            | Longhand::MinHeight
            | Longhand::MaxHeight
            | Longhand::VerticalAlign
            | Longhand::Position
            | Longhand::InsetTop
            | Longhand::InsetRight
            | Longhand::InsetBottom
            | Longhand::InsetLeft
            | Longhand::ZIndex
            | Longhand::ColumnCount
            | Longhand::ColumnWidth
            | Longhand::ColumnGap
            | Longhand::RowGap
            | Longhand::ColumnRuleWidth
            | Longhand::ColumnRuleStyle
            | Longhand::ColumnRuleColor
            | Longhand::ColumnSpan
            | Longhand::ColumnFill => false,
            // <<< the compile-time proof's seventh arm goes here >>>
        }
    }

    /// The longhand a name refers to, or `None` when this build does not
    /// implement that name as one.
    ///
    /// Linear over eighty-three entries, which a declaration pays once and
    /// only when it carries a defaulting keyword.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Longhand> {
        Longhand::ALL.iter().copied().find(|l| l.name() == name)
    }
}

impl Property {
    /// Which longhand this property sets.
    ///
    /// Exhaustive on purpose: it is what stops a property being added that
    /// no defaulting keyword can name, which would be `color: inherit`
    /// working and the new property's `inherit` silently doing nothing.
    #[must_use]
    pub fn longhand(&self) -> Longhand {
        match self {
            Property::Color(..) => Longhand::Color,
            Property::FontFamily(..) => Longhand::FontFamily,
            Property::FontSize(..) => Longhand::FontSize,
            Property::FontStyle(..) => Longhand::FontStyle,
            Property::FontVariant(..) => Longhand::FontVariant,
            Property::FontWeight(..) => Longhand::FontWeight,
            Property::LineHeight(..) => Longhand::LineHeight,
            Property::LetterSpacing(..) => Longhand::LetterSpacing,
            Property::WordSpacing(..) => Longhand::WordSpacing,
            Property::TextAlign(..) => Longhand::TextAlign,
            Property::TextIndent(..) => Longhand::TextIndent,
            Property::TextDecoration(..) => Longhand::TextDecoration,
            Property::WhiteSpace(..) => Longhand::WhiteSpace,
            Property::ListStyleType(..) => Longhand::ListStyleType,
            Property::Visibility(..) => Longhand::Visibility,
            Property::Display(..) => Longhand::Display,
            Property::Float(..) => Longhand::Float,
            Property::Clear(..) => Longhand::Clear,
            Property::BoxSizing(..) => Longhand::BoxSizing,
            Property::Width(..) => Longhand::Width,
            Property::Height(..) => Longhand::Height,
            Property::Margin(side, ..) => match side {
                Side::Top => Longhand::MarginTop,
                Side::Right => Longhand::MarginRight,
                Side::Bottom => Longhand::MarginBottom,
                Side::Left => Longhand::MarginLeft,
            },
            Property::Padding(side, ..) => match side {
                Side::Top => Longhand::PaddingTop,
                Side::Right => Longhand::PaddingRight,
                Side::Bottom => Longhand::PaddingBottom,
                Side::Left => Longhand::PaddingLeft,
            },
            Property::BorderWidth(side, ..) => match side {
                Side::Top => Longhand::BorderWidthTop,
                Side::Right => Longhand::BorderWidthRight,
                Side::Bottom => Longhand::BorderWidthBottom,
                Side::Left => Longhand::BorderWidthLeft,
            },
            Property::BorderStyle(side, ..) => match side {
                Side::Top => Longhand::BorderStyleTop,
                Side::Right => Longhand::BorderStyleRight,
                Side::Bottom => Longhand::BorderStyleBottom,
                Side::Left => Longhand::BorderStyleLeft,
            },
            Property::BorderColor(side, ..) => match side {
                Side::Top => Longhand::BorderColorTop,
                Side::Right => Longhand::BorderColorRight,
                Side::Bottom => Longhand::BorderColorBottom,
                Side::Left => Longhand::BorderColorLeft,
            },
            Property::BackgroundColor(..) => Longhand::BackgroundColor,
            Property::PageBreakBefore(..) => Longhand::PageBreakBefore,
            Property::PageBreakAfter(..) => Longhand::PageBreakAfter,
            Property::PageBreakInside(..) => Longhand::PageBreakInside,
            Property::Orphans(..) => Longhand::Orphans,
            Property::Widows(..) => Longhand::Widows,
            Property::OverflowWrap(..) => Longhand::OverflowWrap,
            Property::LineBreak(..) => Longhand::LineBreak,
            Property::WordBreak(..) => Longhand::WordBreak,
            Property::BorderCollapse(..) => Longhand::BorderCollapse,
            Property::BorderSpacing(..) => Longhand::BorderSpacing,
            Property::TableLayout(..) => Longhand::TableLayout,
            Property::FlexDirection(..) => Longhand::FlexDirection,
            Property::FlexWrap(..) => Longhand::FlexWrap,
            Property::FlexGrow(..) => Longhand::FlexGrow,
            Property::FlexShrink(..) => Longhand::FlexShrink,
            Property::FlexBasis(..) => Longhand::FlexBasis,
            Property::JustifyContent(..) => Longhand::JustifyContent,
            Property::AlignItems(..) => Longhand::AlignItems,
            Property::AlignSelf(..) => Longhand::AlignSelf,
            Property::AlignContent(..) => Longhand::AlignContent,
            Property::Order(..) => Longhand::Order,
            Property::MinWidth(..) => Longhand::MinWidth,
            Property::MaxWidth(..) => Longhand::MaxWidth,
            Property::MinHeight(..) => Longhand::MinHeight,
            Property::MaxHeight(..) => Longhand::MaxHeight,
            Property::VerticalAlign(..) => Longhand::VerticalAlign,
            Property::Position(..) => Longhand::Position,
            Property::Inset(side, ..) => match side {
                Side::Top => Longhand::InsetTop,
                Side::Right => Longhand::InsetRight,
                Side::Bottom => Longhand::InsetBottom,
                Side::Left => Longhand::InsetLeft,
            },
            Property::ZIndex(..) => Longhand::ZIndex,
            Property::ColumnCount(..) => Longhand::ColumnCount,
            Property::ColumnWidth(..) => Longhand::ColumnWidth,
            Property::ColumnGap(..) => Longhand::ColumnGap,
            Property::RowGap(..) => Longhand::RowGap,
            Property::ColumnRuleWidth(..) => Longhand::ColumnRuleWidth,
            Property::ColumnRuleStyle(..) => Longhand::ColumnRuleStyle,
            Property::ColumnRuleColor(..) => Longhand::ColumnRuleColor,
            Property::ColumnSpan(..) => Longhand::ColumnSpan,
            Property::ColumnFill(..) => Longhand::ColumnFill,
            // <<< the compile-time proof's fifth arm goes here >>>
        }
    }
}
