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

use crate::flow::{Abreast, BlockRecord, FloatRecord, Flow, Item, ItemKind};
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

/// The part of one band a page draws, in band-local coordinates.
///
/// A band is **one** flow item, so the page cutter's index cursor has no way to
/// say "half of it". This is the other half of that sentence, and it is the
/// idea [`FloatCursor`] already carries for a float: how much of one thing is
/// drawn, kept across pages. It is a coordinate and not an index because a
/// band's items are its cells' concatenated -- monotone within a cell and not
/// across the band -- so "one past the last drawn" has no single index to be.
#[derive(Clone, Copy, Debug)]
struct Slice {
    /// Band-local `y` above which everything is already on an earlier page.
    from: f64,
    /// And at or below which everything is on a later one.
    to: f64,
}

impl Slice {
    /// A band drawn whole, which is every band that fits a page.
    ///
    /// Unbounded on **both** sides rather than starting at zero, so a band
    /// whose first item sits above its own origin is drawn exactly as it was
    /// before there was a cut to express. A neutral value that quietly dropped
    /// such an item would be a slicer that lost content on the books it was
    /// not slicing.
    const WHOLE: Slice = Slice {
        from: f64::NEG_INFINITY,
        to: f64::INFINITY,
    };

    /// Whether an atomic item belongs to this slice, **by its top edge**.
    ///
    /// The top and not the middle or the bottom, because [`slice`] takes the
    /// cut where nothing atomic straddles it: every such item is wholly inside
    /// the slice its top is in. The one exception is an item taller than a
    /// whole page, which no cut avoids -- it is drawn on the page it starts on,
    /// overflows it, and is what [`Warning::TableRowTallerThanPage`] and
    /// [`Warning::FlexLineTallerThanPage`] now name.
    fn holds(self, y: f64) -> bool {
        y + EPSILON >= self.from && y + EPSILON < self.to
    }
}

/// The one item on a page a half-open **index** range cannot describe: a band
/// cut across pages, and how much of it this page draws.
#[derive(Clone, Copy, Debug)]
struct Cutting {
    /// Its index into the flow.
    at: usize,
    /// The part of it this page draws.
    slice: Slice,
}

impl Cutting {
    /// A page cut only between items, which is nearly all of them.
    const NONE: Cutting = Cutting {
        at: usize::MAX,
        slice: Slice::WHOLE,
    };

    /// What of the item at this index the page draws.
    fn of(self, index: usize) -> Slice {
        if index == self.at {
            self.slice
        } else {
            Slice::WHOLE
        }
    }
}

/// Where this page's slice of a band ends, in band-local coordinates.
///
/// `css-break-3` §3.1's class-3 break -- a break **inside** a box, which CSS
/// 2.2 §13.3.3 gives no position for at all. It is here because the
/// alternative, and what this build did until now, is a band drawn over the
/// bottom edge of the page with a warning saying so.
///
/// **A line box is atomic and so is a nested band.** The cut is taken at the
/// top of the first thing that will not fit, so nothing that cannot be halved
/// is halved. A border edge and a row's own spacer *are* divided, because a
/// length can be and a box of text cannot -- and the decorations they anchor
/// are clipped to the cut by [`draw_band`].
///
/// **The `rowspan` constraint is honoured one level up and not here.** A band
/// is already the maximal run of grid rows a spanning cell joins, so any cut
/// inside one crosses a `rowspan` by construction. That is the reason a band
/// is cut only when it begins a page and overflows it anyway: an ordinary
/// table break goes between bands, where [`permitted`] puts it, and this
/// function is reached only when there is no page left to move the band to.
///
/// Returns the cut, and whether the page had to be overflowed to make it.
fn slice(band: &Abreast, from: f64, available: f64) -> (f64, bool) {
    let limit = from + available;
    let mut cut = limit;
    let mut forced = false;
    for item in &band.items {
        match item.kind {
            ItemKind::Line(_)
            | ItemKind::Rows(_)
            | ItemKind::FlexLine(_)
            | ItemKind::Columns(_) => {}
            ItemKind::Margin(_) | ItemKind::Edge => continue,
        }
        if item.y + item.height <= limit + EPSILON {
            continue;
        }
        if item.y <= from + EPSILON {
            // It begins where this page begins and does not end on it. No cut
            // avoids it, so it is drawn here and overflows.
            forced = true;
            continue;
        }
        cut = cut.min(item.y);
    }
    if cut <= from + EPSILON {
        // Every candidate cut is the top of the page, which is a page with
        // nothing on it and a loop with no end. Ruling 2: the page overflows
        // and says so, rather than the book failing.
        forced = true;
        cut = limit;
    }
    (cut, forced)
}

/// Cuts a flow into pages.
pub(crate) fn paginate(flow: Flow, options: &Options, limits: &Limits) -> Result<Layout, Refusal> {
    let mut warnings = flow.warnings.clone();
    let mut pages: Vec<Page> = Vec::new();
    let mut floats: Vec<FloatCursor> = vec![FloatCursor::default(); flow.floats.len()];
    // §9.6's out-of-flow boxes get cursors of their own rather than joining the
    // floats': they are drawn **after** every float, which is §9.9.1's painting
    // order, and they are never pushed.
    let mut placed: Vec<FloatCursor> = vec![FloatCursor::default(); flow.positioned.len()];
    if flow.items.is_empty() && flow.floats.is_empty() && flow.positioned.is_empty() {
        // A book with nothing in it is one empty page rather than none: a
        // caller that got zero pages would have to invent one, and inventing
        // one is where a page of the wrong size comes from.
        pages.push(Page::default());
        return Ok(Layout { pages, warnings });
    }

    let mut cursor = 0usize;
    // How much of the band at `cursor` is already on an earlier page. Zero
    // whenever `cursor` names anything else, which is every item but a band --
    // and zero again the moment `cursor` moves, which is what keeps one `f64`
    // enough for a whole book: only the item the cutter is standing on can be
    // half drawn.
    let mut drawn = 0.0f64;
    let mut after = 0.0;
    while cursor < flow.items.len() {
        let top = flow.items[cursor].y + drawn;
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

        // **Pushed, unless pushing it would not help.** This is the rule
        // [`beside`] already states for a float and it is `css-break-3` §4.3's:
        // a box that does not fit the space left goes whole to the next
        // fragmentainer, and a box that would not fit an empty one either is
        // broken where it stands rather than pushed for ever. For every other
        // item `choose` is the push, because the break it finds is before the
        // item. For a band it is this arm, because there is no break position
        // *inside* one for `choose` to find.
        //
        // Getting this wrong is a blank page, not a wrong one, which is why it
        // is worth the sentence: a band taller than a page that is pushed
        // anyway leaves the page it came from empty and then overflows the
        // next one.
        let overflowing = overflow_at.and_then(|at| match &flow.items[at].kind {
            ItemKind::Rows(band) | ItemKind::FlexLine(band) | ItemKind::Columns(band) => {
                Some((at, &**band))
            }
            ItemKind::Line(_) | ItemKind::Margin(_) | ItemKind::Edge => None,
        });
        if let Some((at, band)) = overflowing {
            let from = if at == cursor { drawn } else { 0.0 };
            let available = limit - flow.items[at].y - from;
            let (end, forced) = slice(band, from, available);
            // Cut it here if it begins this page -- there is nowhere left to
            // move it to -- or if a page of its own would not hold it either
            // and cutting it here costs no overflow. A cut that would overflow
            // is worse than a push, because the push gets a whole page to try
            // again with.
            let hopeless = flow.items[at].height - from > options.height + EPSILON;
            if available > EPSILON && (at == cursor || (hopeless && !forced)) {
                if forced {
                    // **Narrowed, not gone.** What overflows a page now is one
                    // box *inside* the band that is itself taller than a page,
                    // and no longer the band. That is why both warnings stay,
                    // and why they are still two: a host with no table in its
                    // book must not be told that a table row overflowed.
                    warn(
                        &mut warnings,
                        match &flow.items[at].kind {
                            ItemKind::FlexLine(_) => Warning::FlexLineTallerThanPage,
                            ItemKind::Columns(_) => Warning::ColumnTallerThanPage,
                            _ => Warning::TableRowTallerThanPage,
                        },
                    );
                }
                let mut built = page(
                    &flow,
                    cursor,
                    at + 1,
                    top,
                    Cutting {
                        at,
                        slice: Slice { from, to: end },
                    },
                );
                outside(
                    &flow,
                    &mut floats,
                    &mut placed,
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
                // `slice` never returns a cut that made no progress against a
                // page with room on it; this is the assertion that says so
                // rather than the hope that says so.
                assert!(end > from, "a band slice made no progress");
                cursor = at;
                drawn = end;
                continue;
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

        // The band at `cursor`, if there is one being cut, finishes on this
        // page: `drawn` is where its remainder starts and there is nothing
        // below it left to clip.
        let rest = if drawn > 0.0 {
            Cutting {
                at: cursor,
                slice: Slice {
                    from: drawn,
                    to: f64::INFINITY,
                },
            }
        } else {
            Cutting::NONE
        };
        let mut built = page(&flow, cursor, cut.end, top, rest);
        outside(
            &flow,
            &mut floats,
            &mut placed,
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
        drawn = 0.0;
    }

    // **A float can outlive the column it was written in**, and this is the
    // loop that stops that from losing it. Floats are taken out of flow, so a
    // figure at the foot of the last page of a chapter can extend past the last
    // line of it; the column has run out and the float has not. Without this,
    // the pages stop where the text does and the rest of the float is simply
    // never drawn — which is text conservation's own example of the defect it
    // exists for, and it renders beautifully.
    let mut top = after;
    while unfinished(&floats, &flow.floats) || unfinished(&placed, &flow.positioned) {
        let mut built = Page::default();
        outside(
            &flow,
            &mut floats,
            &mut placed,
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

/// Whether any of these records still has something left to draw.
fn unfinished(cursors: &[FloatCursor], records: &[FloatRecord]) -> bool {
    cursors
        .iter()
        .zip(records)
        .any(|(cursor, record)| cursor.next < record.items.len())
}

/// Everything on a page that is not in the column, in CSS 2.2 §9.9.1's order.
///
/// Floats first, then the absolutely positioned boxes, then the `fixed` ones —
/// §9.9.1's layers 5, 8 and 8 again, with `fixed` last because §9.6.1 makes it
/// the one thing that is on every page and therefore over everything on each of
/// them. The reading-order stamp still decides where its **text** goes: `order`
/// sorts the runs afterwards, so a running header printed on every page reads
/// where the document wrote it and not where the page drew it.
#[allow(clippy::too_many_arguments)]
fn outside(
    flow: &Flow,
    floats: &mut [FloatCursor],
    placed: &mut [FloatCursor],
    out: &mut Page,
    top: f64,
    height: f64,
    warnings: &mut Vec<(Warning, usize)>,
) {
    beside(&flow.floats, floats, out, top, height, warnings);
    beside(&flow.positioned, placed, out, top, height, warnings);
    // **§9.6.1's paged answer, in one loop.** *"In the case of paged media,
    // fixed boxes are repeated on every page, and are fixed with respect to the
    // page box."* Their own cursors are not kept, because a box that is drawn
    // whole on every page has nothing to carry forward.
    for record in &flow.fixed {
        emit(
            &record.items,
            &record.blocks,
            0,
            record.items.len(),
            0.0,
            Cutting::NONE,
            out,
        );
    }
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
    records: &[FloatRecord],
    cursors: &mut [FloatCursor],
    out: &mut Page,
    top: f64,
    height: f64,
    warnings: &mut Vec<(Warning, usize)>,
) {
    for (float, cursor) in records.iter().zip(cursors.iter_mut()) {
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
            // **And an absolutely positioned box is never pushed.** The
            // push moves a box to the next page, which is the one thing
            // `position: absolute` forbids: where the box is is the whole of
            // what the declaration said. So a positioned box that does not fit
            // is broken exactly where a float taller than a page is.
            let fits = float.bottom <= top + height + EPSILON;
            if float.pushable && !fits && float.bottom - float.top <= height + EPSILON {
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
        emit(
            &float.items,
            &float.blocks,
            start,
            cursor.next,
            offset,
            Cutting::NONE,
            out,
        );
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
        // items §17.6.1's vertical spacing emits.
        //
        // **This is still the answer now that a band can be cut**, and it is
        // what honours the `rowspan`: a band is the maximal run of grid rows a
        // spanning cell joins, so no cut inside one can avoid crossing one.
        // `slice` is reached only when the band begins a page and overflows it
        // anyway -- when the choice is a crossed `rowspan` or a page drawn
        // over its own bottom edge -- and never when moving the band whole to
        // the next page would do.
        ItemKind::Edge | ItemKind::Rows(_) | ItemKind::FlexLine(_) | ItemKind::Columns(_) => {
            (tier == Tier::WithoutAc).then_some(Cut {
                end: index,
                next: index,
            })
        }
    }
}

/// Builds one page out of a half-open range of flow items.
///
/// `cutting` names the one band this page draws only part of, if there is
/// one, which is the only item a half-open **index** range cannot describe.
fn page(flow: &Flow, start: usize, end: usize, top: f64, cutting: Cutting) -> Page {
    let mut out = Page::default();
    emit(
        &flow.items,
        &flow.blocks,
        start,
        end,
        -top,
        cutting,
        &mut out,
    );
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
    cutting: Cutting,
    out: &mut Page,
) {
    // Decorations first and in tree order, so an ancestor's background is
    // under its descendants'.
    for block in blocks {
        if !block.painted {
            continue;
        }
        let Some(head) = block.first else {
            continue;
        };
        let from = head.max(start);
        let to = block.last.min(end);
        if from >= to {
            continue;
        }
        // A decoration anchored on a band this page only half draws is only
        // half painted: the rest of it belongs to the page the rest of the
        // band is on. `Slice::WHOLE`'s two infinities are what make this a
        // no-op on every page that is cut between items rather than inside
        // one.
        let head_y = items[from].y;
        let box_top = head_y.max(head_y + cutting.of(from).from);
        let tail = &items[to - 1];
        let box_bottom = (tail.y + tail.height).min(tail.y + cutting.of(to - 1).to);
        if box_bottom < box_top {
            continue;
        }
        out.boxes.push(BoxFragment {
            x: block.x,
            // CSS 2.2 §9.4.3's offset, which the flow deliberately does not
            // carry: a relatively positioned box keeps its place in the column
            // and only its ink moves.
            y: box_top + offset + block.dy,
            width: block.width,
            height: (box_bottom - box_top).max(0.0),
            background: block.background,
            border_width: block.border_width,
            border_style: block.border_style,
            border_color: block.border_color,
        });
    }
    for (at, item) in items[start..end].iter().enumerate() {
        match &item.kind {
            ItemKind::Line(line) => {
                let baseline = item.y + offset + line.baseline;
                for run in &line.runs {
                    let mut run = run.clone();
                    // **Added, not assigned.** A run's `y` inside a line box is
                    // CSS 2.2 §10.8.1's shift from the baseline, which
                    // [`LineBox`] documents and `vertical-align` is; this is
                    // where the two become one number on a page.
                    run.y += baseline;
                    out.runs.push(run);
                }
                // §9.2.2's atomic boxes, each a flow of its own hung from this
                // line's baseline. Its runs keep their own reading-order
                // stamps, so an `inline-block` reads where it was written.
                for placed in &line.boxes {
                    emit(
                        &placed.items,
                        &placed.blocks,
                        0,
                        placed.items.len(),
                        baseline + placed.dy,
                        Cutting::NONE,
                        out,
                    );
                }
            }
            // A band is a flow of its own at the band's origin, and it is cut
            // by height where this one is cut by index -- so it has its own
            // function rather than this one recursing. One function still, and
            // the same reason: every band in the book goes through
            // [`draw_band`], nested or not and cut or not, so a nested table's
            // backgrounds cannot quietly stop being drawn and a cut band's
            // cannot quietly stop being clipped.
            ItemKind::Rows(band) | ItemKind::FlexLine(band) | ItemKind::Columns(band) => {
                draw_band(band, item.y + offset, cutting.of(start + at), out);
            }
            ItemKind::Margin(_) | ItemKind::Edge => {}
        }
    }
}

/// Draws one band, or the part of one that belongs to this page.
///
/// `offset` puts the band's local origin in the page's coordinates. A band
/// that began on an earlier page gets a **negative** one, which is what lands
/// the item at band-local `window.from` on this page's top edge.
fn draw_band(band: &Abreast, offset: f64, window: Slice, out: &mut Page) {
    for block in &band.blocks {
        if !block.painted {
            continue;
        }
        let Some(head) = block.first else {
            continue;
        };
        if head >= block.last || block.last > band.items.len() {
            continue;
        }
        let box_top = band.items[head].y.max(window.from);
        let tail = &band.items[block.last - 1];
        let box_bottom = (tail.y + tail.height).min(window.to);
        if box_bottom < box_top {
            continue;
        }
        out.boxes.push(BoxFragment {
            x: block.x,
            y: box_top + offset + block.dy,
            width: block.width,
            height: (box_bottom - box_top).max(0.0),
            background: block.background,
            border_width: block.border_width,
            border_style: block.border_style,
            border_color: block.border_color,
        });
    }
    for item in &band.items {
        if !window.holds(item.y) {
            continue;
        }
        match &item.kind {
            ItemKind::Line(line) => {
                let baseline = item.y + offset + line.baseline;
                for run in &line.runs {
                    let mut run = run.clone();
                    // **Added, not assigned.** A run's `y` inside a line box is
                    // CSS 2.2 §10.8.1's shift from the baseline, which
                    // [`LineBox`] documents and `vertical-align` is; this is
                    // where the two become one number on a page.
                    run.y += baseline;
                    out.runs.push(run);
                }
                // The same atomic boxes, drawn the same way: one function
                // for a band and one for the column, and neither of them
                // gets to forget an `inline-block`.
                for placed in &line.boxes {
                    emit(
                        &placed.items,
                        &placed.blocks,
                        0,
                        placed.items.len(),
                        baseline + placed.dy,
                        Cutting::NONE,
                        out,
                    );
                }
            }
            // A band inside a band is atomic here for a line box's reason:
            // this cut is one height across every cell of the outer band, and
            // the inner one has cells of its own that the same height would
            // not cut in the same places. It is drawn whole on the page its
            // top is on -- and if that overflows, [`slice`] has already said
            // so by name.
            ItemKind::Rows(inner) | ItemKind::FlexLine(inner) | ItemKind::Columns(inner) => {
                draw_band(inner, item.y + offset, Slice::WHOLE, out);
            }
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
