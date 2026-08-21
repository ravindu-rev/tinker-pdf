//! Fragmentation: one continuous column into pages, CSS 2.2 §13.3.
//!
//! # Where a break is *permitted* is not where a break is *preferred*
//!
//! An implementation written from the property list alone knows §13.3.1's
//! `page-break-before`, `page-break-after` and `page-break-inside` and
//! §13.3.2's `orphans` and `widows`, and will fill a page until it is full and
//! then cut. That answers *where should the break go*. §13.3.3 answers a
//! different question — *where may a break happen at all* — and a fragmenter
//! that only answers the first breaks inside things it must not.
//!
//! §13.3.3 gives two kinds of position and four rules:
//!
//! - **(1)** in the vertical margin between block-level boxes;
//! - **(2)** between line boxes inside a block container.
//!
//! - **Rule A** — a break at (1) is allowed only if the `page-break-after` and
//!   `page-break-before` of every element meeting at that margin allow it: at
//!   least one is `always`/`left`/`right`, or all of them are `auto`.
//! - **Rule B** — but if all of them are `auto` and a common ancestor has
//!   `page-break-inside: avoid`, it is not allowed.
//! - **Rule C** — a break at (2) is allowed only if at least `orphans` line
//!   boxes are left behind and at least `widows` are carried forward.
//! - **Rule D** — and only if `page-break-inside` is `auto`.
//!
//! **`orphans` and `widows` are two constraints that interact**, and rule C is
//! written as one sentence with an *and* in it for that reason. A fixture that
//! satisfies one can violate the other: a paragraph of three lines with
//! `orphans: 2` may be broken after its second line and not after its first,
//! and with `widows: 2` as well it may not be broken at all. The tests assert
//! each side on its own and then the pair, because a build that checked only
//! the orphan count passes every fixture that happens to be long enough.
//!
//! # The escape, which is the part an implementation omits
//!
//! §13.3.3 ends: *"if the above does not provide enough break points to keep
//! content from overflowing the page boxes, then rules B and D are dropped in
//! order to find additional breakpoints. If that still does not lead to
//! sufficient break points, rules A and C are dropped as well."* Without it, a
//! book with `page-break-inside: avoid` on `body` — which is a thing a real
//! stylesheet does — is one page as tall as the book, and every page after the
//! first is blank. With it, the break happens and [`Warning::BreakForcedPastTheRules`]
//! says where the rules had to be given up.

use crate::flow::{BlockRecord, Flow, Item, ItemKind};
use crate::{BoxFragment, Layout, Limits, Options, Page, Refusal, Warning};

/// Slack for a comparison against a page height, in points.
///
/// A line whose bottom lands on the page boundary to within a thousandth of a
/// point fits: the alternative is an accumulated rounding error pushing one
/// line onto a page of its own, which is a whole extra page in a book and is
/// not a decision anybody made.
const EPSILON: f64 = 1e-6;

/// Where a page ends and the next begins.
#[derive(Clone, Copy, Debug)]
struct Cut {
    /// One past the last item on this page.
    end: usize,
    /// The first item of the next page. Equal to `end` for a break between
    /// line boxes, and one more for a break **in** a margin — the margin is
    /// consumed by the break rather than appearing at the top of the next
    /// page, which is what stops a chapter opening a page one margin down.
    next: usize,
}

/// Which of §13.3.3's rules are still standing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Tier {
    /// A, B, C and D.
    All,
    /// A and C: *"rules B and D are dropped"*.
    WithoutBd,
    /// None of them: *"rules A and C are dropped as well"*.
    WithoutAc,
}

/// How much of one float has been drawn.
#[derive(Clone, Copy, Debug, Default)]
struct FloatCursor {
    /// One past the last of its items already on a page.
    next: usize,
    /// Whether any of it has been drawn at all. A float that begins below this
    /// page is not started; one that began above it and is still going is.
    started: bool,
}

/// Cuts a flow into pages.
pub(crate) fn paginate(flow: Flow, options: &Options, limits: &Limits) -> Result<Layout, Refusal> {
    let mut warnings = flow.warnings.clone();
    let mut pages: Vec<Page> = Vec::new();
    let mut floats: Vec<FloatCursor> = vec![FloatCursor::default(); flow.floats.len()];
    if flow.items.is_empty() && flow.floats.is_empty() {
        // A book with nothing in it is one empty page rather than none: a
        // caller that got zero pages would have to invent one, and inventing
        // one is where a page of the wrong size comes from.
        pages.push(Page::default());
        return Ok(Layout { pages, warnings });
    }

    // **A band taller than a page has no break position inside it**, so it is
    // drawn where it is and the fact is said out loud. Slicing a band -- every
    // cell cut at the same height and continued on the next page -- is
    // `css-break-3`'s and is the staged half of milestone 11; see
    // [`Warning::TableRowTallerThanPage`] and gap 31's row, amended in place.
    for item in &flow.items {
        if matches!(item.kind, ItemKind::Rows(_)) && item.height > options.height + EPSILON {
            warn(&mut warnings, Warning::TableRowTallerThanPage);
        }
    }

    let mut cursor = 0usize;
    let mut after = 0.0;
    while cursor < flow.items.len() {
        let top = flow.items[cursor].y;
        after = top + options.height;
        let limit = top + options.height;
        let mut forced_at = None;
        let mut overflow_at = None;
        for index in cursor..flow.items.len() {
            if index > cursor {
                if let ItemKind::Margin(margin) = &flow.items[index].kind {
                    if margin.forced {
                        forced_at = Some(index);
                        break;
                    }
                }
            }
            if flow.items[index].y + flow.items[index].height > limit + EPSILON {
                overflow_at = Some(index);
                break;
            }
        }

        let cut = match (forced_at, overflow_at) {
            (Some(at), _) => Cut {
                end: at,
                next: at + 1,
            },
            (None, None) => Cut {
                end: flow.items.len(),
                next: flow.items.len(),
            },
            (None, Some(at)) => choose(&flow, cursor, at, &mut warnings),
        };

        let mut built = page(&flow, cursor, cut.end, top);
        beside(
            &flow,
            &mut floats,
            &mut built,
            top,
            options.height,
            &mut warnings,
        );
        order(&mut built);
        pages.push(built);
        if pages.len() > limits.max_pages {
            return Err(Refusal::TooManyPages { pages: pages.len() });
        }
        // A cut that made no progress would loop for ever; `choose` never
        // returns one, and this is the assertion that says so rather than the
        // hope that says so.
        assert!(cut.next > cursor, "a page break made no progress");
        cursor = cut.next;
    }

    // **A float can outlive the column it was written in**, and this is the
    // loop that stops that from losing it. Floats are taken out of flow, so a
    // figure at the foot of the last page of a chapter can extend past the last
    // line of it; the column has run out and the float has not. Without this,
    // the pages stop where the text does and the rest of the float is simply
    // never drawn — which is text conservation's own example of the defect it
    // exists for, and it renders beautifully.
    let mut top = after;
    while floats
        .iter()
        .zip(&flow.floats)
        .any(|(cursor, float)| cursor.next < float.items.len())
    {
        let mut built = Page::default();
        beside(
            &flow,
            &mut floats,
            &mut built,
            top,
            options.height,
            &mut warnings,
        );
        order(&mut built);
        pages.push(built);
        if pages.len() > limits.max_pages {
            return Err(Refusal::TooManyPages { pages: pages.len() });
        }
        top += options.height;
    }

    Ok(Layout { pages, warnings })
}

/// Sorts a page's runs into document order.
///
/// **Stable**, and that is load-bearing twice: a list marker shares its line's
/// first stamp and has to stay in front of it, and two runs of one line share
/// nothing but their order in the line.
fn order(page: &mut Page) {
    page.runs.sort_by_key(|run| run.order);
}

/// Draws whatever of each float belongs on this page.
///
/// A float is placed in the column's coordinates and drawn in the page's, and
/// the two agree exactly until a float does not fit the page it started on.
/// Then it is **broken**: what fits is drawn here and the rest starts at the
/// top of the next page. `css-break-3` would push the whole box instead, which
/// is a different layout of the text beside it and not a different position for
/// the box — see [`Warning::FloatBrokenAcrossPages`].
fn beside(
    flow: &Flow,
    cursors: &mut [FloatCursor],
    out: &mut Page,
    top: f64,
    height: f64,
    warnings: &mut Vec<(Warning, usize)>,
) {
    for (float, cursor) in flow.floats.iter().zip(cursors.iter_mut()) {
        if cursor.next >= float.items.len() {
            continue;
        }
        let start = cursor.next;
        if !cursor.started {
            if float.items[start].y >= top + height - EPSILON {
                // It begins on a page that has not been reached yet.
                continue;
            }
            // **A float that has not begun is pushed rather than broken**, and
            // that is `css-break-3`'s rule rather than a convenience: a figure
            // that would fit on a page of its own belongs whole on the next
            // one. It costs nothing here because nothing of it has been drawn
            // yet — which is exactly why the same cannot be done once it has,
            // and why a float taller than a whole page is broken wherever it
            // starts rather than pushed for ever.
            //
            // **The margin box decides, not the first item.** Asking whether
            // the first item fits is asking about a zero-height margin, which
            // always fits: the push never ran, the break path did its work
            // instead, and the fixture named for the push passed. The
            // injection campaign is what said so — see the plan's milestone 10
            // note.
            let fits = float.bottom <= top + height + EPSILON;
            if !fits && float.bottom - float.top <= height + EPSILON {
                continue;
            }
            cursor.started = true;
        }
        // A float continuing onto this page starts at the top of it, and
        // everything after it keeps the spacing the column gave it.
        let shift = -(float.items[start].y - top).min(0.0);
        let offset = shift - top;
        while cursor.next < float.items.len() {
            let item = &float.items[cursor.next];
            if item.y + offset + item.height > height + EPSILON && cursor.next > start {
                warn(warnings, Warning::FloatBrokenAcrossPages);
                break;
            }
            cursor.next += 1;
        }
        emit(&float.items, &float.blocks, start, cursor.next, offset, out);
    }
}

/// The best permitted cut at or before `overflow`, relaxing §13.3.3's rules in
/// the order §13.3.3 relaxes them.
fn choose(
    flow: &Flow,
    cursor: usize,
    overflow: usize,
    warnings: &mut Vec<(Warning, usize)>,
) -> Cut {
    for tier in [Tier::All, Tier::WithoutBd, Tier::WithoutAc] {
        let mut best: Option<Cut> = None;
        for index in (cursor + 1)..=overflow {
            if let Some(cut) = permitted(flow, index, tier) {
                best = Some(cut);
            }
        }
        if let Some(cut) = best {
            if tier == Tier::WithoutAc {
                warn(warnings, Warning::BreakForcedPastTheRules);
            }
            return cut;
        }
    }
    // Nothing at all is permitted, which happens when the overflowing item is
    // the first on the page — a line box taller than the page, or a border and
    // padding that fill it. The content is emitted anyway: overflowing a page
    // loses nothing, and dropping the item would lose a line of the book.
    warn(warnings, Warning::BreakForcedPastTheRules);
    Cut {
        end: (overflow).max(cursor + 1),
        next: (overflow).max(cursor + 1),
    }
}

/// Whether a break at this item is permitted under a tier's rules.
fn permitted(flow: &Flow, index: usize, tier: Tier) -> Option<Cut> {
    match &flow.items[index].kind {
        ItemKind::Margin(margin) => {
            let ok = match tier {
                Tier::All => margin.allowed_by_a && margin.allowed_by_b,
                Tier::WithoutBd => margin.allowed_by_a,
                Tier::WithoutAc => true,
            };
            ok.then_some(Cut {
                end: index,
                next: index + 1,
            })
        }
        ItemKind::Line(line) => {
            let before = line.index_in_block;
            let after = line.lines_in_block.saturating_sub(before);
            let rule_c = before >= usize::from(line.orphans) && after >= usize::from(line.widows);
            let rule_d = !line.avoid_inside;
            let ok = match tier {
                Tier::All => rule_c && rule_d,
                Tier::WithoutBd => rule_c,
                Tier::WithoutAc => true,
            };
            ok.then_some(Cut {
                end: index,
                next: index,
            })
        }
        // §13.3.3 gives no break position between a block container's content
        // edge and its child content — that is `css-break-3`'s addition and
        // not CSS 2.2's — so a border or a padding is not a break position
        // until every rule has been dropped. A band of table rows is the same
        // answer for a different reason: **a break inside it would cut a cell
        // in half across a `rowspan`**, and §13.3.3 gives no position there
        // either. A table breaks between its bands, which are the `Margin`
        // items §17.6.1's vertical spacing emits, and a band that is the whole
        // page's worth is drawn where it is.
        ItemKind::Edge | ItemKind::Rows(_) => (tier == Tier::WithoutAc).then_some(Cut {
            end: index,
            next: index,
        }),
    }
}

/// Builds one page out of a half-open range of flow items.
fn page(flow: &Flow, start: usize, end: usize, top: f64) -> Page {
    let mut out = Page::default();
    emit(&flow.items, &flow.blocks, start, end, -top, &mut out);
    out
}

/// Draws a half-open range of one flow's items at a stated offset.
///
/// One function for the column and for every float, because they are the same
/// thing at different origins — and because a second copy of it is where a
/// float's backgrounds would quietly stop being clipped to the page.
fn emit(
    items: &[Item],
    blocks: &[BlockRecord],
    start: usize,
    end: usize,
    offset: f64,
    out: &mut Page,
) {
    // Decorations first and in tree order, so an ancestor's background is
    // under its descendants'.
    for block in blocks {
        if !block.painted {
            continue;
        }
        let Some(first) = block.first else {
            continue;
        };
        let from = first.max(start);
        let to = block.last.min(end);
        if from >= to {
            continue;
        }
        let box_top = items[from].y;
        let box_bottom = items[to - 1].y + items[to - 1].height;
        out.boxes.push(BoxFragment {
            x: block.x,
            y: box_top + offset,
            width: block.width,
            height: (box_bottom - box_top).max(0.0),
            background: block.background,
            border_width: block.border_width,
            border_style: block.border_style,
            border_color: block.border_color,
        });
    }
    for item in &items[start..end] {
        match &item.kind {
            ItemKind::Line(line) => {
                let baseline = item.y + offset + line.baseline;
                for run in &line.runs {
                    let mut run = run.clone();
                    run.y = baseline;
                    out.runs.push(run);
                }
            }
            // A band is a flow of its own at the band's origin, which is the
            // same shape a float is and is drawn by the same function. One
            // function and not two, so a nested table's backgrounds cannot
            // quietly stop being drawn.
            ItemKind::Rows(band) => emit(
                &band.items,
                &band.blocks,
                0,
                band.items.len(),
                item.y + offset,
                out,
            ),
            ItemKind::Margin(_) | ItemKind::Edge => {}
        }
    }
}

fn warn(warnings: &mut Vec<(Warning, usize)>, warning: Warning) {
    if let Some(entry) = warnings.iter_mut().find(|(w, _)| *w == warning) {
        entry.1 += 1;
        return;
    }
    warnings.push((warning, 1));
}
