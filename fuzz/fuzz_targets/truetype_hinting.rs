//! The TrueType bytecode interpreter, over programs it was never meant to
//! see: arbitrary `fpgm`, `prep`, `cvt ` and glyph programs on a face built
//! from them, hinted at the sizes the input chooses (FT-04b.13,
//! `docs/design/hinting.md`).
//!
//! The input is carved into fields by `tinker_pdf_fuzz::hinting`, not parsed
//! as a font: the sizes and options, `maxp`'s five limits, the control value
//! table, the font program, the control value program, a simple glyph's
//! program and points, and a composite's program, which places the simple
//! glyph twice by an offset, by matching a point or under a scale as the
//! options say. So every byte a mutation changes lands in a program or a
//! value the interpreter reads, and none in a table directory a mutation
//! would only break. The face's `fpgm` runs once, each size's `prep` once,
//! and both glyphs are hinted at each size.
//!
//! Ruling 1: bytecode is untrusted input, and the interpreter's bounds — its
//! stack of at most 65 536 values, its storage, CVT, function and twilight
//! limits from `maxp`, its 64 nested calls and its 2^20 steps a run — are
//! what keep every input finite. A run that passes one ends in a named
//! `Unhinted`, never a panic or a wait.
//!
//! # What this target checks, and what it does not
//!
//! Beyond "it did not panic, hang or exhaust memory", every glyph that hints
//! is held by `tinker_pdf_fuzz::hinting`'s `check`: its outline inside the
//! segment cap with every coordinate finite and on a 128th of a pixel, and
//! its steps inside the budgets of the runs that made it; and a composite of
//! two 64-point glyphs never reports more points than a zone holds.
//!
//! It does not check that a hinted point is where the instruction set puts
//! it. A program the interpreter runs wrong runs wrong here as consistently
//! as a right one runs right: each instruction's effect is held to its
//! specification by `crates/tinker-pdf-font/src/hint/tests/`, a file a
//! family, and the committed faces' hinted outlines by
//! `hinting_fingerprints.rs`. The seeds are replayed on stable by
//! `crates/tinker-pdf-font/tests/fuzz_seeds.rs`.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = tinker_pdf_fuzz::hinting::exercise(data);
});
