//! The glyphs a lookup acts on, and the bookkeeping that survives one.
//!
//! A [`Buffer`] is the working state of a shaping run: the glyphs so far, in
//! visual order for the direction they were pushed in, each carrying the
//! cluster it came from and the position GPOS has given it. `docs/design/
//! shaping.md` fixes the public shape — [`ShapedGlyph`] is six integers and
//! no more — so everything a lookup needs *between* lookups lives beside it
//! in [`Props`], which is private and does not survive the run.
//!
//! # Why the cluster is not the index
//!
//! A cluster is a byte offset into the text the run was made from, and it is
//! the only thing that survives substitution. A ligature takes the smallest
//! cluster of the glyphs it replaced, so the three code points behind an
//! Arabic lam-alef-hamza still name one position in the source; a multiple
//! substitution gives every output glyph the input's cluster, so a
//! decomposition does not invent offsets that the text does not have. Milestone
//! 7 of the design turns that back into `/ToUnicode`, and it can only do so if
//! nothing here quietly renumbers.
//!
//! # Integer positions, and no floats anywhere
//!
//! Every number here is font design units — the FWORDs the tables carry,
//! widened to `i32`. The crate denies `clippy::float_arithmetic` for ruling
//! 4's reason: scaling to points is `units * size / upem`, one multiply and
//! one divide, both correctly rounded by IEEE 754 and therefore identical on
//! every target, and it happens in the *consumer*. A shaper that carried
//! fractional advances would be a shaper whose output depended on the order
//! its host's compiler chose to fold the additions in.

use crate::gdef::GlyphClass;

/// One positioned glyph.
///
/// The six fields `docs/design/shaping.md` names, in font design units. A
/// consumer scales them by `size / units_per_em`; nothing here does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShapedGlyph {
    /// The glyph index in the face this run was shaped against.
    pub glyph: u16,
    /// The byte offset, in the original text, of the character this glyph
    /// stands for. Several glyphs may share one; one glyph may stand for
    /// several characters, in which case it carries the first.
    pub cluster: u32,
    /// How far the pen moves horizontally after drawing this glyph.
    pub x_advance: i32,
    /// How far the pen moves vertically after drawing this glyph.
    pub y_advance: i32,
    /// Where this glyph is drawn relative to the pen, horizontally.
    pub x_offset: i32,
    /// Where this glyph is drawn relative to the pen, vertically.
    pub y_offset: i32,
}

/// Which way the run is set.
///
/// **Nothing in this milestone reads it, and that is a finding rather than an
/// oversight.** The two places a shaper usually branches on direction are
/// cursive attachment and the attachment pass at the end of positioning, and
/// neither does here: which glyph a cursive join moves is the lookup's own
/// `RIGHT_TO_LEFT` flag rather than the run's direction, and the attachment
/// arithmetic is stated in terms of pen positions *within the buffer's own
/// order*, which makes it the same expression whichever way the pen travels.
///
/// It is carried because the direction is a property of the run that the
/// consumer needs — a `ShapedRun` has one, per `docs/design/shaping.md` — and
/// because milestone 3 derives it from UAX #9 and milestone 6's line
/// reordering is written against it. A caller sets it; until then it is
/// left-to-right.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Direction {
    /// Latin, Devanagari, Han: the pen moves right.
    #[default]
    LeftToRight,
    /// Arabic, Hebrew: the pen moves left.
    RightToLeft,
}

impl Direction {
    /// Whether the pen advances in the direction indices increase in.
    #[must_use]
    pub const fn is_forward(self) -> bool {
        matches!(self, Direction::LeftToRight)
    }
}

/// Everything a lookup needs to know about a glyph that is not part of the
/// output.
///
/// Kept in a parallel vector rather than in [`ShapedGlyph`], because the
/// output type is fixed by the design doc at six integers and because none of
/// this means anything once the run is over.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Props {
    /// The GDEF glyph class, as it stands *now*. It is not simply a lookup in
    /// `GlyphClassDef`: a ligature substitution makes its output a ligature
    /// even in a face whose GDEF says nothing, and a lookup that ignores
    /// ligatures has to see that.
    pub(crate) class: GlyphClass,
    /// The GDEF mark attachment class, for the `markAttachmentType` half of a
    /// lookup flag. Zero where the face defines none.
    pub(crate) mark_attach: u16,
    /// Which ligature this glyph belongs to, or zero. Serial numbers, not
    /// glyph indices, because two identical ligatures in one run are two
    /// different attachment targets.
    pub(crate) lig_id: u16,
    /// Which component of that ligature, counting from one. Zero on the
    /// ligature glyph itself and on everything that is not part of one.
    pub(crate) lig_comp: u16,
    /// How many components the ligature this glyph *is* was made of; one for
    /// everything else. Needed because ligatures nest: a lookup may ligate a
    /// ligature, and the component numbering of the marks hanging off the
    /// inner one has to be renumbered into the outer one's.
    pub(crate) num_comps: u16,
    /// The glyph this one hangs off, as a signed distance in buffer
    /// positions, or `None` for a glyph that stands on its own.
    ///
    /// Signed because either side is possible: a mark's base is behind it,
    /// and a cursive join whose lookup sets `RIGHT_TO_LEFT` moves the
    /// *earlier* glyph onto the later one.
    ///
    /// Attachment cannot be resolved where it is discovered, because the
    /// glyph attached to may still be moved by a lookup that has not run. So
    /// the lookup records the relationship and
    /// [`Buffer::propagate_attachments`] turns every one of them into a
    /// number, once, at the end.
    pub(crate) attached_to: Option<i32>,
}

impl Props {
    /// The state a glyph starts in: nothing known, one component, unattached.
    fn new() -> Self {
        Self {
            num_comps: 1,
            ..Self::default()
        }
    }
}

/// The glyphs a lookup list is applied to.
///
/// Built by a caller from glyph indices — milestone 2's shaper maps text
/// through `cmap` to make one — then handed to
/// [`crate::Layout::substitute`] and [`crate::Layout::position`].
#[derive(Clone, Debug, Default)]
pub struct Buffer {
    glyphs: Vec<ShapedGlyph>,
    props: Vec<Props>,
    direction: Direction,
    /// The next ligature serial. Starts at one so that zero can mean "no
    /// ligature" without a sentinel of its own.
    lig_serial: u16,
}

impl Buffer {
    /// An empty buffer, set left to right.
    #[must_use]
    pub fn new() -> Self {
        Self {
            glyphs: Vec::new(),
            props: Vec::new(),
            direction: Direction::LeftToRight,
            lig_serial: 1,
        }
    }

    /// A buffer of glyph indices, each its own cluster.
    ///
    /// The shape a conformance fixture arrives in — glyph indices with no
    /// text behind them — and the shape milestone 2's `cmap` mapping will
    /// produce with real cluster offsets instead of positions.
    #[must_use]
    pub fn from_glyphs(glyphs: &[u16]) -> Self {
        let mut buffer = Self::new();
        for (at, glyph) in glyphs.iter().enumerate() {
            buffer.push(*glyph, u32::try_from(at).unwrap_or(u32::MAX));
        }
        buffer
    }

    /// Appends a glyph with no position of its own.
    pub fn push(&mut self, glyph: u16, cluster: u32) {
        self.glyphs.push(ShapedGlyph {
            glyph,
            cluster,
            ..ShapedGlyph::default()
        });
        self.props.push(Props::new());
    }

    /// How many glyphs the buffer holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.glyphs.len()
    }

    /// Whether the buffer holds nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }

    /// The glyphs, in order.
    #[must_use]
    pub fn glyphs(&self) -> &[ShapedGlyph] {
        &self.glyphs
    }

    /// One glyph.
    #[must_use]
    pub fn glyph(&self, at: usize) -> Option<&ShapedGlyph> {
        self.glyphs.get(at)
    }

    /// One glyph, to be adjusted.
    ///
    /// A caller fills in the advances from `hmtx` this way before positioning;
    /// it cannot change the buffer's length, which is what keeps the parallel
    /// [`Props`] vector in step.
    pub fn glyph_mut(&mut self, at: usize) -> Option<&mut ShapedGlyph> {
        self.glyphs.get_mut(at)
    }

    /// Which way the run is set.
    #[must_use]
    pub fn direction(&self) -> Direction {
        self.direction
    }

    /// Sets which way the run is set.
    pub fn set_direction(&mut self, direction: Direction) {
        self.direction = direction;
    }

    // --- crate-internal editing ------------------------------------------
    //
    // Every one of these keeps `glyphs` and `props` the same length, which is
    // the invariant the whole crate is written on top of and the reason none
    // of them is public.

    pub(crate) fn props(&self, at: usize) -> Props {
        self.props.get(at).copied().unwrap_or_else(Props::new)
    }

    pub(crate) fn props_mut(&mut self, at: usize) -> Option<&mut Props> {
        self.props.get_mut(at)
    }

    pub(crate) fn glyph_id(&self, at: usize) -> Option<u16> {
        self.glyphs.get(at).map(|g| g.glyph)
    }

    /// Replaces the glyph at `at`, leaving its cluster and position alone.
    pub(crate) fn set_glyph(&mut self, at: usize, glyph: u16) {
        if let Some(slot) = self.glyphs.get_mut(at) {
            slot.glyph = glyph;
        }
    }

    /// Replaces the glyph at `at` with `glyphs`, all sharing its cluster.
    ///
    /// An empty replacement deletes, which GSUB type 2 permits in as many
    /// words: a `Sequence` with a `glyphCount` of zero removes the glyph.
    pub(crate) fn replace(&mut self, at: usize, glyphs: &[u16]) {
        let Some(existing) = self.glyphs.get(at).copied() else {
            return;
        };
        let props = self.props(at);
        let replacements: Vec<ShapedGlyph> = glyphs
            .iter()
            .map(|glyph| ShapedGlyph {
                glyph: *glyph,
                ..existing
            })
            .collect();
        let props = vec![props; glyphs.len()];
        self.glyphs.splice(at..=at, replacements);
        self.props.splice(at..=at, props);
    }

    /// Removes the glyph at `at`.
    pub(crate) fn remove(&mut self, at: usize) {
        if at < self.glyphs.len() {
            self.glyphs.remove(at);
            self.props.remove(at);
        }
    }

    /// A fresh ligature serial, or zero once the run has made 65 534 of them.
    ///
    /// Zero is the "not a ligature" sentinel, so running out means later
    /// ligatures carry no identity and marks fall back to the last component
    /// — a degradation rather than a wrong attachment, and unreachable in any
    /// run a reader would look at.
    pub(crate) fn next_lig_id(&mut self) -> u16 {
        let id = self.lig_serial;
        self.lig_serial = self.lig_serial.saturating_add(1);
        if id == u16::MAX {
            0
        } else {
            id
        }
    }

    /// Records that the glyph at `at` hangs off the one at `to`.
    pub(crate) fn attach(&mut self, at: usize, to: usize) {
        let chain = i64::try_from(to).unwrap_or(0) - i64::try_from(at).unwrap_or(0);
        let Ok(chain) = i32::try_from(chain) else {
            return;
        };
        if let Some(props) = self.props.get_mut(at) {
            props.attached_to = Some(chain);
        }
    }

    /// Zeroes the advance of every glyph currently classed as a mark.
    ///
    /// The class read is the one on [`Props`] rather than a fresh `GDEF`
    /// lookup, which matters after substitution: a ligature made entirely of
    /// marks is a mark, and a face with no `GlyphClassDef` has only what the
    /// substitutions declared. See [`crate::MarkWidths`] for why this is a
    /// caller's decision at all.
    pub(crate) fn zero_mark_advances(&mut self) {
        for (glyph, props) in self.glyphs.iter_mut().zip(self.props.iter()) {
            if props.class == GlyphClass::Mark {
                glyph.x_advance = 0;
                glyph.y_advance = 0;
            }
        }
    }

    /// Resolves every attachment recorded during positioning.
    ///
    /// # What this pass is for
    ///
    /// A mark-to-base lookup knows only that the mark's anchor must meet the
    /// base's. It cannot write a final offset, for two reasons that both
    /// arrive later: the base may still be moved by a lookup that has not run
    /// yet, and the pen has advanced past the base — and past anything between
    /// them — by the time the mark is drawn. So the lookup records the
    /// relationship and this pass turns it into a number, once, when nothing
    /// can move again.
    ///
    /// Cursive attachment resolves through the same arithmetic as mark
    /// attachment, which it would not in an implementation that expressed a
    /// cursive join by rewriting advances; see [`crate::gpos`]'s type 3 for
    /// why this one does not.
    ///
    /// # The arithmetic, stated once
    ///
    /// Write `pen(k)` for the sum of the advances of every glyph before `k`.
    /// A glyph is drawn at `pen(k) + offset(k)`, so aligning the child's
    /// anchor with the parent's means
    ///
    /// ```text
    /// offset(child) = offset(parent) + (pen(parent) - pen(child))
    ///               + (anchor(parent) - anchor(child))
    /// ```
    ///
    /// The lookup already wrote the anchor difference; this pass adds the
    /// other two terms. `pen(parent) - pen(child)` is the signed sum of the
    /// advances between them, and it is signed rather than direction-
    /// dependent on purpose: the formula is stated in the buffer's own order,
    /// so it is the same arithmetic whether the parent is behind the child (a
    /// mark on its base) or ahead of it (a cursive join whose lookup sets
    /// `RIGHT_TO_LEFT`), and whether the run reads left to right or right to
    /// left.
    ///
    /// The parent's own offset is inherited first, which is what makes a mark
    /// on a mark on a base land where the base did, and a chain of cursively
    /// joined letters rise and fall as one. Each glyph is resolved once and a
    /// glyph already on the current chain ends it, so attachments that form a
    /// cycle terminate with the cycle broken rather than looping.
    pub(crate) fn propagate_attachments(&mut self) {
        let len = self.glyphs.len();
        for start in 0..len {
            // Walk to the end of this glyph's chain, then unwind, so a parent
            // is always resolved before the child that inherits from it.
            let mut chain: Vec<usize> = Vec::new();
            let mut at = start;
            while let Some(step) = self.props(at).attached_to {
                if chain.contains(&at) || chain.len() > len {
                    break;
                }
                chain.push(at);
                let next = i64::from(step) + i64::try_from(at).unwrap_or(0);
                let Ok(next) = usize::try_from(next) else {
                    break;
                };
                if next >= len {
                    break;
                }
                at = next;
            }
            for at in chain.into_iter().rev() {
                self.resolve_one(at);
            }
        }
    }

    /// One link of an attachment chain, with its target already resolved.
    fn resolve_one(&mut self, at: usize) {
        let Some(step) = self.props(at).attached_to else {
            return;
        };
        let target = i64::from(step) + i64::try_from(at).unwrap_or(0);
        let Ok(target) = usize::try_from(target) else {
            return;
        };
        let Some(parent) = self.glyphs.get(target).copied() else {
            return;
        };
        // Cleared first, so a chain that revisits this glyph stops here
        // rather than adding the same correction twice.
        if let Some(props) = self.props.get_mut(at) {
            props.attached_to = None;
        }
        let (mut x, mut y) = (parent.x_offset, parent.y_offset);
        // `pen(parent) - pen(child)`: the advances between them, added when
        // the parent is ahead and subtracted when it is behind.
        let (from, to, ahead) = if target > at {
            (at, target, true)
        } else {
            (target, at, false)
        };
        for between in from..to {
            let Some(glyph) = self.glyphs.get(between) else {
                break;
            };
            if ahead {
                x = x.saturating_add(glyph.x_advance);
                y = y.saturating_add(glyph.y_advance);
            } else {
                x = x.saturating_sub(glyph.x_advance);
                y = y.saturating_sub(glyph.y_advance);
            }
        }
        if let Some(glyph) = self.glyphs.get_mut(at) {
            glyph.x_offset = glyph.x_offset.saturating_add(x);
            glyph.y_offset = glyph.y_offset.saturating_add(y);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Buffer, Direction};

    #[test]
    fn a_buffer_from_glyphs_numbers_its_own_clusters() {
        let buffer = Buffer::from_glyphs(&[3, 1, 4]);
        assert_eq!(buffer.len(), 3);
        assert_eq!(
            buffer
                .glyphs()
                .iter()
                .map(|g| g.cluster)
                .collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
    }

    #[test]
    fn replacement_keeps_the_two_vectors_in_step() {
        let mut buffer = Buffer::from_glyphs(&[1, 2, 3]);
        buffer.replace(1, &[7, 8, 9]);
        assert_eq!(
            buffer.glyphs().iter().map(|g| g.glyph).collect::<Vec<_>>(),
            vec![1, 7, 8, 9, 3]
        );
        // Every output of a multiple substitution carries the input's
        // cluster, so text extraction still finds one offset behind them.
        assert_eq!(
            buffer
                .glyphs()
                .iter()
                .map(|g| g.cluster)
                .collect::<Vec<_>>(),
            vec![0, 1, 1, 1, 2]
        );
        for at in 0..buffer.len() {
            assert!(buffer.props_mut(at).is_some(), "props fell behind glyphs");
        }
    }

    #[test]
    fn an_empty_replacement_deletes() {
        let mut buffer = Buffer::from_glyphs(&[1, 2, 3]);
        buffer.replace(1, &[]);
        assert_eq!(
            buffer.glyphs().iter().map(|g| g.glyph).collect::<Vec<_>>(),
            vec![1, 3]
        );
        assert!(buffer.props_mut(1).is_some());
        assert!(buffer.props_mut(2).is_none());
    }

    #[test]
    fn direction_is_carried_and_read_back() {
        let mut buffer = Buffer::new();
        assert_eq!(buffer.direction(), Direction::LeftToRight);
        buffer.set_direction(Direction::RightToLeft);
        assert!(!buffer.direction().is_forward());
    }
}
