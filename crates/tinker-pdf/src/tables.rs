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
//! # Inferred tables
//!
//! [`crate::Page::inferred_tables`] — opt-in, never the default, and never a
//! structure element — builds tables from the page's rules. Rules along one
//! axis within [`LATTICE_MERGE_EMS`] of each other are one lattice line (the
//! two halves of a collapsed CSS border, the two borders of cells with spacing
//! between), and collinear pieces whose gaps are no wider are one segment; a
//! segment shorter than [`RULE_MIN_EMS`] is not a rule. A horizontal and a
//! vertical line meet when each reaches the other to within the same
//! distance, and a lattice is a maximal set of lines joined by meetings. Its
//! row lines and column lines bound the cells; every character goes to the
//! cell holding the centre of its box, so a line the text device joined across
//! two cells is split at the rule, and a glyph straddling a rule is counted
//! ([`TableWarning::TextCrossesRule`]). Rows are read top to bottom and cells
//! left to right, and [`InferredTable::permutation`] gives every character's
//! stream position, so the caller can undo the table character for character.
//!
//! **What is not a table.** A lattice of one cell is a box. One with text in
//! fewer than two cells, or in fewer than one cell in [`LATTICE_TEXT_SHARE`],
//! is a grid of empty boxes — a form — or a page of hatching. A lattice inside
//! the frame of another — in one of its cells, or in a cell merged from
//! several — is a nested table, refused by name
//! ([`TableWarning::NestedLattice`]) with the outer one returned; two whose
//! frames overlap with neither inside the other have lines that pass each
//! other without meeting, which no table draws, and neither is read
//! ([`TableWarning::LatticesCross`]). So no character is in two tables. A table whose
//! last rule lies in the page's foot band may continue overleaf
//! ([`TableWarning::MayContinue`]); joining it to the next page is not done
//! here. On a page whose structure tree states a table, the stated one is the
//! answer ([`TableWarning::TreePresent`]): a guess is never preferred to a
//! statement.
//!
//! # Every question is bounded
//!
//! The spans are the file's, so a cell may claim four billion columns. A
//! table has no more columns than it has cells — a column no cell starts in
//! is a span reaching past the table — so the grid is clamped there and the
//! clamp named. Placement asks of a column only where it is next free, which a
//! segment tree over the columns answers in a logarithm, so a table whose
//! every cell spans every row is a sort and not a square.

use std::collections::BTreeMap;

use tinker_pdf_content::{Quad, TextChar, TextPage};
use tinker_pdf_cos::{ObjRef, TableScope};

use crate::observe::{Fill, Observed};
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

/// How close two rules along one axis may be, and how near a rule's end may
/// come to another rule, and still be one lattice line or a meeting, in ems of
/// the page's median text size.
///
/// The two borders of adjacent cells with CSS's default `border-spacing`
/// stand 2.25 pt apart at twelve points, and a separated border stops that
/// short of its neighbour; half an em takes both, and a cell is never
/// narrower than half an em of text and its padding.
pub const LATTICE_MERGE_EMS: f64 = 0.5;

/// The shortest lattice segment, in ems, after collinear pieces are merged.
///
/// **One, not the design's two**: a single-line row with CSS's ordinary
/// padding is about one and three quarter ems tall, so at two a one-row
/// table's verticals — and every row's, before they are merged — were not
/// rules. An underline is long and has no vertical to meet, so it is not a
/// table at any length.
pub const RULE_MIN_EMS: f64 = 1.0;

/// A lattice whose cells hold text in fewer than one in this many is not a
/// table: a form's empty boxes, or a page of hatching. It also bounds what is
/// made: cells are made only for a lattice that passes, so at most this
/// multiple of the page's characters.
pub const LATTICE_TEXT_SHARE: usize = 4;

/// The narrowest gap between two glyphs of a line that cuts it into two
/// fragments for an aligned table, in ems: a word space is a quarter to a
/// third of one, so a gap of a whole em is a gap between cells.
pub const ALIGNED_GAP_EMS: f64 = 1.0;

/// How far apart, in ems, the edges of fragments may be and still start one
/// column of an aligned table (the design's quarter em).
pub const ALIGNED_EDGE_EMS: f64 = 0.25;

/// On how many rows a column's start must recur — and how many consecutive
/// rows of two fragments or more make a run — for an aligned table (the
/// design's three).
pub const ALIGNED_REPEATS: usize = 3;

/// How many lines apart two rows of an aligned table may stand and still be
/// one run.
pub const ALIGNED_ROW_LINES: f64 = 2.0;

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

/// Which tables a caller asks [`crate::Page::tables`] for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TableSource {
    /// The tables the structure tree states: [`crate::Page::stated_tables`].
    #[default]
    Stated,
    /// Tables inferred from what the page draws, labelled as such:
    /// [`crate::Page::inferred_tables`].
    Inferred,
}

/// A page's tables, labelled by where they came from.
#[derive(Clone, Debug)]
pub enum PageTables {
    /// What the structure tree states.
    Stated(Vec<StatedTable>),
    /// What this engine inferred from the page's geometry.
    Inferred(InferredTables),
}

/// How [`crate::Page::inferred_tables`] reads the page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TableOptions {
    /// Infer even on a page whose structure tree states a table.
    ///
    /// For **measuring** the inference against the tables it hid; the answer
    /// carries [`TableWarning::TreePresent`].
    pub hide_structure: bool,
}

/// What the page shows about a table's first row — evidence, never a header.
///
/// A `TH` is a producer's statement; this is what the ink says, and a caller
/// decides what it means.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum HeaderEvidence {
    /// The table has one row, so no row is set apart.
    None,
    /// Every cell of the first row is shaded and no cell of the second is.
    FillBeneath,
    /// The rule under the first row is heavier than the table's other
    /// interior rules, by half again, or doubled where they are single.
    RuleBeneath,
    /// Nothing sets the first row apart: no evidence, and said so.
    FirstRow,
}

/// What a table was inferred from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TableEvidence {
    /// A lattice of drawn rules.
    Ruled,
    /// Text whose fragments start in recurring columns, where no rule was
    /// drawn: weaker evidence, and never averaged with [`Self::Ruled`].
    Aligned,
}

/// One cell of an [`InferredTable`].
#[derive(Clone, Debug)]
pub struct InferredCell {
    /// The row it starts in, from the top, from 0.
    pub row: usize,
    /// The column it starts in, in reading order, from 0.
    pub column: usize,
    /// How many rows it spans.
    pub row_span: usize,
    /// How many columns it spans.
    pub col_span: usize,
    /// Its text, in the order its characters are read.
    pub text: String,
    /// Its characters, taken unchanged from the page the table was inferred
    /// over.
    pub chars: Vec<TextChar>,
    /// The cell's rectangle, between the lattice lines that bound it.
    pub quad: Quad,
}

/// A table this engine **inferred** from what a page draws.
///
/// Not the file's statement of a table — see the module documentation — and
/// carrying its evidence and its undo.
#[derive(Clone, Debug)]
pub struct InferredTable {
    /// How many rows.
    pub rows: usize,
    /// How many columns.
    pub columns: usize,
    /// The cells, row by row in reading order.
    pub cells: Vec<InferredCell>,
    /// The rectangle the lattice's outer lines bound.
    pub bounds: Quad,
    /// What the table was built from.
    pub evidence: TableEvidence,
    /// What the page shows about its first row.
    pub header: HeaderEvidence,
    /// For each character in the table's order, its position in the page's
    /// characters in stream order — blocks, then lines, then characters.
    pub permutation: Vec<usize>,
    /// What building it had to tolerate.
    pub warnings: Vec<TableWarning>,
}

/// A page's inferred tables, the rules they were built from, and what reading
/// the page had to tolerate.
#[derive(Clone, Debug, Default)]
pub struct InferredTables {
    /// The tables, top to bottom.
    pub tables: Vec<InferredTable>,
    /// Every rule the page draws: the evidence, whether or not a table was
    /// made of it.
    pub rules: Vec<TableRule>,
    /// Page-level warnings: the rules' ([`TableWarning::TooManyRules`],
    /// [`TableWarning::ClipNotRectangular`]), [`TableWarning::LatticesCross`]
    /// and [`TableWarning::TreePresent`].
    pub warnings: Vec<TableWarning>,
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
    /// Glyphs whose boxes straddle an interior rule of the table: each was
    /// put in the cell holding its centre.
    TextCrossesRule {
        /// How many.
        chars: usize,
    },
    /// The table's last rule lies in the page's foot band, so it may continue
    /// on the next page, where it is a second table here.
    MayContinue,
    /// A lattice inside this table's frame — a nested table, in one of its
    /// cells or in a cell merged from several — was not read.
    NestedLattice,
    /// The ruling of a lattice merges grid cells into a shape that is not a
    /// rectangle where a rule is missing, so no span can be read from it; the
    /// grid cells are reported as they are. For a stated table: see
    /// [`TableWarning::SpanInconsistent`].
    SpanNotRectangular {
        /// The first grid row of the shape.
        row: usize,
        /// Its first grid column.
        column: usize,
    },
    /// The table was inferred from aligned text, not from rules: nothing the
    /// page drew bounds its cells.
    NoRules,
    /// The page's structure tree states a table, so the stated one is the
    /// answer and nothing was inferred — or, with
    /// [`TableOptions::hide_structure`], the inference is a measurement.
    TreePresent,
    /// Lattices whose frames overlap with neither inside the other: their
    /// lines pass each other without meeting, which no table draws, and which
    /// of them owns the overlap is not a question the lines answer. None of
    /// them was read.
    LatticesCross {
        /// How many.
        lattices: usize,
    },
}

impl Page {
    /// The rules this page draws: the evidence an inferred table is built
    /// on, read on its own. See the module documentation.
    #[must_use]
    pub fn table_rules(&self) -> TableRules {
        rules_of(&Observed::read(self, false))
    }

    /// The page's tables from `source`, labelled by where they came from.
    ///
    /// [`TableSource::Inferred`] on a page whose structure tree states a table
    /// answers with the stated ones: a guess is never preferred to a
    /// statement.
    #[must_use]
    pub fn tables(&self, source: TableSource) -> PageTables {
        let stated = self.stated_tables();
        match source {
            TableSource::Stated => PageTables::Stated(stated),
            TableSource::Inferred if !stated.is_empty() => PageTables::Stated(stated),
            // Read once: the page states none, which is what the inference
            // would ask the tree again to learn.
            TableSource::Inferred => {
                PageTables::Inferred(self.inferred_tables_beside(&TableOptions::default(), false))
            }
        }
    }

    /// Tables inferred from what this page draws, labelled as such.
    ///
    /// Opt-in and never the default; see the module documentation. On a page
    /// whose structure tree states a table it infers nothing and says so
    /// ([`TableWarning::TreePresent`]) unless
    /// [`TableOptions::hide_structure`] is set.
    #[must_use]
    pub fn inferred_tables(&self, options: &TableOptions) -> InferredTables {
        self.inferred_tables_beside(options, !self.stated_tables().is_empty())
    }

    /// [`Page::inferred_tables`], told whether the tree states a table here.
    fn inferred_tables_beside(&self, options: &TableOptions, stated: bool) -> InferredTables {
        if stated && !options.hide_structure {
            return InferredTables {
                warnings: vec![TableWarning::TreePresent],
                ..InferredTables::default()
            };
        }
        let observed = Observed::read(self, options.hide_structure);
        let mut tables = infer_tables(&observed, self.crop_box());
        if stated {
            tables.warnings.insert(0, TableWarning::TreePresent);
        }
        tables
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
        let tables: Vec<(&StructElement, Vec<Vec<&StructElement>>)> = tree
            .elements()
            .into_iter()
            .filter(|e| e.standard_type == "Table")
            .map(|table| (table, rows_of(table)))
            .collect();
        // Every table's cells joined in one call, so the page's characters
        // are grouped by sequence once for the page and not once a table:
        // a call a table was the page's characters times its tables.
        let cells: Vec<&StructElement> = tables
            .iter()
            .flat_map(|(_, rows)| rows.iter().flatten().copied())
            .collect();
        let mut runs = tree.element_runs(&cells, index, &text).into_iter();
        let mut out = Vec::new();
        for (table, rows) in &tables {
            let count = rows.iter().map(Vec::len).sum();
            let runs: Vec<Vec<StructuredNode>> = runs.by_ref().take(count).collect();
            let here =
                table.page == Some(index) || runs.iter().flatten().any(|n| !n.text.is_empty());
            if here {
                out.push(stated(table, rows, &runs));
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
            // Each span against what is left of the table, never added to
            // where it starts first: a `/RowSpan` of four billion in the
            // second row overflows a 32-bit `usize`.
            let rows_left = rows.len().saturating_sub(row);
            if row_span > rows_left {
                row_span = rows_left;
                fits = false;
            }
            if column >= limit || col_span > limit - column {
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
        .map(|c| c.column.saturating_add(c.col_span).min(limit))
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

// ---------------------------------------------------------------------------
// Inferred tables: the lattice
// ---------------------------------------------------------------------------

/// Infers the tables of an observed page, whose crop box is `frame`: ruled,
/// then aligned where no ruled table stands.
pub(crate) fn infer_tables(observed: &Observed, frame: (f64, f64, f64, f64)) -> InferredTables {
    tables_of(observed, frame, true)
}

/// The ruled tables of an observed page alone — what the reading-order
/// inference hands off (`crate::reading_order`).
pub(crate) fn ruled_tables(observed: &Observed, frame: (f64, f64, f64, f64)) -> Vec<InferredTable> {
    tables_of(observed, frame, false).tables
}

fn tables_of(observed: &Observed, frame: (f64, f64, f64, f64), aligned: bool) -> InferredTables {
    let read = rules_of(observed);
    let mut warnings = read.warnings.clone();
    let page = &observed.text;
    let flat: Vec<&TextChar> = page
        .blocks
        .iter()
        .flat_map(|b| b.lines.iter())
        .flat_map(|l| l.chars.iter())
        .collect();
    let em = median_size(&flat).unwrap_or(10.0);
    // Which of the text device's lines each character is on, by its place
    // among the page's lines, with that line's top and whether it reads
    // right to left.
    let line_of: Vec<LineOf> = page
        .blocks
        .iter()
        .flat_map(|b| b.lines.iter())
        .enumerate()
        .flat_map(|(at, line)| {
            let top = line.quad.bounds().3;
            let rtl = line.rtl;
            line.chars.iter().map(move |_| LineOf { at, top, rtl })
        })
        .collect();
    let lines = lattice_lines(&read.rules, em);
    let mut lattices = components(&lines, em);
    // Two lattices are two components: no line of one meets a line of the
    // other. So where their frames overlap, one lies in a cell of the other —
    // a table in a cell, merged or not, which the first delivery does not
    // read: the outer table is returned and says so — or their lines pass
    // each other without meeting, which is hatching and no table. What is
    // left has frames that do not overlap, so no character is in two tables.
    // Every pair is one comparison of two rectangles: at the rule cap, a few
    // million.
    let frames: Vec<(f64, f64, f64, f64)> = lattices.iter().map(Lattice::bounds).collect();
    let mut nested = vec![false; lattices.len()];
    let mut crossed = vec![false; lattices.len()];
    let mut refused_inside: Vec<usize> = vec![0; lattices.len()];
    for (a, fa) in frames.iter().enumerate() {
        for (b, fb) in frames.iter().enumerate().skip(a + 1) {
            let (inner, outer) = if !overlaps(*fa, *fb) {
                continue;
            } else if contains(*fa, *fb) {
                (b, a)
            } else if contains(*fb, *fa) {
                (a, b)
            } else {
                for at in [a, b] {
                    if let Some(flag) = crossed.get_mut(at) {
                        *flag = true;
                    }
                }
                continue;
            };
            if let Some(flag) = nested.get_mut(inner) {
                *flag = true;
            }
            if let Some(count) = refused_inside.get_mut(outer) {
                *count += 1;
            }
        }
    }
    let crossing = crossed.iter().filter(|c| **c).count();
    if crossing > 0 {
        warnings.push(TableWarning::LatticesCross { lattices: crossing });
    }
    let centres = Centres::of(&flat);
    let mut tables = Vec::new();
    let bands = foot_band(frame);
    for (at, lattice) in lattices.drain(..).enumerate() {
        if nested.get(at).copied().unwrap_or(false) || crossed.get(at).copied().unwrap_or(false) {
            continue;
        }
        let within = centres.within(lattice.bounds());
        let Some(mut table) = lattice.table(&within, &flat, &line_of, &observed.fills) else {
            continue;
        };
        if refused_inside.get(at).copied().unwrap_or(0) > 0 {
            table.warnings.push(TableWarning::NestedLattice);
        }
        if lattice.ys.last().is_some_and(|bottom| *bottom <= bands) {
            table.warnings.push(TableWarning::MayContinue);
        }
        tables.push(table);
    }
    // Aligned text where no ruled table stands.
    if aligned {
        let ruled: Vec<(f64, f64, f64, f64)> = tables.iter().map(|t| t.bounds.bounds()).collect();
        tables.extend(infer_aligned(
            fragments(page, &flat, em, &ruled),
            &flat,
            &line_of,
            em,
        ));
    }
    // Top to bottom, then left to right, as a page is read.
    tables.sort_by(|a, b| {
        let (ab, bb) = (a.bounds.bounds(), b.bounds.bounds());
        bb.3.total_cmp(&ab.3).then(ab.0.total_cmp(&bb.0))
    });
    InferredTables {
        tables,
        rules: read.rules,
        warnings,
    }
}

/// Whether two `(x0, y0, x1, y1)` rectangles share any interior.
fn overlaps(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> bool {
    a.0 < b.2 && b.0 < a.2 && a.1 < b.3 && b.1 < a.3
}

/// Whether rectangle `outer` holds rectangle `inner`.
fn contains(outer: (f64, f64, f64, f64), inner: (f64, f64, f64, f64)) -> bool {
    inner.0 >= outer.0 && inner.2 <= outer.2 && inner.1 >= outer.1 && inner.3 <= outer.3
}

/// The page's characters by the centres of their boxes, along each axis, so a
/// lattice reads only the characters within its frame: of the two runs a
/// binary search finds — those within its columns' span, and those within
/// its rows' — the shorter. The frames read do not overlap, so a page of a
/// thousand small tables is not a thousand walks over every character.
struct Centres {
    by_x: Vec<(f64, usize)>,
    by_y: Vec<(f64, usize)>,
}

impl Centres {
    fn of(flat: &[&TextChar]) -> Centres {
        let mut by_x = Vec::with_capacity(flat.len());
        let mut by_y = Vec::with_capacity(flat.len());
        for (at, c) in flat.iter().enumerate() {
            if !c.quad.is_finite() {
                continue;
            }
            let (x0, y0, x1, y1) = c.quad.bounds();
            by_x.push(((x0 + x1) / 2.0, at));
            by_y.push(((y0 + y1) / 2.0, at));
        }
        by_x.sort_by(|a, b| a.0.total_cmp(&b.0));
        by_y.sort_by(|a, b| a.0.total_cmp(&b.0));
        Centres { by_x, by_y }
    }

    /// The characters whose centres lie strictly inside `(x0, y0, x1, y1)`
    /// along one axis — the axis whose run is shorter; [`Lattice::slot`]
    /// decides the other.
    fn within(&self, (x0, y0, x1, y1): (f64, f64, f64, f64)) -> Vec<usize> {
        let run = |axis: &[(f64, usize)], lo: f64, hi: f64| -> std::ops::Range<usize> {
            let from = axis.partition_point(|(c, _)| *c <= lo);
            from..axis.partition_point(|(c, _)| *c < hi).max(from)
        };
        let (across, down) = (run(&self.by_x, x0, x1), run(&self.by_y, y0, y1));
        let (axis, range) = if across.len() <= down.len() {
            (&self.by_x, across)
        } else {
            (&self.by_y, down)
        };
        axis.get(range)
            .unwrap_or_default()
            .iter()
            .map(|(_, at)| *at)
            .collect()
    }
}

/// The `y` at or under which a table's last rule sits in the page's foot
/// band, where a table continued overleaf ends.
fn foot_band(frame: (f64, f64, f64, f64)) -> f64 {
    let (_, y0, _, y1) = frame;
    y0.min(y1) + (y1 - y0).abs() * crate::reading_order::MARGIN_BAND
}

/// The median size of `chars`, or `None` for none.
fn median_size(chars: &[&TextChar]) -> Option<f64> {
    let mut sizes: Vec<f64> = chars
        .iter()
        .map(|c| c.size)
        .filter(|s| s.is_finite() && *s > 0.0)
        .collect();
    sizes.sort_by(f64::total_cmp);
    sizes.get(sizes.len() / 2).copied()
}

/// One line of a lattice: rules along one axis at one position, merged.
#[derive(Clone, Copy, Debug)]
struct Line {
    horizontal: bool,
    at: f64,
    from: f64,
    to: f64,
    /// The heaviest rule merged into it.
    weight: f64,
    /// How many distinct strokes, a point or more apart, were merged into it:
    /// two for a doubled rule.
    strands: usize,
}

/// The page's rules as lattice lines: rules along one axis within
/// [`LATTICE_MERGE_EMS`] of each other are one line — the two halves of a
/// collapsed CSS border, or the two borders of adjacent cells with spacing
/// between them — and collinear pieces whose gaps are no wider are one
/// segment. A segment shorter than [`RULE_MIN_EMS`] is not a rule.
fn lattice_lines(rules: &[TableRule], em: f64) -> Vec<Line> {
    let merge = LATTICE_MERGE_EMS * em;
    let mut out = Vec::new();
    for horizontal in [true, false] {
        let mut axis: Vec<&TableRule> = rules
            .iter()
            .filter(|r| r.horizontal == horizontal)
            .collect();
        axis.sort_by(|a, b| a.at.total_cmp(&b.at));
        let mut start = 0usize;
        while start < axis.len() {
            let Some(first) = axis.get(start).map(|r| r.at) else {
                break;
            };
            let mut end = start + 1;
            while axis.get(end).is_some_and(|r| r.at - first <= merge) {
                end += 1;
            }
            let cluster: Vec<&TableRule> = axis.get(start..end).unwrap_or_default().to_vec();
            start = end;
            // Where the line stands: midway between its outermost strokes.
            let low = cluster.iter().map(|r| r.at).fold(f64::INFINITY, f64::min);
            let high = cluster
                .iter()
                .map(|r| r.at)
                .fold(f64::NEG_INFINITY, f64::max);
            let at = (low + high) / 2.0;
            // Distinct strokes: positions a point or more apart.
            let mut strands = 0usize;
            let mut last = f64::NEG_INFINITY;
            for r in &cluster {
                if r.at - last >= 1.0 {
                    strands += 1;
                    last = r.at;
                }
            }
            let mut pieces: Vec<(f64, f64, f64)> =
                cluster.iter().map(|r| (r.from, r.to, r.width)).collect();
            pieces.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut current: Option<(f64, f64, f64)> = None;
            let flush = |piece: (f64, f64, f64), out: &mut Vec<Line>| {
                if piece.1 - piece.0 >= RULE_MIN_EMS * em {
                    out.push(Line {
                        horizontal,
                        at,
                        from: piece.0,
                        to: piece.1,
                        weight: piece.2,
                        strands,
                    });
                }
            };
            for (from, to, width) in pieces {
                current = match current {
                    Some((a, b, w)) if from <= b + merge => Some((a, b.max(to), w.max(width))),
                    Some(done) => {
                        flush(done, &mut out);
                        Some((from, to, width))
                    }
                    None => Some((from, to, width)),
                };
            }
            if let Some(done) = current {
                flush(done, &mut out);
            }
        }
    }
    out
}

/// A connected set of lines, as a lattice: its row lines from the top and
/// its column lines from the left, and the segments themselves.
#[derive(Clone, Debug)]
struct Lattice {
    /// `y` of every horizontal line, descending.
    ys: Vec<f64>,
    /// `x` of every vertical line, ascending.
    xs: Vec<f64>,
    /// The segments, horizontal and vertical, each sorted by where it
    /// stands, then where it starts.
    horizontals: Vec<Line>,
    verticals: Vec<Line>,
    /// How near a segment's end may come to a boundary and still rule it.
    reach: f64,
}

/// The lines that meet, grouped: a horizontal and a vertical line meet when
/// each reaches the other to within [`LATTICE_MERGE_EMS`], and a lattice is a
/// maximal set of lines joined by meetings. A lattice of fewer than two cells
/// is a box, not a table.
fn components(lines: &[Line], em: f64) -> Vec<Lattice> {
    let reach = LATTICE_MERGE_EMS * em;
    let mut parent: Vec<usize> = (0..lines.len()).collect();
    fn root(parent: &mut [usize], mut at: usize) -> usize {
        while let Some(&up) = parent.get(at) {
            if up == at {
                break;
            }
            // Halve the path as it is walked, so every later find is short.
            let grand = parent.get(up).copied().unwrap_or(up);
            if let Some(slot) = parent.get_mut(at) {
                *slot = grand;
            }
            at = grand;
        }
        at
    }
    // Verticals by `x`, so each horizontal asks only those within its reach.
    let mut verticals: Vec<usize> = (0..lines.len())
        .filter(|i| lines.get(*i).is_some_and(|l| !l.horizontal))
        .collect();
    verticals.sort_by(|a, b| {
        let at = |i: &usize| lines.get(*i).map_or(0.0, |l| l.at);
        at(a).total_cmp(&at(b))
    });
    let xs: Vec<f64> = verticals
        .iter()
        .map(|i| lines.get(*i).map_or(0.0, |l| l.at))
        .collect();
    for (h, line) in lines.iter().enumerate().filter(|(_, l)| l.horizontal) {
        let from = xs.partition_point(|x| *x < line.from - reach);
        let to = xs.partition_point(|x| *x <= line.to + reach);
        for v in verticals.get(from..to).unwrap_or_default() {
            let Some(vertical) = lines.get(*v) else {
                continue;
            };
            if line.at >= vertical.from - reach && line.at <= vertical.to + reach {
                let (a, b) = (root(&mut parent, h), root(&mut parent, *v));
                if a != b {
                    if let Some(slot) = parent.get_mut(a) {
                        *slot = b;
                    }
                }
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<Line>> = BTreeMap::new();
    for (at, line) in lines.iter().enumerate() {
        let r = root(&mut parent, at);
        groups.entry(r).or_default().push(*line);
    }
    let mut out = Vec::new();
    for (_, group) in groups {
        let mut ys: Vec<f64> = group
            .iter()
            .filter(|l| l.horizontal)
            .map(|l| l.at)
            .collect();
        let mut xs: Vec<f64> = group
            .iter()
            .filter(|l| !l.horizontal)
            .map(|l| l.at)
            .collect();
        ys.sort_by(|a, b| b.total_cmp(a));
        ys.dedup();
        xs.sort_by(f64::total_cmp);
        xs.dedup();
        if ys.len() < 2 || xs.len() < 2 || (ys.len() - 1) * (xs.len() - 1) < 2 {
            continue;
        }
        let by_place = |a: &Line, b: &Line| a.at.total_cmp(&b.at).then(a.from.total_cmp(&b.from));
        let mut horizontals: Vec<Line> = group.iter().filter(|l| l.horizontal).copied().collect();
        let mut verticals: Vec<Line> = group.iter().filter(|l| !l.horizontal).copied().collect();
        horizontals.sort_by(by_place);
        verticals.sort_by(by_place);
        out.push(Lattice {
            ys,
            xs,
            horizontals,
            verticals,
            reach,
        });
    }
    out
}

impl Lattice {
    /// `(x0, y0, x1, y1)`.
    fn bounds(&self) -> (f64, f64, f64, f64) {
        (
            self.xs.first().copied().unwrap_or(0.0),
            self.ys.last().copied().unwrap_or(0.0),
            self.xs.last().copied().unwrap_or(0.0),
            self.ys.first().copied().unwrap_or(0.0),
        )
    }

    /// The row and column whose grid cell holds `(x, y)`, or `None` outside.
    fn slot(&self, x: f64, y: f64) -> Option<(usize, usize)> {
        let (x0, y0, x1, y1) = self.bounds();
        if !(x > x0 && x < x1 && y > y0 && y < y1) {
            return None;
        }
        let column = self.xs.partition_point(|at| *at <= x).checked_sub(1)?;
        let row = self.ys.partition_point(|at| *at >= y).checked_sub(1)?;
        Some((row, column))
    }

    /// Whether a segment of `lines` stands at `at` and covers at least half
    /// of `from..to`, reaching to within the lattice's reach.
    fn ruled(&self, lines: &[Line], at: f64, from: f64, to: f64) -> bool {
        let first = lines.partition_point(|l| l.at < at);
        let need = (to - from) / 2.0;
        lines
            .get(first..)
            .unwrap_or_default()
            .iter()
            .take_while(|l| l.at == at)
            .any(|l| (l.to + self.reach).min(to) - (l.from - self.reach).max(from) >= need)
    }

    /// The heaviest segment and the most strands at `at` among `lines`.
    fn weight_at(lines: &[Line], at: f64) -> (f64, usize) {
        let first = lines.partition_point(|l| l.at < at);
        lines
            .get(first..)
            .unwrap_or_default()
            .iter()
            .take_while(|l| l.at == at)
            .fold((0.0f64, 0usize), |(w, s), l| {
                (w.max(l.weight), s.max(l.strands))
            })
    }

    /// The table this lattice rules, its text assigned and ordered; `None`
    /// when fewer than two of its grid cells hold any text, or fewer than one
    /// in [`LATTICE_TEXT_SHARE`] — a grid of empty boxes, a form or a page of
    /// hatching, and not a table. Checked before any cell is made, so the
    /// cells made are at most that multiple of the page's characters.
    ///
    /// A grid cell whose boundary with its neighbour is not ruled is one cell
    /// with it: the merged shape is a span when it is a rectangle, and named
    /// ([`TableWarning::SpanNotRectangular`]) and left as its grid cells when
    /// it is not. Columns are counted, and cells read, right to left when most
    /// of the table's characters are on right-to-left lines.
    fn table(
        &self,
        within: &[usize],
        flat: &[&TextChar],
        line_of: &[LineOf],
        fills: &[Fill],
    ) -> Option<InferredTable> {
        let rows = self.ys.len().checked_sub(1)?;
        let columns = self.xs.len().checked_sub(1)?;
        let mut held: BTreeMap<(usize, usize), Vec<usize>> = BTreeMap::new();
        let mut crossing = 0usize;
        let mut rtl = 0usize;
        let interior = self.xs.get(1..columns).unwrap_or_default();
        for &at in within {
            let Some(c) = flat.get(at) else { continue };
            if !c.quad.is_finite() {
                continue;
            }
            let (x0, y0, x1, y1) = c.quad.bounds();
            let Some(slot) = self.slot((x0 + x1) / 2.0, (y0 + y1) / 2.0) else {
                continue;
            };
            // A glyph runs across a rule when an interior column line passes
            // through the middle half of its box; a glyph merely touching a
            // rule at its edge does not. A binary search over the lines.
            let quarter = (x1 - x0) / 4.0;
            let next = interior.partition_point(|x| *x <= x0 + quarter);
            if interior.get(next).is_some_and(|x| *x < x1 - quarter) {
                crossing += 1;
            }
            if line_of.get(at).is_some_and(|l| l.rtl) {
                rtl += 1;
            }
            held.entry(slot).or_default().push(at);
        }
        let slots = rows.checked_mul(columns)?;
        if held.len() < 2 || held.len().saturating_mul(LATTICE_TEXT_SHARE) < slots {
            return None;
        }
        let total: usize = held.values().map(Vec::len).sum();
        let right_to_left = rtl * 2 > total;
        let mut warnings = Vec::new();
        if crossing > 0 {
            warnings.push(TableWarning::TextCrossesRule { chars: crossing });
        }

        // Grid cells joined across every boundary no rule draws.
        let mut parent: Vec<usize> = (0..slots).collect();
        fn root(parent: &mut [usize], mut at: usize) -> usize {
            while let Some(&up) = parent.get(at) {
                if up == at {
                    break;
                }
                let grand = parent.get(up).copied().unwrap_or(up);
                if let Some(slot) = parent.get_mut(at) {
                    *slot = grand;
                }
                at = grand;
            }
            at
        }
        let join = |parent: &mut Vec<usize>, a: usize, b: usize| {
            let (ra, rb) = (root(parent, a), root(parent, b));
            if ra != rb {
                // The smaller index stays the root, so a shape's root is its
                // first grid cell in reading order of the grid.
                let (keep, child) = (ra.min(rb), ra.max(rb));
                if let Some(slot) = parent.get_mut(child) {
                    *slot = keep;
                }
            }
        };
        let at = |v: &[f64], i: usize| v.get(i).copied().unwrap_or(0.0);
        for row in 0..rows {
            for column in 0..columns {
                let here = row * columns + column;
                let (top, bottom) = (at(&self.ys, row), at(&self.ys, row + 1));
                let (left, right) = (at(&self.xs, column), at(&self.xs, column + 1));
                if column + 1 < columns && !self.ruled(&self.verticals, right, bottom, top) {
                    join(&mut parent, here, here + 1);
                }
                if row + 1 < rows && !self.ruled(&self.horizontals, bottom, left, right) {
                    join(&mut parent, here, here + columns);
                }
            }
        }
        // Each shape: its grid cells, which make one cell when they make a
        // rectangle and stay grid cells when they do not.
        let mut shapes: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for slot in 0..slots {
            let r = root(&mut parent, slot);
            shapes.entry(r).or_default().push(slot);
        }
        // (row, column, row span, column span) of every cell.
        let mut placed: Vec<(usize, usize, usize, usize)> = Vec::new();
        for members in shapes.into_values() {
            let rows_of = members.iter().map(|s| s / columns);
            let columns_of = members.iter().map(|s| s % columns);
            let (r0, r1) = (
                rows_of.clone().min().unwrap_or(0),
                rows_of.max().unwrap_or(0),
            );
            let (c0, c1) = (
                columns_of.clone().min().unwrap_or(0),
                columns_of.max().unwrap_or(0),
            );
            let (height, width) = (r1 - r0 + 1, c1 - c0 + 1);
            if height * width == members.len() {
                placed.push((r0, c0, height, width));
            } else {
                warnings.push(TableWarning::SpanNotRectangular {
                    row: r0,
                    column: c0,
                });
                placed.extend(members.iter().map(|s| (s / columns, s % columns, 1, 1)));
            }
        }
        placed.sort_unstable();
        // Read in the table's direction.
        let mirrored = |column: usize, span: usize| columns - column - span;
        if right_to_left {
            for cell in &mut placed {
                cell.1 = mirrored(cell.1, cell.3);
            }
            placed.sort_unstable();
        }

        let mut permutation = Vec::new();
        let mut out = Vec::with_capacity(placed.len());
        for (row, column, row_span, col_span) in placed {
            // The grid columns the cell covers, back in left-to-right terms.
            let first = if right_to_left {
                mirrored(column, col_span)
            } else {
                column
            };
            let mut chars: Vec<usize> = Vec::new();
            for r in row..row + row_span {
                for c in first..first + col_span {
                    chars.extend(held.remove(&(r, c)).unwrap_or_default());
                }
            }
            order_cell(&mut chars, line_of);
            permutation.extend(chars.iter().copied());
            let text_chars: Vec<TextChar> = chars
                .iter()
                .filter_map(|i| flat.get(*i).map(|c| (*c).clone()))
                .collect();
            let (x0, x1) = (at(&self.xs, first), at(&self.xs, first + col_span));
            let (y1, y0) = (at(&self.ys, row), at(&self.ys, row + row_span));
            out.push(InferredCell {
                row,
                column,
                row_span,
                col_span,
                text: text_chars.iter().map(|c| c.text.as_str()).collect(),
                chars: text_chars,
                quad: rect(x0, y0, x1, y1),
            });
        }
        let header = self.header(&out, rows, fills);
        let (x0, y0, x1, y1) = self.bounds();
        Some(InferredTable {
            rows,
            columns,
            cells: out,
            bounds: rect(x0, y0, x1, y1),
            evidence: TableEvidence::Ruled,
            header,
            permutation,
            warnings,
        })
    }

    /// What the ink says about the first row: shading under every first-row
    /// cell and none under the second's, or a heavier or doubled rule under
    /// it than the table's other interior rules.
    fn header(&self, cells: &[InferredCell], rows: usize, fills: &[Fill]) -> HeaderEvidence {
        if rows < 2 {
            return HeaderEvidence::None;
        }
        let shaded = |cell: &InferredCell| {
            let (x0, y0, x1, y1) = cell.quad.bounds();
            let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
            fills.iter().any(|f| {
                let (a, b, c, d) = f.rect;
                f.inked && a <= cx && cx <= c && b <= cy && cy <= d
            })
        };
        let first: Vec<&InferredCell> = cells.iter().filter(|c| c.row == 0).collect();
        let second: Vec<&InferredCell> = cells.iter().filter(|c| c.row == 1).collect();
        if !first.is_empty() && first.iter().all(|c| shaded(c)) && !second.iter().any(|c| shaded(c))
        {
            return HeaderEvidence::FillBeneath;
        }
        let beneath = Lattice::weight_at(&self.horizontals, self.ys.get(1).copied().unwrap_or(0.0));
        let others: Vec<(f64, usize)> = self
            .ys
            .iter()
            .skip(2)
            .take(rows.saturating_sub(2))
            .map(|y| Lattice::weight_at(&self.horizontals, *y))
            .collect();
        if !others.is_empty() {
            let heaviest = others.iter().map(|o| o.0).fold(0.0f64, f64::max);
            let most = others.iter().map(|o| o.1).max().unwrap_or(0);
            if beneath.0 >= heaviest * 1.5 || beneath.1 > most {
                return HeaderEvidence::RuleBeneath;
            }
        }
        HeaderEvidence::FirstRow
    }
}

/// A cell's characters in the order they are read: the text device's lines
/// that reach into the cell, top first, each line's characters in the page's
/// own order, which is logical (ruling 14). A stable sort, so stream order
/// decides between two lines level with each other.
fn order_cell(chars: &mut [usize], line_of: &[LineOf]) {
    let key = |at: &usize| {
        line_of
            .get(*at)
            .map_or((usize::MAX, 0.0), |l| (l.at, l.top))
    };
    chars.sort_by(|a, b| {
        let ((la, ta), (lb, tb)) = (key(a), key(b));
        tb.total_cmp(&ta).then(la.cmp(&lb)).then(a.cmp(b))
    });
}

/// The text device's line a character is on.
#[derive(Clone, Copy, Debug)]
struct LineOf {
    /// Its place among the page's lines.
    at: usize,
    /// Its top.
    top: f64,
    /// Whether it holds a right-to-left character (ruling 14).
    rtl: bool,
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Quad {
    Quad {
        ul: (x0, y1),
        ur: (x1, y1),
        ll: (x0, y0),
        lr: (x1, y0),
    }
}

// ---------------------------------------------------------------------------
// Inferred tables: aligned text, where no rule was drawn
// ---------------------------------------------------------------------------

/// One stretch of a text line between gaps of [`ALIGNED_GAP_EMS`] or more.
#[derive(Clone, Debug)]
struct Fragment {
    /// The page's characters it holds, by their stream position.
    chars: Vec<usize>,
    x0: f64,
    x1: f64,
    y0: f64,
    y1: f64,
    baseline: f64,
    rtl: bool,
}

/// The fragments of the page's lines that no ruled table holds, each line cut
/// at every gap of [`ALIGNED_GAP_EMS`] or more between neighbouring glyphs.
fn fragments(
    page: &TextPage,
    flat: &[&TextChar],
    em: f64,
    ruled: &[(f64, f64, f64, f64)],
) -> Vec<Fragment> {
    let gap = ALIGNED_GAP_EMS * em;
    let inside = |x: f64, y: f64| {
        ruled
            .iter()
            .any(|(a, b, c, d)| x >= *a && x <= *c && y >= *b && y <= *d)
    };
    let mut out = Vec::new();
    let mut next = 0usize;
    for line in page.blocks.iter().flat_map(|b| b.lines.iter()) {
        let first = next;
        next += line.chars.len();
        if line.wmode != tinker_pdf_content::WritingMode::Horizontal {
            continue;
        }
        // The line's glyphs left to right, so a gap is between neighbours on
        // the page whatever order the line is read in.
        let mut placed: Vec<(usize, (f64, f64, f64, f64))> = (first..next)
            .filter_map(|at| {
                let c = flat.get(at)?;
                c.quad.is_finite().then(|| (at, c.quad.bounds()))
            })
            .filter(|(_, (x0, y0, x1, y1))| !inside((x0 + x1) / 2.0, (y0 + y1) / 2.0))
            .collect();
        placed.sort_by(|a, b| a.1 .0.total_cmp(&b.1 .0));
        let mut current: Option<Fragment> = None;
        for (at, (x0, y0, x1, y1)) in placed {
            // A space is part of the gap it stands in, not ink that closes
            // it: a producer that aligns columns with spaces in one string
            // draws them as glyphs. It stays with the fragment before it, so
            // no character leaves the table, and moves no edge.
            if flat.get(at).is_some_and(|c| c.text.trim().is_empty()) {
                if let Some(f) = current.as_mut() {
                    f.chars.push(at);
                }
                continue;
            }
            let baseline = flat.get(at).map_or(y0, |c| c.origin.1);
            let starts = current.as_ref().is_none_or(|f| x0 - f.x1 >= gap);
            if starts {
                out.extend(current.take());
                current = Some(Fragment {
                    chars: vec![at],
                    x0,
                    x1,
                    y0,
                    y1,
                    baseline,
                    rtl: line.rtl,
                });
            } else if let Some(f) = current.as_mut() {
                f.chars.push(at);
                f.x1 = f.x1.max(x1);
                f.y0 = f.y0.min(y0);
                f.y1 = f.y1.max(y1);
            }
        }
        out.extend(current);
    }
    out
}

/// Tables of aligned text, labelled [`TableEvidence::Aligned`]: runs of at
/// least three consecutive rows — fragments sharing a baseline to within half
/// an em — each with two fragments or more, whose column starts recur. A
/// column is a left edge (a right edge, on a right-to-left table) that
/// [`ALIGNED_REPEATS`] rows share to within [`ALIGNED_EDGE_EMS`]; a fragment
/// belongs to the last column starting at or before it. Two columns of prose
/// side by side are columns of a page, not of a table: a run in which more
/// than one column is as wide as [`crate::reading_order::COLUMN_MIN_WIDTH_EMS`]
/// is not one.
fn infer_aligned(
    fragments: Vec<Fragment>,
    flat: &[&TextChar],
    line_of: &[LineOf],
    em: f64,
) -> Vec<InferredTable> {
    // Rows: fragments by baseline, top first.
    let mut fragments = fragments;
    fragments.sort_by(|a, b| {
        b.baseline
            .total_cmp(&a.baseline)
            .then(a.x0.total_cmp(&b.x0))
    });
    let mut rows: Vec<Vec<Fragment>> = Vec::new();
    for fragment in fragments {
        match rows.last_mut() {
            Some(row)
                if row
                    .first()
                    .is_some_and(|f| (f.baseline - fragment.baseline).abs() <= em * 0.5) =>
            {
                row.push(fragment);
            }
            _ => rows.push(vec![fragment]),
        }
    }
    // Runs of consecutive rows of two fragments or more, no further apart
    // than two lines.
    let mut runs: Vec<Vec<Vec<Fragment>>> = Vec::new();
    let mut current: Vec<Vec<Fragment>> = Vec::new();
    let mut last_baseline: Option<f64> = None;
    for row in rows {
        let baseline = row.first().map_or(0.0, |f| f.baseline);
        let near = last_baseline.is_none_or(|b| b - baseline <= ALIGNED_ROW_LINES * em * 1.2);
        if row.len() >= 2 && near {
            current.push(row);
        } else {
            if current.len() >= ALIGNED_REPEATS {
                runs.push(std::mem::take(&mut current));
            }
            current.clear();
            if row.len() >= 2 {
                current.push(row);
            }
        }
        last_baseline = Some(baseline);
    }
    if current.len() >= ALIGNED_REPEATS {
        runs.push(current);
    }
    runs.into_iter()
        .filter_map(|run| aligned_table(run, flat, line_of, em))
        .collect()
}

/// One run of rows as a table, or `None` when its columns do not recur or
/// look like a page's.
fn aligned_table(
    run: Vec<Vec<Fragment>>,
    flat: &[&TextChar],
    line_of: &[LineOf],
    em: f64,
) -> Option<InferredTable> {
    let total: usize = run.iter().flatten().map(|f| f.chars.len()).sum();
    let rtl_chars: usize = run
        .iter()
        .flatten()
        .filter(|f| f.rtl)
        .map(|f| f.chars.len())
        .sum();
    let right_to_left = rtl_chars * 2 > total;
    // A column's edge: where its fragments start in reading order.
    let edge = |f: &Fragment| if right_to_left { f.x1 } else { f.x0 };
    let tolerance = ALIGNED_EDGE_EMS * em;
    let mut edges: Vec<(f64, usize)> = run
        .iter()
        .enumerate()
        .flat_map(|(r, row)| row.iter().map(move |f| (edge(f), r)))
        .collect();
    edges.sort_by(|a, b| a.0.total_cmp(&b.0));
    // Clusters of edges within the tolerance of their first, kept when
    // enough distinct rows share them.
    let mut starts: Vec<f64> = Vec::new();
    let mut at = 0usize;
    while at < edges.len() {
        let Some(&(first, _)) = edges.get(at) else {
            break;
        };
        let mut end = at + 1;
        while edges.get(end).is_some_and(|e| e.0 - first <= tolerance) {
            end += 1;
        }
        let mut rows: Vec<usize> = edges
            .get(at..end)
            .unwrap_or_default()
            .iter()
            .map(|e| e.1)
            .collect();
        rows.sort_unstable();
        rows.dedup();
        if rows.len() >= ALIGNED_REPEATS {
            starts.push(first);
        }
        at = end;
    }
    if starts.len() < 2 {
        return None;
    }
    // Reading order of the columns: left to right, or right to left.
    if right_to_left {
        starts.reverse();
    }
    // A binary search: starts ascend left to right, and descend right to
    // left.
    let column_of = |f: &Fragment| -> usize {
        let e = edge(f);
        let after = if right_to_left {
            starts.partition_point(|s| *s >= e - tolerance)
        } else {
            starts.partition_point(|s| *s <= e + tolerance)
        };
        after.saturating_sub(1)
    };
    let columns = starts.len();
    let mut widths: Vec<Vec<f64>> = vec![Vec::new(); columns];
    let mut grid: BTreeMap<(usize, usize), Vec<usize>> = BTreeMap::new();
    let mut bands: Vec<(f64, f64)> = Vec::with_capacity(run.len());
    for (r, row) in run.iter().enumerate() {
        let top = row.iter().map(|f| f.y1).fold(f64::NEG_INFINITY, f64::max);
        let bottom = row.iter().map(|f| f.y0).fold(f64::INFINITY, f64::min);
        bands.push((bottom, top));
        for f in row {
            let c = column_of(f);
            if let Some(w) = widths.get_mut(c) {
                w.push(f.x1 - f.x0);
            }
            grid.entry((r, c))
                .or_default()
                .extend(f.chars.iter().copied());
        }
    }
    // Text in fewer than one cell in four is not a table, checked before any
    // cell is made, as for a lattice.
    let slots = run.len().checked_mul(columns)?;
    if grid.len().saturating_mul(LATTICE_TEXT_SHARE) < slots {
        return None;
    }
    // Prose set in columns is not a table.
    let mut wide = 0usize;
    for w in &mut widths {
        w.sort_by(f64::total_cmp);
        if w.get(w.len() / 2)
            .is_some_and(|m| *m >= crate::reading_order::COLUMN_MIN_WIDTH_EMS * em)
        {
            wide += 1;
        }
    }
    if wide > 1 {
        return None;
    }
    let rows = run.len();
    let left = run
        .iter()
        .flatten()
        .map(|f| f.x0)
        .fold(f64::INFINITY, f64::min);
    let right = run
        .iter()
        .flatten()
        .map(|f| f.x1)
        .fold(f64::NEG_INFINITY, f64::max);
    let low = bands.iter().map(|b| b.0).fold(f64::INFINITY, f64::min);
    let high = bands.iter().map(|b| b.1).fold(f64::NEG_INFINITY, f64::max);
    // Column extents in page terms: from a column's start to the next's.
    let mut bounds_x: Vec<f64> = starts.clone();
    bounds_x.sort_by(f64::total_cmp);
    let mut permutation = Vec::new();
    let mut cells = Vec::with_capacity(rows * columns);
    for r in 0..rows {
        for c in 0..columns {
            let mut chars = grid.remove(&(r, c)).unwrap_or_default();
            order_cell(&mut chars, line_of);
            permutation.extend(chars.iter().copied());
            let held: Vec<TextChar> = chars
                .iter()
                .filter_map(|i| flat.get(*i).map(|ch| (*ch).clone()))
                .collect();
            // The page column this reading column is.
            let page_column = if right_to_left { columns - 1 - c } else { c };
            let x0 = if page_column == 0 {
                left
            } else {
                bounds_x.get(page_column).copied().unwrap_or(left)
            };
            let x1 = bounds_x.get(page_column + 1).copied().unwrap_or(right);
            let (y0, y1) = bands.get(r).copied().unwrap_or((low, high));
            cells.push(InferredCell {
                row: r,
                column: c,
                row_span: 1,
                col_span: 1,
                text: held.iter().map(|ch| ch.text.as_str()).collect(),
                chars: held,
                quad: rect(x0, y0, x1, y1),
            });
        }
    }
    Some(InferredTable {
        rows,
        columns,
        cells,
        bounds: rect(left, low, right, high),
        evidence: TableEvidence::Aligned,
        header: if rows < 2 {
            HeaderEvidence::None
        } else {
            HeaderEvidence::FirstRow
        },
        permutation,
        warnings: vec![TableWarning::NoRules],
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
