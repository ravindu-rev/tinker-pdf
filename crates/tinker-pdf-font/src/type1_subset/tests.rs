//! Every program here is built in this file, through [`crate::type1`]'s one
//! cipher, so every charstring, subroutine number and byte of the output is
//! known in advance — which is what makes "the same glyph came out" and "this
//! subroutine kept its number" checkable.

use super::*;
use crate::type1::decrypt;
use crate::Segment;

/// The lead bytes the fixtures' charstrings and eexec section carry, as a
/// font tool might choose them; a reader discards them.
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
const RLINETO: u8 = 5;
const HLINETO: u8 = 6;
const VLINETO: u8 = 7;
const CLOSEPATH: u8 = 9;
const CALLSUBR: u8 = 10;
const RETURN: u8 = 11;
const HSBW: u8 = 13;
const ENDCHAR: u8 = 14;
const RMOVETO: u8 = 21;
const SEAC: [u8; 2] = [12, 6];
const CALLOTHERSUBR: [u8; 2] = [12, 16];
const POP: [u8; 2] = [12, 17];
const SETCURRENTPOINT: [u8; 2] = [12, 33];

/// A charstring from its pieces.
fn cs(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

/// `hsbw` and a move to the side bearing.
fn start(sbx: i32, width: i32) -> Vec<u8> {
    cs(&[&n(&[sbx, width]), &[HSBW], &n(&[0, 0]), &[RMOVETO]])
}

// ---------------------------------------------------------------------------
// Programs.
// ---------------------------------------------------------------------------

/// A Type 1 program assembled from its parts.
#[derive(Clone)]
struct Face {
    /// Each glyph's name and plaintext charstring, in order.
    glyphs: Vec<(Vec<u8>, Vec<u8>)>,
    /// Each subroutine's `dup` index and plaintext charstring, in order.
    subrs: Vec<(i64, Vec<u8>)>,
    /// `/lenIV` as written; absent means 4.
    len_iv: Option<i64>,
    /// How the program spells `RD`, `ND` and `NP`.
    spelling: (&'static str, &'static str, &'static str),
    /// Text written into the private section after its first line.
    private_text: Vec<u8>,
    /// Whether the private section ends with `closefile`.
    closefile: bool,
    /// What follows the encrypted portion.
    trailer: Vec<u8>,
    /// The eexec section's four lead bytes.
    eexec_lead: [u8; 4],
}

/// 2.3's 512 zeros, in lines of 64, and `cleartomark`.
fn standard_trailer() -> Vec<u8> {
    let mut out = b"\n".to_vec();
    for _ in 0..8 {
        out.extend_from_slice(&[b'0'; 64]);
        out.push(b'\n');
    }
    out.extend_from_slice(b"cleartomark\n");
    out
}

impl Face {
    fn build(&self) -> Vec<u8> {
        let lead = vec![PAD; usize::try_from(self.len_iv.unwrap_or(4).clamp(0, 16)).unwrap_or(4)];
        let (rd, nd, np) = self.spelling;
        let mut private = b"dup /Private 8 dict dup begin\n".to_vec();
        private.extend_from_slice(&self.private_text);
        if let Some(len_iv) = self.len_iv {
            private.extend_from_slice(format!("/lenIV {len_iv} def\n").as_bytes());
        }
        private.extend_from_slice(format!("/Subrs {} array\n", self.subrs.len()).as_bytes());
        for (index, code) in &self.subrs {
            let encrypted = encrypt(code, CHARSTRING_KEY, &lead);
            private.extend_from_slice(format!("dup {index} {} {rd} ", encrypted.len()).as_bytes());
            private.extend_from_slice(&encrypted);
            private.extend_from_slice(format!(" {np}\n").as_bytes());
        }
        private.extend_from_slice(format!("{nd}\n").as_bytes());
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
            private.extend_from_slice(format!(" {} {rd} ", encrypted.len()).as_bytes());
            private.extend_from_slice(&encrypted);
            private.extend_from_slice(format!(" {nd}\n").as_bytes());
        }
        private.extend_from_slice(b"end\nend\nreadonly put\nnoaccess put\n");
        private.extend_from_slice(b"dup /FontName get exch definefont pop\n");
        if self.closefile {
            private.extend_from_slice(b"mark currentfile closefile\n");
        }

        let mut out = self.clear_text();
        out.extend(encrypt(&private, EEXEC_KEY, &self.eexec_lead));
        out.extend_from_slice(&self.trailer);
        out
    }

    fn clear_text(&self) -> Vec<u8> {
        b"%!PS-AdobeFont-1.0: Fixture 001.000\n\
          11 dict begin\n\
          /FontName /Fixture def\n\
          /FontType 1 def\n\
          /FontMatrix [0.001 0 0 0.001 0 0] readonly def\n\
          /FontBBox {-50 -50 1000 1000} readonly def\n\
          /UniqueID 4000001 def\n\
          /Encoding 256 array\n\
          0 1 255 {1 index exch /.notdef put} for\n\
          dup 65 /A put\n\
          dup 66 /B put\n\
          dup 67 /C put\n\
          dup 68 /D put\n\
          dup 69 /E put\n\
          dup 194 /acute put\n\
          dup 201 /Aacute put\n\
          readonly def\n\
          currentdict end\n\
          currentfile eexec\n"
            .to_vec()
    }
}

/// The fixture's subroutines: 0 to 3 as chapter 8.4 fixes them, and a
/// subroutine for each thing a glyph below draws.
fn standard_subrs() -> Vec<(i64, Vec<u8>)> {
    vec![
        // 8.4: flex's end, its start, a flex point, and hint replacement's
        // no-op.
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
        // Three sides of a 400-unit box.
        (
            6,
            cs(&[
                &n(&[400]),
                &[HLINETO],
                &n(&[400]),
                &[VLINETO],
                &n(&[-400]),
                &[HLINETO, RETURN],
            ]),
        ),
        // A hint replacement's new stems, and one more set it calls.
        (
            7,
            cs(&[&n(&[0, 60]), &[HSTEM], &n(&[8]), &[CALLSUBR, RETURN]]),
        ),
        (8, cs(&[&n(&[10, 20]), &[VSTEM, RETURN]])),
        // A diagonal glyph E draws.
        (9, cs(&[&n(&[300, 300]), &[RLINETO, RETURN]])),
        // Nobody calls this one.
        (10, cs(&[&n(&[1, 1]), &[RLINETO, RETURN]])),
    ]
}

/// The fixture's glyphs, in order: `.notdef` 0, `A` 1, `B` 2, `C` 3, `D` 4,
/// `acute` 5, `Aacute` 6 and `E` 7.
fn standard_glyphs() -> Vec<(Vec<u8>, Vec<u8>)> {
    let flex = {
        // Chapter 8.3's worked example: from (100, -10), two curves joining at
        // (150, 0) and ending at (200, -10), flex height 50.
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
    };
    let hinted = cs(&[
        &n(&[0, 500]),
        &[HSBW],
        &n(&[0, 50]),
        &[HSTEM],
        &n(&[0, 0]),
        &[RMOVETO],
        &n(&[100]),
        &[HLINETO],
        // 8.1: subr# 1 3 callothersubr pop callsubr.
        &n(&[7, 1, 3]),
        &CALLOTHERSUBR,
        &POP,
        &[CALLSUBR],
        &n(&[100]),
        &[VLINETO, CLOSEPATH, ENDCHAR],
    ]);
    vec![
        (b".notdef".to_vec(), cs(&[&n(&[0, 250]), &[HSBW, ENDCHAR]])),
        (
            b"A".to_vec(),
            cs(&[&start(20, 600), &n(&[5]), &[CALLSUBR, CLOSEPATH, ENDCHAR]]),
        ),
        (
            b"B".to_vec(),
            cs(&[&start(30, 650), &n(&[6]), &[CALLSUBR, CLOSEPATH, ENDCHAR]]),
        ),
        (b"C".to_vec(), flex),
        (b"D".to_vec(), hinted),
        (
            b"acute".to_vec(),
            cs(&[
                &start(0, 300),
                &n(&[50]),
                &[HLINETO],
                &n(&[50]),
                &[VLINETO],
                &n(&[-50]),
                &[HLINETO, CLOSEPATH, ENDCHAR],
            ]),
        ),
        (
            b"Aacute".to_vec(),
            // asb adx ady bchar achar seac: `A` is StandardEncoding 65 and
            // `acute` 194.
            cs(&[&n(&[0, 600]), &[HSBW], &n(&[0, 100, 200, 65, 194]), &SEAC]),
        ),
        (
            b"E".to_vec(),
            cs(&[&start(0, 500), &n(&[9]), &[CALLSUBR, CLOSEPATH, ENDCHAR]]),
        ),
    ]
}

fn face() -> Face {
    Face {
        glyphs: standard_glyphs(),
        subrs: standard_subrs(),
        len_iv: Some(4),
        spelling: ("RD", "ND", "NP"),
        private_text: Vec::new(),
        closefile: true,
        trailer: standard_trailer(),
        eexec_lead: [PAD; 4],
    }
}

/// The glyph indices `names` stand at in the fixture.
fn indices(program: &[u8], names: &[&[u8]]) -> BTreeSet<u16> {
    let font = Type1::parse(program).expect("the fixture parses");
    names
        .iter()
        .map(|name| {
            font.glyph_for_name(name)
                .expect("the fixture has the glyph")
        })
        .collect()
}

/// Every glyph in `names` draws and advances in `subset` exactly as in
/// `original`, found by its name.
#[track_caller]
fn draws_alike(original: &[u8], subset: &[u8], names: &[&[u8]]) {
    let before = Type1::parse(original).expect("the original parses");
    let after = Type1::parse(subset).expect("the subset parses");
    for name in names {
        let a = before.glyph_for_name(name).expect("in the original");
        let b = after
            .glyph_for_name(name)
            .unwrap_or_else(|| panic!("{} is not in the subset", String::from_utf8_lossy(name)));
        assert_eq!(
            before.outline(a).map(|o| o.segments),
            after.outline(b).map(|o| o.segments),
            "{} draws something else in the subset",
            String::from_utf8_lossy(name)
        );
        assert_eq!(before.advance(a), after.advance(b));
    }
}

/// The subroutines of a program as the scanner reads them: each `dup` index
/// and its ciphertext, in order.
fn subroutines_of(program: &[u8]) -> Vec<(i64, Vec<u8>)> {
    let layout = Layout::read(program).expect("a program");
    scan_subrs(&layout.private)
        .entries
        .iter()
        .filter_map(|entry| match entry.key {
            Key::Subr(index) => Some((index, layout.private[entry.data.clone()].to_vec())),
            Key::Glyph(_) => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The exit's tests.
// ---------------------------------------------------------------------------

#[test]
fn the_fixture_is_a_program_the_reader_draws() {
    let program = face().build();
    let font = Type1::parse(&program).expect("it parses");
    assert_eq!(font.glyph_count(), 8);
    for glyph in 1..font.glyph_count() as u16 {
        assert!(
            !font.outline(glyph).expect("it outlines").is_empty(),
            "glyph {glyph} draws"
        );
    }
    // The flex glyph's two curves are there, and the hinted one's line.
    let flex = font.outline(3).expect("C").segments;
    assert_eq!(
        flex.iter()
            .filter(|s| matches!(s, Segment::CurveTo { .. }))
            .count(),
        2,
        "chapter 8.3's two curves: {flex:?}"
    );
}

/// The headline: every glyph asked for draws and advances as it did, found
/// by its name, and the glyphs nobody asked for are gone.
#[test]
fn every_kept_glyph_outlines_as_before() {
    let program = face().build();
    let wanted = indices(&program, &[b"A", b"B", b"C", b"D"]);
    let subset = subset_type1(&program, &wanted).expect("it subsets");

    draws_alike(
        &program,
        &subset.program,
        &[b".notdef", b"A", b"B", b"C", b"D"],
    );
    let after = Type1::parse(&subset.program).expect("the subset parses");
    assert_eq!(after.glyph_count(), 5);
    for gone in [b"E".as_slice(), b"acute", b"Aacute"] {
        assert_eq!(
            after.glyph_for_name(gone),
            None,
            "{} was not asked for",
            String::from_utf8_lossy(gone)
        );
    }
    assert_eq!(
        subset.names,
        [
            b".notdef".to_vec(),
            b"A".to_vec(),
            b"B".to_vec(),
            b"C".to_vec(),
            b"D".to_vec()
        ],
        "the kept names, in program order"
    );
    assert!(
        subset.program.len() < program.len(),
        "the subset is smaller: {} against {}",
        subset.program.len(),
        program.len()
    );
}

/// `seac` draws two StandardEncoding glyphs as one, so keeping the accented
/// letter keeps its base and its accent — and the letter still draws as both.
#[test]
fn a_seac_glyph_keeps_its_components() {
    let program = face().build();
    let wanted = indices(&program, &[b"Aacute"]);
    let subset = subset_type1(&program, &wanted).expect("it subsets");

    let after = Type1::parse(&subset.program).expect("the subset parses");
    for component in [b"A".as_slice(), b"acute"] {
        assert!(
            after.glyph_for_name(component).is_some(),
            "{} is a component of the kept glyph",
            String::from_utf8_lossy(component)
        );
    }
    draws_alike(&program, &subset.program, &[b"Aacute", b"A", b"acute"]);
    let composed = after
        .outline(after.glyph_for_name(b"Aacute").expect("kept"))
        .expect("it composes");
    assert!(!composed.is_empty(), "the accented glyph draws both parts");
    // Subroutines 5 and 4 draw `A`, so they are reached through it.
    let subrs = subroutines_of(&subset.program);
    let original = subroutines_of(&program);
    for index in [4usize, 5] {
        assert_eq!(
            subrs[index], original[index],
            "subroutine {index} is reached"
        );
    }
}

/// A subroutine nobody reaches becomes `return` at its own number, so every
/// `callsubr` operand still names what it named; the ones a kept glyph
/// reaches, and 0 to 3, keep their ciphertext byte for byte.
#[test]
fn an_unreached_subroutine_keeps_its_number() {
    let program = face().build();
    let wanted = indices(&program, &[b"B"]);
    let subset = subset_type1(&program, &wanted).expect("it subsets");

    let before = subroutines_of(&program);
    let after = subroutines_of(&subset.program);
    assert_eq!(
        after.iter().map(|(index, _)| *index).collect::<Vec<_>>(),
        before.iter().map(|(index, _)| *index).collect::<Vec<_>>(),
        "every subroutine is still there, at its own number and in its order"
    );
    for (i, ((index, was), (_, now))) in before.iter().zip(&after).enumerate() {
        match index {
            0..=3 | 6 => assert_eq!(now, was, "subroutine {index} is kept"),
            _ => assert_eq!(
                decrypt(now, CHARSTRING_KEY, 4),
                vec![RETURN],
                "subroutine {index} (entry {i}) is nobody's and is `return`"
            ),
        }
    }
    let layout = Layout::read(&subset.program).expect("a program");
    assert!(
        find(&layout.private, b"/Subrs 11 array").is_some(),
        "the array keeps its size"
    );
    draws_alike(&program, &subset.program, &[b"B"]);
}

/// The subroutine a hint replacement names is what an interpreter that
/// replaces hints calls, so it is kept — with what it calls — though this
/// reader calls subroutine 3 in its place.
#[test]
fn a_hint_replacement_keeps_the_subroutine_it_names() {
    let program = face().build();
    let wanted = indices(&program, &[b"D"]);
    let subset = subset_type1(&program, &wanted).expect("it subsets");
    let before = subroutines_of(&program);
    let after = subroutines_of(&subset.program);
    for index in [7usize, 8] {
        assert_eq!(after[index], before[index], "subroutine {index} is kept");
    }
    for index in [4usize, 5, 6, 9, 10] {
        assert_eq!(decrypt(&after[index].1, CHARSTRING_KEY, 4), vec![RETURN]);
    }
    draws_alike(&program, &subset.program, &[b"D"]);
}

/// Ruling 4: nothing random enters, so the same program and glyphs give the
/// same bytes — and the lead bytes the input's eexec section chose do not
/// reach the output, which writes its own.
#[test]
fn the_same_input_gives_the_same_bytes() {
    let program = face().build();
    let wanted = indices(&program, &[b"A", b"C"]);
    let once = subset_type1(&program, &wanted).expect("it subsets");
    let twice = subset_type1(&program, &wanted).expect("it subsets");
    assert_eq!(once, twice);

    let mut other = face();
    other.eexec_lead = [0x12, 0x34, 0x56, 0x78];
    let other = other.build();
    assert_ne!(
        other, program,
        "the inputs differ in their eexec lead bytes"
    );
    assert_eq!(
        subset_type1(&other, &wanted).expect("it subsets").program,
        once.program,
        "and the subsets do not"
    );
}

/// Ruling 1: a hostile program is declined, never panicked on.
#[test]
fn garbage_is_declined_not_panicked_on() {
    let none = BTreeSet::new();
    assert_eq!(subset_type1(b"", &none), Err(Type1Refusal::Unreadable));
    assert_eq!(
        subset_type1(b"not a font at all", &none),
        Err(Type1Refusal::Unreadable)
    );
    assert_eq!(
        subset_type1(&[0u8; 512], &none),
        Err(Type1Refusal::Unreadable)
    );

    let program = face().build();
    let wanted: BTreeSet<u16> = [0u16, 1, 3, 6, 9999].into_iter().collect();
    for cut in 0..program.len() {
        let _ = subset_type1(&program[..cut], &wanted);
    }
    for at in (0..program.len()).step_by(7) {
        let mut damaged = program.clone();
        damaged[at] ^= 0xA5;
        if let Ok(subset) = subset_type1(&damaged, &wanted) {
            // Whatever comes out reads, and its lengths are its own.
            assert!(Type1::parse(&subset.program).is_some());
            assert_eq!(subset.lengths.iter().sum::<usize>(), subset.program.len());
        }
    }
}

// ---------------------------------------------------------------------------
// The program as it is written.
// ---------------------------------------------------------------------------

/// Table 125's three lengths are the clear text, the encrypted portion and
/// the fixed-content portion, which is the input's own and follows the
/// encrypted portion as written.
#[test]
fn the_three_segments_are_the_clear_text_the_encryption_and_the_trailer() {
    let program = face().build();
    let wanted = indices(&program, &[b"B"]);
    let subset = subset_type1(&program, &wanted).expect("it subsets");
    let [clear, encrypted, fixed] = subset.lengths;
    assert_eq!(clear + encrypted + fixed, subset.program.len());
    assert_eq!(&subset.program[..clear], face().clear_text().as_slice());
    assert!(subset.program[..clear].ends_with(b"eexec\n"));
    assert_eq!(
        &subset.program[clear + encrypted..],
        standard_trailer().as_slice()
    );

    // A `{restore}if` after `cleartomark` closes a `save` the clear text
    // opened, and goes with the trailer.
    let mut restored = face();
    restored.trailer.extend_from_slice(b"{restore}if\n");
    let restored = restored.build();
    let subset = subset_type1(&restored, &wanted).expect("it subsets");
    let [clear, encrypted, _] = subset.lengths;
    assert!(subset.program[clear + encrypted..].ends_with(b"cleartomark\n{restore}if\n"));

    // Table 125: a `Length3` of 0 is a program whose trailer the reader adds.
    let mut bare = face();
    bare.trailer = Vec::new();
    let bare = bare.build();
    let subset = subset_type1(&bare, &wanted).expect("it subsets");
    assert_eq!(subset.lengths[2], 0);
    draws_alike(&bare, &subset.program, &[b"B"]);
}

/// Both cipher layers are written with zero lead bytes: the eexec section's
/// first ciphertext byte is then 0xD9, which 7.2 requires to be neither white
/// space nor a hexadecimal digit, and a stub decrypts to `lenIV` zeros and
/// `return` whatever `lenIV` the program states.
#[test]
fn both_ciphers_are_written_with_zero_lead_bytes() {
    for len_iv in [0i64, 2, 4] {
        let mut fixture = face();
        fixture.len_iv = Some(len_iv);
        let program = fixture.build();
        let wanted = indices(&program, &[b"B"]);
        let subset = subset_type1(&program, &wanted).expect("it subsets");
        let clear = subset.lengths[0];
        assert_eq!(subset.program[clear], 0xD9);
        let section = &subset.program[clear..clear + subset.lengths[1]];
        assert_eq!(&decrypt(section, EEXEC_KEY, 0)[..4], &[0, 0, 0, 0]);

        let lead = usize::try_from(len_iv).expect("small");
        let stub = &subroutines_of(&subset.program)[10].1;
        let mut expected = vec![0u8; lead];
        expected.push(RETURN);
        assert_eq!(decrypt(stub, CHARSTRING_KEY, 0), expected, "lenIV {len_iv}");
        draws_alike(&program, &subset.program, &[b"B"]);
    }
}

/// The program's own spellings of `RD`, `ND` and `NP` are what the subset
/// writes: a stub takes the entry's own `-|`.
#[test]
fn the_programs_own_spellings_are_kept() {
    let mut fixture = face();
    fixture.spelling = ("-|", "|-", "|");
    let program = fixture.build();
    let wanted = indices(&program, &[b"A"]);
    let subset = subset_type1(&program, &wanted).expect("it subsets");
    let layout = Layout::read(&subset.program).expect("a program");
    let scan = scan_subrs(&layout.private);
    for entry in &scan.entries {
        assert_eq!(&layout.private[entry.rd.clone()], b"-|");
    }
    draws_alike(&program, &subset.program, &[b"A"]);
}

/// Glyph indices past the program are no glyphs, and `.notdef` is kept
/// whether or not it was asked for.
#[test]
fn notdef_is_kept_and_indices_past_the_program_are_none() {
    let program = face().build();
    let subset = subset_type1(&program, &[9999u16].into_iter().collect()).expect("it subsets");
    assert_eq!(subset.names, [b".notdef".to_vec()]);
    draws_alike(&program, &subset.program, &[b".notdef"]);
}

/// A program with no `/Subrs` at all subsets as one with them does.
#[test]
fn a_program_with_no_subroutines_subsets() {
    let mut fixture = face();
    fixture.subrs = Vec::new();
    fixture.glyphs = vec![
        (b".notdef".to_vec(), cs(&[&n(&[0, 250]), &[HSBW, ENDCHAR]])),
        (
            b"I".to_vec(),
            cs(&[
                &start(10, 300),
                &n(&[0, 600]),
                &[RLINETO, CLOSEPATH, ENDCHAR],
            ]),
        ),
        (
            b"J".to_vec(),
            cs(&[
                &start(10, 300),
                &n(&[600, 0]),
                &[RLINETO, CLOSEPATH, ENDCHAR],
            ]),
        ),
    ];
    let program = fixture.build();
    let subset = subset_type1(&program, &indices(&program, &[b"J"])).expect("it subsets");
    assert_eq!(subset.names, [b".notdef".to_vec(), b"J".to_vec()]);
    draws_alike(&program, &subset.program, &[b"J"]);
}

/// A `/Subrs` index the reader does not store is text it never reads, and is
/// left as it was.
#[test]
fn an_entry_the_reader_does_not_store_is_left_as_text() {
    let mut fixture = face();
    fixture
        .subrs
        .push((70_000, cs(&[&n(&[5, 5]), &[RLINETO, RETURN]])));
    let program = fixture.build();
    let subset = subset_type1(&program, &indices(&program, &[b"B"])).expect("it subsets");
    let before = subroutines_of(&program);
    let after = subroutines_of(&subset.program);
    assert_eq!(after.last(), before.last(), "index 70 000 is untouched");
}

// ---------------------------------------------------------------------------
// The refusals, each by name.
// ---------------------------------------------------------------------------

#[test]
fn a_pfb_container_is_refused() {
    let mut program = vec![0x80, 0x01, 0, 0, 0, 0];
    program.extend(face().build());
    assert_eq!(
        subset_type1(&program, &BTreeSet::new()),
        Err(Type1Refusal::Pfb)
    );
}

#[test]
fn a_hexadecimal_encrypted_portion_is_refused() {
    let fixture = face();
    let binary = fixture.build();
    let clear = fixture.clear_text();
    let mut program = clear.clone();
    for byte in &binary[clear.len()..binary.len() - fixture.trailer.len()] {
        program.extend_from_slice(format!("{byte:02x}").as_bytes());
    }
    program.extend_from_slice(&fixture.trailer);
    assert!(Type1::parse(&program).is_some(), "the reader reads it");
    assert_eq!(
        subset_type1(&program, &BTreeSet::new()),
        Err(Type1Refusal::HexEncrypted)
    );
}

#[test]
fn a_len_iv_the_reader_does_not_decrypt_as_written_is_refused() {
    for len_iv in [-1i64, 17, 20] {
        let mut fixture = face();
        fixture.len_iv = Some(len_iv);
        assert_eq!(
            subset_type1(&fixture.build(), &BTreeSet::new()),
            Err(Type1Refusal::LenIv),
            "lenIV {len_iv}"
        );
    }
}

/// 10.3: `hires` or `FontDirectory` after `/Private` is a hybrid or synthetic
/// program; the same letters inside a longer name are not.
#[test]
fn a_synthetic_or_hybrid_program_is_refused() {
    for text in [
        &b"/hires true def\n"[..],
        b"FontDirectory /Fixture known pop\n",
    ] {
        let mut fixture = face();
        fixture.private_text = text.to_vec();
        assert_eq!(
            subset_type1(&fixture.build(), &BTreeSet::new()),
            Err(Type1Refusal::SyntheticOrHybrid),
            "{}",
            String::from_utf8_lossy(text)
        );
    }
    let mut fixture = face();
    fixture.private_text = b"/hiresolution 1 def\n".to_vec();
    assert!(subset_type1(&fixture.build(), &BTreeSet::new()).is_ok());
}

#[test]
fn a_private_section_with_no_closefile_is_refused() {
    let mut fixture = face();
    fixture.closefile = false;
    assert_eq!(
        subset_type1(&fixture.build(), &BTreeSet::new()),
        Err(Type1Refusal::NoCloseFile)
    );
}

#[test]
fn two_charstrings_under_one_name_are_refused() {
    let mut fixture = face();
    let again = fixture.glyphs[1].clone();
    fixture.glyphs.push(again);
    assert_eq!(
        subset_type1(&fixture.build(), &BTreeSet::new()),
        Err(Type1Refusal::DuplicateName)
    );
}

#[test]
fn one_subroutine_index_defined_twice_is_refused() {
    let mut fixture = face();
    fixture.subrs.push((4, vec![RETURN]));
    assert_eq!(
        subset_type1(&fixture.build(), &BTreeSet::new()),
        Err(Type1Refusal::DuplicateSubroutine)
    );
}

/// A kept glyph calling a subroutine the program does not define: past the
/// array, into a hole in it, or named by a hint replacement.
#[test]
fn a_call_to_a_missing_subroutine_is_refused() {
    let calls = |index: i32| cs(&[&start(0, 500), &n(&[index]), &[CALLSUBR, ENDCHAR]]);
    for (code, why) in [
        (calls(50), "past the array"),
        (calls(11), "into a hole: 0 to 10 and 12 are defined"),
        (
            cs(&[
                &start(0, 500),
                &n(&[60, 1, 3]),
                &CALLOTHERSUBR,
                &POP,
                &[CALLSUBR, ENDCHAR],
            ]),
            "a hint replacement naming 60",
        ),
    ] {
        let mut fixture = face();
        fixture.subrs.push((12, vec![RETURN]));
        fixture.glyphs.push((b"X".to_vec(), code));
        let program = fixture.build();
        assert_eq!(
            subset_type1(&program, &indices(&program, &[b"X"])),
            Err(Type1Refusal::MissingSubroutine),
            "{why}"
        );
        // A glyph nobody keeps may call what it likes.
        assert!(
            subset_type1(&program, &indices(&program, &[b"B"])).is_ok(),
            "{why}"
        );
    }
}

/// A run that meets one of the reader's bounds draws what this reader
/// stopped at, which the program does not say.
#[test]
fn a_kept_glyph_that_meets_a_bound_is_refused_by_name() {
    let recursive = cs(&[&n(&[11]), &[CALLSUBR, RETURN]]);
    let mut many = Vec::new();
    for _ in 0..49 {
        many.extend(n(&[1]));
    }
    many.push(ENDCHAR);
    for (glyph, subr, refusal) in [
        (
            cs(&[&start(0, 500), &n(&[11]), &[CALLSUBR, ENDCHAR]]),
            Some(recursive),
            Type1Refusal::SubroutineDepth,
        ),
        (many, None, Type1Refusal::StackOverflow),
        (vec![CLOSEPATH; 100_001], None, Type1Refusal::Budget),
    ] {
        let mut fixture = face();
        if let Some(subr) = subr {
            fixture.subrs.push((11, subr));
        }
        fixture.glyphs.push((b"X".to_vec(), glyph));
        let program = fixture.build();
        assert_eq!(
            subset_type1(&program, &indices(&program, &[b"X"])),
            Err(refusal)
        );
    }
}

/// A charstring whose `ND` is missing cannot be told from the next
/// definition, so deleting it is refused rather than taking its neighbour.
#[test]
fn a_charstring_whose_end_cannot_be_found_is_refused() {
    let fixture = face();
    let program = fixture.build();
    let layout = Layout::read(&program).expect("a program");
    let mut private = layout.private.clone();
    // The section as written, without the trailer decrypted after it.
    let close = find(&private, b"closefile").expect("closefile");
    private.truncate(close + b"closefile\n".len());
    // Drop the ` ND` after `acute`, so `/Aacute` follows its bytes.
    let scan = scan_charstrings(&private);
    let acute = &scan.entries[5];
    let tail = private[acute.data.end..].to_vec();
    let nd = find(&tail, b" ND").expect("acute's ND");
    private.drain(acute.data.end + nd..acute.data.end + nd + 3);
    let mut damaged = fixture.clear_text();
    damaged.extend(encrypt(&private, EEXEC_KEY, &[PAD; 4]));
    damaged.extend_from_slice(&fixture.trailer);
    assert!(Type1::parse(&damaged).is_some());
    assert_eq!(
        subset_type1(&damaged, &indices(&damaged, &[b"B"])),
        Err(Type1Refusal::Entries)
    );
}

#[test]
fn more_charstrings_than_the_reader_reads_are_refused() {
    let mut fixture = face();
    for i in 0..crate::type1::MAX_GLYPHS {
        fixture
            .glyphs
            .push((format!("g{i}").into_bytes(), vec![ENDCHAR]));
    }
    assert_eq!(
        subset_type1(&fixture.build(), &BTreeSet::new()),
        Err(Type1Refusal::TooManyGlyphs)
    );
}

/// A program with no charstrings has nothing to keep.
#[test]
fn a_program_with_no_charstrings_is_unreadable() {
    let mut fixture = face();
    fixture.glyphs = Vec::new();
    assert_eq!(
        subset_type1(&fixture.build(), &BTreeSet::new()),
        Err(Type1Refusal::Unreadable)
    );
}

/// The writer's own check: a subset that draws a kept glyph otherwise is
/// refused, whatever made it so.
#[test]
fn a_subset_that_draws_otherwise_is_a_mismatch() {
    let program = face().build();
    let font = Type1::parse(&program).expect("it parses");
    let keep = indices(&program, &[b"A", b"B"]);
    let mut spend = |_: u64| -> Result<(), Type1Refusal> { Ok(()) };

    // B's charstring swapped for E's under B's name: every glyph is there,
    // and one draws otherwise.
    let mut swapped = face();
    swapped.glyphs[2].1 = swapped.glyphs[7].1.clone();
    let swapped = swapped.build();
    let every: BTreeSet<u16> = (0..8).collect();
    assert_eq!(
        verify(&font, &swapped, &every, &mut spend),
        Err(Type1Refusal::Mismatch)
    );
    assert_eq!(verify(&font, &program, &every, &mut spend), Ok(()));
    assert_eq!(
        verify(&font, &program, &keep, &mut spend),
        Err(Type1Refusal::Mismatch),
        "the whole face holds more glyphs than the two kept"
    );
    let subset = subset_type1(&program, &keep).expect("it subsets");
    let kept: BTreeSet<u16> = [0u16, 1, 2].into_iter().collect();
    assert_eq!(verify(&font, &subset.program, &kept, &mut spend), Ok(()));
}

/// The cheapest true statement of [`Type1Refusal`]'s `Display`: every
/// refusal says something.
#[test]
fn every_refusal_names_itself() {
    for refusal in [
        Type1Refusal::Unreadable,
        Type1Refusal::Pfb,
        Type1Refusal::HexEncrypted,
        Type1Refusal::LenIv,
        Type1Refusal::SyntheticOrHybrid,
        Type1Refusal::NoCloseFile,
        Type1Refusal::Entries,
        Type1Refusal::DuplicateName,
        Type1Refusal::DuplicateSubroutine,
        Type1Refusal::TooManyGlyphs,
        Type1Refusal::MissingSubroutine,
        Type1Refusal::SubroutineDepth,
        Type1Refusal::Budget,
        Type1Refusal::StackOverflow,
        Type1Refusal::Mismatch,
    ] {
        assert!(!refusal.to_string().is_empty());
    }
}

/// Writes `fuzz/corpus/type1_subset`: the fixture, the fixture with no
/// trailer, and one whose `lenIV` is 0 and whose spellings are `-|` and `|-`.
///
/// `#[ignore]`d: it writes into the tree, and a run that rewrites a seed is a
/// diff to read.
#[test]
#[ignore = "writes into fuzz/corpus/type1_subset, which is committed"]
fn write_the_fuzz_seeds() {
    let base =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/type1_subset");
    std::fs::create_dir_all(&base).expect("the corpus directory");
    let mut bare = face();
    bare.trailer = Vec::new();
    let mut spelled = face();
    spelled.len_iv = Some(0);
    spelled.spelling = ("-|", "|-", "|");
    for (name, fixture) in [
        ("subrs_flex_hints_seac.t1", face()),
        ("no_trailer.t1", bare),
        ("leniv0_spelled.t1", spelled),
    ] {
        std::fs::write(base.join(name), fixture.build()).expect("the seed writes");
    }
}
