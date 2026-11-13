//! The machine: a bounded stack of 32-bit values, the instruction stream
//! decoded with its inline push data, the steps a run may take, and the
//! errors that stop one.
//!
//! This file executes the instructions that touch nothing but the stack —
//! *Pushing data onto the interpreter stack*, *Managing the stack*, *Logical
//! functions*, *Arithmetic and math instructions* and *Debugging* — and
//! dispatches every other opcode to the family that executes it.

use super::fixed;

/// The steps one run may take: a run of `fpgm`, of `prep`, or of a glyph's or
/// a component's program, the functions it calls included
/// (`docs/design/hinting.md`, "Bounds that keep hostile bytecode finite").
///
/// A step is an instruction dispatched, or one of the units of work an
/// instruction does beyond its own: each stack element `MINDEX` moves.
pub const BUDGET: u32 = 1 << 20;

/// A run asks its stop predicate once in this many steps.
pub const STOP_EVERY: u32 = 4096;

/// The fewest stack entries a face is given, whatever its `maxp` declares.
const STACK_FLOOR: usize = 256;
/// The most, whatever it declares: 256 KiB of values.
const STACK_CEILING: usize = 65_536;
/// What a face is given beyond its `maxStackElements`, so a producer that
/// under-declares still runs.
const STACK_HEADROOM: usize = 32;
/// An opcode is a byte, so no face defines more instructions than this.
const INSTRUCTION_DEFS_CEILING: usize = 256;
/// The most twilight points a face is given, whatever its `maxp` declares.
const TWILIGHT_CEILING: usize = 16_384;

/// The five fields of a version 1.0 `maxp` table that bound a face's
/// programs, as the table states them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MaxProfile {
    /// `maxTwilightPoints`: the points of zone 0.
    pub max_twilight_points: u16,
    /// `maxStorage`: the storage area's locations.
    pub max_storage: u16,
    /// `maxFunctionDefs`: one more than the highest function number.
    pub max_function_defs: u16,
    /// `maxInstructionDefs`: how many opcodes `IDEF` may define.
    pub max_instruction_defs: u16,
    /// `maxStackElements`: the deepest stack any of the face's programs needs.
    pub max_stack_elements: u16,
}

/// What a face's programs may use: its `maxp` profile with this engine's
/// bounds applied, so a hostile profile asks for no more than these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    stack: usize,
    storage: usize,
    functions: usize,
    instruction_defs: usize,
    twilight: usize,
}

impl Limits {
    /// A profile's limits: the stack at `maxStackElements` plus 32, at least
    /// 256 and at most 65 536 entries; the instruction definitions at most
    /// 256; the twilight points at most 16 384; storage and functions as
    /// declared.
    #[must_use]
    pub fn new(profile: MaxProfile) -> Limits {
        Limits {
            stack: (usize::from(profile.max_stack_elements) + STACK_HEADROOM)
                .clamp(STACK_FLOOR, STACK_CEILING),
            storage: usize::from(profile.max_storage),
            functions: usize::from(profile.max_function_defs),
            instruction_defs: usize::from(profile.max_instruction_defs)
                .min(INSTRUCTION_DEFS_CEILING),
            twilight: usize::from(profile.max_twilight_points).min(TWILIGHT_CEILING),
        }
    }

    /// The limits a `maxp` table states, or `None` for one that is not
    /// version 1.0 and whole: a face with no TrueType profile is not hinted.
    #[must_use]
    pub fn from_maxp(maxp: &[u8]) -> Option<Limits> {
        let field = |at: usize| -> Option<u16> {
            let bytes = maxp.get(at..at + 2)?;
            Some(u16::from_be_bytes([bytes[0], bytes[1]]))
        };
        // The version 1.0 table is 32 bytes: version, numGlyphs, then
        // fourteen uint16 fields, maxTwilightPoints at 16.
        let version = maxp.get(0..4)?;
        if version != [0, 1, 0, 0] || maxp.len() < 32 {
            return None;
        }
        Some(Limits::new(MaxProfile {
            max_twilight_points: field(16)?,
            max_storage: field(18)?,
            max_function_defs: field(20)?,
            max_instruction_defs: field(22)?,
            max_stack_elements: field(24)?,
        }))
    }

    /// The deepest the stack may grow.
    #[must_use]
    pub fn stack(&self) -> usize {
        self.stack
    }

    /// The storage area's locations.
    #[must_use]
    pub fn storage(&self) -> usize {
        self.storage
    }

    /// How many functions may be defined, numbered from 0.
    #[must_use]
    pub fn functions(&self) -> usize {
        self.functions
    }

    /// How many opcodes `IDEF` may define.
    #[must_use]
    pub fn instruction_defs(&self) -> usize {
        self.instruction_defs
    }

    /// The points of zone 0, the twilight zone.
    #[must_use]
    pub fn twilight(&self) -> usize {
        self.twilight
    }
}

/// Which of a face's programs a run executes. Some instructions answer
/// differently by it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Program {
    /// The font program, `fpgm`, run once a face.
    Font,
    /// The control value program, `prep`, run once a size.
    ControlValue,
    /// A glyph's instructions, or a composite's.
    Glyph,
}

/// Why a run stopped short. Each names the bound or the rule a program broke;
/// the glyph, size or face it was hinting is drawn unhinted
/// (`docs/design/hinting.md`, "Failure").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// A push past the stack's bound ([`Limits::stack`]).
    StackOverflow,
    /// A pop from an empty stack.
    StackUnderflow,
    /// `CINDEX` or `MINDEX` named an element the stack does not hold.
    StackIndex(i32),
    /// A push instruction whose inline data runs past its program's end.
    TruncatedPush,
    /// An opcode this interpreter has no instruction for, and no `IDEF`
    /// defines.
    NoInstruction(u8),
    /// `DIV` by zero.
    DivideByZero,
    /// The run took more than [`BUDGET`] steps.
    Budget,
    /// The caller's stop predicate answered yes.
    Stopped,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::StackOverflow => write!(f, "the stack grew past its bound"),
            Error::StackUnderflow => write!(f, "a pop from an empty stack"),
            Error::StackIndex(k) => write!(f, "stack element {k} does not exist"),
            Error::TruncatedPush => write!(f, "a push ran past its program's end"),
            Error::NoInstruction(op) => write!(f, "opcode 0x{op:02X} has no instruction"),
            Error::DivideByZero => write!(f, "DIV by zero"),
            Error::Budget => write!(f, "the program ran past its {BUDGET} steps"),
            Error::Stopped => write!(f, "the run was stopped"),
        }
    }
}

/// The interpreter: a face's bounds and the state its programs read and
/// write.
#[derive(Clone, Debug)]
pub struct Vm {
    limits: Limits,
    /// The interpreter stack, bottom first.
    stack: Vec<i32>,
    /// The program the current run, or the last, executes.
    program: Program,
    /// The steps the current run, or the last, took.
    steps: u32,
    /// The step count at which the stop predicate is next asked.
    stop_due: u32,
}

impl Vm {
    /// A machine for a face with these limits, its stack empty.
    #[must_use]
    pub fn new(limits: Limits) -> Vm {
        Vm {
            limits,
            stack: Vec::new(),
            program: Program::Font,
            steps: 0,
            stop_due: STOP_EVERY,
        }
    }

    /// The limits the machine was made with.
    #[must_use]
    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// Runs `code` as `program` from an empty stack, asking `stop` once in
    /// [`STOP_EVERY`] steps whether to give up. What the program left on the
    /// stack, and the steps it took, are there to read afterwards, whether it
    /// ended or stopped with an error.
    pub fn run(
        &mut self,
        code: &[u8],
        program: Program,
        stop: Option<&dyn Fn() -> bool>,
    ) -> Result<(), Error> {
        self.stack.clear();
        self.start(program);
        self.execute(code, stop)
    }

    /// The stack as the last run left it, bottom first.
    #[must_use]
    pub fn stack(&self) -> &[i32] {
        &self.stack
    }

    /// The steps the last run took.
    #[must_use]
    pub fn steps(&self) -> u32 {
        self.steps
    }

    /// The program the last run executed.
    #[must_use]
    pub fn program(&self) -> Program {
        self.program
    }

    /// Resets what a run counts.
    fn start(&mut self, program: Program) {
        self.program = program;
        self.steps = 0;
        self.stop_due = STOP_EVERY;
    }

    /// Spends `n` steps of the run's budget.
    pub(super) fn spend(&mut self, n: usize) -> Result<(), Error> {
        let n = u32::try_from(n).unwrap_or(u32::MAX);
        self.steps = self.steps.saturating_add(n);
        if self.steps > BUDGET {
            return Err(Error::Budget);
        }
        Ok(())
    }

    /// Pushes a value.
    pub(super) fn push(&mut self, value: i32) -> Result<(), Error> {
        if self.stack.len() >= self.limits.stack {
            return Err(Error::StackOverflow);
        }
        self.stack.push(value);
        Ok(())
    }

    /// Pops a value.
    pub(super) fn pop(&mut self) -> Result<i32, Error> {
        self.stack.pop().ok_or(Error::StackUnderflow)
    }

    /// Executes `code` from its first byte to its end.
    pub(super) fn execute(
        &mut self,
        code: &[u8],
        stop: Option<&dyn Fn() -> bool>,
    ) -> Result<(), Error> {
        let mut at = 0;
        while let Some(&op) = code.get(at) {
            self.spend(1)?;
            if self.steps >= self.stop_due {
                self.stop_due = (self.steps / STOP_EVERY + 1).saturating_mul(STOP_EVERY);
                if stop.is_some_and(|stop| stop()) {
                    return Err(Error::Stopped);
                }
            }
            at = self.instruction(op, code, at, stop)?;
        }
        Ok(())
    }

    /// Executes the instruction `op` found at `at` in `code`, and answers
    /// where the next one begins.
    fn instruction(
        &mut self,
        op: u8,
        code: &[u8],
        at: usize,
        _stop: Option<&dyn Fn() -> bool>,
    ) -> Result<usize, Error> {
        match op {
            // NPUSHB[ ], NPUSHW[ ]: a count byte, then that many bytes or words.
            0x40 | 0x41 => {
                let count = *code.get(at + 1).ok_or(Error::TruncatedPush)?;
                return self.push_inline(code, at + 2, usize::from(count), op == 0x41);
            }
            // PUSHB[abc], PUSHW[abc]: abc + 1 bytes or words.
            0xB0..=0xB7 => return self.push_inline(code, at + 1, usize::from(op - 0xAF), false),
            0xB8..=0xBF => return self.push_inline(code, at + 1, usize::from(op - 0xB7), true),
            0x20..=0x26 | 0x8A => self.manage_stack(op)?,
            0x50..=0x55 | 0x5A..=0x5C => self.logical(op)?,
            0x60..=0x67 | 0x8B | 0x8C => self.arithmetic(op)?,
            // DEBUG[ ] pops its number; this engine has no debugger to give it.
            0x4F => {
                self.pop()?;
            }
            _ => return Err(Error::NoInstruction(op)),
        }
        Ok(at + 1)
    }

    /// Pushes `count` bytes, each padded to 32 bits unsigned, or `count`
    /// words, high byte first, each sign-extended — the data from `from` in
    /// the instruction stream — and answers where the next instruction
    /// begins.
    fn push_inline(
        &mut self,
        code: &[u8],
        from: usize,
        count: usize,
        words: bool,
    ) -> Result<usize, Error> {
        let width = if words { 2 } else { 1 };
        let end = from + count * width;
        let data = code.get(from..end).ok_or(Error::TruncatedPush)?;
        for value in data.chunks_exact(width) {
            self.push(match *value {
                [high, low] => i32::from(i16::from_be_bytes([high, low])),
                [byte] => i32::from(byte),
                // `chunks_exact` gives `width`-byte chunks, one or two.
                _ => 0,
            })?;
        }
        Ok(end)
    }

    /// *Managing the stack*.
    fn manage_stack(&mut self, op: u8) -> Result<(), Error> {
        match op {
            // DUP[ ]: e, then e and e.
            0x20 => {
                let e = self.pop()?;
                self.push(e)?;
                self.push(e)?;
            }
            // POP[ ].
            0x21 => {
                self.pop()?;
            }
            // CLEAR[ ]: every element.
            0x22 => self.stack.clear(),
            // SWAP[ ]: pops e2, then e1; pushes e2, then e1.
            0x23 => {
                let e2 = self.pop()?;
                let e1 = self.pop()?;
                self.push(e2)?;
                self.push(e1)?;
            }
            // DEPTH[ ]: the number of elements, before the push.
            0x24 => {
                let depth = i32::try_from(self.stack.len()).unwrap_or(i32::MAX);
                self.push(depth)?;
            }
            // CINDEX[ ], MINDEX[ ]: pops k, then copies or moves the k-th
            // element, counted from the top from 1, to the top.
            0x25 | 0x26 => {
                let k = self.pop()?;
                let depth = self.stack.len();
                let from_top = usize::try_from(k)
                    .ok()
                    .filter(|&k| (1..=depth).contains(&k))
                    .ok_or(Error::StackIndex(k))?;
                let index = depth - from_top;
                if op == 0x25 {
                    let e = *self.stack.get(index).ok_or(Error::StackIndex(k))?;
                    self.push(e)?;
                } else {
                    // Every element above the one moved moves down one.
                    self.spend(from_top)?;
                    let e = self.stack.remove(index);
                    self.stack.push(e);
                }
            }
            // ROLL[ ]: pops a, b, c; pushes b, a, c — the third element to the
            // top, as MINDEX[ ] 3 moves it.
            _ => {
                let a = self.pop()?;
                let b = self.pop()?;
                let c = self.pop()?;
                self.push(b)?;
                self.push(a)?;
                self.push(c)?;
            }
        }
        Ok(())
    }

    /// *Logical functions*: each pushes 1 for true and 0 for false. The
    /// comparisons are signed, as the distances they compare are.
    fn logical(&mut self, op: u8) -> Result<(), Error> {
        let answer = match op {
            // NOT[ ]: pops e.
            0x5C => self.pop()? == 0,
            // AND[ ], OR[ ]: pops e1, then e2.
            0x5A | 0x5B => {
                let e1 = self.pop()? != 0;
                let e2 = self.pop()? != 0;
                if op == 0x5A {
                    e1 && e2
                } else {
                    e1 || e2
                }
            }
            // LT[ ] to NEQ[ ]: pops e2, then e1, and compares e1 with e2.
            _ => {
                let e2 = self.pop()?;
                let e1 = self.pop()?;
                match op {
                    0x50 => e1 < e2,
                    0x51 => e1 <= e2,
                    0x52 => e1 > e2,
                    0x53 => e1 >= e2,
                    0x54 => e1 == e2,
                    _ => e1 != e2,
                }
            }
        };
        self.push(i32::from(answer))
    }

    /// *Arithmetic and math instructions*, on 26.6 values with no overflow
    /// protection: a result past 32 bits wraps.
    fn arithmetic(&mut self, op: u8) -> Result<(), Error> {
        let result = match op {
            // ABS[ ], NEG[ ], FLOOR[ ], CEILING[ ]: pops n.
            0x64 => self.pop()?.wrapping_abs(),
            0x65 => self.pop()?.wrapping_neg(),
            0x66 => fixed::floor(self.pop()?),
            0x67 => fixed::ceil(self.pop()?),
            // ADD[ ] to MUL[ ], MAX[ ], MIN[ ]: pops n1, then n2.
            _ => {
                let n1 = self.pop()?;
                let n2 = self.pop()?;
                match op {
                    0x60 => n2.wrapping_add(n1),
                    0x61 => n2.wrapping_sub(n1),
                    // DIV[ ]: n2 / n1, n1 the divisor.
                    0x62 => fixed::div(n2, n1).ok_or(Error::DivideByZero)?,
                    0x63 => fixed::mul(n2, n1),
                    0x8B => n1.max(n2),
                    _ => n1.min(n2),
                }
            }
        };
        self.push(result)
    }
}

#[cfg(test)]
impl Vm {
    /// Runs `code` as `program` on a stack that starts as `stack`, bottom
    /// first: how the tests put a value no push instruction spells on it.
    pub(crate) fn run_on(
        &mut self,
        stack: &[i32],
        code: &[u8],
        program: Program,
    ) -> Result<(), Error> {
        self.stack.clear();
        for &value in stack {
            self.push(value)?;
        }
        self.start(program);
        self.execute(code, None)
    }
}
