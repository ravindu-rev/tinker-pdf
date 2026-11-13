//! Collision avoidance: the slots a pass flags shifted out of collision
//! inside their rectangles, and the slots it flags kerned clear of what
//! follows them.
//!
//! `docs/design/shaping.md`, *Collision avoidance*, is the reading this
//! follows. The GDL states what the search weighs and not how it searches;
//! this search is its own and integer. A glyph's shape is its box cut by the
//! four diagonals of its octabox, and the shape of its exclusion glyph where
//! it names one. For each slot flagged to move, in stream order, the
//! candidates are its own place and the places inside its rectangle at which
//! an edge of its shape meets an edge of a neighbour's, widened by the
//! neighbour's margin, along x and along y; each is weighed by the distance
//! moved, the ink it would overlap, the margin it would intrude on weighted
//! by its `marginweight`, and its sequence order and alignment against the
//! neighbours of its sequence classes; the least wins, the nearest its start
//! breaking a tie, and a move smaller than the pass's threshold is not made.
//! A slot no candidate clears keeps its place and is named. A slot flagged
//! to kern takes the least advance that clears the sequence after it by its
//! margin and the width of any space between, inside its x limits. Every
//! slot pair compared is charged against the run's collision ceiling.

use super::machine::{Reads, SlotAttribute};
use super::position::{self, Geometry};
use super::{Pass, Refusal, Slots};

/// `collision.flags`: this glyph is moved by the search.
const FIX: i32 = 1;
/// It is ignored by the search: neither moved nor in the way.
const IGNORE: i32 = 2;
/// It begins a range whose glyphs may collide.
const START: i32 = 4;
/// It ends one.
const END: i32 = 8;
/// It is kerned clear of what follows it.
const KERN: i32 = 16;
/// It is treated as white space by kerning.
const IS_SPACE: i32 = 128;
/// `sequence.order`: not above the glyphs of its proximate class.
const NO_ABOVE: i64 = 4;
/// Not below them.
const NO_BELOW: i64 = 8;

/// What a unit of ink overlapped costs: more than any move inside any
/// rectangle a face can state.
const INK: i64 = 1 << 20;

/// A glyph's shape: its extent along x, along y, along x + y and along
/// x − y, each an inclusive interval.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Shape {
    x: (i64, i64),
    y: (i64, i64),
    sum: (i64, i64),
    difference: (i64, i64),
}

impl Shape {
    /// The shape moved by `(dx, dy)`.
    const fn moved(self, dx: i64, dy: i64) -> Shape {
        Shape {
            x: (self.x.0 + dx, self.x.1 + dx),
            y: (self.y.0 + dy, self.y.1 + dy),
            sum: (self.sum.0 + dx + dy, self.sum.1 + dx + dy),
            difference: (self.difference.0 + dx - dy, self.difference.1 + dx - dy),
        }
    }

    /// The shape widened by `margin` on every side.
    const fn widened(self, margin: i64) -> Shape {
        Shape {
            x: (self.x.0 - margin, self.x.1 + margin),
            y: (self.y.0 - margin, self.y.1 + margin),
            sum: (self.sum.0 - margin, self.sum.1 + margin),
            difference: (self.difference.0 - margin, self.difference.1 + margin),
        }
    }

    /// How far the two overlap: the least move along x or y that would part
    /// them, 0 where they are apart along any axis.
    fn intrusion(&self, other: &Shape) -> i64 {
        let overlap = |a: (i64, i64), b: (i64, i64)| a.1.min(b.1) - a.0.max(b.0);
        [
            overlap(self.x, other.x),
            overlap(self.y, other.y),
            overlap(self.sum, other.sum),
            overlap(self.difference, other.difference),
        ]
        .into_iter()
        .min()
        .unwrap_or(0)
        .max(0)
    }
}

/// `glyph`'s shape at its own origin: its box from `glyf`, cut by its
/// octabox's diagonals where it has one; `None` for a glyph with no
/// outline.
fn glyph_shape(slots: &Slots<'_>, glyph: u16) -> Option<Shape> {
    let face = slots.face();
    let bounds = tinker_pdf_font::glyf::bounds(face.sfnt, glyph)?;
    let (x0, y0, x1, y1) = (
        i64::from(bounds.x_min),
        i64::from(bounds.y_min),
        i64::from(bounds.x_max),
        i64::from(bounds.y_max),
    );
    if x0 >= x1 && y0 >= y1 {
        return None;
    }
    let mut shape = Shape {
        x: (x0, x1),
        y: (y0, y1),
        sum: (x0 + y0, x1 + y1),
        difference: (x0 - y1, x1 - y0),
    };
    let span = (x1 - x0) + (y1 - y0);
    let octabox = face
        .attributes
        .and_then(|attributes| attributes.octabox(glyph).ok().flatten());
    if let (Some(octabox), true) = (octabox, span > 0) {
        // FT-07d.4's reading: each diagonal byte is 255ths of the box's
        // width plus its height, from the corner it is measured from.
        let scaled = |byte: u8, from: i64| from + i64::from(byte) * span / 255;
        shape.sum = (
            scaled(octabox.sum_min(), x0 + y0),
            scaled(octabox.sum_max(), x0 + y0),
        );
        shape.difference = (
            scaled(octabox.difference_min(), x0 - y1),
            scaled(octabox.difference_max(), x0 - y1),
        );
    }
    Some(shape)
}

/// What the search knows of each slot.
struct Scene {
    /// Each slot's shapes where it stands now: its glyph's, and its
    /// exclusion glyph's.
    shapes: Vec<Vec<Shape>>,
    /// Its `collision.flags`.
    flags: Vec<i32>,
}

/// Slot `at`'s `attribute`, 0 where the stream has none to give.
fn value(slots: &Slots<'_>, at: usize, attribute: SlotAttribute) -> i64 {
    slots.slot_attribute(at, attribute, 0).map_or(0, i64::from)
}

/// The scene the stream stands in, its places those of `geometry`.
fn scene(slots: &Slots<'_>, geometry: &Geometry) -> Scene {
    let count = slots.len();
    let glyphs = slots.real_glyphs();
    let mut shapes = Vec::with_capacity(count);
    let mut flags = Vec::with_capacity(count);
    for at in 0..count {
        let (x, y) = (geometry.x[at], geometry.y[at]);
        let mut own = Vec::new();
        if let Some(shape) = glyphs.get(at).and_then(|glyph| glyph_shape(slots, *glyph)) {
            own.push(shape.moved(x, y));
        }
        let exclude = value(slots, at, SlotAttribute::CollisionExcludeGlyph);
        if let Ok(glyph) = u16::try_from(exclude) {
            if glyph != 0 {
                if let Some(shape) = glyph_shape(slots, glyph) {
                    let dx = value(slots, at, SlotAttribute::CollisionExcludeOffsetX);
                    let dy = value(slots, at, SlotAttribute::CollisionExcludeOffsetY);
                    own.push(shape.moved(x + dx, y + dy));
                }
            }
        }
        shapes.push(own);
        flags.push(i32::try_from(value(slots, at, SlotAttribute::CollisionFlags)).unwrap_or(0));
    }
    Scene { shapes, flags }
}

/// The ranges the start and end flags draw: a range begins at a slot flagged
/// to start one and ends after a slot flagged to end one, and a run with
/// neither is one range.
fn ranges(flags: &[i32]) -> Vec<Vec<usize>> {
    let mut out: Vec<Vec<usize>> = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    for (at, flag) in flags.iter().enumerate() {
        if flag & START != 0 && !current.is_empty() {
            out.push(core::mem::take(&mut current));
        }
        current.push(at);
        if flag & END != 0 {
            out.push(core::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Charges `pairs` slot pairs against the run's ceiling; false once it is
/// spent.
fn charge(slots: &mut Slots<'_>, pairs: usize) -> bool {
    slots.spend_collision_work(u64::try_from(pairs).unwrap_or(u64::MAX))
}

/// Runs a pass's collision avoidance: its shifting, as many times as its
/// flags give, and then its kerning where its flags ask for it.
pub(crate) fn avoid(
    slots: &mut Slots<'_>,
    pass: &Pass<'_>,
    right_to_left: bool,
    refusals: &mut Vec<Refusal>,
) {
    let runs = pass.collision_runs();
    let threshold = i64::from(pass.collision_threshold);
    for _ in 0..runs {
        if !shift_all(slots, right_to_left, threshold, refusals) {
            return;
        }
    }
    if runs > 0 && pass.kerning() > 0 {
        kern_all(slots, right_to_left, refusals);
    }
}

/// One run of shifting over every range; false once the work ceiling is
/// spent.
fn shift_all(
    slots: &mut Slots<'_>,
    right_to_left: bool,
    threshold: i64,
    refusals: &mut Vec<Refusal>,
) -> bool {
    let geometry = position::geometry(slots, right_to_left);
    let mut scene = scene(slots, &geometry);
    for range in ranges(&scene.flags) {
        for &at in &range {
            let flag = scene.flags[at];
            if flag & FIX == 0 || flag & IGNORE != 0 || scene.shapes[at].is_empty() {
                continue;
            }
            let neighbours: Vec<usize> = range
                .iter()
                .copied()
                .filter(|other| *other != at && scene.flags[*other] & IGNORE == 0)
                .filter(|other| !scene.shapes[*other].is_empty())
                .collect();
            match shift_one(slots, &scene, at, &neighbours, threshold) {
                Some(Moved::To(fix)) => {
                    let current = slots.fix(at);
                    let dx = i64::from(fix.0) - i64::from(current.0);
                    let dy = i64::from(fix.1) - i64::from(current.1);
                    slots.set_fix(at, fix);
                    for shape in &mut scene.shapes[at] {
                        *shape = shape.moved(dx, dy);
                    }
                }
                Some(Moved::Stays) => {}
                Some(Moved::NoRoom) => {
                    let glyph = slots.real_glyphs().get(at).copied().unwrap_or(0);
                    super::pass::refuse(refusals, Refusal::CollisionUnresolved { glyph });
                }
                None => {
                    super::pass::refuse(refusals, Refusal::CollisionWorkExceeded);
                    return false;
                }
            }
        }
    }
    true
}

/// What the search did with one slot.
enum Moved {
    /// It moved, to this total fix.
    To((i32, i32)),
    /// It stays where it is, nothing in its way or the move under the
    /// threshold.
    Stays,
    /// No candidate inside its rectangle clears the ink in its way: it keeps
    /// its place.
    NoRoom,
}

/// The search for slot `at`; `None` once the work ceiling is spent.
fn shift_one(
    slots: &mut Slots<'_>,
    scene: &Scene,
    at: usize,
    neighbours: &[usize],
    threshold: i64,
) -> Option<Moved> {
    let rectangle = (
        value(slots, at, SlotAttribute::CollisionMinX),
        value(slots, at, SlotAttribute::CollisionMaxX),
        value(slots, at, SlotAttribute::CollisionMinY),
        value(slots, at, SlotAttribute::CollisionMaxY),
    );
    let current = slots.fix(at);
    let current = (i64::from(current.0), i64::from(current.1));
    // The slot's shapes at its place before any fix.
    let base: Vec<Shape> = scene.shapes[at]
        .iter()
        .map(|shape| shape.moved(-current.0, -current.1))
        .collect();
    let weight = value(slots, at, SlotAttribute::CollisionMarginWeight);
    let order = value(slots, at, SlotAttribute::SequenceOrder);
    let class = value(slots, at, SlotAttribute::SequenceClass);
    let proximate = match value(slots, at, SlotAttribute::SequenceProxClass) {
        0 => class,
        other => other,
    };
    let margins: Vec<i64> = neighbours
        .iter()
        .map(|n| value(slots, *n, SlotAttribute::CollisionMargin))
        .collect();
    let classes: Vec<i64> = neighbours
        .iter()
        .map(|n| value(slots, *n, SlotAttribute::SequenceClass))
        .collect();
    let alignments: Vec<(i64, i64)> = neighbours
        .iter()
        .map(|n| slots.sequence_alignment(*n))
        .collect();
    // Candidates: no move, the move it has, and each edge meeting an edge,
    // along x and along y.
    let mut candidates: Vec<(i64, i64)> = vec![(0, 0), current];
    for (index, neighbour) in neighbours.iter().enumerate() {
        let margin = margins[index];
        for mine in &base {
            for theirs in &scene.shapes[*neighbour] {
                let widened = theirs.widened(margin);
                for dx in [
                    widened.x.0 - mine.x.1,
                    widened.x.1 - mine.x.0,
                    widened.sum.0 - mine.sum.1,
                    widened.sum.1 - mine.sum.0,
                    widened.difference.0 - mine.difference.1,
                    widened.difference.1 - mine.difference.0,
                ] {
                    candidates.push((dx, 0));
                }
                for dy in [
                    widened.y.0 - mine.y.1,
                    widened.y.1 - mine.y.0,
                    widened.sum.0 - mine.sum.1,
                    widened.sum.1 - mine.sum.0,
                    mine.difference.1 - widened.difference.0,
                    mine.difference.0 - widened.difference.1,
                ] {
                    candidates.push((0, dy));
                }
            }
        }
    }
    let inside = |c: &(i64, i64)| {
        rectangle.0 <= c.0 && c.0 <= rectangle.1 && rectangle.2 <= c.1 && c.1 <= rectangle.3
    };
    candidates.retain(inside);
    let mut best: Option<((i64, i64), i64, i64)> = None;
    for candidate in &candidates {
        if !charge(slots, neighbours.len()) {
            return None;
        }
        let moved: Vec<Shape> = base
            .iter()
            .map(|shape| shape.moved(candidate.0, candidate.1))
            .collect();
        let mut ink = 0i64;
        let mut cost = candidate.0.abs() + candidate.1.abs();
        for (index, neighbour) in neighbours.iter().enumerate() {
            for theirs in &scene.shapes[*neighbour] {
                let widened = theirs.widened(margins[index]);
                for mine in &moved {
                    ink += mine.intrusion(theirs);
                    cost = cost.saturating_add(mine.intrusion(&widened).saturating_mul(weight));
                    // The vertical orders: kept off the far side of the
                    // proximate class's glyphs, as ink is.
                    if classes[index] == proximate && proximate != 0 {
                        if order & NO_ABOVE != 0 && mine.y.0 > theirs.y.1 {
                            ink += mine.y.0 - theirs.y.1;
                        }
                        if order & NO_BELOW != 0 && mine.y.1 < theirs.y.0 {
                            ink += theirs.y.0 - mine.y.1;
                        }
                    }
                    // Alignment with a glyph of its own class: the closer
                    // their centres stand to one height, the dearer.
                    let (height, align) = alignments[index];
                    if classes[index] == class && class != 0 && height > 0 {
                        let centres = ((mine.y.0 + mine.y.1) - (theirs.y.0 + theirs.y.1)).abs() / 2;
                        if centres < height {
                            cost = cost.saturating_add((height - centres).saturating_mul(align));
                        }
                    }
                }
            }
        }
        cost = cost.saturating_add(ink.saturating_mul(INK));
        let distance = candidate.0.abs() + candidate.1.abs();
        let better = match best {
            None => true,
            Some((_, best_cost, best_distance)) => {
                cost < best_cost || (cost == best_cost && distance < best_distance)
            }
        };
        if better {
            best = Some((*candidate, cost, distance));
        }
    }
    let Some((choice, _, _)) = best else {
        return Some(Moved::Stays);
    };
    // Whether the choice still overlaps ink: then no candidate clears it.
    let clears = base.iter().all(|shape| {
        let moved = shape.moved(choice.0, choice.1);
        neighbours.iter().all(|n| {
            scene.shapes[*n]
                .iter()
                .all(|theirs| moved.intrusion(theirs) == 0)
        })
    });
    let blocked_now = base.iter().any(|shape| {
        let moved = shape.moved(current.0, current.1);
        neighbours.iter().any(|n| {
            scene.shapes[*n]
                .iter()
                .any(|theirs| moved.intrusion(theirs) > 0)
        })
    });
    if !clears && blocked_now {
        return Some(Moved::NoRoom);
    }
    let step = (choice.0 - current.0).abs() + (choice.1 - current.1).abs();
    if choice == current || step < threshold {
        return Some(Moved::Stays);
    }
    let narrow = |v: i64| i32::try_from(v).unwrap_or(0);
    Some(Moved::To((narrow(choice.0), narrow(choice.1))))
}

/// Kerns every slot flagged to kern, in stream order: its advance grows,
/// or shrinks, by the least that clears the sequence after it by its margin
/// and the width of any space between, inside its x limits.
fn kern_all(slots: &mut Slots<'_>, right_to_left: bool, refusals: &mut Vec<Refusal>) {
    let count = slots.len();
    for at in 0..count {
        let geometry = position::geometry(slots, right_to_left);
        let scene = scene(slots, &geometry);
        let flag = scene.flags[at];
        if flag & KERN == 0 || flag & IGNORE != 0 {
            continue;
        }
        // This sequence: back to the slot after the last one kerned, and
        // what is attached to it.
        let first = (0..at)
            .rev()
            .find(|before| scene.flags[*before] & KERN != 0)
            .map_or(0, |before| before + 1);
        let mine: Vec<usize> = (0..count)
            .filter(|slot| {
                let root = geometry.roots[*slot];
                (first..=at).contains(&root) || (first..=at).contains(slot)
            })
            .collect();
        // The spaces after it, and the sequence after them up to the next
        // slot kerned.
        let mut spaces = 0i64;
        let mut next = at + 1;
        while next < count
            && (scene.flags[next] & IS_SPACE != 0 || scene.shapes[next].is_empty())
            && !mine.contains(&next)
        {
            spaces += value(slots, next, SlotAttribute::AdvanceX);
            next += 1;
        }
        let last = (next..count)
            .find(|after| scene.flags[*after] & KERN != 0)
            .unwrap_or(count.saturating_sub(1));
        let theirs: Vec<usize> = (next..=last.max(next))
            .filter(|slot| *slot < count && !mine.contains(slot))
            .filter(|slot| scene.flags[*slot] & IGNORE == 0)
            .collect();
        if !charge(slots, mine.len().saturating_mul(theirs.len())) {
            super::pass::refuse(refusals, Refusal::CollisionWorkExceeded);
            return;
        }
        let margin = value(slots, at, SlotAttribute::CollisionMargin);
        let mut need: Option<i64> = None;
        for a in &mine {
            for b in &theirs {
                for ink in &scene.shapes[*a] {
                    for other in &scene.shapes[*b] {
                        if ink.y.1 < other.y.0 || other.y.1 < ink.y.0 {
                            continue;
                        }
                        let gap = if right_to_left {
                            ink.x.0 - other.x.1
                        } else {
                            other.x.0 - ink.x.1
                        };
                        let wanted = margin + spaces - gap;
                        need = Some(need.map_or(wanted, |need| need.max(wanted)));
                    }
                }
            }
        }
        let low = value(slots, at, SlotAttribute::CollisionMinX);
        let high = value(slots, at, SlotAttribute::CollisionMaxX);
        let kern = need.unwrap_or(low).clamp(low.min(high), high.max(low));
        if kern != 0 {
            slots.kern(at, i32::try_from(kern).unwrap_or(0));
        }
    }
}
