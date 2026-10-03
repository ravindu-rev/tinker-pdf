//! Tables: the ones a producer **stated** in the structure tree, and — opt-in,
//! labelled as such — the ones this engine **infers** from what a page draws.
//!
//! Design: `docs/design/table-reconstruction.md`. This module holds both
//! halves, and they are different types on purpose: a [`StatedTable`] is a
//! reader of what the file says, and a guessed grid is never one.
//!
//! # Stated tables
//!
//! [`crate::Page::stated_tables`] walks the structure tree's `Table` elements
//! (14.8.4.3.4, after the role map), their `TR` rows — through `THead`,
//! `TBody` and `TFoot`, which group rows and say nothing about the grid — and
//! each row's `TH` and `TD` cells, and gives each cell the characters
//! [`crate::StructureTree::text_for_page`]'s join claims for it over the
//! **same** [`crate::TextPage`] [`crate::Page::text`] returns. Cells are placed on the
//! grid the way a table model places them: each in the first column of its
//! row that no cell above still spans, `/RowSpan` and `/ColSpan` (Table 349)
//! taken as written. A placement the arithmetic does not allow — a cell over
//! a slot another already holds, a row span past the last row — is
//! [`TableWarning::SpanInconsistent`] rather than a repair, and a row whose
//! width is not the table's is [`TableWarning::RaggedRows`]: the reader says
//! what the file said, and that it did not add up.
//!
//! # Rules
//!
//! [`crate::Page::table_rules`] is the evidence an inferred table is built on,
//! read on its own: every stroked segment and thin filled rectangle the page
//! draws that a table could be ruled with, in default user space, cut to the
//! rectangular clip in force when it was drawn — the page read through the
//! same one interpretation the text is (`observe.rs`). A rule under a clip that
//! is not a rectangle is refused and counted
//! ([`TableWarning::ClipNotRectangular`]); a page that draws more than
//! [`MAX_TABLE_RULES`] has none read ([`TableWarning::TooManyRules`]).
//!
//! # Every question is bounded
//!
//! The spans are the file's, so a cell may claim four billion columns. A
//! table has no more columns than it has cells — a column no cell starts in
//! is a span reaching past the table — so the grid is clamped there and the
//! clamp named. Placement asks of a column only where it is next free, which a
//! segment tree over the columns answers in a logarithm, so a table whose
//! every cell spans every row is a sort and not a square.

use tinker_pdf_content::{Quad, TextChar};
use tinker_pdf_cos::{ObjRef, TableScope};

use crate::observe::Observed;
use crate::structure::{self, StructElement, StructKid, StructuredNode};
use crate::Page;

/// How many rules one page may contribute to table reconstruction.
///
/// Finding where rules meet is quadratic in their number — every horizontal
/// rule against every vertical one — and a page of hatching is a denial of
/// service with a table's name. Past this the page's rules are not read at
/// all and [`TableWarning::TooManyRules`] says how many there were; the text
/// is untouched, and so is every other inference.
///
/// | | Rules |
/// | --- | --- |
/// | The most any fixture in this repository spends: the one built to spend it | 16 384 |
/// | Any other fixture here: a ruled grid of a dozen cells drawn as cell borders | 48 |
/// | A 200-page comic archive | 0 |
/// | A 200-page fixed document | 8 000 |
/// | A 300-page reflowable book | 240 |
/// | **This cap** | **16 384** |
///
/// The comic is one image a page. The fixed document is gap 30's yardstick,
/// 2 000 drawable elements a page, taken at its worst for this count: every
/// element a stroked rectangle, four rules each. The book is arithmetic about
/// what the EPUB path draws: a forty-cell table with a border on every cell,
/// a border being four filled rectangles, plus its frame and a rule under its
/// header — about 170 — rounded up. At the cap the junction search is 2^26
/// comparisons, a fraction of a second. **Not sized from the corpus**, which
/// the design asked for: the fetched corpora were not reachable where this
/// landed, so `table_census.rs` prints the most rules any corpus page draws
/// and this number is owed a look against it.
pub const MAX_TABLE_RULES: usize = 1 << 14;

/// A straight, axis-aligned stretch of ink a table could be ruled with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TableRule {
    /// Whether it runs along `x`.
    pub horizontal: bool,
    /// Where it stands across its length: `y` for a horizontal rule, `x` for
    /// a vertical one, in default user space.
    pub at: f64,
    /// Where it starts along its length; never more than `to`.
    pub from: f64,
    /// Where it ends along its length.
    pub to: f64,
    /// Its weight, in points: a stroke's width, or a filled rectangle's
    /// thinner dimension.
    pub width: f64,
}

/// A page's rules and what reading them had to tolerate.
#[derive(Clone, Debug, Default)]
pub struct TableRules {
    /// The rules, in the order the page drew them.
    pub rules: Vec<TableRule>,
    /// [`TableWarning::TooManyRules`] and
    /// [`TableWarning::ClipNotRectangular`].
    pub warnings: Vec<TableWarning>,
}

/// One table the structure tree states, on one page.
#[derive(Clone, Debug)]
pub struct StatedTable {
    /// The `Table` element's own reference, when it had one.
    pub element: Option<ObjRef>,
    /// How many rows: the `TR` elements, in the tree's order.
    pub rows: usize,
    /// How many columns the placed cells reach.
    pub columns: usize,
    /// The cells, row by row in the tree's order.
    pub cells: Vec<StatedCell>,
    /// The quad enclosing every character the table's cells claim on this
    /// page; `None` when they claim none here.
    pub quad: Option<Quad>,
    /// `/Summary` (Table 349), what the table is for.
    pub summary: Option<String>,
    /// What the table's arithmetic did not allow.
    pub warnings: Vec<TableWarning>,
}

/// One cell of a [`StatedTable`].
#[derive(Clone, Debug)]
pub struct StatedCell {
    /// The row it starts in, from 0.
    pub row: usize,
    /// The column it starts in, from 0.
    pub column: usize,
    /// How many rows it spans: `/RowSpan`, 1 when the file states none.
    pub row_span: usize,
    /// How many columns: `/ColSpan`, 1 when the file states none.
    pub col_span: usize,
    /// Whether the element is a `TH` rather than a `TD`.
    pub header: bool,
    /// `/Scope` (Table 349), for a header cell that states one.
    pub scope: Option<TableScope>,
    /// `/Headers`: the identifiers of the cells that head this one.
    pub headers: Vec<Vec<u8>>,
    /// `/ID`, the cell's own identifier.
    pub id: Option<Vec<u8>>,
    /// The cell's text on this page, its runs in structure order.
    pub text: String,
    /// The characters the cell claims on this page; empty where its text is
    /// an `/ActualText` (14.9.4) or it draws nothing here.
    pub chars: Vec<TextChar>,
    /// The quad enclosing `chars`; `None` when there are none.
    pub quad: Option<Quad>,
}

/// What a table reader had to tolerate, or would not do (ruling 10).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TableWarning {
    /// A cell's `/RowSpan` or `/ColSpan` puts it over a slot another cell
    /// already holds, past the last row, or past the most columns a table of
    /// this many cells can have. The cell is reported where its row's free
    /// column put it, at the span it states.
    SpanInconsistent {
        /// The row the cell starts in.
        row: usize,
        /// The column it starts in.
        column: usize,
    },
    /// A row whose cells, with the row spans reaching into it, fill a
    /// different number of columns from the table's.
    RaggedRows {
        /// The first such row.
        row: usize,
        /// How many columns it fills.
        width: usize,
        /// How many the table has.
        columns: usize,
    },
    /// The page draws more than [`MAX_TABLE_RULES`] rules, so none were read.
    TooManyRules {
        /// How many it draws.
        drawn: usize,
    },
    /// Rules drawn under a clip that is not a rectangle, which were not read:
    /// whether any of one shows through is not a question a rectangle answers.
    ClipNotRectangular {
        /// How many.
        rules: usize,
    },
}

impl Page {
    /// The rules this page draws: the evidence an inferred table is built
    /// on, read on its own. See the module documentation.
    #[must_use]
    pub fn table_rules(&self) -> TableRules {
        rules_of(&Observed::read(self, false))
    }

    /// The tables this page's structure tree states, in tree order: every
    /// `Table` element with a cell that claims text on this page, or whose
    /// `/Pg` is this page.
    ///
    /// Empty for an untagged document. A reader of what the producer said;
    /// nothing here is inferred.
    #[must_use]
    pub fn stated_tables(&self) -> Vec<StatedTable> {
        let Some(tree) = structure::bind(&self.doc) else {
            return Vec::new();
        };
        let text = self.text();
        let index = self.index();
        let mut out = Vec::new();
        for table in tree
            .elements()
            .into_iter()
            .filter(|e| e.standard_type == "Table")
        {
            let rows = rows_of(table);
            let cells: Vec<&StructElement> = rows.iter().flatten().copied().collect();
            let runs = tree.element_runs(&cells, index, &text);
            let here =
                table.page == Some(index) || runs.iter().flatten().any(|n| !n.text.is_empty());
            if here {
                out.push(stated(table, &rows, &runs));
            }
        }
        out
    }
}

/// The rules of an observed page, with the warnings reading them earned.
pub(crate) fn rules_of(observed: &Observed) -> TableRules {
    let mut warnings = Vec::new();
    if observed.rules_drawn > MAX_TABLE_RULES {
        warnings.push(TableWarning::TooManyRules {
            drawn: observed.rules_drawn,
        });
    }
    if observed.rules_unclipped > 0 {
        warnings.push(TableWarning::ClipNotRectangular {
            rules: observed.rules_unclipped,
        });
    }
    TableRules {
        rules: observed.rules.clone(),
        warnings,
    }
}

/// A table's rows, each its cells, in the tree's order: `TR` elements found
/// through any grouping element, `TH` and `TD` found through anything but
/// another row. A `Table` inside a cell is a table of its own and is not
/// walked into.
fn rows_of(table: &StructElement) -> Vec<Vec<&StructElement>> {
    fn rows<'a>(kids: &'a [StructKid], out: &mut Vec<Vec<&'a StructElement>>) {
        for kid in kids {
            let StructKid::Element(element) = kid else {
                continue;
            };
            match element.standard_type.as_str() {
                "TR" => {
                    let mut cells = Vec::new();
                    cells_of(&element.kids, &mut cells);
                    out.push(cells);
                }
                "Table" | "TH" | "TD" => {}
                _ => rows(&element.kids, out),
            }
        }
    }
    fn cells_of<'a>(kids: &'a [StructKid], out: &mut Vec<&'a StructElement>) {
        for kid in kids {
            let StructKid::Element(element) = kid else {
                continue;
            };
            match element.standard_type.as_str() {
                "TH" | "TD" => out.push(element),
                "TR" | "Table" => {}
                _ => cells_of(&element.kids, out),
            }
        }
    }
    let mut out = Vec::new();
    rows(&table.kids, &mut out);
    out
}

/// The stated table, its cells placed.
fn stated(
    table: &StructElement,
    rows: &[Vec<&StructElement>],
    runs: &[Vec<StructuredNode>],
) -> StatedTable {
    let cell_count: usize = rows.iter().map(Vec::len).sum();
    // A table of `n` cells has at most `n` columns that any cell starts in.
    let limit = cell_count.max(1);
    let mut grid = Occupancy::new(limit);
    let mut warnings = Vec::new();
    let mut cells = Vec::with_capacity(cell_count);
    let mut widths = vec![0i64; rows.len() + 1];
    let mut runs = runs.iter();
    for (row, elements) in rows.iter().enumerate() {
        let mut cursor = 0usize;
        for element in elements {
            let attributes = element.table.as_ref();
            let span = |value: Option<u32>| value.map_or(1, |v| (v as usize).max(1));
            let mut row_span = span(attributes.and_then(|a| a.row_span));
            let mut col_span = span(attributes.and_then(|a| a.col_span));
            let column = grid.first_free(cursor, row).unwrap_or(limit);
            let mut fits = true;
            if row + row_span > rows.len() {
                row_span = rows.len() - row;
                fits = false;
            }
            if column >= limit || column + col_span > limit {
                col_span = limit.saturating_sub(column).max(1);
                fits = false;
            }
            if column < limit {
                if grid.max(column, column + col_span) > row {
                    fits = false;
                }
                grid.assign(column, column + col_span, row + row_span);
            }
            if !fits {
                warnings.push(TableWarning::SpanInconsistent { row, column });
            }
            // How many columns each row fills, as a difference array over
            // the rows, so a span costs two entries whatever its height.
            let filled = i64::try_from(col_span).unwrap_or(i64::MAX);
            if let Some(start) = widths.get_mut(row) {
                *start = start.saturating_add(filled);
            }
            if let Some(end) = widths.get_mut(row + row_span) {
                *end = end.saturating_sub(filled);
            }
            cursor = column.saturating_add(col_span);

            let nodes = runs.next().map(Vec::as_slice).unwrap_or_default();
            let text: String = nodes.iter().map(|n| n.text.as_str()).collect();
            let chars: Vec<TextChar> = nodes.iter().flat_map(|n| n.chars.iter().cloned()).collect();
            cells.push(StatedCell {
                row,
                column,
                row_span: attributes
                    .and_then(|a| a.row_span)
                    .map_or(1, |v| (v as usize).max(1)),
                col_span: attributes
                    .and_then(|a| a.col_span)
                    .map_or(1, |v| (v as usize).max(1)),
                header: element.standard_type == "TH",
                scope: attributes.and_then(|a| a.scope),
                headers: attributes.map(|a| a.headers.clone()).unwrap_or_default(),
                id: element.id.clone(),
                quad: enclosing(chars.iter()),
                text,
                chars,
            });
        }
    }
    let columns = cells
        .iter()
        .map(|c| (c.column + c.col_span).min(limit))
        .max()
        .unwrap_or(0);
    let mut width = 0i64;
    for (row, delta) in widths.iter().take(rows.len()).enumerate() {
        width = width.saturating_add(*delta);
        let filled = usize::try_from(width).unwrap_or(0);
        if filled != columns {
            warnings.push(TableWarning::RaggedRows {
                row,
                width: filled,
                columns,
            });
            break;
        }
    }
    StatedTable {
        element: table.reference,
        rows: rows.len(),
        columns,
        quad: enclosing(cells.iter().flat_map(|c| c.chars.iter())),
        cells,
        summary: table.table.as_ref().and_then(|a| a.summary.clone()),
        warnings,
    }
}

/// The axis-aligned quad enclosing `chars`, or `None` for none with a finite
/// quad.
pub(crate) fn enclosing<'a>(chars: impl Iterator<Item = &'a TextChar>) -> Option<Quad> {
    let mut out: Option<(f64, f64, f64, f64)> = None;
    for c in chars {
        if !c.quad.is_finite() {
            continue;
        }
        let (x0, y0, x1, y1) = c.quad.bounds();
        out = Some(match out {
            None => (x0, y0, x1, y1),
            Some((a, b, cc, d)) => (a.min(x0), b.min(y0), cc.max(x1), d.max(y1)),
        });
    }
    out.map(|(x0, y0, x1, y1)| Quad {
        ul: (x0, y1),
        ur: (x1, y1),
        ll: (x0, y0),
        lr: (x1, y0),
    })
}

/// For each column of a table, the first row at which it is free again — a
/// segment tree with range assignment, so placing a cell and asking where the
/// next free column is are each a logarithm in the columns.
struct Occupancy {
    n: usize,
    /// Per node: the least and the greatest value under it, and a pending
    /// assignment to push down.
    low: Vec<usize>,
    high: Vec<usize>,
    pending: Vec<Option<usize>>,
}

impl Occupancy {
    fn new(n: usize) -> Occupancy {
        let size = 4 * n.max(1);
        Occupancy {
            n: n.max(1),
            low: vec![0; size],
            high: vec![0; size],
            pending: vec![None; size],
        }
    }

    fn apply(&mut self, node: usize, value: usize) {
        if let (Some(l), Some(h), Some(p)) = (
            self.low.get_mut(node),
            self.high.get_mut(node),
            self.pending.get_mut(node),
        ) {
            *l = value;
            *h = value;
            *p = Some(value);
        }
    }

    fn push(&mut self, node: usize) {
        if let Some(value) = self.pending.get_mut(node).and_then(Option::take) {
            self.apply(2 * node, value);
            self.apply(2 * node + 1, value);
        }
    }

    fn pull(&mut self, node: usize) {
        let at = |values: &[usize], i: usize| values.get(i).copied().unwrap_or(0);
        let low = at(&self.low, 2 * node).min(at(&self.low, 2 * node + 1));
        let high = at(&self.high, 2 * node).max(at(&self.high, 2 * node + 1));
        if let Some(l) = self.low.get_mut(node) {
            *l = low;
        }
        if let Some(h) = self.high.get_mut(node) {
            *h = high;
        }
    }

    /// Sets every column in `lo..hi` to `value`.
    fn assign(&mut self, lo: usize, hi: usize, value: usize) {
        self.assign_in(1, 0, self.n, lo, hi.min(self.n), value);
    }

    fn assign_in(
        &mut self,
        node: usize,
        from: usize,
        to: usize,
        lo: usize,
        hi: usize,
        value: usize,
    ) {
        if hi <= from || to <= lo {
            return;
        }
        if lo <= from && to <= hi {
            self.apply(node, value);
            return;
        }
        self.push(node);
        let mid = from + (to - from) / 2;
        self.assign_in(2 * node, from, mid, lo, hi, value);
        self.assign_in(2 * node + 1, mid, to, lo, hi, value);
        self.pull(node);
    }

    /// The greatest value over `lo..hi`.
    fn max(&mut self, lo: usize, hi: usize) -> usize {
        self.max_in(1, 0, self.n, lo, hi.min(self.n))
    }

    fn max_in(&mut self, node: usize, from: usize, to: usize, lo: usize, hi: usize) -> usize {
        if hi <= from || to <= lo {
            return 0;
        }
        if lo <= from && to <= hi {
            return self.high.get(node).copied().unwrap_or(0);
        }
        self.push(node);
        let mid = from + (to - from) / 2;
        self.max_in(2 * node, from, mid, lo, hi)
            .max(self.max_in(2 * node + 1, mid, to, lo, hi))
    }

    /// The first column at or after `from` that is free at `row`.
    fn first_free(&mut self, from: usize, row: usize) -> Option<usize> {
        self.first_in(1, 0, self.n, from, row)
    }

    fn first_in(
        &mut self,
        node: usize,
        lo: usize,
        hi: usize,
        from: usize,
        row: usize,
    ) -> Option<usize> {
        if hi <= from || self.low.get(node).copied().unwrap_or(0) > row {
            return None;
        }
        if hi - lo == 1 {
            return Some(lo);
        }
        self.push(node);
        let mid = lo + (hi - lo) / 2;
        self.first_in(2 * node, lo, mid, from, row)
            .or_else(|| self.first_in(2 * node + 1, mid, hi, from, row))
    }
}

#[cfg(test)]
mod tests {
    use super::Occupancy;

    /// The segment tree, against the plain array it stands for.
    #[test]
    fn the_occupancy_tree_is_the_array_it_stands_for() {
        let n = 37;
        let mut tree = Occupancy::new(n);
        let mut plain = vec![0usize; n];
        let mut state = 7u64;
        for step in 0..2_000 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let a = (state >> 33) as usize % n;
            let b = a + 1 + (state >> 45) as usize % (n - a);
            let value = (state >> 20) as usize % 9;
            match step % 3 {
                0 => {
                    tree.assign(a, b, value);
                    for v in plain.iter_mut().take(b).skip(a) {
                        *v = value;
                    }
                }
                1 => {
                    let expect = plain[a..b].iter().copied().max().unwrap_or(0);
                    assert_eq!(tree.max(a, b), expect, "max {a}..{b}");
                }
                _ => {
                    let expect = (a..n).find(|c| plain[*c] <= value);
                    assert_eq!(
                        tree.first_free(a, value),
                        expect,
                        "first free from {a} at {value}"
                    );
                }
            }
        }
    }
}
