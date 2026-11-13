//! TrueType hinting: the bytecode interpreter a face's `fpgm`, its `prep` and
//! each glyph's instructions run on.
//!
//! Design: `docs/design/hinting.md`, FT-04b's sections. Every instruction is
//! transcribed from the instruction set's published specifications and from
//! nothing else (ruling 21): the OpenType specification's *TrueType
//! Instruction Set* chapter, version 1.9.1, read beside chapter 5 of Apple's
//! *TrueType Reference Manual*. The OpenType text governs where the two
//! differ, and a choice both leave to the engine is made in the design
//! document and named as this engine's where the code makes it. Comments cite
//! the OpenType chapter by its section headings.
//!
//! Bytecode is untrusted input (ruling 1). Every count a program can spend is
//! bounded, by `maxp` or by the design, and a program that passes a bound
//! stops with an [`Error`] that names it: the glyph it was hinting is then
//! drawn from the outline its font states. Nothing here panics on any byte
//! sequence.
//!
//! - [`fixed`]: the 26.6 arithmetic, shared with the other hinters.
//! - [`vm`]: the machine — its stack, its decoding and dispatch, its bounds.
//! - `flow`: `IF` and the jumps, function and instruction definitions.
//! - [`storage`]: the storage area, the control value table, and the
//!   answers a program asks of its engine, at a [`Size`].
//! - [`state`]: the zones and their points, and the graphics state.
//! - `moves`: measuring points and moving them along the vectors.
//! - `shift`: shifting, aligning, interpolating and flipping points.
//! - `iup`: interpolating the untouched points, and the delta exceptions.

pub mod fixed;
mod flow;
mod iup;
mod moves;
mod shift;
pub mod state;
pub mod storage;
pub mod vm;

pub use state::{Dropout, GraphicsState, Point, Round, Vector, Zone};
pub use storage::Size;
pub use vm::{Error, Limits, MaxProfile, Program, Vm};

#[cfg(test)]
mod tests;
