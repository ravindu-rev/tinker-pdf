//! Every face here is assembled byte by byte, so each number a test asserts
//! is one the test wrote, at the offset the table's own chapter gives it.

use super::*;

/// A table directory over `tables`, with no checksums: [`Sfnt::parse`]
/// reads the tags, offsets and lengths, and nothing here reads the rest.
fn sfnt(version: u32, tables: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut out = version.to_be_bytes().to_vec();
    out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0; 6]);
    let mut offset = 12 + tables.len() * 16;
    let mut body = Vec::new();
    for (tag, data) in tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut padded = data.clone();
        while padded.len() % 4 != 0 {
            padded.push(0);
        }
        offset += padded.len();
        body.extend(padded);
    }
    out.extend(body);
    out
}

fn put16(table: &mut [u8], at: usize, value: u16) {
    table[at..at + 2].copy_from_slice(&value.to_be_bytes());
}

fn put32(table: &mut [u8], at: usize, value: u32) {
    table[at..at + 4].copy_from_slice(&value.to_be_bytes());
}

/// `head` at 2048 units per em, its box `[-100, -400, 1900, 1800]`, with
/// `macStyle` as given and long `loca` offsets.
fn head(mac_style: u16) -> Vec<u8> {
    let mut head = vec![0u8; 54];
    put32(&mut head, 0, 0x0001_0000);
    put16(&mut head, 18, 2048);
    for (at, value) in [(36, -100i16), (38, -400), (40, 1900), (42, 1800)] {
        put16(&mut head, at, value as u16);
    }
    put16(&mut head, 44, mac_style);
    put16(&mut head, 50, 1);
    head
}

/// `hhea` with its ascender and descender.
fn hhea(ascender: i16, descender: i16) -> Vec<u8> {
    let mut hhea = vec![0u8; 36];
    put32(&mut hhea, 0, 0x0001_0000);
    put16(&mut hhea, 4, ascender as u16);
    put16(&mut hhea, 6, descender as u16);
    hhea
}

/// `OS/2` at `version`, with `fsType`, `sFamilyClass`, `fsSelection`, the
/// typographic ascender and descender, and — from version 2 — `sxHeight`
/// and `sCapHeight`.
fn os2(version: u16, fs_type: u16, family_class: u16, selection: u16) -> Vec<u8> {
    let mut os2 = vec![0u8; 96];
    put16(&mut os2, 0, version);
    put16(&mut os2, 8, fs_type);
    put16(&mut os2, 30, family_class);
    put16(&mut os2, 62, selection);
    put16(&mut os2, 68, 1600);
    put16(&mut os2, 70, (-500i16) as u16);
    put16(&mut os2, 86, 1024);
    put16(&mut os2, 88, 1434);
    os2
}

/// `post` version 3.0, with its italic angle as 16.16 and `isFixedPitch`.
fn post(angle: i32, fixed: bool) -> Vec<u8> {
    let mut post = vec![0u8; 32];
    put32(&mut post, 0, 0x0003_0000);
    put32(&mut post, 4, angle as u32);
    put32(&mut post, 12, u32::from(fixed));
    post
}

/// A `name` table holding `records`, each `(platform, encoding, id, bytes)`.
fn name(records: &[(u16, u16, u16, Vec<u8>)]) -> Vec<u8> {
    let storage = 6 + records.len() * 12;
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&(records.len() as u16).to_be_bytes());
    out.extend_from_slice(&(storage as u16).to_be_bytes());
    let mut strings = Vec::new();
    for (platform, encoding, id, bytes) in records {
        for value in [*platform, *encoding, 0x0409, *id, bytes.len() as u16] {
            out.extend_from_slice(&value.to_be_bytes());
        }
        out.extend_from_slice(&(strings.len() as u16).to_be_bytes());
        strings.extend_from_slice(bytes);
    }
    out.extend(strings);
    out
}

fn utf16(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_be_bytes).collect()
}

/// A face of `head`, `hhea`, `OS/2`, `post` and `name`, and nothing to draw.
fn described(os2_table: Vec<u8>, names: Vec<u8>) -> Vec<u8> {
    sfnt(
        0x0001_0000,
        &[
            (*b"OS/2", os2_table),
            (*b"head", head(0)),
            (*b"hhea", hhea(1854, -434)),
            (*b"name", names),
            (*b"post", post((-12 << 16) | 0x8000, true)),
        ],
    )
}

#[track_caller]
fn close(found: Option<f64>, expected: f64) {
    let found = found.expect("the face states it");
    assert!(
        (found - expected).abs() < 1e-9,
        "{found} where {expected} was written"
    );
}

/// Each table's number, at the offset its chapter gives it, scaled from
/// 2048 units per em to thousandths of an em.
#[test]
fn an_sfnt_states_its_name_licence_metrics_and_class() {
    let face = described(
        os2(4, 0x0008, 0x0801, 0x0001),
        name(&[(3, 1, 6, utf16("Facts-BoldItalic"))]),
    );
    let facts = face_facts(&face).expect("the face reads");
    assert_eq!(facts.postscript_name.as_deref(), Some("Facts-BoldItalic"));
    assert_eq!(facts.fs_type, Some(0x0008));
    assert_eq!(facts.family_class, Some(0x0801));
    assert_eq!(facts.italic, Some(true), "fsSelection bit 0");
    assert_eq!(facts.fixed_pitch, Some(true));
    let em = 1000.0 / 2048.0;
    let bbox = facts.bbox.expect("head states a box");
    for (found, written) in bbox.iter().zip([-100.0, -400.0, 1900.0, 1800.0]) {
        assert!((found - written * em).abs() < 1e-9, "{bbox:?}");
    }
    close(facts.ascent, 1854.0 * em);
    close(facts.descent, -434.0 * em);
    close(facts.cap_height, 1434.0 * em);
    close(facts.x_height, 1024.0 * em);
    close(facts.italic_angle, -11.5);
    assert_eq!(facts.stem_v, None, "a glyf face states no stem");
}

/// An `OS/2` before version 2 has no heights, and one with neither an
/// `hhea` nor heights falls back to what the face's own tables still say:
/// its typographic pair, and nothing invented for the heights.
#[test]
fn an_early_os2_states_no_heights_and_its_typographic_pair_stands_in() {
    let face = sfnt(
        0x0001_0000,
        &[(*b"OS/2", os2(1, 0, 0, 0)), (*b"head", head(0b10))],
    );
    let facts = face_facts(&face).expect("the face reads");
    let em = 1000.0 / 2048.0;
    close(facts.ascent, 1600.0 * em);
    close(facts.descent, -500.0 * em);
    assert_eq!(
        facts.cap_height, None,
        "no glyph reaches H, and v1 states none"
    );
    assert_eq!(facts.x_height, None);
    assert_eq!(facts.italic, Some(true), "macStyle bit 1");
    assert_eq!(facts.italic_angle, None, "there is no post");
    assert_eq!(facts.fixed_pitch, None);
    assert_eq!(facts.postscript_name, None, "there is no name");
}

/// Where `OS/2` states no heights, the tops of the glyphs `H` and `x` reach
/// through the `cmap` are the face's own statement of them.
#[test]
fn heights_come_from_the_glyphs_where_os2_states_none() {
    // Glyph 1 is `H`, 1400 units tall; glyph 2 is `x`, 1000.
    fn glyph(top: i16) -> Vec<u8> {
        let mut glyph = Vec::new();
        for value in [1i16, 0, 0, 1000, top] {
            glyph.extend_from_slice(&value.to_be_bytes());
        }
        glyph.extend_from_slice(&3u16.to_be_bytes());
        glyph.extend_from_slice(&0u16.to_be_bytes());
        glyph.extend_from_slice(&[0x01; 4]);
        for dx in [0i16, 1000, 0, -1000] {
            glyph.extend_from_slice(&dx.to_be_bytes());
        }
        for dy in [0i16, 0, top, 0] {
            glyph.extend_from_slice(&dy.to_be_bytes());
        }
        glyph
    }
    let (h, x) = (glyph(1400), glyph(1000));
    let mut glyf = h.clone();
    glyf.extend(&x);
    let mut loca = Vec::new();
    for offset in [0u32, 0, h.len() as u32, (h.len() + x.len()) as u32] {
        loca.extend_from_slice(&offset.to_be_bytes());
    }
    let mut maxp = vec![0u8; 6];
    put32(&mut maxp, 0, 0x0000_5000);
    put16(&mut maxp, 4, 3);
    // A (3,1) format 4 subtable: `H` to glyph 1, `x` to glyph 2.
    let mut sub = Vec::new();
    for value in [4u16, 0, 0, 6, 0, 0, 0] {
        sub.extend_from_slice(&value.to_be_bytes());
    }
    for end in [0x48u16, 0x78, 0xFFFF] {
        sub.extend_from_slice(&end.to_be_bytes());
    }
    sub.extend_from_slice(&0u16.to_be_bytes());
    for start in [0x48u16, 0x78, 0xFFFF] {
        sub.extend_from_slice(&start.to_be_bytes());
    }
    for delta in [1u16.wrapping_sub(0x48), 2u16.wrapping_sub(0x78), 1] {
        sub.extend_from_slice(&delta.to_be_bytes());
    }
    sub.extend_from_slice(&[0; 6]);
    let length = sub.len() as u16;
    put16(&mut sub, 2, length);
    let mut cmap = Vec::new();
    for value in [0u16, 1, 3, 1] {
        cmap.extend_from_slice(&value.to_be_bytes());
    }
    cmap.extend_from_slice(&12u32.to_be_bytes());
    cmap.extend(sub);

    let face = sfnt(
        0x0001_0000,
        &[
            (*b"OS/2", os2(1, 0, 0, 0)),
            (*b"cmap", cmap),
            (*b"glyf", glyf),
            (*b"head", head(0)),
            (*b"loca", loca),
            (*b"maxp", maxp),
        ],
    );
    let facts = face_facts(&face).expect("the face reads");
    let em = 1000.0 / 2048.0;
    close(facts.cap_height, 1400.0 * em);
    close(facts.x_height, 1000.0 * em);
}

/// A record that is not a PostScript name — a space, a slash, sixty-four
/// characters, an odd UTF-16 length — is passed over for one that is, and a
/// face whose every record is one of those states no name at all.
#[test]
fn a_name_that_is_no_postscript_name_is_not_one() {
    let long = "N".repeat(64);
    let face = described(
        os2(4, 0, 0, 0),
        name(&[
            (3, 1, 6, utf16("Facts Sans")),
            (3, 1, 6, utf16("Facts/Sans")),
            (3, 1, 6, utf16(&long)),
            (3, 1, 6, vec![0, 0x46, 0]),
            (3, 1, 1, utf16("FactsFamily")),
            (1, 0, 6, b"Facts-Mac".to_vec()),
        ]),
    );
    assert_eq!(
        face_facts(&face)
            .expect("it reads")
            .postscript_name
            .as_deref(),
        Some("Facts-Mac"),
        "the one record of ID 6 that is a PostScript name"
    );
    let unnamed = described(os2(4, 0, 0, 0), name(&[(3, 1, 6, utf16("Facts Sans"))]));
    assert_eq!(
        face_facts(&unnamed).expect("it reads").postscript_name,
        None
    );
}

/// Wraps `items` as a CFF INDEX with four-byte offsets.
fn index(items: &[Vec<u8>]) -> Vec<u8> {
    if items.is_empty() {
        return vec![0, 0];
    }
    let mut out = (items.len() as u16).to_be_bytes().to_vec();
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

/// One DICT entry, its operands in the fixed five-byte form.
fn entry(op: u16, operands: &[i32]) -> Vec<u8> {
    let mut out = Vec::new();
    for value in operands {
        out.push(29);
        out.extend_from_slice(&value.to_be_bytes());
    }
    if op > 0xFF {
        out.extend_from_slice(&[12, (op & 0xFF) as u8]);
    } else {
        out.push(op as u8);
    }
    out
}

/// A Type 2 operand in its 16-bit form, which every value here fits.
fn number(out: &mut Vec<u8>, value: i16) {
    out.push(28);
    out.extend_from_slice(&value.to_be_bytes());
}

/// A charstring drawing a box 500 wide and `top` tall from the origin.
fn tall_box(top: i16) -> Vec<u8> {
    let mut out = Vec::new();
    number(&mut out, 0);
    number(&mut out, 0);
    out.push(21); // rmoveto
    for (dx, dy) in [(500, 0), (0, top), (-500, 0)] {
        number(&mut out, dx);
        number(&mut out, dy);
        out.push(5); // rlineto
    }
    out.push(14); // endchar
    out
}

/// A bare CFF named `FactsSerif`: `H` 700 tall and `x` 450, a box of
/// `[-50, -210, 950, 820]`, an italic angle of -12, fixed pitch, and a
/// `StdVW` of 88 in its Private DICT.
fn bare_cff() -> Vec<u8> {
    let header = [1u8, 0, 4, 4];
    let names = index(&[b"FactsSerif".to_vec()]);
    let strings = index(&[]);
    let globals = index(&[]);
    // SIDs 41 and 89 are `H` and `x` among the standard strings.
    let mut charset = vec![0u8];
    for sid in [41u16, 89] {
        charset.extend_from_slice(&sid.to_be_bytes());
    }
    let charstrings = index(&[vec![14], tall_box(700), tall_box(450)]);
    let mut private = entry(11, &[88]);
    private.extend(entry(20, &[500]));
    private.extend(entry(21, &[500]));
    let top = |charset_at: i32, charstrings_at: i32, private_at: i32| {
        let mut out = entry(5, &[-50, -210, 950, 820]);
        out.extend(entry(0x0C02, &[-12]));
        out.extend(entry(0x0C01, &[1]));
        out.extend(entry(15, &[charset_at]));
        out.extend(entry(17, &[charstrings_at]));
        out.extend(entry(18, &[private.len() as i32, private_at]));
        out
    };
    let top_index_len = index(&[top(0, 0, 0)]).len();
    let charset_at = header.len() + names.len() + top_index_len + strings.len() + globals.len();
    let charstrings_at = charset_at + charset.len();
    let private_at = charstrings_at + charstrings.len();
    let tops = index(&[top(
        charset_at as i32,
        charstrings_at as i32,
        private_at as i32,
    )]);
    let mut out = header.to_vec();
    for part in [
        &names,
        &tops,
        &strings,
        &globals,
        &charset,
        &charstrings,
        &private,
    ] {
        out.extend_from_slice(part);
    }
    out
}

/// A bare CFF states its name, box, angle, pitch and stem in its DICTs and
/// its heights in the glyphs named `H` and `x`; with no `hhea` of its own,
/// its box's top and bottom stand for its ascent and descent. It states no
/// licence and no class.
#[test]
fn a_bare_cff_states_its_box_angle_stem_and_name() {
    let program = bare_cff();
    let facts = face_facts(&program).expect("the program reads");
    assert_eq!(facts.postscript_name.as_deref(), Some("FactsSerif"));
    assert_eq!(facts.bbox, Some([-50.0, -210.0, 950.0, 820.0]));
    close(facts.italic_angle, -12.0);
    assert_eq!(facts.fixed_pitch, Some(true));
    close(facts.stem_v, 88.0);
    close(facts.cap_height, 700.0);
    close(facts.x_height, 450.0);
    close(facts.ascent, 820.0);
    close(facts.descent, -210.0);
    assert_eq!((facts.fs_type, facts.family_class), (None, None));
}

/// The same CFF in an `OTTO` wrapper keeps its stem and its glyphs' heights
/// from the CFF and takes everything an sfnt states from the sfnt — its
/// `hhea`, not its box, for the ascent.
#[test]
fn an_opentype_cff_face_reads_its_tables_and_its_outlines() {
    // `H` and `x` through a (3,1) format 4 subtable, to glyphs 1 and 2.
    let mut sub = Vec::new();
    for value in [4u16, 0, 0, 6, 0, 0, 0] {
        sub.extend_from_slice(&value.to_be_bytes());
    }
    for end in [0x48u16, 0x78, 0xFFFF] {
        sub.extend_from_slice(&end.to_be_bytes());
    }
    sub.extend_from_slice(&0u16.to_be_bytes());
    for start in [0x48u16, 0x78, 0xFFFF] {
        sub.extend_from_slice(&start.to_be_bytes());
    }
    for delta in [1u16.wrapping_sub(0x48), 2u16.wrapping_sub(0x78), 1] {
        sub.extend_from_slice(&delta.to_be_bytes());
    }
    sub.extend_from_slice(&[0; 6]);
    let length = sub.len() as u16;
    put16(&mut sub, 2, length);
    let mut cmap = Vec::new();
    for value in [0u16, 1, 3, 1] {
        cmap.extend_from_slice(&value.to_be_bytes());
    }
    cmap.extend_from_slice(&12u32.to_be_bytes());
    cmap.extend(sub);

    let mut head1000 = head(0);
    put16(&mut head1000, 18, 1000);
    let face = sfnt(
        0x4F54_544F,
        &[
            (*b"CFF ", bare_cff()),
            (*b"OS/2", os2(1, 0x0100, 0x0203, 0)),
            (*b"cmap", cmap),
            (*b"head", head1000),
            (*b"hhea", hhea(900, -250)),
        ],
    );
    let facts = face_facts(&face).expect("the face reads");
    close(facts.ascent, 900.0);
    close(facts.descent, -250.0);
    close(facts.stem_v, 88.0);
    close(facts.cap_height, 700.0);
    close(facts.x_height, 450.0);
    assert_eq!(facts.fs_type, Some(0x0100));
    assert_eq!(facts.family_class, Some(0x0203));
    assert_eq!(
        facts.postscript_name.as_deref(),
        Some("FactsSerif"),
        "no name table, so the CFF's Name INDEX"
    );
}

/// Bytes neither parser reads state nothing, and nothing panics on them
/// (ruling 1): an empty slice, a truncated directory, a CFF header alone.
#[test]
fn garbage_states_nothing() {
    for bytes in [
        &[][..],
        &[0u8, 1, 0][..],
        &[1u8, 0, 4, 4][..],
        &b"%!PS-AdobeFont-1.0"[..],
    ] {
        assert_eq!(face_facts(bytes), None, "{bytes:?}");
    }
    // A directory whose every table is too short to hold its fields reads,
    // and states nothing.
    let hollow = sfnt(
        0x0001_0000,
        &[
            (*b"OS/2", vec![0, 4]),
            (*b"head", vec![0; 20]),
            (*b"hhea", vec![0; 4]),
            (*b"name", vec![0, 0, 0, 1]),
            (*b"post", vec![0; 6]),
        ],
    );
    let facts = face_facts(&hollow).expect("the directory reads");
    assert_eq!(facts.bbox, None);
    assert_eq!(facts.ascent, None);
    assert_eq!(facts.fs_type, None);
    assert_eq!(facts.italic_angle, None);
    assert_eq!(facts.postscript_name, None);
}
