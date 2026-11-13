//! Size-scaling complexity tests for the font parsers
//! (`docs/design/resource-exhaustion.md`, "The size-scaling law").
//!
//! Each shape is a font this file writes at a size n and at four times n,
//! read through the counted siblings of the parsers' doors, which return the
//! steps they took: table records, subtable entries, points, components,
//! INDEX entries, charstring operands and operators, decrypted and
//! decompressed bytes, CMap tokens, codespace ranges and ranges. The law is
//! the design's: the work at 4 n is at most 1.1 × 4 times the work at n, and
//! for a shape whose lookups bisect, 1.1 × 4 × log₂(4 n) ÷ log₂ n times. A
//! quadratic shape costs sixteen times as much and an n¹·⁵ one eight.
//!
//! Every shape is held to what it is as well as to what it costs: each read
//! gives back what was written, through the counted and the uncounted doors
//! alike. No clock is read here. Nothing here depends on a feature, so the
//! file runs alike with and without the default ones.

use tinker_pdf_font::outline::{Outline, Segment};
use tinker_pdf_font::{cmap, glyf, woff, Cff, Sfnt, Type1};

/// Four times the size: the factor the law is stated for.
const K: u64 = 4;

/// The law for a shape a parser reads each element of a bounded number of
/// times, with both figures in the failure.
fn holds_the_law(shape: &str, n: u64, kn: u64) {
    println!("  {shape:24} work at n {n:>8}  at 4n {kn:>8}");
    assert!(n > 0, "{shape}: nothing was counted at n");
    assert!(
        kn * 10 <= n * K * 11,
        "{shape}: {kn} steps at 4n against {n} at n, past 1.1 x {K}"
    );
}

/// The law for a shape whose lookups bisect: one more factor of
/// log₂(4 n) ÷ log₂ n, for `size` elements at n.
fn holds_the_log_law(shape: &str, size: u64, n: u64, kn: u64) {
    println!("  {shape:24} work at n {n:>8}  at 4n {kn:>8}");
    assert!(n > 0, "{shape}: nothing was counted at n");
    let log = |x: u64| f64::from(u64::BITS - x.leading_zeros());
    let bound = 1.1 * K as f64 * log(K * size) / log(size);
    assert!(
        kn as f64 <= bound * n as f64,
        "{shape}: {kn} steps at 4n against {n} at n, past {bound:.2}"
    );
}

// ---------------------------------------------------------------------------
// The sfnt container.
// ---------------------------------------------------------------------------

/// An sfnt holding `tables` in the order given, each padded to four bytes.
fn sfnt(tables: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let count = tables.len();
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&(count as u16).to_be_bytes());
    out.extend_from_slice(&[0; 6]);
    let mut offset = 12 + 16 * count;
    for (tag, table) in tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(table.len() as u32).to_be_bytes());
        offset += table.len().div_ceil(4) * 4;
    }
    for (_, table) in tables {
        out.extend_from_slice(table);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    out
}

/// A `head` whose `unitsPerEm` is 1000 and whose `indexToLocFormat` says
/// `loca` is long.
fn head() -> Vec<u8> {
    let mut head = vec![0u8; 54];
    head[18..20].copy_from_slice(&1000u16.to_be_bytes());
    head[50..52].copy_from_slice(&1u16.to_be_bytes());
    head
}

/// An `hhea` with one full metric.
fn hhea() -> Vec<u8> {
    let mut hhea = vec![0u8; 36];
    hhea[34..36].copy_from_slice(&1u16.to_be_bytes());
    hhea
}

/// A face of `n` tables of four bytes each, beside `head` and `hhea`.
fn many_tables(n: usize) -> Vec<u8> {
    let mut tables = vec![(*b"head", head()), (*b"hhea", hhea())];
    for i in 0..n {
        let tag = format!("T{i:03}");
        let mut name = [0u8; 4];
        name.copy_from_slice(tag.as_bytes());
        tables.push((name, (i as u32).to_be_bytes().to_vec()));
    }
    tables.sort_by_key(|(tag, _)| *tag);
    sfnt(&tables)
}

fn directory(n: usize) -> u64 {
    let face = many_tables(n);
    let (font, steps) = Sfnt::parse_counted(&face);
    let font = font.expect("the face parses");
    assert_eq!(font.units_per_em, 1000, "head was found");
    for i in [0, n - 1] {
        let tag = u32::from_be_bytes(*format!("T{i:03}").as_bytes().first_chunk().unwrap());
        assert_eq!(
            font.table(tag),
            Some(&(i as u32).to_be_bytes()[..]),
            "table {i} reads back"
        );
    }
    assert!(
        Sfnt::parse(&face).is_some(),
        "and through the uncounted door"
    );
    steps
}

#[test]
fn the_sfnt_table_directory_costs_work_linear_in_its_tables() {
    // Inside the directory's 512 records at both sizes.
    holds_the_law("sfnt directory", directory(32), directory(128));
}

// ---------------------------------------------------------------------------
// cmap subtables.
// ---------------------------------------------------------------------------

/// A `cmap` with one encoding record, `(platform, encoding)`, over `subtable`.
fn cmap_table(platform: u16, encoding: u16, subtable: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&platform.to_be_bytes());
    out.extend_from_slice(&encoding.to_be_bytes());
    out.extend_from_slice(&12u32.to_be_bytes());
    out.extend_from_slice(subtable);
    out
}

/// The codes the subtable shapes map: every other code from U+0100, so each
/// is a segment or a group of its own, clear of the surrogates.
fn code(i: usize) -> char {
    char::from_u32(0x100 + 2 * i as u32).expect("a scalar value")
}

/// A format 4 subtable of `n` one-code segments, code `i` to glyph `i + 1`,
/// and the closing `0xFFFF` segment.
fn format_4(n: usize) -> Vec<u8> {
    let segs = n + 1;
    let mut ends = Vec::new();
    let mut starts = Vec::new();
    let mut deltas = Vec::new();
    for i in 0..n {
        let c = code(i) as u32 as u16;
        ends.extend_from_slice(&c.to_be_bytes());
        starts.extend_from_slice(&c.to_be_bytes());
        // glyph = code + delta, modulo 65 536.
        deltas.extend_from_slice(&((i as u16 + 1).wrapping_sub(c)).to_be_bytes());
    }
    ends.extend_from_slice(&0xFFFFu16.to_be_bytes());
    starts.extend_from_slice(&0xFFFFu16.to_be_bytes());
    deltas.extend_from_slice(&1u16.to_be_bytes());
    let mut sub = Vec::new();
    sub.extend_from_slice(&4u16.to_be_bytes());
    let length = 16 + 8 * segs;
    sub.extend_from_slice(&(length as u16).to_be_bytes());
    sub.extend_from_slice(&0u16.to_be_bytes());
    sub.extend_from_slice(&((2 * segs) as u16).to_be_bytes());
    sub.extend_from_slice(&[0; 6]);
    sub.extend_from_slice(&ends);
    sub.extend_from_slice(&0u16.to_be_bytes());
    sub.extend_from_slice(&starts);
    sub.extend_from_slice(&deltas);
    sub.extend_from_slice(&vec![0u8; 2 * segs]);
    sub
}

/// A format 12 subtable of `n` one-code groups, code `i` to glyph `i + 1`.
fn format_12(n: usize) -> Vec<u8> {
    let mut sub = Vec::new();
    sub.extend_from_slice(&12u16.to_be_bytes());
    sub.extend_from_slice(&0u16.to_be_bytes());
    sub.extend_from_slice(&((16 + 12 * n) as u32).to_be_bytes());
    sub.extend_from_slice(&0u32.to_be_bytes());
    sub.extend_from_slice(&(n as u32).to_be_bytes());
    for i in 0..n {
        let c = code(i) as u32;
        sub.extend_from_slice(&c.to_be_bytes());
        sub.extend_from_slice(&c.to_be_bytes());
        sub.extend_from_slice(&(i as u32 + 1).to_be_bytes());
    }
    sub
}

/// Every segment's code mapped once, through a face whose `cmap` is `table`.
fn mapped(n: usize, table: Vec<u8>) -> u64 {
    let face = sfnt(&[(*b"cmap", table), (*b"head", head())]);
    let font = Sfnt::parse(&face).expect("the face parses");
    let plain = Sfnt::parse(&face).expect("the face parses");
    let mut steps = 0u64;
    for i in 0..n {
        let (glyph, taken) = font.glyph_for_char_counted(code(i));
        assert_eq!(glyph, Some(i as u16 + 1), "code {i} maps to its glyph");
        assert_eq!(plain.glyph_for_char(code(i)), glyph, "alike uncounted");
        steps += taken;
    }
    steps
}

#[test]
fn a_format_4_subtable_of_more_segments_costs_work_n_log_n_in_its_segments() {
    let at = |n| mapped(n, cmap_table(3, 1, &format_4(n)));
    holds_the_log_law("cmap format 4", 500, at(500), at(2_000));
}

#[test]
fn a_format_12_subtable_of_more_groups_costs_work_n_log_n_in_its_groups() {
    let at = |n| mapped(n, cmap_table(3, 10, &format_12(n)));
    holds_the_log_law("cmap format 12", 500, at(500), at(2_000));
}

/// A subtable written out of order is read as written, the first match
/// winning, as it always was: the order check reaches only the subtables a
/// bisection reads alike.
#[test]
fn a_subtable_out_of_order_still_matches_its_first_segment() {
    // Two groups both holding U+0102, the second written first: a scan finds
    // the one written first, which maps to glyph 9.
    let mut sub = Vec::new();
    sub.extend_from_slice(&12u16.to_be_bytes());
    sub.extend_from_slice(&0u16.to_be_bytes());
    sub.extend_from_slice(&40u32.to_be_bytes());
    sub.extend_from_slice(&0u32.to_be_bytes());
    sub.extend_from_slice(&2u32.to_be_bytes());
    for (start, end, glyph) in [(0x102u32, 0x102u32, 9u32), (0x100, 0x104, 3)] {
        sub.extend_from_slice(&start.to_be_bytes());
        sub.extend_from_slice(&end.to_be_bytes());
        sub.extend_from_slice(&glyph.to_be_bytes());
    }
    let face = sfnt(&[(*b"cmap", cmap_table(3, 10, &sub)), (*b"head", head())]);
    let font = Sfnt::parse(&face).expect("the face parses");
    assert_eq!(font.glyph_for_char('\u{102}'), Some(9));
    assert_eq!(
        font.glyph_for_char('\u{101}'),
        Some(4),
        "3 + (0x101 - 0x100)"
    );
}

// ---------------------------------------------------------------------------
// glyf outlines.
// ---------------------------------------------------------------------------

/// A face whose `glyf` holds `glyphs`, with a long `loca` over them.
fn glyf_face(glyphs: &[Vec<u8>]) -> Vec<u8> {
    let mut glyf = Vec::new();
    let mut loca = Vec::new();
    for glyph in glyphs {
        loca.extend_from_slice(&(glyf.len() as u32).to_be_bytes());
        glyf.extend_from_slice(glyph);
        while glyf.len() % 4 != 0 {
            glyf.push(0);
        }
    }
    loca.extend_from_slice(&(glyf.len() as u32).to_be_bytes());
    sfnt(&[(*b"glyf", glyf), (*b"head", head()), (*b"loca", loca)])
}

/// A simple glyph of one contour of `n` on-curve points, each one font unit
/// right of and above the last.
fn simple_glyph(n: usize) -> Vec<u8> {
    let mut glyph = Vec::new();
    glyph.extend_from_slice(&1i16.to_be_bytes());
    glyph.extend_from_slice(&[0; 8]);
    glyph.extend_from_slice(&((n - 1) as u16).to_be_bytes());
    glyph.extend_from_slice(&0u16.to_be_bytes());
    // On the curve, both deltas one positive byte.
    glyph.extend(std::iter::repeat_n(0x01 | 0x02 | 0x04 | 0x10 | 0x20, n));
    glyph.extend(std::iter::repeat_n(1u8, n));
    glyph.extend(std::iter::repeat_n(1u8, n));
    glyph
}

fn simple_outline(n: usize) -> u64 {
    let face = glyf_face(&[simple_glyph(n)]);
    let font = Sfnt::parse(&face).expect("the face parses");
    let (outline, steps) = glyf::outline_counted(&font, 0);
    let outline = outline.expect("an outline");
    let lines = outline
        .segments
        .iter()
        .filter(|s| matches!(s, Segment::LineTo { .. }))
        .count();
    assert_eq!(
        lines, n,
        "a line to every point but the first, and one closing it"
    );
    assert_eq!(
        glyf::outline(&font, 0).map(|o| o.segments),
        Some(outline.segments),
        "alike uncounted"
    );
    steps
}

#[test]
fn a_simple_glyph_of_more_points_costs_work_linear_in_its_points() {
    holds_the_law("glyf simple", simple_outline(1_000), simple_outline(4_000));
}

/// A composite of `n` components, each the triangle at glyph 1 placed at its
/// own offset.
fn composite_outline(n: usize) -> u64 {
    let mut composite = Vec::new();
    composite.extend_from_slice(&(-1i16).to_be_bytes());
    composite.extend_from_slice(&[0; 8]);
    for i in 0..n {
        let more = if i + 1 < n { 0x0020 } else { 0 };
        // ARG_1_AND_2_ARE_WORDS and ARGS_ARE_XY_VALUES.
        composite.extend_from_slice(&(0x0001u16 | 0x0002 | more).to_be_bytes());
        composite.extend_from_slice(&1u16.to_be_bytes());
        composite.extend_from_slice(&(10 * i as i16).to_be_bytes());
        composite.extend_from_slice(&0i16.to_be_bytes());
    }
    let face = glyf_face(&[composite, simple_glyph(3)]);
    let font = Sfnt::parse(&face).expect("the face parses");
    let (outline, steps) = glyf::outline_counted(&font, 0);
    let outline = outline.expect("an outline");
    let contours = outline
        .segments
        .iter()
        .filter(|s| matches!(s, Segment::MoveTo { .. }))
        .count();
    assert_eq!(contours, n, "every component drew its contour");
    assert_eq!(
        glyf::outline(&font, 0).map(|o| o.segments),
        Some(outline.segments),
        "alike uncounted"
    );
    steps
}

#[test]
fn a_composite_of_more_components_costs_work_linear_in_its_components() {
    // Inside the 64 components one composite reads and the 256 a glyph may
    // draw in all.
    holds_the_law(
        "glyf composite",
        composite_outline(12),
        composite_outline(48),
    );
}

// ---------------------------------------------------------------------------
// CFF.
// ---------------------------------------------------------------------------

/// A CFF INDEX of `items`, with four-byte offsets.
fn index(items: &[Vec<u8>]) -> Vec<u8> {
    let mut out = (items.len() as u16).to_be_bytes().to_vec();
    if items.is_empty() {
        return out;
    }
    out.push(4);
    let mut offset = 1u32;
    out.extend_from_slice(&offset.to_be_bytes());
    for item in items {
        offset += item.len() as u32;
        out.extend_from_slice(&offset.to_be_bytes());
    }
    for item in items {
        out.extend_from_slice(item);
    }
    out
}

/// A DICT operand as a five-byte integer, so a DICT's size does not depend on
/// the offsets it holds.
fn dict_int(value: u32) -> Vec<u8> {
    let mut out = vec![29];
    out.extend_from_slice(&value.to_be_bytes());
    out
}

/// A Type 2 number: one byte from -107 to 107, two to 1131, three beyond.
fn number(value: i32) -> Vec<u8> {
    match value {
        -107..=107 => vec![(value + 139) as u8],
        108..=1131 => {
            let v = value - 108;
            vec![(v / 256 + 247) as u8, (v % 256) as u8]
        }
        _ => {
            let mut out = vec![28];
            out.extend_from_slice(&(value as i16).to_be_bytes());
            out
        }
    }
}

/// A bare CFF of `charstrings`, with `subrs` as its local subroutines.
fn cff(charstrings: &[Vec<u8>], subrs: &[Vec<u8>]) -> Vec<u8> {
    let header = vec![1u8, 0, 4, 4];
    let names = index(&[b"Test".to_vec()]);
    let strings = index(&[]);
    let globals = index(&[]);
    // The Top DICT is of a fixed size: CharStrings (17) and Private (18).
    let top_len = 5 + 1 + 5 + 5 + 1;
    let top_index_len = 2 + 1 + 4 * 2 + top_len;
    let charstrings_at = header.len() + names.len() + top_index_len + strings.len() + globals.len();
    let charstrings_index = index(charstrings);
    let private_at = charstrings_at + charstrings_index.len();
    // defaultWidthX (20) of zero, and Subrs (19), relative to the Private
    // DICT, right after it.
    let mut private = vec![139u8, 20];
    if !subrs.is_empty() {
        private.extend(dict_int(8));
        private.push(19);
        assert_eq!(private.len(), 8);
    }
    let mut top = dict_int(charstrings_at as u32);
    top.push(17);
    top.extend(dict_int(private.len() as u32));
    top.extend(dict_int(private_at as u32));
    top.push(18);
    assert_eq!(top.len(), top_len);
    let mut out = header;
    out.extend(names);
    out.extend(index(&[top]));
    out.extend(strings);
    out.extend(globals);
    assert_eq!(out.len(), charstrings_at);
    out.extend(charstrings_index);
    out.extend(private);
    if !subrs.is_empty() {
        out.extend(index(subrs));
    }
    out
}

/// A charstring that moves to the origin, draws `lines` lines and ends.
fn lines_charstring(lines: usize) -> Vec<u8> {
    let mut code = number(0);
    code.extend(number(0));
    code.push(21); // rmoveto
    for _ in 0..lines {
        code.extend(number(10));
        code.extend(number(0));
        code.push(5); // rlineto
    }
    code.push(14); // endchar
    code
}

/// The lines an outline draws.
fn lines(outline: &Outline) -> usize {
    outline
        .segments
        .iter()
        .filter(|s| matches!(s, Segment::LineTo { .. }))
        .count()
}

/// A CFF of `n` glyphs, each a line, parsed and every glyph outlined.
fn glyphs(n: usize) -> u64 {
    let program = cff(&vec![lines_charstring(1); n], &[]);
    let (font, mut steps) = Cff::parse_counted(&program);
    let font = font.expect("the program parses");
    assert_eq!(font.glyph_count(), n);
    let plain = Cff::parse(&program).expect("the program parses");
    for glyph in 0..n as u16 {
        let (outline, taken) = font.outline_counted(glyph);
        let outline = outline.expect("an outline");
        assert_eq!(lines(&outline), 1, "glyph {glyph} draws its line");
        assert_eq!(
            plain.outline(glyph).map(|o| o.segments),
            Some(outline.segments),
            "alike uncounted"
        );
        steps += taken;
    }
    steps
}

#[test]
fn a_cff_of_more_glyphs_costs_work_linear_in_its_index() {
    holds_the_law("CFF INDEX", glyphs(500), glyphs(2_000));
}

/// One glyph whose charstring draws `n` lines.
fn long_charstring(n: usize) -> u64 {
    let program = cff(&[lines_charstring(n)], &[]);
    let font = Cff::parse(&program).expect("the program parses");
    let (outline, steps) = font.outline_counted(0);
    let outline = outline.expect("an outline");
    assert_eq!(lines(&outline), n, "every line is drawn");
    assert_eq!(
        font.outline(0).map(|o| o.segments),
        Some(outline.segments),
        "alike uncounted"
    );
    steps
}

#[test]
fn a_longer_charstring_costs_work_linear_in_its_operators() {
    // Inside the machine's budget at both sizes.
    holds_the_law(
        "CFF charstring",
        long_charstring(1_000),
        long_charstring(4_000),
    );
}

/// One glyph whose charstring calls a one-line subroutine `n` times.
fn subroutine_calls(n: usize) -> u64 {
    let subr = {
        let mut code = number(10);
        code.extend(number(0));
        code.push(5); // rlineto
        code.push(11); // return
        code
    };
    let mut code = number(0);
    code.extend(number(0));
    code.push(21); // rmoveto
    for _ in 0..n {
        // Subroutine 0 under the bias of 107 a set of fewer than 1 240 has.
        code.extend(number(-107));
        code.push(10); // callsubr
    }
    code.push(14); // endchar
    let program = cff(&[code], &[subr]);
    let font = Cff::parse(&program).expect("the program parses");
    let (outline, steps) = font.outline_counted(0);
    let outline = outline.expect("an outline");
    assert_eq!(lines(&outline), n, "every call drew its line");
    assert_eq!(
        font.outline(0).map(|o| o.segments),
        Some(outline.segments),
        "alike uncounted"
    );
    steps
}

#[test]
fn more_subroutine_calls_cost_work_linear_in_their_count() {
    holds_the_law(
        "CFF subroutine calls",
        subroutine_calls(500),
        subroutine_calls(2_000),
    );
}

// ---------------------------------------------------------------------------
// Type 1.
// ---------------------------------------------------------------------------

/// Adobe's cipher in the encrypting direction, after `pad` leading bytes.
fn encrypt(plain: &[u8], key: u16, pad: usize) -> Vec<u8> {
    const C1: u16 = 52_845;
    const C2: u16 = 22_719;
    let mut r = key;
    let mut out = Vec::new();
    let mut input = vec![0x55u8; pad];
    input.extend_from_slice(plain);
    for byte in input {
        let cipher = byte ^ (r >> 8) as u8;
        r = (u16::from(cipher).wrapping_add(r))
            .wrapping_mul(C1)
            .wrapping_add(C2);
        out.push(cipher);
    }
    out
}

/// A Type 1 charstring: a width, one line, the end.
const TYPE1_LINE: [u8; 7] = [139, 239, 13, 149, 149, 5, 14];

/// A Type 1 program of `glyphs` one-line charstrings, its private section
/// padded with `padding` bytes of a string nothing reads.
fn type1(glyphs: usize, padding: usize) -> Vec<u8> {
    let mut private = format!(
        "dup /Private 8 dict dup begin\n/lenIV 4 def\n/Pad ({}) def\n\
         /Subrs 0 array\n2 index /CharStrings {glyphs} dict dup begin\n",
        "x".repeat(padding)
    )
    .into_bytes();
    let charstring = encrypt(&TYPE1_LINE, 4330, 4);
    for glyph in 0..glyphs {
        private.extend_from_slice(format!("/g{glyph} {} RD ", charstring.len()).as_bytes());
        private.extend_from_slice(&charstring);
        private.extend_from_slice(b" ND\n");
    }
    private.extend_from_slice(b"end\nend\nmark currentfile closefile\n");
    let mut program = b"%!FontType1-1.0: Test 001\n\
        /FontMatrix [0.001 0 0 0.001 0 0] readonly def\ncurrentfile eexec\n"
        .to_vec();
    program.extend(encrypt(&private, 55_665, 4));
    program
}

/// A Type 1 font of `glyphs` charstrings, parsed and each outlined.
fn type1_glyphs(glyphs: usize, padding: usize) -> u64 {
    let program = type1(glyphs, padding);
    let (font, mut steps) = Type1::parse_counted(&program);
    let font = font.expect("the program parses");
    assert_eq!(font.glyph_count(), glyphs);
    let plain = Type1::parse(&program).expect("the program parses");
    for glyph in 0..glyphs as u16 {
        let (outline, taken) = font.outline_counted(glyph);
        let outline = outline.expect("an outline");
        assert_eq!(lines(&outline), 1, "glyph {glyph} draws its line");
        assert_eq!(
            plain.outline(glyph).map(|o| o.segments),
            Some(outline.segments),
            "alike uncounted"
        );
        steps += taken;
    }
    steps
}

#[test]
fn a_longer_eexec_section_costs_work_linear_in_its_bytes() {
    holds_the_law(
        "Type 1 eexec",
        type1_glyphs(1, 10_000),
        type1_glyphs(1, 40_000),
    );
}

#[test]
fn a_type_1_font_of_more_charstrings_costs_work_linear_in_its_glyphs() {
    holds_the_law(
        "Type 1 charstrings",
        type1_glyphs(500, 0),
        type1_glyphs(2_000, 0),
    );
}

// ---------------------------------------------------------------------------
// WOFF and WOFF2.
// ---------------------------------------------------------------------------

/// The four-byte sfnt checksum of `data`, zero-padded.
fn checksum(data: &[u8]) -> u32 {
    let mut sum = 0u32;
    for chunk in data.chunks(4) {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum = sum.wrapping_add(u32::from_be_bytes(word));
    }
    sum
}

/// The tables of a face of `n` small tables, `head` among them, sorted by tag.
fn small_tables(n: usize) -> Vec<([u8; 4], Vec<u8>)> {
    let mut tables = vec![(*b"head", head())];
    for i in 0..n {
        let mut name = [0u8; 4];
        name.copy_from_slice(format!("T{i:03}").as_bytes());
        tables.push((name, (i as u32).to_be_bytes().to_vec()));
    }
    tables.sort_by_key(|(tag, _)| *tag);
    tables
}

/// A WOFF 1.0 of `n` small tables, each stored rather than compressed.
fn woff1(n: usize) -> Vec<u8> {
    let tables = small_tables(n);
    let count = tables.len();
    let header = 44;
    let directory = header + 20 * count;
    let mut data = Vec::new();
    let mut entries = Vec::new();
    let mut total = 12 + 16 * count;
    for (tag, table) in &tables {
        let mut sum_input = table.clone();
        if tag == b"head" {
            // The checksum adjustment is summed as zero.
            sum_input[8..12].copy_from_slice(&[0; 4]);
        }
        entries.extend_from_slice(tag);
        entries.extend_from_slice(&((directory + data.len()) as u32).to_be_bytes());
        entries.extend_from_slice(&(table.len() as u32).to_be_bytes());
        entries.extend_from_slice(&(table.len() as u32).to_be_bytes());
        entries.extend_from_slice(&checksum(&sum_input).to_be_bytes());
        data.extend_from_slice(table);
        while data.len() % 4 != 0 {
            data.push(0);
        }
        total += table.len().div_ceil(4) * 4;
    }
    let mut out = Vec::new();
    out.extend_from_slice(b"wOFF");
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&((directory + data.len()) as u32).to_be_bytes());
    out.extend_from_slice(&(count as u16).to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&(total as u32).to_be_bytes());
    out.extend_from_slice(&[0; 24]);
    assert_eq!(out.len(), header);
    out.extend(entries);
    out.extend(data);
    out
}

/// An LSB-first bit writer, for a brotli stream.
struct Bits {
    out: Vec<u8>,
    used: u32,
}

impl Bits {
    fn put(&mut self, value: u32, width: u32) {
        for bit in 0..width {
            if self.used == 0 {
                self.out.push(0);
            }
            if (value >> bit) & 1 == 1 {
                if let Some(last) = self.out.last_mut() {
                    *last |= 1 << self.used;
                }
            }
            self.used = (self.used + 1) % 8;
        }
    }

    fn align(&mut self) {
        while self.used != 0 {
            self.put(0, 1);
        }
    }
}

/// `data` as a brotli stream of uncompressed meta-blocks (RFC 7932 §9.2), a
/// window of 2¹⁶ − 16 bytes, closed by an empty last meta-block.
fn brotli_stored(data: &[u8]) -> Vec<u8> {
    let mut bits = Bits {
        out: Vec::new(),
        used: 0,
    };
    // WBITS 16: a single 0 bit.
    bits.put(0, 1);
    for chunk in data.chunks(1 << 16) {
        bits.put(0, 1); // ISLAST
        bits.put(0, 2); // MNIBBLES: four
        bits.put(chunk.len() as u32 - 1, 16);
        bits.put(1, 1); // ISUNCOMPRESSED
        bits.align();
        bits.out.extend_from_slice(chunk);
    }
    bits.put(1, 1); // ISLAST
    bits.put(1, 1); // ISLASTEMPTY
    bits.align();
    bits.out
}

/// The `UIntBase128` encoding of `value`.
fn base128(mut value: u32) -> Vec<u8> {
    let mut out = vec![(value & 0x7F) as u8];
    value >>= 7;
    while value > 0 {
        out.insert(0, 0x80 | (value & 0x7F) as u8);
        value >>= 7;
    }
    out
}

/// A WOFF2 of `n` small tables, none transformed, in one brotli block.
fn woff2(n: usize) -> Vec<u8> {
    let tables = small_tables(n);
    let mut directory = Vec::new();
    let mut block = Vec::new();
    let mut total = 12 + 16 * tables.len();
    for (tag, table) in &tables {
        if tag == b"head" {
            directory.push(1); // head, the null transform
        } else {
            directory.push(63);
            directory.extend_from_slice(tag);
        }
        directory.extend(base128(table.len() as u32));
        block.extend_from_slice(table);
        total += table.len().div_ceil(4) * 4;
    }
    let compressed = brotli_stored(&block);
    let length = 48 + directory.len() + compressed.len();
    let mut out = Vec::new();
    out.extend_from_slice(b"wOF2");
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&(length as u32).to_be_bytes());
    out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&(total as u32).to_be_bytes());
    out.extend_from_slice(&(compressed.len() as u32).to_be_bytes());
    out.extend_from_slice(&[0; 24]);
    out.extend(directory);
    out.extend(compressed);
    out
}

/// A WOFF of either kind decoded, and its tables found in the sfnt.
fn woff_tables(n: usize, file: Vec<u8>) -> u64 {
    let (font, steps) = woff::decode_counted(&file, 1 << 24);
    let font = font.expect("the container decodes");
    let face = Sfnt::parse(&font).expect("into an sfnt");
    for i in [0, n - 1] {
        let tag = u32::from_be_bytes(*format!("T{i:03}").as_bytes().first_chunk().unwrap());
        assert_eq!(face.table(tag), Some(&(i as u32).to_be_bytes()[..]));
    }
    assert_eq!(woff::decode(&file, 1 << 24), Ok(font), "alike uncounted");
    steps
}

// Each inside the 512 directory records `Sfnt::parse` reads back.
#[test]
fn a_woff_of_more_tables_costs_work_linear_in_its_tables() {
    holds_the_law(
        "WOFF",
        woff_tables(120, woff1(120)),
        woff_tables(480, woff1(480)),
    );
}

#[test]
fn a_woff2_of_more_tables_costs_work_linear_in_its_tables() {
    holds_the_law(
        "WOFF2",
        woff_tables(120, woff2(120)),
        woff_tables(480, woff2(480)),
    );
}

// ---------------------------------------------------------------------------
// Embedded CMaps.
// ---------------------------------------------------------------------------

/// A CMap with a two-byte codespace and `body` for its mappings.
fn cmap_source(body: &str) -> String {
    format!(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
         1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n{body}\
         endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n"
    )
}

/// The string of the codes the CMap shapes map: `2 i` for each `i`.
fn codes(n: usize) -> Vec<u8> {
    (0..n)
        .flat_map(|i| ((2 * i) as u16).to_be_bytes())
        .collect()
}

/// `n` `bfchar` mappings, code `2 i` to U+4E00 + `i`, in blocks of a hundred;
/// the CMap parsed, a string of every code split, and each code mapped.
fn bfchar(n: usize) -> u64 {
    let mut body = String::new();
    for block in (0..n).collect::<Vec<_>>().chunks(100) {
        body.push_str(&format!("{} beginbfchar\n", block.len()));
        for &i in block {
            body.push_str(&format!("<{:04X}> <{:04X}>\n", 2 * i, 0x4E00 + i));
        }
        body.push_str("endbfchar\n");
    }
    let source = cmap_source(&body);
    let (map, mut steps) = cmap::parse_counted(source.as_bytes());
    let (split, taken) = map.decode_codes_counted(&codes(n));
    steps += taken;
    assert_eq!(split.len(), n, "every code is two bytes");
    for (i, (code, len)) in split.into_iter().enumerate() {
        assert_eq!(len, 2);
        let (text, taken) = map.to_unicode_counted(code);
        steps += taken;
        let expected = char::from_u32(0x4E00 + i as u32).expect("a scalar value");
        assert_eq!(text, Some(&[expected][..]), "code {i} maps");
        assert_eq!(map.to_unicode(code), text, "alike uncounted");
    }
    steps
}

#[test]
fn an_embedded_cmap_of_more_bfchars_costs_work_linear_in_its_mappings() {
    holds_the_law("CMap bfchar", bfchar(500), bfchar(2_000));
}

/// `n` one-code `cidrange` ranges, code `2 i` to CID `i + 1`, in blocks of a
/// hundred; the CMap parsed, a string of every code split, and each mapped.
fn cidrange(n: usize) -> u64 {
    let mut body = String::new();
    for block in (0..n).collect::<Vec<_>>().chunks(100) {
        body.push_str(&format!("{} begincidrange\n", block.len()));
        for &i in block {
            body.push_str(&format!("<{0:04X}> <{0:04X}> {1}\n", 2 * i, i + 1));
        }
        body.push_str("endcidrange\n");
    }
    let source = cmap_source(&body);
    let (map, mut steps) = cmap::parse_counted(source.as_bytes());
    let (split, taken) = map.decode_codes_counted(&codes(n));
    steps += taken;
    for (i, (code, _)) in split.into_iter().enumerate() {
        let (cid, taken) = map.cid_counted(code);
        steps += taken;
        assert_eq!(cid, Some(i as u32 + 1), "code {i} maps to its CID");
        assert_eq!(map.cid(code), cid, "alike uncounted");
    }
    steps
}

#[test]
fn an_embedded_cmap_of_more_cidranges_costs_work_n_log_n_in_its_ranges() {
    holds_the_log_law("CMap cidrange", 500, cidrange(500), cidrange(2_000));
}

/// Ranges written out of order are read as written: the first that holds a
/// code wins, as it always did.
#[test]
fn cidranges_out_of_order_still_match_the_first_written() {
    let source = cmap_source("2 begincidrange\n<0010> <0010> 9\n<0000> <0020> 100\nendcidrange\n");
    let map = cmap::parse(source.as_bytes());
    assert_eq!(map.cid(0x10), Some(9), "the range written first");
    assert_eq!(map.cid(0x11), Some(117), "100 + 0x11");
}
