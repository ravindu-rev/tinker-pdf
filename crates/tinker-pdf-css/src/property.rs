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

/// `display`, at the five values this build lays out.
///
/// `flex`, `grid`, `table` and the rest are `Unsupported` **by name and by
/// value**, which is the whole of device 2: mapping `display: flex` onto
/// `block` produces a page that looks entirely reasonable and is wrong.
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
            | Property::WordBreak(_) => true,
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
            | Property::PageBreakInside(_) => false,
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
    "align-content",
    "align-items",
    "align-self",
    "animation",
    "background-attachment",
    "background-image",
    "background-position",
    "background-repeat",
    "background-size",
    "border-collapse",
    "border-image",
    "border-radius",
    "border-spacing",
    "bottom",
    "box-shadow",
    "break-after",
    "break-before",
    "break-inside",
    "caption-side",
    "clip",
    "clip-path",
    "color-scheme",
    "column-count",
    "column-fill",
    "column-gap",
    "column-rule",
    "column-span",
    "column-width",
    "columns",
    "content",
    "counter-increment",
    "counter-reset",
    "cursor",
    "direction",
    "empty-cells",
    "filter",
    "flex",
    "flex-basis",
    "flex-direction",
    "flex-flow",
    "flex-grow",
    "flex-shrink",
    "flex-wrap",
    "font",
    "font-display",
    "font-feature-settings",
    "font-kerning",
    "font-stretch",
    "font-variant-numeric",
    "gap",
    "grid",
    "grid-area",
    "grid-column",
    "grid-row",
    "grid-template",
    "grid-template-areas",
    "grid-template-columns",
    "grid-template-rows",
    "hyphens",
    "justify-content",
    "justify-items",
    "justify-self",
    "left",
    "list-style",
    "list-style-image",
    "list-style-position",
    "max-height",
    "max-width",
    "min-height",
    "min-width",
    "mix-blend-mode",
    "opacity",
    "order",
    "outline",
    "outline-color",
    "outline-offset",
    "outline-style",
    "outline-width",
    "overflow",
    "overflow-x",
    "overflow-y",
    "page",
    "position",
    "quotes",
    "resize",
    "right",
    "row-gap",
    "speak",
    "src",
    "tab-size",
    "table-layout",
    "text-emphasis",
    "text-emphasis-style",
    "text-overflow",
    "text-shadow",
    "text-transform",
    "top",
    "transform",
    "transform-origin",
    "transition",
    "unicode-bidi",
    "unicode-range",
    "vertical-align",
    "word-wrap",
    "writing-mode",
    "z-index",
];

// ---- parsing ----------------------------------------------------------------

/// Turns one declaration into decision 5's three-way split.
pub fn parse_declaration(name: &str, values: &[ComponentValue]) -> Parsed {
    let significant: Vec<&ComponentValue> = values.iter().filter(|v| !v.is_whitespace()).collect();
    if significant.is_empty() {
        return Parsed::Invalid;
    }
    // `inherit`, `initial`, `unset`, `revert` and `revert-layer` are
    // `css-cascade-5` §7.1's explicit defaulting keywords, valid on **every**
    // property. This build implements none of them, and the fact is reported
    // per property rather than per keyword because decision 5 keys on the
    // (property, value) pair: `color: inherit` is a gap in `color`.
    if significant.len() == 1 {
        if let Some(Token::Ident(word)) = significant[0].token() {
            let lower = word.to_ascii_lowercase();
            if CSS_WIDE_KEYWORDS.contains(&lower.as_str()) {
                if let Some(known) = implemented_name(name) {
                    return Parsed::Unsupported {
                        property: known,
                        value: lower,
                    };
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

/// `css-cascade-5` §7.1's explicit defaulting keywords, valid on every
/// property and implemented on none.
const CSS_WIDE_KEYWORDS: &[&str] = &["inherit", "initial", "unset", "revert", "revert-layer"];

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
    "background",
    "background-color",
    "border",
    "border-bottom",
    "border-bottom-color",
    "border-bottom-style",
    "border-bottom-width",
    "border-color",
    "border-left",
    "border-left-color",
    "border-left-style",
    "border-left-width",
    "border-right",
    "border-right-color",
    "border-right-style",
    "border-right-width",
    "border-style",
    "border-top",
    "border-top-color",
    "border-top-style",
    "border-top-width",
    "border-width",
    "box-sizing",
    "clear",
    "color",
    "display",
    "float",
    "font-family",
    "font-size",
    "font-style",
    "font-variant",
    "font-weight",
    "height",
    "letter-spacing",
    "line-break",
    "line-height",
    "list-style-type",
    "margin",
    "margin-bottom",
    "margin-left",
    "margin-right",
    "margin-top",
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
    "text-align",
    "text-decoration",
    "text-indent",
    "visibility",
    "white-space",
    "widows",
    "width",
    "word-break",
    "word-spacing",
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
                _ => return None,
            }))
        }),
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
