//! Every program here is built in this file through [`crate::type1`]'s one
//! cipher, so every charstring, subroutine and encoding entry is known in
//! advance, and each converted glyph is held to the Type 1 reader's outline
//! and advance of the same glyph — the reader is the program's meaning here,
//! and the converter's own check is run again from outside.

use super::*;
use crate::cff::{dict_get, parse_dict, Index};
use crate::type1::{encrypt, CHARSTRING_KEY, EEXEC_KEY};
use crate::{subset_cff, Segment};

/// The lead bytes the fixtures' charstrings and eexec section carry.
const PAD: u8 = 0x55;

// ---------------------------------------------------------------------------
// Charstrings.
// ---------------------------------------------------------------------------

/// Type 1 numbers in their shortest forms (chapter 6.2).
fn n(values: &[i32]) -> Vec<u8> {
    let mut out = Vec::new();
    for &v in values {
        if (-107..=107).contains(&v) {
            out.push((v + 139) as u8);
        } else if (108..=1131).contains(&v) {
            let v = v - 108;
            out.push((v / 256 + 247) as u8);
            out.push((v % 256) as u8);
        } else if (-1131..=-108).contains(&v) {
            let v = -v - 108;
            out.push((v / 256 + 251) as u8);
            out.push((v % 256) as u8);
        } else {
            out.push(255);
            out.extend_from_slice(&v.to_be_bytes());
        }
    }
    out
}

const HSTEM: u8 = 1;
const VSTEM: u8 = 3;
const VMOVETO: u8 = 4;
const RLINETO: u8 = 5;
const HLINETO: u8 = 6;
const VLINETO: u8 = 7;
const RRCURVETO: u8 = 8;
const CLOSEPATH: u8 = 9;
const CALLSUBR: u8 = 10;
const RETURN: u8 = 11;
const HSBW: u8 = 13;
const ENDCHAR: u8 = 14;
const RMOVETO: u8 = 21;
const HMOVETO: u8 = 22;
const VHCURVETO: u8 = 30;
const HVCURVETO: u8 = 31;
const DOTSECTION: [u8; 2] = [12, 0];
const VSTEM3: [u8; 2] = [12, 1];
const HSTEM3: [u8; 2] = [12, 2];
const SEAC: [u8; 2] = [12, 6];
const SBW: [u8; 2] = [12, 7];
const DIV: [u8; 2] = [12, 12];
const CALLOTHERSUBR: [u8; 2] = [12, 16];
const POP: [u8; 2] = [12, 17];
const SETCURRENTPOINT: [u8; 2] = [12, 33];

/// A charstring from its pieces.
fn cs(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

/// `0 width hsbw x y rmoveto`.
fn start(width: i32, x: i32, y: i32) -> Vec<u8> {
    cs(&[&n(&[0, width]), &[HSBW], &n(&[x, y]), &[RMOVETO]])
}

/// A glyph `width` wide drawing a `w` by `h` box from `(x, y)`.
fn rect(width: i32, x: i32, y: i32, w: i32, h: i32) -> Vec<u8> {
    cs(&[
        &start(width, x, y),
        &n(&[w]),
        &[HLINETO],
        &n(&[h]),
        &[VLINETO],
        &n(&[-w]),
        &[HLINETO, CLOSEPATH, ENDCHAR],
    ])
}

// ---------------------------------------------------------------------------
// Programs.
// ---------------------------------------------------------------------------

/// A built-in `/Encoding`.
#[derive(Clone)]
enum Encoding {
    /// `/Encoding StandardEncoding def` (chapter 10.3).
    Standard,
    /// `dup <code> /<name> put` entries over an array of `.notdef`.
    Entries(Vec<(u8, &'static str)>),
}

/// A Type 1 program assembled from its parts.
#[derive(Clone)]
struct Face {
    /// Each glyph's name and plaintext charstring, in order.
    glyphs: Vec<(Vec<u8>, Vec<u8>)>,
    /// Each subroutine's `dup` index and plaintext charstring, in order.
    subrs: Vec<(i64, Vec<u8>)>,
    encoding: Encoding,
    /// The clear text's `/FontName`.
    font_name: String,
    /// Clear-text keys, written after `/FontMatrix`.
    clear_keys: String,
    /// Private-section keys, written before `/lenIV`.
    private_keys: String,
}

impl Face {
    fn build(&self) -> Vec<u8> {
        let mut clear = format!(
            "%!PS-AdobeFont-1.0: Fixture 001.000\n\
             11 dict begin\n\
             /FontName /{} def\n\
             /FontType 1 def\n\
             /PaintType 0 def\n\
             /FontMatrix [0.001 0 0 0.001 0 0] readonly def\n",
            self.font_name
        )
        .into_bytes();
        clear.extend_from_slice(self.clear_keys.as_bytes());
        match &self.encoding {
            Encoding::Standard => clear.extend_from_slice(b"/Encoding StandardEncoding def\n"),
            Encoding::Entries(entries) => {
                clear.extend_from_slice(
                    b"/Encoding 256 array\n0 1 255 {1 index exch /.notdef put} for\n",
                );
                for (code, name) in entries {
                    clear.extend_from_slice(format!("dup {code} /{name} put\n").as_bytes());
                }
                clear.extend_from_slice(b"readonly def\n");
            }
        }
        clear.extend_from_slice(b"currentdict end\ncurrentfile eexec\n");

        let lead = [PAD; 4];
        let mut private = b"dup /Private 8 dict dup begin\n\
            /RD{string currentfile exch readstring pop}executeonly def\n\
            /ND{noaccess def}executeonly def\n\
            /NP{noaccess put}executeonly def\n"
            .to_vec();
        private.extend_from_slice(self.private_keys.as_bytes());
        private.extend_from_slice(b"/lenIV 4 def\n");
        private.extend_from_slice(format!("/Subrs {} array\n", self.subrs.len()).as_bytes());
        for (index, code) in &self.subrs {
            let encrypted = encrypt(code, CHARSTRING_KEY, &lead);
            private.extend_from_slice(format!("dup {index} {} RD ", encrypted.len()).as_bytes());
            private.extend_from_slice(&encrypted);
            private.extend_from_slice(b" NP\n");
        }
        private.extend_from_slice(b"ND\n");
        private.extend_from_slice(
            format!(
                "2 index /CharStrings {} dict dup begin\n",
                self.glyphs.len()
            )
            .as_bytes(),
        );
        for (name, code) in &self.glyphs {
            let encrypted = encrypt(code, CHARSTRING_KEY, &lead);
            private.push(b'/');
            private.extend_from_slice(name);
            private.extend_from_slice(format!(" {} RD ", encrypted.len()).as_bytes());
            private.extend_from_slice(&encrypted);
            private.extend_from_slice(b" ND\n");
        }
        private.extend_from_slice(
            b"end\nend\nreadonly put\nnoaccess put\n\
              dup /FontName get exch definefont pop\nmark currentfile closefile\n",
        );

        let mut out = clear;
        out.extend(encrypt(&private, EEXEC_KEY, &lead));
        out.push(b'\n');
        for _ in 0..8 {
            out.extend_from_slice(&[b'0'; 64]);
            out.push(b'\n');
        }
        out.extend_from_slice(b"cleartomark\n");
        out
    }
}

/// Chapter 8.4's flex and hint-replacement subroutines, and those the
/// glyphs below call.
fn standard_subrs() -> Vec<(i64, Vec<u8>)> {
    vec![
        (
            0,
            cs(&[
                &n(&[3, 0]),
                &CALLOTHERSUBR,
                &POP,
                &POP,
                &SETCURRENTPOINT,
                &[RETURN],
            ]),
        ),
        (1, cs(&[&n(&[0, 1]), &CALLOTHERSUBR, &[RETURN]])),
        (2, cs(&[&n(&[0, 2]), &CALLOTHERSUBR, &[RETURN]])),
        (3, vec![RETURN]),
        // A short line, which subroutine 5 calls.
        (4, cs(&[&n(&[0, -10]), &[RLINETO, RETURN]])),
        // Three sides of a 500-unit box, then subroutine 4.
        (
            5,
            cs(&[
                &n(&[500]),
                &[HLINETO],
                &n(&[500]),
                &[VLINETO],
                &n(&[-500]),
                &[HLINETO],
                &n(&[4]),
                &[CALLSUBR, RETURN],
            ]),
        ),
        // A hint replacement's stems, and the subroutine it calls for more.
        (
            6,
            cs(&[&n(&[0, 60]), &[HSTEM], &n(&[7]), &[CALLSUBR, RETURN]]),
        ),
        (7, cs(&[&n(&[10, 20]), &[VSTEM, RETURN]])),
        // Two sides and an `endchar`, which ends only the subroutine.
        (
            8,
            cs(&[
                &n(&[100, 0]),
                &[RLINETO],
                &n(&[0, 100]),
                &[RLINETO, CLOSEPATH, ENDCHAR],
            ]),
        ),
    ]
}

/// Chapter 8.3's flex from `(100, -10)`: two curves meeting at `(150, 0)`
/// and ending at `(200, -10)`, height 50.
fn flex_glyph() -> Vec<u8> {
    let mut code = cs(&[&n(&[0, 500]), &[HSBW], &n(&[100, -10]), &[RMOVETO]]);
    code.extend(n(&[1]));
    code.push(CALLSUBR);
    for (dx, dy) in [
        (50, 0),
        (-35, 0),
        (10, 10),
        (25, 0),
        (25, 0),
        (10, -10),
        (15, 0),
    ] {
        code.extend(n(&[dx, dy]));
        code.push(RMOVETO);
        code.extend(n(&[2]));
        code.push(CALLSUBR);
    }
    code.extend(n(&[50, 200, -10, 0]));
    code.extend([CALLSUBR, CLOSEPATH, ENDCHAR]);
    code
}

/// The fixture's glyphs, in program order, each exercising what its name
/// says.
fn standard_glyphs() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        (".notdef", cs(&[&n(&[0, 250]), &[HSBW, ENDCHAR]])),
        // Subroutines calling subroutines.
        (
            "A",
            cs(&[
                &start(600, 20, 0),
                &n(&[5]),
                &[CALLSUBR, CLOSEPATH, ENDCHAR],
            ]),
        ),
        // Stems, and the three curve operators.
        (
            "B",
            cs(&[
                &n(&[30, 650]),
                &[HSBW],
                &n(&[0, 20]),
                &[HSTEM],
                &n(&[400, 30]),
                &[HSTEM],
                &n(&[40, 50]),
                &[VSTEM],
                &n(&[0, 0]),
                &[RMOVETO],
                &n(&[100, 0, 50, 50, 0, 100]),
                &[RRCURVETO],
                &n(&[50, 50, 50, 0]),
                &[VHCURVETO],
                &n(&[50, 0, 50, -50]),
                &[HVCURVETO],
                &n(&[-100]),
                &[VLINETO, CLOSEPATH, ENDCHAR],
            ]),
        ),
        ("C", flex_glyph()),
        // Hint replacement: subroutine 6's stems replace the glyph's own.
        (
            "D",
            cs(&[
                &n(&[0, 500]),
                &[HSBW],
                &n(&[0, 50]),
                &[HSTEM],
                &n(&[0, 0]),
                &[RMOVETO],
                &n(&[100]),
                &[HLINETO],
                &n(&[6, 1, 3]),
                &CALLOTHERSUBR,
                &POP,
                &[CALLSUBR],
                &n(&[100]),
                &[VLINETO],
                &n(&[-100]),
                &[HLINETO, CLOSEPATH, ENDCHAR],
            ]),
        ),
        // `div`: a fractional width and three fractional sides.
        (
            "E",
            cs(&[
                &n(&[0, 1001, 2]),
                &DIV,
                &[HSBW],
                &n(&[10, 0]),
                &[RMOVETO],
                &n(&[100, 3]),
                &DIV,
                &n(&[0]),
                &[RLINETO],
                &n(&[0, 200, 3]),
                &DIV,
                &[RLINETO],
                &n(&[-100, 3]),
                &DIV,
                &n(&[0]),
                &[RLINETO, CLOSEPATH, ENDCHAR],
            ]),
        ),
        // `sbw` with no vertical metrics, `hstem3`, `vstem3`, `dotsection`.
        (
            "F",
            cs(&[
                &n(&[0, 0, 600, 0]),
                &SBW,
                &n(&[0, 20, 300, 20, 600, 20]),
                &HSTEM3,
                &n(&[100, 30, 250, 30, 400, 30]),
                &VSTEM3,
                &DOTSECTION,
                &n(&[0, 0]),
                &[RMOVETO],
                &n(&[500]),
                &[HLINETO],
                &n(&[500]),
                &[VLINETO],
                &n(&[-500]),
                &[HLINETO, CLOSEPATH],
                &DOTSECTION,
                &[ENDCHAR],
            ]),
        ),
        // A subroutine's `endchar` ends the subroutine; the glyph goes on.
        (
            "G",
            cs(&[
                &start(500, 0, 0),
                &n(&[8]),
                &[CALLSUBR],
                &n(&[200, 0]),
                &[RMOVETO],
                &n(&[100]),
                &[HLINETO],
                &n(&[100]),
                &[VLINETO],
                &n(&[-100]),
                &[HLINETO, CLOSEPATH, ENDCHAR],
            ]),
        ),
        // A side bearing of 12.5, which a 16.16 operand holds.
        (
            "H",
            cs(&[
                &n(&[25, 2]),
                &DIV,
                &n(&[600]),
                &[HSBW],
                &n(&[10, 0]),
                &[RMOVETO],
                &n(&[100]),
                &[HLINETO],
                &n(&[100]),
                &[VLINETO],
                &n(&[-100]),
                &[HLINETO, CLOSEPATH, ENDCHAR],
            ]),
        ),
        // A side bearing of 100/3, which none does: the first move is the
        // side bearing and the operand added.
        (
            "I",
            cs(&[
                &n(&[100, 3]),
                &DIV,
                &n(&[600]),
                &[HSBW],
                &n(&[10, 20]),
                &[RMOVETO],
                &n(&[100]),
                &[HLINETO],
                &n(&[100]),
                &[VLINETO, CLOSEPATH, ENDCHAR],
            ]),
        ),
        // `hsbw` again in the middle of the path: the pen jumps to the new
        // side bearing, and the width is the second.
        (
            "J",
            cs(&[
                &start(500, 10, 10),
                &n(&[100]),
                &[HLINETO],
                &n(&[50, 600]),
                &[HSBW],
                &n(&[100]),
                &[VLINETO, CLOSEPATH, ENDCHAR],
            ]),
        ),
        // A dividend past 16 bits, written as its quotient.
        (
            "K",
            cs(&[
                &start(500, 0, 0),
                &n(&[100_000, 1000]),
                &DIV,
                &[HLINETO],
                &n(&[100]),
                &[VLINETO, CLOSEPATH, ENDCHAR],
            ]),
        ),
        // `hmoveto`, `vmoveto` straight after it, and curves alone.
        (
            "L",
            cs(&[
                &n(&[0, 400]),
                &[HSBW],
                &n(&[50]),
                &[HMOVETO],
                &n(&[50]),
                &[VMOVETO],
                &n(&[100, 50, 50, 100]),
                &[VHCURVETO],
                &n(&[100, 50, 50, 100]),
                &[HVCURVETO, CLOSEPATH, ENDCHAR],
            ]),
        ),
        ("acute", rect(300, 100, 750, 150, 150)),
        (
            "Aacute",
            // asb adx ady bchar achar seac: `A` is StandardEncoding 65 and
            // `acute` 194.
            cs(&[&n(&[0, 600]), &[HSBW], &n(&[0, 100, 200, 65, 194]), &SEAC]),
        ),
        // In no encoding.
        ("M", cs(&[&n(&[0, 300]), &[HSBW, ENDCHAR]])),
    ]
}

/// The built-in encoding: `A` to `L` at 65 to 76 and `A` again at 200,
/// `acute` at 194 and `Aacute` at 201; `M` in none.
fn standard_encoding() -> Vec<(u8, &'static str)> {
    let mut entries: Vec<(u8, &'static str)> =
        ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L"]
            .iter()
            .enumerate()
            .map(|(at, name)| (65 + at as u8, *name))
            .collect();
    entries.extend([(194, "acute"), (200, "A"), (201, "Aacute")]);
    entries
}

fn named(glyphs: Vec<(&str, Vec<u8>)>) -> Vec<(Vec<u8>, Vec<u8>)> {
    glyphs
        .into_iter()
        .map(|(name, code)| (name.as_bytes().to_vec(), code))
        .collect()
}

fn face() -> Face {
    Face {
        glyphs: named(standard_glyphs()),
        subrs: standard_subrs(),
        encoding: Encoding::Entries(standard_encoding()),
        font_name: "Fixture".to_owned(),
        clear_keys: String::new(),
        private_keys: String::new(),
    }
}

/// A program of `.notdef` and one glyph `A` over the standard subroutines and
/// `extra`.
fn single(glyph: Vec<u8>, extra: &[(i64, Vec<u8>)]) -> Vec<u8> {
    let mut subrs = standard_subrs();
    subrs.extend(extra.iter().cloned());
    Face {
        glyphs: named(vec![
            (".notdef", cs(&[&n(&[0, 250]), &[HSBW, ENDCHAR]])),
            ("A", glyph),
        ]),
        subrs,
        encoding: Encoding::Entries(vec![(65, "A")]),
        ..face()
    }
    .build()
}

/// What converting `glyph` alone is refused as.
#[track_caller]
fn refused(glyph: Vec<u8>, extra: &[(i64, Vec<u8>)]) -> Type1CffRefusal {
    match type1_to_cff(&single(glyph, extra)) {
        Ok(_) => panic!("the glyph converted"),
        Err(why) => why,
    }
}

// ---------------------------------------------------------------------------
// Reading a CFF back.
// ---------------------------------------------------------------------------

/// The Top DICT, the Private DICT and the String INDEX of a CFF.
fn dicts(program: &[u8]) -> (Vec<(u16, Vec<f64>)>, Vec<(u16, Vec<f64>)>, Vec<Vec<u8>>) {
    let header = usize::from(program[2]);
    let (_, names_end) = Index::parse(program, header).expect("a Name INDEX");
    let (top, top_end) = Index::parse(program, names_end).expect("a Top DICT INDEX");
    let (strings, _) = Index::parse(program, top_end).expect("a String INDEX");
    let top = parse_dict(top.get(0).expect("one Top DICT"));
    let at = dict_get(&top, 18).expect("a Private DICT");
    let (size, offset) = (at[0] as usize, at[1] as usize);
    let private = parse_dict(&program[offset..offset + size]);
    let strings = (0..strings.len())
        .map(|i| strings.get(i).expect("a string").to_vec())
        .collect();
    (top, private, strings)
}

/// One glyph's Type 2 charstring.
fn charstring(program: &[u8], gid: u16) -> Vec<u8> {
    let (top, _, _) = dicts(program);
    let at = dict_get(&top, 17).expect("CharStrings")[0] as usize;
    let (index, _) = Index::parse(program, at).expect("a CharStrings INDEX");
    index.get(usize::from(gid)).expect("the glyph").to_vec()
}

/// One operator of a Type 2 charstring: its code — a two-byte one as
/// `0x0C00` and its second byte — the values on the stack when it ran, and a
/// `hintmask`'s bytes.
#[derive(Debug, PartialEq)]
struct Op {
    op: u16,
    operands: Vec<f64>,
    mask: Vec<u8>,
}

/// A Type 2 charstring's operators, `div` and `add` run as they are read and
/// a `hintmask` measured by the stems declared before it.
fn operators(code: &[u8]) -> Vec<Op> {
    let mut out = Vec::new();
    let mut stack: Vec<f64> = Vec::new();
    let mut stems = 0usize;
    let mut at = 0usize;
    while at < code.len() {
        let b = code[at];
        at += 1;
        match b {
            32..=246 => stack.push(f64::from(b) - 139.0),
            247..=250 => {
                stack.push((f64::from(b) - 247.0) * 256.0 + f64::from(code[at]) + 108.0);
                at += 1;
            }
            251..=254 => {
                stack.push(-(f64::from(b) - 251.0) * 256.0 - f64::from(code[at]) - 108.0);
                at += 1;
            }
            28 => {
                stack.push(f64::from(i16::from_be_bytes([code[at], code[at + 1]])));
                at += 2;
            }
            255 => {
                let v = i32::from_be_bytes([code[at], code[at + 1], code[at + 2], code[at + 3]]);
                stack.push(f64::from(v) / 65536.0);
                at += 4;
            }
            12 => {
                let second = code[at];
                at += 1;
                match second {
                    10 | 12 => {
                        let b = stack.pop().expect("two operands");
                        let a = stack.pop().expect("two operands");
                        stack.push(if second == 10 { a + b } else { a / b });
                    }
                    _ => out.push(Op {
                        op: 0x0C00 | u16::from(second),
                        operands: core::mem::take(&mut stack),
                        mask: Vec::new(),
                    }),
                }
            }
            1 | 3 | 18 | 23 => {
                stems += stack.len() / 2;
                out.push(Op {
                    op: u16::from(b),
                    operands: core::mem::take(&mut stack),
                    mask: Vec::new(),
                });
            }
            19 | 20 => {
                stems += stack.len() / 2;
                let bytes = stems.div_ceil(8);
                out.push(Op {
                    op: u16::from(b),
                    operands: core::mem::take(&mut stack),
                    mask: code[at..at + bytes].to_vec(),
                });
                at += bytes;
            }
            _ => out.push(Op {
                op: u16::from(b),
                operands: core::mem::take(&mut stack),
                mask: Vec::new(),
            }),
        }
    }
    out
}

/// Each converted glyph's charstring operator codes.
fn codes(program: &[u8], gid: u16) -> Vec<u16> {
    operators(&charstring(program, gid))
        .iter()
        .map(|op| op.op)
        .collect()
}

/// The converted glyph a Type 1 name became.
fn gid_of(converted: &Type1Cff, font: &Type1<'_>, name: &[u8]) -> u16 {
    let index = font.glyph_for_name(name).expect("the fixture has it");
    converted.glyphs[usize::from(index)]
}

/// Every Type 1 glyph draws and advances in the CFF exactly as the reader
/// draws it.
#[track_caller]
fn draws_alike(program: &[u8], converted: &Type1Cff) {
    let font = Type1::parse(program).expect("the program parses");
    let cff = Cff::parse(&converted.program).expect("the conversion parses");
    for index in 0..font.glyph_count() {
        let gid = converted.glyphs[index];
        let name = String::from_utf8_lossy(font.glyph_name(index).expect("named")).into_owned();
        assert_eq!(
            cff.outline(gid).map(|o| o.segments),
            font.outline(index as u16).map(|o| o.segments),
            "{name} draws something else"
        );
        assert_eq!(
            cff.advance(gid),
            font.advance(index as u16),
            "{name} advances otherwise"
        );
    }
}

// ---------------------------------------------------------------------------
// The exit's tests.
// ---------------------------------------------------------------------------

/// The fixture is what its names say: each glyph draws, the flex's two
/// curves are there, and the reader composes the accented letter.
#[test]
fn the_fixture_is_a_program_the_reader_draws() {
    let program = face().build();
    let font = Type1::parse(&program).expect("it parses");
    assert_eq!(font.glyph_count(), 16);
    for name in [
        b"A".as_slice(),
        b"B",
        b"C",
        b"D",
        b"E",
        b"F",
        b"G",
        b"H",
        b"I",
        b"J",
        b"K",
        b"L",
        b"acute",
        b"Aacute",
    ] {
        let glyph = font.glyph_for_name(name).expect("in the fixture");
        assert!(
            !font.outline(glyph).expect("it outlines").is_empty(),
            "{} draws",
            String::from_utf8_lossy(name)
        );
    }
    let flex = font
        .outline(font.glyph_for_name(b"C").expect("C"))
        .expect("C outlines")
        .segments;
    assert_eq!(
        flex.iter()
            .filter(|s| matches!(s, Segment::CurveTo { .. }))
            .count(),
        2,
        "chapter 8.3's two curves: {flex:?}"
    );
    // G's subroutine ends in `endchar`, and the glyph draws on after it.
    let g = font
        .outline(font.glyph_for_name(b"G").expect("G"))
        .expect("G outlines")
        .segments;
    assert_eq!(
        g.iter()
            .filter(|s| matches!(s, Segment::MoveTo { .. }))
            .count(),
        2,
        "{g:?}"
    );
    assert_eq!(
        font.advance(font.glyph_for_name(b"E").expect("E")),
        Some(500.5)
    );
    assert_eq!(
        font.advance(font.glyph_for_name(b"J").expect("J")),
        Some(600.0)
    );
}

/// **The headline**: every glyph — subroutines inlined, curves, a flex, hint
/// replacement, `div`, `sbw`, the three-stem hints, a subroutine's
/// `endchar`, fractional side bearings, a second `hsbw`, a 32-bit dividend,
/// the one-axis moves and a `seac` — outlines and advances in the CFF
/// exactly as the Type 1 reader draws it.
#[test]
fn every_glyph_converts_to_the_same_outline_and_advance() {
    let program = face().build();
    let converted = type1_to_cff(&program).expect("it converts");
    draws_alike(&program, &converted);

    let font = Type1::parse(&program).expect("it parses");
    // The flex is Type 2's `flex`, the subroutines are gone, and `div` is
    // Type 2's `div`.
    assert!(codes(&converted.program, gid_of(&converted, &font, b"C")).contains(&0x0C23));
    for name in [b"A".as_slice(), b"G"] {
        assert!(!codes(&converted.program, gid_of(&converted, &font, name)).contains(&10));
    }
    let e = charstring(&converted.program, gid_of(&converted, &font, b"E"));
    assert!(e.windows(2).any(|w| w == DIV), "{e:?}");
    // The side bearing of 100/3 and the first move, added.
    let i = charstring(&converted.program, gid_of(&converted, &font, b"I"));
    assert!(i.windows(2).any(|w| w == [12, 10]), "{i:?}");
    // 100 000 / 1000 is written as 100, with no `div`.
    let k = charstring(&converted.program, gid_of(&converted, &font, b"K"));
    assert!(!k.windows(2).any(|w| w == DIV), "{k:?}");
    // The accented letter is one charstring: the base's path and the
    // accent's, with no `endchar` of `seac`'s four operands.
    let aacute = operators(&charstring(
        &converted.program,
        gid_of(&converted, &font, b"Aacute"),
    ));
    assert_eq!(aacute.last().map(|op| op.op), Some(14));
    assert!(aacute.last().is_some_and(|op| op.operands.len() < 4));
}

/// The charset names every glyph, `.notdef` first and the encoded ones next
/// in the order of their codes; every code of the built-in encoding reaches
/// the glyph it reaches in the program, a second code for one glyph through
/// a supplement, and a glyph in no encoding is reached by no code.
#[test]
fn the_charset_names_every_glyph_and_the_encoding_maps_every_code() {
    let program = face().build();
    let font = Type1::parse(&program).expect("it parses");
    let converted = type1_to_cff(&program).expect("it converts");
    let cff = Cff::parse(&converted.program).expect("the conversion parses");

    assert_eq!(cff.glyph_count(), font.glyph_count());
    assert_eq!(cff.glyph_name(0), Some(".notdef"));
    for index in 0..font.glyph_count() {
        let name = font.glyph_name(index).expect("named");
        let gid = converted.glyphs[index];
        assert_eq!(cff.glyph_name(gid).map(str::as_bytes), Some(name));
        assert_eq!(converted.names[usize::from(gid)], name);
        let text = core::str::from_utf8(name).expect("ASCII");
        assert_eq!(cff.gid_for_name(text), Some(gid));
    }
    let order: Vec<&[u8]> = converted.names.iter().map(Vec::as_slice).collect();
    assert_eq!(
        order,
        [
            b".notdef".as_slice(),
            b"A",
            b"B",
            b"C",
            b"D",
            b"E",
            b"F",
            b"G",
            b"H",
            b"I",
            b"J",
            b"K",
            b"L",
            b"acute",
            b"Aacute",
            b"M",
        ]
    );

    for code in 0..=255u8 {
        let expected = font
            .glyph_for_code(code)
            .and_then(|glyph| font.glyph_name(usize::from(glyph)))
            .filter(|name| *name != b".notdef");
        let actual = cff
            .gid_for_code(code)
            .and_then(|gid| cff.glyph_name(gid))
            .map(str::as_bytes);
        assert_eq!(actual, expected, "code {code}");
    }
    assert_eq!(cff.gid_for_code(200), cff.gid_for_code(65));
    let m = gid_of(&converted, &font, b"M");
    assert!((0..=255u8).all(|code| cff.gid_for_code(code) != Some(m)));

    // A custom table: the Top DICT's Encoding is an offset, not 0.
    let (top, _, _) = dicts(&converted.program);
    assert!(dict_get(&top, 16).is_some_and(|at| at[0] > 1.0));
}

/// A program that says `StandardEncoding` takes the predefined Standard
/// encoding, offset 0, and reaches each glyph by its standard code.
#[test]
fn a_program_that_says_standard_encoding_takes_the_predefined_one() {
    let program = Face {
        glyphs: named(vec![
            (".notdef", cs(&[&n(&[0, 250]), &[HSBW, ENDCHAR]])),
            ("space", cs(&[&n(&[0, 250]), &[HSBW, ENDCHAR]])),
            ("A", rect(600, 20, 0, 500, 500)),
            ("B", rect(600, 30, 0, 400, 600)),
            ("acute", rect(300, 100, 750, 150, 150)),
        ]),
        encoding: Encoding::Standard,
        ..face()
    }
    .build();
    let converted = type1_to_cff(&program).expect("it converts");
    draws_alike(&program, &converted);
    let (top, _, _) = dicts(&converted.program);
    assert_eq!(dict_get(&top, 16), Some([0.0].as_slice()));
    let cff = Cff::parse(&converted.program).expect("it parses");
    for (code, name) in [(32u8, "space"), (65, "A"), (66, "B"), (194, "acute")] {
        assert_eq!(
            cff.gid_for_code(code).and_then(|g| cff.glyph_name(g)),
            Some(name)
        );
    }
    assert_eq!(cff.gid_for_code(67), None, "no C");
}

/// The conversion is a CFF the subsetter cuts: the kept glyphs draw as the
/// Type 1 program draws them, the others are empty, and it is smaller.
#[test]
fn the_output_parses_and_subsets() {
    let program = face().build();
    let font = Type1::parse(&program).expect("it parses");
    let converted = type1_to_cff(&program).expect("it converts");
    let cff = Cff::parse(&converted.program).expect("the conversion parses");
    assert!(!cff.is_cid());

    let kept = [b"B".as_slice(), b"C", b"D", b"Aacute"];
    let wanted: BTreeSet<u16> = kept
        .iter()
        .map(|name| gid_of(&converted, &font, name))
        .collect();
    let subset = subset_cff(&converted.program, &wanted).expect("the conversion subsets");
    assert!(subset.len() < converted.program.len());
    let cut = Cff::parse(&subset).expect("the subset parses");
    assert_eq!(cut.glyph_count(), cff.glyph_count());
    for name in kept {
        let index = font.glyph_for_name(name).expect("in the fixture");
        let gid = converted.glyphs[usize::from(index)];
        assert_eq!(
            cut.outline(gid).map(|o| o.segments),
            font.outline(index).map(|o| o.segments),
            "{}",
            String::from_utf8_lossy(name)
        );
        assert_eq!(cut.advance(gid), font.advance(index));
    }
    let a = gid_of(&converted, &font, b"A");
    assert!(
        cut.outline(a).expect("A answers").is_empty(),
        "A is nobody's"
    );
}

// ---------------------------------------------------------------------------
// What the CFF carries.
// ---------------------------------------------------------------------------

/// Stems without a hint replacement are `hstem` and `vstem`, moved by the
/// side bearing; the width rides on the first.
#[test]
fn stems_are_declared_from_the_side_bearing() {
    let program = face().build();
    let font = Type1::parse(&program).expect("it parses");
    let converted = type1_to_cff(&program).expect("it converts");
    let b = operators(&charstring(
        &converted.program,
        gid_of(&converted, &font, b"B"),
    ));
    assert_eq!(b[0].op, 1);
    // The width, then (0, 20) and (400, 30), the second from the first's top.
    assert_eq!(b[0].operands, [650.0, 0.0, 20.0, 380.0, 30.0]);
    // `vstem`'s x is relative to the side bearing, 30.
    assert_eq!(b[1].op, 3);
    assert_eq!(b[1].operands, [70.0, 50.0]);
    assert!(b.iter().all(|op| !matches!(op.op, 18 | 19 | 23)));

    // `hstem3` and `vstem3` are three stems each.
    let f = operators(&charstring(
        &converted.program,
        gid_of(&converted, &font, b"F"),
    ));
    assert_eq!(f[0].op, 1);
    assert_eq!(f[0].operands, [600.0, 0.0, 20.0, 280.0, 20.0, 280.0, 20.0]);
    assert_eq!(f[1].op, 3);
    assert_eq!(f[1].operands, [100.0, 30.0, 120.0, 30.0, 120.0, 30.0]);
}

/// A hint replacement declares every stem at the start as `hstemhm` and
/// `vstemhm`, and a `hintmask` selects each set where it takes effect.
#[test]
fn hint_replacement_becomes_hint_masks() {
    let program = face().build();
    let font = Type1::parse(&program).expect("it parses");
    let converted = type1_to_cff(&program).expect("it converts");
    let d = operators(&charstring(
        &converted.program,
        gid_of(&converted, &font, b"D"),
    ));
    let sequence: Vec<u16> = d.iter().map(|op| op.op).collect();
    assert_eq!(sequence, [18, 23, 19, 22, 6, 19, 7, 14], "{d:?}");
    // The stems: (0, 50) the glyph's own, (0, 60) and the vertical (10, 20)
    // the replacement's.
    assert_eq!(d[0].operands, [500.0, 0.0, 50.0, -50.0, 60.0]);
    assert_eq!(d[1].operands, [10.0, 20.0]);
    assert_eq!(d[2].mask, [0b1000_0000]);
    assert_eq!(d[5].mask, [0b0110_0000]);
}

/// The Top DICT carries the program's `FontBBox`, a `FontMatrix` that is
/// not the default, and its `FontInfo`; the Private DICT its zones, stems
/// and the rest of its hinting values, zones and snaps as delta arrays; and
/// no subroutine survives anywhere.
#[test]
fn the_dicts_carry_the_programs_values() {
    let program = Face {
        font_name: "Fixture-Bold".to_owned(),
        clear_keys: "/FontBBox {-50 -250 1000 900} readonly def\n\
            /FontInfo 9 dict dup begin\n\
            /version (001.002) readonly def\n\
            /Notice (Copyright \\(c\\) nobody) readonly def\n\
            /FullName (Fixture Bold) readonly def\n\
            /FamilyName (Fixture) readonly def\n\
            /Weight (Bold) readonly def\n\
            /ItalicAngle -12.5 def\n\
            /isFixedPitch true def\n\
            /UnderlinePosition -100 def\n\
            /UnderlineThickness 50 def\n\
            end readonly def\n"
            .to_owned(),
        private_keys: "/BlueValues [-15 0 721 736] def\n\
            /OtherBlues [-250 -240] def\n\
            /StdHW [50] def\n\
            /StdVW [80] def\n\
            /StemSnapH [50 60] def\n\
            /BlueScale 0.039625 def\n\
            /BlueShift 7 def\n\
            /BlueFuzz 1 def\n\
            /ForceBold false def\n\
            /LanguageGroup 0 def\n"
            .to_owned(),
        ..face()
    }
    .build();
    let converted = type1_to_cff(&program).expect("it converts");
    let (top, private, strings) = dicts(&converted.program);
    let string = |op: u16| -> Vec<u8> {
        let sid = dict_get(&top, op).expect("stated")[0] as usize;
        match STANDARD_STRINGS.get(sid) {
            Some(standard) => standard.as_bytes().to_vec(),
            None => strings[sid - STANDARD_STRINGS.len()].clone(),
        }
    };
    assert_eq!(string(0), b"001.002");
    assert_eq!(string(1), b"Copyright (c) nobody");
    assert_eq!(string(2), b"Fixture Bold");
    assert_eq!(string(3), b"Fixture");
    // `Bold` is a standard string.
    assert!(dict_get(&top, 4).is_some_and(|sid| (sid[0] as usize) < STANDARD_STRINGS.len()));
    assert_eq!(string(4), b"Bold");
    assert_eq!(dict_get(&top, 0x0C02), Some([-12.5].as_slice()));
    assert_eq!(dict_get(&top, 0x0C01), Some([1.0].as_slice()));
    assert_eq!(dict_get(&top, 0x0C03), Some([-100.0].as_slice()));
    assert_eq!(dict_get(&top, 0x0C04), Some([50.0].as_slice()));
    assert_eq!(
        dict_get(&top, 5),
        Some([-50.0, -250.0, 1000.0, 900.0].as_slice())
    );
    // The default matrix is left to the default.
    assert_eq!(dict_get(&top, 0x0C07), None);

    assert_eq!(
        dict_get(&private, 6),
        Some([-15.0, 15.0, 721.0, 15.0].as_slice())
    );
    assert_eq!(dict_get(&private, 7), Some([-250.0, 10.0].as_slice()));
    assert_eq!(dict_get(&private, 10), Some([50.0].as_slice()));
    assert_eq!(dict_get(&private, 11), Some([80.0].as_slice()));
    assert_eq!(dict_get(&private, 0x0C0C), Some([50.0, 10.0].as_slice()));
    assert_eq!(dict_get(&private, 0x0C09), Some([0.039625].as_slice()));
    assert_eq!(dict_get(&private, 0x0C0A), Some([7.0].as_slice()));
    assert_eq!(dict_get(&private, 0x0C0B), Some([1.0].as_slice()));
    assert_eq!(dict_get(&private, 0x0C0E), Some([0.0].as_slice()));
    assert_eq!(dict_get(&private, 0x0C11), Some([0.0].as_slice()));
    assert_eq!(dict_get(&private, 19), None, "no subroutines");

    // The Name INDEX holds the program's `/FontName`.
    let (names, _) = Index::parse(&converted.program, 4).expect("a Name INDEX");
    assert_eq!(names.get(0), Some(b"Fixture-Bold".as_slice()));
    // And the global subroutine INDEX is empty.
    let header = usize::from(converted.program[2]);
    let (_, names_end) = Index::parse(&converted.program, header).expect("names");
    let (_, top_end) = Index::parse(&converted.program, names_end).expect("top");
    let (_, strings_end) = Index::parse(&converted.program, top_end).expect("strings");
    let (gsubrs, _) = Index::parse(&converted.program, strings_end).expect("gsubrs");
    assert_eq!(gsubrs.len(), 0);
}

/// A `FontMatrix` other than the default is carried, and every glyph still
/// draws as in the program.
#[test]
fn a_font_matrix_that_is_not_the_default_is_carried() {
    let mut program = face().build();
    let from = b"/FontMatrix [0.001 0 0 0.001 0 0]";
    let at = find(&program, from).expect("the fixture's matrix");
    program.splice(
        at..at + from.len(),
        b"/FontMatrix [0.0005 0 0 0.0005 0 0]".iter().copied(),
    );
    let converted = type1_to_cff(&program).expect("it converts");
    draws_alike(&program, &converted);
    let cff = Cff::parse(&converted.program).expect("it parses");
    assert_eq!(cff.font_matrix, [0.0005, 0.0, 0.0, 0.0005, 0.0, 0.0]);
}

/// A program with no `.notdef` gains an empty one at glyph 0, which every
/// CFF begins with; every glyph the program has keeps its drawing.
#[test]
fn a_program_without_notdef_gains_an_empty_one() {
    let program = Face {
        glyphs: named(vec![
            ("A", rect(600, 20, 0, 500, 500)),
            ("B", rect(600, 30, 0, 400, 600)),
        ]),
        encoding: Encoding::Entries(vec![(65, "A"), (66, "B")]),
        ..face()
    }
    .build();
    let converted = type1_to_cff(&program).expect("it converts");
    draws_alike(&program, &converted);
    let cff = Cff::parse(&converted.program).expect("it parses");
    assert_eq!(cff.glyph_count(), 3);
    assert_eq!(cff.glyph_name(0), Some(".notdef"));
    assert!(cff.outline(0).expect("glyph 0").is_empty());
    assert_eq!(cff.advance(0), Some(0.0));
    assert_eq!(converted.glyphs, [1, 2]);
}

/// The same program converts to the same bytes: nothing random enters
/// (ruling 4).
#[test]
fn the_same_program_converts_to_the_same_bytes() {
    let program = face().build();
    assert_eq!(type1_to_cff(&program), type1_to_cff(&program));
}

// ---------------------------------------------------------------------------
// What it refuses.
// ---------------------------------------------------------------------------

#[test]
fn bytes_that_are_no_type1_program_are_unreadable() {
    for bytes in [
        b"".as_slice(),
        b"not a font",
        b"%!PS-AdobeFont-1.0: X\ncurrentfile eexec\n",
    ] {
        assert_eq!(type1_to_cff(bytes), Err(Type1CffRefusal::Unreadable));
    }
}

#[test]
fn a_multiple_master_program_is_refused() {
    let program = Face {
        clear_keys: "/WeightVector [0.5 0.5] def\n".to_owned(),
        ..face()
    }
    .build();
    assert_eq!(type1_to_cff(&program), Err(Type1CffRefusal::MultipleMaster));
}

#[test]
fn an_othersubr_past_three_is_refused() {
    let glyph = cs(&[
        &start(500, 0, 0),
        &n(&[7, 1, 5]),
        &CALLOTHERSUBR,
        &[ENDCHAR],
    ]);
    assert_eq!(refused(glyph, &[]), Type1CffRefusal::OtherSubr);
}

/// `pop` with nothing a protocol left, `setcurrentpoint` outside a flex,
/// and a hint replacement's subroutine number drawn with.
#[test]
fn the_protocols_are_held_to_their_forms() {
    let pop = cs(&[&start(500, 0, 0), &POP, &[RLINETO, ENDCHAR]]);
    assert_eq!(refused(pop, &[]), Type1CffRefusal::Protocol);
    let point = cs(&[
        &start(500, 0, 0),
        &n(&[10, 10]),
        &SETCURRENTPOINT,
        &[ENDCHAR],
    ]);
    assert_eq!(refused(point, &[]), Type1CffRefusal::Protocol);
    let drawn = cs(&[
        &start(500, 0, 0),
        &n(&[3, 1, 3]),
        &CALLOTHERSUBR,
        &POP,
        &n(&[0]),
        &[RLINETO, ENDCHAR],
    ]);
    assert_eq!(refused(drawn, &[]), Type1CffRefusal::Protocol);
}

/// `sbw` converts with no vertical metrics and is refused with either.
#[test]
fn an_sbw_with_vertical_metrics_is_refused() {
    for operands in [[0, 10, 600, 0], [0, 0, 600, 50]] {
        let glyph = cs(&[
            &n(&operands),
            &SBW,
            &n(&[0, 0]),
            &[RMOVETO],
            &n(&[100]),
            &[HLINETO, ENDCHAR],
        ]);
        assert_eq!(refused(glyph, &[]), Type1CffRefusal::VerticalMetrics);
    }
}

#[test]
fn more_stems_than_type2_declares_are_refused() {
    let mut glyph = cs(&[&n(&[0, 500]), &[HSBW]]);
    for at in 0..97 {
        glyph.extend(n(&[at * 10, 5]));
        glyph.push(HSTEM);
    }
    glyph.extend(cs(&[
        &n(&[0, 0]),
        &[RMOVETO],
        &n(&[10]),
        &[HLINETO, ENDCHAR],
    ]));
    assert_eq!(refused(glyph, &[]), Type1CffRefusal::TooManyStems);
}

/// A flex of six points, one inside another, one never ended, and one a
/// line interrupts.
#[test]
fn a_flex_outside_chapter_8_3s_form_is_refused() {
    let flex = |points: usize, between: &[u8], end: bool| {
        let mut code = cs(&[&start(500, 100, -10), &n(&[1]), &[CALLSUBR]]);
        code.extend_from_slice(between);
        for _ in 0..points {
            code.extend(n(&[10, 0]));
            code.push(RMOVETO);
            code.extend(n(&[2]));
            code.push(CALLSUBR);
        }
        if end {
            code.extend(n(&[50, 200, -10, 0]));
            code.extend([CALLSUBR, CLOSEPATH, ENDCHAR]);
        }
        code
    };
    assert_eq!(refused(flex(6, &[], true), &[]), Type1CffRefusal::Flex);
    assert_eq!(
        refused(flex(7, &cs(&[&n(&[1]), &[CALLSUBR]]), true), &[]),
        Type1CffRefusal::Flex
    );
    assert_eq!(refused(flex(3, &[], false), &[]), Type1CffRefusal::Flex);
    assert_eq!(
        refused(flex(7, &cs(&[&n(&[10, 10]), &[RLINETO]]), true), &[]),
        Type1CffRefusal::Flex
    );
    // And the well-formed one converts.
    assert!(type1_to_cff(&single(flex(7, &[], true), &[])).is_ok());
}

/// A hint replacement naming no subroutine, naming one the program lacks,
/// or naming one that draws.
#[test]
fn a_hint_replacement_that_declares_no_stems_is_refused() {
    let replace = |count: i32, subr: i32| {
        let mut code = start(500, 0, 0);
        if count > 0 {
            code.extend(n(&[subr]));
        }
        code.extend(n(&[count, 3]));
        code.extend_from_slice(&CALLOTHERSUBR);
        code.extend_from_slice(&POP);
        code.extend([CALLSUBR, ENDCHAR]);
        code
    };
    assert_eq!(
        refused(replace(0, 0), &[]),
        Type1CffRefusal::HintReplacement
    );
    assert_eq!(
        refused(replace(1, 40), &[]),
        Type1CffRefusal::HintReplacement
    );
    let draws = (9, cs(&[&n(&[10, 10]), &[RLINETO, RETURN]]));
    assert_eq!(
        refused(replace(1, 9), &[draws]),
        Type1CffRefusal::HintReplacement
    );
    // A replacement whose subroutine declares stems converts.
    assert!(type1_to_cff(&single(replace(1, 6), &[])).is_ok());
}

/// A line drawn after `closepath` and one before the first move.
#[test]
fn a_path_drawn_without_a_move_is_refused() {
    let after = cs(&[
        &start(500, 0, 0),
        &n(&[100]),
        &[HLINETO, CLOSEPATH],
        &n(&[100]),
        &[VLINETO, ENDCHAR],
    ]);
    assert_eq!(refused(after, &[]), Type1CffRefusal::Path);
    let before = cs(&[&n(&[0, 500]), &[HSBW], &n(&[100]), &[HLINETO, ENDCHAR]]);
    assert_eq!(refused(before, &[]), Type1CffRefusal::Path);
}

/// A `seac` whose accent the program lacks, and one whose base code names
/// no standard glyph.
#[test]
fn a_seac_whose_component_is_missing_is_refused() {
    for (bchar, achar) in [(65, 197), (1, 194)] {
        let glyph = cs(&[
            &n(&[0, 600]),
            &[HSBW],
            &n(&[0, 100, 200, bchar, achar]),
            &SEAC,
        ]);
        assert_eq!(refused(glyph, &[]), Type1CffRefusal::Accent);
    }
}

#[test]
fn two_charstrings_under_one_name_are_refused() {
    let program = Face {
        glyphs: named(vec![
            (".notdef", cs(&[&n(&[0, 250]), &[HSBW, ENDCHAR]])),
            ("A", rect(600, 20, 0, 500, 500)),
            ("A", rect(600, 30, 0, 400, 600)),
        ]),
        encoding: Encoding::Entries(vec![(65, "A")]),
        ..face()
    }
    .build();
    assert_eq!(type1_to_cff(&program), Err(Type1CffRefusal::DuplicateName));
}

#[test]
fn a_glyph_name_the_cff_reader_cannot_look_up_is_refused() {
    let program = Face {
        glyphs: vec![
            (b".notdef".to_vec(), cs(&[&n(&[0, 250]), &[HSBW, ENDCHAR]])),
            (vec![0xC3, 0x28], rect(600, 20, 0, 500, 500)),
        ],
        encoding: Encoding::Entries(Vec::new()),
        ..face()
    }
    .build();
    assert_eq!(type1_to_cff(&program), Err(Type1CffRefusal::GlyphName));
}

#[test]
fn a_font_name_a_cff_cannot_carry_is_refused() {
    let program = Face {
        font_name: "A".repeat(128),
        ..face()
    }
    .build();
    assert_eq!(type1_to_cff(&program), Err(Type1CffRefusal::FontName));
}

/// A call past the array, into a hole of it, and to a negative index.
#[test]
fn a_call_to_a_subroutine_the_program_does_not_define_is_refused() {
    for index in [50, 20, -1] {
        let glyph = cs(&[&start(500, 0, 0), &n(&[index]), &[CALLSUBR, ENDCHAR]]);
        // Subroutine 21 makes 20 a hole below it.
        let past = (21, vec![RETURN]);
        assert_eq!(
            refused(glyph, &[past]),
            Type1CffRefusal::MissingSubroutine,
            "subroutine {index}"
        );
    }
}

#[test]
fn subroutines_past_ten_deep_are_refused() {
    let looping = (20, cs(&[&n(&[20]), &[CALLSUBR, RETURN]]));
    let glyph = cs(&[&start(500, 0, 0), &n(&[20]), &[CALLSUBR, ENDCHAR]]);
    assert_eq!(refused(glyph, &[looping]), Type1CffRefusal::SubroutineDepth);
}

#[test]
fn an_operand_past_the_stack_is_refused() {
    let mut glyph = start(500, 0, 0);
    for _ in 0..49 {
        glyph.extend(n(&[1]));
    }
    glyph.extend([RLINETO, ENDCHAR]);
    assert_eq!(refused(glyph, &[]), Type1CffRefusal::StackOverflow);
}

#[test]
fn a_glyph_past_the_operator_budget_is_refused() {
    let mut glyph = start(500, 0, 0);
    for _ in 0..34_000 {
        glyph.extend(n(&[3]));
        glyph.push(CALLSUBR);
    }
    glyph.push(ENDCHAR);
    assert_eq!(refused(glyph, &[]), Type1CffRefusal::Budget);
}

/// 100 000 / 3 is a quotient a 16.16 operand does not hold, of a dividend
/// no Type 2 operand holds.
#[test]
fn a_quotient_no_operand_holds_is_refused() {
    let glyph = cs(&[
        &start(500, 0, 0),
        &n(&[100_000, 3]),
        &DIV,
        &n(&[0]),
        &[RLINETO, ENDCHAR],
    ]);
    assert_eq!(refused(glyph, &[]), Type1CffRefusal::Number);
}

/// Lines enough that their Type 2 charstring passes 65 535 bytes.
#[test]
fn a_charstring_past_type2s_bound_is_refused() {
    let mut glyph = start(500, 0, 0);
    for _ in 0..32_900 {
        glyph.extend(n(&[1, 1]));
        glyph.push(RLINETO);
    }
    glyph.push(ENDCHAR);
    assert_eq!(refused(glyph, &[]), Type1CffRefusal::TooLarge);
}

/// The converter's own check refuses a CFF that advances a glyph otherwise,
/// and glyphs it would find under other names.
#[test]
fn a_conversion_that_draws_otherwise_is_a_mismatch() {
    let program = face().build();
    let font = Type1::parse(&program).expect("it parses");
    let converted = type1_to_cff(&program).expect("it converts");
    assert_eq!(
        verify(&font, &converted.program, &converted.glyphs, false, &mut 0),
        Ok(())
    );

    // A's charstring opens with its width, 600, in the two-byte form; one
    // more in its second byte is 601.
    let a = gid_of(&converted, &font, b"A");
    let code = charstring(&converted.program, a);
    assert_eq!(operators(&code)[0].operands.first(), Some(&600.0));
    assert!((247..=250).contains(&code[0]), "{code:?}");
    let at = find(&converted.program, &code).expect("A's charstring") + 1;
    let mut wrong = converted.program.clone();
    wrong[at] = wrong[at].wrapping_add(1);
    assert_eq!(
        verify(&font, &wrong, &converted.glyphs, false, &mut 0),
        Err(Type1CffRefusal::Mismatch)
    );

    let mut swapped = converted.glyphs.clone();
    swapped.swap(1, 2);
    assert_eq!(
        verify(&font, &converted.program, &swapped, false, &mut 0),
        Err(Type1CffRefusal::Mismatch)
    );
}

// ---------------------------------------------------------------------------
// Hostile input.
// ---------------------------------------------------------------------------

/// Every prefix of the fixture converts or is refused, and whatever converts
/// is a CFF that parses.
#[test]
fn every_truncation_is_survivable() {
    let program = face().build();
    for cut in (0..program.len()).step_by(11) {
        if let Ok(converted) = type1_to_cff(&program[..cut]) {
            assert!(Cff::parse(&converted.program).is_some(), "cut at {cut}");
        }
    }
}

/// Bytes of the encrypted portion changed at random — every byte of a
/// damaged program decrypts to *something* — convert or are refused,
/// and whatever converts draws as the reader draws it.
#[test]
fn damaged_programs_are_declined_or_converted_not_panicked_on() {
    let program = face().build();
    let from = find(&program, b"eexec").expect("eexec") + 6;
    let mut state = 0x2545_F491u32;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    };
    for _ in 0..300 {
        let mut damaged = program.clone();
        for _ in 0..3 {
            let at = from + (next() as usize) % (damaged.len() - from);
            damaged[at] ^= (next() & 0xFF) as u8 | 1;
        }
        if let Ok(converted) = type1_to_cff(&damaged) {
            draws_alike(&damaged, &converted);
        }
    }
}
