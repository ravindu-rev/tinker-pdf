//! What the writer does with a CFF face (9.9, 9.6.4).
//!
//! Until August 2026 `tinker_pdf_font::subset` answered `None` for every CFF
//! program, both call sites in `build.rs` swallowed that, and the whole face
//! went into the file with **no warning at all** — the only observable
//! difference being the missing `ABCDEF+` tag on `/BaseFont`, which nothing
//! checked. Two things are pinned here: that a CFF face is now cut down and
//! tagged, and that a face which still cannot be cut down says so through a
//! typed value (ruling 10).
//!
//! Every byte of every font is built in this file. `testdata/` is four
//! self-authored PDFs and none of them embeds a CFF, and "the subset draws the
//! same glyph" is only checkable against a fixture whose every glyph is a box
//! of a size chosen here.

use tinker_pdf_cos::{
    CosDocument, Dict, DocumentBuilder, EmbeddedWhole, Glyph, Object, SubsetRefusal,
};

// ---------------------------------------------------------------------------
// Building a CFF font program.
// ---------------------------------------------------------------------------

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

/// One DICT entry: operands in the fixed five-byte form, then the operator.
fn entry(op: u16, operands: &[i32]) -> Vec<u8> {
    let mut out = Vec::new();
    for value in operands {
        out.push(29);
        out.extend_from_slice(&value.to_be_bytes());
    }
    if op > 0xFF {
        out.push(12);
        out.push((op & 0xFF) as u8);
    } else {
        out.push(op as u8);
    }
    out
}

/// A Type 2 operand, in the two forms these fixtures need.
fn t2(value: i32) -> Vec<u8> {
    if (-107..=107).contains(&value) {
        vec![(value + 139) as u8]
    } else {
        let bytes = (value as i16).to_be_bytes();
        vec![28, bytes[0], bytes[1]]
    }
}

/// A subroutine drawing a box `size` units on a side, then returning.
fn box_subr(size: i32) -> Vec<u8> {
    let mut out = Vec::new();
    for (dx, dy) in [(size, 0), (0, size), (-size, 0)] {
        out.extend(t2(dx));
        out.extend(t2(dy));
        out.push(5); // rlineto
    }
    out.push(11); // return
    out
}

/// How many glyphs the fixtures carry, `.notdef` included. Twenty-seven, so
/// that glyphs 1..=26 are `A`..`Z` among the standard strings.
const GLYPHS: usize = 27;

/// The bias for a local subroutine INDEX of `count` entries.
fn bias(count: usize) -> i32 {
    if count < 1240 {
        107
    } else if count < 33900 {
        1131
    } else {
        32768
    }
}

/// A CFF program: twenty-six letters, each drawn by a local subroutine of its
/// own, so that dropping a glyph drops a subroutine and every survivor's
/// `callsubr` operand has to be rewritten.
///
/// `charstrings` may be overridden to build the refusal fixtures.
fn cff_program(charstrings: Option<Vec<Vec<u8>>>) -> Vec<u8> {
    let subrs: Vec<Vec<u8>> = (0..GLYPHS - 1)
        .map(|i| box_subr(200 + i as i32 * 10))
        .collect();
    let charstrings = charstrings.unwrap_or_else(|| {
        let mut out = vec![vec![14u8]];
        for i in 0..GLYPHS - 1 {
            let mut code = t2(0);
            code.extend(t2(0));
            code.push(21); // rmoveto
            code.extend(t2(i as i32 - bias(subrs.len())));
            code.push(10); // callsubr
            code.push(14); // endchar
            out.push(code);
        }
        out
    });

    // defaultWidthX and nominalWidthX, then Subrs six bytes on.
    let mut private = entry(20, &[600]);
    private.extend(entry(21, &[600]));
    private.extend(entry(19, &[private.len() as i32 + 6]));

    let header = [1u8, 0, 4, 4];
    let names = index(&[b"Fixture".to_vec()]);
    let strings = index(&[]);
    let gsubrs = index(&[]);
    let charstring_index = index(&charstrings);
    let subr_index = index(&subrs);

    // The charset in format 0: SIDs 34..59 are `A`..`Z`.
    let mut charset = vec![0u8];
    for sid in 0..(charstrings.len() as u16 - 1) {
        charset.extend_from_slice(&(34 + sid).to_be_bytes());
    }

    let top = |charset_at: i32, charstrings_at: i32, private_at: i32| {
        let mut out = entry(15, &[charset_at]);
        out.extend(entry(16, &[0]));
        out.extend(entry(17, &[charstrings_at]));
        out.extend(entry(18, &[private.len() as i32, private_at]));
        out
    };

    let top_len = top(0, 0, 0).len();
    let mut cursor =
        header.len() + names.len() + (2 + 1 + 8 + top_len) + strings.len() + gsubrs.len();
    let charset_at = cursor;
    cursor += charset.len();
    let charstrings_at = cursor;
    cursor += charstring_index.len();
    let private_at = cursor;

    let mut out = header.to_vec();
    out.extend_from_slice(&names);
    out.extend_from_slice(&index(&[top(
        charset_at as i32,
        charstrings_at as i32,
        private_at as i32,
    )]));
    out.extend_from_slice(&strings);
    out.extend_from_slice(&gsubrs);
    assert_eq!(out.len(), charset_at, "the charset lands where it was put");
    out.extend_from_slice(&charset);
    assert_eq!(out.len(), charstrings_at);
    out.extend_from_slice(&charstring_index);
    assert_eq!(out.len(), private_at);
    out.extend_from_slice(&private);
    out.extend_from_slice(&subr_index);
    out
}

/// Wraps a CFF program in an `OTTO` sfnt with the tables a font dictionary's
/// widths and glyph lookups come out of.
fn otto(cff: &[u8]) -> Vec<u8> {
    let count = GLYPHS as u16;

    let mut head = vec![0u8; 54];
    head[18..20].copy_from_slice(&1000u16.to_be_bytes()); // unitsPerEm

    let mut maxp = vec![0u8; 6];
    maxp[0..4].copy_from_slice(&0x0000_5000u32.to_be_bytes()); // CFF maxp version
    maxp[4..6].copy_from_slice(&count.to_be_bytes());

    let mut hhea = vec![0u8; 36];
    hhea[34..36].copy_from_slice(&count.to_be_bytes()); // numberOfHMetrics

    let mut hmtx = Vec::new();
    for glyph in 0..count {
        hmtx.extend_from_slice(&(500 + glyph).to_be_bytes());
        hmtx.extend_from_slice(&0i16.to_be_bytes());
    }

    // cmap format 4, mapping 'A'..'Z' onto glyphs 1..26 by a constant delta.
    let first = u16::from(b'A');
    let last = first + count - 2;
    let mut sub = Vec::new();
    for value in [
        4u16,
        32,
        0,
        4,
        4,
        1,
        0,
        last,
        0xFFFF,
        0,
        first,
        0xFFFF,
        1u16.wrapping_sub(first),
        1,
        0,
        0,
    ] {
        sub.extend_from_slice(&value.to_be_bytes());
    }
    let mut cmap = Vec::new();
    for value in [0u16, 1, 3, 1] {
        cmap.extend_from_slice(&value.to_be_bytes());
    }
    cmap.extend_from_slice(&12u32.to_be_bytes());
    cmap.extend_from_slice(&sub);

    let tables: Vec<(&[u8; 4], Vec<u8>)> = vec![
        (b"CFF ", cff.to_vec()),
        (b"cmap", cmap),
        (b"head", head),
        (b"hhea", hhea),
        (b"hmtx", hmtx),
        (b"maxp", maxp),
    ];

    let mut out = Vec::new();
    out.extend_from_slice(b"OTTO");
    out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0; 6]); // the binary-search hints, unread here

    let mut offset = 12 + tables.len() * 16;
    let mut body = Vec::new();
    for (tag, data) in &tables {
        out.extend_from_slice(*tag);
        out.extend_from_slice(&0u32.to_be_bytes()); // checksum
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        offset += data.len();
        while offset % 4 != 0 {
            offset += 1;
        }
        body.extend_from_slice(data);
        while body.len() % 4 != 0 {
            body.push(0);
        }
    }
    out.extend_from_slice(&body);
    out
}

/// A CFF whose one drawn glyph takes its subroutine number off the transient
/// array, so the number is not a token the subsetter could rewrite. The reader
/// accepts it; the writer refuses to guess.
fn uncuttable_cff() -> Vec<u8> {
    let mut charstrings = vec![vec![14u8]];
    for _ in 1..GLYPHS {
        let mut code = t2(0);
        code.extend([12, 21]); // get
        code.push(10); // callsubr
        code.push(14);
        charstrings.push(code);
    }
    cff_program(Some(charstrings))
}

// ---------------------------------------------------------------------------
// Reading the written document back.
// ---------------------------------------------------------------------------

fn page_font(doc: &CosDocument, name: &[u8]) -> Dict {
    let pages = tinker_pdf_cos::pages::collect(doc);
    let page = pages.first().expect("a page");
    let resources = page.resources.as_ref().expect("the page has resources");
    let fonts = doc.resolve_key(resources, doc.intern(b"Font"));
    let fonts = fonts.as_dict().expect("a /Font sub-dictionary");
    doc.resolve_key(fonts, doc.intern(name))
        .as_dict()
        .cloned()
        .expect("the font resource")
}

/// The descendant of a Type0 font, or the font itself when it is simple.
fn descendant(doc: &CosDocument, font: &Dict) -> Dict {
    let descendants = doc.resolve_key(font, doc.intern(b"DescendantFonts"));
    match descendants.as_array().and_then(<[Object]>::first) {
        Some(first) => doc
            .resolve(first)
            .as_dict()
            .cloned()
            .expect("the descendant font"),
        None => font.clone(),
    }
}

fn name_of(doc: &CosDocument, dict: &Dict, key: &[u8]) -> Option<Vec<u8>> {
    doc.resolve_key(dict, doc.intern(key))
        .as_name()
        .and_then(|n| doc.name_bytes(n))
        .map(|bytes| bytes.to_vec())
}

/// The embedded program, the descriptor key it hung off, and the stream's own
/// `/Subtype` where it has one.
fn embedded(doc: &CosDocument, resource: &[u8]) -> (Vec<u8>, Vec<u8>, Option<Vec<u8>>) {
    let font = page_font(doc, resource);
    let descendant = descendant(doc, &font);
    let descriptor = doc.resolve_key(&descendant, doc.intern(b"FontDescriptor"));
    let descriptor = descriptor.as_dict().expect("a font descriptor");

    for key in [b"FontFile2".as_slice(), b"FontFile3"] {
        let Some(reference) = descriptor.get_ref(doc.intern(key)) else {
            continue;
        };
        let bytes = doc.stream_decoded(reference).expect("the program decodes");
        let dict = doc.resolve(&Object::Ref(reference));
        let subtype = dict
            .as_dict()
            .and_then(|dict| name_of(doc, dict, b"Subtype"));
        return (bytes, key.to_vec(), subtype);
    }
    panic!("no font program was embedded");
}

/// Asserts the `/BaseFont` carries the 9.6.4 six-letter tag, and returns the
/// name after it.
#[track_caller]
fn tagged(name: &[u8]) -> Vec<u8> {
    let at = name
        .iter()
        .position(|b| *b == b'+')
        .unwrap_or_else(|| panic!("{} carries no subset tag", String::from_utf8_lossy(name)));
    assert_eq!(at, 6, "9.6.4: six letters and a plus sign");
    assert!(name[..6].iter().all(u8::is_ascii_uppercase));
    name[at + 1..].to_vec()
}

/// Every glyph in `kept` draws in `subset` exactly what it draws in
/// `original`, and the charset still names the same glyphs.
#[track_caller]
fn outlines_agree(original: &[u8], subset: &[u8], kept: &[u16]) {
    let before = tinker_pdf_font::Cff::parse(original).expect("the original parses");
    let after = tinker_pdf_font::Cff::parse(subset).expect("the subset parses");
    assert_eq!(before.glyph_count(), after.glyph_count());
    for &glyph in kept {
        assert_eq!(
            before.outline(glyph).expect("the original").segments,
            after.outline(glyph).expect("the subset").segments,
            "glyph {glyph} draws something else in the subset"
        );
    }
    for name in ["A", "M", "Z"] {
        assert_eq!(
            after.gid_for_name(name),
            before.gid_for_name(name),
            "{name} moved"
        );
    }
}

/// The `CFF ` table of an sfnt, or the bytes themselves when they are bare.
fn cff_of(program: &[u8]) -> Vec<u8> {
    match tinker_pdf_font::Sfnt::parse(program) {
        Some(sfnt) => sfnt.table(0x4346_4620).expect("a CFF table").to_vec(),
        None => program.to_vec(),
    }
}

/// A one-page document drawing `text` with the program under `F0`.
fn simple_document(program: &[u8], text: &str) -> (Vec<u8>, Vec<EmbeddedWhole>) {
    let mut builder = DocumentBuilder::new();
    assert!(
        builder.add_embedded_font(b"F0", b"Fixture", program),
        "the program registers"
    );
    builder.add_page(200.0, 100.0, |page| {
        page.text(b"F0", 24.0, 20.0, 40.0, text);
    });
    builder.finish_reporting()
}

// ---------------------------------------------------------------------------
// The tests.
// ---------------------------------------------------------------------------

#[test]
fn the_fixture_is_a_face_this_engine_reads() {
    let cff = cff_program(None);
    let parsed = tinker_pdf_font::Cff::parse(&cff).expect("it parses");
    assert_eq!(parsed.glyph_count(), GLYPHS);
    assert_eq!(parsed.gid_for_name("A"), Some(1));
    assert_eq!(parsed.gid_for_name("Z"), Some(26));
    assert!(!parsed.outline(3).expect("an outline").segments.is_empty());

    let wrapped = otto(&cff);
    let sfnt = tinker_pdf_font::Sfnt::parse(&wrapped).expect("the wrapper parses");
    assert_eq!(sfnt.glyph_for_char('C'), Some(3));
    assert_eq!(sfnt.advance(3), Some(503));
}

/// The headline for an `OpenType/CFF` face: the program shrinks, the name
/// carries the 9.6.4 tag, and every glyph the pages drew still draws exactly
/// what it drew.
#[test]
fn an_opentype_cff_face_is_subsetted_and_tagged() {
    let cff = cff_program(None);
    let program = otto(&cff);
    let (bytes, warnings) = simple_document(&program, "AC");

    assert!(warnings.is_empty(), "nothing declined: {warnings:?}");
    let doc = CosDocument::open(bytes).expect("the document opens");
    let font = page_font(&doc, b"F0");
    let base = name_of(&doc, &font, b"BaseFont").expect("a /BaseFont");
    assert_eq!(tagged(&base), b"Fixture");

    let (embedded_program, key, subtype) = embedded(&doc, b"F0");
    assert_eq!(key, b"FontFile3", "9.9 Table 126: CFF outlines are FontFile3");
    assert_eq!(subtype.as_deref(), Some(&b"OpenType"[..]));
    assert!(
        embedded_program.len() < program.len(),
        "the subset is smaller: {} vs {}",
        embedded_program.len(),
        program.len()
    );

    // `A` is glyph 1 and `C` is glyph 3; every other letter is gone.
    outlines_agree(&cff, &cff_of(&embedded_program), &[0, 1, 3]);
    let inner = cff_of(&embedded_program);
    let after = tinker_pdf_font::Cff::parse(&inner).expect("it parses");
    assert!(
        after.outline(2).expect("glyph 2 answers").segments.is_empty(),
        "`B` was not drawn and must draw nothing"
    );
}

/// A bare `/FontFile3` program. Until this work `add_embedded_font` refused
/// one outright — `Sfnt::parse` was the gate — so a CFF face could not be
/// embedded at all, subsetted or otherwise.
#[test]
fn a_bare_cff_program_is_embedded_as_a_type1_font() {
    let cff = cff_program(None);
    let (bytes, warnings) = simple_document(&cff, "AC");
    assert!(warnings.is_empty(), "nothing declined: {warnings:?}");

    let doc = CosDocument::open(bytes).expect("the document opens");
    let font = page_font(&doc, b"F0");
    assert_eq!(
        name_of(&doc, &font, b"Subtype").as_deref(),
        Some(&b"Type1"[..]),
        "9.6.2.1: a bare CFF is a Type 1 font program"
    );

    let (embedded_program, key, subtype) = embedded(&doc, b"F0");
    assert_eq!(key, b"FontFile3");
    assert_eq!(subtype.as_deref(), Some(&b"Type1C"[..]));
    assert!(embedded_program.len() < cff.len());
    outlines_agree(&cff, &embedded_program, &[0, 1, 3]);

    // The widths come from the program's own `defaultWidthX` through its
    // `FontMatrix`, not from the 500 an unresolved glyph falls back to.
    let widths = doc.resolve_key(&font, doc.intern(b"Widths"));
    let widths: Vec<f64> = widths
        .as_array()
        .expect("a /Widths array")
        .iter()
        .filter_map(|value| doc.resolve(value).as_number())
        .collect();
    // /FirstChar is 32, so `A` at code 65 is entry 33.
    assert_eq!(widths.get(33).copied(), Some(600.0), "the advance of `A`");
}

/// Ruling 10. A face that cannot be cut down is embedded whole — larger and
/// correct — and the refusal is a typed value naming the resource, not a
/// missing tag nobody looks at.
#[test]
fn a_face_that_cannot_be_cut_down_is_embedded_whole_and_says_so() {
    let program = uncuttable_cff();
    assert!(
        tinker_pdf_font::subset(&program, &[1u16].into_iter().collect()).is_none(),
        "the fixture really is one the subsetter refuses"
    );

    let (bytes, warnings) = simple_document(&program, "AC");
    assert_eq!(
        warnings,
        vec![EmbeddedWhole {
            resource: b"F0".to_vec(),
            base_font: b"Fixture".to_vec(),
            bytes: program.len(),
            reason: SubsetRefusal::ProgramNotRebuildable,
        }]
    );

    let doc = CosDocument::open(bytes).expect("the document opens");
    let font = page_font(&doc, b"F0");
    assert_eq!(
        name_of(&doc, &font, b"BaseFont").as_deref(),
        Some(&b"Fixture"[..]),
        "no subset tag, because there is no subset"
    );
    let (embedded_program, _, _) = embedded(&doc, b"F0");
    assert_eq!(embedded_program, program, "the whole face went in");
}

/// The other refusal: the pages drew text and the program claims none of it.
/// Subsetting on that evidence keeps `.notdef` alone and every letter comes
/// out blank, which reads as a rendering bug and gets found far too late.
#[test]
fn a_face_that_claims_none_of_the_text_is_embedded_whole_and_says_so() {
    // A charset of custom SIDs: the glyphs have names, and none of them is a
    // name any character resolves to.
    let mut program = cff_program(None);
    let charset_at = {
        let cff = tinker_pdf_font::Cff::parse(&program).expect("it parses");
        assert_eq!(cff.gid_for_name("A"), Some(1));
        // The charset is the first thing after the global subroutine INDEX,
        // and format 0 begins with a zero byte; find it by its content.
        let mut wanted = vec![0u8];
        for sid in 0..(GLYPHS as u16 - 1) {
            wanted.extend_from_slice(&(34 + sid).to_be_bytes());
        }
        program
            .windows(wanted.len())
            .position(|w| w == wanted)
            .expect("the charset is in the file")
    };
    // SIDs 391 upward index a String INDEX this fixture leaves empty, so no
    // glyph has a resolvable name at all.
    for sid in 0..(GLYPHS - 1) {
        let at = charset_at + 1 + sid * 2;
        program[at..at + 2].copy_from_slice(&(391 + sid as u16).to_be_bytes());
    }
    let cff = tinker_pdf_font::Cff::parse(&program).expect("it still parses");
    assert_eq!(cff.gid_for_name("A"), None, "no glyph is named `A` any more");
    assert!(
        tinker_pdf_font::glyphs_for(&program, "AC").is_empty(),
        "and nothing resolves the text"
    );

    let (_, warnings) = simple_document(&program, "AC");
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].reason, SubsetRefusal::NoGlyphResolved);
    assert_eq!(warnings[0].resource, b"F0");
}

/// A composite font over an `OpenType/CFF` face: the glyphs are addressed by
/// index, so `/W`, `/CIDToGIDMap` and `/ToUnicode` all rest on the subset
/// leaving those indices where they were.
#[test]
fn a_composite_font_over_a_cff_face_is_subsetted() {
    let cff = cff_program(None);
    let program = otto(&cff);

    let mut builder = DocumentBuilder::new();
    assert!(builder.add_cid_font(b"C0", b"Fixture", &program));
    builder.add_page(200.0, 100.0, |page| {
        page.glyphs(
            b"C0",
            24.0,
            20.0,
            40.0,
            &[
                Glyph { id: 4, text: "D" },
                Glyph { id: 9, text: "I" },
                Glyph { id: 20, text: "T" },
            ],
        );
    });
    let (bytes, warnings) = builder.finish_reporting();
    assert!(warnings.is_empty(), "nothing declined: {warnings:?}");

    let doc = CosDocument::open(bytes).expect("the document opens");
    let font = page_font(&doc, b"C0");
    let base = name_of(&doc, &font, b"BaseFont").expect("a /BaseFont");
    assert_eq!(tagged(&base), b"Fixture");

    let cid = descendant(&doc, &font);
    assert_eq!(
        name_of(&doc, &cid, b"Subtype").as_deref(),
        Some(&b"CIDFontType2"[..])
    );
    assert_eq!(
        name_of(&doc, &cid, b"CIDToGIDMap").as_deref(),
        Some(&b"Identity"[..]),
        "which is only true because the subset left the indices alone"
    );

    let (embedded_program, key, subtype) = embedded(&doc, b"C0");
    assert_eq!(key, b"FontFile3");
    assert_eq!(subtype.as_deref(), Some(&b"OpenType"[..]));
    assert!(embedded_program.len() < program.len());
    outlines_agree(&cff, &cff_of(&embedded_program), &[0, 4, 9, 20]);

    // `/W` is the *original* program's `hmtx` for those ids, and the ids it
    // names are the ones the page drew.
    let widths = doc.resolve_key(&cid, doc.intern(b"W"));
    let widths = widths.as_array().expect("a /W array");
    let first = doc.resolve(&widths[0]).as_int();
    assert_eq!(first, Some(4), "the first run starts at the first glyph drawn");
}

/// A **CID-keyed** CFF cannot go down the composite path: its charset maps a
/// CID onto a glyph and the two are different numbers, while
/// `PageBuilder::glyphs` addresses glyphs and `/Identity-H` would make every
/// one of those numbers a CID.
#[test]
fn a_cid_keyed_bare_cff_is_refused_by_the_composite_path() {
    // The same program with `ROS` spliced into its Top DICT. `entry(16, &[0])`
    // — the `Encoding` this fixture does not use — is six bytes, and `ROS`
    // with three one-byte operands and a two-byte operator is six as well.
    let mut program = cff_program(None);
    let target = entry(16, &[0]);
    let at = program
        .windows(target.len())
        .position(|w| w == target)
        .expect("the Encoding entry is in the file");
    // SID 391 and SID 392 in the two-byte operand form, then `12 30`. Four
    // operand bytes and two operator bytes is exactly the six the `Encoding`
    // entry occupied; the supplement is left off, and the reader defaults it,
    // because ROS's presence — not its operands — is what makes a font
    // CID-keyed.
    let ros = vec![247u8, 27, 247, 28, 12, 30];
    assert_eq!(ros.len(), target.len());
    program[at..at + target.len()].copy_from_slice(&ros);

    let parsed = tinker_pdf_font::Cff::parse(&program).expect("it parses");
    assert!(parsed.is_cid(), "the fixture really is CID-keyed now");

    let mut builder = DocumentBuilder::new();
    assert!(
        !builder.add_cid_font(b"C0", b"Fixture", &program),
        "9.7.4.2: the CID is not the glyph index for a CID-keyed CFF"
    );
    // And the simple path still takes it, so the refusal is about the
    // composite semantics and not about the bytes.
    assert!(builder.add_embedded_font(b"F0", b"Fixture", &program));
}

/// Turning subsetting off is the caller's stated intent, not a capability this
/// engine lacked, so nothing is reported.
#[test]
fn subsetting_turned_off_reports_nothing() {
    let cff = cff_program(None);
    let mut builder = DocumentBuilder::new();
    builder.set_subset_fonts(false);
    assert!(builder.add_embedded_font(b"F0", b"Fixture", &cff));
    builder.add_page(200.0, 100.0, |page| {
        page.text(b"F0", 24.0, 20.0, 40.0, "AC");
    });
    let (bytes, warnings) = builder.finish_reporting();
    assert!(warnings.is_empty(), "the caller asked for the whole face");

    let doc = CosDocument::open(bytes).expect("the document opens");
    let (embedded_program, _, _) = embedded(&doc, b"F0");
    assert_eq!(embedded_program, cff);
}
