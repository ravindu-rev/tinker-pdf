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
use crate::universal::Category;

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
/// Milestone 2 recorded that nothing read this, and gave a reason for each of
/// the two places a shaper usually branches on direction. **Both reasons were
/// wrong, and text-rendering-tests SHARAN-1 is what showed it.**
///
/// Cursive attachment reads it, because the line-direction half of a join is
/// paid for by shortening an advance and which of the two glyphs loses the
/// advance is which way the pen travels. And
/// [`Buffer::propagate_attachments`] reads it, because a right-to-left run is
/// reversed before it is drawn, so a mark that follows its base in the buffer
/// precedes it under the pen and the advances between them are added rather
/// than subtracted. Each place says so at length.
///
/// What was right is that the direction is a property of the run a consumer
/// needs — a `ShapedRun` has one, per `docs/design/shaping.md` — and that
/// milestone 3 derives it from UAX #9. A caller sets it; until then it is
/// left-to-right, which is what makes a buffer built from bare glyph indices
/// behave as it always did.
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
    /// Which features may touch this glyph. See [`Buffer::set_mask`].
    pub(crate) mask: u32,
    /// Which cluster of a Brahmic script this glyph belongs to, or zero. See
    /// [`Buffer::set_syllable`].
    pub(crate) syllable: u16,
    /// What the Universal Shaping Engine's cluster model calls this glyph.
    ///
    /// Set from the *character* before any lookup runs and carried on the
    /// glyph from then on, which is the whole point: after `GSUB` has built a
    /// conjunct there is no character left to ask, and the reordering pause
    /// happens after that. See [`Buffer::set_category`].
    pub(crate) category: Category,
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
    /// Whether this glyph is the reph its syllable's `rphf` lookup produced.
    ///
    /// Unlike every other field here it is set from the **buffer** rather than
    /// from the character, and at one named moment: see
    /// [`Buffer::set_repha`].
    pub(crate) repha: bool,
    /// Whether this glyph is a joiner that has to leave before the run does.
    ///
    /// Set from the character, like [`Props::category`] and for the same
    /// reason, and read once at the end of `GSUB`. See
    /// [`Buffer::set_ignorable`].
    pub(crate) ignorable: bool,
    /// Whether that attachment is a cursive join rather than a mark's.
    ///
    /// The two resolve differently and the difference is not cosmetic. A mark
    /// has to be dragged back over every advance between it and its base,
    /// because the pen has moved on since the base was drawn. A cursive join
    /// has already been paid for in the *advances* — see [`crate::gpos`]'s
    /// type 3 — so all that is left to inherit is the parent's own placement
    /// across the line. Walking the advances for a cursive child as well would
    /// subtract the join twice.
    pub(crate) attached_cursively: bool,
}

impl Props {
    /// The state a glyph starts in: nothing known, one component, unattached,
    /// and reachable by every feature the whole run asked for.
    fn new() -> Self {
        Self {
            num_comps: 1,
            mask: Buffer::GLOBAL,
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
    /// The mask bit every glyph carries, and that a feature the whole run
    /// asked for is registered under.
    ///
    /// A caller that never sets a mask gets this on every glyph, so a lookup
    /// list built with no per-glyph features behaves exactly as it did before
    /// masks existed. That is what keeps `tests/aots.rs` — 275 cases that know
    /// nothing about Arabic — meaning the same thing it meant.
    pub const GLOBAL: u32 = 1;

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

    /// Which features may touch the glyph at `at`.
    ///
    /// A bit per feature that is not on everywhere. [`Buffer::GLOBAL`] must be
    /// part of whatever is set, or the glyph becomes invisible to every
    /// feature the run asked for as a whole.
    ///
    /// The reason this exists is that `init`, `medi`, `fina` and `isol` are
    /// one lookup each in most Arabic faces — a single substitution covering
    /// every letter — and the *position* is the whole of what distinguishes
    /// them. A shaper that ran the `init` lookup over the buffer would give
    /// every letter of a word its initial form. So the run decides, once,
    /// which form each glyph is in, and a lookup that came only from `init`
    /// is applied only where that bit is set.
    ///
    /// The mask travels with the glyph through substitution: a ligature keeps
    /// the first component's, a decomposition gives every output the input's.
    /// That is the same rule the cluster follows, and for the same reason.
    pub fn set_mask(&mut self, at: usize, mask: u32) {
        if let Some(props) = self.props.get_mut(at) {
            props.mask = mask;
        }
    }

    /// Which Brahmic cluster the glyph at `at` belongs to, or zero for none.
    ///
    /// # What this restricts, and what it deliberately does not
    ///
    /// A `GSUB` lookup never matches across a syllable boundary once syllables
    /// are set: two adjacent clusters are two words as far as a conjunct-
    /// forming rule is concerned, and a rule that reached across one would
    /// build a conjunct out of the end of one syllable and the start of the
    /// next. The Universal Shaping Engine turns this on for every feature it
    /// asks for, which is why it is a property of the buffer here rather than
    /// a flag on each lookup.
    ///
    /// **`GPOS` is not restricted by it**, and that is the specification's own
    /// asymmetry rather than an omission: kerning and mark attachment are
    /// between neighbours, and two neighbours are frequently in two syllables.
    ///
    /// Zero means "no syllable" and imposes nothing, so a run that never calls
    /// this — every Latin, Arabic and Han run — behaves exactly as it did.
    ///
    /// # What the restriction costs here, measured
    ///
    /// Switching it off entirely — `GSUB` confined to nothing, like `GPOS` —
    /// **gains three text-rendering-tests cases and costs none**:
    /// `SHLANA-3/3`, `SHLANA-4/2` and `SHLANA-7/5`. So the rule has no
    /// positive evidence in this corpus and three cases of negative evidence,
    /// and it stays anyway.
    ///
    /// The reason is that the three do not say the restriction is wrong. They
    /// say the *boundaries* are: `crate::universal`'s syllable rule is a
    /// one-character lookback standing in for USE's regular expression over
    /// cluster types, so a lookup blocked at a boundary this crate invented is
    /// a defect in the grammar and not in the confinement. Dropping the
    /// confinement would trade a rule USE states for three cases and let a
    /// conjunct-forming lookup build one out of two words.
    pub fn set_syllable(&mut self, at: usize, syllable: u16) {
        if let Some(props) = self.props.get_mut(at) {
            props.syllable = syllable;
        }
    }

    /// Records what the cluster model calls the glyph at `at`.
    ///
    /// # Why this is on the glyph and not looked up from the text
    ///
    /// Milestone 5 computed the reordering permutation over the syllable's
    /// **characters** and applied it to its *glyphs*, which is only the same
    /// thing while substitution has not changed how many glyphs a character
    /// stands for. Where it had -- a conjunct built out of three characters, a
    /// decomposition that made two glyphs out of one -- the lengths differed
    /// and the syllable was left alone, so a face that forms its conjuncts
    /// before the reordering pause got no reordering in exactly the clusters
    /// where it mattered.
    ///
    /// Carrying the category on the glyph is what USE does and what closes
    /// that: [`Buffer::replace`] copies a glyph's props onto every glyph it
    /// becomes, and a ligature keeps its first component's -- so a conjunct
    /// built from a base, a halant and a base is a base, which is what a
    /// pre-base vowel in front of it needs it to be.
    pub(crate) fn set_category(&mut self, at: usize, category: Category) {
        if let Some(props) = self.props.get_mut(at) {
            props.category = category;
        }
    }

    /// What the cluster model calls the glyph at `at`, or `None` past the end.
    pub(crate) fn props_category(&self, at: usize) -> Option<Category> {
        self.props.get(at).map(|props| props.category)
    }

    /// Records that the glyph at `at` is the reph its syllable's `rphf`
    /// produced.
    ///
    /// # Why this is asked of the buffer and not of the text
    ///
    /// Everything else the cluster model carries is read off the character
    /// before any lookup runs, because the character is what has the property.
    /// **Whether a `RA` became a reph is not a property of the character**: it
    /// is a property of the *face*, whose `rphf` coverage is the only thing in
    /// the system that knows which consonant it treats that way. The text can
    /// say where the lookup is offered a position — that is
    /// `crate::shape::repha_positions` and the mask it sets — and only the
    /// buffer can say whether it took one.
    ///
    /// So this is set at one named moment, immediately after the `rphf` stage
    /// and before anything else has run, by comparing the mask against what
    /// survived: the pair was two glyphs both carrying the bit, and a reph is
    /// one glyph carrying it with the halant gone. Asking later would confuse
    /// `rphf` with `half`, which forms the same shape out of the same two
    /// characters and is not a reph.
    pub(crate) fn set_repha(&mut self, at: usize, repha: bool) {
        if let Some(props) = self.props.get_mut(at) {
            props.repha = repha;
        }
    }

    /// Whether the glyph at `at` is a reph. See [`Buffer::set_repha`].
    pub(crate) fn props_repha(&self, at: usize) -> bool {
        self.props.get(at).is_some_and(|props| props.repha)
    }

    /// The feature mask of the glyph at `at`, or zero past the end.
    pub(crate) fn props_mask(&self, at: usize) -> u32 {
        self.props.get(at).map_or(0, |props| props.mask)
    }

    /// Marks the glyph at `at` as one that must not survive the run.
    ///
    /// # A joiner has to be in the buffer and must not come out of it
    ///
    /// `ZWNJ` and `ZWJ` exist to be *seen by a lookup*: blocking a ligature —
    /// or demanding one — is the whole of what they are for, and a shaper that
    /// dropped them at `cmap` time would ligate exactly the pairs the author
    /// wrote them to keep apart. So they are mapped, pushed, and carried
    /// through every `GSUB` stage like any other glyph.
    ///
    /// They must equally not reach the caller. A face is free to give `ZWNJ` a
    /// real outline and a real advance — `NotoSansKannada` gives U+200C gid91,
    /// which draws — and a consumer that painted it would put a mark in the
    /// middle of a word. text-rendering-tests `SHKNDA-3/31` states it: four
    /// glyphs expected from text ending in a `ZWNJ`.
    ///
    /// So the flag is set here from the character and read once by
    /// [`Buffer::delete_ignorable`], after the last `GSUB` stage.
    pub(crate) fn set_ignorable(&mut self, at: usize, ignorable: bool) {
        if let Some(props) = self.props.get_mut(at) {
            props.ignorable = ignorable;
        }
    }

    /// Removes every glyph [`Buffer::set_ignorable`] marked, and says how many.
    ///
    /// Backwards, so that a removal does not move the index of one not yet
    /// looked at.
    pub(crate) fn delete_ignorable(&mut self) -> usize {
        let mut removed = 0usize;
        for at in (0..self.props.len()).rev() {
            if self.props.get(at).is_some_and(|props| props.ignorable) {
                self.remove(at);
                removed = removed.saturating_add(1);
            }
        }
        removed
    }

    /// Rearranges `range` so that its *n*th glyph is the one `order` names.
    ///
    /// Reordering is the one editing operation that would break
    /// [`ShapedGlyph::cluster`]'s monotonicity, so it merges the range's
    /// clusters as it goes: every glyph in the range comes out carrying the
    /// smallest of them. That is the right answer as well as the convenient
    /// one — after a pre-base vowel has been moved in front of its consonant,
    /// no glyph of the cluster stands for one character any more, and the
    /// cluster they share is exactly the statement that the whole syllable
    /// stands for the whole of its text.
    pub(crate) fn reorder(&mut self, range: core::ops::Range<usize>, order: &[usize]) {
        if range.end > self.glyphs.len() || range.len() != order.len() {
            return;
        }
        let glyphs: Vec<ShapedGlyph> = order
            .iter()
            .filter_map(|at| self.glyphs.get(range.start.saturating_add(*at)).copied())
            .collect();
        let props: Vec<Props> = order
            .iter()
            .filter_map(|at| self.props.get(range.start.saturating_add(*at)).copied())
            .collect();
        if glyphs.len() != range.len() || props.len() != range.len() {
            return;
        }
        let cluster = glyphs.iter().map(|glyph| glyph.cluster).min().unwrap_or(0);
        for (slot, mut glyph) in range.clone().zip(glyphs) {
            glyph.cluster = cluster;
            if let Some(existing) = self.glyphs.get_mut(slot) {
                *existing = glyph;
            }
        }
        for (slot, props) in range.zip(props) {
            if let Some(existing) = self.props.get_mut(slot) {
                *existing = props;
            }
        }
    }

    /// The half-open ranges of glyphs sharing one non-zero syllable number.
    ///
    /// Read after substitution has changed the buffer's length, so it is
    /// computed rather than remembered.
    pub(crate) fn syllable_ranges(&self) -> Vec<core::ops::Range<usize>> {
        let mut out: Vec<core::ops::Range<usize>> = Vec::new();
        for (at, props) in self.props.iter().enumerate() {
            if props.syllable == 0 {
                continue;
            }
            match out.last_mut() {
                Some(last)
                    if last.end == at
                        && self
                            .props
                            .get(last.start)
                            .is_some_and(|first| first.syllable == props.syllable) =>
                {
                    last.end = at.saturating_add(1);
                }
                _ => out.push(at..at.saturating_add(1)),
            }
        }
        out
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
        self.attach_kind(at, to, false);
    }

    /// The same, for a cursive join. See [`Props::attached_cursively`].
    pub(crate) fn attach_cursive(&mut self, at: usize, to: usize) {
        self.attach_kind(at, to, true);
    }

    fn attach_kind(&mut self, at: usize, to: usize, cursively: bool) {
        let chain = i64::try_from(to).unwrap_or(0) - i64::try_from(at).unwrap_or(0);
        let Ok(chain) = i32::try_from(chain) else {
            return;
        };
        if let Some(props) = self.props.get_mut(at) {
            props.attached_to = Some(chain);
            props.attached_cursively = cursively;
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
    /// other two terms.
    ///
    /// # `pen(parent) - pen(child)` **is** direction-dependent
    ///
    /// Milestone 2 recorded the opposite — that the formula, being stated in
    /// the buffer's own order, was the same arithmetic whichever way the run
    /// read — and text-rendering-tests SHARAN-1 says otherwise. The buffer is
    /// in logical order and a right-to-left run is **reversed before it is
    /// drawn**, so a mark that follows its base in the buffer *precedes* it
    /// under the pen. The pen has not passed the base yet; it has still to
    /// cross the mark's own advance and everything between. So the sum is
    /// added rather than subtracted, and it runs over a window shifted by one.
    ///
    /// The claim was not wrong so much as untested: every fixture milestone 2
    /// had was left to right, and the two forms agree there. The cost of
    /// getting it wrong is one dot of `لسان` sitting 861 units to the left of
    /// the letter it belongs to, which is what this looked like before it was
    /// fixed.
    ///
    /// A cursive join takes neither form; see [`Props::attached_cursively`].
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
        if self.props(at).attached_cursively {
            // A cursive join was already paid for in the advances, so the only
            // thing left to inherit is the parent's placement across the line.
            // Adding the advance walk as well would subtract the join twice.
            if let Some(glyph) = self.glyphs.get_mut(at) {
                glyph.y_offset = glyph.y_offset.saturating_add(parent.y_offset);
            }
            return;
        }
        let (mut x, mut y) = (parent.x_offset, parent.y_offset);
        // `pen(parent) - pen(child)`, and which advances that is depends on
        // which way the consumer will walk the run. See the note on direction
        // in `propagate_attachments`.
        let (from, to) = if self.direction.is_forward() {
            // The mark is drawn after its base, so the pen has already moved
            // over the base and everything between: subtract them.
            (target.min(at), at.max(target))
        } else {
            // The run is reversed before it is drawn, so the mark is drawn
            // *before* its base and the pen has not reached the base yet: add
            // the advances of everything from the mark back to just after the
            // base, the mark's own included.
            (
                target.min(at).saturating_add(1),
                at.max(target).saturating_add(1),
            )
        };
        let sign = if self.direction.is_forward() { -1 } else { 1 };
        for between in from..to {
            let Some(glyph) = self.glyphs.get(between) else {
                break;
            };
            x = x.saturating_add(glyph.x_advance.saturating_mul(sign));
            y = y.saturating_add(glyph.y_advance.saturating_mul(sign));
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
