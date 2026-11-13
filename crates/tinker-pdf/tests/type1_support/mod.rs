//! Type 1 programs written byte by byte, and pages that embed them, for the
//! facade's multiple-master tests.
//!
//! `multiple_master.rs` and `determinism.rs` share it. The program is the
//! one `tinker-pdf-font`'s own instancing tests build: two axes — a weight
//! axis mapped linearly from 100 to 900, a width axis of three points — and
//! four masters at the corners of the blend space, every operand of its one
//! glyph, `A`, blended. Beside it, [`instance_program`] writes a plain program
//! of one master whose operands are an instance's, blended **here**, from the
//! blend design, by the supplement's arithmetic: a page embedding the first
//! under `/MMType1` and one embedding the second under `/Type1` are held to
//! one bitmap.
//!
//! Every design vector used picks weights that are fractions of a power of
//! two, so an instance's operands are exact in `f64` and the plain program can
//! write each as `p q div`, which divides back to the same bits.
//!
//! # Why the allow
//!
//! Each binary compiles its own copy of this module and uses a different
//! subset.

#![allow(
    dead_code,
    reason = "shared by two test binaries; each uses a different subset"
)]

/// The eexec key (the Type 1 book's chapter 7).
const EEXEC_KEY: u16 = 55_665;
/// The charstring key.
const CHARSTRING_KEY: u16 = 4330;

/// Adobe's cipher in the encrypting direction, `pad` leading bytes first, as
/// a font tool writes it.
fn encrypt(plain: &[u8], key: u16, pad: usize) -> Vec<u8> {
    const C1: u16 = 52_845;
    const C2: u16 = 22_719;
    let mut r = key;
    let mut out = Vec::with_capacity(plain.len() + pad);
    for byte in core::iter::repeat(0x55u8)
        .take(pad)
        .chain(plain.iter().copied())
    {
        let cipher = byte ^ (r >> 8) as u8;
        r = (u16::from(cipher).wrapping_add(r))
            .wrapping_mul(C1)
            .wrapping_add(C2);
        out.push(cipher);
    }
    out
}

/// One charstring number, in the shortest of the Type 1 encodings.
fn number(v: i32, out: &mut Vec<u8>) {
    if (-107..=107).contains(&v) {
        out.push((v + 139) as u8);
    } else if (108..=1131).contains(&v) {
        let u = v - 108;
        out.push((u / 256 + 247) as u8);
        out.push((u % 256) as u8);
    } else if (-1131..=-108).contains(&v) {
        let u = -v - 108;
        out.push((u / 256 + 251) as u8);
        out.push((u % 256) as u8);
    } else {
        out.push(255);
        out.extend_from_slice(&v.to_be_bytes());
    }
}

/// One operand exactly: an integer as itself, a fraction whose denominator is
/// a power of two as `p q div`.
fn exact(v: f64, out: &mut Vec<u8>) {
    for shift in 0..=16u32 {
        let q = f64::from(1u32 << shift);
        let p = v * q;
        if p == p.trunc() && p.abs() <= f64::from(i32::MAX) {
            number(p as i32, out);
            if shift > 0 {
                number(1 << shift, out);
                out.extend_from_slice(&[12, 12]); // div
            }
            return;
        }
    }
    panic!("{v} is no fraction of a power of two");
}

/// One blended group of a multiple-master charstring, as the supplement's
/// section 3.11 writes it: each quantity's first-master value, each
/// quantity's deltas, the count, the blend othersubr, `callothersubr` and one
/// `pop` a quantity.
fn blended(masters: &[Vec<i32>], out: &mut Vec<u8>) {
    let quantities = masters[0].len();
    for q in 0..quantities {
        number(masters[0][q], out);
    }
    for q in 0..quantities {
        for master in &masters[1..] {
            number(master[q] - masters[0][q], out);
        }
    }
    number((masters.len() * quantities) as i32, out);
    number(
        match quantities {
            1 => 14,
            2 => 15,
            3 => 16,
            4 => 17,
            6 => 18,
            other => panic!("no blend othersubr takes {other} quantities"),
        },
        out,
    );
    out.extend_from_slice(&[12, 16]); // callothersubr
    for _ in 0..quantities {
        out.extend_from_slice(&[12, 17]); // pop
    }
}

/// The glyph `A`'s four masters: an advance, a move, a box's two sides and a
/// curve back over its top, each quantity different in every master.
pub struct Masters {
    pub hsbw: Vec<Vec<i32>>,
    pub moveto: Vec<Vec<i32>>,
    pub width: Vec<Vec<i32>>,
    pub height: Vec<Vec<i32>>,
    pub curve_a: Vec<Vec<i32>>,
    pub curve_b: Vec<Vec<i32>>,
}

/// The masters, in the program's order: at the corners `[0 0]`, `[1 0]`,
/// `[0 1]` and `[1 1]` of the blend space.
pub fn masters() -> Masters {
    Masters {
        hsbw: vec![vec![10, 500], vec![30, 700], vec![50, 600], vec![70, 900]],
        moveto: vec![vec![0, 0], vec![20, 10], vec![40, 20], vec![60, 30]],
        width: vec![vec![400], vec![600], vec![500], vec![800]],
        height: vec![vec![300], vec![300], vec![500], vec![500]],
        curve_a: vec![
            vec![-100, 50, -100],
            vec![-150, 60, -150],
            vec![-120, 70, -130],
            vec![-200, 80, -200],
        ],
        curve_b: vec![
            vec![0, -200, -50],
            vec![0, -300, -60],
            vec![10, -250, -80],
            vec![20, -400, -100],
        ],
    }
}

/// The program's own `/WeightVector`: its default instance.
pub const WEIGHT_VECTOR: [f64; 4] = [0.125, 0.375, 0.125, 0.375];

/// The test's own piecewise-linear map (the supplement's section 3.8).
fn through_map(map: &[(f64, f64)], d: f64) -> f64 {
    if d <= map[0].0 {
        return map[0].1;
    }
    for pair in map.windows(2) {
        if d <= pair[1].0 {
            let ((d0, b0), (d1, b1)) = (pair[0], pair[1]);
            return b0 + (d - d0) * (b1 - b0) / (d1 - d0);
        }
    }
    map[map.len() - 1].1
}

/// The weights at a design vector, computed here from the blend design:
/// each coordinate through its axis's map, and each master weighted by its
/// corner (section 3.9).
pub fn weights_at(design: [f64; 2]) -> [f64; 4] {
    let b = [
        through_map(&[(100.0, 0.0), (900.0, 1.0)], design[0]),
        through_map(&[(0.0, 0.0), (400.0, 0.5), (1000.0, 1.0)], design[1]),
    ];
    let corners = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
    let mut out = [0.0; 4];
    for (weight, corner) in out.iter_mut().zip(corners) {
        *weight = (0..2)
            .map(|axis| {
                if corner[axis] == 1.0 {
                    b[axis]
                } else {
                    1.0 - b[axis]
                }
            })
            .product();
    }
    out
}

/// Each quantity of a group blended at `weights`: the masters' values times
/// their weights, summed.
fn mix(group: &[Vec<i32>], weights: &[f64; 4]) -> Vec<f64> {
    (0..group[0].len())
        .map(|q| {
            group
                .iter()
                .zip(weights)
                .map(|(master, w)| f64::from(master[q]) * w)
                .sum()
        })
        .collect()
}

/// A program whose cleartext carries `keys` and whose `/CharStrings` holds
/// `A`, its built-in `/Encoding` putting `A` at 65.
fn program(keys: &str, charstring: &[u8]) -> Vec<u8> {
    let encrypted = encrypt(charstring, CHARSTRING_KEY, 4);
    let mut private = Vec::new();
    private.extend_from_slice(b"dup /Private 8 dict dup begin\n/lenIV 4 def\n");
    private.extend_from_slice(b"/Subrs 0 array ND\n/CharStrings 1 dict dup begin\n");
    private.extend_from_slice(format!("/A {} RD ", encrypted.len()).as_bytes());
    private.extend_from_slice(&encrypted);
    private.extend_from_slice(b" ND\nend\nend\n");

    let mut out = Vec::new();
    out.extend_from_slice(b"%!PS-AdobeFont-1.0: TestMM 001.000\n");
    out.extend_from_slice(keys.as_bytes());
    out.extend_from_slice(b"/FontMatrix [0.001 0 0 0.001 0 0] readonly def\n");
    out.extend_from_slice(b"/Encoding 256 array\ndup 65 /A put\nreadonly def\n");
    out.extend_from_slice(b"currentfile eexec\n");
    out.extend_from_slice(&encrypt(&private, EEXEC_KEY, 4));
    out
}

/// The two-axis, four-master program: its blend design, its weight vector,
/// and `A` with every operand blended, the curve's six split over two calls of
/// othersubr 16 as section 3.11 splits them.
pub fn multiple_master_program() -> Vec<u8> {
    let masters = masters();
    let mut cs = Vec::new();
    blended(&masters.hsbw, &mut cs);
    cs.push(13); // hsbw
    blended(&masters.moveto, &mut cs);
    cs.push(21); // rmoveto
    blended(&masters.width, &mut cs);
    cs.push(6); // hlineto
    blended(&masters.height, &mut cs);
    cs.push(7); // vlineto
    blended(&masters.curve_a, &mut cs);
    blended(&masters.curve_b, &mut cs);
    cs.push(8); // rrcurveto
    cs.push(9); // closepath
    cs.push(14); // endchar
    program(
        "/FontInfo 5 dict dup begin\n\
         /BlendAxisTypes [/Weight /Width ] def\n\
         /BlendDesignPositions [[0 0] [1 0] [0 1] [1 1]] def\n\
         /BlendDesignMap [[[100 0][900 1]] [[0 0][400 0.5][1000 1]]] def\n\
         end readonly def\n\
         /FontName /TestMM def\n\
         /WeightVector [0.125 0.375 0.125 0.375 ] def\n",
        &cs,
    )
}

/// A plain Type 1 program of one master, the instance at `weights`: `A`'s
/// every operand the four masters blended here, written exactly.
pub fn instance_program(weights: &[f64; 4]) -> Vec<u8> {
    let masters = masters();
    let mut cs = Vec::new();
    for value in mix(&masters.hsbw, weights) {
        exact(value, &mut cs);
    }
    cs.push(13);
    for value in mix(&masters.moveto, weights) {
        exact(value, &mut cs);
    }
    cs.push(21);
    exact(mix(&masters.width, weights)[0], &mut cs);
    cs.push(6);
    exact(mix(&masters.height, weights)[0], &mut cs);
    cs.push(7);
    for value in mix(&masters.curve_a, weights)
        .into_iter()
        .chain(mix(&masters.curve_b, weights))
    {
        exact(value, &mut cs);
    }
    cs.push(8);
    cs.push(9);
    cs.push(14);
    program("/FontName /TestMMInstance def\n", &cs)
}

/// One page, `width` by `height`, its font `/F0` of `subtype` named
/// `base_font` and embedding `program` under `/FontFile`, drawing `content`;
/// with a cross-reference table, so it opens without repair.
pub fn page(
    subtype: &str,
    base_font: &str,
    program: &[u8],
    content: &str,
    width: u32,
    height: u32,
) -> Vec<u8> {
    // A name holds no space as itself (7.3.5): 9.6.2.3's names turn theirs
    // to underscores, and one that kept them writes `#20`.
    let base_font = base_font.replace(' ', "#20");
    let mut objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] \
             /Resources << /Font << /F0 4 0 R >> >> /Contents 6 0 R >>"
        )
        .into_bytes(),
        format!(
            "<< /Type /Font /Subtype /{subtype} /BaseFont /{base_font} \
             /FirstChar 65 /LastChar 65 /Widths [700] /FontDescriptor 5 0 R >>"
        )
        .into_bytes(),
        format!(
            "<< /Type /FontDescriptor /FontName /{base_font} /Flags 32 \
             /FontBBox [-100 -200 1000 1000] /ItalicAngle 0 /Ascent 1000 /Descent -200 \
             /CapHeight 700 /StemV 80 /FontFile 7 0 R >>"
        )
        .into_bytes(),
    ];
    let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
    stream.extend_from_slice(content.as_bytes());
    stream.extend_from_slice(b"\nendstream");
    objects.push(stream);
    // 9.9's Table 125: the cleartext up to and including `eexec` and its end
    // of line, then the encrypted rest, and no fixed-content part.
    let clear = program
        .windows(6)
        .position(|window| window == b"eexec\n")
        .map_or(program.len(), |at| at + 6);
    let mut font = format!(
        "<< /Length {} /Length1 {clear} /Length2 {} /Length3 0 >>\nstream\n",
        program.len(),
        program.len() - clear
    )
    .into_bytes();
    font.extend_from_slice(program);
    font.extend_from_slice(b"\nendstream");
    objects.push(font);

    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// The content every page here draws: `A` twice, at two sizes, the second
/// on a half point so its edges land on another sub-pixel phase.
pub const CONTENT: &str = "BT /F0 40 Tf 8 12 Td (A) Tj ET BT /F0 24 Tf 52.5 20 Td (A) Tj ET";

/// The page every test here draws: [`CONTENT`] on a 100 by 60 page.
pub fn content_page(subtype: &str, base_font: &str, program: &[u8]) -> Vec<u8> {
    page(subtype, base_font, program, CONTENT, 100, 60)
}
