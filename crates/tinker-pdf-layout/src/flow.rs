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
//! # The collapsed value is not a maximum
//!
//! §8.3.1: *"the maximum of the positive adjoining margins, plus the minimum of
//! the negative ones"*. A build that took `max()` over signed values gets every
//! ordinary book right and every negative margin wrong, and a negative margin
//! is what a book uses to pull a drop cap up.

use std::collections::HashMap;

use tinker_pdf_css::property::{
    BorderStyle, Clear, Color, Display, Float, LengthPercentage, ListStyleType, MarginValue,
    OverflowWrap, PageBreak, PageBreakInside, Side, Sides, Size, TextAlign,
};

use crate::metrics::Metrics;
use crate::style::{consume, Consumed};
use crate::text::{self, Collapser};
use crate::uax14;
use crate::{BoxNode, Budget, Content, Limits, Options, Refusal, TextRun, Warning};

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

/// What a flow item is.
#[derive(Clone, Debug)]
pub(crate) enum ItemKind {
    /// A collapsed margin. Breaking here is §13.3.3's case (1).
    Margin(MarginBreak),
    /// A border or a padding edge. Nothing may break inside one.
    Edge,
    /// A line box. Breaking before one is §13.3.3's case (2).
    Line(Box<LineBox>),
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

/// A whole book as one continuous column, before it is cut into pages.
#[derive(Clone, Debug, Default)]
pub(crate) struct Flow {
    pub items: Vec<Item>,
    pub blocks: Vec<BlockRecord>,
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
}

/// One run of text in an inline formatting context, after phase I.
struct Piece {
    text: String,
    style: Consumed,
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
        let style = consume(&node.style);
        if style.is_none() {
            return Ok(());
        }
        self.budget.spend_box()?;
        if style.float != Float::None {
            self.warn(Warning::FloatInFlow(style.float));
        }
        if style.clear != Clear::None {
            self.warn(Warning::ClearIgnored(style.clear));
        }
        if style.display == Display::InlineBlock {
            self.warn(Warning::InlineBlockAsBlock);
        }
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

        // The top margin joins whatever is adjoining, and the box's
        // `page-break-before` joins the break position that margin is. The
        // avoid set is taken **before** this box is opened, because an element
        // is not its own ancestor.
        self.pending.breaks.push(style.page_break_before);
        self.pending.meet(&self.open_avoid.clone());
        self.pending.add(margin_top);

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
        self.children(node, &style, content_x, content_width, depth, avoid, block)?;
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
                        self.gather(child, &mut pieces, &mut collapser, depth + 1)?;
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
                for (child, child_style) in children.iter().zip(&styles) {
                    if child_style.is_none() {
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
            self.gather(child, &mut pieces, &mut collapser, depth + 1)?;
        }
        self.lines(&pieces, style, block, content_x, content_width)
    }

    /// Collects one inline-level subtree's text, phase I applied **across**
    /// the whole context.
    fn gather(
        &mut self,
        node: &BoxNode,
        out: &mut Vec<Piece>,
        collapser: &mut Collapser,
        depth: usize,
    ) -> Result<(), Refusal> {
        if depth > self.limits.max_depth {
            return Err(Refusal::TooDeep { depth });
        }
        let style = consume(&node.style);
        if style.is_none() {
            return Ok(());
        }
        self.budget.spend_box()?;
        match &node.content {
            Content::Text(source) => {
                let text = collapser.push(source, style.white_space);
                if !text.is_empty() {
                    out.push(Piece { text, style });
                }
            }
            Content::Children(children) => {
                for child in children {
                    if consume(&child.style).is_block_level() {
                        self.warn(Warning::BlockInInline);
                    }
                    self.gather(child, out, collapser, depth + 1)?;
                }
            }
        }
        Ok(())
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
            let available = (content_width - if first_line { indent } else { 0.0 }).max(0.0);
            // `cursor` is where the previous line stopped looking, and it is
            // not an optimisation. Restarting the scan at zero for every line
            // makes filling a paragraph `O(lines x opportunities)`, which for a
            // page one point wide is `O(characters^2)` -- and a paragraph is
            // exactly the input a hostile book has an unlimited supply of.
            // `5adf502`'s finding, in the loop rather than in the recursion.
            while cursor < opportunities.len() && opportunities[cursor].at <= start {
                cursor += 1;
            }
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
                &content,
                &spans,
                pieces,
                container,
                block,
                content_x + if first_line { indent } else { 0.0 },
                (content_width - if first_line { indent } else { 0.0 }).max(0.0),
                trim_start,
                trim_end,
                justify,
                lines_here,
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
        self.emit(height, ItemKind::Line(Box::new(line)), true);
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
