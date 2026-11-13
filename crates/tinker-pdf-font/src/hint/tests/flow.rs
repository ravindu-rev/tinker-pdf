//! FT-04b.3: `IF`, `ELSE` and `EIF`, the relative jumps, function and
//! instruction definitions, and the bounds a program that loops or recurses
//! meets.

use super::{machine, op, program, push, run, run_on, Lcg};
use crate::hint::vm::{Error, Limits, MaxProfile, Program, Vm, BUDGET, CALL_DEPTH};

/// A machine whose font program `fpgm` has run, as a face's has before its
/// other programs run.
fn defined(fpgm: &[u8]) -> Vm {
    let mut vm = machine();
    assert_eq!(
        vm.run(fpgm, Program::Font, None),
        Ok(()),
        "the font program runs"
    );
    vm
}

/// Runs `code` as a glyph program on `vm`, answering the stack it left.
fn glyph(vm: &mut Vm, code: &[u8]) -> Result<Vec<i32>, Error> {
    vm.run(code, Program::Glyph, None)?;
    Ok(vm.stack().to_vec())
}

/// `FDEF` of function `f` with `body`, as a font program spells it.
fn fdef(f: i32, body: &[u8]) -> Vec<u8> {
    program(&[&push(&[f]), &[op::FDEF], body, &[op::ENDF]])
}

/// `IDEF` of `opcode` with `body`.
fn idef(opcode: i32, body: &[u8]) -> Vec<u8> {
    program(&[&push(&[opcode]), &[op::IDEF], body, &[op::ENDF]])
}

/// A nest of `IF`s, each branch pushing markers.
enum Block {
    /// Pushes the value.
    Push(i32),
    /// `IF`, its branch, and its `ELSE` branch where it has one.
    If {
        /// The value the `IF` tests, pushed before it.
        test: i32,
        then: Vec<Block>,
        otherwise: Option<Vec<Block>>,
    },
}

/// The blocks as instructions.
fn assemble(blocks: &[Block], code: &mut Vec<u8>) {
    for block in blocks {
        match block {
            Block::Push(value) => code.extend(push(&[*value])),
            Block::If {
                test,
                then,
                otherwise,
            } => {
                code.extend(push(&[*test]));
                code.push(op::IF);
                assemble(then, code);
                if let Some(otherwise) = otherwise {
                    code.push(op::ELSE);
                    assemble(otherwise, code);
                }
                code.push(op::EIF);
            }
        }
    }
}

/// What the blocks push, by *IF test*: a nonzero value runs the branch after
/// the `IF` to its `ELSE` or `EIF`, zero the branch after the `ELSE`, or
/// nothing where there is none.
fn evaluate(blocks: &[Block], out: &mut Vec<i32>) {
    for block in blocks {
        match block {
            Block::Push(value) => out.push(*value),
            Block::If {
                test,
                then,
                otherwise,
            } => {
                if *test != 0 {
                    evaluate(then, out);
                } else if let Some(otherwise) = otherwise {
                    evaluate(otherwise, out);
                }
            }
        }
    }
}

/// A random nest at most `depth` deep. Its markers include the bytes that
/// spell `IF`, `ELSE`, `EIF` and `ENDF`, pushed as data a skip must pass over.
fn nest(lcg: &mut Lcg, depth: u32) -> Vec<Block> {
    let markers = [
        i32::from(op::IF),
        i32::from(op::ELSE),
        i32::from(op::EIF),
        i32::from(op::ENDF),
        7,
        -300,
        1000,
    ];
    (0..lcg.below(4))
        .map(|_| {
            if depth == 0 || lcg.below(3) == 0 {
                Block::Push(markers[lcg.below(markers.len() as u32) as usize])
            } else {
                let tests = [0, 0, 1, -1, 64, i32::from(i16::MAX)];
                Block::If {
                    test: tests[lcg.below(tests.len() as u32) as usize],
                    then: nest(lcg, depth - 1),
                    otherwise: (lcg.below(2) == 0).then(|| nest(lcg, depth - 1)),
                }
            }
        })
        .collect()
}

#[test]
fn if_else_eif_take_the_specified_branch_at_any_nesting() {
    // The three cases *IF test* draws.
    let case1 = program(&[
        &push(&[1]),
        &[op::IF],
        &push(&[10]),
        &[op::EIF],
        &push(&[20]),
    ]);
    assert_eq!(run(&case1), Ok(vec![10, 20]), "true, ended by EIF");
    let case2 = program(&[
        &push(&[1]),
        &[op::IF],
        &push(&[10]),
        &[op::ELSE],
        &push(&[11]),
        &[op::EIF],
        &push(&[20]),
    ]);
    assert_eq!(run(&case2), Ok(vec![10, 20]), "true: ELSE jumps to the EIF");
    let case3 = program(&[
        &push(&[0]),
        &[op::IF],
        &push(&[10]),
        &[op::ELSE],
        &push(&[11]),
        &[op::EIF],
        &push(&[20]),
    ]);
    assert_eq!(run(&case3), Ok(vec![11, 20]), "false: from the ELSE");
    let no_else = program(&[
        &push(&[0]),
        &[op::IF],
        &push(&[10]),
        &[op::EIF],
        &push(&[20]),
    ]);
    assert_eq!(
        run(&no_else),
        Ok(vec![20]),
        "false with no ELSE: past the EIF"
    );
    // Any nonzero value is true, the negative ones too.
    for test in [1, -1, 2, i32::from(i16::MIN), i32::from(i16::MAX)] {
        let code = program(&[&push(&[test]), &[op::IF], &push(&[5]), &[op::EIF]]);
        assert_eq!(run(&code), Ok(vec![5]), "{test} is true");
    }

    // Random nests up to eight deep, the interpreter held to the semantics.
    let mut lcg = Lcg(0x1F_E1_5E);
    for case in 0..500 {
        let blocks = nest(&mut lcg, 1 + case % 8);
        let mut code = Vec::new();
        assemble(&blocks, &mut code);
        let mut expected = Vec::new();
        evaluate(&blocks, &mut expected);
        assert_eq!(run(&code), Ok(expected), "case {case}: {code:02X?}");
    }

    // An EIF the program reaches alone marks nothing; an ELSE reached alone
    // skips to its EIF.
    assert_eq!(run(&program(&[&[op::EIF], &push(&[3])])), Ok(vec![3]));
    let lone_else = program(&[&[op::ELSE], &push(&[1]), &[op::EIF], &push(&[2])]);
    assert_eq!(run(&lone_else), Ok(vec![2]));

    // A skip that finds no EIF, or no data where a push needs it.
    let open = program(&[&push(&[0]), &[op::IF], &push(&[1])]);
    assert_eq!(
        run(&open),
        Err(Error::UnterminatedIf),
        "a false IF with no EIF"
    );
    let open_else = program(&[&push(&[1]), &[op::IF], &[op::ELSE], &push(&[1])]);
    assert_eq!(
        run(&open_else),
        Err(Error::UnterminatedIf),
        "an ELSE with no EIF"
    );
    let nested_open = program(&[&push(&[0]), &[op::IF, op::IF, op::EIF]]);
    assert_eq!(
        run(&nested_open),
        Err(Error::UnterminatedIf),
        "the inner EIF is the inner IF's"
    );
    let cut = vec![op::PUSHB, 0, op::IF, op::NPUSHB, 3, 1];
    assert_eq!(
        run(&cut),
        Err(Error::TruncatedPush),
        "a skip reads the push lengths"
    );
    assert_eq!(run(&[op::IF]), Err(Error::StackUnderflow));
}

#[test]
fn jumps_move_by_their_signed_offsets() {
    // JMPR at 2 by 3 lands at 5, past the push at 3: "The jump is relative
    // to the position of the instruction itself."
    let code = program(&[&push(&[3]), &[op::JMPR], &push(&[11]), &push(&[22])]);
    assert_eq!(code[5], op::PUSHB);
    assert_eq!(run(&code), Ok(vec![22]));
    // "an offset of +1 causes the instruction immediately following the
    // JMPR[] instruction to be executed".
    let code = program(&[&push(&[1]), &[op::JMPR], &push(&[11]), &push(&[22])]);
    assert_eq!(run(&code), Ok(vec![11, 22]));
    // Landing on the end ends the program; past either end is an error.
    let code = program(&[&push(&[1]), &[op::JMPR]]);
    assert_eq!(run(&code), Ok(vec![]), "the end");
    let code = program(&[&push(&[2]), &[op::JMPR]]);
    assert_eq!(run(&code), Err(Error::JumpOutOfRange), "one past the end");
    let code = program(&[&push(&[-4]), &[op::JMPR]]);
    assert_eq!(code.len(), 4);
    assert_eq!(run(&code), Err(Error::JumpOutOfRange), "before the start");
    let code = program(&[&push(&[-3]), &[op::JMPR]]);
    assert_eq!(code[3], op::JMPR);
    assert_eq!(
        run(&code),
        Err(Error::Budget),
        "-3 lands on the push again, forever"
    );

    // A countdown: each pass subtracts one, keeps the count, and JROT jumps
    // back to the subtraction while it is nonzero. JROT pops e, then the
    // offset, so the offset is pushed first and the count copied above it.
    let start = 5;
    let mut code = program(&[&push(&[start])]);
    let top = code.len();
    code.extend(program(&[&push(&[1]), &[op::SUB, op::DUP]]));
    let jrot_at = code.len() + 3 + 2 + 1;
    let back = top as i32 - jrot_at as i32;
    code.extend(program(&[
        &push(&[back]),
        &push(&[2]),
        &[op::CINDEX, op::JROT],
    ]));
    assert_eq!(code[jrot_at], op::JROT);
    // The counts kept, from start - 1 to 0, and the last one's copy.
    let mut expected: Vec<i32> = (0..start).rev().collect();
    expected.push(0);
    assert_eq!(run(&code), Ok(expected));

    // JROT jumps on true and JROF on false, each by the offset below the
    // value it tests; neither jumps otherwise.
    for (opcode, e, jumps) in [
        (op::JROT, 1, true),
        (op::JROT, -9, true),
        (op::JROT, 0, false),
        (op::JROF, 0, true),
        (op::JROF, 1, false),
        (op::JROF, -9, false),
    ] {
        // The offset and the value as two words, so the jump stands at 5;
        // by 3 it lands at 8, past the push at 6.
        let [high, low] = (e as i16).to_be_bytes();
        let code = program(&[
            &[op::PUSHW + 1, 0, 3, high, low],
            &[opcode],
            &push(&[11]),
            &push(&[22]),
        ]);
        assert_eq!(code[5], opcode);
        let expected = if jumps { vec![22] } else { vec![11, 22] };
        assert_eq!(run(&code), Ok(expected), "0x{opcode:02X} on {e}");
        assert_eq!(
            run_on(&[3], &[opcode]),
            Err(Error::StackUnderflow),
            "0x{opcode:02X} with no offset"
        );
        let out = program(&[
            &push(&[-100, if opcode == op::JROT { 1 } else { 0 }]),
            &[opcode],
        ]);
        assert_eq!(
            run(&out),
            Err(Error::JumpOutOfRange),
            "0x{opcode:02X} out of range"
        );
        let stays = program(&[
            &push(&[-100, if opcode == op::JROT { 0 } else { 1 }]),
            &[opcode],
        ]);
        assert_eq!(
            run(&stays),
            Ok(vec![]),
            "0x{opcode:02X} not taken checks no offset"
        );
    }

    // A jump may land inside push data, which then runs as instructions:
    // JMPR at 2 by 3 lands on the second byte PUSHB[001] at 3 pushes, 0x24,
    // which is DEPTH.
    let code = vec![op::PUSHB, 3, op::JMPR, op::PUSHB + 1, 0x99, op::DEPTH];
    assert_eq!(run(&code), Ok(vec![0]));
}

#[test]
fn fdef_call_and_loopcall_run_their_bodies_as_specified() {
    // FDEF keeps its body without running it, and CALL runs it.
    let fpgm = program(&[&fdef(0, &push(&[7])), &fdef(1, &[op::DUP, op::ADD])]);
    let mut vm = defined(&fpgm);
    assert_eq!(vm.stack(), &[] as &[i32], "a definition runs nothing");
    assert_eq!(
        glyph(&mut vm, &program(&[&push(&[0]), &[op::CALL]])),
        Ok(vec![7])
    );
    assert_eq!(
        glyph(&mut vm, &program(&[&push(&[5, 1]), &[op::CALL]])),
        Ok(vec![10])
    );

    // *LOOP and CALL function*'s example: count 5, then function 17, "call
    // function 17, 5 times". Each call here pushes the depth it finds.
    let fpgm = program(&[
        &fdef(17, &[op::DEPTH]),
        &push(&[5]),
        &push(&[17]),
        &[op::LOOPCALL],
    ]);
    let vm = defined(&fpgm);
    assert_eq!(vm.stack(), &[0, 1, 2, 3, 4]);
    // A doubling five times over is 32 times.
    let mut vm = defined(&fdef(1, &[op::DUP, op::ADD]));
    assert_eq!(
        glyph(&mut vm, &program(&[&push(&[3, 5, 1]), &[op::LOOPCALL]])),
        Ok(vec![96])
    );
    // A count below one calls nothing.
    for count in [0, -1, i32::from(i16::MIN)] {
        let code = program(&[&push(&[3, count, 1]), &[op::LOOPCALL]]);
        assert_eq!(glyph(&mut vm, &code), Ok(vec![3]), "count {count}");
    }

    // Definitions last from run to run: one fpgm made is called from prep,
    // one prep made from a glyph, and a function calls a function.
    let mut vm = defined(&fdef(0, &push(&[1])));
    let prep = program(&[
        &fdef(2, &program(&[&push(&[0]), &[op::CALL], &push(&[2])])),
        &push(&[0]),
        &[op::CALL],
    ]);
    assert_eq!(vm.run(&prep, Program::ControlValue, None), Ok(()));
    assert_eq!(vm.stack(), &[1]);
    assert_eq!(
        glyph(&mut vm, &program(&[&push(&[2]), &[op::CALL]])),
        Ok(vec![1, 2])
    );
    // A later FDEF of a number replaces the earlier one.
    assert_eq!(
        vm.run(&fdef(0, &push(&[9])), Program::ControlValue, None),
        Ok(())
    );
    assert_eq!(
        glyph(&mut vm, &program(&[&push(&[2]), &[op::CALL]])),
        Ok(vec![9, 2])
    );
    // A clone shares the definitions until it defines its own.
    let mut copy = vm.clone();
    assert_eq!(
        copy.run(&fdef(0, &push(&[4])), Program::ControlValue, None),
        Ok(())
    );
    assert_eq!(
        glyph(&mut copy, &program(&[&push(&[0]), &[op::CALL]])),
        Ok(vec![4])
    );
    assert_eq!(
        glyph(&mut vm, &program(&[&push(&[0]), &[op::CALL]])),
        Ok(vec![9])
    );

    // The body ends at the first ENDF in the instruction stream, push data
    // passed over: 0x2D pushed as a byte is not one.
    let mut vm = defined(&fdef(3, &push(&[i32::from(op::ENDF)])));
    assert_eq!(
        glyph(&mut vm, &program(&[&push(&[3]), &[op::CALL]])),
        Ok(vec![0x2D])
    );
    // A jump inside a function is relative to it; landing on its end returns.
    let body = program(&[&push(&[3]), &[op::JMPR], &push(&[1]), &push(&[2])]);
    let mut vm = defined(&fdef(4, &body));
    assert_eq!(
        glyph(&mut vm, &program(&[&push(&[4]), &[op::CALL]])),
        Ok(vec![2])
    );
    let leaves = program(&[&push(&[7]), &[op::JMPR]]);
    let mut vm = defined(&fdef(5, &leaves));
    assert_eq!(
        glyph(
            &mut vm,
            &program(&[&push(&[5]), &[op::CALL], &push(&[1]), &push(&[2])])
        ),
        Err(Error::JumpOutOfRange),
        "a jump past the function's own bytes"
    );

    // Where a definition may not stand, and what a call may not name.
    let mut vm = machine();
    assert_eq!(
        vm.run(&fdef(0, &[]), Program::Glyph, None),
        Err(Error::DefinitionInGlyphProgram),
        "FDEF in a glyph program"
    );
    assert_eq!(
        vm.run(&fdef(0, &[]), Program::ControlValue, None),
        Ok(()),
        "FDEF in prep"
    );
    // Function numbers run from 0 to maxFunctionDefs - 1: 64 here.
    assert_eq!(
        run(&fdef(64, &[])).map(|_| ()),
        Err(Error::DefinitionInGlyphProgram)
    );
    let mut vm = machine();
    assert_eq!(
        vm.run(&fdef(64, &[]), Program::Font, None),
        Err(Error::FunctionIndex(64))
    );
    assert_eq!(
        vm.run(&fdef(-1, &[]), Program::Font, None),
        Err(Error::FunctionIndex(-1))
    );
    assert_eq!(vm.run(&fdef(63, &[]), Program::Font, None), Ok(()));
    let calls = [
        (64, Error::FunctionIndex(64)),
        (-1, Error::FunctionIndex(-1)),
        (62, Error::UndefinedFunction(62)),
    ];
    for (f, error) in calls {
        assert_eq!(
            glyph(&mut vm, &program(&[&push(&[f]), &[op::CALL]])),
            Err(error),
            "CALL {f}"
        );
        let code = program(&[&push(&[2, f]), &[op::LOOPCALL]]);
        assert_eq!(glyph(&mut vm, &code), Err(error), "LOOPCALL {f}");
        let code = program(&[&push(&[0, f]), &[op::LOOPCALL]]);
        assert_eq!(
            glyph(&mut vm, &code),
            Err(error),
            "LOOPCALL {f} no times names it still"
        );
    }
    assert_eq!(glyph(&mut vm, &[op::CALL]), Err(Error::StackUnderflow));
    assert_eq!(
        glyph(&mut vm, &program(&[&push(&[63]), &[op::LOOPCALL]])),
        Err(Error::StackUnderflow)
    );
    // An FDEF with no ENDF, an ENDF with no definition, a definition run
    // inside a definition.
    let mut vm = machine();
    let open = program(&[&push(&[0]), &[op::FDEF], &push(&[3])]);
    assert_eq!(
        vm.run(&open, Program::Font, None),
        Err(Error::UnterminatedDefinition)
    );
    let cut = vec![op::PUSHB, 0, op::FDEF, op::PUSHB + 1, 3];
    assert_eq!(vm.run(&cut, Program::Font, None), Err(Error::TruncatedPush));
    assert_eq!(
        vm.run(&[op::ENDF], Program::Font, None),
        Err(Error::EndfOutsideDefinition)
    );
    assert_eq!(
        vm.run(&[op::FDEF], Program::Font, None),
        Err(Error::StackUnderflow)
    );
    let mut vm = defined(&program(&[
        &fdef(0, &program(&[&push(&[1]), &[op::FDEF]])),
        &fdef(1, &program(&[&push(&[0x28]), &[op::IDEF]])),
    ]));
    assert_eq!(
        vm.run(&program(&[&push(&[0]), &[op::CALL]]), Program::Font, None),
        Err(Error::NestedDefinition)
    );
    assert_eq!(
        vm.run(
            &program(&[&push(&[1]), &[op::CALL]]),
            Program::ControlValue,
            None
        ),
        Err(Error::NestedDefinition)
    );

    // LOOPCALL spends a step a call, beside the calls' own instructions.
    let mut vm = defined(&fdef(0, &[]));
    assert_eq!(
        vm.run(
            &program(&[&push(&[10, 0]), &[op::LOOPCALL]]),
            Program::Glyph,
            None
        ),
        Ok(())
    );
    assert_eq!(
        vm.steps(),
        1 + 1 + 10,
        "the push, LOOPCALL and ten calls of an empty body"
    );
}

#[test]
fn idef_defines_an_unused_opcode() {
    // 0x28 has no instruction; an IDEF gives it one, and the glyph's 0x28
    // runs it.
    let mut vm = defined(&idef(0x28, &push(&[42])));
    assert_eq!(glyph(&mut vm, &[0x28, 0x28]), Ok(vec![42, 42]));
    // *GET VARIATION*'s own example: the IDEF a font that uses the opcode
    // must carry, pushing the default zero for each of two axes; and "the
    // last one executed will be used".
    let zeros = program(&[&[op::NPUSHW, 2, 0, 0, 0, 0]]);
    let instance = program(&[&[op::NPUSHW, 2, 0xC0, 0x00, 0, 0]]);
    let mut vm = defined(&idef(145, &zeros));
    assert_eq!(glyph(&mut vm, &[0x91]), Ok(vec![0, 0]));
    assert_eq!(
        vm.run(&idef(145, &instance), Program::ControlValue, None),
        Ok(()),
        "an IDEF in prep"
    );
    assert_eq!(glyph(&mut vm, &[0x91]), Ok(vec![-16384, 0]));

    // "If the opcode in question is already defined, the interpreter will
    // ignore the IDEF": DUP still duplicates, and the body was passed over.
    let mut vm = defined(&idef(i32::from(op::DUP), &push(&[99])));
    assert_eq!(vm.stack(), &[] as &[i32]);
    assert_eq!(
        glyph(&mut vm, &program(&[&push(&[5]), &[op::DUP]])),
        Ok(vec![5, 5])
    );
    // Every opcode the set defines is ignored so, and even with no IDEF
    // allowed at all.
    let none = Limits::new(MaxProfile {
        max_function_defs: 8,
        max_instruction_defs: 0,
        ..MaxProfile::default()
    });
    let mut vm = Vm::new(none);
    let undefined = super::undefined_opcodes();
    for opcode in 0..=255u8 {
        let result = vm.run(&idef(i32::from(opcode), &push(&[1])), Program::Font, None);
        if undefined.contains(&opcode) {
            assert_eq!(result, Err(Error::InstructionDefinitions), "0x{opcode:02X}");
        } else {
            assert_eq!(result, Ok(()), "0x{opcode:02X} is defined, so ignored");
        }
    }

    // maxInstructionDefs bounds how many opcodes may be defined: eight here.
    // Redefining one already defined is not another.
    let mut vm = machine();
    let opcodes = [0x28, 0x7B, 0x83, 0x84, 0x8F, 0x90, 0x91, 0x92];
    for (i, &opcode) in opcodes.iter().enumerate() {
        let body = push(&[i as i32]);
        assert_eq!(
            vm.run(&idef(opcode, &body), Program::Font, None),
            Ok(()),
            "IDEF 0x{opcode:02X}"
        );
    }
    assert_eq!(
        vm.run(&idef(0x28, &push(&[50])), Program::Font, None),
        Ok(()),
        "a redefinition"
    );
    assert_eq!(
        vm.run(&idef(0x93, &[]), Program::Font, None),
        Err(Error::InstructionDefinitions)
    );
    let every: Vec<u8> = opcodes.iter().map(|&o| o as u8).collect();
    assert_eq!(glyph(&mut vm, &every), Ok(vec![50, 1, 2, 3, 4, 5, 6, 7]));

    // An opcode with neither an instruction nor an IDEF; an IDEF where none
    // may stand; a value that is no opcode.
    assert_eq!(glyph(&mut vm, &[0x93]), Err(Error::NoInstruction(0x93)));
    assert_eq!(
        vm.run(&idef(0x94, &[]), Program::Glyph, None),
        Err(Error::DefinitionInGlyphProgram)
    );
    for n in [256, -1, i32::from(i16::MAX)] {
        assert_eq!(
            vm.run(&idef(n, &[]), Program::Font, None),
            Err(Error::OpcodeOutOfRange(n)),
            "{n}"
        );
    }
    let open = program(&[&push(&[0x28]), &[op::IDEF], &push(&[1])]);
    assert_eq!(
        vm.run(&open, Program::Font, None),
        Err(Error::UnterminatedDefinition)
    );

    // A defined instruction runs as a call: it may call, and it nests.
    let fpgm = program(&[
        &fdef(0, &push(&[3])),
        &idef(0x28, &program(&[&push(&[0]), &[op::CALL], &push(&[4])])),
    ]);
    let mut vm = defined(&fpgm);
    assert_eq!(glyph(&mut vm, &[0x28]), Ok(vec![3, 4]));
}

#[test]
fn a_program_that_never_ends_stops_at_its_budget() {
    // JMPR by -3 back to the push before it, forever.
    let forever = program(&[&push(&[-3]), &[op::JMPR]]);
    let mut vm = machine();
    assert_eq!(vm.run(&forever, Program::Glyph, None), Err(Error::Budget));
    assert_eq!(vm.steps(), BUDGET + 1);
    // The same loop inside a function, called from prep.
    let mut vm = defined(&fdef(0, &forever));
    let call = program(&[&push(&[0]), &[op::CALL]]);
    assert_eq!(
        vm.run(&call, Program::ControlValue, None),
        Err(Error::Budget)
    );
    // LOOPCALL of an empty function the most times a word counts, nested
    // three deep, asks for 2^45 calls.
    let fpgm = program(&[
        &fdef(0, &[]),
        &fdef(
            1,
            &program(&[&push(&[i32::from(i16::MAX), 0]), &[op::LOOPCALL]]),
        ),
        &fdef(
            2,
            &program(&[&push(&[i32::from(i16::MAX), 1]), &[op::LOOPCALL]]),
        ),
    ]);
    let mut vm = defined(&fpgm);
    let code = program(&[&push(&[i32::from(i16::MAX), 2]), &[op::LOOPCALL]]);
    assert_eq!(vm.run(&code, Program::Glyph, None), Err(Error::Budget));

    // What a skip or a definition passes over is spent: an IF skipping n
    // instructions and its EIF, an FDEF passing n and its ENDF.
    let n = 1000;
    let skipped = program(&[&push(&[0]), &[op::IF], &vec![op::POP; n], &[op::EIF]]);
    let mut vm = machine();
    assert_eq!(vm.run(&skipped, Program::Glyph, None), Ok(()));
    assert_eq!(vm.steps() as usize, 2 + n + 1);
    let mut vm = machine();
    assert_eq!(
        vm.run(&fdef(0, &vec![op::POP; n]), Program::Font, None),
        Ok(())
    );
    assert_eq!(vm.steps() as usize, 2 + n + 1);
    // So a loop around a long skip stops at the budget too.
    let mut code = program(&[&push(&[0]), &[op::IF], &vec![op::POP; 5000], &[op::EIF]]);
    let back = -(code.len() as i32) - 3;
    code.extend(program(&[&push(&[back]), &[op::JMPR]]));
    assert_eq!(code[code.len() - 1], op::JMPR);
    let mut vm = machine();
    assert_eq!(vm.run(&code, Program::Glyph, None), Err(Error::Budget));
}

#[test]
fn recursion_past_the_call_depth_is_an_error() {
    // A function that calls itself.
    let mut vm = defined(&fdef(0, &program(&[&push(&[0]), &[op::CALL]])));
    assert_eq!(
        glyph(&mut vm, &program(&[&push(&[0]), &[op::CALL]])),
        Err(Error::CallDepth)
    );
    // LOOPCALL nests as CALL does.
    let mut vm = defined(&fdef(0, &program(&[&push(&[1, 0]), &[op::LOOPCALL]])));
    assert_eq!(
        glyph(&mut vm, &program(&[&push(&[0]), &[op::CALL]])),
        Err(Error::CallDepth)
    );
    // And so does an instruction an IDEF defines.
    let mut vm = defined(&idef(0x28, &[0x28]));
    assert_eq!(glyph(&mut vm, &[0x28]), Err(Error::CallDepth));

    // A chain of calls CALL_DEPTH deep runs; one deeper does not. Function i
    // calls function i + 1, and the last pushes a marker.
    let limits = Limits::new(MaxProfile {
        max_function_defs: 100,
        ..MaxProfile::default()
    });
    let chain = |length: u32| {
        let mut fpgm = Vec::new();
        for i in 0..length - 1 {
            fpgm.extend(fdef(
                i as i32,
                &program(&[&push(&[i as i32 + 1]), &[op::CALL]]),
            ));
        }
        fpgm.extend(fdef(length as i32 - 1, &push(&[77])));
        let mut vm = Vm::new(limits);
        assert_eq!(vm.run(&fpgm, Program::Font, None), Ok(()));
        vm.run(&program(&[&push(&[0]), &[op::CALL]]), Program::Glyph, None)
            .map(|()| vm.stack().to_vec())
    };
    assert_eq!(chain(CALL_DEPTH), Ok(vec![77]), "{CALL_DEPTH} deep");
    assert_eq!(chain(CALL_DEPTH + 1), Err(Error::CallDepth), "one deeper");
}
