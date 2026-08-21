//! Tables: CSS 2.2 §17's box tree, both width algorithms and both border
//! models (gap 31, milestone 11).
//!
//! Everything here is a **pure function of the box tree**. Nothing in this file
//! measures text, emits an item or touches a budget; the driver that does lives
//! in [`crate::flow`] and calls into this. The split is not tidiness — it is
//! what lets §17.2.1's generation, §17.5.2.2's two passes and §17.6.2.1's
//! conflict resolution each be asserted on their own inputs rather than through
//! a laid-out page, which is the failure mode the injection campaign found
//! costs a milestone: *a defect caught only by a broad end-to-end fixture is
//! not caught*.
//!
//! # §17.2.1's generation is a sequence, and each step is separately omittable
//!
//! This is the fixup a real book actually needs, and **which part of it a real
//! book needs was measured rather than assumed**. Hand-written and legacy HTML
//! omits `<tbody>`, and XML has no tree-construction stage to put it back, so
//! such a `<table>` arrives here with bare `<tr>` children and something has to
//! generate the row group. The two producers in gap 31's committed corpus turn
//! out **not** to be that case — pandoc and calibre both write `<thead>` and
//! `<tbody>` in full — and what they do write is *indentation*, so the step
//! that fires on every real table here is rule 3 rather than the row group.
//! `epub_tables.rs`'s corpus fixture asserts that measurement in the direction
//! it came out. Generating the row group is
//! one of **nine** steps, and the reason each is its own named function with
//! its own counter is that eight of them produce a table that
//! looks right: an implementation missing step 3 draws a table with a stray
//! empty cell in it, one missing step 7 loses a `<div>` a producer put between
//! two `<td>`s, and neither shows up in a page count.
//!
//! Eight of the nine steps are [`generate`]'s and are counted in [`Generated`];
//! the ninth is [`misparented_run`], which runs one level up because its input
//! is a block container's child list rather than a table's.
//!
//! Two of §17.2.1's own nine rules are **not** two steps here, and saying which
//! is the point of the list:
//!
//! - **Rule 8** — *"for each `table-cell` box whose parent is not a
//!   `table-row`, generate an anonymous row"* — is not implemented separately,
//!   because it is unreachable once rule 9 and rule 5 are. A misparented cell
//!   is wrapped in an anonymous table by [`misparented_run`], and that table's
//!   children are then not proper table children, so [`Step::RowForTableChild`]
//!   generates the row. A second enforcement would be the same rule twice with
//!   only one half reachable — which is exactly what the injection matrix
//!   found hides a defect in `epub/xhtml.rs`'s end-tag handling, and the reason
//!   it is written down here rather than left as an omission.
//! - **The row group a book leaves out** is not one of §17.2.1's rules at all;
//!   it is §17.2's box tree, where a table's rows are in row groups. It is
//!   [`Step::GroupForBareRows`] and it is the step this build exists to have.
//!
//! # §17.5.2.2 is two passes and the difference is only visible where it binds
//!
//! [`constraints`] is the first pass — a minimum and a maximum content width
//! per column — and [`distribute`] is the second. A one-pass approximation
//! that hands each column a share of the available width in proportion to its
//! content produces a perfectly plausible table, and it differs from this one
//! **only where a column's minimum is greater than its proportional share**.
//! So the tests assert the two intermediates as well as the answer, and the
//! fixture named for it is one where the one-pass answer would put a column
//! below its own minimum.
//!
//! # §17.6.2.1 is five ordered rules, not a preference
//!
//! [`resolve`] applies them in order and each has a fixture in which it alone
//! decides: `hidden` beats everything, `none` loses to everything, then width,
//! then style, then the box the border was declared on. A build that checked
//! only the width gets four of the five right on every table anybody writes.

use tinker_pdf_css::cascade::ComputedStyle;
use tinker_pdf_css::property::{BorderStyle, Color, Display};

use crate::{BoxNode, CellSpan, Content};

// ---- §17.2.1: the anonymous table objects -----------------------------------

/// One of the generation steps, so a counter and a test can name it.
///
/// An `enum` rather than nine fields for [`Generated`]'s sake: a step added
/// without a counter would not compile, which is `Property::name`'s device one
/// crate down.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Step {
    /// §17.2.1 (1): every child of a `table-column` box is `display: none`.
    DropColumnChildren,
    /// §17.2.1 (2): a child of a `table-column-group` that is not a
    /// `table-column` is `display: none`.
    DropNonColumnFromColumnGroup,
    /// §17.2.1 (3): white space between proper table children of a tabular
    /// container is `display: none`. This is the step that decides whether the
    /// newline a producer wrote between `</td>` and `<td>` becomes a cell.
    DropWhitespaceInContainer,
    /// §17.2.1 (4): white space between two internal table siblings, wherever
    /// they are. Rule 3's parent is a tabular container and rule 4's is
    /// anything at all, which is why they are two rules and two steps.
    DropWhitespaceBetweenSiblings,
    /// §17.2.1 (5): a table's child that is not a proper table child is
    /// wrapped, with its consecutive siblings, in an anonymous `table-row`.
    RowForTableChild,
    /// §17.2.1 (6): a row group's child that is not a `table-row` is wrapped,
    /// with its consecutive siblings, in an anonymous `table-row`.
    RowForRowGroupChild,
    /// §17.2.1 (7): a row's child that is not a `table-cell` is wrapped, with
    /// its consecutive siblings, in an anonymous `table-cell`.
    CellForRowChild,
    /// §17.2: a table's bare `table-row` children go in an anonymous row group.
    /// **The `<tbody>` a real book omits.**
    GroupForBareRows,
}

impl Step {
    /// The eight [`generate`] performs, in the order §17.2.1 states them.
    ///
    /// **The ninth is [`misparented_run`]** and it is deliberately not here:
    /// its input is a *block container's* child list, not a table's, so it runs
    /// before there is a table to generate anything into. A ninth variant with
    /// a counter that could never be bumped would be an assertion that cannot
    /// fail, which is the one thing this milestone's discipline names twice.
    ///
    /// A `const` array rather than a derive so that the tests can sweep it, and
    /// so that a ninth step added without a place in the order fails the sweep
    /// rather than being silently last.
    pub const ALL: [Step; 8] = [
        Step::DropColumnChildren,
        Step::DropNonColumnFromColumnGroup,
        Step::DropWhitespaceInContainer,
        Step::DropWhitespaceBetweenSiblings,
        Step::RowForTableChild,
        Step::RowForRowGroupChild,
        Step::CellForRowChild,
        Step::GroupForBareRows,
    ];
}

/// How many boxes each step generated or removed.
///
/// It exists so a fixture can assert *which* step ran rather than that the
/// table came out looking right. Eight of the nine produce a plausible table
/// when they are missing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Generated {
    counts: [usize; 8],
}

impl Generated {
    /// How many times a step fired.
    #[must_use]
    pub fn count(&self, step: Step) -> usize {
        self.counts[Self::at(step)]
    }

    /// Every step that fired at least once, in [`Step::ALL`]'s order.
    #[must_use]
    pub fn fired(&self) -> Vec<Step> {
        Step::ALL
            .into_iter()
            .filter(|step| self.count(*step) > 0)
            .collect()
    }

    fn bump(&mut self, step: Step) {
        self.counts[Self::at(step)] += 1;
    }

    fn at(step: Step) -> usize {
        match step {
            Step::DropColumnChildren => 0,
            Step::DropNonColumnFromColumnGroup => 1,
            Step::DropWhitespaceInContainer => 2,
            Step::DropWhitespaceBetweenSiblings => 3,
            Step::RowForTableChild => 4,
            Step::RowForRowGroupChild => 5,
            Step::CellForRowChild => 6,
            Step::GroupForBareRows => 7,
        }
    }
}

/// A cell's box: one the document wrote, or one §17.2.1 generated.
///
/// The generated one **owns** its content, and it is the only owned box in this
/// module. An anonymous row and an anonymous row group have no content of their
/// own — their children are already in the structure — so they are an absent
/// borrow. An anonymous cell has to hold the run of siblings it wrapped, and a
/// borrow cannot hold a `Vec` that does not exist anywhere.
#[derive(Clone, Debug)]
pub enum CellBox<'a> {
    /// A `display: table-cell` the document wrote.
    Real(&'a BoxNode),
    /// §17.2.1 rule 7's anonymous cell, holding the run it wrapped.
    Anonymous(Box<BoxNode>),
}

impl CellBox<'_> {
    /// The box to lay out, whichever it is.
    #[must_use]
    pub fn node(&self) -> &BoxNode {
        match self {
            CellBox::Real(node) => node,
            CellBox::Anonymous(node) => node,
        }
    }

    /// Whether §17.2.1 generated it.
    #[must_use]
    pub fn is_anonymous(&self) -> bool {
        matches!(self, CellBox::Anonymous(_))
    }
}

/// One cell of a table, before it has a place in the grid.
#[derive(Clone, Debug)]
pub struct Cell<'a> {
    /// Its box.
    pub content: CellBox<'a>,
    /// `colspan` and `rowspan`, exactly as the caller stated them —
    /// **unclamped**, because `rowspan: 0` means *to the end of this row
    /// group* and the row group is not known until [`Grid::place`] runs.
    pub span: CellSpan,
}

/// One table row.
#[derive(Clone, Debug)]
pub struct Row<'a> {
    /// The `table-row` box, or `None` for one §17.2.1 generated.
    pub node: Option<&'a BoxNode>,
    /// Its cells, in document order.
    pub cells: Vec<Cell<'a>>,
}

/// Which of §17.2's three row-group values a group is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GroupKind {
    /// `table-header-group`, rendered first.
    Header,
    /// `table-row-group`, and the anonymous group a bare row goes in.
    Body,
    /// `table-footer-group`, rendered last **however early it was written** —
    /// and HTML 4.01 required it to be written before the bodies, so a real
    /// book's `<tfoot>` is read before content that is drawn above it.
    Footer,
}

/// One row group.
#[derive(Clone, Debug)]
pub struct RowGroup<'a> {
    /// The row-group box, or `None` for the anonymous one a bare row goes in.
    pub node: Option<&'a BoxNode>,
    /// Which kind it is.
    pub kind: GroupKind,
    /// Its rows, in document order.
    pub rows: Vec<Row<'a>>,
}

/// One column of the grid, and the boxes that describe it.
#[derive(Clone, Debug, Default)]
pub struct ColumnBox<'a> {
    /// The `table-column` box, if the document declared one for this column.
    pub node: Option<&'a BoxNode>,
    /// Its `table-column-group`, if it had one.
    pub group: Option<&'a BoxNode>,
}

/// A table's box tree, after §17.2.1.
#[derive(Clone, Debug, Default)]
pub struct TableBox<'a> {
    /// `table-caption` boxes, in document order. §17.4 puts them outside the
    /// table box and this build puts every one of them above it; `caption-side`
    /// is `Unsupported` by name, so `caption-side: bottom` is a reported gap
    /// rather than a caption in the wrong place.
    pub captions: Vec<&'a BoxNode>,
    /// The columns the document declared, flattened out of any column groups.
    /// A grid wider than this list has columns nothing described.
    pub columns: Vec<ColumnBox<'a>>,
    /// The row groups, in **document** order. [`TableBox::visual_groups`] is
    /// the other one.
    pub groups: Vec<RowGroup<'a>>,
    /// What generation did.
    pub generated: Generated,
}

impl<'a> TableBox<'a> {
    /// The row groups in the order §17.2 renders them: every header group,
    /// then every body group, then every footer group, each set in document
    /// order.
    ///
    /// **This is the second place in this crate where reading order stops
    /// being emission order**, and the first was milestone 10's floats. A
    /// `<tfoot>` written before the `<tbody>` — which HTML 4.01 required and
    /// which real books therefore contain — is read where it was written and
    /// drawn at the bottom. Every run carries [`crate::TextRun::order`], the
    /// cells are laid out in **document** order and the rows are emitted in
    /// **this** one, which is what keeps text conservation an ordered
    /// comparison that passes.
    #[must_use]
    pub fn visual_groups(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.groups.len()).collect();
        // A stable sort by kind, so two bodies keep their document order.
        order.sort_by_key(|at| self.groups[*at].kind);
        order
    }

    /// Every row, in the order they are rendered, as `(group, row)`.
    #[must_use]
    pub fn visual_rows(&self) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for group in self.visual_groups() {
            for row in 0..self.groups[group].rows.len() {
                out.push((group, row));
            }
        }
        out
    }
}

/// Whether a box is `display: none`, which generates nothing at all.
fn is_none(node: &BoxNode) -> bool {
    node.style.display == Display::None
}

/// Whether a box is an anonymous inline box holding only white space, which is
/// what §17.2.1's rules 3 and 4 are about.
///
/// A node with **children** is not one however empty its text is: §17.2.1 says
/// *"an anonymous inline box that contains only white space"*, and a `<span>`
/// holding a space is a box the document wrote.
fn is_whitespace(node: &BoxNode) -> bool {
    match &node.content {
        Content::Text(text) => text.chars().all(char::is_whitespace),
        Content::Children(_) => false,
    }
}

/// The children a box has, or nothing when its content is text.
fn kids(node: &BoxNode) -> &[BoxNode] {
    match &node.content {
        Content::Children(children) => children,
        Content::Text(_) => &[],
    }
}

/// An anonymous box's style, §17.2.1: *"inherit from the table box"*, with
/// every non-inherited property at its initial value.
fn anonymous_style(parent: &ComputedStyle, display: Display) -> ComputedStyle {
    let mut style = ComputedStyle::inherit_from(parent);
    style.display = display;
    style
}

/// An anonymous cell wrapping a run of boxes, §17.2.1 rule 7.
fn anonymous_cell(parent: &ComputedStyle, run: Vec<BoxNode>) -> CellBox<'static> {
    CellBox::Anonymous(Box::new(BoxNode {
        style: anonymous_style(parent, Display::TableCell),
        content: Content::Children(run),
        anchor: None,
        span: CellSpan::ONE,
    }))
}

/// An anonymous cell wrapping a table's or a row's own text content.
fn anonymous_text_cell(parent: &ComputedStyle, text: &str) -> CellBox<'static> {
    let mut inner = ComputedStyle::inherit_from(parent);
    inner.display = Display::Inline;
    anonymous_cell(
        parent,
        vec![BoxNode {
            style: inner,
            content: Content::Text(text.to_owned()),
            anchor: None,
            span: CellSpan::ONE,
        }],
    )
}

/// §17.2.1, applied to one `display: table` box.
///
/// The steps run in the order [`Step::ALL`] states them, which is §17.2.1's own
/// order and matters: a whitespace box removed by step 3 must be gone before
/// step 5 could wrap it in a row, and a bare row must exist before step 8 can
/// group it.
#[must_use]
pub fn generate<'a>(table: &'a BoxNode) -> TableBox<'a> {
    let mut out = TableBox::default();
    let children = kids(table);

    // A table whose whole content is text has one anonymous row holding one
    // anonymous cell holding it -- rules 5 and 7 with nothing to iterate over.
    if let Content::Text(text) = &table.content {
        if text.chars().all(char::is_whitespace) {
            out.generated.bump(Step::DropWhitespaceInContainer);
            return out;
        }
        out.generated.bump(Step::RowForTableChild);
        out.generated.bump(Step::CellForRowChild);
        out.groups.push(RowGroup {
            node: None,
            kind: GroupKind::Body,
            rows: vec![Row {
                node: None,
                cells: vec![Cell {
                    content: anonymous_text_cell(&table.style, text),
                    span: CellSpan::ONE,
                }],
            }],
        });
        out.generated.bump(Step::GroupForBareRows);
        return out;
    }

    // Pass one over the table's children: classify, drop, and collect the runs
    // that need a row around them.
    let mut stray: Vec<&'a BoxNode> = Vec::new();
    for (at, child) in children.iter().enumerate() {
        if is_none(child) {
            continue;
        }
        let display = child.style.display;
        if is_whitespace(child) {
            // Step 3 is about a tabular container's own children, which is
            // where the newline between `</tr>` and `<tr>` lives. It applies
            // when the neighbours on **both** sides -- if there are any -- are
            // proper table descendants, which is why it is not simply "drop
            // every whitespace child".
            if surrounded_by_table_boxes(children, at) {
                out.generated.bump(Step::DropWhitespaceInContainer);
                continue;
            }
        }
        match display {
            Display::TableCaption => {
                flush_stray(&mut stray, &mut out, table);
                out.captions.push(child);
            }
            Display::TableColumn => {
                flush_stray(&mut stray, &mut out, table);
                // Step 1: a column's children generate nothing, so they are
                // counted and not walked. `span` is HTML's `<col span>`.
                for kid in kids(child) {
                    if !is_none(kid) {
                        out.generated.bump(Step::DropColumnChildren);
                    }
                }
                for _ in 0..child.span.columns.max(1) {
                    out.columns.push(ColumnBox {
                        node: Some(child),
                        group: None,
                    });
                }
            }
            Display::TableColumnGroup => {
                flush_stray(&mut stray, &mut out, table);
                let mut declared = 0usize;
                for kid in kids(child) {
                    if is_none(kid) {
                        continue;
                    }
                    if kid.style.display == Display::TableColumn {
                        for _ in 0..kid.span.columns.max(1) {
                            out.columns.push(ColumnBox {
                                node: Some(kid),
                                group: Some(child),
                            });
                            declared += 1;
                        }
                        for grandchild in kids(kid) {
                            if !is_none(grandchild) {
                                out.generated.bump(Step::DropColumnChildren);
                            }
                        }
                    } else {
                        // Step 2, and it is **not** step 1: this one is about
                        // a child of a column *group* that is not a column at
                        // all, which a `<colgroup>` holding a stray `<div>`
                        // produces.
                        out.generated.bump(Step::DropNonColumnFromColumnGroup);
                    }
                }
                // A `<colgroup span="3">` with no `<col>` in it declares three
                // columns, which is HTML's own reading of the attribute.
                if declared == 0 {
                    for _ in 0..child.span.columns.max(1) {
                        out.columns.push(ColumnBox {
                            node: None,
                            group: Some(child),
                        });
                    }
                }
            }
            Display::TableRowGroup | Display::TableHeaderGroup | Display::TableFooterGroup => {
                flush_stray(&mut stray, &mut out, table);
                let kind = match display {
                    Display::TableHeaderGroup => GroupKind::Header,
                    Display::TableFooterGroup => GroupKind::Footer,
                    _ => GroupKind::Body,
                };
                let rows = row_group_rows(child, &mut out.generated);
                out.groups.push(RowGroup {
                    node: Some(child),
                    kind,
                    rows,
                });
            }
            Display::TableRow => {
                flush_stray(&mut stray, &mut out, table);
                let row = Row {
                    node: Some(child),
                    cells: row_cells(child, &mut out.generated),
                };
                // Step 8: a bare row goes in an anonymous row group, and
                // consecutive bare rows go in the **same** one -- one `<tbody>`
                // for a `<table>` of `<tr>`s, not one each.
                match out.groups.last_mut() {
                    Some(group) if group.node.is_none() && group.kind == GroupKind::Body => {
                        group.rows.push(row);
                    }
                    _ => {
                        out.generated.bump(Step::GroupForBareRows);
                        out.groups.push(RowGroup {
                            node: None,
                            kind: GroupKind::Body,
                            rows: vec![row],
                        });
                    }
                }
            }
            _ => stray.push(child),
        }
    }
    flush_stray(&mut stray, &mut out, table);
    out
}

/// Whether the neighbours of a whitespace child are table boxes, §17.2.1 (3).
///
/// *"its immediately preceding and following siblings, if any, are proper table
/// descendants"* — **if any**, so a whitespace-only child that is the table's
/// only child is removed, and one standing between a `<td>` and a paragraph is
/// not.
fn surrounded_by_table_boxes(children: &[BoxNode], at: usize) -> bool {
    let before = children[..at]
        .iter()
        .rev()
        .find(|node| !is_none(node) && !is_whitespace(node));
    let after = children[at + 1..]
        .iter()
        .find(|node| !is_none(node) && !is_whitespace(node));
    let proper = |node: Option<&BoxNode>| match node {
        None => true,
        Some(node) => node.style.display.is_internal_table(),
    };
    proper(before) && proper(after)
}

/// Step 5: the run of a table's children that are not proper table children,
/// wrapped in one anonymous row holding one anonymous cell.
fn flush_stray<'a>(stray: &mut Vec<&'a BoxNode>, out: &mut TableBox<'a>, table: &'a BoxNode) {
    if stray.is_empty() {
        return;
    }
    let run: Vec<BoxNode> = stray.drain(..).cloned().collect();
    out.generated.bump(Step::RowForTableChild);
    out.generated.bump(Step::CellForRowChild);
    let row = Row {
        node: None,
        cells: vec![Cell {
            content: anonymous_cell(&table.style, run),
            span: CellSpan::ONE,
        }],
    };
    match out.groups.last_mut() {
        Some(group) if group.node.is_none() && group.kind == GroupKind::Body => {
            group.rows.push(row);
        }
        _ => {
            out.generated.bump(Step::GroupForBareRows);
            out.groups.push(RowGroup {
                node: None,
                kind: GroupKind::Body,
                rows: vec![row],
            });
        }
    }
}

/// Step 6: a run of a row group's non-row children, in one anonymous row
/// holding one anonymous cell.
fn flush_group_stray<'a>(
    stray: &mut Vec<&'a BoxNode>,
    rows: &mut Vec<Row<'a>>,
    group: &'a BoxNode,
    generated: &mut Generated,
) {
    if stray.is_empty() {
        return;
    }
    let run: Vec<BoxNode> = stray.drain(..).cloned().collect();
    generated.bump(Step::RowForRowGroupChild);
    generated.bump(Step::CellForRowChild);
    rows.push(Row {
        node: None,
        cells: vec![Cell {
            content: anonymous_cell(&group.style, run),
            span: CellSpan::ONE,
        }],
    });
}

/// Step 7: a run of a row's non-cell children, in one anonymous cell.
fn flush_row_stray<'a>(
    stray: &mut Vec<&'a BoxNode>,
    cells: &mut Vec<Cell<'a>>,
    row: &'a BoxNode,
    generated: &mut Generated,
) {
    if stray.is_empty() {
        return;
    }
    let run: Vec<BoxNode> = stray.drain(..).cloned().collect();
    generated.bump(Step::CellForRowChild);
    cells.push(Cell {
        content: anonymous_cell(&row.style, run),
        span: CellSpan::ONE,
    });
}

/// A row group's rows, with step 6 for anything that is not one.
fn row_group_rows<'a>(group: &'a BoxNode, generated: &mut Generated) -> Vec<Row<'a>> {
    let mut rows: Vec<Row<'a>> = Vec::new();
    let children = kids(group);
    let mut stray: Vec<&'a BoxNode> = Vec::new();
    for (at, child) in children.iter().enumerate() {
        if is_none(child) {
            continue;
        }
        if is_whitespace(child) && surrounded_by_table_boxes(children, at) {
            generated.bump(Step::DropWhitespaceInContainer);
            continue;
        }
        if child.style.display == Display::TableRow {
            flush_group_stray(&mut stray, &mut rows, group, generated);
            let cells = row_cells(child, generated);
            rows.push(Row {
                node: Some(child),
                cells,
            });
        } else {
            stray.push(child);
        }
    }
    flush_group_stray(&mut stray, &mut rows, group, generated);
    if let Content::Text(text) = &group.content {
        if text.chars().all(char::is_whitespace) {
            generated.bump(Step::DropWhitespaceInContainer);
        } else {
            generated.bump(Step::RowForRowGroupChild);
            generated.bump(Step::CellForRowChild);
            rows.push(Row {
                node: None,
                cells: vec![Cell {
                    content: anonymous_text_cell(&group.style, text),
                    span: CellSpan::ONE,
                }],
            });
        }
    }
    rows
}

/// A row's cells, with step 7 for anything that is not one.
fn row_cells<'a>(row: &'a BoxNode, generated: &mut Generated) -> Vec<Cell<'a>> {
    let mut cells: Vec<Cell<'a>> = Vec::new();
    let children = kids(row);
    let mut stray: Vec<&'a BoxNode> = Vec::new();
    for (at, child) in children.iter().enumerate() {
        if is_none(child) {
            continue;
        }
        if is_whitespace(child) && surrounded_by_table_boxes(children, at) {
            generated.bump(Step::DropWhitespaceInContainer);
            continue;
        }
        if child.style.display == Display::TableCell {
            flush_row_stray(&mut stray, &mut cells, row, generated);
            cells.push(Cell {
                content: CellBox::Real(child),
                span: child.span,
            });
        } else {
            stray.push(child);
        }
    }
    flush_row_stray(&mut stray, &mut cells, row, generated);
    if let Content::Text(text) = &row.content {
        if text.chars().all(char::is_whitespace) {
            generated.bump(Step::DropWhitespaceInContainer);
        } else {
            generated.bump(Step::CellForRowChild);
            cells.push(Cell {
                content: anonymous_text_cell(&row.style, text),
                span: CellSpan::ONE,
            });
        }
    }
    cells
}

/// §17.2.1 rule 4: white space **between** two internal table siblings, in a
/// parent that is not a tabular container at all.
///
/// It is a separate function from rule 3 because it is a separate rule with a
/// separate reachable case: rule 3's parent is a table or a row parent, and
/// this one's is the `<div>` that rule 9 is about. A build with only rule 3
/// wraps the newline between two misparented `<td>`s in a cell of its own, and
/// the table it draws has an extra empty column in it.
#[must_use]
pub fn is_whitespace_between_table_boxes(children: &[BoxNode], at: usize) -> bool {
    if !is_whitespace(&children[at]) {
        return false;
    }
    let before = children[..at].iter().rev().find(|node| !is_none(node));
    let after = children[at + 1..].iter().find(|node| !is_none(node));
    matches!((before, after), (Some(b), Some(a))
        if b.style.display.is_internal_table() && a.style.display.is_internal_table())
}

/// §17.2.1 rule 9: one past the last of the run of misparented internal table
/// boxes beginning at `from`.
///
/// White space inside the run does not end it — that is rule 4, applied here
/// rather than by deleting boxes, because these children belong to somebody
/// else's list and this module does not own it.
///
/// Returns `from` when the child at `from` is not an internal table box at all,
/// which is the caller's "no anonymous table here".
#[must_use]
pub fn misparented_run(children: &[BoxNode], from: usize) -> usize {
    if from >= children.len() || !children[from].style.display.is_internal_table() {
        return from;
    }
    let mut end = from + 1;
    let mut at = from + 1;
    while at < children.len() {
        let child = &children[at];
        if is_none(child) {
            at += 1;
            continue;
        }
        if child.style.display.is_internal_table() {
            at += 1;
            end = at;
            continue;
        }
        if is_whitespace_between_table_boxes(children, at) {
            at += 1;
            continue;
        }
        break;
    }
    end
}

// ---- §17.5: the grid --------------------------------------------------------

/// Where one cell sits in the grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    /// Which group it is in, indexing [`TableBox::groups`].
    pub group: usize,
    /// Which row of that group, counting from zero.
    pub row: usize,
    /// Which cell of that row, counting from zero.
    pub cell: usize,
    /// The grid row it starts at, counting across the whole table in **visual**
    /// order.
    pub top: usize,
    /// The grid column it starts at.
    pub left: usize,
    /// How many columns it occupies, at least one.
    pub columns: usize,
    /// How many rows it occupies, at least one, already clamped to its row
    /// group.
    pub rows: usize,
}

/// The whole grid: every cell, placed.
#[derive(Clone, Debug, Default)]
pub struct Grid {
    /// The cells, in the order they were placed — which is visual row order,
    /// and therefore not document order once a `<tfoot>` is written early.
    pub slots: Vec<Slot>,
    /// How many columns the widest row needed.
    pub columns: usize,
    /// How many grid rows there are.
    pub rows: usize,
    /// Whether any `rowspan` had to be clamped to its row group.
    pub clamped: usize,
}

impl Grid {
    /// Places every cell, CSS 2.2 §17.5's *"cells are placed in the first
    /// available slot"*.
    ///
    /// `charge` is called once per slot **before** it is occupied, so the
    /// caller can refuse a `colspan="4000000000"` before this allocates for it
    /// — the posture [`crate::limits::MAX_LINE_BREAK_WORK`] already takes and
    /// the reason a hostile span costs a refusal rather than a gigabyte.
    ///
    /// # Errors
    /// Whatever `charge` returns.
    pub fn place<E>(
        table: &TableBox<'_>,
        mut charge: impl FnMut(usize) -> Result<(), E>,
    ) -> Result<Grid, E> {
        let mut grid = Grid::default();
        // Occupancy, one row of `Vec<bool>` per grid row, grown as needed.
        let mut taken: Vec<Vec<bool>> = Vec::new();
        let mut top = 0usize;
        for (group, at) in group_row_bounds(table) {
            let rows_left = at.1 - at.0;
            for row in at.0..at.1 {
                let in_group = row - at.0;
                let mut column = 0usize;
                for (cell, spec) in table.groups[group].rows[row].cells.iter().enumerate() {
                    let columns = spec.span.columns.max(1) as usize;
                    // `rowspan: 0` is HTML's *"to the end of the row group"*,
                    // and a span past the group's last row is clamped there:
                    // CSS 2.2 §17.5 says a cell *"is clamped so that it does
                    // not extend beyond the last row"*.
                    let asked = if spec.span.rows == 0 {
                        rows_left - in_group
                    } else {
                        spec.span.rows as usize
                    }
                    .max(1);
                    let rows = asked.min(rows_left - in_group);
                    if rows < asked {
                        grid.clamped += 1;
                    }
                    charge(columns.saturating_mul(rows))?;
                    while taken.len() <= top + rows {
                        taken.push(Vec::new());
                    }
                    while taken[top].len() > column && taken[top][column] {
                        column += 1;
                    }
                    for line in taken.iter_mut().skip(top).take(rows) {
                        if line.len() < column + columns {
                            line.resize(column + columns, false);
                        }
                        for slot in line.iter_mut().skip(column).take(columns) {
                            *slot = true;
                        }
                    }
                    grid.slots.push(Slot {
                        group,
                        row,
                        cell,
                        top,
                        left: column,
                        columns,
                        rows,
                    });
                    grid.columns = grid.columns.max(column + columns);
                    grid.rows = grid.rows.max(top + rows);
                    column += columns;
                }
                top += 1;
            }
        }
        grid.rows = grid.rows.max(top);
        Ok(grid)
    }
}

/// Every group's rows as `(group, (first, one past last))`, in visual order.
fn group_row_bounds(table: &TableBox<'_>) -> Vec<(usize, (usize, usize))> {
    table
        .visual_groups()
        .into_iter()
        .map(|group| (group, (0, table.groups[group].rows.len())))
        .collect()
}

// ---- §17.5.2: the two width algorithms --------------------------------------

/// §17.5.2.2's first pass: a minimum and a maximum content width per column.
///
/// **The pass that a one-pass approximation does not have.** It is a type of
/// its own, returned by [`constraints`] and taken by [`distribute`], so that a
/// test can assert the intermediate rather than only the answer — which is the
/// difference between asserting the algorithm and asserting a table that came
/// out looking plausible.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Constraints {
    /// Per column, the minimum content width: the widest unbreakable thing in
    /// it.
    pub min: Vec<f64>,
    /// Per column, the maximum content width: the width at which nothing in it
    /// wraps.
    pub max: Vec<f64>,
}

impl Constraints {
    /// MIN: the sum of the minimums.
    #[must_use]
    pub fn total_min(&self) -> f64 {
        self.min.iter().sum()
    }

    /// MAX: the sum of the maximums.
    #[must_use]
    pub fn total_max(&self) -> f64 {
        self.max.iter().sum()
    }
}

/// One cell's contribution to the first pass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellWidths {
    /// The grid column it starts at.
    pub left: usize,
    /// How many columns it spans.
    pub columns: usize,
    /// Its minimum content width, borders and padding included.
    pub min: f64,
    /// Its maximum content width.
    pub max: f64,
    /// Its own `width`, resolved, if it stated one.
    pub specified: Option<f64>,
}

/// §17.5.2.2's **first pass**: cells in, per-column minimums and maximums out.
///
/// Single-column cells set their column's constraints directly. A cell that
/// spans several is applied **after** all of them, which is §17.5.2.2's own
/// order — *"the minimum widths of the columns it spans should be increased so
/// their sum is at least the cell's minimum width"* — and doing it in one pass
/// with the others gives a different and wrong answer: a spanning cell met
/// first would push its whole minimum into the first column it touches and the
/// single-column cells after it would not take it back.
///
/// The deficit is shared in proportion to each column's maximum, and equally
/// where every maximum is zero. CSS 2.2 says only *"should be increased"* and
/// names no distribution; this is the one every browser uses and the note is
/// here because a reader is entitled to know which sentence is the
/// specification's and which is this build's.
#[must_use]
pub fn constraints(columns: usize, cells: &[CellWidths], declared: &[Option<f64>]) -> Constraints {
    let mut out = Constraints {
        min: vec![0.0; columns],
        max: vec![0.0; columns],
    };
    for cell in cells.iter().filter(|cell| cell.columns == 1) {
        if cell.left >= columns {
            continue;
        }
        out.min[cell.left] = out.min[cell.left].max(cell.min);
        out.max[cell.left] = out.max[cell.left].max(cell.max.max(cell.min));
        if let Some(width) = cell.specified {
            out.min[cell.left] = out.min[cell.left].max(cell.min.min(width));
            out.max[cell.left] = out.max[cell.left].max(width);
        }
    }
    // A `<col width>` is a constraint on the column itself, §17.5.2.2's *"the
    // column's specified width"*, and it applies whether or not a cell in it
    // said anything.
    for (at, width) in declared.iter().enumerate().take(columns) {
        if let Some(width) = width {
            out.max[at] = out.max[at].max(*width);
            out.min[at] = out.min[at].max(out.min[at]);
        }
    }
    for cell in cells.iter().filter(|cell| cell.columns > 1) {
        let span = cell.left..(cell.left + cell.columns).min(columns);
        if span.is_empty() {
            continue;
        }
        spread(&mut out.min, span.clone(), cell.min, &out.max);
        let want = cell.max.max(cell.min);
        let maxima = out.max.clone();
        spread(&mut out.max, span, want, &maxima);
    }
    for at in 0..columns {
        out.max[at] = out.max[at].max(out.min[at]);
    }
    out
}

/// Raises a range of values so their sum reaches `want`, in proportion to
/// `weights`.
fn spread(values: &mut [f64], span: std::ops::Range<usize>, want: f64, weights: &[f64]) {
    let have: f64 = values[span.clone()].iter().sum();
    let deficit = want - have;
    if deficit <= 0.0 {
        return;
    }
    let total: f64 = weights[span.clone()].iter().sum();
    let count = span.len() as f64;
    for at in span {
        let share = if total > 0.0 {
            weights[at] / total
        } else {
            1.0 / count
        };
        values[at] += deficit * share;
    }
}

/// §17.5.2.2's **second pass**: the table's used content width over the
/// columns.
///
/// Three cases, and the middle one is the algorithm:
///
/// - at or below MIN, every column gets its minimum and the table overflows;
/// - at or above MAX, every column gets its maximum and the surplus is shared
///   equally, which is CSS 2.2's *"the extra width should be distributed over
///   the columns"* read literally;
/// - **between them, each column gets its minimum plus its own share of the
///   slack, in proportion to how much it could grow.** This is the case a
///   one-pass approximation gets wrong, because a share of the *available*
///   width in proportion to a column's content can be less than that column's
///   own minimum — and a column below its minimum is a table whose text
///   overflows its cell.
#[must_use]
pub fn distribute(constraints: &Constraints, width: f64) -> Vec<f64> {
    let columns = constraints.min.len();
    if columns == 0 {
        return Vec::new();
    }
    let min = constraints.total_min();
    let max = constraints.total_max();
    if width <= min {
        return constraints.min.clone();
    }
    if width >= max {
        let extra = (width - max) / columns as f64;
        return constraints.max.iter().map(|w| w + extra).collect();
    }
    let range = max - min;
    let slack = width - min;
    (0..columns)
        .map(|at| {
            let growth = constraints.max[at] - constraints.min[at];
            let share = if range > 0.0 { growth / range } else { 0.0 };
            constraints.min[at] + slack * share
        })
        .collect()
}

/// §17.5.2.2's table width when `width` is `auto`, which is **shrink to fit**.
///
/// The specification's own sentence is *"the used width is the greater of the
/// table's containing block width, CAPMIN, and MIN. However, if either CAPMIN
/// or the maximum width required by the columns (MAX) is less than that of the
/// containing block, use max(MAX, CAPMIN)."* Read literally the exception
/// swallows the rule: CAPMIN is zero for a table with no caption, zero is less
/// than every containing block, and the first sentence becomes unreachable.
///
/// So this is the reading that agrees with the reference implementation, which
/// is also the only reading under which both halves of that sentence do
/// something: `max(CAPMIN, MIN, min(MAX, available))`. A table narrower than
/// its containing block is its own width — which is what a `<table>` of two
/// short cells looks like in every browser — and one wider than it is the
/// containing block, down to the point where its own minimum stops it and it
/// overflows.
///
/// `available` is the containing block width already less the horizontal
/// spacing and the table's own borders and padding, so every number here is a
/// content width and the comparison is between like things.
#[must_use]
pub fn automatic_width(constraints: &Constraints, available: f64, capmin: f64) -> f64 {
    constraints
        .total_min()
        .max(capmin)
        .max(available.min(constraints.total_max()))
}

/// §17.5.2.1's fixed algorithm: the horizontal layout that does not depend on
/// the contents of the cells.
///
/// The sources, in the specification's own order: the column box's `width`,
/// then — only for the cells of the **first row** — the cell's own `width`
/// divided over the columns it spans, then an equal share of what is left.
///
/// **It is not used when the table's `width` is `auto`**, and that sentence is
/// §17.5.2.1's own: *"a value of `auto` means use the automatic table layout
/// algorithm"*. A build that applied this whenever `table-layout: fixed` was
/// declared lays out every such table at the containing block's width with the
/// columns evenly divided, which looks like a table and is not the one the
/// author asked for. The caller decides; [`crate::flow`]'s dispatch is where
/// the fixture for it points.
#[must_use]
pub fn fixed(
    columns: usize,
    declared: &[Option<f64>],
    first_row: &[CellWidths],
    width: f64,
) -> Vec<f64> {
    let mut out: Vec<Option<f64>> = vec![None; columns];
    for (at, column) in declared.iter().enumerate().take(columns) {
        if let Some(value) = column {
            out[at] = Some(value.max(0.0));
        }
    }
    for cell in first_row {
        let Some(specified) = cell.specified else {
            continue;
        };
        let span = cell.left..(cell.left + cell.columns).min(columns);
        if span.is_empty() {
            continue;
        }
        let each = specified.max(0.0) / span.len() as f64;
        for at in span {
            if out[at].is_none() {
                out[at] = Some(each);
            }
        }
    }
    let assigned: f64 = out.iter().flatten().sum();
    let auto = out.iter().filter(|w| w.is_none()).count();
    if auto > 0 {
        let each = ((width - assigned) / auto as f64).max(0.0);
        for slot in out.iter_mut() {
            if slot.is_none() {
                *slot = Some(each);
            }
        }
        return out.into_iter().flatten().collect();
    }
    // Every column stated a width. §17.5.2.1: *"if the sum is less than the
    // table's width, the extra space is distributed over the columns"*.
    let mut widths: Vec<f64> = out.into_iter().flatten().collect();
    if assigned < width && !widths.is_empty() {
        let extra = (width - assigned) / widths.len() as f64;
        for value in &mut widths {
            *value += extra;
        }
    }
    widths
}

// ---- §17.6.2.1: border conflict resolution ----------------------------------

/// Which box a border was declared on, CSS 2.2 §17.6.2.1's rule 5.
///
/// The order of the variants **is** the rule: *"borders of the cell win over
/// the row, which wins over the row group, the column, the column group and,
/// last, the table"*. Deriving `Ord` from that order is what stops a second
/// place in this file from having a second opinion about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Origin {
    /// The table box. Lowest priority.
    Table,
    /// A `table-column-group`.
    ColumnGroup,
    /// A `table-column`.
    Column,
    /// A row group.
    RowGroup,
    /// A `table-row`.
    Row,
    /// A `table-cell`. Highest priority.
    Cell,
}

/// One border, as §17.6.2.1 compares them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edge {
    /// `border-*-style`.
    pub style: BorderStyle,
    /// `border-*-width`, **as specified**, not as
    /// [`crate::style::consume`] used it.
    ///
    /// §8.5.3 makes a `none` or `hidden` border's used width zero, and the
    /// specified one is what belongs here because [`resolve`] applies
    /// §17.6.2.1's rules **in order**: rules 1 and 2 dispose of `hidden` and
    /// `none` before rule 3 ever compares a width, so a border whose used width
    /// is zero for §8.5.3's reason never reaches the comparison. Carrying the
    /// specified value keeps the two facts separate — what the author wrote, and
    /// what is drawn once the conflict is over, which is [`Edge::used_width`].
    ///
    /// **An earlier draft of this comment claimed a build comparing used widths
    /// would lose rule 3 and draw the border the author hid.** It would not, for
    /// the reason above, and the injection matrix is what said so: the defect
    /// was written, injected, and survived every test in the suite because
    /// nothing can reach it. The claim is corrected here rather than deleted,
    /// because a comment that says which rule protects which is the thing a
    /// reader is entitled to check.
    pub width: f64,
    /// `border-*-color`.
    pub color: Color,
    /// The box it was declared on.
    pub origin: Origin,
}

impl Edge {
    /// No border at all, which is what a box that declared nothing brings to
    /// the conflict.
    #[must_use]
    pub fn none(origin: Origin) -> Self {
        Self {
            style: BorderStyle::None,
            width: 0.0,
            color: Color::BLACK,
            origin,
        }
    }

    /// The used width once the conflict is over: zero for `none` and for
    /// `hidden`, §8.5.3.
    #[must_use]
    pub fn used_width(&self) -> f64 {
        match self.style {
            BorderStyle::None | BorderStyle::Hidden => 0.0,
            _ => self.width.max(0.0),
        }
    }
}

/// §17.6.2.1's style precedence, as a rank: `double` is highest.
///
/// The specification's order is *"double, solid, dashed, dotted, ridge, outset,
/// groove, inset, none"*. The four this build's [`BorderStyle`] does not have —
/// `ridge`, `outset`, `groove` and `inset` — are `Unsupported` at the parser by
/// name, so they cannot arrive here; the order of the four that can is the
/// specification's own, unchanged.
fn rank(style: BorderStyle) -> u8 {
    match style {
        BorderStyle::Double => 5,
        BorderStyle::Solid => 4,
        BorderStyle::Dashed => 3,
        BorderStyle::Dotted => 2,
        BorderStyle::Hidden => 1,
        BorderStyle::None => 0,
    }
}

/// CSS 2.2 §17.6.2.1, the five rules in order.
///
/// `a` is the border of the box **earlier** in document order — the cell above
/// or to the left — which is the tie-break the specification's last sentence
/// gives: *"if border styles differ only in colour, a style set on a cell wins
/// over one on a row … if they are on the same element, the one further to the
/// left (or top) wins"*.
#[must_use]
pub fn resolve(a: Edge, b: Edge) -> Edge {
    // Rule 1. `hidden` beats everything, whatever its width and whatever it is
    // beside. It is first because it is the only rule that can make a border
    // *disappear*, and any later rule reached first would have drawn one.
    if a.style == BorderStyle::Hidden || b.style == BorderStyle::Hidden {
        // The winner is returned **as it was declared**, width and all. Zeroing
        // it here as well as in [`Edge::used_width`] was §8.5.3's rule enforced
        // twice, and the injection matrix is what said so: a defect that
        // deleted `used_width`'s `Hidden` arm survived every test in the suite,
        // because nothing could reach it. One place, and it is the one that
        // answers *what is drawn* rather than *what won*.
        return if a.style == BorderStyle::Hidden { a } else { b };
    }
    // Rule 2. `none` has the lowest priority: a conflict with one is not a
    // conflict at all. Written as its own arm rather than left to rule 3's
    // width comparison, because a `border: none` with a stated `border-width`
    // is a real declaration and would win on width.
    match (a.style == BorderStyle::None, b.style == BorderStyle::None) {
        (true, true) => return a,
        (true, false) => return b,
        (false, true) => return a,
        (false, false) => {}
    }
    // Rule 3. The wider wins.
    if a.width > b.width {
        return a;
    }
    if b.width > a.width {
        return b;
    }
    // Rule 4. Equal widths, so the style decides.
    match rank(a.style).cmp(&rank(b.style)) {
        std::cmp::Ordering::Greater => return a,
        std::cmp::Ordering::Less => return b,
        std::cmp::Ordering::Equal => {}
    }
    // Rule 5. Equal in every respect but the box it came from.
    if b.origin > a.origin {
        b
    } else {
        a
    }
}

/// The collapsed border at one grid line, out of every box that touches it.
///
/// `edges` is in no particular order and the result does not depend on one:
/// [`resolve`] is associative over this set because each rule is a total order
/// on a value, and the tie-break falls to the first argument — so the fold
/// starts from [`Edge::none`] at the lowest origin and every real edge beats
/// it.
#[must_use]
pub fn collapse(edges: &[Edge]) -> Edge {
    let mut out = Edge::none(Origin::Table);
    let mut first = true;
    for edge in edges {
        if first {
            out = *edge;
            first = false;
        } else {
            out = resolve(out, *edge);
        }
    }
    out
}
