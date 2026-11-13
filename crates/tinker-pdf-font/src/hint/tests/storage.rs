//! FT-04b.4: the storage area, the control value table scaled to the size,
//! `MPPEM` and `MPS`, `GETINFO`'s selectors, `INSTCTRL`, and an index past
//! either table.

use super::{machine, op, program, push, roomy};
use crate::hint::storage::{Size, VERSION};
use crate::hint::vm::{Error, Limits, MaxProfile, Program, Vm};

/// A `cvt ` table of these FWORDs, big-endian as the font file spells them.
fn cvt_table(fwords: &[i16]) -> Vec<u8> {
    fwords.iter().flat_map(|v| v.to_be_bytes()).collect()
}

/// A machine at `size` whose control value table is `fwords`.
fn sized(size: Size, fwords: &[i16]) -> Vm {
    let mut vm = machine();
    vm.set_size(size);
    vm.load_cvt(&cvt_table(fwords));
    vm
}

/// `v` font units at `ppem` (26.6) in an em of `upem` units, in 26.6: `v ·
/// ppem / upem`, its magnitude rounded half up so the value rounds half away
/// from zero, then wrapped to 32 bits.
fn scaled(v: i32, ppem: i32, upem: u16) -> i32 {
    if upem == 0 {
        return 0;
    }
    let product = i128::from(v) * i128::from(ppem);
    let em = i128::from(upem);
    let magnitude = (2 * product.abs() + em) / (2 * em);
    (if product < 0 { -magnitude } else { magnitude }) as i32
}

/// Runs `code` as `program` on `vm`, answering the stack it left.
fn on(vm: &mut Vm, code: &[u8], program: Program) -> Result<Vec<i32>, Error> {
    vm.run(code, program, None)?;
    Ok(vm.stack().to_vec())
}

#[test]
fn storage_reads_back_what_was_written() {
    // Every location of the 64 maxStorage gives, written and read back:
    // WS pops a value, then a location; RS pops a location.
    let mut vm = machine();
    assert_eq!(vm.storage().len(), 64);
    let value = |i: i32| (i * 37 - 1000) * if i % 2 == 0 { 1 } else { -1 };
    for i in 0..64 {
        let code = program(&[&push(&[i, value(i)]), &[op::WS]]);
        assert_eq!(on(&mut vm, &code, Program::Glyph), Ok(vec![]), "WS {i}");
    }
    for i in 0..64 {
        let code = program(&[&push(&[i]), &[op::RS]]);
        assert_eq!(
            on(&mut vm, &code, Program::Glyph),
            Ok(vec![value(i)]),
            "RS {i}"
        );
    }
    assert_eq!(vm.storage(), &(0..64).map(value).collect::<Vec<_>>()[..]);

    // A location never written reads zero (the specification's
    // "unpredictable results", answered by this engine).
    let mut fresh = machine();
    let code = program(&[&push(&[17]), &[op::RS]]);
    assert_eq!(on(&mut fresh, &code, Program::Font), Ok(vec![0]));

    // Any 32-bit value is kept as it was written.
    for v in [i32::MIN, i32::MAX, -1, 0, 1 << 20] {
        let mut vm = machine();
        assert_eq!(
            vm.run_on(
                &[9, v],
                &program(&[&[op::WS], &push(&[9]), &[op::RS]]),
                Program::Glyph
            ),
            Ok(())
        );
        assert_eq!(vm.stack(), &[v]);
    }

    // *Managing Anti-Aliasing*'s sample writes storage 2: "PUSHB[2] 2 0 WS",
    // the value on top.
    let mut vm = machine();
    assert_eq!(
        on(
            &mut vm,
            &[op::PUSHB + 1, 2, 0, op::WS],
            Program::ControlValue
        ),
        Ok(vec![])
    );
    assert_eq!(vm.storage()[2], 0);
    assert_eq!(
        on(
            &mut vm,
            &[op::PUSHB + 1, 2, 1, op::WS],
            Program::ControlValue
        ),
        Ok(vec![])
    );
    assert_eq!(vm.storage()[2], 1);
    // What a run wrote, a later run of the same machine reads.
    let code = program(&[&push(&[2]), &[op::RS]]);
    assert_eq!(on(&mut vm, &code, Program::Glyph), Ok(vec![1]));
}

#[test]
fn the_cvt_is_scaled_to_the_size_and_written_in_pixels_or_font_units() {
    let fwords = [
        0i16,
        1,
        -1,
        2,
        100,
        -100,
        511,
        512,
        513,
        1000,
        1234,
        2048,
        -2049,
        i16::MAX,
        i16::MIN,
    ];
    let sizes = [
        (12 * 64, 2048),
        (9 * 64 + 32, 1000),
        (100 * 64, 1000),
        (64, 16),
        (1024 * 64, 16),
        (13 * 64 + 7, 2048),
        (1024 * 64, 16_384),
        (0, 1000),
    ];
    for (ppem, upem) in sizes {
        let size = Size {
            ppem,
            points: 0,
            units_per_em: upem,
            monochrome: false,
        };
        let mut vm = sized(size, &fwords);
        let expected: Vec<i32> = fwords
            .iter()
            .map(|&v| scaled(i32::from(v), ppem, upem))
            .collect();
        assert_eq!(vm.cvt(), &expected[..], "{ppem}/64 px in {upem}");
        // RCVT pops a location and pushes its value, in pixels.
        for (i, &value) in expected.iter().enumerate() {
            let code = program(&[&push(&[i as i32]), &[op::RCVT]]);
            assert_eq!(
                on(&mut vm, &code, Program::Glyph),
                Ok(vec![value]),
                "RCVT {i}"
            );
        }
        // WCVTF writes font units, scaled as the table was.
        for v in [7, -7, 3000, -32768, 32767] {
            let code = program(&[&push(&[4, v]), &[op::WCVTF], &push(&[4]), &[op::RCVT]]);
            assert_eq!(
                on(&mut vm, &code, Program::Glyph),
                Ok(vec![scaled(v, ppem, upem)]),
                "WCVTF {v}"
            );
        }
        assert_eq!(size.scale(1000), scaled(1000, ppem, upem));
    }

    // WCVTP writes pixels unchanged: "The value is written to the CVT table
    // unchanged." The value is on top, the location below it.
    let mut vm = sized(
        Size {
            ppem: 12 * 64,
            points: 0,
            units_per_em: 2048,
            monochrome: false,
        },
        &[10, 20, 30],
    );
    for v in [300, -300, 0, 1] {
        let code = program(&[&push(&[2, v]), &[op::WCVTP], &push(&[2]), &[op::RCVT]]);
        assert_eq!(on(&mut vm, &code, Program::Glyph), Ok(vec![v]), "WCVTP {v}");
    }
    for v in [i32::MIN, i32::MAX] {
        let code = program(&[&[op::WCVTP], &push(&[1]), &[op::RCVT]]);
        assert_eq!(vm.run_on(&[1, v], &code, Program::Glyph), Ok(()));
        assert_eq!(vm.stack(), &[v], "WCVTP {v}");
    }
    // A value of font units past the 32 bits at this size wraps.
    let big = Size {
        ppem: 1024 * 64,
        points: 0,
        units_per_em: 16,
        monochrome: false,
    };
    let mut vm = sized(big, &[0]);
    assert_eq!(
        vm.run_on(
            &[0, i32::MAX],
            &program(&[&[op::WCVTF], &push(&[0]), &[op::RCVT]]),
            Program::Glyph
        ),
        Ok(())
    );
    assert_eq!(vm.stack(), &[scaled(i32::MAX, 1024 * 64, 16)]);

    // A trailing odd byte is no entry; a size set after the load keeps the
    // values the load scaled; a face that states no units per em scales
    // everything to zero.
    let mut vm = machine();
    vm.set_size(Size {
        ppem: 64,
        points: 0,
        units_per_em: 64,
        monochrome: false,
    });
    let mut odd = cvt_table(&[5, 6]);
    odd.push(0x7F);
    vm.load_cvt(&odd);
    assert_eq!(vm.cvt(), &[5, 6]);
    vm.set_size(Size::default());
    assert_eq!(vm.cvt(), &[5, 6]);
    assert_eq!(Size::default().scale(1000), 0);
    let no_em = Size {
        ppem: 12 * 64,
        points: 0,
        units_per_em: 0,
        monochrome: false,
    };
    assert_eq!(no_em.scale(1000), 0);
}

#[test]
fn mppem_and_mps_report_the_size() {
    // MPPEM: the pixels per em, "always an integer", ppem rounded to the
    // nearest whole pixel; MPS: the point size in 26.6.
    for (ppem, points) in [
        (12 * 64, 9 * 64),
        (12 * 64 + 31, 9 * 64 + 1),
        (12 * 64 + 32, 9 * 64 + 32),
        (64, 48),
        (1024 * 64, 768 * 64),
        (17 * 64 + 63, 0),
        (0, 0),
    ] {
        let mut vm = machine();
        vm.set_size(Size {
            ppem,
            points,
            units_per_em: 2048,
            monochrome: true,
        });
        let whole = (ppem + 32) / 64;
        assert_eq!(
            on(&mut vm, &[op::MPPEM], Program::ControlValue),
            Ok(vec![whole]),
            "{ppem}/64"
        );
        assert_eq!(
            on(&mut vm, &[op::MPS], Program::Glyph),
            Ok(vec![points]),
            "{points}/64 pt"
        );
        assert_eq!(vm.size().pixels_per_em(), whole);
    }
    // A machine given no size answers zero for both.
    let mut vm = machine();
    assert_eq!(
        on(&mut vm, &[op::MPPEM, op::MPS], Program::Glyph),
        Ok(vec![0, 0])
    );
}

#[test]
fn getinfo_answers_each_selector_as_specified() {
    let at = |monochrome: bool| Size {
        ppem: 12 * 64,
        points: 12 * 64,
        units_per_em: 1000,
        monochrome,
    };
    // *GET INFOrmation*'s table, answered by this engine: version 35 in bits
    // 0 to 7, never rotated (bit 8) or stretched (bit 9), no variations (bit
    // 10), the phantom points kept (bit 11), greyscale (bit 12) unless the
    // glyph is drawn whole or empty, and no ClearType (bits 13 to 19).
    let answer = |selector: i32, monochrome: bool| {
        let mut result = 0;
        if selector & 0x1 != 0 {
            result |= 35;
        }
        if selector & 0x10 != 0 {
            result |= 0x800;
        }
        if selector & 0x20 != 0 && !monochrome {
            result |= 0x1000;
        }
        result
    };
    assert_eq!(VERSION, 35);
    for monochrome in [false, true] {
        let mut vm = machine();
        vm.set_size(at(monochrome));
        // Each selector bit alone, the reserved ones included, then bits
        // asked for together, which are OR'd: "a selector value of 6 requests
        // both information on rotation and stretching", neither true here.
        let alone = (0..32).map(|bit| 1i32 << bit);
        for selector in alone.chain([0, 6, 0x3F, 0x31, 0x1FFF, 0x7FFF, -1]) {
            assert_eq!(
                vm.run_on(&[selector], &[op::GETINFO], Program::Glyph),
                Ok(())
            );
            assert_eq!(
                vm.stack(),
                &[answer(selector, monochrome)],
                "selector {selector:#x}, monochrome {monochrome}"
            );
        }
        // Spelt with a push, as a font asks: the version alone.
        assert_eq!(
            on(
                &mut vm,
                &program(&[&push(&[1]), &[op::GETINFO]]),
                Program::Glyph
            ),
            Ok(vec![35])
        );
    }

    // *Managing Anti-Aliasing*'s sample program, spelt as the chapter gives
    // it, asks the version, then the greyscale bit, and sets storage 2 when
    // the rasterizer answers greyscale.
    #[rustfmt::skip]
    let sample = [
        op::PUSHB + 1, 2, 0, op::WS,
        op::PUSHB + 2, 23, 17, 1, op::GETINFO, op::DUP,
        op::PUSHB, 34, op::LTEQ, op::ROLL, op::SWAP, op::JROT,
        op::PUSHB, 64, op::GT, op::JROT,
        op::PUSHB + 1, 10, 32, op::GETINFO,
        op::PUSHW, 0x10, 0x00, op::NEQ, op::JROT,
        op::PUSHB + 1, 2, 1, op::WS,
        op::PUSHB, 3, op::JMPR,
        op::POP, op::POP,
    ];
    for monochrome in [false, true] {
        let mut vm = machine();
        vm.set_size(at(monochrome));
        assert_eq!(
            on(&mut vm, &sample, Program::ControlValue),
            Ok(vec![]),
            "monochrome {monochrome}"
        );
        assert_eq!(
            vm.storage()[2],
            i32::from(!monochrome),
            "monochrome {monochrome}"
        );
    }
}

#[test]
fn instctrl_is_honoured_only_where_the_specification_allows() {
    // INSTCTRL pops a selector, then a value. In prep, selector 1 sets bit 0
    // from the value 1 and clears it from 0, selector 2 bit 1 from 2, and
    // selector 3 bit 2 from 4, which is kept; the flag takes the value's bit
    // of its own weight.
    let mut vm = machine();
    let steps = [
        (1, 1, 0b001),
        (2, 2, 0b011),
        (3, 4, 0b111),
        (1, 0, 0b110),
        (2, 1, 0b100),
        (2, 3, 0b110),
        (3, 0, 0b010),
        (1, 7, 0b011),
    ];
    for (selector, value, flags) in steps {
        let code = program(&[&push(&[value, selector]), &[op::INSTCTRL]]);
        assert_eq!(
            on(&mut vm, &code, Program::ControlValue),
            Ok(vec![]),
            "INSTCTRL {selector} {value}"
        );
        assert_eq!(vm.instruct_control(), flags, "INSTCTRL {selector} {value}");
    }
    // A selector past 3, or below 1, changes nothing, but pops its values.
    for selector in [0, 4, 5, -1, 255] {
        let code = program(&[&push(&[42, 7, selector]), &[op::INSTCTRL]]);
        assert_eq!(
            on(&mut vm, &code, Program::ControlValue),
            Ok(vec![42]),
            "selector {selector}"
        );
        assert_eq!(vm.instruct_control(), 0b011);
    }
    // Outside the control value program the flags do not change: "Flags 1
    // and 2 must be set in the CV program", and "INSTCTRL[] can only be
    // executed in the CVT program".
    for program_kind in [Program::Font, Program::Glyph] {
        for (selector, value) in [(1, 0), (2, 0), (3, 4)] {
            let code = program(&[&push(&[9, value, selector]), &[op::INSTCTRL]]);
            assert_eq!(
                on(&mut vm, &code, program_kind),
                Ok(vec![9]),
                "{program_kind:?}"
            );
            assert_eq!(
                vm.instruct_control(),
                0b011,
                "{program_kind:?} {selector} {value}"
            );
        }
    }

    // Apple's example sequence for prep turns grid-fitting off only for a
    // glyph rotated or stretched, which a hinted glyph never is here.
    let example = [
        op::PUSHB,
        6,
        op::GETINFO,
        op::IF,
        op::PUSHB,
        1,
        op::PUSHB,
        1,
        op::INSTCTRL,
        op::EIF,
    ];
    let mut vm = machine();
    assert_eq!(on(&mut vm, &example, Program::ControlValue), Ok(vec![]));
    assert_eq!(vm.instruct_control(), 0);
    // Taken unconditionally, the inner sequence sets the flag.
    assert_eq!(
        on(&mut vm, &example[4..9], Program::ControlValue),
        Ok(vec![])
    );
    assert_eq!(vm.instruct_control(), 0b001);

    assert_eq!(
        on(
            &mut vm,
            &program(&[&push(&[1]), &[op::INSTCTRL]]),
            Program::ControlValue
        ),
        Err(Error::StackUnderflow)
    );
}

#[test]
fn an_index_out_of_range_is_an_error() {
    // The storage area runs from 0 to maxStorage - 1: 64 locations here.
    let mut vm = machine();
    assert_eq!(roomy().storage(), 64);
    for location in [64, 65, -1, i32::from(i16::MIN), i32::from(i16::MAX)] {
        let code = program(&[&push(&[location]), &[op::RS]]);
        assert_eq!(
            on(&mut vm, &code, Program::Glyph),
            Err(Error::StorageIndex(location)),
            "RS {location}"
        );
        let code = program(&[&push(&[location, 5]), &[op::WS]]);
        assert_eq!(
            on(&mut vm, &code, Program::Glyph),
            Err(Error::StorageIndex(location)),
            "WS {location}"
        );
    }
    assert_eq!(
        vm.run_on(&[i32::MIN], &[op::RS], Program::Glyph),
        Err(Error::StorageIndex(i32::MIN))
    );
    // A face with no storage area has no location at all.
    let mut bare = Vm::new(Limits::new(MaxProfile::default()));
    assert_eq!(
        on(
            &mut bare,
            &program(&[&push(&[0]), &[op::RS]]),
            Program::Glyph
        ),
        Err(Error::StorageIndex(0))
    );

    // The control value table holds the cvt table's entries: none without
    // one, three here.
    for code in [
        program(&[&push(&[0]), &[op::RCVT]]),
        program(&[&push(&[0, 1]), &[op::WCVTP]]),
        program(&[&push(&[0, 1]), &[op::WCVTF]]),
    ] {
        assert_eq!(
            on(&mut vm, &code, Program::Glyph),
            Err(Error::CvtIndex(0)),
            "{code:02X?}"
        );
    }
    let mut vm = sized(
        Size {
            ppem: 64,
            points: 0,
            units_per_em: 64,
            monochrome: false,
        },
        &[1, 2, 3],
    );
    for location in [3, 4, -1, i32::from(i16::MAX)] {
        let read = program(&[&push(&[location]), &[op::RCVT]]);
        assert_eq!(
            on(&mut vm, &read, Program::Glyph),
            Err(Error::CvtIndex(location)),
            "RCVT {location}"
        );
        for opcode in [op::WCVTP, op::WCVTF] {
            let write = program(&[&push(&[location, 1]), &[opcode]]);
            assert_eq!(
                on(&mut vm, &write, Program::Glyph),
                Err(Error::CvtIndex(location)),
                "0x{opcode:02X} {location}"
            );
        }
    }
    assert_eq!(vm.cvt(), &[1, 2, 3], "nothing was written");
    let code = program(&[&push(&[2]), &[op::RCVT]]);
    assert_eq!(
        on(&mut vm, &code, Program::Glyph),
        Ok(vec![3]),
        "the last entry"
    );

    // Each pops what it needs first.
    for (opcode, count) in [
        (op::RS, 1),
        (op::WS, 2),
        (op::RCVT, 1),
        (op::WCVTP, 2),
        (op::WCVTF, 2),
        (op::GETINFO, 1),
        (op::INSTCTRL, 2),
    ] {
        let short = vec![0; count - 1];
        assert_eq!(
            vm.run_on(&short, &[opcode], Program::ControlValue),
            Err(Error::StackUnderflow),
            "0x{opcode:02X}"
        );
    }
}
