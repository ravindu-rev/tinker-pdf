//! *Managing the flow of control* and *Defining and using functions and
//! instructions*: `IF`, `ELSE` and `EIF` at any nesting, the relative jumps
//! `JMPR`, `JROT` and `JROF`, and `FDEF`, `ENDF`, `CALL`, `LOOPCALL` and
//! `IDEF`, with an opcode the instruction set leaves undefined dispatched to
//! the definition an `IDEF` gave it.
//!
//! A definition's body is kept as its own bytes: from the instruction after
//! its `FDEF` or `IDEF` to the one before the first `ENDF` the instruction
//! stream holds after it, push data passed over. Running a definition is
//! running those bytes, so a jump inside one is relative to it and cannot
//! leave it, and the end of its bytes is its `ENDF`. Each call nests one
//! level, at most [`CALL_DEPTH`], and every instruction a skip or a
//! definition passes over is a step of the run's budget.

use std::sync::Arc;

use super::vm::{Error, Program, Vm, CALL_DEPTH};

/// The functions and instructions a face's programs defined.
#[derive(Clone, Debug, Default)]
pub(super) struct Definitions {
    /// Each function's body, by number; `None` where none is defined.
    functions: Vec<Option<Arc<[u8]>>>,
    /// Each opcode's instruction definition, by opcode; empty until the
    /// first `IDEF` that defines one.
    instructions: Vec<Option<Arc<[u8]>>>,
    /// How many opcodes have a definition.
    instruction_count: usize,
}

/// Whether the instruction set has an instruction for `op`, so that an
/// `IDEF` of it is ignored: every opcode but the 37 the *TrueType Reference
/// Manual*'s instruction set summary assigns to none (0x28, 0x7B, 0x83,
/// 0x84 and 0x8F to 0xAF). `GETVARIATION`, which the OpenType text puts at
/// 0x91, is among them here: this engine does not execute it, so the `IDEF`
/// the OpenType text requires a font that uses it to carry stands in for it.
pub(super) fn is_defined(op: u8) -> bool {
    !matches!(op, 0x28 | 0x7B | 0x83 | 0x84 | 0x8F..=0xAF)
}

/// Where a jump by `offset` from the instruction at `at` lands: "The jump is
/// relative to the position of the instruction itself" (*JuMP*). Landing at
/// the end of `code` ends it; landing outside it is an error.
fn jump(code: &[u8], at: usize, offset: i32) -> Result<usize, Error> {
    let target = i64::try_from(at).unwrap_or(i64::MAX) + i64::from(offset);
    usize::try_from(target)
        .ok()
        .filter(|&target| target <= code.len())
        .ok_or(Error::JumpOutOfRange)
}

impl Vm {
    /// Executes the control flow instruction `op` at `at` in `code`, and
    /// answers where the next instruction begins.
    pub(super) fn flow(
        &mut self,
        op: u8,
        code: &[u8],
        at: usize,
        stop: Option<&dyn Fn() -> bool>,
    ) -> Result<usize, Error> {
        match op {
            // IF[ ]: "if it is zero (FALSE), the instruction pointer is jumped
            // to the next ELSE or EIF instruction"; a true value runs on.
            0x58 => {
                if self.pop()? != 0 {
                    Ok(at + 1)
                } else {
                    self.skip(code, at + 1, true)
                }
            }
            // ELSE[ ], reached by the instructions an IF ran: "If an else
            // statement is found before the EIF, the instruction pointer is
            // moved to the EIF statement."
            0x1B => self.skip(code, at + 1, false),
            // EIF[ ] marks the end of an IF, and does nothing itself.
            0x59 => Ok(at + 1),
            // JMPR[ ]: pops offset.
            0x1C => {
                let offset = self.pop()?;
                jump(code, at, offset)
            }
            // JROT[ ], JROF[ ]: "Pops and tests the element value, and then
            // pops the offset", jumping on true or on false.
            0x78 | 0x79 => {
                let e = self.pop()?;
                let offset = self.pop()?;
                if (e != 0) == (op == 0x78) {
                    jump(code, at, offset)
                } else {
                    Ok(at + 1)
                }
            }
            0x2C | 0x89 => self.define(op, code, at),
            // ENDF[ ]: the end of the definition running. Its own ENDF is not
            // among a definition's bytes, so one is reached only by a jump
            // into push data.
            0x2D => {
                if self.depth == 0 {
                    Err(Error::EndfOutsideDefinition)
                } else {
                    Ok(code.len())
                }
            }
            // CALL[ ]: pops f.
            0x2B => {
                let f = self.pop()?;
                let body = self.function(f)?;
                self.call(&body, stop)?;
                Ok(at + 1)
            }
            // LOOPCALL[ ]: pops f, then count, and "Calls the function f,
            // count number of times" — none for a count below one.
            _ => {
                let f = self.pop()?;
                let count = self.pop()?;
                let body = self.function(f)?;
                for _ in 0..count.max(0) {
                    self.spend(1)?;
                    self.call(&body, stop)?;
                }
                Ok(at + 1)
            }
        }
    }

    /// An opcode this interpreter executes no instruction for: the definition
    /// an `IDEF` gave it, run as a call, or an error where none did — as for
    /// every opcode the instruction set defines, which no `IDEF` can.
    pub(super) fn undefined(
        &mut self,
        op: u8,
        stop: Option<&dyn Fn() -> bool>,
    ) -> Result<(), Error> {
        let body = self
            .definitions
            .instructions
            .get(usize::from(op))
            .cloned()
            .flatten()
            .ok_or(Error::NoInstruction(op))?;
        self.call(&body, stop)
    }

    /// Runs a definition's body one call deeper.
    fn call(&mut self, body: &[u8], stop: Option<&dyn Fn() -> bool>) -> Result<(), Error> {
        if self.depth >= CALL_DEPTH {
            return Err(Error::CallDepth);
        }
        self.depth += 1;
        let result = self.execute(body, stop);
        self.depth -= 1;
        result
    }

    /// Function `f`'s body.
    fn function(&self, f: i32) -> Result<Arc<[u8]>, Error> {
        let index = usize::try_from(f)
            .ok()
            .filter(|&index| index < self.limits.functions())
            .ok_or(Error::FunctionIndex(f))?;
        self.definitions
            .functions
            .get(index)
            .cloned()
            .flatten()
            .ok_or(Error::UndefinedFunction(f))
    }

    /// FDEF[ ] and IDEF[ ] at `at`: pops the function number or the opcode,
    /// keeps the body that follows to its `ENDF`, and answers the instruction
    /// after that `ENDF`. "A function definition can appear only in the Font
    /// Program or the CVT program", and so can an instruction definition; and
    /// "An IDEF affects only undefined opcodes. If the opcode in question is
    /// already defined, the interpreter will ignore the IDEF." A later
    /// definition of the same number or opcode replaces an earlier one.
    fn define(&mut self, op: u8, code: &[u8], at: usize) -> Result<usize, Error> {
        let n = self.pop()?;
        if self.program() == Program::Glyph {
            return Err(Error::DefinitionInGlyphProgram);
        }
        if self.depth > 0 {
            return Err(Error::NestedDefinition);
        }
        // Where the body goes, `None` for an IDEF the specification ignores.
        let slot = if op == 0x2C {
            let f = usize::try_from(n)
                .ok()
                .filter(|&f| f < self.limits.functions())
                .ok_or(Error::FunctionIndex(n))?;
            Some((true, f))
        } else {
            let opcode = u8::try_from(n).map_err(|_| Error::OpcodeOutOfRange(n))?;
            if is_defined(opcode) {
                None
            } else {
                let index = usize::from(opcode);
                let new = !matches!(self.definitions.instructions.get(index), Some(Some(_)));
                if new && self.definitions.instruction_count >= self.limits.instruction_defs() {
                    return Err(Error::InstructionDefinitions);
                }
                Some((false, index))
            }
        };
        let end = self.definition_end(code, at + 1)?;
        if let Some((function, index)) = slot {
            let body: Arc<[u8]> = Arc::from(code.get(at + 1..end).unwrap_or_default());
            let definitions = Arc::make_mut(&mut self.definitions);
            let table = if function {
                &mut definitions.functions
            } else {
                &mut definitions.instructions
            };
            if table.len() <= index {
                table.resize(index + 1, None);
            }
            if table[index].replace(body).is_none() && !function {
                definitions.instruction_count += 1;
            }
        }
        Ok(end + 1)
    }

    /// Where the `ENDF` that ends the definition whose body begins at `from`
    /// stands.
    fn definition_end(&mut self, code: &[u8], from: usize) -> Result<usize, Error> {
        let mut at = from;
        while let Some(&op) = code.get(at) {
            if op == 0x2D {
                self.spend(1)?;
                return Ok(at);
            }
            at = self.pass_over(code, at)?;
        }
        Err(Error::UnterminatedDefinition)
    }

    /// Passes over the instructions from `from` to the `EIF` that matches the
    /// `IF` or `ELSE` before them — or, `to_else`, to an `ELSE` of the same
    /// nesting first — and answers the instruction after it.
    fn skip(&mut self, code: &[u8], from: usize, to_else: bool) -> Result<usize, Error> {
        let mut nesting = 0usize;
        let mut at = from;
        while let Some(&op) = code.get(at) {
            let next = self.pass_over(code, at)?;
            match op {
                0x58 => nesting += 1,
                0x1B if to_else && nesting == 0 => return Ok(next),
                0x59 => {
                    if nesting == 0 {
                        return Ok(next);
                    }
                    nesting -= 1;
                }
                _ => {}
            }
            at = next;
        }
        Err(Error::UnterminatedIf)
    }
}
