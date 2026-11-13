//! The text options, exercised the way a caller reaches them (ruling 11):
//! through `Page::text()` on a document the writer produced, and with every
//! type named from `tinker_pdf` rather than from the crate underneath.
//!
//! The rules themselves are unit-tested beside the code that applies them —
//! `crates/tinker-pdf-content/src/plain.rs` for hyphen rejoining, `search.rs`
//! for the search options. What this file adds is that the lines extraction
//! really makes, from a real content stream, are the lines those rules see,
//! and that the default is the unoptioned answer to the byte.

mod written_support;

use tinker_pdf::{
    Document, PlainTextOptions, SearchOptions, TextFormat, TextOptions, TextWarning, Unmapped,
};
use tinker_pdf_cos::DocumentBuilder;

/// A one-page document whose content stream is `content`, with Helvetica
/// as `/F0`.
fn document(content: &str) -> Document {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    builder.add_page(300.0, 200.0, |page| {
        page.raw(content.as_bytes());
    });
    Document::open(built("document", builder)).expect("it opens")
}

/// Four lines: a word broken at a hard hyphen before a lower-case start,
/// which joins; a compound broken before a capital, which does not; and a
/// line with a hyphen in the middle, which nothing touches.
const HYPHENATED: &str = concat!(
    "BT /F0 10 Tf 10 150 Td (The hyphen-) Tj 0 -12 Td (ation of a Franco-) Tj ",
    "0 -12 Td (Prussian well-known war) Tj 0 -12 Td (ends here) Tj ET",
);

#[test]
fn a_hyphenated_line_end_rejoins_only_when_asked() {
    let doc = document(HYPHENATED);
    let text = doc.page(0).expect("a page").text();
    assert_eq!(text.lines().len(), 4, "{:?}", text.plain_text());

    let default = text.plain_text_with(&PlainTextOptions::default());
    assert_eq!(
        default.text,
        text.plain_text(),
        "the default is plain_text to the byte"
    );
    assert_eq!(default.hyphens.joins(), 0);

    let joined = text.plain_text_with(&PlainTextOptions {
        rejoin_hyphens: true,
    });
    assert_eq!(
        joined.text,
        "The hyphenation of a Franco-\nPrussian well-known war\nends here\n"
    );
    assert_eq!(joined.hyphens.hard_joins, 1, "one inferred join, counted");
    assert_eq!(joined.hyphens.soft_joins, 0);
}

/// One line with the three spellings a search option tells apart: accented
/// (`\351` is `é` in `WinAnsiEncoding`), lower case and capitalised.
const RESUMES: &str = "BT /F0 10 Tf 10 150 Td (R\\351sum\\351, the resume of a Resume) Tj ET";

#[test]
fn each_search_option_finds_what_it_says_and_the_default_is_search() {
    let doc = document(RESUMES);
    let text = doc.page(0).expect("a page").text();
    assert_eq!(
        text.plain_text(),
        "R\u{e9}sum\u{e9}, the resume of a Resume\n"
    );

    let with = |needle: &str, options: SearchOptions| text.search_with(needle, &options).len();
    for needle in ["resume", "RESUME", "sum", "r\u{e9}sum\u{e9}", "e, t", ""] {
        assert_eq!(
            text.search_with(needle, &SearchOptions::default()),
            text.search(needle),
            "{needle:?}"
        );
    }

    assert_eq!(with("resume", SearchOptions::default()), 2);
    let accents = SearchOptions {
        diacritic_insensitive: true,
        ..SearchOptions::default()
    };
    assert_eq!(with("resume", accents), 3);
    assert_eq!(with("r\u{e9}sum\u{e9}", accents), 3);
    let exact_case = SearchOptions {
        case_sensitive: true,
        ..accents
    };
    assert_eq!(with("resume", exact_case), 1);
    assert_eq!(with("sum", SearchOptions::default()), 3);
    let whole = SearchOptions {
        whole_word: true,
        ..SearchOptions::default()
    };
    assert_eq!(
        with("sum", whole),
        0,
        "sum is inside three words and is none of them"
    );
    assert_eq!(with("resume", whole), 2);
}

/// One line in Helvetica under `WinAnsiEncoding` drawing two codes the
/// encoding leaves undefined (Annex D.2), `\201` twice and `\215` once,
/// between letters it names.
const UNMAPPED: &str = "BT /F0 10 Tf 10 150 Td (A\\201\\201B\\215C) Tj ET";

/// **A code nothing maps is reported** (FT-17.2): a page whose font names
/// no character for two of the codes it draws says so, once for each code,
/// in the order the page draws them, and reads the letters it can.
#[test]
fn an_unmapped_code_is_reported() {
    let doc = document(UNMAPPED);
    let text = doc.page(0).expect("a page").text();
    assert_eq!(text.plain_text(), "ABC\n");
    assert_eq!(
        text.warnings,
        vec![
            TextWarning::UnmappedCode { code: 0x81 },
            TextWarning::UnmappedCode { code: 0x8D },
        ]
    );

    // The letters are one line of three characters: the undefined codes'
    // glyphs add nothing to it, as before the report.
    let lines = text.lines();
    assert_eq!(lines.len(), 1, "{:?}", text.plain_text());
    let spelled: String = lines[0].chars.iter().map(|c| c.text.as_str()).collect();
    assert_eq!(spelled, "ABC");

    // The same report in content order, and in what the structured writers
    // write for the page.
    let stream = doc.page(0).expect("a page").text_with(&TextOptions {
        content_order: true,
        ..TextOptions::default()
    });
    assert_eq!(stream.warnings, text.warnings);
    let page = doc.page(0).expect("a page");
    let json = text.serialize(TextFormat::Json, &page.text_frame());
    assert!(
        json.contains(
            "{\"kind\":\"unmapped-code\",\"code\":129},{\"kind\":\"unmapped-code\",\"code\":141}"
        ),
        "{json}"
    );

    // A page whose every code maps reports nothing.
    let sound = document("BT /F0 10 Tf 10 150 Td (ABC) Tj ET");
    let sound = sound.page(0).expect("a page").text();
    assert_eq!(sound.plain_text(), "ABC\n");
    assert!(sound.warnings.is_empty(), "{:?}", sound.warnings);
}

/// The Liberation face the composite pages embed.
fn liberation_sans() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tinker-pdf-font/data/liberation/LiberationSans-Regular.ttf"
    ))
    .expect("the committed Liberation face")
}

/// A one-page document of 300 by 200 points whose content stream is
/// `content` and whose `/F0` is `face` embedded as a composite font
/// (9.7): `Identity-H`, a CIDFontType2 descendant whose `/CIDToGIDMap` is the
/// stream `cid_to_gid` gives, `/DW 500` and `/W` as written, and no
/// `/ToUnicode`. Its objects are written with a cross-reference table, so
/// nothing in the face's bytes is read as an object.
fn composite_page(face: &[u8], content: &str, cid_to_gid: &[u8], widths: &str) -> Vec<u8> {
    let mut out: Vec<u8> = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    let mut object = |out: &mut Vec<u8>, body: &[u8], stream: Option<&[u8]>| {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", offsets.len()).as_bytes());
        out.extend_from_slice(body);
        if let Some(data) = stream {
            out.extend_from_slice(b"\nstream\n");
            out.extend_from_slice(data);
            out.extend_from_slice(b"\nendstream");
        }
        out.extend_from_slice(b"\nendobj\n");
    };
    object(&mut out, b"<< /Type /Catalog /Pages 2 0 R >>", None);
    object(&mut out, b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>", None);
    object(
        &mut out,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] \
          /Resources << /Font << /F0 5 0 R >> >> /Contents 4 0 R >>",
        None,
    );
    object(
        &mut out,
        format!("<< /Length {} >>", content.len()).as_bytes(),
        Some(content.as_bytes()),
    );
    object(
        &mut out,
        b"<< /Type /Font /Subtype /Type0 /BaseFont /LiberationSans \
          /Encoding /Identity-H /DescendantFonts [6 0 R] >>",
        None,
    );
    object(
        &mut out,
        format!(
            "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /LiberationSans \
             /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
             /FontDescriptor 7 0 R /DW 500 /W {widths} /CIDToGIDMap 9 0 R >>"
        )
        .as_bytes(),
        None,
    );
    object(
        &mut out,
        b"<< /Type /FontDescriptor /FontName /LiberationSans /Flags 32 \
          /FontBBox [-200 -300 1200 1000] /ItalicAngle 0 /Ascent 900 /Descent -200 \
          /CapHeight 700 /StemV 80 /FontFile2 8 0 R >>",
        None,
    );
    object(
        &mut out,
        format!("<< /Length {0} /Length1 {0} >>", face.len()).as_bytes(),
        Some(face),
    );
    object(
        &mut out,
        format!("<< /Length {} >>", cid_to_gid.len()).as_bytes(),
        Some(cid_to_gid),
    );
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes(),
    );
    for at in &offsets {
        out.extend_from_slice(format!("{at:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            offsets.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// A `/CIDToGIDMap` stream (9.7.4.2, Table 117) sending each of `pairs`'
/// CIDs to its glyph and every other CID up to the largest to glyph 0.
fn cid_to_gid_map(pairs: &[(u16, u16)]) -> Vec<u8> {
    let last = pairs.iter().map(|(cid, _)| *cid).max().unwrap_or(0);
    let mut map = vec![0u8; (usize::from(last) + 1) * 2];
    for (cid, glyph) in pairs {
        let at = usize::from(*cid) * 2;
        map[at..at + 2].copy_from_slice(&glyph.to_be_bytes());
    }
    map
}

/// Whether two points are one, to a millionth of a point.
fn near(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6
}

/// **An unmapped code falls back to its code or its glyph when asked**
/// (FT-17.4, `docs/design/extraction-options.md`'s option 5): a page drawing
/// `A`, two codes nothing maps and `B` in a composite font with no
/// `/ToUnicode` — codes 257 and 258, past the single-byte range a base
/// encoding names, which its `/CIDToGIDMap` sends to the glyphs of `Q` and
/// `Z` as the face's own `cmap` gives them — reads `AB` by default; with
/// `Unmapped::Code` the two codes stand as U+F0101 and U+F0102, and with
/// `Unmapped::Glyph` as U+100000 plus each glyph's index; each kept
/// character has the em box its glyph is drawn in; and the two codes are
/// reported in every mode.
#[test]
fn an_unmapped_code_falls_back_to_its_code_or_its_glyph_when_asked() {
    let face = liberation_sans();
    let sfnt = tinker_pdf_font::Sfnt::parse(&face).expect("the face parses");
    let q = sfnt.glyph_for_char('Q').expect("the face draws Q");
    let z = sfnt.glyph_for_char('Z').expect("the face draws Z");
    assert!(q != 0 && z != 0 && q != z);
    let bytes = composite_page(
        &face,
        "BT /F0 20 Tf 50 100 Td <0041010101020042> Tj ET",
        &cid_to_gid_map(&[(257, q), (258, z)]),
        "[257 [700 800]]",
    );
    let doc = Document::open(bytes).expect("it opens");
    let page = doc.page(0).expect("a page");
    let read = |unmapped: Unmapped| {
        page.text_with(&TextOptions {
            unmapped,
            ..TextOptions::default()
        })
    };
    let reported = vec![
        TextWarning::UnmappedCode { code: 257 },
        TextWarning::UnmappedCode { code: 258 },
    ];

    let dropped = read(Unmapped::Drop);
    assert_eq!(dropped.plain_text(), "AB\n");
    assert_eq!(dropped.warnings, reported);
    assert_eq!(
        format!("{dropped:?}"),
        format!("{:?}", page.text()),
        "the default is Page::text"
    );

    let in_area = |area: u32, value: u16| {
        char::from_u32(area + u32::from(value)).expect("a private-use character")
    };
    for (mode, first, second) in [
        (
            Unmapped::Code,
            in_area(0xF_0000, 257),
            in_area(0xF_0000, 258),
        ),
        (
            Unmapped::Glyph,
            in_area(0x10_0000, q),
            in_area(0x10_0000, z),
        ),
    ] {
        let kept = read(mode);
        assert_eq!(
            kept.plain_text(),
            format!("A{first}{second}B\n"),
            "{mode:?}"
        );
        assert_eq!(kept.warnings, reported, "{mode:?}");
        let lines = kept.lines();
        assert_eq!(lines.len(), 1, "{mode:?}");
        let chars = &lines[0].chars;
        assert_eq!(chars.len(), 4, "{mode:?}");
        // 9.4.4: `A` from 50 at /DW's 500 thousandths of 20 points, then
        // 700 and 800 thousandths, each glyph's box 0.8 of an em above the
        // baseline at 100 and 0.2 below.
        for (c, (x0, x1)) in chars[1..3].iter().zip([(60.0, 74.0), (74.0, 90.0)]) {
            assert!(near(c.quad.ll, (x0, 96.0)), "{mode:?}: {:?}", c.quad);
            assert!(near(c.quad.lr, (x1, 96.0)), "{mode:?}: {:?}", c.quad);
            assert!(near(c.quad.ul, (x0, 116.0)), "{mode:?}: {:?}", c.quad);
            assert!(near(c.quad.ur, (x1, 116.0)), "{mode:?}: {:?}", c.quad);
            assert!(near(c.origin, (x0, 100.0)), "{mode:?}: {:?}", c.origin);
            assert_eq!(c.font.as_deref(), Some("LiberationSans"));
        }
        assert_eq!(
            kept.search(&first.to_string()).len(),
            1,
            "{mode:?}: a fallback is found where it stands"
        );
    }
}

/// What `builder` writes, held to the validate-on-save helper as a document
/// this engine wrote from nothing it did not write.
#[track_caller]
fn built(label: &str, builder: tinker_pdf::DocumentBuilder) -> Vec<u8> {
    let bytes = builder.finish();
    written_support::authored(label, &bytes);
    bytes
}
