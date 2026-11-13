//! The stack machine: the code a rule's constraint and action are, checked
//! before it ever runs, and run.
//!
//! `docs/design/shaping.md`, *The stack machine*, *A rule's constraint* and
//! *The numbering an instruction names*, is the reading this follows: the
//! GTF appendix's command table and its operands, and the GDL's meaning of
//! each attribute an instruction names.
//!
//! # Checked once, before it runs
//!
//! Code comes from the face, so every check is a refusal and never a panic
//! (ruling 1). [`check`] decodes a block into a [`Program`] or names the
//! instruction it refused and why ([`Fault`]): an opcode the table does not
//! list or marks unimplemented, operands past the block, a `ContextItem`
//! whose skip lands inside an instruction or whose block is not one test, an
//! instruction of the wrong kind for its block, a stack that would fall below
//! empty or rise past [`Limits::max_stack_depth`], and an operand out of range
//! — a class, a feature, a user attribute, or an attribute or metric number no
//! vendored face's code reveals. Nothing that fails its check ever runs, so a
//! rule whose constraint or action fails it never fires.
//!
//! # Run under a budget
//!
//! A program cannot loop — no instruction jumps backwards, and a
//! `ContextItem` only skips forward — but a run of text runs a great many of
//! them, so every instruction executed is charged against
//! [`Limits::max_instructions`] through a [`Budget`] the whole run shares.
//! Once it is spent the run's remaining code is refused by name
//! ([`Refusal::TooManyInstructions`]). Values are `i32`; arithmetic wraps, as
//! two's complement does on every target, and a division by zero, or of
//! `i32::MIN` by −1, ends the code and is named.

use crate::limits::Limits;

use super::Refusal;

/// The stack-machine language version [`Op::PushVersion`] pushes: `Silf` 5's
/// `ruleVersion`, the latest the GTF defines and the one this machine runs.
pub const ENGINE_VERSION: i32 = 0x0005_0000;

/// Which block of a pass a piece of code is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Block {
    /// The pass constraint, run once a pass with no slot under test.
    PassConstraint,
    /// The constraint of the rule numbered.
    Constraint(u16),
    /// The action of the rule numbered.
    Action(u16),
}

impl Block {
    /// Whether the block is an action rather than a constraint.
    #[must_use]
    pub const fn is_action(self) -> bool {
        matches!(self, Block::Action(_))
    }
}

/// Where a block of code is: its pass and which block of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Site {
    /// The pass's number in its description.
    pub pass: u8,
    /// The block.
    pub block: Block,
}

/// What a check found wrong with a block of code, or what stopped one
/// running.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Fault {
    /// A byte that is no opcode the GTF's table lists.
    IllegalOpcode(u8),
    /// An opcode the table marks not implemented: `NextN`, `PushIGlyphAttr`,
    /// `PushProcState`, `PutSubs2` and `PutSubs3`.
    Unimplemented(u8),
    /// An instruction whose operands run past the end of its block.
    Truncated,
    /// A `ContextItem` whose skip lands inside an instruction, past its
    /// block, or past the end of a `ContextItem` block around it.
    SkipOffBoundary,
    /// A `ContextItem`'s block that does not leave exactly one value on the
    /// stack: it is not the one test the skip stands in for.
    NotOneTest,
    /// A `ContextItem` in an action, where no slot is under test.
    ContextItemInAction,
    /// An instruction that acts on the slot stream, in a constraint.
    ActionInConstraint(u8),
    /// A pop from an empty stack.
    StackUnderflow,
    /// A push past [`Limits::max_stack_depth`].
    StackOverflow,
    /// A slot attribute whose number no vendored face's code reveals.
    UnknownSlotAttribute(u8),
    /// A glyph metric whose number no vendored face's code reveals.
    UnknownMetric(u8),
    /// An attribute an instruction cannot take: an indexed one where an
    /// instruction takes none, or the reverse; a read-only one set; a number
    /// set where a slot reference is, or the reverse.
    AttributeMisused(u8),
    /// A negative attachment level or attribute index.
    OperandOutOfRange,
    /// A class past the description's class map.
    ClassOutOfRange(u16),
    /// A feature past the face's `Feat`.
    FeatureOutOfRange(u8),
    /// A user attribute past the description's count of them.
    UserAttributeOutOfRange(u8),
    /// A division by zero.
    DivisionByZero,
    /// `i32::MIN` divided by −1, whose quotient `i32` cannot hold.
    DivisionOverflow,
    /// A slot offset reaching before the stream's start or past its end,
    /// or an action's pointer where no slot is.
    SlotOutOfRange,
    /// An insertion past [`Limits::max_glyphs`].
    TooManySlots,
    /// A class that gives no glyph: the input glyph is not in the input
    /// class, or the output class holds no glyph at its index.
    NoGlyphInClass(u16),
    /// An instruction a positioning pass may not run — `Insert`, `Delete`,
    /// or a `PutCopy` of another slot — because a positioning pass makes
    /// "only a one-to-one substitution".
    NotOneToOne(u8),
}

/// A slot attribute an instruction reads or sets, by the number compiled
/// code gives it.
///
/// The GTF and the GDL name the attributes and number none, so each number
/// here was read off a vendored face's compiled code against the GDL source
/// its authors publish beside it (`docs/design/shaping.md`, *The numbering an
/// instruction names*), the rule each was read from on its variant, and
/// `tests/graphite.rs` holds each to that face's bytes. A number no vendored
/// code reveals is refused by name ([`Fault::UnknownSlotAttribute`]) rather
/// than guessed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SlotAttribute {
    /// `advance.x`, 0: Awami Nastaliq's pass 7, rule 0, reads it for
    /// `seqWidth = @P.advance.x + @P.seqWidth`; pass 19, rule 16, adds to it.
    AdvanceX,
    /// `attach.to`, 2, a slot reference: set by Awami Nastaliq's pass 7,
    /// rule 0, through `AttrSetSlot`, and read by pass 14, rule 0's
    /// `{att.to==0}`.
    AttachTo,
    /// `attach.at.x`, 3: Awami Nastaliq's pass 7, rule 0, sets it from the
    /// point on the glyph attached to (`PushAttToGlyphAttr`).
    AttachAtX,
    /// `attach.at.y`, 4: the same rule.
    AttachAtY,
    /// `attach.with.x`, 8: the same rule, from the point on the slot's own
    /// glyph.
    AttachWithX,
    /// `attach.with.y`, 9: the same rule.
    AttachWithY,
    /// `breakweight`, 14: Dai Banna SIL's line-break pass 0, rule 0.
    BreakWeight,
    /// `component.X.reference`, 15, indexed and a slot reference: Tagmukay's
    /// pass 0, rule 0, through `IAttrSetSlot`.
    ComponentReference,
    /// `directionality`, 16: Awami Nastaliq's pass 6, rule 24,
    /// `_ {directionality == 1}`.
    Directionality,
    /// `insert`, 17: Awami Nastaliq's pass 7, rule 0, clears it on the slot
    /// it attaches and sets it on the slot after (`{insert=1}`).
    Insert,
    /// `position.y`, 19, read only: Awami Nastaliq's pass 11, rule 31, and
    /// pass 12, rule 22, `{position.y + bb.top > SHORTFINAL2}`.
    PositionY,
    /// `shift.x`, 20: Awami Nastaliq's pass 17, rule 17,
    /// `shift { x = 300m; y = 350m; }`.
    ShiftX,
    /// `shift.y`, 21: the same rule.
    ShiftY,
    /// `userX`, 55, indexed by X − 1: Awami Nastaliq's pass 7, rule 0, sets
    /// `seqWidth`, its `user5`, as index 4.
    User,
    /// `collision.flags`, 57: Awami Nastaliq's pass 17, rule 17,
    /// `flags = CLEAR_IGNORE(collision.flags)`.
    CollisionFlags,
    /// `collision.min.x`, 58: the same rule's `min.x = -200m`.
    CollisionMinX,
    /// `collision.min.y`, 59: its `min.y = -300m`.
    CollisionMinY,
    /// `collision.max.x`, 60: its `max.x = 250m`.
    CollisionMaxX,
    /// `collision.max.y`, 61: its `max.y = 200m`.
    CollisionMaxY,
    /// `collision.fix.x`, 62, read only: Awami Nastaliq's pass 19, rule 16,
    /// `advance.x += max(0, @N.collision.fix.x) + 300m`.
    CollisionFixX,
    /// `collision.margin`, 64: Awami Nastaliq's pass 17, rule 69,
    /// `margin = 300m; marginweight = 500`.
    CollisionMargin,
    /// `collision.marginweight`, 65: the same rule.
    CollisionMarginWeight,
    /// `collision.exclude.glyph`, 66: Awami Nastaliq's pass 9, rule 73,
    /// `exclude.glyph = collexclude.glyph`.
    CollisionExcludeGlyph,
    /// `collision.exclude.offset.x`, 67: the same rule.
    CollisionExcludeOffsetX,
    /// `collision.exclude.offset.y`, 68: the same rule.
    CollisionExcludeOffsetY,
    /// `sequence.class`, 69: Awami Nastaliq's pass 17, rule 17,
    /// `sequence { class = ORDER_UDIACS; proxClass = ORDER_UNUQTAS; order =
    /// ORDER_NOBELOW; }`.
    SequenceClass,
    /// `sequence.proxClass`, 70: the same rule.
    SequenceProxClass,
    /// `sequence.order`, 71: the same rule.
    SequenceOrder,
}

impl SlotAttribute {
    /// Every attribute, in number order.
    pub const ALL: [SlotAttribute; 28] = [
        SlotAttribute::AdvanceX,
        SlotAttribute::AttachTo,
        SlotAttribute::AttachAtX,
        SlotAttribute::AttachAtY,
        SlotAttribute::AttachWithX,
        SlotAttribute::AttachWithY,
        SlotAttribute::BreakWeight,
        SlotAttribute::ComponentReference,
        SlotAttribute::Directionality,
        SlotAttribute::Insert,
        SlotAttribute::PositionY,
        SlotAttribute::ShiftX,
        SlotAttribute::ShiftY,
        SlotAttribute::User,
        SlotAttribute::CollisionFlags,
        SlotAttribute::CollisionMinX,
        SlotAttribute::CollisionMinY,
        SlotAttribute::CollisionMaxX,
        SlotAttribute::CollisionMaxY,
        SlotAttribute::CollisionFixX,
        SlotAttribute::CollisionMargin,
        SlotAttribute::CollisionMarginWeight,
        SlotAttribute::CollisionExcludeGlyph,
        SlotAttribute::CollisionExcludeOffsetX,
        SlotAttribute::CollisionExcludeOffsetY,
        SlotAttribute::SequenceClass,
        SlotAttribute::SequenceProxClass,
        SlotAttribute::SequenceOrder,
    ];

    /// The number compiled code gives the attribute.
    #[must_use]
    pub const fn number(self) -> u8 {
        match self {
            SlotAttribute::AdvanceX => 0,
            SlotAttribute::AttachTo => 2,
            SlotAttribute::AttachAtX => 3,
            SlotAttribute::AttachAtY => 4,
            SlotAttribute::AttachWithX => 8,
            SlotAttribute::AttachWithY => 9,
            SlotAttribute::BreakWeight => 14,
            SlotAttribute::ComponentReference => 15,
            SlotAttribute::Directionality => 16,
            SlotAttribute::Insert => 17,
            SlotAttribute::PositionY => 19,
            SlotAttribute::ShiftX => 20,
            SlotAttribute::ShiftY => 21,
            SlotAttribute::User => 55,
            SlotAttribute::CollisionFlags => 57,
            SlotAttribute::CollisionMinX => 58,
            SlotAttribute::CollisionMinY => 59,
            SlotAttribute::CollisionMaxX => 60,
            SlotAttribute::CollisionMaxY => 61,
            SlotAttribute::CollisionFixX => 62,
            SlotAttribute::CollisionMargin => 64,
            SlotAttribute::CollisionMarginWeight => 65,
            SlotAttribute::CollisionExcludeGlyph => 66,
            SlotAttribute::CollisionExcludeOffsetX => 67,
            SlotAttribute::CollisionExcludeOffsetY => 68,
            SlotAttribute::SequenceClass => 69,
            SlotAttribute::SequenceProxClass => 70,
            SlotAttribute::SequenceOrder => 71,
        }
    }

    /// The attribute numbered `number`, where vendored code reveals it.
    #[must_use]
    pub fn from_number(number: u8) -> Option<SlotAttribute> {
        SlotAttribute::ALL
            .iter()
            .copied()
            .find(|attribute| attribute.number() == number)
    }

    /// Whether an instruction names it with an index: a ligature component's
    /// reference, or a user attribute.
    #[must_use]
    pub const fn is_indexed(self) -> bool {
        matches!(
            self,
            SlotAttribute::ComponentReference | SlotAttribute::User
        )
    }

    /// Whether its value is a slot reference, set by `AttrSetSlot` or
    /// `IAttrSetSlot` and never by a number.
    #[must_use]
    pub const fn is_slot_reference(self) -> bool {
        matches!(
            self,
            SlotAttribute::AttachTo | SlotAttribute::ComponentReference
        )
    }

    /// Whether code may only read it: a slot's position, and the shift
    /// collision fixing gave it.
    #[must_use]
    pub const fn is_read_only(self) -> bool {
        matches!(
            self,
            SlotAttribute::PositionY | SlotAttribute::CollisionFixX
        )
    }
}

/// A glyph metric an instruction reads, by the number compiled code gives
/// it; numbered as [`SlotAttribute`]s are, a number no vendored code
/// reveals refused by name ([`Fault::UnknownMetric`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Metric {
    /// `bb.top`, the top of the glyph's bounding box, 2: Awami Nastaliq's
    /// pass 12, rule 22, `{position.y + bb.top > SHORTFINAL2}`.
    BoundingBoxTop,
}

impl Metric {
    /// The number compiled code gives the metric.
    #[must_use]
    pub const fn number(self) -> u8 {
        match self {
            Metric::BoundingBoxTop => 2,
        }
    }

    /// The metric numbered `number`, where vendored code reveals it.
    #[must_use]
    pub const fn from_number(number: u8) -> Option<Metric> {
        match number {
            2 => Some(Metric::BoundingBoxTop),
            _ => None,
        }
    }
}

/// One instruction, decoded: the GTF appendix's command, its operands read.
///
/// A slot operand is an offset from the slot the instruction acts upon: in a
/// constraint the slot under test, in an action the current slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// `Nop`.
    Nop,
    /// `PushByte`, `PushByteU`, `PushShort`, `PushShortU` and `PushLong`:
    /// the value, sign- or zero-extended as the opcode reads it.
    Push(i32),
    /// `Add`.
    Add,
    /// `Sub`: the top from the next.
    Sub,
    /// `Mul`.
    Mul,
    /// `Div`: the next by the top, truncated toward zero.
    Div,
    /// `Min`.
    Min,
    /// `Max`.
    Max,
    /// `Neg`.
    Neg,
    /// `Trunc8`: the low eight bits.
    Trunc8,
    /// `Trunc16`: the low sixteen bits.
    Trunc16,
    /// `Cond`: of three popped, the second where the first (the top) is
    /// true, and the third where it is false.
    Cond,
    /// `And`, logical.
    And,
    /// `Or`, logical.
    Or,
    /// `Not`, logical.
    Not,
    /// `Equal`.
    Equal,
    /// `NotEqu`.
    NotEqual,
    /// `Less`: the next less than the top.
    Less,
    /// `Gtr`.
    Greater,
    /// `LessEq`.
    LessOrEqual,
    /// `GtrEq`.
    GreaterOrEqual,
    /// `BitAnd`.
    BitAnd,
    /// `BitOr`.
    BitOr,
    /// `BitNot`.
    BitNot,
    /// `SetBits`: the mask's bits cleared and the value's set, both its
    /// signed sixteen-bit operands.
    SetBits {
        /// The bits to clear.
        mask: i32,
        /// The bits to set.
        value: i32,
    },
    /// `PushVersion`: [`ENGINE_VERSION`].
    PushVersion,
    /// `PushSlotAttr`.
    PushSlotAttribute {
        /// The attribute.
        attribute: SlotAttribute,
        /// The slot's offset.
        slot: i8,
    },
    /// `PushISlotAttr`.
    PushIndexedSlotAttribute {
        /// The attribute.
        attribute: SlotAttribute,
        /// The slot's offset.
        slot: i8,
        /// The index.
        index: u8,
    },
    /// `PushGlyphAttr`, in either width.
    PushGlyphAttribute {
        /// The glyph attribute.
        attribute: u16,
        /// The slot's offset.
        slot: i8,
    },
    /// `PushGlyphMetric`.
    PushGlyphMetric {
        /// The metric.
        metric: Metric,
        /// The slot's offset.
        slot: i8,
        /// The attachment level of the cluster it measures; 0 the glyph
        /// alone.
        level: u8,
    },
    /// `PushFeat`: the value of the feature numbered in `Feat`'s order.
    PushFeature {
        /// The feature.
        feature: u8,
        /// The slot's offset.
        slot: i8,
    },
    /// `PushAttToGlyphAttr`, in either width: the attribute of the glyph the
    /// slot is attached to, 0 where it is attached to none.
    PushAttachedGlyphAttribute {
        /// The glyph attribute.
        attribute: u16,
        /// The slot's offset.
        slot: i8,
    },
    /// `PushAttToGlyphMetric`.
    PushAttachedGlyphMetric {
        /// The metric.
        metric: Metric,
        /// The slot's offset.
        slot: i8,
        /// The attachment level.
        level: u8,
    },
    /// `ContextItem`: the instructions up to `end` run only where the slot
    /// under test is the one at `slot` from the scan position; elsewhere
    /// they are skipped and a true stands in their place.
    ContextItem {
        /// The slot's offset from the scan position.
        slot: i8,
        /// The index of the instruction after the block.
        end: usize,
    },
    /// `PopRet`.
    PopReturn,
    /// `RetZero`.
    ReturnZero,
    /// `RetTrue`.
    ReturnTrue,
    /// `Next`: the current slot pointer moved forward one slot.
    Next,
    /// `CopyNext`: the current slot copied to the output and the pointer
    /// moved on.
    CopyNext,
    /// `PutGlyph`, in either width: the first glyph of the class.
    PutGlyph {
        /// The output class.
        class: u16,
    },
    /// `PutSubs`, in either width.
    PutSubstitute {
        /// The input slot's offset.
        slot: i8,
        /// The class the input glyph's index is found in.
        input: u16,
        /// The class the glyph at that index is put from.
        output: u16,
    },
    /// `PutCopy`: the glyph the input had at the slot, copied.
    PutCopy {
        /// The input slot's offset.
        slot: i8,
    },
    /// `Insert`: a new slot before the current one, made current.
    Insert,
    /// `Delete`: the current slot deleted.
    Delete,
    /// `Assoc`: the current slot associated with the input slots its
    /// operands name, held in [`Program::associations`] from `first`.
    Associate {
        /// Where the offsets start in the program's list of them.
        first: u32,
        /// How many.
        count: u8,
    },
    /// `AttrSet`.
    SetAttribute {
        /// The attribute.
        attribute: SlotAttribute,
    },
    /// `AttrAdd`.
    AddAttribute {
        /// The attribute.
        attribute: SlotAttribute,
    },
    /// `AttrSub`.
    SubtractAttribute {
        /// The attribute.
        attribute: SlotAttribute,
    },
    /// `AttrSetSlot`: a slot reference, relative to the current stream
    /// position.
    SetSlotReference {
        /// The attribute.
        attribute: SlotAttribute,
    },
    /// `IAttrSetSlot`.
    SetIndexedSlotReference {
        /// The attribute.
        attribute: SlotAttribute,
        /// The index.
        index: u8,
    },
    /// `IAttrSet`.
    SetIndexedAttribute {
        /// The attribute.
        attribute: SlotAttribute,
        /// The index.
        index: u8,
    },
    /// `IAttrAdd`.
    AddIndexedAttribute {
        /// The attribute.
        attribute: SlotAttribute,
        /// The index.
        index: u8,
    },
    /// `IAttrSub`.
    SubtractIndexedAttribute {
        /// The attribute.
        attribute: SlotAttribute,
        /// The index.
        index: u8,
    },
    /// `SetFeat`: the feature of the slot at the offset, clipped to the
    /// largest value it declares.
    SetFeature {
        /// The feature.
        feature: u8,
        /// The slot's offset.
        slot: i8,
    },
}

impl Op {
    /// How many values the instruction pops and pushes; a `ContextItem`'s
    /// block is counted by its own instructions.
    const fn stack(self) -> (usize, usize) {
        match self {
            Op::Nop
            | Op::ContextItem { .. }
            | Op::ReturnZero
            | Op::ReturnTrue
            | Op::Next
            | Op::CopyNext
            | Op::PutGlyph { .. }
            | Op::PutSubstitute { .. }
            | Op::PutCopy { .. }
            | Op::Insert
            | Op::Delete
            | Op::Associate { .. } => (0, 0),
            Op::Push(_)
            | Op::PushVersion
            | Op::PushSlotAttribute { .. }
            | Op::PushIndexedSlotAttribute { .. }
            | Op::PushGlyphAttribute { .. }
            | Op::PushGlyphMetric { .. }
            | Op::PushFeature { .. }
            | Op::PushAttachedGlyphAttribute { .. }
            | Op::PushAttachedGlyphMetric { .. } => (0, 1),
            Op::Add
            | Op::Sub
            | Op::Mul
            | Op::Div
            | Op::Min
            | Op::Max
            | Op::And
            | Op::Or
            | Op::Equal
            | Op::NotEqual
            | Op::Less
            | Op::Greater
            | Op::LessOrEqual
            | Op::GreaterOrEqual
            | Op::BitAnd
            | Op::BitOr => (2, 1),
            Op::Neg | Op::Trunc8 | Op::Trunc16 | Op::Not | Op::BitNot | Op::SetBits { .. } => {
                (1, 1)
            }
            Op::Cond => (3, 1),
            Op::PopReturn
            | Op::SetAttribute { .. }
            | Op::AddAttribute { .. }
            | Op::SubtractAttribute { .. }
            | Op::SetSlotReference { .. }
            | Op::SetIndexedSlotReference { .. }
            | Op::SetIndexedAttribute { .. }
            | Op::AddIndexedAttribute { .. }
            | Op::SubtractIndexedAttribute { .. }
            | Op::SetFeature { .. } => (1, 0),
        }
    }

    /// Whether the instruction acts on the slot stream, as only an action
    /// may.
    #[must_use]
    pub const fn acts(self) -> bool {
        matches!(
            self,
            Op::Next
                | Op::CopyNext
                | Op::PutGlyph { .. }
                | Op::PutSubstitute { .. }
                | Op::PutCopy { .. }
                | Op::Insert
                | Op::Delete
                | Op::Associate { .. }
                | Op::SetAttribute { .. }
                | Op::AddAttribute { .. }
                | Op::SubtractAttribute { .. }
                | Op::SetSlotReference { .. }
                | Op::SetIndexedSlotReference { .. }
                | Op::SetIndexedAttribute { .. }
                | Op::AddIndexedAttribute { .. }
                | Op::SubtractIndexedAttribute { .. }
                | Op::SetFeature { .. }
        )
    }
}

/// One decoded instruction and where it stood in its block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Instruction {
    /// Its byte offset in the block: what a refusal names.
    pub at: u16,
    /// The opcode it was read from.
    pub opcode: u8,
    /// The instruction.
    pub op: Op,
}

/// What the operands of a block are checked against: the description's and
/// the face's counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Bounds {
    /// How many classes the description's class map holds.
    pub classes: usize,
    /// How many features the face's `Feat` declares.
    pub features: usize,
    /// How many user attributes the description counts.
    pub user_attributes: u8,
    /// Whether the block is a positioning pass's.
    pub positioning: bool,
}

impl Bounds {
    /// The bounds `description`'s code is checked against, in a face whose
    /// `Feat` declares `features` features, for a pass before its
    /// positioning passes.
    #[must_use]
    pub fn of(description: &super::Description<'_>, features: usize) -> Bounds {
        Bounds {
            classes: description.classes().len(),
            features,
            user_attributes: description.user_attributes,
            positioning: false,
        }
    }

    /// The same, for pass `pass` of the description: a positioning pass
    /// from `iPos` on.
    #[must_use]
    pub fn for_pass(description: &super::Description<'_>, features: usize, pass: usize) -> Bounds {
        Bounds {
            positioning: pass >= usize::from(description.positioning),
            ..Bounds::of(description, features)
        }
    }
}

/// A block of code that passed its check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    site: Site,
    code: Vec<Instruction>,
    associations: Vec<i8>,
}

impl Program {
    /// Where the block is.
    #[must_use]
    pub const fn site(&self) -> Site {
        self.site
    }

    /// Its instructions, in order.
    #[must_use]
    pub fn instructions(&self) -> &[Instruction] {
        &self.code
    }

    /// The slot offsets of every `Assoc` in it, in order: an
    /// [`Op::Associate`] names its own run of them.
    #[must_use]
    pub fn associations(&self) -> &[i8] {
        &self.associations
    }

    /// The offsets an [`Op::Associate`] names.
    #[must_use]
    pub fn associated(&self, first: u32, count: u8) -> &[i8] {
        let start = usize::try_from(first).unwrap_or(usize::MAX);
        let end = start.saturating_add(usize::from(count));
        self.associations.get(start..end).unwrap_or(&[])
    }

    /// Whether the block has no instructions.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.code.is_empty()
    }
}

/// Decodes and checks a block of code from `site`, its operands held to
/// `bounds` and its stack to `limits`.
///
/// # Errors
///
/// [`Refusal::CodeRefused`] naming the site, the byte offset of the
/// instruction refused, and the [`Fault`]; the first fault met in the
/// block's order is the one named.
pub fn check(
    code: &[u8],
    site: Site,
    bounds: &Bounds,
    limits: &Limits,
) -> Result<Program, Refusal> {
    let refuse = |at: usize, fault: Fault| Refusal::CodeRefused {
        pass: site.pass,
        block: site.block,
        at: u16::try_from(at).unwrap_or(u16::MAX),
        fault,
    };
    let mut decoded: Vec<(usize, Op, Option<usize>)> = Vec::new();
    let mut associations: Vec<i8> = Vec::new();
    let mut at = 0usize;
    while at < code.len() {
        let (op, length, skip) =
            decode(code, at, bounds, &mut associations).map_err(|fault| refuse(at, fault))?;
        if site.block.is_action() {
            if matches!(op, Op::ContextItem { .. }) {
                return Err(refuse(at, Fault::ContextItemInAction));
            }
            let reorders = matches!(op, Op::Insert | Op::Delete)
                || matches!(op, Op::PutCopy { slot } if slot != 0);
            if bounds.positioning && reorders {
                let opcode = code.get(at).copied().unwrap_or(0);
                return Err(refuse(at, Fault::NotOneToOne(opcode)));
            }
        } else if op.acts() {
            let opcode = code.get(at).copied().unwrap_or(0);
            return Err(refuse(at, Fault::ActionInConstraint(opcode)));
        }
        decoded.push((at, op, skip));
        at += length;
    }
    // Each `ContextItem`'s skip, from a byte count to the instruction it
    // lands on: a boundary inside the block, or the block's end.
    let boundary = |byte: usize| -> Option<usize> {
        if byte == code.len() {
            return Some(decoded.len());
        }
        decoded
            .binary_search_by(|(start, ..)| start.cmp(&byte))
            .ok()
    };
    let mut instructions = Vec::with_capacity(decoded.len());
    for (start, op, skip) in &decoded {
        let op = match (*op, *skip) {
            (Op::ContextItem { slot, .. }, Some(target)) => Op::ContextItem {
                slot,
                end: boundary(target).ok_or_else(|| refuse(*start, Fault::SkipOffBoundary))?,
            },
            (op, _) => op,
        };
        instructions.push(Instruction {
            at: u16::try_from(*start).unwrap_or(u16::MAX),
            opcode: code.get(*start).copied().unwrap_or(0),
            op,
        });
    }
    depth(&instructions, site, limits).map_err(|(index, fault)| {
        let at = instructions
            .get(index)
            .map_or(code.len(), |instruction| usize::from(instruction.at));
        refuse(at, fault)
    })?;
    Ok(Program {
        site,
        code: instructions,
        associations,
    })
}

/// The instruction at `at` of `code`: its op, its length in bytes, and for
/// a `ContextItem` the byte its skip lands on.
fn decode(
    code: &[u8],
    at: usize,
    bounds: &Bounds,
    associations: &mut Vec<i8>,
) -> Result<(Op, usize, Option<usize>), Fault> {
    let byte = |n: usize| -> Result<u8, Fault> {
        code.get(at.checked_add(n).ok_or(Fault::Truncated)?)
            .copied()
            .ok_or(Fault::Truncated)
    };
    let signed = |n: usize| -> Result<i8, Fault> { Ok(i8::from_ne_bytes([byte(n)?])) };
    let short =
        |n: usize| -> Result<u16, Fault> { Ok(u16::from_be_bytes([byte(n)?, byte(n + 1)?])) };
    let class = |class: u16| -> Result<u16, Fault> {
        if usize::from(class) < bounds.classes {
            Ok(class)
        } else {
            Err(Fault::ClassOutOfRange(class))
        }
    };
    let feature = |feature: u8| -> Result<u8, Fault> {
        if usize::from(feature) < bounds.features {
            Ok(feature)
        } else {
            Err(Fault::FeatureOutOfRange(feature))
        }
    };
    let attribute = |number: u8| -> Result<SlotAttribute, Fault> {
        SlotAttribute::from_number(number).ok_or(Fault::UnknownSlotAttribute(number))
    };
    let metric = |number: u8| -> Result<Metric, Fault> {
        Metric::from_number(number).ok_or(Fault::UnknownMetric(number))
    };
    let level = |value: i8| -> Result<u8, Fault> {
        u8::try_from(value).map_err(|_| Fault::OperandOutOfRange)
    };
    // A plain attribute an instruction reads, or sets where `set`: neither
    // indexed nor, where it is set, read only or a slot reference.
    let plain = |number: u8, set: bool| -> Result<SlotAttribute, Fault> {
        let found = attribute(number)?;
        if found.is_indexed() || (set && (found.is_read_only() || found.is_slot_reference())) {
            return Err(Fault::AttributeMisused(number));
        }
        Ok(found)
    };
    // An indexed attribute, its index held to the description's count of
    // user attributes where it is one.
    let indexed =
        |number: u8, index: u8, reference: Option<bool>| -> Result<SlotAttribute, Fault> {
            let found = attribute(number)?;
            if !found.is_indexed()
                || reference.is_some_and(|wanted| wanted != found.is_slot_reference())
            {
                return Err(Fault::AttributeMisused(number));
            }
            if found == SlotAttribute::User && index >= bounds.user_attributes {
                return Err(Fault::UserAttributeOutOfRange(index));
            }
            Ok(found)
        };
    let opcode = byte(0)?;
    let op = match opcode {
        0x00 => (Op::Nop, 1),
        0x01 => (Op::Push(i32::from(signed(1)?)), 2),
        0x02 => (Op::Push(i32::from(byte(1)?)), 2),
        0x03 => (
            Op::Push(i32::from(i16::from_be_bytes(short(1)?.to_be_bytes()))),
            3,
        ),
        0x04 => (Op::Push(i32::from(short(1)?)), 3),
        0x05 => (
            Op::Push(i32::from_be_bytes([byte(1)?, byte(2)?, byte(3)?, byte(4)?])),
            5,
        ),
        0x06 => (Op::Add, 1),
        0x07 => (Op::Sub, 1),
        0x08 => (Op::Mul, 1),
        0x09 => (Op::Div, 1),
        0x0A => (Op::Min, 1),
        0x0B => (Op::Max, 1),
        0x0C => (Op::Neg, 1),
        0x0D => (Op::Trunc8, 1),
        0x0E => (Op::Trunc16, 1),
        0x0F => (Op::Cond, 1),
        0x10 => (Op::And, 1),
        0x11 => (Op::Or, 1),
        0x12 => (Op::Not, 1),
        0x13 => (Op::Equal, 1),
        0x14 => (Op::NotEqual, 1),
        0x15 => (Op::Less, 1),
        0x16 => (Op::Greater, 1),
        0x17 => (Op::LessOrEqual, 1),
        0x18 => (Op::GreaterOrEqual, 1),
        0x19 => (Op::Next, 1),
        0x1B => (Op::CopyNext, 1),
        0x1C => (
            Op::PutGlyph {
                class: class(u16::from(byte(1)?))?,
            },
            2,
        ),
        0x1D => (
            Op::PutSubstitute {
                slot: signed(1)?,
                input: class(u16::from(byte(2)?))?,
                output: class(u16::from(byte(3)?))?,
            },
            4,
        ),
        0x1E => (Op::PutCopy { slot: signed(1)? }, 2),
        0x1F => (Op::Insert, 1),
        0x20 => (Op::Delete, 1),
        0x21 => {
            let count = byte(1)?;
            let first = u32::try_from(associations.len()).map_err(|_| Fault::Truncated)?;
            let mut read = Vec::with_capacity(usize::from(count));
            for n in 0..usize::from(count) {
                read.push(signed(2 + n)?);
            }
            associations.extend(read);
            (Op::Associate { first, count }, 2 + usize::from(count))
        }
        0x22 => {
            let slot = signed(1)?;
            let skip = usize::from(byte(2)?);
            return Ok((
                Op::ContextItem { slot, end: 0 },
                3,
                Some(at.saturating_add(3 + skip)),
            ));
        }
        0x23 => (
            Op::SetAttribute {
                attribute: plain(byte(1)?, true)?,
            },
            2,
        ),
        0x24 => (
            Op::AddAttribute {
                attribute: plain(byte(1)?, true)?,
            },
            2,
        ),
        0x25 => (
            Op::SubtractAttribute {
                attribute: plain(byte(1)?, true)?,
            },
            2,
        ),
        0x26 => {
            let number = byte(1)?;
            let found = attribute(number)?;
            if found.is_indexed() || !found.is_slot_reference() {
                return Err(Fault::AttributeMisused(number));
            }
            (Op::SetSlotReference { attribute: found }, 2)
        }
        0x27 => {
            let index = byte(2)?;
            (
                Op::SetIndexedSlotReference {
                    attribute: indexed(byte(1)?, index, Some(true))?,
                    index,
                },
                3,
            )
        }
        0x28 => (
            Op::PushSlotAttribute {
                attribute: plain(byte(1)?, false)?,
                slot: signed(2)?,
            },
            3,
        ),
        0x29 => (
            Op::PushGlyphAttribute {
                attribute: u16::from(byte(1)?),
                slot: signed(2)?,
            },
            3,
        ),
        0x2A => (
            Op::PushGlyphMetric {
                metric: metric(byte(1)?)?,
                slot: signed(2)?,
                level: level(signed(3)?)?,
            },
            4,
        ),
        0x2B => (
            Op::PushFeature {
                feature: feature(byte(1)?)?,
                slot: signed(2)?,
            },
            3,
        ),
        0x2C => (
            Op::PushAttachedGlyphAttribute {
                attribute: u16::from(byte(1)?),
                slot: signed(2)?,
            },
            3,
        ),
        0x2D => (
            Op::PushAttachedGlyphMetric {
                metric: metric(byte(1)?)?,
                slot: signed(2)?,
                level: level(signed(3)?)?,
            },
            4,
        ),
        0x2E => {
            let index = level(signed(3)?)?;
            (
                Op::PushIndexedSlotAttribute {
                    attribute: indexed(byte(1)?, index, None)?,
                    slot: signed(2)?,
                    index,
                },
                4,
            )
        }
        0x30 => (Op::PopReturn, 1),
        0x31 => (Op::ReturnZero, 1),
        0x32 => (Op::ReturnTrue, 1),
        0x33..=0x35 => {
            let index = byte(2)?;
            let found = indexed(byte(1)?, index, Some(false))?;
            let op = match opcode {
                0x33 => Op::SetIndexedAttribute {
                    attribute: found,
                    index,
                },
                0x34 => Op::AddIndexedAttribute {
                    attribute: found,
                    index,
                },
                _ => Op::SubtractIndexedAttribute {
                    attribute: found,
                    index,
                },
            };
            (op, 3)
        }
        0x37 => (Op::PushVersion, 1),
        0x38 => (
            Op::PutSubstitute {
                slot: signed(1)?,
                input: class(short(2)?)?,
                output: class(short(4)?)?,
            },
            6,
        ),
        0x3B => (
            Op::PutGlyph {
                class: class(short(1)?)?,
            },
            3,
        ),
        0x3C => (
            Op::PushGlyphAttribute {
                attribute: short(1)?,
                slot: signed(3)?,
            },
            4,
        ),
        0x3D => (
            Op::PushAttachedGlyphAttribute {
                attribute: short(1)?,
                slot: signed(3)?,
            },
            4,
        ),
        0x3E => (Op::BitAnd, 1),
        0x3F => (Op::BitOr, 1),
        0x40 => (Op::BitNot, 1),
        0x41 => (
            Op::SetBits {
                mask: i32::from(i16::from_be_bytes(short(1)?.to_be_bytes())),
                value: i32::from(i16::from_be_bytes(short(3)?.to_be_bytes())),
            },
            5,
        ),
        0x42 => (
            Op::SetFeature {
                feature: feature(byte(1)?)?,
                slot: signed(2)?,
            },
            3,
        ),
        0x1A | 0x2F | 0x36 | 0x39 | 0x3A => return Err(Fault::Unimplemented(opcode)),
        other => return Err(Fault::IllegalOpcode(other)),
    };
    Ok((op.0, op.1, None))
}

/// The stack's depth along the block: a constraint starts with its true on
/// the stack and an action with none; a `ContextItem`'s block leaves one
/// value whether it runs or is skipped, so the depth after it is one more
/// either way. Each instruction's pops are taken before its pushes.
///
/// Returns the index of the instruction refused and why.
fn depth(code: &[Instruction], site: Site, limits: &Limits) -> Result<(), (usize, Fault)> {
    let ceiling = usize::try_from(limits.max_stack_depth).unwrap_or(usize::MAX);
    let mut depth = usize::from(!site.block.is_action());
    if depth > ceiling {
        return Err((0, Fault::StackOverflow));
    }
    // The open `ContextItem` blocks: where each ends and the depth it began
    // at, innermost last.
    let mut open: Vec<(usize, usize)> = Vec::new();
    let close = |open: &mut Vec<(usize, usize)>, index: usize, depth: usize| {
        while let Some(&(end, start)) = open.last() {
            if end != index {
                break;
            }
            open.pop();
            if depth != start + 1 {
                return Err((index, Fault::NotOneTest));
            }
        }
        Ok(())
    };
    for (index, instruction) in code.iter().enumerate() {
        close(&mut open, index, depth)?;
        if let Op::ContextItem { end, .. } = instruction.op {
            let enclosing = open.last().map_or(code.len(), |(end, _)| *end);
            if end <= index || end > enclosing {
                return Err((index, Fault::SkipOffBoundary));
            }
            open.push((end, depth));
            continue;
        }
        let (pops, pushes) = instruction.op.stack();
        depth = depth
            .checked_sub(pops)
            .ok_or((index, Fault::StackUnderflow))?
            + pushes;
        if depth > ceiling {
            return Err((index, Fault::StackOverflow));
        }
    }
    close(&mut open, code.len(), depth)?;
    // Every block ends at an instruction or the code's end, so none is left
    // open; a skip landing past the code's end was refused above.
    if open.is_empty() {
        Ok(())
    } else {
        Err((code.len(), Fault::SkipOffBoundary))
    }
}

/// What code reads: the slot stream as it stands, and the face behind it.
///
/// Slots are named by their index in the stream; the machine has already
/// turned an instruction's offset into an index and held it inside the
/// stream.
pub trait Reads {
    /// How many slots the stream holds.
    fn slot_count(&self) -> usize;

    /// Slot attribute `attribute` of slot `slot`; `index` names a component
    /// or a user attribute where the attribute is indexed, and is 0
    /// otherwise.
    ///
    /// # Errors
    ///
    /// A refusal where the stream cannot answer it.
    fn slot_attribute(
        &self,
        slot: usize,
        attribute: SlotAttribute,
        index: u8,
    ) -> Result<i32, Refusal>;

    /// Glyph attribute `attribute` of the glyph in slot `slot`: 0 where the
    /// glyph's runs hold none.
    ///
    /// # Errors
    ///
    /// The refusal the face's `Glat` gives for a glyph it cannot read.
    fn glyph_attribute(&self, slot: usize, attribute: u16) -> Result<i32, Refusal>;

    /// Metric `metric` of the glyph in slot `slot`, over the slots attached
    /// to it at `level` or lower; 0 is the glyph alone.
    ///
    /// # Errors
    ///
    /// A refusal where the face cannot give the metric.
    fn metric(&self, slot: usize, metric: Metric, level: u8) -> Result<i32, Refusal>;

    /// The value of feature `feature`, numbered in `Feat`'s order, for slot
    /// `slot`.
    fn feature(&self, slot: usize, feature: u8) -> i32;

    /// The slot that slot `slot` is attached to, where it is attached.
    fn attached_to(&self, slot: usize) -> Option<usize>;
}

/// The instructions a run may still execute: [`Limits::max_instructions`],
/// shared by every block the run runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    left: u32,
    spent: bool,
}

impl Budget {
    /// A run's whole budget.
    #[must_use]
    pub const fn new(limits: &Limits) -> Budget {
        Budget {
            left: limits.max_instructions,
            spent: false,
        }
    }

    /// The instructions still to spend.
    #[must_use]
    pub const fn left(&self) -> u32 {
        self.left
    }

    /// Whether the run has asked for more than it had.
    #[must_use]
    pub const fn is_spent(&self) -> bool {
        self.spent
    }

    /// Charges one instruction.
    pub(crate) fn charge(&mut self) -> Result<(), Refusal> {
        if self.left == 0 {
            self.spent = true;
            return Err(Refusal::TooManyInstructions);
        }
        self.left -= 1;
        Ok(())
    }
}

/// What a constraint is run against: the scan position and the slot under
/// test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    /// The scan position: the stream index of the rule's first modified
    /// slot, which `ContextItem` offsets count from.
    pub position: usize,
    /// The stream index of the slot under test, which every other slot
    /// offset counts from; `None` for a pass constraint, which tests no
    /// slot, counts from the stream's first, and skips every `ContextItem`.
    pub under_test: Option<usize>,
}

/// Runs a constraint and returns its answer: the value `PopRet` returns, or
/// the top of the stack where the code ends without one. The rule's test
/// for this slot holds where it is not 0.
///
/// # Errors
///
/// [`Refusal::TooManyInstructions`] once the budget is spent, and
/// [`Refusal::CodeRefused`] for a division the machine cannot do, a slot
/// offset outside the stream, or an action's program handed here; and a
/// refusal the stream gives for a read.
pub fn answer<E: Reads + ?Sized>(
    program: &Program,
    stream: &E,
    frame: Frame,
    budget: &mut Budget,
) -> Result<i32, Refusal> {
    let site = program.site;
    let base = frame
        .under_test
        .map_or(0, |slot| i64::try_from(slot).unwrap_or(i64::MAX));
    let mut stack: Vec<i32> = Vec::new();
    if !site.block.is_action() {
        stack.push(1);
    }
    let mut index = 0usize;
    while let Some(instruction) = program.code.get(index) {
        budget.charge()?;
        let refuse = |fault: Fault| Refusal::CodeRefused {
            pass: site.pass,
            block: site.block,
            at: instruction.at,
            fault,
        };
        match instruction.op {
            Op::ContextItem { slot, end } => {
                let position = i64::try_from(frame.position).unwrap_or(i64::MAX);
                let tested = frame.under_test.and_then(|tested| {
                    let wanted = offset(position, slot, stream.slot_count());
                    (wanted == Some(tested)).then_some(())
                });
                if tested.is_none() {
                    stack.push(1);
                    index = end;
                    continue;
                }
            }
            Op::PopReturn => return stack.pop().ok_or_else(|| refuse(Fault::StackUnderflow)),
            Op::ReturnZero => return Ok(0),
            Op::ReturnTrue => return Ok(1),
            op => compute(op, &mut stack, stream, base).map_err(|fault| match fault {
                Computed::Fault(fault) => refuse(fault),
                Computed::Refused(refusal) => refusal,
                // An action's instruction: no constraint holds one, so this
                // is an action's program handed to a constraint's run.
                Computed::NotComputing => refuse(Fault::ActionInConstraint(instruction.opcode)),
            })?,
        }
        index += 1;
    }
    Ok(stack.last().copied().unwrap_or(0))
}

/// Whether the rule's test for the slot under test holds: [`answer`], not 0.
///
/// # Errors
///
/// As [`answer`].
pub fn holds<E: Reads + ?Sized>(
    program: &Program,
    stream: &E,
    frame: Frame,
    budget: &mut Budget,
) -> Result<bool, Refusal> {
    answer(program, stream, frame, budget).map(|value| value != 0)
}

/// Why a computing instruction could not go on.
pub(crate) enum Computed {
    /// A fault the machine names at the instruction.
    Fault(Fault),
    /// A refusal the stream gave for a read.
    Refused(Refusal),
    /// The instruction is not one that computes: control and action
    /// instructions are their caller's.
    NotComputing,
}

/// The stream index `slot` slots from `base`, where the stream holds it;
/// `base` may stand before the stream, as an action's inserted slot does.
pub(crate) fn offset(base: i64, slot: i8, count: usize) -> Option<usize> {
    let at = base.checked_add(i64::from(slot))?;
    let at = usize::try_from(at).ok()?;
    (at < count).then_some(at)
}

/// Runs one computing instruction against `stack`, its slot offsets
/// counted from `base`.
pub(crate) fn compute<E: Reads + ?Sized>(
    op: Op,
    stack: &mut Vec<i32>,
    stream: &E,
    base: i64,
) -> Result<(), Computed> {
    let mut pop = || stack.pop().ok_or(Computed::Fault(Fault::StackUnderflow));
    let slot_at = |slot: i8| {
        offset(base, slot, stream.slot_count()).ok_or(Computed::Fault(Fault::SlotOutOfRange))
    };
    let value = match op {
        Op::Nop => return Ok(()),
        Op::Push(value) => value,
        Op::PushVersion => ENGINE_VERSION,
        Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Min | Op::Max | Op::And | Op::Or => {
            let top = pop()?;
            let next = pop()?;
            match op {
                Op::Add => next.wrapping_add(top),
                Op::Sub => next.wrapping_sub(top),
                Op::Mul => next.wrapping_mul(top),
                Op::Div => {
                    if top == 0 {
                        return Err(Computed::Fault(Fault::DivisionByZero));
                    }
                    next.checked_div(top)
                        .ok_or(Computed::Fault(Fault::DivisionOverflow))?
                }
                Op::Min => next.min(top),
                Op::Max => next.max(top),
                Op::And => i32::from(next != 0 && top != 0),
                _ => i32::from(next != 0 || top != 0),
            }
        }
        Op::Equal
        | Op::NotEqual
        | Op::Less
        | Op::Greater
        | Op::LessOrEqual
        | Op::GreaterOrEqual
        | Op::BitAnd
        | Op::BitOr => {
            let top = pop()?;
            let next = pop()?;
            match op {
                Op::Equal => i32::from(next == top),
                Op::NotEqual => i32::from(next != top),
                Op::Less => i32::from(next < top),
                Op::Greater => i32::from(next > top),
                Op::LessOrEqual => i32::from(next <= top),
                Op::GreaterOrEqual => i32::from(next >= top),
                Op::BitAnd => next & top,
                _ => next | top,
            }
        }
        Op::Neg => pop()?.wrapping_neg(),
        Op::Trunc8 => pop()? & 0xFF,
        Op::Trunc16 => pop()? & 0xFFFF,
        Op::Not => i32::from(pop()? == 0),
        Op::BitNot => !pop()?,
        Op::SetBits { mask, value } => (pop()? & !mask) | value,
        Op::Cond => {
            let condition = pop()?;
            let when_true = pop()?;
            let when_false = pop()?;
            if condition == 0 {
                when_false
            } else {
                when_true
            }
        }
        Op::PushSlotAttribute { attribute, slot } => stream
            .slot_attribute(slot_at(slot)?, attribute, 0)
            .map_err(Computed::Refused)?,
        Op::PushIndexedSlotAttribute {
            attribute,
            slot,
            index,
        } => stream
            .slot_attribute(slot_at(slot)?, attribute, index)
            .map_err(Computed::Refused)?,
        Op::PushGlyphAttribute { attribute, slot } => stream
            .glyph_attribute(slot_at(slot)?, attribute)
            .map_err(Computed::Refused)?,
        Op::PushGlyphMetric {
            metric,
            slot,
            level,
        } => stream
            .metric(slot_at(slot)?, metric, level)
            .map_err(Computed::Refused)?,
        Op::PushFeature { feature, slot } => stream.feature(slot_at(slot)?, feature),
        Op::PushAttachedGlyphAttribute { attribute, slot } => {
            match stream.attached_to(slot_at(slot)?) {
                Some(parent) => stream
                    .glyph_attribute(parent, attribute)
                    .map_err(Computed::Refused)?,
                None => 0,
            }
        }
        Op::PushAttachedGlyphMetric {
            metric,
            slot,
            level,
        } => match stream.attached_to(slot_at(slot)?) {
            Some(parent) => stream
                .metric(parent, metric, level)
                .map_err(Computed::Refused)?,
            None => 0,
        },
        Op::ContextItem { .. }
        | Op::PopReturn
        | Op::ReturnZero
        | Op::ReturnTrue
        | Op::Next
        | Op::CopyNext
        | Op::PutGlyph { .. }
        | Op::PutSubstitute { .. }
        | Op::PutCopy { .. }
        | Op::Insert
        | Op::Delete
        | Op::Associate { .. }
        | Op::SetAttribute { .. }
        | Op::AddAttribute { .. }
        | Op::SubtractAttribute { .. }
        | Op::SetSlotReference { .. }
        | Op::SetIndexedSlotReference { .. }
        | Op::SetIndexedAttribute { .. }
        | Op::AddIndexedAttribute { .. }
        | Op::SubtractIndexedAttribute { .. }
        | Op::SetFeature { .. } => return Err(Computed::NotComputing),
    };
    stack.push(value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{check, Block, Bounds, Fault, Op, Site, SlotAttribute};
    use crate::graphite::Refusal;
    use crate::limits::Limits;

    const RULE: Site = Site {
        pass: 3,
        block: Block::Constraint(7),
    };

    fn bounds() -> Bounds {
        Bounds {
            classes: 4,
            features: 2,
            user_attributes: 3,
            positioning: false,
        }
    }

    #[test]
    fn every_opcode_decodes_to_its_length() {
        // Each opcode with operands that are in range, and the length the
        // appendix's table gives it.
        let cases: [(&[u8], usize); 12] = [
            (&[0x05, 1, 2, 3, 4], 1),
            (&[0x22, 0, 2, 0x01, 5], 2),
            (&[0x28, 20, 0], 1),
            (&[0x2A, 2, 0, 0], 1),
            (&[0x2E, 55, 0, 2], 1),
            (&[0x3C, 0, 9, 0], 1),
            (&[0x3D, 1, 0, 0], 1),
            (&[0x41, 0, 1, 0, 1], 1),
            (&[0x2B, 1, 0], 1),
            (&[0x01, 1, 0x02, 2, 0x03, 0, 3, 0x04, 0, 4], 4),
            (&[0x29, 9, 1, 0x2C, 9, 1], 2),
            (&[0x2D, 2, 0, 1, 0x37], 2),
        ];
        for (code, count) in cases {
            let program = check(code, RULE, &bounds(), &Limits::DEFAULT)
                .unwrap_or_else(|refusal| panic!("{code:02X?}: {refusal:?}"));
            assert_eq!(program.instructions().len(), count, "{code:02X?}");
        }
    }

    #[test]
    fn a_skip_must_land_on_an_instruction() {
        // `ContextItem 0 1` lands inside `PushShort`'s operands.
        let code = [0x22, 0, 1, 0x03, 0, 1];
        assert_eq!(
            check(&code, RULE, &bounds(), &Limits::DEFAULT).map(|_| ()),
            Err(Refusal::CodeRefused {
                pass: 3,
                block: Block::Constraint(7),
                at: 0,
                fault: Fault::SkipOffBoundary
            })
        );
        // Landing on the next instruction, the block one test, is clean.
        let code = [0x22, 0, 3, 0x03, 0, 1, 0x10];
        let program = check(&code, RULE, &bounds(), &Limits::DEFAULT).expect("clean");
        assert_eq!(
            program.instructions()[0].op,
            Op::ContextItem { slot: 0, end: 2 }
        );
    }

    #[test]
    fn attributes_are_held_to_the_instruction_that_names_them() {
        // `AttrSet` of a read-only attribute, of a slot reference, and of an
        // indexed one; `PushSlotAttr` of an indexed one.
        let action = Site {
            pass: 0,
            block: Block::Action(0),
        };
        for (code, number) in [
            ([0x01, 1, 0x23, 19], 19u8),
            ([0x01, 1, 0x23, 2], 2),
            ([0x01, 1, 0x23, 55], 55),
            ([0x01, 1, 0x26, 20], 20),
        ] {
            assert!(matches!(
                check(&code, action, &bounds(), &Limits::DEFAULT),
                Err(Refusal::CodeRefused {
                    fault: Fault::AttributeMisused(n),
                    at: 2,
                    ..
                }) if n == number
            ));
        }
        assert!(matches!(
            check(&[0x28, 55, 0], RULE, &bounds(), &Limits::DEFAULT),
            Err(Refusal::CodeRefused {
                fault: Fault::AttributeMisused(55),
                ..
            })
        ));
        // Every revealed number round-trips, and the rest are unknown.
        for number in 0..=255u8 {
            match SlotAttribute::from_number(number) {
                Some(attribute) => assert_eq!(attribute.number(), number),
                None => assert!(!SlotAttribute::ALL.iter().any(|a| a.number() == number)),
            }
        }
    }
}
