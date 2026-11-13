//! The text options, exercised the way a caller reaches them (ruling 11):
//! through `Page::text()` on a document the writer produced, and with every
//! type named from `tinker_pdf` rather than from the crate underneath.
//!
//! The rules themselves are unit-tested beside the code that applies them —
//! `crates/tinker-pdf-content/src/plain.rs` for hyphen rejoining, `search.rs`
//! for the search options. What this file adds is that the lines extraction
//! really makes, from a real content stream, are the lines those rules see,
//! and that the default is the unoptioned answer to the byte.

mod epub_support;
mod written_support;

use epub_support::typeface::Face;
use tinker_pdf::{
    CharBoxes, Document, PlainTextOptions, SearchOptions, TextFormat, TextOptions, TextWarning,
    Unmapped,
};
use tinker_pdf_cos::build::{Glyph, PlacedGlyph};
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

/// The face's own numbers for one glyph, in its units: its `hmtx` advance
/// and left side bearing and its `glyf` header's box.
struct FaceGlyph {
    advance: f64,
    lsb: f64,
    x_min: f64,
    y_min: f64,
    x_max: f64,
    y_max: f64,
}

/// `face`'s `hhea` ascender and descender and its `head` units an em, read
/// from the tables' own bytes.
fn face_vertical(face: &[u8]) -> (f64, f64, f64) {
    let sfnt = tinker_pdf_font::Sfnt::parse(face).expect("the face parses");
    let hhea = sfnt.table(u32::from_be_bytes(*b"hhea")).expect("an hhea");
    let at = |offset: usize| f64::from(i16::from_be_bytes([hhea[offset], hhea[offset + 1]]));
    (at(4), at(6), f64::from(sfnt.units_per_em))
}

/// The face's numbers for the glyph its `cmap` gives `c`.
fn face_glyph(face: &[u8], c: char) -> FaceGlyph {
    let sfnt = tinker_pdf_font::Sfnt::parse(face).expect("the face parses");
    let glyph = sfnt.glyph_for_char(c).expect("the face draws it");
    let bounds = tinker_pdf_font::glyf::bounds(&sfnt, glyph).expect("a glyf header");
    FaceGlyph {
        advance: f64::from(sfnt.advance(glyph).expect("an advance")),
        lsb: f64::from(sfnt.left_side_bearing(glyph).expect("a side bearing")),
        x_min: f64::from(bounds.x_min),
        y_min: f64::from(bounds.y_min),
        x_max: f64::from(bounds.x_max),
        y_max: f64::from(bounds.y_max),
    }
}

/// **Each box option is the face's own metrics** (FT-17.5,
/// `docs/design/extraction-options.md`'s options 6 and 7): on a page that
/// embeds Liberation Sans as a simple TrueType font and draws `Hog` upright
/// and `Hy` under a quarter turn, at 20 points, each character's quad in
/// each mode is the one the test computes from the face's tables and the
/// character's origin — the em box from the advance `/Widths` writes; the
/// metrics from `hhea`'s ascender and descender and the `hmtx` left side
/// bearing, the right one taken off the advance; the outline from the
/// `glyf` header — and no mode changes the text, a line, a block or an
/// origin.
#[test]
fn each_box_option_is_the_faces_own_metrics() {
    const SIZE: f64 = 20.0;
    let face = liberation_sans();
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_embedded_font(b"F0", b"LiberationSans", &face));
    builder.add_page(300.0, 200.0, |page| {
        page.raw(
            b"BT /F0 20 Tf 40 150 Td (Hog) Tj ET \
              q 0 1 -1 0 250 40 cm BT /F0 20 Tf 0 0 Td (Hy) Tj ET Q",
        );
    });
    let doc = Document::open(built("boxes", builder)).expect("it opens");
    let page = doc.page(0).expect("a page");
    let read = |boxes: CharBoxes| {
        page.text_with(&TextOptions {
            boxes,
            ..TextOptions::default()
        })
    };
    let (ascender, descender, units) = face_vertical(&face);

    let em = read(CharBoxes::EmBox);
    assert_eq!(
        format!("{em:?}"),
        format!("{:?}", page.text()),
        "the em box is Page::text"
    );
    let mut checked = 0usize;
    for boxes in [CharBoxes::EmBox, CharBoxes::FontMetrics, CharBoxes::Outline] {
        let text = read(boxes);
        assert_eq!(text.plain_text(), em.plain_text(), "{boxes:?}");
        assert_eq!(text.blocks().len(), em.blocks().len(), "{boxes:?}");
        let (lines, em_lines) = (text.lines(), em.lines());
        assert_eq!(lines.len(), 2, "{boxes:?}: {:?}", text.plain_text());
        for (line, em_line) in lines.iter().zip(&em_lines) {
            assert_eq!(line.text, em_line.text);
            // The baseline's direction and its normal on the page: upright,
            // or the quarter turn `0 1 -1 0` gives the text's axes.
            let turned = line.text == "Hy";
            let (u, v) = if turned {
                ((0.0, 1.0), (-1.0, 0.0))
            } else {
                ((1.0, 0.0), (0.0, 1.0))
            };
            for (c, em_c) in line.chars.iter().zip(&em_line.chars) {
                assert_eq!(c.origin, em_c.origin, "{boxes:?}: the origin stays");
                let letter = c.text.chars().next().expect("a letter");
                let g = face_glyph(&face, letter);
                // `/Widths` holds the advance in whole thousandths.
                let advance = (g.advance * 1000.0 / units).round() / 1000.0;
                let (x0, y0, x1, y1) = match boxes {
                    CharBoxes::EmBox => (0.0, -0.2, advance, 0.8),
                    CharBoxes::FontMetrics => (
                        g.lsb / units,
                        descender / units,
                        (g.lsb + g.x_max - g.x_min) / units,
                        ascender / units,
                    ),
                    CharBoxes::Outline => (
                        g.x_min / units,
                        g.y_min / units,
                        g.x_max / units,
                        g.y_max / units,
                    ),
                };
                let at = |gx: f64, gy: f64| {
                    (
                        c.origin.0 + SIZE * (gx * u.0 + gy * v.0),
                        c.origin.1 + SIZE * (gx * u.1 + gy * v.1),
                    )
                };
                for (corner, expected) in [
                    (c.quad.ll, at(x0, y0)),
                    (c.quad.lr, at(x1, y0)),
                    (c.quad.ul, at(x0, y1)),
                    (c.quad.ur, at(x1, y1)),
                ] {
                    assert!(
                        near(corner, expected),
                        "{boxes:?} {letter}: {corner:?} against {expected:?} in {:?}",
                        c.quad
                    );
                }
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 15, "five letters in three modes");
}

/// Every glyph the stated pages draw: each stated text that is no single
/// letter of the face is drawn with a glyph of its own, so that no two
/// texts claim one glyph's `/ToUnicode`.
const STATED_COVERS: &str = "abcdxypqr \u{5E9}\u{5DC}\u{5D5}\u{5DD}";

/// The size the stated pages draw at: the face's 500-unit advance of a
/// 1000-unit em is ten points.
const STATED_SIZE: f64 = 20.0;

/// One glyph of a stated page: the face's glyph for `char`, standing for
/// `&str`, its origin `f64` along the run from the run's origin.
type Stated<'a> = (char, &'a str, f64);

/// A one-page document drawing each of `runs` — a baseline and its glyphs —
/// from 40 points in, at [`STATED_SIZE`], in a composite font of a face that
/// covers [`STATED_COVERS`], each glyph standing for the text it is given.
fn stated_page(runs: &[(f64, Vec<Stated<'_>>)]) -> Document {
    let face = Face::new("Fixture Ligatures", STATED_COVERS);
    let program = face.build();
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_cid_font(b"F0", b"FixtureLigatures", &program));
    let mut content = Vec::new();
    for (baseline, strokes) in runs {
        let glyphs: Vec<PlacedGlyph<'_>> = strokes
            .iter()
            .map(|&(ch, text, x)| PlacedGlyph {
                glyph: Glyph {
                    id: face
                        .glyph_of(ch)
                        .expect("the face covers what the page draws"),
                    text,
                },
                x,
                rise: 0.0,
            })
            .collect();
        assert!(builder.glyph_run(
            &mut content,
            b"F0",
            STATED_SIZE,
            [1.0, 0.0, 0.0, 1.0, 40.0, *baseline],
            &glyphs
        ));
    }
    builder.add_page(300.0, 200.0, |page| page.raw(&content));
    Document::open(built("stated page", builder)).expect("it opens")
}

/// The text and the left and right of the box of each character of
/// `line`.
fn boxes_of(line: &tinker_pdf::TextLine) -> Vec<(String, f64, f64)> {
    line.chars
        .iter()
        .map(|c| {
            let (x0, _, x1, _) = c.quad.bounds();
            (c.text.clone(), x0, x1)
        })
        .collect()
}

/// **Ligatures split into letters when not kept** (FT-17.6,
/// `docs/design/extraction-options.md`'s option 1): a glyph whose
/// `/ToUnicode` spells `ffi` and one that names U+FB01 come back as three and
/// two characters whose boxes tile the glyph's, in the order the letters are
/// read, on a left-to-right line and inside a right-to-left one; the line's
/// text is the kept one with U+FB01 spelled out; and kept by default.
#[test]
fn ligatures_split_into_letters_when_not_kept() {
    let doc = stated_page(&[
        (
            150.0,
            vec![
                ('a', "a", 0.0),
                ('x', "ffi", 10.0),
                ('y', "\u{FB01}", 20.0),
                ('b', "b", 30.0),
            ],
        ),
        (
            100.0,
            vec![
                ('\u{5DD}', "\u{5DD}", 0.0),
                ('\u{5D5}', "\u{5D5}", 10.0),
                ('\u{5DC}', "\u{5DC}", 20.0),
                ('\u{5E9}', "\u{5E9}", 30.0),
                (' ', " ", 40.0),
                ('x', "ffi", 50.0),
                ('y', "\u{FB01}", 60.0),
                (' ', " ", 70.0),
                ('\u{5DD}', "\u{5DD}", 80.0),
                ('\u{5D5}', "\u{5D5}", 90.0),
                ('\u{5DC}', "\u{5DC}", 100.0),
                ('\u{5E9}', "\u{5E9}", 110.0),
            ],
        ),
    ]);
    let page = doc.page(0).expect("a page");
    let kept = page.text();
    let split = page.text_with(&TextOptions {
        split_ligatures: true,
        ..TextOptions::default()
    });
    let (kept_lines, split_lines) = (kept.lines(), split.lines());
    assert_eq!(kept_lines.len(), 2, "{:?}", kept.plain_text());
    assert_eq!(split_lines.len(), 2);
    assert_eq!(kept_lines[0].text, "affi\u{FB01}b", "kept by default");
    assert_eq!(split_lines[0].text, "affifib");
    assert!(kept_lines[1].rtl, "the second line reads right to left");
    assert!(
        kept_lines[1].text.contains("ffi\u{FB01}"),
        "{:?}",
        kept_lines[1].text
    );
    assert_eq!(
        split_lines[1].text,
        kept_lines[1].text.replace('\u{FB01}', "fi"),
        "the right-to-left line reads its ligatures' letters where they stood"
    );

    // Each glyph's box, from 40 in, each glyph ten points wide.
    for (line, ffi, fi) in [(split_lines[0], 50.0, 60.0), (split_lines[1], 90.0, 100.0)] {
        let boxes = boxes_of(line);
        let at = boxes
            .iter()
            .position(|(_, x0, _)| (x0 - ffi).abs() < 1e-6)
            .expect("the ffi glyph's first letter");
        let third = 10.0 / 3.0;
        let expected = [
            ("f", ffi, ffi + third),
            ("f", ffi + third, ffi + 2.0 * third),
            ("i", ffi + 2.0 * third, ffi + 10.0),
            ("f", fi, fi + 5.0),
            ("i", fi + 5.0, fi + 10.0),
        ];
        for (k, (text, x0, x1)) in expected.iter().enumerate() {
            let (t, a, b) = &boxes[at + k];
            assert!(
                t == text && (a - x0).abs() < 1e-6 && (b - x1).abs() < 1e-6,
                "{} letter {k}: {:?} against {text:?} {x0}..{x1}",
                line.text,
                boxes[at + k]
            );
        }
        let (_, y0, _, y1) = line.chars[at].quad.bounds();
        let baseline = if line.rtl { 100.0 } else { 150.0 };
        assert!(
            (y0 - (baseline - 0.2 * STATED_SIZE)).abs() < 1e-6
                && (y1 - (baseline + 0.8 * STATED_SIZE)).abs() < 1e-6,
            "the glyph's height: {y0}..{y1}"
        );
    }

    // A match inside a ligature is boxed inside it.
    let hits = split.search("fi");
    assert_eq!(hits.len(), 4, "{hits:?}");
    let (x0, _, x1, _) = hits[0].bounds();
    assert!(
        (x0 - (50.0 + 10.0 / 3.0)).abs() < 1e-6 && (x1 - 60.0).abs() < 1e-6,
        "inside ffi: {x0}..{x1}"
    );
    assert_eq!(
        kept.search("fi").len(),
        2,
        "kept, only inside the ffi glyphs"
    );
}

/// **White space is normalised when not kept** (FT-17.6, option 2): a tab, a
/// no-break space and an ideographic space each read as one U+0020, each
/// character keeping its box; kept by default.
#[test]
fn whitespace_is_normalised_when_not_kept() {
    let doc = stated_page(&[(
        150.0,
        vec![
            ('a', "a", 0.0),
            ('p', "\t", 10.0),
            ('b', "b", 20.0),
            ('q', "\u{A0}", 30.0),
            ('c', "c", 40.0),
            ('r', "\u{3000}", 50.0),
            ('d', "d", 60.0),
        ],
    )]);
    let page = doc.page(0).expect("a page");
    let kept = page.text();
    assert_eq!(kept.plain_text(), "a\tb\u{A0}c\u{3000}d\n");
    let normalised = page.text_with(&TextOptions {
        normalize_whitespace: true,
        ..TextOptions::default()
    });
    assert_eq!(normalised.plain_text(), "a b c d\n");
    let (kept_line, line) = (kept.lines()[0], normalised.lines()[0]);
    assert_eq!(line.chars.len(), 7);
    for (c, k) in line.chars.iter().zip(&kept_line.chars) {
        assert_eq!(c.quad, k.quad, "{:?} keeps its box", k.text);
        assert_eq!(c.origin, k.origin);
    }
    assert_eq!(
        normalised.search("b c").len(),
        1,
        "the spaces are searched as spaces"
    );
    let both = page.text_with(&TextOptions {
        normalize_whitespace: true,
        split_ligatures: true,
        ..TextOptions::default()
    });
    assert_eq!(both.plain_text(), "a b c d\n");
}

/// What `builder` writes, held to the validate-on-save helper as a document
/// this engine wrote from nothing it did not write.
#[track_caller]
fn built(label: &str, builder: tinker_pdf::DocumentBuilder) -> Vec<u8> {
    let bytes = builder.finish();
    written_support::authored(label, &bytes);
    bytes
}
