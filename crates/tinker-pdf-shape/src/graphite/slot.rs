//! The slot stream: one slot a glyph, the attributes rules set on it, and
//! the characters it stands for; and the actions that rewrite it.
//!
//! `docs/design/shaping.md`, *The slot stream* and *Acting on the stream*,
//! is the reading this follows.
//!
//! # A slot's attributes
//!
//! A slot holds only what a rule set. Every other attribute is its glyph's:
//! the advance from the face's metrics, of the real glyph a pseudo-glyph
//! stands for; `breakweight` and `directionality` from the glyph attributes
//! the description names; the collision and sequence attributes from the run
//! of glyph attributes starting at `attCollisions`, where the description
//! has a collision pass; `insert` true; and the rest 0. So a glyph a rule
//! puts in a slot brings its own advance with it, and an attribute a rule set
//! stays set.
//!
//! # The characters a slot stands for
//!
//! Each slot keeps the first and last character it is associated with, as
//! byte offsets into the run's text; its cluster is the first. An `Assoc`
//! gives a slot the span of the input slots it names; an inserted slot takes
//! the characters of the slot after it, or of the slot before it at the
//! stream's end; a deleted slot's characters join the slot before it, or the
//! one after it at the start. [`Slots::clusters`] merges a span of slots whose
//! characters overlap or go backwards to the least of them, so clusters come
//! out monotonic.
//!
//! # The input an action reads
//!
//! An action edits the stream in place, and its operands count slots in the
//! stream as the action found it — "the glyph that was in the input in the
//! given slot" — from the input slot it stands on, an inserted slot standing
//! on the input slot before it. The compiled faces settle that last point: a
//! glyph inserted at the end of a match and associated with the match's last
//! slot is `Assoc 0`, and one inserted at its start and associated with the
//! slot after it is `Assoc 1`. So an action keeps a journal of what it
//! inserted and deleted, and a copy of each input slot it changed, taken
//! before the change.

use tinker_pdf_font::Sfnt;

use super::glat::Attributes;
use super::machine::{self, Budget, Fault, Metric, Op, Program, Reads, SlotAttribute};
use super::{Description, Features, Refusal};
use crate::limits::Limits;

/// The face a stream's slots take their glyphs' defaults from.
#[derive(Clone, Copy, Debug)]
pub struct Source<'f> {
    /// The face: advances from `hmtx`, boxes from `glyf`.
    pub sfnt: &'f Sfnt<'f>,
    /// Its glyph attributes, where it carries `Gloc` and `Glat`.
    pub attributes: Option<&'f Attributes<'f>>,
    /// The description whose passes run.
    pub description: &'f Description<'f>,
    /// Its features, where it carries a `Feat`.
    pub features: Option<&'f Features<'f>>,
}

impl Source<'_> {
    /// Glyph attribute `number` of `glyph`: 0 where the face carries no
    /// attributes or the glyph's runs hold none.
    fn glyph_attribute(&self, glyph: u16, number: u16) -> Result<i32, Refusal> {
        match self.attributes {
            Some(attributes) => attributes.lookup(glyph, number).map(i32::from),
            None => Ok(0),
        }
    }

    /// The real glyph `glyph` stands for: the glyph its `attrPseudo`
    /// attribute names where that is not 0, and itself otherwise.
    fn real(&self, glyph: u16) -> u16 {
        let number = u16::from(self.description.attr_pseudo);
        match self.glyph_attribute(glyph, number) {
            Ok(real) if real > 0 => u16::try_from(real).unwrap_or(glyph),
            _ => glyph,
        }
    }

    /// Whether the description's glyphs carry collision attributes from
    /// `attCollisions`: its flags' bit 5, "has collision pass".
    const fn has_collisions(&self) -> bool {
        self.description.flags & 0x20 != 0
    }
}

/// One slot of the stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slot {
    /// The glyph, which may be a pseudo-glyph.
    glyph: u16,
    /// A number no other slot of the stream has had, which a slot reference
    /// names, so that it survives insertions and deletions.
    id: u32,
    /// The byte offset of the first character the slot stands for.
    first: u32,
    /// And of the last.
    last: u32,
    /// The attributes a rule set, each with its index; the rest are the
    /// glyph's.
    values: Vec<(SlotAttribute, u8, i32)>,
    /// The slot `attach.to` names, by id.
    attach_to: Option<u32>,
    /// The slots its components reference, by index and id.
    components: Vec<(u8, u32)>,
    /// Its feature values, where `SetFeat` changed one; the run's otherwise.
    features: Option<Vec<i16>>,
}

impl Slot {
    /// The value a rule set for `attribute` at `index`.
    fn set_value(&self, attribute: SlotAttribute, index: u8) -> Option<i32> {
        self.values
            .iter()
            .find(|(a, i, _)| *a == attribute && *i == index)
            .map(|(_, _, value)| *value)
    }

    /// Sets `attribute` at `index`.
    fn store(&mut self, attribute: SlotAttribute, index: u8, value: i32) {
        match self
            .values
            .iter_mut()
            .find(|(a, i, _)| *a == attribute && *i == index)
        {
            Some(entry) => entry.2 = value,
            None => self.values.push((attribute, index, value)),
        }
    }

    /// The slot's glyph.
    #[must_use]
    pub const fn glyph(&self) -> u16 {
        self.glyph
    }

    /// The first and last characters it stands for, as byte offsets.
    #[must_use]
    pub const fn characters(&self) -> (u32, u32) {
        (self.first, self.last)
    }
}

/// Where an action left the slot pointer, and the offset it returned: the
/// next scan position is the two added.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Acted {
    /// The stream index the slot pointer ended on; −1 where the action
    /// deleted the first slot and did not move on.
    pub pointer: i64,
    /// What the action returned.
    pub offset: i32,
}

/// A run's slot stream, over the face its glyphs come from.
#[derive(Clone, Debug)]
pub struct Slots<'f> {
    face: Source<'f>,
    slots: Vec<Slot>,
    /// Each id's stream index; `u32::MAX` once its slot is gone.
    index: Vec<u32>,
    /// How many ids have been given out, so that none is given twice.
    issued: u32,
    /// The run's feature values, which every slot carries until `SetFeat`
    /// changes its own.
    features: Vec<i16>,
}

impl<'f> Slots<'f> {
    /// A stream of one slot each `(glyph, first, last)`: the glyph and the
    /// byte offsets of the first and last characters it stands for. Every
    /// slot carries `features`, one value a feature in `Feat`'s order.
    #[must_use]
    pub fn new(face: Source<'f>, glyphs: &[(u16, u32, u32)], features: &[i16]) -> Slots<'f> {
        let slots: Vec<Slot> = glyphs
            .iter()
            .enumerate()
            .map(|(id, (glyph, first, last))| Slot {
                glyph: *glyph,
                id: u32::try_from(id).unwrap_or(u32::MAX),
                first: *first,
                last: *last,
                values: Vec::new(),
                attach_to: None,
                components: Vec::new(),
                features: None,
            })
            .collect();
        let issued = u32::try_from(slots.len()).unwrap_or(u32::MAX);
        let mut stream = Slots {
            face,
            slots,
            index: Vec::new(),
            issued,
            features: features.to_vec(),
        };
        stream.reindex();
        stream
    }

    /// The face the stream reads.
    #[must_use]
    pub const fn face(&self) -> &Source<'f> {
        &self.face
    }

    /// How many slots the stream holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Whether it holds none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Slot `at`.
    #[must_use]
    pub fn slot(&self, at: usize) -> Option<&Slot> {
        self.slots.get(at)
    }

    /// Every slot's glyph, in order.
    #[must_use]
    pub fn glyphs(&self) -> Vec<u16> {
        self.slots.iter().map(|slot| slot.glyph).collect()
    }

    /// Every slot's real glyph: a pseudo-glyph's the glyph it stands for.
    #[must_use]
    pub fn real_glyphs(&self) -> Vec<u16> {
        self.slots
            .iter()
            .map(|slot| self.face.real(slot.glyph))
            .collect()
    }

    /// Each slot's cluster: the first character it stands for, a span of
    /// slots whose characters overlap or go backwards merged to the least
    /// of them, so the clusters never go backwards.
    #[must_use]
    pub fn clusters(&self) -> Vec<u32> {
        // A stack of groups, each its first slot and the least and greatest
        // character it stands for; a group that begins at or before the end
        // of the one below it is merged into it.
        let mut groups: Vec<(usize, u32, u32)> = Vec::new();
        for (at, slot) in self.slots.iter().enumerate() {
            let mut group = (at, slot.first.min(slot.last), slot.first.max(slot.last));
            while let Some(&(start, least, greatest)) = groups.last() {
                if group.1 > greatest {
                    break;
                }
                groups.pop();
                group = (start, least.min(group.1), greatest.max(group.2));
            }
            groups.push(group);
        }
        let mut out = vec![0u32; self.slots.len()];
        for (n, (start, least, _)) in groups.iter().enumerate() {
            let end = groups.get(n + 1).map_or(self.slots.len(), |next| next.0);
            if let Some(span) = out.get_mut(*start..end) {
                span.fill(*least);
            }
        }
        out
    }

    /// The stream index of the slot whose id is `id`, where it is there.
    fn find(&self, id: u32) -> Option<usize> {
        let at = *self.index.get(usize::try_from(id).ok()?)?;
        (at != u32::MAX).then_some(at as usize)
    }

    /// Rebuilds the id-to-index table after the stream's structure changed.
    fn reindex(&mut self) {
        self.index.clear();
        self.index.resize(self.issued as usize, u32::MAX);
        for (at, slot) in self.slots.iter().enumerate() {
            if let Some(entry) = self.index.get_mut(slot.id as usize) {
                *entry = u32::try_from(at).unwrap_or(u32::MAX);
            }
        }
    }

    /// The id the next inserted slot takes, given out once.
    fn next_id(&mut self) -> u32 {
        let id = self.issued;
        self.issued = self.issued.saturating_add(1);
        id
    }

    /// The value of `attribute` (at `index`, where it is indexed) for `slot`,
    /// as it reads: what a rule set, or its glyph's.
    fn value_of(
        &self,
        slot: &Slot,
        at: usize,
        attribute: SlotAttribute,
        index: u8,
    ) -> Result<i32, Refusal> {
        if attribute == SlotAttribute::AttachTo {
            return Ok(self.reference_offset(slot.attach_to, at));
        }
        if attribute == SlotAttribute::ComponentReference {
            let id = slot
                .components
                .iter()
                .find(|(i, _)| *i == index)
                .map(|(_, id)| *id);
            return Ok(self.reference_offset(id, at));
        }
        if attribute == SlotAttribute::PositionY {
            return Err(Refusal::PositionNotResolved);
        }
        if let Some(value) = slot.set_value(attribute, index) {
            return Ok(value);
        }
        self.default_of(slot.glyph, attribute)
    }

    /// The offset from the slot at `at` to the slot `id` names, or 0 where
    /// it names none, or one no longer in the stream.
    fn reference_offset(&self, id: Option<u32>, at: usize) -> i32 {
        let Some(target) = id.and_then(|id| self.find(id)) else {
            return 0;
        };
        let difference = i64::try_from(target).unwrap_or(0) - i64::try_from(at).unwrap_or(0);
        i32::try_from(difference).unwrap_or(0)
    }

    /// The value `attribute` reads as for a slot of `glyph` that no rule
    /// set it on.
    fn default_of(&self, glyph: u16, attribute: SlotAttribute) -> Result<i32, Refusal> {
        let face = &self.face;
        let from_collisions = |offset: u16| -> Result<i32, Refusal> {
            if !face.has_collisions() {
                return Ok(0);
            }
            let number = u16::from(face.description.attr_collisions) + offset;
            face.glyph_attribute(glyph, number)
        };
        match attribute {
            SlotAttribute::AdvanceX => Ok(face.sfnt.advance(face.real(glyph)).map_or(0, i32::from)),
            SlotAttribute::BreakWeight => {
                face.glyph_attribute(glyph, u16::from(face.description.attr_break_weight))
            }
            SlotAttribute::Directionality => {
                face.glyph_attribute(glyph, u16::from(face.description.attr_directionality))
            }
            SlotAttribute::Insert => Ok(1),
            SlotAttribute::CollisionFlags => from_collisions(0),
            SlotAttribute::CollisionMinX => from_collisions(1),
            SlotAttribute::CollisionMinY => from_collisions(2),
            SlotAttribute::CollisionMaxX => from_collisions(3),
            SlotAttribute::CollisionMaxY => from_collisions(4),
            SlotAttribute::CollisionMargin => from_collisions(5),
            SlotAttribute::CollisionMarginWeight => from_collisions(6),
            SlotAttribute::SequenceClass => from_collisions(7),
            SlotAttribute::SequenceProxClass => from_collisions(8),
            SlotAttribute::SequenceOrder => from_collisions(9),
            _ => Ok(0),
        }
    }

    /// The value of feature `feature` for the slot at `at`.
    fn feature_of(&self, slot: &Slot, feature: u8) -> i32 {
        let values = slot.features.as_deref().unwrap_or(&self.features);
        values
            .get(usize::from(feature))
            .map_or(0, |v| i32::from(*v))
    }

    /// The top of `glyph`'s bounding box, from `glyf`.
    fn top_of(&self, glyph: u16) -> Result<i32, Refusal> {
        let real = self.face.real(glyph);
        tinker_pdf_font::glyf::bounds(self.face.sfnt, real)
            .map(|bounds| i32::from(bounds.y_max))
            .ok_or(Refusal::GlyphBoundsUnavailable { glyph: real })
    }

    /// Runs `program`, an action, at scan position `position`.
    ///
    /// # Errors
    ///
    /// [`Refusal::TooManyInstructions`] once the budget is spent;
    /// [`Refusal::CodeRefused`] for an instruction that cannot go on — a
    /// slot outside the stream or the input, a class that gives no glyph, an
    /// insertion past [`Limits::max_glyphs`] ([`Fault::TooManySlots`]), and
    /// the computing faults [`machine::answer`] names; and a refusal a read
    /// gives. The action ends there, and what it did to the stream until then
    /// stands.
    pub fn act(
        &mut self,
        program: &Program,
        position: usize,
        budget: &mut Budget,
        limits: &Limits,
    ) -> Result<Acted, Refusal> {
        let mut journal = Journal::new(self.slots.len());
        let acted = self.run(program, position, budget, limits, &mut journal);
        self.settle(&journal);
        acted
    }

    /// The body of [`Slots::act`], its journal the caller's to settle.
    fn run(
        &mut self,
        program: &Program,
        position: usize,
        budget: &mut Budget,
        limits: &Limits,
        journal: &mut Journal,
    ) -> Result<Acted, Refusal> {
        let site = program.site();
        let mut pointer = i64::try_from(position).unwrap_or(i64::MAX);
        let mut stack: Vec<i32> = Vec::new();
        let mut index = 0usize;
        let instructions = program.instructions();
        while let Some(instruction) = instructions.get(index) {
            budget.charge()?;
            let refuse = |fault: Fault| Refusal::CodeRefused {
                pass: site.pass,
                block: site.block,
                at: instruction.at,
                fault,
            };
            let cursor = journal.cursor(self, pointer);
            match instruction.op {
                Op::PopReturn => {
                    let offset = stack.pop().ok_or_else(|| refuse(Fault::StackUnderflow))?;
                    return Ok(Acted { pointer, offset });
                }
                Op::ReturnZero => return Ok(Acted { pointer, offset: 0 }),
                Op::ReturnTrue => return Ok(Acted { pointer, offset: 1 }),
                // `CopyNext` leaves the current slot as it stands: in place,
                // it is already the input's.
                Op::Next | Op::CopyNext => {
                    if pointer >= i64::try_from(self.len()).unwrap_or(i64::MAX) {
                        return Err(refuse(Fault::SlotOutOfRange));
                    }
                    pointer += 1;
                }
                Op::Insert => {
                    let at = usize::try_from(pointer)
                        .ok()
                        .filter(|at| *at <= self.len())
                        .ok_or_else(|| refuse(Fault::SlotOutOfRange))?;
                    if self.len() >= limits.max_glyphs {
                        return Err(refuse(Fault::TooManySlots));
                    }
                    self.insert(at);
                    journal.edits.push(Edit::Inserted(at));
                }
                Op::Delete => {
                    let at = self
                        .current(pointer)
                        .ok_or_else(|| refuse(Fault::SlotOutOfRange))?;
                    journal.save(self, at);
                    // Where its characters go unless an `Assoc` of this
                    // action names them: the slot before, or the one after
                    // at the start.
                    let neighbour = if at > 0 {
                        self.slots.get(at - 1)
                    } else {
                        self.slots.get(at + 1)
                    }
                    .map(|slot| slot.id);
                    if let Some(gone) = self.slots.get(at) {
                        journal.deleted.push(Deleted {
                            input: journal.origin(at),
                            id: gone.id,
                            first: gone.first,
                            last: gone.last,
                            neighbour,
                        });
                    }
                    self.delete(at);
                    journal.edits.push(Edit::Deleted(at));
                    pointer -= 1;
                }
                op => {
                    let view = Window {
                        stream: self,
                        journal,
                    };
                    let base = cursor.unwrap_or(-1);
                    match machine::compute(op, &mut stack, &view, base) {
                        Ok(()) => {}
                        Err(machine::Computed::Fault(fault)) => return Err(refuse(fault)),
                        Err(machine::Computed::Refused(refusal)) => return Err(refusal),
                        Err(machine::Computed::NotComputing) => {
                            self.edit(op, program, &mut stack, pointer, cursor, journal)
                                .map_err(|outcome| match outcome {
                                    Outcome::Fault(fault) => refuse(fault),
                                    Outcome::Refused(refusal) => refusal,
                                })?;
                        }
                    }
                }
            }
            index += 1;
        }
        Ok(Acted {
            pointer,
            offset: stack.last().copied().unwrap_or(0),
        })
    }

    /// The stream index of the current slot, where the pointer stands on
    /// one.
    fn current(&self, pointer: i64) -> Option<usize> {
        usize::try_from(pointer).ok().filter(|at| *at < self.len())
    }

    /// Runs one instruction that changes a slot: the current one, or the
    /// one at a feature's offset.
    fn edit(
        &mut self,
        op: Op,
        program: &Program,
        stack: &mut Vec<i32>,
        pointer: i64,
        cursor: Option<i64>,
        journal: &mut Journal,
    ) -> Result<(), Outcome> {
        let out_of_range = Outcome::Fault(Fault::SlotOutOfRange);
        let at = self
            .current(pointer)
            .ok_or(Outcome::Fault(Fault::SlotOutOfRange))?;
        let cursor = cursor.ok_or(Outcome::Fault(Fault::SlotOutOfRange))?;
        // The input slot an operand names, as the action found it.
        let input = |stream: &Slots<'_>, journal: &Journal, slot: i8| -> Result<Slot, Outcome> {
            let k = cursor + i64::from(slot);
            usize::try_from(k)
                .ok()
                .and_then(|k| journal.input(stream, k))
                .ok_or(Outcome::Fault(Fault::SlotOutOfRange))
        };
        // The stream index an operand names, where its input slot is still
        // there.
        let live = |journal: &Journal, value: i64| -> Result<usize, Outcome> {
            usize::try_from(cursor + value)
                .ok()
                .and_then(|k| journal.live(k))
                .ok_or(Outcome::Fault(Fault::SlotOutOfRange))
        };
        let mut pop = || stack.pop().ok_or(Outcome::Fault(Fault::StackUnderflow));
        let classes = self.face.description.classes();
        match op {
            Op::PutGlyph { class } => {
                let glyph = classes
                    .glyph(usize::from(class), 0)
                    .ok_or(Outcome::Fault(Fault::NoGlyphInClass(class)))?;
                journal.save(self, at);
                self.slot_mut(at)?.glyph = glyph;
            }
            Op::PutSubstitute {
                slot,
                input: from,
                output,
            } => {
                let source = input(self, journal, slot)?.glyph;
                let index = classes
                    .index(usize::from(from), source)
                    .ok_or(Outcome::Fault(Fault::NoGlyphInClass(from)))?;
                let glyph = classes
                    .glyph(usize::from(output), usize::from(index))
                    .ok_or(Outcome::Fault(Fault::NoGlyphInClass(output)))?;
                journal.save(self, at);
                self.slot_mut(at)?.glyph = glyph;
            }
            Op::PutCopy { slot } => {
                let source = input(self, journal, slot)?;
                journal.save(self, at);
                let target = self.slot_mut(at)?;
                let (id, first, last) = (target.id, target.first, target.last);
                *target = Slot {
                    id,
                    first,
                    last,
                    ..source
                };
            }
            Op::Associate { first, count } => {
                let mut span: Option<(u32, u32)> = None;
                for slot in program.associated(first, count) {
                    let named = input(self, journal, *slot)?;
                    if let Ok(k) = usize::try_from(cursor + i64::from(*slot)) {
                        journal.named.push(k);
                    }
                    span = Some(match span {
                        None => (named.first, named.last),
                        Some((low, high)) => (low.min(named.first), high.max(named.last)),
                    });
                }
                if let Some((low, high)) = span {
                    journal.save(self, at);
                    let target = self.slot_mut(at)?;
                    target.first = low;
                    target.last = high;
                }
            }
            Op::SetAttribute { attribute }
            | Op::AddAttribute { attribute }
            | Op::SubtractAttribute { attribute } => {
                let value = pop()?;
                let slot = self.slots.get(at).ok_or(out_of_range)?;
                let now = self
                    .value_of(slot, at, attribute, 0)
                    .map_err(Outcome::Refused)?;
                let value = match op {
                    Op::SetAttribute { .. } => value,
                    Op::AddAttribute { .. } => now.wrapping_add(value),
                    _ => now.wrapping_sub(value),
                };
                journal.save(self, at);
                self.slot_mut(at)?.store(attribute, 0, value);
            }
            Op::SetIndexedAttribute { attribute, index }
            | Op::AddIndexedAttribute { attribute, index }
            | Op::SubtractIndexedAttribute { attribute, index } => {
                let value = pop()?;
                let now = self
                    .slots
                    .get(at)
                    .and_then(|slot| slot.set_value(attribute, index))
                    .unwrap_or(0);
                let value = match op {
                    Op::SetIndexedAttribute { .. } => value,
                    Op::AddIndexedAttribute { .. } => now.wrapping_add(value),
                    _ => now.wrapping_sub(value),
                };
                journal.save(self, at);
                self.slot_mut(at)?.store(attribute, index, value);
            }
            Op::SetSlotReference { .. } | Op::SetIndexedSlotReference { .. } => {
                let value = pop()?;
                let target = live(journal, i64::from(value))?;
                let id = self.slots.get(target).ok_or(out_of_range)?.id;
                journal.save(self, at);
                let slot = self.slot_mut(at)?;
                match op {
                    Op::SetIndexedSlotReference { index, .. } => {
                        match slot.components.iter_mut().find(|(i, _)| *i == index) {
                            Some(entry) => entry.1 = id,
                            None => slot.components.push((index, id)),
                        }
                    }
                    _ => slot.attach_to = Some(id),
                }
            }
            Op::SetFeature { feature, slot } => {
                let value = pop()?;
                let target = live(journal, i64::from(slot))?;
                let ceiling = self
                    .face
                    .features
                    .map_or(1, |features| features.max_value(usize::from(feature)));
                let clipped = i16::try_from(value.min(i32::from(ceiling))).unwrap_or(i16::MIN);
                journal.save(self, target);
                let run = self.features.clone();
                let values = self.slot_mut(target)?.features.get_or_insert(run);
                if let Some(entry) = values.get_mut(usize::from(feature)) {
                    *entry = clipped;
                }
            }
            // Every other instruction is the caller's: control, structure
            // and computing.
            _ => return Err(out_of_range),
        }
        Ok(())
    }

    /// Slot `at`, to change.
    fn slot_mut(&mut self, at: usize) -> Result<&mut Slot, Outcome> {
        self.slots
            .get_mut(at)
            .ok_or(Outcome::Fault(Fault::SlotOutOfRange))
    }

    /// Inserts a slot at `at`: a copy of the slot after it, or of the one
    /// before it at the stream's end, attached to nothing and referencing
    /// no component.
    fn insert(&mut self, at: usize) {
        let model = self
            .slots
            .get(at)
            .or_else(|| at.checked_sub(1).and_then(|before| self.slots.get(before)));
        let mut slot = model.cloned().unwrap_or(Slot {
            glyph: 0,
            id: 0,
            first: 0,
            last: 0,
            values: Vec::new(),
            attach_to: None,
            components: Vec::new(),
            features: None,
        });
        slot.id = self.next_id();
        slot.attach_to = None;
        slot.components.clear();
        // `act` holds `at` at or before the stream's end.
        self.slots.insert(at.min(self.slots.len()), slot);
        self.reindex();
    }

    /// Deletes the slot at `at`.
    fn delete(&mut self, at: usize) {
        if at >= self.slots.len() {
            return;
        }
        self.slots.remove(at);
        self.reindex();
    }

    /// Gives the characters of every slot an action deleted, and no
    /// `Assoc` of it named, to the slot that stood before it, or after it
    /// at the start — the GDL's default for a deletion "which has not been
    /// associated". Where that slot went too, they follow it to its own.
    fn settle(&mut self, journal: &Journal) {
        for gone in &journal.deleted {
            if gone.input.is_some_and(|k| journal.named.contains(&k)) {
                continue;
            }
            let mut target = gone.neighbour;
            let mut steps = 0usize;
            while let Some(id) = target {
                if let Some(at) = self.find(id) {
                    if let Some(slot) = self.slots.get_mut(at) {
                        slot.first = slot.first.min(gone.first);
                        slot.last = slot.last.max(gone.last);
                    }
                    break;
                }
                steps += 1;
                if steps > journal.deleted.len() {
                    break;
                }
                target = journal
                    .deleted
                    .iter()
                    .find(|other| other.id == id)
                    .and_then(|other| other.neighbour);
            }
        }
    }
}

/// What stopped an instruction that changes a slot.
enum Outcome {
    /// A fault the machine names at the instruction.
    Fault(Fault),
    /// A refusal a read gave.
    Refused(Refusal),
}

/// A structural change an action made, in stream indices at the moment it
/// was made.
#[derive(Clone, Copy, Debug)]
enum Edit {
    /// A slot inserted at the index.
    Inserted(usize),
    /// The slot at the index deleted.
    Deleted(usize),
}

/// What an action has done to the stream so far, so that an operand
/// counting slots of the stream as the action found it can be followed.
#[derive(Clone, Debug)]
struct Journal {
    /// How many slots the stream held when the action began.
    input_len: usize,
    /// Its insertions and deletions, in order.
    edits: Vec<Edit>,
    /// Each input slot it changed, as it was before the change.
    saved: Vec<(usize, Slot)>,
    /// Each slot it deleted, in order.
    deleted: Vec<Deleted>,
    /// Each input slot an `Assoc` of it named.
    named: Vec<usize>,
}

/// A slot an action deleted, and where its characters go unless an
/// `Assoc` names it.
#[derive(Clone, Copy, Debug)]
struct Deleted {
    /// The input slot it was; `None` for one the action inserted.
    input: Option<usize>,
    /// Its id.
    id: u32,
    /// Its first character.
    first: u32,
    /// Its last.
    last: u32,
    /// The slot that stood before it, or after it at the start, by id.
    neighbour: Option<u32>,
}

impl Journal {
    const fn new(input_len: usize) -> Journal {
        Journal {
            input_len,
            edits: Vec::new(),
            saved: Vec::new(),
            deleted: Vec::new(),
            named: Vec::new(),
        }
    }

    /// The stream index input slot `k` stands at now; `None` once it is
    /// deleted, or for an index past the input.
    fn live(&self, k: usize) -> Option<usize> {
        if k >= self.input_len {
            return None;
        }
        let mut at = k;
        for edit in &self.edits {
            match *edit {
                Edit::Inserted(q) if at >= q => at += 1,
                Edit::Deleted(q) if at == q => return None,
                Edit::Deleted(q) if at > q => at -= 1,
                _ => {}
            }
        }
        Some(at)
    }

    /// The input slot the slot now at stream index `q` was; `None` for one
    /// the action inserted.
    fn origin(&self, q: usize) -> Option<usize> {
        let mut at = q;
        for edit in self.edits.iter().rev() {
            match *edit {
                Edit::Inserted(e) if at == e => return None,
                Edit::Inserted(e) if at > e => at -= 1,
                Edit::Deleted(e) if at >= e => at += 1,
                _ => {}
            }
        }
        Some(at)
    }

    /// The input slot the pointer's operands count from: the current
    /// slot's own, or, for a slot the action inserted, the one before the
    /// input slot after it. `None` where the pointer is before the stream.
    ///
    /// A pointer past the stream's last slot counts from past the input's
    /// last, and one before its first — where the action deleted the first
    /// slot — from before the input's first.
    fn cursor(&self, stream: &Slots<'_>, pointer: i64) -> Option<i64> {
        let Ok(start) = usize::try_from(pointer) else {
            return Some(pointer);
        };
        let input_len = i64::try_from(self.input_len).ok()?;
        if start >= stream.len() {
            return Some(input_len);
        }
        if let Some(k) = self.origin(start) {
            return i64::try_from(k).ok();
        }
        for q in start + 1..stream.len() {
            if let Some(k) = self.origin(q) {
                return i64::try_from(k).ok().map(|k| k - 1);
            }
        }
        Some(input_len - 1)
    }

    /// Saves the input slot now at stream index `at` before its first
    /// change; a slot the action inserted has no input to save.
    fn save(&mut self, stream: &Slots<'_>, at: usize) {
        let Some(k) = self.origin(at) else {
            return;
        };
        if self.saved.iter().any(|(saved, _)| *saved == k) {
            return;
        }
        if let Some(slot) = stream.slots.get(at) {
            self.saved.push((k, slot.clone()));
        }
    }

    /// Input slot `k` as the action found it.
    fn input(&self, stream: &Slots<'_>, k: usize) -> Option<Slot> {
        if let Some((_, slot)) = self.saved.iter().find(|(saved, _)| *saved == k) {
            return Some(slot.clone());
        }
        stream.slots.get(self.live(k)?).cloned()
    }
}

/// The stream as an action's computing instructions read it: slots counted
/// as the action found them, each read as it stands now, and one the action
/// deleted as it was found.
struct Window<'s, 'f> {
    stream: &'s Slots<'f>,
    journal: &'s Journal,
}

impl Window<'_, '_> {
    /// Input slot `k`, and the stream index it stands at now where it is
    /// still there.
    fn resolve(&self, k: usize) -> Option<(Slot, Option<usize>)> {
        match self.journal.live(k) {
            Some(at) => self
                .stream
                .slots
                .get(at)
                .map(|slot| (slot.clone(), Some(at))),
            None => self.journal.input(self.stream, k).map(|slot| (slot, None)),
        }
    }
}

impl Reads for Window<'_, '_> {
    fn slot_count(&self) -> usize {
        self.journal.input_len
    }

    fn slot_attribute(
        &self,
        slot: usize,
        attribute: SlotAttribute,
        index: u8,
    ) -> Result<i32, Refusal> {
        let Some((found, at)) = self.resolve(slot) else {
            return Ok(0);
        };
        match at {
            Some(at) => self.stream.value_of(&found, at, attribute, index),
            // A deleted slot references nothing that is still there.
            None => {
                if attribute.is_slot_reference() {
                    Ok(0)
                } else {
                    self.stream.value_of(&found, usize::MAX, attribute, index)
                }
            }
        }
    }

    fn glyph_attribute(&self, slot: usize, attribute: u16) -> Result<i32, Refusal> {
        let Some((found, _)) = self.resolve(slot) else {
            return Ok(0);
        };
        self.stream.face.glyph_attribute(found.glyph, attribute)
    }

    fn metric(&self, slot: usize, metric: Metric, level: u8) -> Result<i32, Refusal> {
        let Some((found, _)) = self.resolve(slot) else {
            return Ok(0);
        };
        if level != 0 {
            return Err(Refusal::PositionNotResolved);
        }
        let Metric::BoundingBoxTop = metric;
        self.stream.top_of(found.glyph)
    }

    fn feature(&self, slot: usize, feature: u8) -> i32 {
        self.resolve(slot)
            .map_or(0, |(found, _)| self.stream.feature_of(&found, feature))
    }

    fn attached_to(&self, slot: usize) -> Option<usize> {
        let (found, _) = self.resolve(slot)?;
        let parent = self.stream.find(found.attach_to?)?;
        self.journal.origin(parent)
    }
}

impl Reads for Slots<'_> {
    fn slot_count(&self) -> usize {
        self.slots.len()
    }

    fn slot_attribute(
        &self,
        slot: usize,
        attribute: SlotAttribute,
        index: u8,
    ) -> Result<i32, Refusal> {
        match self.slots.get(slot) {
            Some(found) => self.value_of(found, slot, attribute, index),
            None => Ok(0),
        }
    }

    fn glyph_attribute(&self, slot: usize, attribute: u16) -> Result<i32, Refusal> {
        match self.slots.get(slot) {
            Some(found) => self.face.glyph_attribute(found.glyph, attribute),
            None => Ok(0),
        }
    }

    fn metric(&self, slot: usize, metric: Metric, level: u8) -> Result<i32, Refusal> {
        let Some(found) = self.slots.get(slot) else {
            return Ok(0);
        };
        if level != 0 {
            return Err(Refusal::PositionNotResolved);
        }
        let Metric::BoundingBoxTop = metric;
        self.top_of(found.glyph)
    }

    fn feature(&self, slot: usize, feature: u8) -> i32 {
        self.slots
            .get(slot)
            .map_or(0, |found| self.feature_of(found, feature))
    }

    fn attached_to(&self, slot: usize) -> Option<usize> {
        self.find(self.slots.get(slot)?.attach_to?)
    }
}

#[cfg(test)]
mod tests {
    use super::{Edit, Journal};

    #[test]
    fn the_journal_follows_input_slots_through_its_edits() {
        // Five input slots; a slot inserted at 0, then input 2 (now at 3)
        // deleted.
        let journal = Journal {
            input_len: 5,
            edits: vec![Edit::Inserted(0), Edit::Deleted(3)],
            saved: Vec::new(),
            deleted: Vec::new(),
            named: Vec::new(),
        };
        assert_eq!(journal.live(0), Some(1));
        assert_eq!(journal.live(1), Some(2));
        assert_eq!(journal.live(2), None);
        assert_eq!(journal.live(3), Some(3));
        assert_eq!(journal.live(4), Some(4));
        assert_eq!(journal.live(5), None);
        assert_eq!(journal.origin(0), None);
        assert_eq!(journal.origin(1), Some(0));
        assert_eq!(journal.origin(3), Some(3));
        assert_eq!(journal.origin(4), Some(4));
    }
}
