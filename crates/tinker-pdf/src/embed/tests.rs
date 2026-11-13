//! The pass's own rules, one at a time; the pass over whole documents is
//! `tests/embed_missing.rs`'s.

use super::*;

use tinker_pdf_cos::CosDocument;

/// 9.9.1 and OpenType's `fsType`: the low four bits are one usage
/// permission, restricted-licence embedding alone forbids it, and where more
/// than one is set the least restrictive applies; bitmap-only forbids
/// outlines; no-subsetting forbids the cut the pass makes.
#[test]
fn the_licence_bits_are_read_as_9_9_1_reads_them() {
    for (bits, verdict) in [
        (None, Ok(())),
        (Some(0x0000), Ok(())),
        (Some(0x0004), Ok(())),
        (Some(0x0008), Ok(())),
        (Some(0x0006), Ok(())),
        (Some(0x000A), Ok(())),
        (Some(0x0002), Err(LeftReason::EmbeddingForbidden)),
        (Some(0x0200), Err(LeftReason::EmbeddingForbidden)),
        (Some(0x0208), Err(LeftReason::EmbeddingForbidden)),
        (Some(0x0302), Err(LeftReason::EmbeddingForbidden)),
        (Some(0x0100), Err(LeftReason::SubsettingForbidden)),
        (Some(0x0108), Err(LeftReason::SubsettingForbidden)),
    ] {
        assert_eq!(licence(bits), verdict, "fsType {bits:04X?}");
    }
}

/// 9.9.2 names a subset after the font it was cut from, which is the face;
/// a face with no PostScript name of its own gives the document's name
/// without its old tag, and an empty one a name that is still a name.
#[test]
fn a_subset_is_named_after_the_face_else_the_document() {
    let named = FaceFacts {
        postscript_name: Some("LiberationSans".to_string()),
        ..FaceFacts::default()
    };
    assert_eq!(stem(&named, "Helvetica"), b"LiberationSans");
    let unnamed = FaceFacts::default();
    assert_eq!(stem(&unnamed, "ABCDEF+Helvetica"), b"Helvetica");
    assert_eq!(stem(&unnamed, "Abc+Helvetica"), b"Abc+Helvetica", "no tag");
    assert_eq!(stem(&unnamed, ""), b"Embedded");
}

/// A document of fonts in every shape a candidate is told apart by.
fn fonts_document() -> CosDocument {
    let bytes: &[u8] = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 0 /Kids [] >>\nendobj\n\
3 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n\
4 0 obj\n<< /Type /Font /Subtype /TrueType /BaseFont /Arial /FontDescriptor << /Flags 32 >> >>\nendobj\n\
5 0 obj\n<< /Type /Font /Subtype /TrueType /BaseFont /Embedded /FontDescriptor << /FontFile2 6 0 R >> >>\nendobj\n\
6 0 obj\n<< /Length 4 >>\nstream\nabcd\nendstream\nendobj\n\
7 0 obj\n<< /Type /Font /Subtype /TrueType /BaseFont /Dangling /FontDescriptor << /FontFile2 99 0 R /FontFile3 8 0 R >> >>\nendobj\n\
8 0 obj\n<< /NotAStream true >>\nendobj\n\
9 0 obj\n<< /Type /Font /Subtype /Type3 /FontMatrix [0.001 0 0 0.001 0 0] /CharProcs << >> >>\nendobj\n\
10 0 obj\n<< /Type /Font /Subtype /Type0 /BaseFont /Mincho /DescendantFonts [11 0 R] >>\nendobj\n\
11 0 obj\n<< /Type /Font /Subtype /CIDFontType0 /BaseFont /Mincho /FontDescriptor << /Flags 4 >> >>\nendobj\n\
12 0 obj\n<< /Type /Font /Subtype /Type0 /BaseFont /Gothic /DescendantFonts [13 0 R] >>\nendobj\n\
13 0 obj\n<< /Type /Font /Subtype /CIDFontType0 /FontDescriptor << /FontFile3 6 0 R >> >>\nendobj\n\
14 0 obj\n<< /Type /Font /Subtype /Type0 /BaseFont /Orphan >>\nendobj\n\
15 0 obj\n<< /Type /Font /BaseFont /NoSubtype >>\nendobj\n\
trailer\n<< /Size 16 /Root 1 0 R >>\n%%EOF\n";
    CosDocument::open(bytes).expect("it opens")
}

fn font_at(doc: &CosDocument, num: u32) -> Dict {
    doc.get(ObjRef::new(num, 0))
        .expect("loads")
        .as_dict()
        .cloned()
        .expect("a dictionary")
}

/// A font is a candidate exactly when its dictionary — a composite font's
/// descendant's — names no `/FontFile*` stream, and it is not a Type 3 font:
/// a key naming nothing, or something that is no stream, embeds nothing.
#[test]
fn a_font_that_names_a_stream_is_no_candidate() {
    let doc = fonts_document();
    for (num, expected, what) in [
        (3, Some(Kind::Simple), "a standard font, with no descriptor"),
        (4, Some(Kind::Simple), "a descriptor that names no program"),
        (5, None, "a /FontFile2 stream"),
        (
            7,
            Some(Kind::Simple),
            "a key naming nothing, and one naming a dictionary",
        ),
        (9, None, "a Type 3 font"),
        (
            10,
            Some(Kind::Composite),
            "a descendant that embeds nothing",
        ),
        (12, None, "a descendant's /FontFile3 stream"),
        (14, None, "a Type 0 font with no descendant"),
        (15, Some(Kind::Simple), "no /Subtype, which reads as Type 1"),
    ] {
        assert_eq!(candidate(&doc, &font_at(&doc, num)), expected, "{what}");
    }
}

/// `/BaseFont` as written, and an empty name where there is none.
#[test]
fn the_base_font_is_read_as_written() {
    let doc = fonts_document();
    assert_eq!(base_font_of(&doc, &font_at(&doc, 3)), "Helvetica");
    assert_eq!(base_font_of(&doc, &font_at(&doc, 9)), "");
}

/// A table directory over `tables`, each empty: enough for the shape an
/// answer is told by, which is the tables it has.
fn directory(version: u32, tags: &[&[u8; 4]]) -> Vec<u8> {
    let mut out = version.to_be_bytes().to_vec();
    out.extend_from_slice(&(tags.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0; 6]);
    let end = (12 + tags.len() * 16) as u32;
    for tag in tags {
        out.extend_from_slice(*tag);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&end.to_be_bytes());
        out.extend_from_slice(&0u32.to_be_bytes());
    }
    out
}

/// A `glyf` face with `loca` is a TrueType answer, whole; one with `glyf`
/// and no `loca`, or with neither outline table, or bytes no parser reads,
/// is no answer the pass can embed.
#[test]
fn an_answer_is_read_by_its_outlines() {
    let truetype = directory(0x0001_0000, &[b"glyf", b"head", b"loca"]);
    let answer = Answer::simple(&truetype).expect("a TrueType answer");
    assert_eq!(answer.format, Format::TrueType);
    assert_eq!(
        answer.program.len(),
        truetype.len(),
        "the whole sfnt is cut"
    );

    for unreadable in [
        directory(0x0001_0000, &[b"glyf", b"head"]),
        directory(0x0001_0000, &[b"head", b"hhea"]),
        directory(0x4F54_544F, &[b"CFF ", b"head"]),
        b"%!PS-AdobeFont-1.0: Helvetica".to_vec(),
        Vec::new(),
    ] {
        assert!(
            Answer::simple(&unreadable).is_none(),
            "{} bytes read as an answer",
            unreadable.len()
        );
    }
}

/// 7.3.3: an integer is written as one, a fraction as a real, and a value
/// with no spelling — not finite — as nothing a reader could take for a
/// width it was given.
#[test]
fn numbers_are_written_as_7_3_3_spells_them() {
    assert_eq!(number(278.0), Object::Int(278));
    assert_eq!(number(-0.0), Object::Int(0));
    assert_eq!(number(556.5), Object::Real(556.5));
    assert_eq!(number(f64::INFINITY), Object::Int(0));
    assert_eq!(number(f64::NAN), Object::Int(0));
    assert_eq!(length(712.6), Object::Int(713));
    assert_eq!(length(-207.4), Object::Int(-207));
    assert_eq!(length(f64::NAN), Object::Int(0));
}

/// The descriptor written for a standard font, which has none: every key
/// Table 120 requires, from the face where it states one, the flags from the
/// face's class and angle, the symbolic bit the font's own, and the
/// program's key.
#[test]
fn a_standard_fonts_descriptor_is_written_from_the_face() {
    let doc = fonts_document();
    let editor = DocumentEditor::new(std::sync::Arc::new(doc));
    let holder = font_at(editor.document(), 3);
    let facts = FaceFacts {
        postscript_name: Some("LiberationSerif-Italic".to_string()),
        family_class: Some(0x0105),
        italic_angle: Some(-12.25),
        bbox: Some([-176.0, -303.0, 1005.0, 981.0]),
        ascent: Some(891.0),
        descent: Some(-216.0),
        cap_height: Some(662.0),
        x_height: None,
        ..FaceFacts::default()
    };
    let name = editor.intern(b"ABCDEF+LiberationSerif-Italic");
    let program = ObjRef::new(40, 0);
    let written = descriptor_for(
        &editor,
        &holder,
        &facts,
        &FontRequest::default(),
        name,
        (Name::FONT_FILE2, program),
        false,
    );
    let int = |key: &[u8]| written.get_int(editor.intern(key));
    assert_eq!(
        written.get_name(Name::TYPE),
        Some(editor.intern(b"FontDescriptor"))
    );
    assert_eq!(written.get_name(editor.intern(b"FontName")), Some(name));
    assert_eq!(
        int(b"Flags"),
        Some(SERIF | ITALIC | NONSYMBOLIC),
        "class 1 is a serif class, the angle italic, and the font not symbolic"
    );
    assert_eq!(
        written.get_number(editor.intern(b"ItalicAngle")),
        Some(-12.25)
    );
    assert_eq!(int(b"Ascent"), Some(891));
    assert_eq!(int(b"Descent"), Some(-216));
    assert_eq!(int(b"CapHeight"), Some(662));
    assert_eq!(int(b"StemV"), Some(0), "a stem the face does not state");
    assert!(
        !written.contains_key(editor.intern(b"XHeight")),
        "a height the face does not state is not written"
    );
    assert_eq!(
        written.get_array(editor.intern(b"FontBBox")),
        Some(
            &[
                Object::Int(-176),
                Object::Int(-303),
                Object::Int(1005),
                Object::Int(981)
            ][..]
        )
    );
    assert_eq!(written.get_ref(Name::FONT_FILE2), Some(program));
}

/// The descriptor written for a font that had one: its entries kept, its
/// flags kept but for the symbolic pair, every stale program key, `/CharSet`
/// and `/CIDSet` dropped, and only what it lacks completed.
#[test]
fn a_fonts_own_descriptor_is_completed_not_replaced() {
    let bytes: &[u8] = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 0 /Kids [] >>\nendobj\n\
3 0 obj\n<< /Type /Font /Subtype /TrueType /BaseFont /Arial /FontDescriptor 4 0 R >>\nendobj\n\
4 0 obj\n<< /Type /FontDescriptor /FontName /Arial /Flags 262178 /Ascent 905 /StemV 88 \
/MissingWidth 250 /FontFile 9 0 R /CharSet (/a/b) /CIDSet 9 0 R >>\nendobj\n\
trailer\n<< /Size 5 /Root 1 0 R >>\n%%EOF\n";
    let editor = DocumentEditor::new(std::sync::Arc::new(
        CosDocument::open(bytes).expect("it opens"),
    ));
    let holder = font_at(editor.document(), 3);
    let facts = FaceFacts {
        ascent: Some(1000.0),
        stem_v: Some(120.0),
        cap_height: Some(716.0),
        ..FaceFacts::default()
    };
    let name = editor.intern(b"GHIJKL+ArialMT");
    let written = descriptor_for(
        &editor,
        &holder,
        &facts,
        &FontRequest::default(),
        name,
        (Name::FONT_FILE3, ObjRef::new(41, 0)),
        true,
    );
    let int = |key: &[u8]| written.get_int(editor.intern(key));
    // 262178 is bits 2, 6 and 19: serif, nonsymbolic and force-bold.
    assert_eq!(
        int(b"Flags"),
        Some((262_178 & !NONSYMBOLIC) | SYMBOLIC),
        "the document's flags, the symbolic pair as the font is"
    );
    assert_eq!(int(b"Ascent"), Some(905), "the document's own value stands");
    assert_eq!(int(b"StemV"), Some(88));
    assert_eq!(int(b"MissingWidth"), Some(250), "every other entry is kept");
    assert_eq!(
        int(b"CapHeight"),
        Some(716),
        "and what it lacked is completed"
    );
    assert_eq!(written.get_name(editor.intern(b"FontName")), Some(name));
    for stale in [&b"FontFile"[..], b"CharSet", b"CIDSet"] {
        assert!(
            !written.contains_key(editor.intern(stale)),
            "{} describes another program",
            String::from_utf8_lossy(stale)
        );
    }
    assert_eq!(written.get_ref(Name::FONT_FILE3), Some(ObjRef::new(41, 0)));
}

/// A composite font's answer is a CID-keyed CFF or no answer: a `glyf` face
/// states no CID, and is named so; an `OTTO` face with no `CFF ` table, or
/// one that does not parse, and bytes no parser reads are unreadable. The
/// CID-keyed and name-keyed CFFs are `tests/embed_missing.rs`'s.
#[test]
fn a_composite_answer_is_a_cid_keyed_cff_or_named() {
    let truetype = directory(0x0001_0000, &[b"glyf", b"head", b"loca"]);
    assert_eq!(
        keyed_program(&truetype),
        Err(LeftReason::NotOfItsCollection)
    );
    for unreadable in [
        directory(0x4F54_544F, &[b"head", b"hhea"]),
        directory(0x4F54_544F, &[b"CFF ", b"head"]),
        b"%!PS-AdobeFont-1.0: Helvetica".to_vec(),
        Vec::new(),
    ] {
        assert_eq!(
            keyed_program(&unreadable),
            Err(LeftReason::Unreadable),
            "{} bytes",
            unreadable.len()
        );
    }
}

/// The CMap name a Type 0 font is named by (Table 119): a predefined CMap's
/// name, an embedded CMap's `/CMapName`, and nothing where there is none or
/// where a font name could not carry it.
#[test]
fn a_type0_font_is_named_by_a_cmap_name_a_font_name_can_carry() {
    let bytes: &[u8] = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 0 /Kids [] >>\nendobj\n\
3 0 obj\n<< /Type /Font /Subtype /Type0 /Encoding /Identity-H >>\nendobj\n\
4 0 obj\n<< /Type /Font /Subtype /Type0 /Encoding 5 0 R >>\nendobj\n\
5 0 obj\n<< /Type /CMap /CMapName /Tinker-Custom-H /Length 4 >>\nstream\nabcd\nendstream\nendobj\n\
6 0 obj\n<< /Type /Font /Subtype /Type0 /Encoding 7 0 R >>\nendobj\n\
7 0 obj\n<< /Type /CMap /Length 4 >>\nstream\nabcd\nendstream\nendobj\n\
8 0 obj\n<< /Type /Font /Subtype /Type0 /Encoding /Not+A-Name >>\nendobj\n\
9 0 obj\n<< /Type /Font /Subtype /Type0 >>\nendobj\n\
trailer\n<< /Size 10 /Root 1 0 R >>\n%%EOF\n";
    let doc = CosDocument::open(bytes).expect("it opens");
    for (num, expected, what) in [
        (3, Some(&b"Identity-H"[..]), "a predefined CMap"),
        (
            4,
            Some(&b"Tinker-Custom-H"[..]),
            "an embedded CMap's /CMapName",
        ),
        (6, None, "an embedded CMap with no /CMapName"),
        (8, None, "a name with 9.9.2's plus sign in it"),
        (9, None, "no /Encoding"),
    ] {
        assert_eq!(
            cmap_name(&doc, &font_at(&doc, num)).as_deref(),
            expected,
            "{what}"
        );
    }
}

/// `/Differences` read as `tinker_pdf_cos::font` reads them, which is what
/// the font was drawn by: names before any number from code 0, a real
/// number restarting the count at its integer part, a negative one leaving
/// it where it was.
#[test]
fn differences_are_read_as_the_font_was() {
    let bytes: &[u8] = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 0 /Kids [] >>\nendobj\n\
3 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Tinker /Encoding 4 0 R >>\nendobj\n\
4 0 obj\n<< /Type /Encoding /Differences [/zero 65 /A /B 10.0 /ten -1 /eleven 200 /x] >>\nendobj\n\
trailer\n<< /Size 5 /Root 1 0 R >>\n%%EOF\n";
    let doc = CosDocument::open(bytes).expect("it opens");
    let read = differences_of(&doc, &font_at(&doc, 4));
    let named: Vec<(u32, String)> = read
        .iter()
        .map(|(code, name)| {
            let bytes = doc.name_bytes(*name).expect("a name");
            (*code, String::from_utf8_lossy(&bytes).into_owned())
        })
        .collect();
    let expected = [
        (0, "zero"),
        (10, "ten"),
        (11, "eleven"),
        (65, "A"),
        (66, "B"),
        (200, "x"),
    ];
    assert_eq!(
        named,
        expected.map(|(code, name)| (code, name.to_string())),
        "the reading"
    );
    // And the font's own reading names each code alike.
    let font = cos_font::read_in(&doc, &font_at(&doc, 3));
    for (code, name) in expected {
        assert_eq!(font.glyph_name(code), Some(name), "code {code}");
    }
}

/// The `/ToUnicode` the pass writes (9.10.3): a one-byte codespace, each
/// code with a text in a `bfchar` entry of its UTF-16BE units, a hundred
/// entries a section; nothing at all where no code has a text, a code past
/// a byte having no place in that codespace.
#[test]
fn the_to_unicode_states_each_text_in_utf16() {
    assert_eq!(to_unicode(&BTreeMap::new()), None);
    assert_eq!(
        to_unicode(&BTreeMap::from([(0x0100, "A".to_string())])),
        None,
        "a code past a byte"
    );

    let texts = BTreeMap::from([
        (0x41, "A".to_string()),
        (0x42, "fi".to_string()),
        (0x43, "\u{1D400}".to_string()),
        (0x44, String::new()),
    ]);
    let written = String::from_utf8(to_unicode(&texts).expect("a CMap")).expect("ASCII");
    assert_eq!(
        written,
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
         1 begincodespacerange\n<00> <FF>\nendcodespacerange\n\
         3 beginbfchar\n<41> <0041>\n<42> <00660069>\n<43> <D835DC00>\nendbfchar\n\
         endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n"
    );

    let many: BTreeMap<u32, String> = (0u32..150).map(|code| (code, "a".to_string())).collect();
    let written = String::from_utf8(to_unicode(&many).expect("a CMap")).expect("ASCII");
    assert_eq!(written.matches("100 beginbfchar\n").count(), 1);
    assert_eq!(written.matches("50 beginbfchar\n").count(), 1);
    assert_eq!(written.matches("endbfchar\n").count(), 2);
}
