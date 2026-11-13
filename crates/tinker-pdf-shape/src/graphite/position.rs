//! Positioning: the attributes a description's positioning passes set,
//! resolved into each glyph's integer advance and offset.
//!
//! `docs/design/shaping.md`, *Positioning*, is the reading this follows: in
//! the run's direction, as the GDL states it, an advance moves the slots
//! after it; a shift moves a slot and what is attached to it and nothing
//! else; an attached slot puts its `attach.with` point on its parent's
//! `attach.at` point, its shift added after, and moves the pen not at all;
//! a cluster grows by the guard space the project documentation's font topics
//! describe; and a positive shift moves "further along" the run, so in a
//! right-to-left run it moves left. Attachment chains are resolved from the
//! root, no deeper than [`crate::Limits::max_attachment_depth`], and a cycle
//! is cut, its slots placed where their pens put them, and named.

use crate::buffer::ShapedGlyph;

use super::machine::{Metric, Reads, SlotAttribute};
use super::{Refusal, Slots};

/// What each slot is attached to once every cycle and every chain past the
/// ceiling is cut, the order a parent comes before its children in, and
/// what was cut.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Tree {
    /// Each slot's parent.
    pub parents: Vec<Option<usize>>,
    /// Every slot, each after its parent.
    pub order: Vec<usize>,
    /// What was cut, each once.
    pub refusals: Vec<Refusal>,
}

/// The attachment tree of `slots`, no chain deeper than `max_depth`.
pub(crate) fn tree(slots: &Slots<'_>, max_depth: usize) -> Tree {
    let count = slots.len();
    let mut parents: Vec<Option<usize>> = (0..count)
        .map(|at| slots.attached_to(at).filter(|parent| *parent < count))
        .collect();
    let mut refusals: Vec<Refusal> = Vec::new();
    let glyph = |at: usize| slots.glyph_at(at).unwrap_or(0);
    // 0 not yet reached, 1 on the walk being made, 2 settled.
    let mut state = vec![0u8; count];
    let mut depth = vec![0usize; count];
    for start in 0..count {
        let mut path: Vec<usize> = Vec::new();
        let mut at = start;
        while state[at] == 0 {
            state[at] = 1;
            path.push(at);
            match parents[at] {
                Some(parent) => at = parent,
                None => break,
            }
        }
        // A walk that came back to a slot on itself is a cycle: every slot
        // of it is cut loose.
        if state[at] == 1 && parents[at].is_some() {
            if let Some(from) = path.iter().position(|slot| *slot == at) {
                for slot in &path[from..] {
                    parents[*slot] = None;
                }
                let refusal = Refusal::AttachmentCycle { glyph: glyph(at) };
                if !refusals.contains(&refusal) {
                    refusals.push(refusal);
                }
            }
        }
        for slot in path.iter().rev() {
            depth[*slot] = match parents[*slot] {
                None => 0,
                Some(parent) => depth[parent] + 1,
            };
            if depth[*slot] > max_depth {
                parents[*slot] = None;
                depth[*slot] = 0;
                let refusal = Refusal::AttachmentTooDeep {
                    glyph: glyph(*slot),
                };
                if !refusals.contains(&refusal) {
                    refusals.push(refusal);
                }
            }
            state[*slot] = 2;
        }
    }
    let mut order: Vec<usize> = (0..count).collect();
    order.sort_by_key(|at| depth[*at]);
    Tree {
        parents,
        order,
        refusals,
    }
}

/// Slot `at`'s `attribute`, 0 where the stream has none to give.
fn value(slots: &Slots<'_>, at: usize, attribute: SlotAttribute) -> i64 {
    slots.slot_attribute(at, attribute, 0).map_or(0, i64::from)
}

/// Each slot's place before the pen is applied: its attachment chain's
/// `attach.at − attach.with`, and the shifts along the chain, its own
/// included.
struct Chains {
    /// The attachment alone, along and up.
    attached: Vec<(i64, i64)>,
    /// The shifts.
    shifted: Vec<(i64, i64)>,
    /// The root of each slot's cluster.
    roots: Vec<usize>,
}

/// The chains of `tree` over `slots`.
fn chains(slots: &Slots<'_>, tree: &Tree) -> Chains {
    let count = slots.len();
    let mut attached = vec![(0i64, 0i64); count];
    let mut shifted = vec![(0i64, 0i64); count];
    let mut roots: Vec<usize> = (0..count).collect();
    for &at in &tree.order {
        let shift = (
            value(slots, at, SlotAttribute::ShiftX),
            value(slots, at, SlotAttribute::ShiftY),
        );
        match tree.parents.get(at).copied().flatten() {
            None => {
                shifted[at] = shift;
            }
            Some(parent) => {
                let point = (
                    value(slots, at, SlotAttribute::AttachAtX)
                        - value(slots, at, SlotAttribute::AttachWithX),
                    value(slots, at, SlotAttribute::AttachAtY)
                        - value(slots, at, SlotAttribute::AttachWithY),
                );
                attached[at] = (attached[parent].0 + point.0, attached[parent].1 + point.1);
                shifted[at] = (shifted[parent].0 + shift.0, shifted[parent].1 + shift.1);
                roots[at] = roots[parent];
            }
        }
    }
    Chains {
        attached,
        shifted,
        roots,
    }
}

/// Each slot's height above the baseline: what `position.y` reads.
pub(crate) fn heights(slots: &Slots<'_>, tree: &Tree) -> Vec<i32> {
    let chains = chains(slots, tree);
    (0..slots.len())
        .map(|at| clamp(chains.attached[at].1 + chains.shifted[at].1 + i64::from(slots.fix(at).1)))
        .collect()
}

/// The top of slot `at`'s cluster at `level`: the glyph alone at 0, and at
/// 1 or more the slot and every slot attached to it, each attachment at the
/// GDL's default level 1.
pub(crate) fn composite_top(
    slots: &Slots<'_>,
    tree: &Tree,
    heights: &[i32],
    at: usize,
) -> Result<i32, Refusal> {
    let base = i64::from(heights.get(at).copied().unwrap_or(0));
    let mut top = i64::from(slots.metric(at, Metric::BoundingBoxTop, 0)?);
    for member in 0..slots.len() {
        if member == at || !descends(tree, member, at) {
            continue;
        }
        let height = i64::from(heights.get(member).copied().unwrap_or(0)) - base;
        let member_top = i64::from(slots.metric(member, Metric::BoundingBoxTop, 0)?);
        top = top.max(height + member_top);
    }
    Ok(clamp(top))
}

/// Whether `slot` is attached, at some remove, to `ancestor`.
fn descends(tree: &Tree, slot: usize, ancestor: usize) -> bool {
    let mut at = slot;
    for _ in 0..tree.parents.len() {
        match tree.parents.get(at).copied().flatten() {
            Some(parent) if parent == ancestor => return true,
            Some(parent) => at = parent,
            None => return false,
        }
    }
    false
}

/// `value`, held inside `i32`.
fn clamp(value: i64) -> i32 {
    i32::try_from(value.clamp(i64::from(i32::MIN), i64::from(i32::MAX))).unwrap_or(0)
}

/// Where each slot of the stream stands along the line and above the
/// baseline, x to the right, each cluster's advance, and the root of each
/// slot's cluster; what laying it out cut.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Geometry {
    /// Each slot's x, from the run's left edge.
    pub x: Vec<i64>,
    /// Each slot's y.
    pub y: Vec<i64>,
    /// Each slot's advance: a cluster's root carries its cluster's, a member
    /// none.
    pub advances: Vec<i64>,
    /// The root of each slot's cluster.
    pub roots: Vec<usize>,
    /// The attachments cut, each once.
    pub refusals: Vec<Refusal>,
}

/// The stream laid out along the line, in the run's direction: each
/// cluster's guard space, each slot's attachment and shifts, and each
/// collision fix.
pub(crate) fn geometry(slots: &Slots<'_>, right_to_left: bool) -> Geometry {
    let count = slots.len();
    let tree = tree(slots, slots.attachment_depth());
    let chains = chains(slots, &tree);
    let advance = |at: usize| value(slots, at, SlotAttribute::AdvanceX);
    // Each root's cluster: the guard space its members push out to its left
    // (`low`) and right (`high`), the GDL's "the wider advance and origin
    // regardless of which component of the cluster they come from". A
    // member pushes the right out where its advance is not 0; it pushes the
    // left out where its origin is left of the root's and, as the font
    // topics put it, "the origin of the diacritic is to the left of where it
    // is attached or the advance is non-zero".
    let mut low = vec![0i64; count];
    let mut high: Vec<i64> = (0..count).map(advance).collect();
    for at in 0..count {
        let root = chains.roots[at];
        if root == at {
            continue;
        }
        let origin = chains.attached[at].0;
        let width = advance(at);
        let with = value(slots, at, SlotAttribute::AttachWithX);
        if origin < 0 && (with > 0 || width != 0) {
            low[root] = low[root].min(origin);
        }
        if width != 0 {
            high[root] = high[root].max(origin + width);
        }
    }
    let advances: Vec<i64> = (0..count)
        .map(|at| {
            if chains.roots[at] == at {
                high[at] - low[at]
            } else {
                0
            }
        })
        .collect();
    let total: i64 = advances.iter().sum();
    // Each root's left edge along the line.
    let mut edge = vec![0i64; count];
    let mut along = 0i64;
    for at in 0..count {
        if chains.roots[at] != at {
            continue;
        }
        edge[at] = if right_to_left {
            total - along - advances[at]
        } else {
            along
        };
        along += advances[at];
    }
    let mut x = Vec::with_capacity(count);
    let mut y = Vec::with_capacity(count);
    for at in 0..count {
        let root = chains.roots[at];
        let shift = if right_to_left {
            -chains.shifted[at].0
        } else {
            chains.shifted[at].0
        };
        let fix = slots.fix(at);
        x.push(edge[root] - low[root] + chains.attached[at].0 + shift + i64::from(fix.0));
        y.push(chains.attached[at].1 + chains.shifted[at].1 + i64::from(fix.1));
    }
    Geometry {
        x,
        y,
        advances,
        roots: chains.roots,
        refusals: tree.refusals,
    }
}

/// The run's glyphs as the crate hands a run on: one a slot in logical
/// order, each the real glyph a pseudo-glyph stands for, with its cluster,
/// its advance and its offset from the pen, x to the right. A right-to-left
/// run is drawn by walking its glyphs backwards, as every run of this crate
/// is, so its pen before a glyph is the advances of the glyphs after it.
///
/// Returns the glyphs and the attachments cut, each once.
#[must_use]
pub fn place(slots: &Slots<'_>, right_to_left: bool) -> (Vec<ShapedGlyph>, Vec<Refusal>) {
    let geometry = geometry(slots, right_to_left);
    let count = slots.len();
    let total: i64 = geometry.advances.iter().sum();
    let clusters = slots.clusters();
    let glyphs = slots.real_glyphs();
    let mut pen = if right_to_left { total } else { 0 };
    let mut out = Vec::with_capacity(count);
    for at in 0..count {
        let advance = geometry.advances[at];
        if right_to_left {
            pen -= advance;
        }
        out.push(ShapedGlyph {
            glyph: glyphs.get(at).copied().unwrap_or(0),
            cluster: clusters.get(at).copied().unwrap_or(0),
            x_advance: clamp(advance),
            y_advance: 0,
            x_offset: clamp(geometry.x[at] - pen),
            y_offset: clamp(geometry.y[at]),
            inserted: false,
        });
        if !right_to_left {
            pen += advance;
        }
    }
    (out, geometry.refusals)
}
