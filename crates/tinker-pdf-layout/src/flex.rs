//! `css-flexbox-1` §9, as separable steps.
//!
//! Scope, design and exit criteria: `docs/plans/gaps/31-epub.md`, milestone 12.
//!
//! # Why this is a module of functions and not one procedure
//!
//! §9 is a numbered algorithm in twelve steps and every one of them is a place
//! an implementation can be wrong on its own. A single procedure that produced
//! plausible boxes would be checkable only end to end, and gap 31's rule —
//! *when a thing has two independent consequences, a test for one of them is
//! not a test* — is what says that is not enough. So each step that has an
//! answer of its own is a function with that answer as its return value:
//! [`ordered`] is §5.4, [`lines`] is §9.3 step 5, [`resolve`] is §9.7 whole,
//! [`justify`] is §8.2, [`align`] is §8.3 and [`align_content`] is §8.4. Each
//! has a fixture that fails when that function alone is wrong.
//!
//! # The four things worth knowing before reading further
//!
//! **The free space is distributed twice and the two are not the same
//! distribution.** §9.7 grows and shrinks by *different* rules: growing is in
//! proportion to `flex-grow`, and shrinking is in proportion to `flex-shrink`
//! **times the flex base size**. A build that used the raw factor for both
//! shrinks a wide item and a narrow one by the same number of pixels, which
//! takes the narrow one below its content and leaves the wide one alone. The
//! scaled factor is the whole reason `flex-shrink` is a separate property from
//! `flex-grow` and not a sign.
//!
//! **§9.7 is a loop and not a division.** Distributing the free space once and
//! clamping the answers loses the space the clamped items gave back. The loop
//! freezes the items that violated a minimum and *redistributes*, which is why
//! `resolve` returns after at most one pass per item rather than exactly one.
//!
//! **The `order` property moves boxes and does not move words.** §5.4's own
//! note is that `order` *"does not affect ordering in non-visual media"*, so
//! the items are laid out in **document order** — which is what keeps
//! [`crate::TextRun::order`] ascending and text conservation an equality — and
//! *positioned* in order-modified document order. Those are two loops over the
//! same items, which is milestone 10's finding met for the third time.
//!
//! **A single-line container ignores `align-content` and a multi-line one does
//! not.** §8.4's first sentence says so, and folding the two alignments into
//! one moves every item in a `nowrap` container the moment an author writes a
//! declaration every browser ignores.

use tinker_pdf_css::property::{
    AlignContent, AlignItems, AlignSelf, FlexDirection, FlexWrap, JustifyContent,
};

/// One flex item, as `css-flexbox-1` §9 sees it.
///
/// Every size here is an **inner** (content-box) main size except [`Item::extra`],
/// and §9's sums are all of *outer* sizes — so the two are kept apart rather
/// than folded together, because a build that added the margins into `base`
/// would grow an item's margins along with its content.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Item {
    /// `flex-grow`, §7.1.
    pub grow: f64,
    /// `flex-shrink`, §7.1.
    pub shrink: f64,
    /// §9.2 step 3: the flex base size.
    pub base: f64,
    /// §9.2 step 4: the hypothetical main size — the base size clamped by the
    /// item's own minimum and maximum.
    pub hypothetical: f64,
    /// §4.5's automatic minimum main size, which is the floor §9.7 step 4's
    /// violations are measured against.
    pub min: f64,
    /// The main-axis margin, border and padding, which every §9 sum adds to an
    /// inner size to get an outer one.
    pub extra: f64,
}

impl Item {
    /// The outer hypothetical main size, §9.3 step 5's measure.
    #[must_use]
    pub fn outer_hypothetical(&self) -> f64 {
        self.hypothetical + self.extra
    }

    /// The outer flex base size, §9.7 step 3's measure.
    #[must_use]
    pub fn outer_base(&self) -> f64 {
        self.base + self.extra
    }
}

/// `css-flexbox-1` §5.4: order-modified document order.
///
/// A **stable** sort by `order`, and the stability is the property rather than
/// an implementation detail: §5.4 says items with the same ordinal group are
/// laid out *"in document order"*, so two items that both wrote `order: 2` keep
/// the order the book wrote them in. An unstable sort gives a page that is
/// correct for every fixture with distinct values in it and reorders a real
/// book's figures at random.
#[must_use]
pub fn ordered(orders: &[i32]) -> Vec<usize> {
    let mut out: Vec<usize> = (0..orders.len()).collect();
    out.sort_by_key(|at| orders[*at]);
    out
}

/// `css-flexbox-1` §9.3 step 5: collect the items into flex lines.
///
/// Returns one half-open range of item indices per line, over the items **in
/// order-modified document order** — §9.3 collects in that order, so an
/// `order` that moves a wide item to the front changes which line every item
/// after it lands on.
///
/// A line always takes at least one item, however wide it is: §9.3's own
/// wording is *"if the very first uncollected item wouldn't fit, collect just
/// it into the line"*, and a build without that clause loops forever on an
/// item wider than its container.
#[must_use]
pub fn lines(items: &[Item], available: f64, wrap: FlexWrap) -> Vec<(usize, usize)> {
    if items.is_empty() {
        return Vec::new();
    }
    if !wrap.wraps() {
        return vec![(0, items.len())];
    }
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut used = 0.0f64;
    for (at, item) in items.iter().enumerate() {
        let outer = item.outer_hypothetical();
        if at > start && used + outer > available + EPSILON {
            out.push((start, at));
            start = at;
            used = 0.0;
        }
        used += outer;
    }
    out.push((start, items.len()));
    out
}

/// Tolerance for the comparisons above and in [`resolve`], in CSS pixels.
///
/// A thousandth of a pixel, which is smaller than anything a page can show and
/// larger than the error of summing a few dozen `f64`s. Without it a line whose
/// items sum to exactly the measure wraps or does not depending on the order
/// the additions happened in.
const EPSILON: f64 = 0.001;

/// `css-flexbox-1` §9.7: resolve one line's flexible lengths.
///
/// Returns the **used inner main size** of each item, in the order given.
///
/// The five steps are the specification's own, and the two that an
/// approximation drops are steps 4 and 2 — in that order, because the first is
/// what an approximation notices and the second is what it does not:
///
/// - **Step 4 is a loop.** Clamping the answers once loses the space a clamped
///   item gave back, and the space is not small: two items at `flex-shrink: 1`
///   in half the room they need, one of which cannot go below its longest word,
///   is exactly the arrangement where the one-pass answer and the
///   specification's differ by the whole of the second item's overflow.
/// - **Step 2 freezes the inflexible items before step 3 measures the free
///   space**, and that ordering is the whole of what step 2 is for. The first
///   draft of this comment said something else — that freezing stops a
///   zero-factor item absorbing a share of the distribution — and the injection
///   matrix proved it wrong twice: deleting the freeze changes nothing in an
///   ordinary arrangement, because a zero factor divides to a zero share and
///   step 4's loop clamps by the same minimum step 2's hypothetical size
///   already encodes. What it *does* change is [`initial_free`], which step 3
///   computes **once** out of the split as it then stood: a frozen item
///   contributes its hypothetical size and an unfrozen one contributes its base
///   size, and those differ exactly when a minimum bit. Step 4b then multiplies
///   that number by the flex factors when they sum to less than one, and the
///   two answers part company. `step_two_freezes_before_step_three_measures_the_free_space`
///   is the fixture, one arrangement per half of step 2's condition.
///
/// The `< 1` clause in step 4b is the one that looks like a mistake and is not:
/// when the flex factors on a line sum to less than one, §9.7 distributes only
/// that *fraction* of the free space, so `flex: 0.5` on a lone item grows it by
/// half the room and leaves the rest empty. It is also, as above, the clause
/// that makes step 2 observable at all.
#[must_use]
pub fn resolve(items: &[Item], available: f64) -> Vec<f64> {
    let count = items.len();
    let mut used: Vec<f64> = items.iter().map(|item| item.hypothetical).collect();
    if count == 0 {
        return used;
    }
    // Step 1: which of the two factors this line uses. The comparison is
    // against the **hypothetical** sizes, so an item already clamped by its own
    // minimum counts at the size it will actually take.
    let total: f64 = items.iter().map(Item::outer_hypothetical).sum();
    let growing = total < available;

    // Step 2: size the inflexible items and freeze them.
    let mut frozen = vec![false; count];
    for (at, item) in items.iter().enumerate() {
        let factor = if growing { item.grow } else { item.shrink };
        let wrong_side = if growing {
            item.base > item.hypothetical
        } else {
            item.base < item.hypothetical
        };
        if factor == 0.0 || wrong_side {
            frozen[at] = true;
            used[at] = item.hypothetical;
        } else {
            used[at] = item.base;
        }
    }

    // Step 3: the initial free space, which step 4b's `< 1` clause needs and
    // which is **not** recomputed as the loop runs.
    let initial_free = available
        - items
            .iter()
            .enumerate()
            .map(|(at, item)| {
                if frozen[at] {
                    item.hypothetical + item.extra
                } else {
                    item.outer_base()
                }
            })
            .sum::<f64>();

    // Step 4. At least one item freezes per pass, so `count` passes is a bound
    // rather than a guess -- and it is written as one so that a rounding error
    // in the violation total cannot spin here.
    for _ in 0..count {
        if frozen.iter().all(|f| *f) {
            break;
        }
        // 4b: the remaining free space, over the sizes as they stand.
        let mut remaining = available
            - items
                .iter()
                .enumerate()
                .map(|(at, item)| {
                    if frozen[at] {
                        used[at] + item.extra
                    } else {
                        item.outer_base()
                    }
                })
                .sum::<f64>();
        let factors: f64 = items
            .iter()
            .enumerate()
            .filter(|(at, _)| !frozen[*at])
            .map(|(_, item)| if growing { item.grow } else { item.shrink })
            .sum();
        if factors < 1.0 {
            let scaled = initial_free * factors;
            if scaled.abs() < remaining.abs() {
                remaining = scaled;
            }
        }

        // 4c: distribute. **Two different proportions**, which is the module
        // header's first point: growing shares by the raw factor and shrinking
        // shares by the factor scaled by the base size.
        if growing {
            let sum: f64 = items
                .iter()
                .enumerate()
                .filter(|(at, _)| !frozen[*at])
                .map(|(_, item)| item.grow)
                .sum();
            if sum > 0.0 {
                for (at, item) in items.iter().enumerate() {
                    if !frozen[at] {
                        used[at] = item.base + remaining * item.grow / sum;
                    }
                }
            }
        } else {
            let sum: f64 = items
                .iter()
                .enumerate()
                .filter(|(at, _)| !frozen[*at])
                .map(|(_, item)| item.shrink * item.base)
                .sum();
            if sum > 0.0 {
                for (at, item) in items.iter().enumerate() {
                    if !frozen[at] {
                        let share = item.shrink * item.base / sum;
                        used[at] = item.base - remaining.abs() * share;
                    }
                }
            }
        }

        // 4d and 4e: the violations, and which items freeze because of them.
        let mut violation = 0.0f64;
        let mut clamped = vec![0.0f64; count];
        for (at, item) in items.iter().enumerate() {
            if frozen[at] {
                clamped[at] = used[at];
                continue;
            }
            clamped[at] = used[at].max(item.min);
            violation += clamped[at] - used[at];
        }
        for (at, value) in clamped.iter().enumerate() {
            if !frozen[at] {
                let was = used[at];
                used[at] = *value;
                if violation.abs() <= EPSILON
                    || (violation > 0.0 && *value > was)
                    || (violation < 0.0 && *value < was)
                {
                    frozen[at] = true;
                }
            }
        }
    }
    used
}

/// `css-flexbox-1` §8.2: the offset before the first item and the gap between
/// two of them.
///
/// **The three distribution values behave as something else when the free
/// space is negative**, which `css-align-3` §9.3 calls the *fallback
/// alignment*: `space-between` falls back to `flex-start` and the other two to
/// `center`. Without it, an overflowing `space-between` line pulls its items
/// apart *backwards* and the first one leaves the container on the left.
#[must_use]
pub fn justify(kind: JustifyContent, free: f64, count: usize) -> (f64, f64) {
    if count == 0 {
        return (0.0, 0.0);
    }
    let items = count as f64;
    let negative = free < 0.0;
    match kind {
        JustifyContent::FlexStart => (0.0, 0.0),
        JustifyContent::FlexEnd => (free, 0.0),
        JustifyContent::Center => (free / 2.0, 0.0),
        JustifyContent::SpaceBetween if negative => (0.0, 0.0),
        JustifyContent::SpaceBetween => {
            if count == 1 {
                (0.0, 0.0)
            } else {
                (0.0, free / (items - 1.0))
            }
        }
        JustifyContent::SpaceAround if negative => (free / 2.0, 0.0),
        JustifyContent::SpaceAround => (free / items / 2.0, free / items),
        JustifyContent::SpaceEvenly if negative => (free / 2.0, 0.0),
        JustifyContent::SpaceEvenly => (free / (items + 1.0), free / (items + 1.0)),
    }
}

/// `css-flexbox-1` §8.3: one item's offset from its line's cross-start edge.
///
/// `line_baseline` is the largest distance from a baseline-aligned item's outer
/// cross-start edge to its first baseline, and `item_baseline` is this item's
/// own — so `baseline` is the one value here that depends on something other
/// than the two sizes, which is why it is a parameter rather than a case
/// inside.
///
/// `stretch` returns zero because stretching is a **size** change and not a
/// position: the item was already given the line's cross size before this is
/// asked, and a build that also offset it would move it by the space it no
/// longer has.
#[must_use]
pub fn align(
    kind: AlignItems,
    line_cross: f64,
    item_cross: f64,
    item_baseline: f64,
    line_baseline: f64,
) -> f64 {
    match kind {
        AlignItems::FlexStart | AlignItems::Stretch => 0.0,
        AlignItems::FlexEnd => line_cross - item_cross,
        AlignItems::Center => (line_cross - item_cross) / 2.0,
        AlignItems::Baseline => line_baseline - item_baseline,
    }
}

/// `css-flexbox-1` §8.4: the offset before the first line, the gap between two
/// of them, and the extra cross size each line takes.
///
/// The third number is what makes this a different function from [`justify`]:
/// `align-content: stretch` is the initial value and it does not move a line at
/// all — it makes every line **taller**, which then moves every item inside
/// each of them through [`align`]. A build that mapped `stretch` onto
/// `flex-start` gets every fixture with one line in it right.
#[must_use]
pub fn align_content(kind: AlignContent, free: f64, count: usize) -> (f64, f64, f64) {
    if count == 0 {
        return (0.0, 0.0, 0.0);
    }
    let lines = count as f64;
    let negative = free < 0.0;
    match kind {
        AlignContent::Stretch if negative => (0.0, 0.0, 0.0),
        AlignContent::Stretch => (0.0, 0.0, free / lines),
        AlignContent::FlexStart => (0.0, 0.0, 0.0),
        AlignContent::FlexEnd => (free, 0.0, 0.0),
        AlignContent::Center => (free / 2.0, 0.0, 0.0),
        AlignContent::SpaceBetween if negative => (0.0, 0.0, 0.0),
        AlignContent::SpaceBetween => {
            if count == 1 {
                (0.0, 0.0, 0.0)
            } else {
                (0.0, free / (lines - 1.0), 0.0)
            }
        }
        AlignContent::SpaceAround if negative => (free / 2.0, 0.0, 0.0),
        AlignContent::SpaceAround => (free / lines / 2.0, free / lines, 0.0),
    }
}

/// Which end of the main axis an item's position is measured from,
/// `css-flexbox-1` §5.1.
///
/// A separate function because it is the whole of what `row-reverse` and
/// `column-reverse` are, and because the mistake it prevents is invisible on
/// every fixture written with `flex-start`: main-start is the **right** edge of
/// a `row-reverse` container, so the first item goes last and
/// `justify-content: flex-start` puts it against the right.
#[must_use]
pub fn main_position(direction: FlexDirection, offset: f64, size: f64, container: f64) -> f64 {
    if direction.is_reversed() {
        container - offset - size
    } else {
        offset
    }
}

/// Which end of the cross axis an offset is measured from, §5.2.
///
/// **Applied twice, and that is the design rather than a repetition.** §5.2
/// makes `wrap-reverse` exchange cross-start and cross-end, which has two
/// consequences that are separately observable: the *lines* stack the other
/// way, and `align-items: flex-start` inside each line means that line's other
/// edge. Everything above computes in cross-start-relative offsets, and this
/// maps one of them to a physical downward `y` — once for a line inside the
/// container and once for an item inside its line. A build that flipped only
/// the lines lays a one-line `wrap-reverse` container out identically to a
/// `wrap` one, which is exactly the arrangement an author writes it in.
///
/// The corollary is that [`AlignSelf::resolve`] needs no flip of its own: the
/// keyword keeps its meaning and the coordinate system moves, which is what
/// §5.2 actually says.
#[must_use]
pub fn cross_position(wrap: FlexWrap, offset: f64, size: f64, container: f64) -> f64 {
    if wrap == FlexWrap::WrapReverse {
        container - offset - size
    } else {
        offset
    }
}

/// §8.3's resolution of one item's `align-self` against its container's
/// `align-items`.
///
/// A function here rather than a method call at the caller so that the one
/// place `auto` stops being `auto` is nameable — and so that the caller cannot
/// read `AlignSelf::Auto` as an alignment by forgetting to resolve it.
#[must_use]
pub fn self_alignment(item: AlignSelf, container: AlignItems) -> AlignItems {
    item.resolve(container)
}
