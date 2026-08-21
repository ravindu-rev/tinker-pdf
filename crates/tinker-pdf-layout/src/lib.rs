//! Layout: a box tree and a source of metrics in, pages of positioned
//! fragments out.
//!
//! Scope, design and exit criteria: `docs/plans/gaps/31-epub.md`, milestone 7.
//!
//! **The tenth leaf.** Ruling 8's August 2026 amendment makes the test of a
//! leaf the definition rather than the list: *a leaf is any crate that takes
//! bytes and plain parameters and returns bytes and values, whatever the list
//! says.* A tree of plain structs is plain parameters. Nothing below knows what
//! a PDF, a page object, an EPUB, an XHTML element or a font file is: the
//! element tree lives in the facade, and measurement arrives through
//! [`metrics::Metrics`], five methods wide.
//!
//! # The seven things worth knowing before reading further
//!
//! **Margin collapsing has three cases and they are not one rule.** Adjacent
//! siblings is the one everybody implements. Parent-and-first-child and a box
//! whose own margins collapse *through* it are where implementations quietly
//! differ, and every one of the three moves every block on every page after it.
//! [`flow`] implements all three with one accumulator, and each is asserted on
//! its own.
//!
//! **Line breaking is UAX #14 over the vendored UCD, checked against Unicode's
//! own 19 338-case conformance file.** A breaker that splits at spaces passes
//! every English test ever written. See [`uax14`].
//!
//! **White-space processing is `css-text-3` §4.1.1 *and* §4.1.2**, and the two
//! run at different times over different things. See [`text`].
//!
//! **Where a page break is *permitted* is a different question from where one
//! is *preferred*.** CSS 2.2 §13.3.3's rules A to D say where one may happen at
//! all, and a fragmenter that implements only the second question breaks inside
//! things it must not. See [`fragment`].
//!
//! **A float is not in the column, and reading order stops being emission
//! order the moment one exists.** CSS 2.2 §9.5.1 is nine constraints on where a
//! float goes and they are a set: an implementation that satisfies eight lays
//! most pages out correctly and fails only where the ninth binds. Each is its
//! own step in [`floats`] with its own fixture. And because a float is laid out
//! where it is written and drawn where it was placed — which can be a page
//! later — every [`TextRun`] carries the position in **document order** that
//! text conservation is an ordering of. See [`floats`] and [`TextRun::order`].
//!
//! **A table's box tree is mostly not in the document.** CSS 2.2 §17.2.1
//! generates the boxes a real book leaves out — and a real book leaves out
//! `<tbody>` every time. Each of [`table`]'s nine generation steps is
//! separately omittable and each has a fixture that fails when that step alone
//! is deleted. §17.5.2.2's automatic width algorithm is **two passes** —
//! minimum and maximum content widths first, distribution second — and a
//! one-pass approximation gives a plausible table that differs only where the
//! constraint binds, which is why the two intermediates are asserted as well as
//! the answer. §17.6.2.1's conflict resolution is an *ordered* set of five
//! rules, not a preference.
//!
//! **A flex container's free space is distributed twice, by two different
//! rules, and the second distribution is a loop.** `css-flexbox-1` §9.7 grows
//! in proportion to `flex-grow` and shrinks in proportion to `flex-shrink`
//! **times the flex base size**, which is the whole reason the two are separate
//! properties and not a sign; and it freezes the items that violated a minimum
//! and *redistributes* rather than clamping once. Each of §9's steps that has
//! an answer of its own is a function in [`flex`] with a fixture that fails
//! when that function alone is wrong. `order` moves the boxes and not the
//! words, so the items are laid out in document order and positioned in
//! order-modified order — [`TextRun::order`]'s third customer.
//!
//! **A computed property with no consumer here does not compile.**
//! [`style::consume`] destructures `ComputedStyle` with no `..`, which is gap
//! 31's decision 5 carried one milestone further than the crate that invented
//! it: `tinker-pdf-css` makes a property that is parsed and never *cascaded*
//! fail to build, and this makes one that is cascaded and never *laid out* fail
//! to build.
//!
//! # Using it
//!
//! ```
//! use tinker_pdf_css::cascade::ComputedStyle;
//! use tinker_pdf_css::property::Display;
//! use tinker_pdf_layout::metrics::FixedPitch;
//! use tinker_pdf_layout::{layout, BoxNode, CellSpan, Content, Limits, Options};
//!
//! let mut style = ComputedStyle::initial();
//! style.display = Display::Block;
//! let mut text = ComputedStyle::initial();
//! text.font_size = 12.0;
//!
//! let tree = BoxNode {
//!     style,
//!     content: Content::Children(vec![BoxNode {
//!         style: text,
//!         content: Content::Text("the sea, the sea".into()),
//!         anchor: None,
//!         span: CellSpan::ONE,
//!     }]),
//!     anchor: None,
//!     span: CellSpan::ONE,
//! };
//! let laid = layout(
//!     &tree,
//!     &FixedPitch::COURIER,
//!     &Options::new(432.0, 648.0),
//!     &Limits::DEFAULT,
//! )?;
//! assert_eq!(laid.pages.len(), 1);
//! assert_eq!(laid.text(), "the sea, the sea");
//! # Ok::<(), tinker_pdf_layout::Refusal>(())
//! ```

#![forbid(unsafe_code)]

pub mod flex;
pub mod floats;
pub mod flow;
pub mod fragment;
pub mod limits;
pub mod metrics;
pub mod style;
pub mod table;
pub mod text;
pub mod uax14;
pub mod unicode;

#[cfg(test)]
mod tests;

use std::fmt;

use tinker_pdf_css::cascade::ComputedStyle;
use tinker_pdf_css::property::{
    BorderStyle, Color, FontFamily, FontStyle, FontVariant, Sides, TextDecoration,
};

/// One node of the tree layout is given.
///
/// Plain structs, in document order, owning their children — which is what
/// keeps this crate off the facade's element tree and off `tinker-pdf-xml`.
/// The style is the **computed** one: everything to do with selectors,
/// specificity and inheritance happened in `tinker-pdf-css` and none of it is
/// visible from here.
#[derive(Clone, Debug)]
pub struct BoxNode {
    /// The element's computed style.
    pub style: ComputedStyle,
    /// What is inside it.
    pub content: Content,
    /// An opaque tag the caller chose, carried unchanged to every
    /// [`TextRun`] this node's text produced.
    ///
    /// **It exists because fragmentation destroys the caller's ability to work
    /// this out for itself.** A caller that knows which element an `<a href>`
    /// is cannot find the rectangle it ended up occupying: the text was
    /// collapsed, re-split at UAX #14 opportunities, distributed over line
    /// boxes and then over pages, and the only thing that survives the walk is
    /// what this crate carries. A caller that instead searched the output for
    /// the anchor's text would find the wrong copy the first time a book used
    /// the same words twice.
    ///
    /// This crate never reads it, never compares two of them and never invents
    /// one — a `u32` rather than a type, so nothing here can acquire an
    /// opinion about what a tag means.
    pub anchor: Option<u32>,
    /// How many grid slots this node takes when it is a table cell, CSS 2.2
    /// §17.5.
    ///
    /// **It is not a style and it could not have been.** `colspan` and
    /// `rowspan` are HTML attributes with no CSS property behind them: there is
    /// nothing in any stylesheet that sets them, so the cascade cannot carry
    /// them and [`style::consume`]'s compile-time device — which is about
    /// *computed styles* — has nothing to say about them. A caller that knows
    /// what a `<td colspan>` is puts the number here; a caller that does not
    /// leaves [`CellSpan::ONE`] and gets a grid.
    ///
    /// Ignored on every node that is not a `display: table-cell`, which is what
    /// HTML says of the attributes themselves.
    pub span: CellSpan,
}

/// How many grid slots a table cell takes, CSS 2.2 §17.5.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellSpan {
    /// `colspan`. Zero is not a span and is read as one, which is what HTML
    /// says of `colspan="0"`.
    pub columns: u32,
    /// `rowspan`. **Zero means *to the end of this row group***, which is
    /// HTML's own reading of `rowspan="0"` and the one case where zero is a
    /// value rather than a mistake — so it survives here rather than being
    /// clamped at the door, where the row group it refers to is not known yet.
    pub rows: u32,
}

impl CellSpan {
    /// One slot, which is every cell that does not say otherwise.
    pub const ONE: Self = Self {
        columns: 1,
        rows: 1,
    };
}

impl Default for CellSpan {
    /// [`CellSpan::ONE`], and **not** `columns: 0, rows: 0`. A `#[derive]`
    /// would have given the second, which is a cell that occupies nothing.
    fn default() -> Self {
        Self::ONE
    }
}

/// What a node holds.
#[derive(Clone, Debug)]
pub enum Content {
    /// Child nodes, in document order.
    Children(Vec<BoxNode>),
    /// A run of text, as the source wrote it — **before** `css-text-3` §4.1.1's
    /// collapsing, which is this crate's to do and not the caller's. A caller
    /// that collapsed first would have thrown away the distinction between a
    /// preserved newline and a collapsible one before `white-space` was
    /// consulted.
    Text(String),
}

impl BoxNode {
    /// A text node with a given style, which is what most of a book is.
    #[must_use]
    pub fn text(style: ComputedStyle, text: impl Into<String>) -> Self {
        Self {
            style,
            content: Content::Text(text.into()),
            anchor: None,
            span: CellSpan::ONE,
        }
    }

    /// A node with children.
    #[must_use]
    pub fn element(style: ComputedStyle, children: Vec<BoxNode>) -> Self {
        Self {
            style,
            content: Content::Children(children),
            anchor: None,
            span: CellSpan::ONE,
        }
    }

    /// The same node, tagged. See [`BoxNode::anchor`].
    #[must_use]
    pub fn with_anchor(mut self, anchor: u32) -> Self {
        self.anchor = Some(anchor);
        self
    }

    /// The same node, spanning slots. See [`BoxNode::span`].
    #[must_use]
    pub fn with_span(mut self, columns: u32, rows: u32) -> Self {
        self.span = CellSpan { columns, rows };
        self
    }

    /// Every character of text under this node, in document order.
    ///
    /// The source side of text conservation, and it deliberately includes text
    /// under a `display: none` — the caller compares against the *spine*, and
    /// what this crate did with a subtree is exactly what is being checked.
    #[must_use]
    pub fn source_text(&self) -> String {
        let mut out = String::new();
        self.collect_text(&mut out);
        out
    }

    fn collect_text(&self, out: &mut String) {
        match &self.content {
            Content::Text(text) => out.push_str(text),
            Content::Children(children) => {
                for child in children {
                    child.collect_text(out);
                }
            }
        }
    }
}

/// The page box layout fills, in points.
///
/// This is the **content area** of a page and not the page itself: page
/// margins are the caller's, because a reading system's margins are a
/// presentation choice and this crate has no opinion about them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    /// The width available to the flow.
    pub width: f64,
    /// The height available to the flow.
    pub height: f64,
}

impl Options {
    /// A page of the given content size.
    #[must_use]
    pub fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }
}

/// One page of positioned fragments.
///
/// `y` grows **downward** from the top of the page's content area, which is
/// the direction a flow runs. A PDF's user space grows upward, and the flip is
/// the caller's — done once, at synthesis, rather than by every producer here.
#[derive(Clone, Debug, Default)]
pub struct Page {
    /// Backgrounds and borders, in paint order: an ancestor before its
    /// descendants, so a child's background covers its parent's.
    pub boxes: Vec<BoxFragment>,
    /// Text, in **reading order**, which is what makes text conservation a
    /// comparison rather than a search.
    pub runs: Vec<TextRun>,
}

/// A block box's decoration on one page.
#[derive(Clone, Debug, PartialEq)]
pub struct BoxFragment {
    /// Border-box left edge.
    pub x: f64,
    /// Border-box top edge.
    pub y: f64,
    /// Border-box width.
    pub width: f64,
    /// Border-box height on **this** page. A box that crosses a page boundary
    /// has one fragment per page and they are different heights.
    pub height: f64,
    /// `background-color`.
    pub background: Color,
    /// `border-*-width`, already zero where the style is `none`.
    pub border_width: Sides<f64>,
    /// `border-*-style`.
    pub border_style: Sides<BorderStyle>,
    /// `border-*-color`.
    pub border_color: Sides<Color>,
}

/// One run of text, positioned.
#[derive(Clone, Debug, PartialEq)]
pub struct TextRun {
    /// The left edge of the run.
    pub x: f64,
    /// The **baseline**, not the top: a run's box is decided by its face and
    /// the caller draws from the baseline, so reporting the top would mean two
    /// places computing the same offset from different ascents.
    pub y: f64,
    /// The advance width the run was measured at.
    pub width: f64,
    /// The characters, after `css-text-3` §4.1's processing.
    pub text: String,
    /// `font-size`, in points.
    pub font_size: f64,
    /// `font-family`, in the author's order.
    pub families: Vec<FontFamily>,
    /// `font-weight`.
    pub weight: u16,
    /// `font-style`.
    pub style: FontStyle,
    /// `font-variant`.
    pub variant: FontVariant,
    /// `color`.
    pub color: Color,
    /// `text-decoration`.
    pub decoration: TextDecoration,
    /// Whether the run is painted at all. A `visibility: hidden` box is laid
    /// out and not drawn, which is a different thing from `display: none` and
    /// moves nothing.
    pub painted: bool,
    /// Extra advance applied between the run's characters, from
    /// `letter-spacing` and `word-spacing` and from justification.
    ///
    /// Carried as a number rather than baked into `width` because the caller
    /// draws the run with a `Tc`/`Tw` pair and needs the figure, and because a
    /// justified line whose runs had the space folded into their widths could
    /// not be re-measured.
    pub letter_spacing: f64,
    /// The extra advance applied at each space in this run.
    pub word_spacing: f64,
    /// Whether this run is content the **source did not contain** — a list
    /// marker, at this milestone.
    ///
    /// It exists so text conservation stays an equality rather than becoming a
    /// containment: generated content is not in the spine and must not be
    /// compared against it, and a build with no such flag either loses the
    /// invariant or loses the markers.
    pub generated: bool,
    /// The [`BoxNode::anchor`] of the node this run's characters came from,
    /// carried through collapsing, line breaking and pagination untouched.
    ///
    /// `None` for a run this crate generated, which a list marker is.
    pub anchor: Option<u32>,
    /// Where this run's characters are in **document order**, counting from
    /// one.
    ///
    /// It exists because milestone 10 made reading order stop being emission
    /// order. A float is laid out where it is written and drawn where it was
    /// placed, and those are different places: its text can be set at the top
    /// of a page whose in-flow text was written before it, and a float broken
    /// over a page boundary finishes after text that follows it in the book.
    /// Neither loses a character, and both would fail an *ordered* comparison
    /// against the source — which is exactly what text conservation is.
    ///
    /// [`Page::runs`] is sorted by it, so a page reads correctly by itself,
    /// and [`Layout::text`] sorts by it across the whole book, so the book
    /// does. A caller that wants the order the ink goes down in has the vector
    /// it is holding; a caller that wants the order the words were written in
    /// has this.
    pub order: usize,
}

/// A whole book, paginated.
#[derive(Clone, Debug, Default)]
pub struct Layout {
    /// The pages, in order.
    pub pages: Vec<Page>,
    /// What could not be honoured, deduplicated with a count — ruling 10's
    /// shape, and `tinker_pdf_css::parser::Report`'s.
    pub warnings: Vec<(Warning, usize)>,
}

impl Layout {
    /// Every character of laid-out text, in reading order, excluding generated
    /// content.
    ///
    /// The other half of text conservation. It is a method here rather than a
    /// helper in a test file so that the fuzz target and the crate's tests
    /// compare the same thing.
    ///
    /// **In document order**, which is [`TextRun::order`]'s and not the pages'.
    /// A float whose box was broken over a page boundary finishes on the page
    /// after the text that follows it in the book, and a comparison against the
    /// source is an ordered one: without the sort, a book that lost nothing at
    /// all would report a divergence at every figure. The sort is stable, so
    /// for a book with no floats in it — where the two orders are the same
    /// order — it changes nothing.
    #[must_use]
    pub fn text(&self) -> String {
        let mut runs: Vec<&TextRun> = self
            .pages
            .iter()
            .flat_map(|page| page.runs.iter())
            .filter(|run| !run.generated)
            .collect();
        runs.sort_by_key(|run| run.order);
        let mut out = String::new();
        for run in runs {
            out.push_str(&run.text);
        }
        out
    }
}

/// Resource ceilings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Box tree depth. See [`limits::MAX_BOX_DEPTH`].
    pub max_depth: usize,
    /// Boxes across the book. See [`limits::MAX_BOX_TREE_NODES`].
    pub max_boxes: usize,
    /// Break opportunities across the book. See
    /// [`limits::MAX_LINE_BREAK_WORK`].
    pub max_break_work: usize,
    /// Float examinations across the book. See [`limits::MAX_LAYOUT_WORK`].
    pub max_layout_work: usize,
    /// Pages. See [`limits::MAX_LAYOUT_PAGES`].
    pub max_pages: usize,
}

impl Limits {
    /// The shipped ceilings.
    pub const DEFAULT: Self = Self {
        max_depth: limits::MAX_BOX_DEPTH,
        max_boxes: limits::MAX_BOX_TREE_NODES,
        max_break_work: limits::MAX_LINE_BREAK_WORK,
        max_layout_work: limits::MAX_LAYOUT_WORK,
        max_pages: limits::MAX_LAYOUT_PAGES,
    };
}

impl Default for Limits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The three totals, spent across a whole book and never refunded.
///
/// One object rather than three counters, for `tinker_pdf_css::Budget`'s
/// reason: a caller that lays out forty spine items makes **one** of these, so
/// the fortieth cannot start from zero.
#[derive(Clone, Copy, Debug)]
pub struct Budget {
    limits: Limits,
    boxes: usize,
    breaks: usize,
    layout: usize,
}

impl Budget {
    /// A fresh budget under the given ceilings.
    #[must_use]
    pub fn new(limits: &Limits) -> Self {
        Self {
            limits: *limits,
            boxes: 0,
            breaks: 0,
            layout: 0,
        }
    }

    /// Charges one generated box.
    ///
    /// # Errors
    /// [`Refusal::TooManyBoxes`] past the cap.
    pub fn spend_box(&mut self) -> Result<(), Refusal> {
        self.boxes += 1;
        if self.boxes > self.limits.max_boxes {
            return Err(Refusal::TooManyBoxes { boxes: self.boxes });
        }
        Ok(())
    }

    /// Charges break opportunities evaluated.
    ///
    /// # Errors
    /// [`Refusal::TooMuchLineBreaking`] past the cap.
    pub fn spend_breaks(&mut self, count: usize) -> Result<(), Refusal> {
        self.breaks = self.breaks.saturating_add(count);
        if self.breaks > self.limits.max_break_work {
            return Err(Refusal::TooMuchLineBreaking {
                evaluations: self.breaks,
            });
        }
        Ok(())
    }

    /// Charges float examinations, CSS 2.2 §9.5.
    ///
    /// **Before the loop, not inside it.** A scan of the placed floats costs
    /// one unit per float and the count is known before it starts, so a book
    /// past the cap is refused rather than swept — `tinker-pdf-zip`'s posture,
    /// and [`limits::MAX_LINE_BREAK_WORK`]'s.
    ///
    /// # Errors
    /// [`Refusal::TooMuchLayoutWork`] past the cap.
    pub fn spend_layout(&mut self, count: usize) -> Result<(), Refusal> {
        self.layout = self.layout.saturating_add(count);
        if self.layout > self.limits.max_layout_work {
            return Err(Refusal::TooMuchLayoutWork {
                examinations: self.layout,
            });
        }
        Ok(())
    }

    /// Boxes generated so far.
    #[must_use]
    pub fn boxes(&self) -> usize {
        self.boxes
    }

    /// Float examinations so far.
    #[must_use]
    pub fn layout(&self) -> usize {
        self.layout
    }

    /// Break opportunities evaluated so far.
    #[must_use]
    pub fn breaks(&self) -> usize {
        self.breaks
    }
}

/// What this crate refuses, each by its own name.
///
/// `Eq` is deliberately absent where every sibling crate's `Refusal` has it:
/// [`Refusal::PageTooSmall`] carries the caller's own two `f64`s so a report
/// can say *which* number was unusable, and a `NaN` page width — which is
/// exactly the input that produces this variant — is not equal to itself.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Refusal {
    /// A box tree deeper than [`limits::MAX_BOX_DEPTH`].
    TooDeep {
        /// How deep it went.
        depth: usize,
    },
    /// The book's box total is spent.
    TooManyBoxes {
        /// How many had been generated.
        boxes: usize,
    },
    /// The book's line-breaking total is spent.
    TooMuchLineBreaking {
        /// How many opportunities had been evaluated.
        evaluations: usize,
    },
    /// The book's float-placement total is spent. Milestone 10's cap, and the
    /// one milestone 7 argued would arrive with the multi-pass layout.
    TooMuchLayoutWork {
        /// How many floats had been examined.
        examinations: usize,
    },
    /// The flow fragments into more pages than [`limits::MAX_LAYOUT_PAGES`].
    TooManyPages {
        /// How many it would have been.
        pages: usize,
    },
    /// A page box with no usable area. A caller error rather than a hostile
    /// file, and refused by name because the alternative is an infinite number
    /// of empty pages.
    PageTooSmall {
        /// The width asked for.
        width: f64,
        /// The height asked for.
        height: f64,
    },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::TooDeep { depth } => write!(f, "a box tree {depth} deep is past the cap"),
            Refusal::TooManyBoxes { boxes } => {
                write!(f, "{boxes} boxes is past the book's total")
            }
            Refusal::TooMuchLineBreaking { evaluations } => {
                write!(
                    f,
                    "{evaluations} break evaluations is past the book's total"
                )
            }
            Refusal::TooMuchLayoutWork { examinations } => {
                write!(
                    f,
                    "{examinations} float examinations is past the book's total"
                )
            }
            Refusal::TooManyPages { pages } => write!(f, "{pages} pages is past the cap"),
            Refusal::PageTooSmall { width, height } => {
                write!(f, "a page of {width} by {height} has no room for a line")
            }
        }
    }
}

impl std::error::Error for Refusal {}

/// What this crate could not honour, said out loud.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Warning {
    /// A float whose box did not fit the page it began on and was broken
    /// across the boundary. Milestone 10.
    ///
    /// `css-break-3` would **push** a float that would fit whole on the next
    /// page rather than break it, and this build breaks it: pushing changes
    /// which lines are shortened beside it, so it is a second layout of the
    /// content and not a second position for the box. Nothing is lost either
    /// way — the whole float is set, on two pages instead of one — and saying
    /// so is the difference between a known gap and a figure that quietly
    /// straddles a page.
    FloatBrokenAcrossPages,
    /// `display: inline-block`, laid out as ordinary inline text: its `width`,
    /// its `height` and its vertical margins do not apply to it.
    ///
    /// **It used to say the opposite, and it used to be unreachable.** The
    /// variant was `InlineBlockAsBlock` and it was raised where a block-level
    /// box is built — which an `inline-block` never reaches, because
    /// `Consumed::is_block_level` sends it down the inline path. Milestone 10
    /// found it by needing a warning it could count, and the two halves of the
    /// fix are one change: it is raised where inline content is gathered, and
    /// it now names what is actually done. See the crate's `Still owed`.
    InlineBlockAsInline,
    /// An inline box with a block-level child, laid out as a block container.
    /// CSS 2.2 §9.2.1.1 splits the inline instead.
    BlockInInline,
    /// A line whose content does not fit and had nowhere to break — the word
    /// is longer than the line and `overflow-wrap` is `normal`, which is what
    /// the specification says to do and is still worth reporting.
    LineOverflowed,
    /// A block taller than a whole page, which had to be broken somewhere no
    /// rule permits. CSS 2.2 §13.3.3's own escape: *"if the above does not
    /// provide enough break points, rules B and D are dropped"*, and then A
    /// and C.
    BreakForcedPastTheRules,
    /// Content that did not fit the width and was drawn outside the page box.
    ContentOverflowedPage,
    /// A table row taller than a whole page, drawn past the page bottom.
    /// Milestone 11.
    ///
    /// **This is the staged half of table fragmentation and it is named rather
    /// than silent.** A table breaks *between* its rows, which is where CSS
    /// 2.2 §13.3.3 puts a break position and is what a real book's table needs;
    /// a row taller than the page has no such position inside it and this build
    /// draws it anyway rather than dropping it. Slicing a row's cells at a line
    /// boundary — every cell cut at the same height, each continuing on the
    /// next page — is `css-break-3`'s and is not here. See the crate's `Still
    /// owed` and gap 31's milestone 11 row, amended in place.
    TableRowTallerThanPage,
    /// A table cell whose `rowspan` reaches past the last row of its row group,
    /// clamped to it. CSS 2.2 §17.5: *"the cell is clamped so that it does not
    /// extend beyond the last row"*.
    RowspanPastTheRowGroup,
    /// `display: inline-flex`, laid out as a **block-level** flex container.
    ///
    /// `css-flexbox-1` §3 makes it inline-level, and this build has no
    /// inline-level box that is not text. The two available answers are to set
    /// it as inline text, which throws the flex layout away entirely, or to lay
    /// it out as a block-level flex container, which gets the box's *outside*
    /// wrong and everything inside it right. It takes the second.
    ///
    /// **Distinct from [`Warning::InlineBlockAsInline`], which took the other
    /// answer**, and the two disagree for a reason rather than by accident: an
    /// `inline-block` holding a sentence set as inline text is very nearly
    /// right, and a flex container set as inline text is a column of words with
    /// no layout in it at all.
    InlineFlexAsBlock,
    /// A flex line taller than a whole page, drawn past the page bottom.
    ///
    /// **The same staged half as [`Warning::TableRowTallerThanPage`] and for
    /// the same reason.** `css-flexbox-1` §11 fragments a flex container
    /// between its lines in a row container and inside a line in a column one;
    /// a line taller than the page has no break position inside it here, and
    /// this build draws it anyway rather than dropping it. A `column` container
    /// is one line whatever its length, so this is the warning a long
    /// `flex-direction: column` raises.
    FlexLineTallerThanPage,
    /// `display: table-column` or `table-column-group` carrying a `width`,
    /// which this build reads, beside anything else on it, which it does not:
    /// a column box's background and borders are §17.5.1's two rendering
    /// layers and neither is painted here.
    ColumnBoxNotPainted,
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Warning::FloatBrokenAcrossPages => {
                f.write_str("a float did not fit its page and was broken across the boundary")
            }
            Warning::InlineBlockAsInline => {
                f.write_str("display: inline-block is laid out as inline text")
            }
            Warning::BlockInInline => {
                f.write_str("an inline box holds a block, and is laid out as one")
            }
            Warning::LineOverflowed => f.write_str("a line had nowhere to break and overflowed"),
            Warning::BreakForcedPastTheRules => {
                f.write_str("a page break was taken where CSS 2.2 13.3.3 permits none")
            }
            Warning::ContentOverflowedPage => f.write_str("content is wider than the page box"),
            Warning::TableRowTallerThanPage => {
                f.write_str("a table row is taller than a page and overflows it")
            }
            Warning::RowspanPastTheRowGroup => {
                f.write_str("a rowspan reaches past its row group and was clamped")
            }
            Warning::ColumnBoxNotPainted => {
                f.write_str("a table column box's background and borders are not painted")
            }
            Warning::InlineFlexAsBlock => {
                f.write_str("display: inline-flex is laid out as a block-level flex container")
            }
            Warning::FlexLineTallerThanPage => {
                f.write_str("a flex line is taller than a page and overflows it")
            }
        }
    }
}

/// Lays a box tree out into pages.
///
/// # Errors
/// Any [`Refusal`]: a cap, or a page box with no usable area.
pub fn layout<M: metrics::Metrics>(
    root: &BoxNode,
    metrics: &M,
    options: &Options,
    limits: &Limits,
) -> Result<Layout, Refusal> {
    let mut budget = Budget::new(limits);
    layout_with(root, metrics, options, limits, &mut budget)
}

/// The same, against a budget shared across a whole book.
///
/// # Errors
/// Any [`Refusal`].
pub fn layout_with<M: metrics::Metrics>(
    root: &BoxNode,
    metrics: &M,
    options: &Options,
    limits: &Limits,
    budget: &mut Budget,
) -> Result<Layout, Refusal> {
    if !(options.width.is_finite() && options.height.is_finite())
        || options.width <= 0.0
        || options.height <= 0.0
    {
        return Err(Refusal::PageTooSmall {
            width: options.width,
            height: options.height,
        });
    }
    let flow = flow::build(root, metrics, options, limits, budget)?;
    fragment::paginate(flow, options, limits)
}
