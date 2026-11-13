//! The interpreter's tests, a file an instruction family, as
//! `docs/design/hinting.md`'s milestones name them.
//!
//! Every expected value is computed here, from the instruction's semantics as
//! the specification states them, transcribed into the test, and never read
//! back from the interpreter.

mod flow;
mod stack;

use super::vm::{Error, Limits, MaxProfile, Program, Vm};

/// The opcodes, by the specification's names: the first of a range for an
/// instruction with flags, which the tests add.
#[allow(dead_code)]
pub(super) mod op {
    pub const SVTCA: u8 = 0x00;
    pub const SPVTCA: u8 = 0x02;
    pub const SFVTCA: u8 = 0x04;
    pub const SPVTL: u8 = 0x06;
    pub const SFVTL: u8 = 0x08;
    pub const SPVFS: u8 = 0x0A;
    pub const SFVFS: u8 = 0x0B;
    pub const GPV: u8 = 0x0C;
    pub const GFV: u8 = 0x0D;
    pub const SFVTPV: u8 = 0x0E;
    pub const ISECT: u8 = 0x0F;
    pub const SRP0: u8 = 0x10;
    pub const SRP1: u8 = 0x11;
    pub const SRP2: u8 = 0x12;
    pub const SZP0: u8 = 0x13;
    pub const SZP1: u8 = 0x14;
    pub const SZP2: u8 = 0x15;
    pub const SZPS: u8 = 0x16;
    pub const SLOOP: u8 = 0x17;
    pub const RTG: u8 = 0x18;
    pub const RTHG: u8 = 0x19;
    pub const SMD: u8 = 0x1A;
    pub const ELSE: u8 = 0x1B;
    pub const JMPR: u8 = 0x1C;
    pub const SCVTCI: u8 = 0x1D;
    pub const SSWCI: u8 = 0x1E;
    pub const SSW: u8 = 0x1F;
    pub const DUP: u8 = 0x20;
    pub const POP: u8 = 0x21;
    pub const CLEAR: u8 = 0x22;
    pub const SWAP: u8 = 0x23;
    pub const DEPTH: u8 = 0x24;
    pub const CINDEX: u8 = 0x25;
    pub const MINDEX: u8 = 0x26;
    pub const ALIGNPTS: u8 = 0x27;
    pub const UTP: u8 = 0x29;
    pub const LOOPCALL: u8 = 0x2A;
    pub const CALL: u8 = 0x2B;
    pub const FDEF: u8 = 0x2C;
    pub const ENDF: u8 = 0x2D;
    pub const MDAP: u8 = 0x2E;
    pub const IUP: u8 = 0x30;
    pub const SHP: u8 = 0x32;
    pub const SHC: u8 = 0x34;
    pub const SHZ: u8 = 0x36;
    pub const SHPIX: u8 = 0x38;
    pub const IP: u8 = 0x39;
    pub const MSIRP: u8 = 0x3A;
    pub const ALIGNRP: u8 = 0x3C;
    pub const RTDG: u8 = 0x3D;
    pub const MIAP: u8 = 0x3E;
    pub const NPUSHB: u8 = 0x40;
    pub const NPUSHW: u8 = 0x41;
    pub const WS: u8 = 0x42;
    pub const RS: u8 = 0x43;
    pub const WCVTP: u8 = 0x44;
    pub const RCVT: u8 = 0x45;
    pub const GC: u8 = 0x46;
    pub const SCFS: u8 = 0x48;
    pub const MD: u8 = 0x49;
    pub const MPPEM: u8 = 0x4B;
    pub const MPS: u8 = 0x4C;
    pub const FLIPON: u8 = 0x4D;
    pub const FLIPOFF: u8 = 0x4E;
    pub const DEBUG: u8 = 0x4F;
    pub const LT: u8 = 0x50;
    pub const LTEQ: u8 = 0x51;
    pub const GT: u8 = 0x52;
    pub const GTEQ: u8 = 0x53;
    pub const EQ: u8 = 0x54;
    pub const NEQ: u8 = 0x55;
    pub const ODD: u8 = 0x56;
    pub const EVEN: u8 = 0x57;
    pub const IF: u8 = 0x58;
    pub const EIF: u8 = 0x59;
    pub const AND: u8 = 0x5A;
    pub const OR: u8 = 0x5B;
    pub const NOT: u8 = 0x5C;
    pub const DELTAP1: u8 = 0x5D;
    pub const SDB: u8 = 0x5E;
    pub const SDS: u8 = 0x5F;
    pub const ADD: u8 = 0x60;
    pub const SUB: u8 = 0x61;
    pub const DIV: u8 = 0x62;
    pub const MUL: u8 = 0x63;
    pub const ABS: u8 = 0x64;
    pub const NEG: u8 = 0x65;
    pub const FLOOR: u8 = 0x66;
    pub const CEILING: u8 = 0x67;
    pub const ROUND: u8 = 0x68;
    pub const NROUND: u8 = 0x6C;
    pub const WCVTF: u8 = 0x70;
    pub const DELTAP2: u8 = 0x71;
    pub const DELTAP3: u8 = 0x72;
    pub const DELTAC1: u8 = 0x73;
    pub const DELTAC2: u8 = 0x74;
    pub const DELTAC3: u8 = 0x75;
    pub const SROUND: u8 = 0x76;
    pub const S45ROUND: u8 = 0x77;
    pub const JROT: u8 = 0x78;
    pub const JROF: u8 = 0x79;
    pub const ROFF: u8 = 0x7A;
    pub const RUTG: u8 = 0x7C;
    pub const RDTG: u8 = 0x7D;
    pub const SANGW: u8 = 0x7E;
    pub const AA: u8 = 0x7F;
    pub const FLIPPT: u8 = 0x80;
    pub const FLIPRGON: u8 = 0x81;
    pub const FLIPRGOFF: u8 = 0x82;
    pub const SCANCTRL: u8 = 0x85;
    pub const SDPVTL: u8 = 0x86;
    pub const GETINFO: u8 = 0x88;
    pub const IDEF: u8 = 0x89;
    pub const ROLL: u8 = 0x8A;
    pub const MAX: u8 = 0x8B;
    pub const MIN: u8 = 0x8C;
    pub const SCANTYPE: u8 = 0x8D;
    pub const INSTCTRL: u8 = 0x8E;
    pub const PUSHB: u8 = 0xB0;
    pub const PUSHW: u8 = 0xB8;
    pub const MDRP: u8 = 0xC0;
    pub const MIRP: u8 = 0xE0;
}

/// The opcodes no instruction of the set has (the *TrueType Reference
/// Manual*'s instruction set summary assigns none of them), with
/// `GETVARIATION`'s 0x91, which this engine does not execute, so a font's
/// required `IDEF` stands in for it.
pub(super) fn undefined_opcodes() -> Vec<u8> {
    let mut opcodes = vec![0x28, 0x7B, 0x83, 0x84];
    opcodes.extend(0x8F..=0xAF);
    opcodes
}

/// The instructions that push `values` in order, the last on top: bytes
/// where every value of a run is one (`PUSHB`, `NPUSHB` past eight values),
/// words otherwise (`PUSHW`, `NPUSHW`). A value must fit a word.
pub(super) fn push(values: &[i32]) -> Vec<u8> {
    let mut code = Vec::new();
    for run in values.chunks(255) {
        let bytes = run.iter().all(|v| (0..=255).contains(v));
        let count = run.len();
        match (bytes, count) {
            (true, 1..=8) => code.push(op::PUSHB + (count - 1) as u8),
            (false, 1..=8) => code.push(op::PUSHW + (count - 1) as u8),
            (true, _) => code.extend([op::NPUSHB, count as u8]),
            (false, _) => code.extend([op::NPUSHW, count as u8]),
        }
        for &value in run {
            if bytes {
                code.push(value as u8);
            } else {
                let word = i16::try_from(value).expect("a pushed value fits a word");
                code.extend(word.to_be_bytes());
            }
        }
    }
    code
}

/// Concatenates pieces of a program.
pub(super) fn program(pieces: &[&[u8]]) -> Vec<u8> {
    pieces.concat()
}

/// The limits the tests' machines have unless a test names its own.
pub(super) fn roomy() -> Limits {
    Limits::new(MaxProfile {
        max_twilight_points: 16,
        max_storage: 64,
        max_function_defs: 64,
        max_instruction_defs: 8,
        max_stack_elements: 1024,
    })
}

/// A machine with [`roomy`] limits.
pub(super) fn machine() -> Vm {
    Vm::new(roomy())
}

/// Runs `code` as a glyph program on a fresh machine, and answers the stack
/// it left, bottom first.
pub(super) fn run(code: &[u8]) -> Result<Vec<i32>, Error> {
    let mut vm = machine();
    vm.run(code, Program::Glyph, None)?;
    Ok(vm.stack().to_vec())
}

/// Runs `code` as a glyph program on a fresh machine whose stack starts as
/// `stack`, bottom first, and answers the stack it left.
pub(super) fn run_on(stack: &[i32], code: &[u8]) -> Result<Vec<i32>, Error> {
    let mut vm = machine();
    vm.run_on(stack, code, Program::Glyph)?;
    Ok(vm.stack().to_vec())
}

/// A small deterministic generator, so a sweep is the same on every run.
pub(super) struct Lcg(pub u64);

impl Lcg {
    /// The next 32 bits.
    pub(super) fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 32) as u32
    }

    /// A value below `n`, which must not be zero.
    pub(super) fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }
}
