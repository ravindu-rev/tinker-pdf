//! FT-04b.2: the push, stack, logic, arithmetic and `DEBUG` instructions, the
//! stack's bound, the budget and the stop predicate.

use std::cell::Cell;

use super::{machine, op, program, push, run, run_on, undefined_opcodes, Lcg};
use crate::hint::vm::{Error, Limits, MaxProfile, Program, Vm, BUDGET, STOP_EVERY};

/// *Pushing data onto the interpreter stack*: a byte is padded to 32 bits
/// unsigned, a word sign-extended, the count byte of `NPUSHB` and `NPUSHW`
/// (0 to 255) is not pushed, and `PUSHB[abc]` and `PUSHW[abc]` push abc + 1.
#[test]
fn every_push_instruction_pushes_its_operands() {
    // Bytes from 0 to 255 and words from -32768 to 32767, each read here as
    // the specification states: a byte as itself, a word high byte first and
    // sign-extended.
    let bytes: Vec<u8> = (0..=255u8).rev().collect();
    let words: Vec<[u8; 2]> = [
        0x0000u16, 0x0001, 0x007F, 0x0080, 0x00FF, 0x7FFF, 0x8000, 0x8001, 0xFFFE, 0xFFFF,
    ]
    .iter()
    .map(|w| w.to_be_bytes())
    .collect();
    let byte_value = |b: u8| i32::from(b);
    let word_value = |w: [u8; 2]| {
        let unsigned = i32::from(w[0]) * 256 + i32::from(w[1]);
        if unsigned >= 0x8000 {
            unsigned - 0x10000
        } else {
            unsigned
        }
    };

    // NPUSHB[ ] at every count from 0 to 255.
    for n in [0usize, 1, 2, 7, 8, 9, 128, 254, 255] {
        let mut code = vec![op::NPUSHB, n as u8];
        code.extend(&bytes[..n]);
        let expected: Vec<i32> = bytes[..n].iter().map(|&b| byte_value(b)).collect();
        assert_eq!(run(&code), Ok(expected), "NPUSHB {n}");
    }
    // NPUSHW[ ].
    for n in [0usize, 1, 3, 10] {
        let mut code = vec![op::NPUSHW, n as u8];
        let chosen: Vec<[u8; 2]> = (0..n).map(|i| words[i % words.len()]).collect();
        for w in &chosen {
            code.extend(w);
        }
        let expected: Vec<i32> = chosen.iter().map(|&w| word_value(w)).collect();
        assert_eq!(run(&code), Ok(expected), "NPUSHW {n}");
    }
    // NPUSHW of 255 words, the most one pushes.
    let mut code = vec![op::NPUSHW, 255];
    let mut expected = Vec::new();
    for i in 0..255u16 {
        let w = (i.wrapping_mul(257) ^ 0x8001).to_be_bytes();
        code.extend(w);
        expected.push(word_value(w));
    }
    assert_eq!(run(&code), Ok(expected), "NPUSHW 255");

    // PUSHB[abc] and PUSHW[abc] for every abc.
    for abc in 0..8u8 {
        let n = usize::from(abc) + 1;
        let mut code = vec![op::PUSHB + abc];
        code.extend(&bytes[100..100 + n]);
        let expected: Vec<i32> = bytes[100..100 + n].iter().map(|&b| byte_value(b)).collect();
        assert_eq!(run(&code), Ok(expected), "PUSHB[{abc:03b}]");

        let mut code = vec![op::PUSHW + abc];
        let chosen: Vec<[u8; 2]> = (0..n)
            .map(|i| words[(i + usize::from(abc)) % words.len()])
            .collect();
        for w in &chosen {
            code.extend(w);
        }
        let expected: Vec<i32> = chosen.iter().map(|&w| word_value(w)).collect();
        assert_eq!(run(&code), Ok(expected), "PUSHW[{abc:03b}]");
    }

    // The data is the stream's: a data byte that spells an opcode is not
    // executed, and the instruction after the data is.
    let code = [op::PUSHB + 1, op::DUP, op::POP, op::DUP];
    assert_eq!(
        run(&code),
        Ok(vec![
            i32::from(op::DUP),
            i32::from(op::POP),
            i32::from(op::POP)
        ]),
        "the pushed bytes are data, and DUP after them runs"
    );
    let code = [op::NPUSHB, 2, op::CLEAR, op::CLEAR, op::PUSHB, 7];
    assert_eq!(
        run(&code),
        Ok(vec![i32::from(op::CLEAR), i32::from(op::CLEAR), 7])
    );

    // The tests' own assembler spells what it means.
    assert_eq!(run(&push(&[1, 2, 300, -4])), Ok(vec![1, 2, 300, -4]));
    let many: Vec<i32> = (0..600).map(|i| i % 256).collect();
    assert_eq!(run(&push(&many)), Ok(many));
}

/// The stack instructions as *Managing the stack* states them, on a model
/// stack, bottom first. `Err` where the instruction cannot do what it says.
fn model(stack: &mut Vec<i32>, opcode: u8) -> Result<(), Error> {
    let pop = |stack: &mut Vec<i32>| stack.pop().ok_or(Error::StackUnderflow);
    match opcode {
        op::DUP => {
            let e = pop(stack)?;
            stack.extend([e, e]);
        }
        op::POP => {
            pop(stack)?;
        }
        op::CLEAR => stack.clear(),
        op::SWAP => {
            let e2 = pop(stack)?;
            let e1 = pop(stack)?;
            stack.extend([e2, e1]);
        }
        op::DEPTH => stack.push(stack.len() as i32),
        op::CINDEX | op::MINDEX => {
            let k = pop(stack)?;
            if k < 1 || k as usize > stack.len() {
                return Err(Error::StackIndex(k));
            }
            // The k-th element, counting the top as the first.
            let at = stack.len() - k as usize;
            let e = if opcode == op::CINDEX {
                stack[at]
            } else {
                stack.remove(at)
            };
            stack.push(e);
        }
        op::ROLL => {
            let a = pop(stack)?;
            let b = pop(stack)?;
            let c = pop(stack)?;
            stack.extend([b, a, c]);
        }
        _ => unreachable!("not a stack instruction"),
    }
    Ok(())
}

#[test]
fn every_stack_instruction_moves_the_stack_as_specified() {
    // The specification's own examples.
    assert_eq!(run_on(&[5], &[op::DUP]), Ok(vec![5, 5]), "DUP");
    assert_eq!(run_on(&[5, 6], &[op::POP]), Ok(vec![5]), "POP");
    assert_eq!(run_on(&[1, 2, 3], &[op::CLEAR]), Ok(vec![]), "CLEAR");
    assert_eq!(run_on(&[1, 2], &[op::SWAP]), Ok(vec![2, 1]), "SWAP");
    // "A stack has three elements. The value 3 is then pushed."
    assert_eq!(
        run_on(&[7, 8, 9], &[op::DEPTH]),
        Ok(vec![7, 8, 9, 3]),
        "DEPTH"
    );
    assert_eq!(run(&[op::DEPTH]), Ok(vec![0]), "DEPTH of an empty stack");
    // "The value 3 is popped from the stack, and four elements, e1 to e4,
    // remain on the stack. An element equal to e3 is pushed": e1 the top.
    let (e4, e3, e2, e1) = (40, 30, 20, 10);
    assert_eq!(
        run_on(&[e4, e3, e2, e1, 3], &[op::CINDEX]),
        Ok(vec![e4, e3, e2, e1, e3]),
        "CINDEX 3"
    );
    // MINDEX 3: "pushed onto a modified stack containing elements e1, e2 and
    // e4".
    assert_eq!(
        run_on(&[e4, e3, e2, e1, 3], &[op::MINDEX]),
        Ok(vec![e4, e2, e1, e3]),
        "MINDEX 3"
    );
    assert_eq!(
        run_on(&[4, 5, 1], &[op::CINDEX]),
        Ok(vec![4, 5, 5]),
        "CINDEX 1 is DUP"
    );
    assert_eq!(
        run_on(&[4, 5, 1], &[op::MINDEX]),
        Ok(vec![4, 5]),
        "MINDEX 1 moves nothing"
    );
    // ROLL: pops a, b, c and pushes b, a, c, "equivalent to MINDEX[ ] 3".
    let (c, b, a) = (1, 2, 3);
    assert_eq!(
        run_on(&[9, c, b, a], &[op::ROLL]),
        Ok(vec![9, b, a, c]),
        "ROLL"
    );
    assert_eq!(
        run_on(&[9, c, b, a], &[op::ROLL]),
        run_on(&[9, c, b, a, 3], &[op::MINDEX]),
        "ROLL is MINDEX 3"
    );

    // A sweep: random programs of stack instructions over random stacks,
    // the interpreter held to the model instruction by instruction.
    let opcodes = [
        op::DUP,
        op::POP,
        op::CLEAR,
        op::SWAP,
        op::DEPTH,
        op::CINDEX,
        op::MINDEX,
        op::ROLL,
    ];
    let mut lcg = Lcg(0x5EED_57AC);
    for case in 0..400 {
        let depth = lcg.below(12) as usize;
        let start: Vec<i32> = (0..depth).map(|_| lcg.next() as i32 % 1000).collect();
        let mut code = Vec::new();
        let mut expected = start.clone();
        let mut outcome = Ok(());
        for _ in 0..lcg.below(10) + 1 {
            let opcode = opcodes[lcg.below(opcodes.len() as u32) as usize];
            if opcode == op::CINDEX || opcode == op::MINDEX {
                // An index from -1 to two past the depth, so some miss.
                let k = lcg.below(expected.len() as u32 + 4) as i32 - 1;
                code.extend(push(&[k.max(0)]));
                code.push(opcode);
                if outcome.is_ok() {
                    expected.push(k.max(0));
                    outcome = model(&mut expected, opcode);
                }
            } else {
                code.push(opcode);
                if outcome.is_ok() {
                    outcome = model(&mut expected, opcode);
                }
            }
        }
        let mut vm = machine();
        let got = vm.run_on(&start, &code, Program::Glyph);
        assert_eq!(got, outcome, "case {case}: {code:02X?} on {start:?}");
        if outcome.is_ok() {
            assert_eq!(
                vm.stack(),
                &expected[..],
                "case {case}: {code:02X?} on {start:?}"
            );
        }
    }
}

/// The 26.6 values the arithmetic and logic sweeps take, the edges of the
/// 32 bits among them.
const OPERANDS: [i32; 26] = [
    0,
    1,
    -1,
    2,
    31,
    32,
    33,
    63,
    64,
    65,
    -63,
    -64,
    -65,
    100,
    -100,
    1000,
    -1000,
    0x4000,
    32767,
    -32768,
    65536,
    1 << 20,
    i32::MAX,
    i32::MIN,
    i32::MAX - 63,
    i32::MIN + 1,
];

/// Each binary instruction's value as its section states it, with `n1` (or
/// `e2`) the value on top and `n2` (or `e1`) the one below it; computed in
/// 128 bits and wrapped to 32, as "no overflow or underflow protection"
/// leaves a result past them.
fn binary(opcode: u8, below: i32, top: i32) -> Option<i32> {
    let (n2, n1) = (i128::from(below), i128::from(top));
    let wrap = |v: i128| v as i32;
    let truth = |b: bool| i32::from(b);
    Some(match opcode {
        // *Logical functions*: "First pops e2, then pops e1" and compares e1
        // with e2.
        op::LT => truth(below < top),
        op::LTEQ => truth(below <= top),
        op::GT => truth(below > top),
        op::GTEQ => truth(below >= top),
        op::EQ => truth(below == top),
        op::NEQ => truth(below != top),
        op::AND => truth(below != 0 && top != 0),
        op::OR => truth(below != 0 || top != 0),
        // *Arithmetic and math instructions*.
        op::ADD => wrap(n2 + n1),
        op::SUB => wrap(n2 - n1),
        op::MAX => below.max(top),
        op::MIN => below.min(top),
        // n1 * n2 in 26.6: the product of two 26.6 values over 64, rounded
        // half away from zero (the design's choice).
        op::MUL => {
            let p = n2 * n1;
            let q = (p.abs() + 32) / 64;
            wrap(if p < 0 { -q } else { q })
        }
        // n2 / n1 in 26.6, truncated: the magnitude's quotient, signed.
        op::DIV => {
            if top == 0 {
                return None;
            }
            let q = (n2.abs() * 64) / n1.abs();
            wrap(if (n2 < 0) != (n1 < 0) { -q } else { q })
        }
        _ => unreachable!("not a binary instruction"),
    })
}

/// Each unary instruction's value as its section states it.
fn unary(opcode: u8, n: i32) -> i32 {
    let wide = i128::from(n);
    match opcode {
        op::NOT => i32::from(n == 0),
        op::ABS => wide.abs() as i32,
        op::NEG => (-wide) as i32,
        // "the greatest integer value less than or equal to n1".
        op::FLOOR => (wide.div_euclid(64) * 64) as i32,
        // "the least integer value greater than or equal to n1".
        op::CEILING => (-((-wide).div_euclid(64) * 64)) as i32,
        _ => unreachable!("not a unary instruction"),
    }
}

#[test]
fn every_logic_and_arithmetic_instruction_computes_its_specified_value() {
    let binaries = [
        op::LT,
        op::LTEQ,
        op::GT,
        op::GTEQ,
        op::EQ,
        op::NEQ,
        op::AND,
        op::OR,
        op::ADD,
        op::SUB,
        op::DIV,
        op::MUL,
        op::MAX,
        op::MIN,
    ];
    for &opcode in &binaries {
        for &below in &OPERANDS {
            for &top in &OPERANDS {
                let got = run_on(&[below, top], &[opcode]);
                match binary(opcode, below, top) {
                    Some(value) => assert_eq!(got, Ok(vec![value]), "0x{opcode:02X} {below} {top}"),
                    None => assert_eq!(
                        got,
                        Err(Error::DivideByZero),
                        "0x{opcode:02X} {below} {top}"
                    ),
                }
            }
        }
    }
    for opcode in [op::NOT, op::ABS, op::NEG, op::FLOOR, op::CEILING] {
        for &n in &OPERANDS {
            assert_eq!(
                run_on(&[n], &[opcode]),
                Ok(vec![unary(opcode, n)]),
                "0x{opcode:02X} {n}"
            );
        }
    }

    // The specification's own examples, in 26.6 where they are fractions.
    let px = |v: f64| (v * 64.0).round() as i32;
    let case = |stack: &[i32], opcode: u8| run_on(stack, &[opcode]).expect("runs");
    // LT: "The values 15 and 20 are popped ... the value 0 (false)": 15 on top.
    assert_eq!(case(&[20, 15], op::LT), vec![0]);
    assert_eq!(case(&[20, 20], op::LTEQ), vec![1]);
    assert_eq!(case(&[20, 20], op::GT), vec![0]);
    assert_eq!(case(&[20, 15], op::GTEQ), vec![1]);
    assert_eq!(case(&[-12, 20], op::EQ), vec![0]);
    assert_eq!(case(&[-12, 20], op::NEQ), vec![1]);
    assert_eq!(case(&[33, 1], op::AND), vec![1]);
    assert_eq!(case(&[0, 1], op::AND), vec![0]);
    assert_eq!(case(&[0, 44], op::OR), vec![1]);
    assert_eq!(case(&[0, 0], op::OR), vec![0]);
    assert_eq!(case(&[px(42.1)], op::NOT), vec![0]);
    assert_eq!(case(&[0], op::NOT), vec![1]);
    assert_eq!(case(&[px(-21.1)], op::ABS), vec![px(21.1)]);
    assert_eq!(case(&[px(14.0)], op::ABS), vec![px(14.0)]);
    assert_eq!(case(&[px(44.66)], op::NEG), vec![-px(44.66)]);
    // FLOOR: 15.8 and 15.3 give 15, -0.8 gives -1.
    assert_eq!(case(&[px(15.8)], op::FLOOR), vec![px(15.0)]);
    assert_eq!(case(&[px(15.3)], op::FLOOR), vec![px(15.0)]);
    assert_eq!(case(&[px(-0.8)], op::FLOOR), vec![px(-1.0)]);
    // CEILING: "the ceiling of 15 is 15, but the ceiling of 15.3 is 16. The
    // ceiling of -0.8 is 0"; 223.0 gives 223.0 and 223.3 gives 224.0.
    assert_eq!(case(&[px(15.0)], op::CEILING), vec![px(15.0)]);
    assert_eq!(case(&[px(15.3)], op::CEILING), vec![px(16.0)]);
    assert_eq!(case(&[px(-0.8)], op::CEILING), vec![0]);
    assert_eq!(case(&[px(223.0)], op::CEILING), vec![px(223.0)]);
    assert_eq!(case(&[px(223.3)], op::CEILING), vec![px(224.0)]);
    // SUB: "subtract a,b would be interpreted as (b-a)", a on top.
    assert_eq!(case(&[px(5.0), px(3.0)], op::SUB), vec![px(2.0)]);
    // MUL and DIV in 26.6: 2 × 3.5 is 7, 1 / 3 truncates to 21/64, and a
    // negative quotient truncates toward zero.
    assert_eq!(case(&[px(2.0), px(3.5)], op::MUL), vec![px(7.0)]);
    assert_eq!(case(&[px(1.0), px(3.0)], op::DIV), vec![21]);
    assert_eq!(case(&[px(-1.0), px(3.0)], op::DIV), vec![-21]);
    assert_eq!(case(&[px(7.0), px(-2.0)], op::DIV), vec![px(-3.5)]);
    // A product's half rounds away from zero: 1/64 × 1/2 is 1/128.
    assert_eq!(case(&[1, 32], op::MUL), vec![1]);
    assert_eq!(case(&[-1, 32], op::MUL), vec![-1]);
    assert_eq!(case(&[1, 31], op::MUL), vec![0]);
    // The sums that leave 32 bits wrap.
    assert_eq!(case(&[i32::MAX, 1], op::ADD), vec![i32::MIN]);
    assert_eq!(case(&[i32::MIN, 1], op::SUB), vec![i32::MAX]);
    assert_eq!(case(&[i32::MIN], op::NEG), vec![i32::MIN]);
    assert_eq!(case(&[i32::MIN], op::ABS), vec![i32::MIN]);

    // DEBUG pops its number and does nothing else.
    assert_eq!(case(&[3, 4], op::DEBUG), vec![3]);
}

#[test]
fn underflow_overflow_a_truncated_push_and_an_unknown_opcode_are_errors() {
    // Each instruction that pops, short of what it pops.
    let needs: [(u8, usize); 26] = [
        (op::DUP, 1),
        (op::POP, 1),
        (op::SWAP, 2),
        (op::CINDEX, 1),
        (op::MINDEX, 1),
        (op::ROLL, 3),
        (op::LT, 2),
        (op::LTEQ, 2),
        (op::GT, 2),
        (op::GTEQ, 2),
        (op::EQ, 2),
        (op::NEQ, 2),
        (op::AND, 2),
        (op::OR, 2),
        (op::NOT, 1),
        (op::ADD, 2),
        (op::SUB, 2),
        (op::DIV, 2),
        (op::MUL, 2),
        (op::ABS, 1),
        (op::NEG, 1),
        (op::FLOOR, 1),
        (op::CEILING, 1),
        (op::MAX, 2),
        (op::MIN, 2),
        (op::DEBUG, 1),
    ];
    for (opcode, count) in needs {
        let short = vec![1; count - 1];
        assert_eq!(
            run_on(&short, &[opcode]),
            Err(Error::StackUnderflow),
            "0x{opcode:02X}"
        );
    }
    // CLEAR and DEPTH pop nothing, so an empty stack is no error for them.
    assert_eq!(run(&[op::CLEAR, op::DEPTH]), Ok(vec![0]));

    // CINDEX and MINDEX name an element from 1 to the depth below k.
    for opcode in [op::CINDEX, op::MINDEX] {
        for k in [0, -1, 3, 4, i32::MAX, i32::MIN] {
            assert_eq!(
                run_on(&[5, 6, k], &[opcode]),
                Err(Error::StackIndex(k)),
                "0x{opcode:02X} {k}"
            );
        }
        assert!(
            run_on(&[5, 6, 2], &[opcode]).is_ok(),
            "0x{opcode:02X} at the depth"
        );
    }

    // The stack holds maxStackElements plus 32, at least 256: a profile of
    // none gives 256 entries.
    let limits = Limits::new(MaxProfile::default());
    assert_eq!(limits.stack(), 256);
    let full = vec![1; 256];
    let mut vm = Vm::new(limits);
    assert_eq!(vm.run_on(&full, &[], Program::Glyph), Ok(()), "256 fit");
    assert_eq!(
        vm.run_on(&full, &push(&[1]), Program::Glyph),
        Err(Error::StackOverflow)
    );
    assert_eq!(
        vm.run_on(&full, &[op::DUP], Program::Glyph),
        Err(Error::StackOverflow)
    );
    assert_eq!(
        vm.run_on(&full, &[op::DEPTH], Program::Glyph),
        Err(Error::StackOverflow)
    );
    assert_eq!(
        vm.run_on(&vec![1; 257], &[], Program::Glyph),
        Err(Error::StackOverflow)
    );
    // NPUSHB of 255 bytes onto two entries short of full stops at the bound.
    let mut code = vec![op::NPUSHB, 255];
    code.extend([0u8; 255]);
    assert_eq!(vm.run(&code, Program::Glyph, None), Ok(()), "255 of 256");
    assert_eq!(
        vm.run_on(&[1, 2], &code, Program::Glyph),
        Err(Error::StackOverflow)
    );
    // SWAP and ROLL at the bound give back what they took.
    assert_eq!(
        vm.run_on(&full, &[op::SWAP, op::ROLL], Program::Glyph),
        Ok(())
    );

    // A push whose data runs past the program's end.
    for code in [
        vec![op::NPUSHB],
        vec![op::NPUSHB, 2, 9],
        vec![op::NPUSHW],
        vec![op::NPUSHW, 1, 9],
        vec![op::NPUSHW, 2, 0, 1, 0],
        vec![op::PUSHB],
        vec![op::PUSHB + 2, 1, 2],
        vec![op::PUSHB + 7, 1, 2, 3, 4, 5, 6, 7],
        vec![op::PUSHW, 1],
        vec![op::PUSHW + 1, 0, 1, 0],
        vec![op::PUSHW + 7],
    ] {
        assert_eq!(run(&code), Err(Error::TruncatedPush), "{code:02X?}");
    }
    // A push that ends exactly at the end is whole.
    assert_eq!(run(&[op::NPUSHB, 0]), Ok(vec![]));
    assert_eq!(run(&[op::PUSHW, 0x12, 0x34]), Ok(vec![0x1234]));

    // An opcode no instruction has.
    for opcode in undefined_opcodes() {
        assert_eq!(
            run(&[opcode]),
            Err(Error::NoInstruction(opcode)),
            "0x{opcode:02X}"
        );
        assert_eq!(
            run(&program(&[&push(&[1, 2]), &[op::ADD, opcode]])),
            Err(Error::NoInstruction(opcode)),
            "0x{opcode:02X} after other instructions"
        );
    }

    // DIV by zero: "The TrueType Rasterizer v.1.7 and later will catch any
    // division-by-zero errors."
    assert_eq!(run_on(&[64, 0], &[op::DIV]), Err(Error::DivideByZero));
    assert_eq!(run_on(&[0, 0], &[op::DIV]), Err(Error::DivideByZero));
}

#[test]
fn a_run_counts_its_steps_and_stops_at_its_budget() {
    // A step an instruction: five instructions are five steps.
    let mut vm = machine();
    let code = program(&[&push(&[1, 2]), &[op::ADD, op::DUP, op::POP, op::DEPTH]]);
    assert_eq!(vm.run(&code, Program::Font, None), Ok(()));
    assert_eq!(vm.steps(), 5);
    assert_eq!(vm.program(), Program::Font);

    // MINDEX spends a step for each element it moves down.
    let mut vm = machine();
    let start: Vec<i32> = (0..100).collect();
    assert_eq!(
        vm.run_on(
            &start,
            &program(&[&push(&[100]), &[op::MINDEX]]),
            Program::Glyph
        ),
        Ok(())
    );
    assert_eq!(
        vm.steps(),
        2 + 100,
        "the push, MINDEX, and the 100 elements"
    );

    // A straight-line program of exactly BUDGET instructions ends; one more
    // instruction is past the budget.
    let pairs = (BUDGET as usize - 1) / 2;
    let mut code = program(&[&push(&[0])]);
    for _ in 0..pairs {
        code.extend([op::DUP, op::POP]);
    }
    code.push(op::DUP);
    assert_eq!(
        code.len() - 1,
        BUDGET as usize,
        "the push is two bytes, one step"
    );
    let mut vm = machine();
    assert_eq!(vm.run(&code, Program::Glyph, None), Ok(()));
    assert_eq!(vm.steps(), BUDGET);
    code.push(op::POP);
    assert_eq!(vm.run(&code, Program::Glyph, None), Err(Error::Budget));
    assert_eq!(vm.steps(), BUDGET + 1);
    // Each run's budget is its own: the next run starts from none.
    assert_eq!(vm.run(&[op::DEPTH], Program::Glyph, None), Ok(()));
    assert_eq!(vm.steps(), 1);
}

#[test]
fn the_stop_predicate_is_asked_every_4096_steps() {
    let long: Vec<u8> = program(&[&push(&[0])])
        .into_iter()
        .chain([op::DUP, op::POP].repeat(5000))
        .collect();
    let steps = 1 + 10_000;

    // Asked at each multiple of STOP_EVERY the run reaches, and never sooner.
    let asked = Cell::new(0u32);
    let count = || {
        asked.set(asked.get() + 1);
        false
    };
    let mut vm = machine();
    assert_eq!(vm.run(&long, Program::Glyph, Some(&count)), Ok(()));
    assert_eq!(asked.get(), steps / STOP_EVERY, "asked at 4096 and 8192");

    // A yes stops the run where it was asked.
    let mut vm = machine();
    assert_eq!(
        vm.run(&long, Program::Glyph, Some(&|| true)),
        Err(Error::Stopped)
    );
    assert_eq!(vm.steps(), STOP_EVERY);

    // A run shorter than STOP_EVERY never asks.
    let asked = Cell::new(false);
    let mark = || {
        asked.set(true);
        true
    };
    let mut vm = machine();
    assert_eq!(
        vm.run(&[op::DEPTH, op::POP], Program::Glyph, Some(&mark)),
        Ok(())
    );
    assert!(!asked.get());
}

#[test]
fn limits_come_from_maxp_with_this_engines_bounds() {
    // A version 1.0 maxp: version, numGlyphs, then the fourteen fields, the
    // five read here at 16, 18, 20, 22 and 24.
    let maxp = |twilight: u16, storage: u16, functions: u16, idefs: u16, stack: u16| {
        let mut t = vec![0, 1, 0, 0, 0, 9];
        for field in [
            0u16, 0, 0, 0, 2, twilight, storage, functions, idefs, stack, 0, 0, 0,
        ] {
            t.extend(field.to_be_bytes());
        }
        t
    };
    let limits = Limits::from_maxp(&maxp(10, 20, 30, 40, 500)).expect("a whole table");
    assert_eq!(limits.twilight(), 10);
    assert_eq!(limits.storage(), 20);
    assert_eq!(limits.functions(), 30);
    assert_eq!(limits.instruction_defs(), 40);
    assert_eq!(limits.stack(), 532, "maxStackElements plus 32");
    assert_eq!(Vm::new(limits).limits(), limits);

    // The bounds a hostile profile meets.
    let hostile =
        Limits::from_maxp(&maxp(u16::MAX, u16::MAX, u16::MAX, u16::MAX, u16::MAX)).expect("whole");
    assert_eq!(hostile.stack(), 65_536, "at most 65 536 entries");
    assert_eq!(hostile.instruction_defs(), 256, "an opcode is a byte");
    assert_eq!(hostile.twilight(), 16_384);
    assert_eq!(hostile.storage(), 65_535);
    assert_eq!(hostile.functions(), 65_535);
    let small = Limits::from_maxp(&maxp(0, 0, 0, 0, 3)).expect("whole");
    assert_eq!(small.stack(), 256, "at least 256 entries");

    // Not version 1.0, or cut short: no limits, so no hinting.
    let mut cff = maxp(1, 1, 1, 1, 1);
    cff[0..4].copy_from_slice(&[0, 0, 0x50, 0]);
    assert_eq!(Limits::from_maxp(&cff), None, "version 0.5");
    assert_eq!(
        Limits::from_maxp(&maxp(1, 1, 1, 1, 1)[..31]),
        None,
        "31 bytes"
    );
    assert_eq!(Limits::from_maxp(&[]), None);
    assert!(
        Limits::from_maxp(&maxp(1, 1, 1, 1, 1)).is_some(),
        "32 bytes"
    );
}
