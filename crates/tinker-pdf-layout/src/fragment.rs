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

use crate::flow::{Flow, ItemKind};
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

/// Cuts a flow into pages.
pub(crate) fn paginate(flow: Flow, options: &Options, limits: &Limits) -> Result<Layout, Refusal> {
    let mut warnings = flow.warnings.clone();
    let mut pages: Vec<Page> = Vec::new();
    if flow.items.is_empty() {
        // A book with nothing in it is one empty page rather than none: a
        // caller that got zero pages would have to invent one, and inventing
        // one is where a page of the wrong size comes from.
        pages.push(Page::default());
        return Ok(Layout { pages, warnings });
    }

    let mut cursor = 0usize;
    while cursor < flow.items.len() {
        let top = flow.items[cursor].y;
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

        pages.push(page(&flow, cursor, cut.end, top));
        if pages.len() > limits.max_pages {
            return Err(Refusal::TooManyPages { pages: pages.len() });
        }
        // A cut that made no progress would loop for ever; `choose` never
        // returns one, and this is the assertion that says so rather than the
        // hope that says so.
        assert!(cut.next > cursor, "a page break made no progress");
        cursor = cut.next;
    }

    Ok(Layout { pages, warnings })
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
        // until every rule has been dropped.
        ItemKind::Edge => (tier == Tier::WithoutAc).then_some(Cut {
            end: index,
            next: index,
        }),
    }
}

/// Builds one page out of a half-open range of flow items.
fn page(flow: &Flow, start: usize, end: usize, top: f64) -> Page {
    let mut out = Page::default();
    // Decorations first and in tree order, so an ancestor's background is
    // under its descendants'.
    for block in &flow.blocks {
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
        let box_top = flow.items[from].y;
        let box_bottom = flow.items[to - 1].y + flow.items[to - 1].height;
        out.boxes.push(BoxFragment {
            x: block.x,
            y: box_top - top,
            width: block.width,
            height: (box_bottom - box_top).max(0.0),
            background: block.background,
            border_width: block.border_width,
            border_style: block.border_style,
            border_color: block.border_color,
        });
    }
    for item in &flow.items[start..end] {
        if let ItemKind::Line(line) = &item.kind {
            let baseline = item.y - top + line.baseline;
            for run in &line.runs {
                let mut run = run.clone();
                run.y = baseline;
                out.runs.push(run);
            }
        }
    }
    out
}

fn warn(warnings: &mut Vec<(Warning, usize)>, warning: Warning) {
    if let Some(entry) = warnings.iter_mut().find(|(w, _)| *w == warning) {
        entry.1 += 1;
        return;
    }
    warnings.push((warning, 1));
}
